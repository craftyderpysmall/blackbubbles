//! Password-protected Notes (the "separate password" kind), decrypted read-only.
//!
//! A locked note stores a salt, an iteration count and a wrapped key on the note, and an
//! IV and GCM tag next to its encrypted body:
//!
//! 1. key-encryption key = PBKDF2-HMAC-SHA256(password, salt, iterations), 16 bytes
//! 2. note key = AES key unwrap (RFC 3394) of the wrapped key with that KEK
//! 3. body = AES-128-GCM(note key, 16-byte IV, tag) → the usual gzipped protobuf
//!
//! A wrong password fails the key-unwrap integrity check in step 2.
//!
//! Account-level scheme (the owner's real database, 2026-11-06): the *account* row holds
//! the salt, 20000 iterations and a 24-byte `ZCRYPTOVERIFIER`; the note has iterations 0
//! and a 16-byte `ZCRYPTOWRAPPEDKEY`. Unwrapping the verifier with the password's KEK
//! proves the password (AES-KW integrity) and yields an account key. How the note key
//! derives from it isn't documented, so [`decrypt`] tries a few constructions and accepts
//! only one whose AES-GCM tag verifies, so it never returns wrong plaintext.

use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use aes_gcm::aead::{Aead, Payload, consts::U16};
use aes_gcm::{AesGcm, Nonce};
use sha2::Sha256;

type Aes128Gcm16 = AesGcm<Aes128, U16>;

const KW_IV: u64 = 0xA6A6_A6A6_A6A6_A6A6;

#[derive(Debug, PartialEq)]
pub enum UnlockError {
    /// Certain: the key-wrap integrity check failed.
    WrongPassword,
    /// The password is right, but no known construction decrypts this note.
    Unsupported,
    Corrupt,
}

/// Password data stored on the Notes account (account-level scheme).
pub struct Account<'a> {
    pub salt: &'a [u8],
    pub iterations: u32,
    pub verifier: &'a [u8],
}

pub struct Locked<'a> {
    pub salt: &'a [u8],
    pub iterations: u32,
    pub wrapped_key: &'a [u8],
    pub iv: &'a [u8],
    pub tag: &'a [u8],
    pub data: &'a [u8],
}

fn kek(password: &str, salt: &[u8], iterations: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut out);
    out
}

/// RFC 3394 AES key unwrap.
pub(crate) fn unwrap_key(kek: &[u8; 16], wrapped: &[u8]) -> Option<Vec<u8>> {
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(kek));
    let n = wrapped.len() / 8 - 1;
    let mut a = u64::from_be_bytes(wrapped[..8].try_into().unwrap());
    let mut r: Vec<[u8; 8]> = wrapped[8..].chunks(8).map(|c| c.try_into().unwrap()).collect();
    for j in (0..6).rev() {
        for i in (1..=n).rev() {
            let t = (n * j + i) as u64;
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&(a ^ t).to_be_bytes());
            block[8..].copy_from_slice(&r[i - 1]);
            let b = GenericArray::from_mut_slice(&mut block);
            cipher.decrypt_block(b);
            a = u64::from_be_bytes(block[..8].try_into().unwrap());
            r[i - 1].copy_from_slice(&block[8..]);
        }
    }
    (a == KW_IV).then(|| r.concat())
}

/// RFC 3394 AES key wrap (only used to build test fixtures).
#[cfg(test)]
pub(crate) fn wrap_key(kek: &[u8; 16], key: &[u8]) -> Vec<u8> {
    use aes::cipher::BlockEncrypt;
    let cipher = Aes128::new(GenericArray::from_slice(kek));
    let n = key.len() / 8;
    let mut a = KW_IV;
    let mut r: Vec<[u8; 8]> = key.chunks(8).map(|c| c.try_into().unwrap()).collect();
    for j in 0..6 {
        for i in 1..=n {
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a.to_be_bytes());
            block[8..].copy_from_slice(&r[i - 1]);
            cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
            a = u64::from_be_bytes(block[..8].try_into().unwrap()) ^ (n * j + i) as u64;
            r[i - 1].copy_from_slice(&block[8..]);
        }
    }
    let mut out = a.to_be_bytes().to_vec();
    out.extend(r.concat());
    out
}

fn gcm_open(key: &[u8], locked: &Locked) -> Option<Vec<u8>> {
    if key.len() != 16 || locked.iv.len() != 16 || locked.tag.len() != 16 {
        return None;
    }
    let cipher = Aes128Gcm16::new(GenericArray::from_slice(key));
    let mut sealed = locked.data.to_vec();
    sealed.extend_from_slice(locked.tag);
    cipher.decrypt(Nonce::<U16>::from_slice(locked.iv), Payload { msg: &sealed, aad: &[] }).ok()
}

fn ecb(key: &[u8], block: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    use aes::cipher::BlockEncrypt;
    if key.len() != 16 || block.len() != 16 {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut b = GenericArray::clone_from_slice(block);
    if encrypt { cipher.encrypt_block(&mut b) } else { cipher.decrypt_block(&mut b) }
    Some(b.to_vec())
}

/// The decrypted body blob (gzipped protobuf, same as an unlocked note's ZDATA).
pub fn decrypt(locked: &Locked, account: Option<&Account>, password: &str) -> Result<Vec<u8>, UnlockError> {
    if locked.iv.len() != 16 || locked.tag.len() != 16 {
        return Err(UnlockError::Corrupt);
    }
    // Per-note scheme: the note has its own iterations and a 24-byte wrapped key.
    if locked.iterations > 0 && locked.wrapped_key.len() >= 24 {
        let kek = kek(password, locked.salt, locked.iterations);
        if let Some(key) = unwrap_key(&kek, locked.wrapped_key) {
            return gcm_open(&key, locked).ok_or(UnlockError::Corrupt);
        }
        if account.is_none() {
            return Err(UnlockError::WrongPassword);
        }
    }
    // Account scheme: the verifier proves the password and yields the account key.
    let account = account.ok_or(UnlockError::Unsupported)?;
    let account_kek = kek(password, account.salt, account.iterations);
    let master = unwrap_key(&account_kek, account.verifier).ok_or(UnlockError::WrongPassword)?;
    let wrapped = locked.wrapped_key;
    let note_kek = kek(password, locked.salt, account.iterations);
    let candidates: Vec<Option<Vec<u8>>> = vec![
        unwrap_key(master.as_slice().try_into().map_err(|_| UnlockError::Corrupt)?, wrapped),
        ecb(&master, wrapped, false),
        ecb(&master, wrapped, true),
        Some(master.clone()),
        (wrapped.len() == 16).then(|| master.iter().zip(wrapped).map(|(a, b)| a ^ b).collect()),
        ecb(&account_kek, wrapped, false),
        unwrap_key(&note_kek, wrapped),
        ecb(&note_kek, wrapped, false),
        Some(note_kek.to_vec()),
    ];
    candidates.into_iter().flatten().find_map(|key| gcm_open(&key, locked)).ok_or(UnlockError::Unsupported)
}

/// Encrypt a body the way Notes does (test fixtures only). Returns (wrapped key, IV, tag, data).
#[cfg(test)]
pub(crate) fn encrypt_for_test(password: &str, salt: &[u8], iterations: u32, body: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let note_key = [7u8; 16];
    let iv = [9u8; 16];
    let wrapped = wrap_key(&kek(password, salt, iterations), &note_key);
    let cipher = Aes128Gcm16::new(GenericArray::from_slice(&note_key));
    let mut sealed = cipher.encrypt(Nonce::<U16>::from_slice(&iv), body).unwrap();
    let tag = sealed.split_off(sealed.len() - 16);
    (wrapped, iv.to_vec(), tag, sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3394_test_vector() {
        // RFC 3394 §4.1: 128-bit key data with a 128-bit KEK.
        let kek: [u8; 16] = hex("000102030405060708090A0B0C0D0E0F").try_into().unwrap();
        let wrapped = hex("1FA68B0A8112B447AEF34BD8FB5A7B829D3E862371D2CFE5");
        assert_eq!(unwrap_key(&kek, &wrapped).unwrap(), hex("00112233445566778899AABBCCDDEEFF"));
        assert_eq!(wrap_key(&kek, &hex("00112233445566778899AABBCCDDEEFF")), wrapped);
    }

    #[test]
    fn round_trip_and_wrong_password() {
        let (wrapped, iv, tag, data) = encrypt_for_test("hunter2", b"saltsaltsaltsalt", 1000, b"secret body");
        let locked = Locked { salt: b"saltsaltsaltsalt", iterations: 1000, wrapped_key: &wrapped, iv: &iv, tag: &tag, data: &data };
        assert_eq!(decrypt(&locked, None, "hunter2").unwrap(), b"secret body");
        assert_eq!(decrypt(&locked, None, "wrong").unwrap_err(), UnlockError::WrongPassword);
    }

    /// The owner's layout: account salt/iterations/24-byte verifier; note iterations 0 and
    /// a 16-byte key (here the note key encrypted with the account key, one candidate).
    #[test]
    fn account_scheme() {
        let account_key = [3u8; 16];
        let note_key = [7u8; 16];
        let verifier = wrap_key(&kek("pw", b"accountsaltsalt!", 500), &account_key);
        let wrapped16 = ecb(&account_key, &note_key, true).unwrap();
        let iv = [9u8; 16];
        let cipher = Aes128Gcm16::new(GenericArray::from_slice(&note_key));
        let mut sealed = cipher.encrypt(Nonce::<U16>::from_slice(&iv), b"locked body".as_slice()).unwrap();
        let tag = sealed.split_off(sealed.len() - 16);
        let note = Locked { salt: b"notesaltnotesalt", iterations: 0, wrapped_key: &wrapped16, iv: &iv, tag: &tag, data: &sealed };
        let account = Account { salt: b"accountsaltsalt!", iterations: 500, verifier: &verifier };
        assert_eq!(decrypt(&note, Some(&account), "pw").unwrap(), b"locked body");
        assert_eq!(decrypt(&note, Some(&account), "nope").unwrap_err(), UnlockError::WrongPassword);
        // Right password but an unknown construction: says so instead of "wrong password".
        let garbage = [0u8; 16];
        let odd = Locked { wrapped_key: &garbage, ..note };
        assert_eq!(decrypt(&odd, Some(&account), "pw").unwrap_err(), UnlockError::Unsupported);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
