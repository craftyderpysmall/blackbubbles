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
//! the salt, 20000 iterations and a 24-byte `ZCRYPTOVERIFIER`; the note has iterations 0,
//! its own 16-byte salt, a 16-byte `ZCRYPTOWRAPPEDKEY`, and an IV and tag on *both* its own
//! row and its note-data row. Unwrapping the verifier with the password's KEK proves the
//! password (AES-KW integrity) and yields an account key. How the note key derives from it
//! isn't documented, so [`decrypt`] tries a matrix of constructions (key derivations ×
//! unwrap methods, including an AES-GCM-wrapped key using the note row's IV/tag) and accepts
//! only one whose AES-GCM tag verifies, so it never returns wrong plaintext. The winning
//! construction's label is logged, so it can be pinned down; `examples/note_probe.rs` runs
//! a much larger matrix offline against a `NoteCryptoDump`.

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
    /// A GCM-wrapped note key authenticated (named construction), but the body didn't.
    KeyOnly(String),
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
    /// IV/tag on the note's own row (may protect the wrapped key, not the body).
    pub key_iv: &'a [u8],
    pub key_tag: &'a [u8],
    /// IV/tag on the note-data row, next to the encrypted body.
    pub iv: &'a [u8],
    pub tag: &'a [u8],
    pub data: &'a [u8],
}

pub fn kek(password: &str, salt: &[u8], iterations: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut out);
    out
}

/// RFC 3394 AES key unwrap.
pub fn unwrap_key(kek: &[u8; 16], wrapped: &[u8]) -> Option<Vec<u8>> {
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

pub fn gcm_open(key: &[u8], iv: &[u8], tag: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if key.len() != 16 || iv.is_empty() || tag.len() != 16 {
        return None;
    }
    let mut sealed = data.to_vec();
    sealed.extend_from_slice(tag);
    let cipher = Aes128Gcm16::new(GenericArray::from_slice(key));
    match iv.len() {
        16 => cipher.decrypt(Nonce::<U16>::from_slice(iv), Payload { msg: &sealed, aad: &[] }).ok(),
        12 => {
            let cipher = aes_gcm::Aes128Gcm::new(GenericArray::from_slice(key));
            cipher.decrypt(aes_gcm::Nonce::from_slice(iv), Payload { msg: &sealed, aad: &[] }).ok()
        }
        _ => None,
    }
}

pub fn ecb(key: &[u8], block: &[u8], encrypt: bool) -> Option<Vec<u8>> {
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
    decrypt_traced(locked, account, password).map(|(body, _)| body)
}

/// [`decrypt`], plus a label naming the construction that worked.
pub fn decrypt_traced(locked: &Locked, account: Option<&Account>, password: &str) -> Result<(Vec<u8>, String), UnlockError> {
    if locked.iv.len() != 16 || locked.tag.len() != 16 {
        return Err(UnlockError::Corrupt);
    }
    // Per-note scheme: the note has its own iterations and a 24-byte wrapped key.
    if locked.iterations > 0 && locked.wrapped_key.len() >= 24 {
        let kek = kek(password, locked.salt, locked.iterations);
        if let Some(key) = unwrap_key(&kek, locked.wrapped_key) {
            let body = gcm_open(&key, locked.iv, locked.tag, locked.data).ok_or(UnlockError::Corrupt)?;
            return Ok((body, "per-note".into()));
        }
        if account.is_none() {
            return Err(UnlockError::WrongPassword);
        }
    }
    // Account scheme: the verifier proves the password and yields the account key.
    let account = account.ok_or(UnlockError::Unsupported)?;
    let account_kek = kek(password, account.salt, account.iterations);
    let master = unwrap_key(&account_kek, account.verifier).ok_or(UnlockError::WrongPassword)?;
    let keks = account_keks(password, &account_kek, &master, account, locked);
    try_matrix(&keks, locked)
}

/// Candidate keys for unwrapping (or directly decrypting) a note in the account scheme.
pub fn account_keks(password: &str, account_kek: &[u8; 16], master: &[u8], account: &Account, locked: &Locked) -> Vec<(String, Vec<u8>)> {
    let ns = locked.salt;
    let mut keks: Vec<(String, Vec<u8>)> = vec![
        ("master".into(), master.to_vec()),
        ("account-kek".into(), account_kek.to_vec()),
        ("pbkdf2(pw,note-salt,acct-iter)".into(), kek(password, ns, account.iterations).to_vec()),
        ("hkdf(master,note-salt)".into(), hkdf16(ns, master, b"")),
        ("hkdf(master,info=note-salt)".into(), hkdf16(b"", master, ns)),
        ("hkdf(acct-kek,note-salt)".into(), hkdf16(ns, account_kek, b"")),
        ("hmac(master,note-salt)".into(), hmac16(master, ns)),
        ("hmac(note-salt,master)".into(), hmac16(ns, master)),
        ("sha256(master|note-salt)".into(), sha16(&[master, ns])),
        ("pbkdf2(master,note-salt,acct-iter)".into(), pbkdf16(master, ns, account.iterations)),
        ("pbkdf2(master,note-salt,1)".into(), pbkdf16(master, ns, 1)),
    ];
    if master.len() == 32 {
        keks.push(("master[16..]".into(), master[16..].to_vec()));
        keks.push(("master[..16]".into(), master[..16].to_vec()));
    }
    keks.retain(|(_, k)| k.len() == 16);
    keks
}

/// Try every (key × unwrap method) for the note key, then the body. AES-GCM must
/// authenticate the body; a GCM-unwrapped key that authenticates but whose body doesn't is
/// reported as [`UnlockError::KeyOnly`].
pub fn try_matrix(keks: &[(String, Vec<u8>)], locked: &Locked) -> Result<(Vec<u8>, String), UnlockError> {
    let w = locked.wrapped_key;
    let mut bodies: Vec<(&[u8], &[u8], &str)> = vec![(locked.iv, locked.tag, "data-iv")];
    if (locked.key_iv, locked.key_tag) != (locked.iv, locked.tag) && locked.key_iv.len() == 16 && locked.key_tag.len() == 16 {
        bodies.push((locked.key_iv, locked.key_tag, "note-iv"));
    }
    let open_body = |key: &[u8], how: String| {
        bodies.iter().find_map(|(iv, tag, which)| gcm_open(key, iv, tag, locked.data).map(|b| (b, format!("{how} → body({which})"))))
    };
    let mut key_only = None;
    for (name, k) in keks {
        let mut note_keys: Vec<(String, Vec<u8>)> = vec![(format!("{name} (direct)"), k.clone())];
        if let Some(nk) = <&[u8; 16]>::try_from(k.as_slice()).ok().and_then(|k| unwrap_key(k, w)) {
            note_keys.push((format!("kw({name})"), nk));
        }
        // AES-GCM-wrapped key: the note row's IV/tag authenticate the 16-byte key.
        for (iv, tag, which) in [(locked.key_iv, locked.key_tag, "note-iv"), (locked.iv, locked.tag, "data-iv")] {
            if let Some(nk) = gcm_open(k, iv, tag, w) {
                let how = format!("gcm-wrap({name},{which})");
                if let Some(hit) = open_body(&nk, how.clone()) {
                    return Ok(hit);
                }
                key_only.get_or_insert(how);
            }
        }
        if let Some(nk) = ecb(k, w, false) {
            note_keys.push((format!("ecb-dec({name})"), nk));
        }
        if let Some(nk) = ecb(k, w, true) {
            note_keys.push((format!("ecb-enc({name})"), nk));
        }
        if w.len() == 16 {
            note_keys.push((format!("xor({name})"), k.iter().zip(w).map(|(a, b)| a ^ b).collect()));
        }
        for (how, nk) in note_keys {
            if let Some(hit) = open_body(&nk, how) {
                return Ok(hit);
            }
        }
    }
    Err(key_only.map(UnlockError::KeyOnly).unwrap_or(UnlockError::Unsupported))
}

fn hmac_full(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac takes any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().to_vec()
}

pub fn hmac16(key: &[u8], msg: &[u8]) -> Vec<u8> {
    hmac_full(key, &[msg])[..16].to_vec()
}

/// HKDF-SHA256 (RFC 5869), 16-byte output.
pub fn hkdf16(salt: &[u8], ikm: &[u8], info: &[u8]) -> Vec<u8> {
    let salt = if salt.is_empty() { &[0u8; 32][..] } else { salt };
    let prk = hmac_full(salt, &[ikm]);
    hmac_full(&prk, &[info, &[1]])[..16].to_vec()
}

pub fn sha16(parts: &[&[u8]]) -> Vec<u8> {
    use sha2::Digest;
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize()[..16].to_vec()
}

pub fn pbkdf16(secret: &[u8], salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut out = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<Sha256>(secret, salt, iterations.max(1), &mut out);
    out.to_vec()
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
        let locked = Locked { salt: b"saltsaltsaltsalt", iterations: 1000, wrapped_key: &wrapped, key_iv: &iv, key_tag: &tag, iv: &iv, tag: &tag, data: &data };
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
        let note = Locked { salt: b"notesaltnotesalt", iterations: 0, wrapped_key: &wrapped16, key_iv: &iv, key_tag: &tag, iv: &iv, tag: &tag, data: &sealed };
        let account = Account { salt: b"accountsaltsalt!", iterations: 500, verifier: &verifier };
        assert_eq!(decrypt(&note, Some(&account), "pw").unwrap(), b"locked body");
        assert_eq!(decrypt(&note, Some(&account), "nope").unwrap_err(), UnlockError::WrongPassword);
        // Right password but an unknown construction: says so instead of "wrong password".
        let garbage = [0u8; 16];
        let odd = Locked { wrapped_key: &garbage, ..note };
        assert_eq!(decrypt(&odd, Some(&account), "pw").unwrap_err(), UnlockError::Unsupported);
    }

    fn gcm_seal(key: &[u8], iv: &[u8], body: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let cipher = Aes128Gcm16::new(GenericArray::from_slice(key));
        let mut sealed = cipher.encrypt(Nonce::<U16>::from_slice(iv), body).unwrap();
        let tag = sealed.split_off(sealed.len() - 16);
        (sealed, tag)
    }

    /// The note key sealed with AES-GCM under the account key, using the note row's IV/tag;
    /// the body under the note key with the note-data row's IV/tag.
    #[test]
    fn account_scheme_gcm_wrapped_key() {
        let account_key = [3u8; 16];
        let note_key = [7u8; 16];
        let verifier = wrap_key(&kek("pw", b"accountsaltsalt!", 500), &account_key);
        let (key_iv, data_iv) = ([1u8; 16], [2u8; 16]);
        let (wrapped, key_tag) = gcm_seal(&account_key, &key_iv, &note_key);
        let (data, tag) = gcm_seal(&note_key, &data_iv, b"gcm body");
        let note = Locked { salt: b"notesaltnotesalt", iterations: 0, wrapped_key: &wrapped, key_iv: &key_iv, key_tag: &key_tag, iv: &data_iv, tag: &tag, data: &data };
        let account = Account { salt: b"accountsaltsalt!", iterations: 500, verifier: &verifier };
        let (body, how) = decrypt_traced(&note, Some(&account), "pw").unwrap();
        assert_eq!((body.as_slice(), how.as_str()), (b"gcm body".as_slice(), "gcm-wrap(master,note-iv) → body(data-iv)"));
        // Key authenticates but the body is damaged: says so distinctly.
        let mut bad = data.clone();
        bad[0] ^= 1;
        let broken = Locked { data: &bad, ..note };
        assert!(matches!(decrypt(&broken, Some(&account), "pw").unwrap_err(), UnlockError::KeyOnly(_)));
    }

    #[test]
    fn hkdf_rfc5869_case1() {
        let okm = hkdf16(&hex("000102030405060708090a0b0c"), &hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b"), &hex("f0f1f2f3f4f5f6f7f8f9"));
        assert_eq!(okm, hex("3cb25f25faacd57a90434f64d0362f2a"));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
