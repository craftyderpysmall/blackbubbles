//! Offline-Finding keys and the BLE advertisement that carries them.
//!
//! An Offline-Finding accessory broadcasts its 28-byte P-224 public key (x coordinate).
//! Nearby Apple devices encrypt their location to that key and upload it to Apple. Six
//! bytes of the key go in the BLE address and the other 22 in the manufacturer data.

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use p224::SecretKey;
use p224::elliptic_curve::sec1::ToEncodedPoint;
use sha2::{Digest, Sha256};

pub const APPLE_COMPANY_ID: u16 = 0x004c;
pub const OFFLINE_FINDING_TYPE: u8 = 0x12;

#[derive(Clone)]
pub struct KeyPair {
    secret: SecretKey,
}

impl KeyPair {
    pub fn generate() -> Self {
        Self { secret: SecretKey::random(&mut rand::rngs::OsRng) }
    }

    pub fn from_private(bytes: &[u8]) -> Result<Self> {
        Ok(Self { secret: SecretKey::from_slice(bytes).context("invalid P-224 private key")? })
    }

    pub fn private_bytes(&self) -> Vec<u8> {
        self.secret.to_bytes().to_vec()
    }

    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }

    /// The 28-byte x coordinate that is advertised.
    pub fn public_key(&self) -> AdvertisedKey {
        let point = self.secret.public_key().to_encoded_point(true);
        AdvertisedKey(point.as_bytes()[1..29].try_into().expect("P-224 x is 28 bytes"))
    }
}

/// The public half: what the beacon advertises and what reports are looked up by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvertisedKey(pub [u8; 28]);

impl AdvertisedKey {
    pub fn from_base64(s: &str) -> Result<Self> {
        let bytes = B64.decode(s.trim())?;
        Ok(Self(bytes.try_into().map_err(|_| anyhow::anyhow!("advertised key must be 28 bytes"))?))
    }

    pub fn to_base64(&self) -> String {
        B64.encode(self.0)
    }

    /// The id Apple stores reports under: base64(SHA-256(key)).
    pub fn hashed_id(&self) -> String {
        B64.encode(Sha256::digest(self.0))
    }

    /// Random static BLE address, most significant byte first. The top two bits must be
    /// set for a static random address, so the real ones travel in the payload.
    pub fn ble_address(&self) -> [u8; 6] {
        let k = &self.0;
        [k[0] | 0b1100_0000, k[1], k[2], k[3], k[4], k[5]]
    }

    /// Full 31-byte legacy advertising data (one manufacturer-specific AD structure).
    pub fn advertising_data(&self) -> [u8; 31] {
        let k = &self.0;
        let mut data = [0u8; 31];
        data[0] = 30; // AD length
        data[1] = 0xff; // manufacturer specific
        data[2..4].copy_from_slice(&APPLE_COMPANY_ID.to_le_bytes());
        data[4] = OFFLINE_FINDING_TYPE;
        data[5] = 25; // payload length
        data[6] = 0x00; // status (battery etc.)
        data[7..29].copy_from_slice(&k[6..28]);
        data[29] = k[0] >> 6;
        data[30] = 0x00; // hint
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_layout() {
        let key = AdvertisedKey(std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(0xf0)));
        let data = key.advertising_data();
        assert_eq!(&data[..7], &[30, 0xff, 0x4c, 0x00, 0x12, 25, 0]);
        assert_eq!(&data[7..29], &key.0[6..]);
        let addr = key.ble_address();
        assert_eq!(addr[0] & 0xc0, 0xc0);
        // The two bits overwritten in the address are recoverable from the payload.
        assert_eq!((addr[0] & 0x3f) | (data[29] << 6), key.0[0]);
    }

    #[test]
    fn key_round_trip() {
        let pair = KeyPair::generate();
        let again = KeyPair::from_private(&pair.private_bytes()).unwrap();
        assert_eq!(pair.public_key(), again.public_key());
        let public = pair.public_key();
        assert_eq!(AdvertisedKey::from_base64(&public.to_base64()).unwrap(), public);
        assert_eq!(public.hashed_id().len(), 44);
    }
}
