//! Decrypting Offline-Finding location reports.
//!
//! Report layout (after dropping the extra byte newer reports carry at index 5):
//! `timestamp:u32be (secs since 2001) | confidence:u8 | ephemeral key:57 | ciphertext:10 | tag:16`.
//! Key agreement is ECDH on P-224 with the beacon's private key, then ANSI X9.63 KDF
//! with SHA-256 giving a 16-byte AES-GCM key and 16-byte IV.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::aes::Aes128;
use aes_gcm::{AesGcm, Nonce};
use anyhow::{Result, bail};
use p224::PublicKey;
use sha2::{Digest, Sha256};

use crate::keys::KeyPair;

type Aes128Gcm16 = AesGcm<Aes128, aes_gcm::aead::consts::U16>;

/// Seconds between the Unix epoch and Apple's 2001-01-01 epoch.
pub const APPLE_EPOCH_OFFSET: i64 = 978_307_200;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecryptedReport {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: u8,
    pub status: u8,
    pub confidence: u8,
    /// Unix seconds.
    pub timestamp: i64,
}

pub(crate) fn kdf(shared: &[u8], ephemeral: &[u8]) -> ([u8; 16], [u8; 16]) {
    let mut hasher = Sha256::new();
    hasher.update(shared);
    hasher.update(1u32.to_be_bytes());
    hasher.update(ephemeral);
    let derived = hasher.finalize();
    (derived[..16].try_into().unwrap(), derived[16..].try_into().unwrap())
}

pub fn decrypt_report(key: &KeyPair, payload: &[u8]) -> Result<DecryptedReport> {
    let mut data = payload.to_vec();
    if data.len() > 88 {
        data.remove(5);
    }
    if data.len() < 88 {
        bail!("report too short ({} bytes)", data.len());
    }
    let timestamp = i64::from(u32::from_be_bytes(data[0..4].try_into()?)) + APPLE_EPOCH_OFFSET;
    let confidence = data[4];
    let ephemeral = &data[5..62];
    let ciphertext_and_tag = &data[62..88];

    let ephemeral_key = PublicKey::from_sec1_bytes(ephemeral)?;
    let shared = p224::ecdh::diffie_hellman(key.secret().to_nonzero_scalar(), ephemeral_key.as_affine());
    let (aes_key, iv) = kdf(shared.raw_secret_bytes(), ephemeral);
    let cipher = Aes128Gcm16::new_from_slice(&aes_key)?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext_and_tag)
        .map_err(|_| anyhow::anyhow!("report does not decrypt with this key"))?;
    if plain.len() < 10 {
        bail!("decrypted report too short");
    }
    Ok(DecryptedReport {
        lat: f64::from(i32::from_be_bytes(plain[0..4].try_into()?)) / 1e7,
        lon: f64::from(i32::from_be_bytes(plain[4..8].try_into()?)) / 1e7,
        accuracy_m: plain[8],
        status: plain[9],
        confidence,
        timestamp,
    })
}

/// Build a report the way an Apple device would. Used in tests and the mock backend.
pub fn encrypt_report(public: &crate::keys::AdvertisedKey, lat: f64, lon: f64, apple_secs: u32) -> Result<Vec<u8>> {
    use p224::elliptic_curve::sec1::ToEncodedPoint;
    // The advertised key is an x coordinate; either y works for ECDH's x-only output.
    let mut compressed = vec![0x02];
    compressed.extend_from_slice(&public.0);
    let beacon = PublicKey::from_sec1_bytes(&compressed)?;
    let ephemeral = p224::SecretKey::random(&mut rand::rngs::OsRng);
    let ephemeral_point = ephemeral.public_key().to_encoded_point(false);
    let shared = p224::ecdh::diffie_hellman(ephemeral.to_nonzero_scalar(), beacon.as_affine());
    let (aes_key, iv) = kdf(shared.raw_secret_bytes(), ephemeral_point.as_bytes());
    let mut plain = Vec::new();
    plain.extend_from_slice(&((lat * 1e7) as i32).to_be_bytes());
    plain.extend_from_slice(&((lon * 1e7) as i32).to_be_bytes());
    plain.extend_from_slice(&[25, 0]);
    let sealed = Aes128Gcm16::new_from_slice(&aes_key)?
        .encrypt(Nonce::from_slice(&iv), plain.as_slice())
        .map_err(|_| anyhow::anyhow!("encrypt failed"))?;
    let mut out = apple_secs.to_be_bytes().to_vec();
    out.push(2);
    out.extend_from_slice(ephemeral_point.as_bytes());
    out.extend_from_slice(&sealed);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_what_an_apple_device_would_send() {
        let key = KeyPair::generate();
        let payload = encrypt_report(&key.public_key(), 52.520008, 13.404954, 800_000_000).unwrap();
        assert_eq!(payload.len(), 88);
        let report = decrypt_report(&key, &payload).unwrap();
        assert!((report.lat - 52.520008).abs() < 1e-6);
        assert!((report.lon - 13.404954).abs() < 1e-6);
        assert_eq!(report.accuracy_m, 25);
        assert_eq!(report.timestamp, 800_000_000 + APPLE_EPOCH_OFFSET);

        // Newer 89-byte format with an extra byte at index 5.
        let mut long = payload.clone();
        long.insert(5, 0xaa);
        assert_eq!(decrypt_report(&key, &long).unwrap(), report);

        assert!(decrypt_report(&KeyPair::generate(), &payload).is_err());
    }
}
