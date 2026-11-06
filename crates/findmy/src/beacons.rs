//! Server-side registry of beacons and their latest decrypted location.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::anisette::Provider;
use crate::keys::KeyPair;
use crate::report::{DecryptedReport, decrypt_report};
use crate::{gsa, write_private};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Beacon {
    pub id: String,
    pub name: String,
    /// Hex-encoded 28-byte private keys (one per rotation slot).
    pub private_keys: Vec<String>,
}

pub struct Registry {
    path: PathBuf,
}

impl Registry {
    pub fn new(dir: &Path) -> Self {
        Self { path: dir.join("beacons.json") }
    }

    pub fn list(&self) -> Vec<Beacon> {
        std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Add or replace a beacon. Keys are validated before anything is stored.
    pub fn register(&self, id: &str, name: &str, private_keys: &[Vec<u8>]) -> Result<()> {
        for key in private_keys {
            KeyPair::from_private(key)?;
        }
        let mut all = self.list();
        all.retain(|b| b.id != id);
        all.push(Beacon {
            id: id.into(),
            name: name.into(),
            private_keys: private_keys.iter().map(hex::encode).collect(),
        });
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        write_private(&self.path, &serde_json::to_vec_pretty(&all)?)
    }
}

impl Beacon {
    pub fn key_pairs(&self) -> Vec<KeyPair> {
        self.private_keys
            .iter()
            .filter_map(|k| hex::decode(k).ok())
            .filter_map(|k| KeyPair::from_private(&k).ok())
            .collect()
    }

    /// Fetch recent reports from Apple and return the newest decrypted location.
    pub fn locate(&self, provider: &dyn Provider, auth: &gsa::SearchPartyAuth) -> Result<Option<DecryptedReport>> {
        let pairs = self.key_pairs();
        let ids: Vec<(String, &KeyPair)> = pairs.iter().map(|k| (k.public_key().hashed_id(), k)).collect();
        let hashed: Vec<String> = ids.iter().map(|(id, _)| id.clone()).collect();
        let mut newest: Option<DecryptedReport> = None;
        // Apple limits ids per request; batch conservatively.
        for chunk in hashed.chunks(64) {
            for report in gsa::fetch_reports(provider, auth, chunk, 7)? {
                let Some((_, key)) = ids.iter().find(|(id, _)| *id == report.id) else { continue };
                let Ok(payload) = gsa::payload_bytes(&report) else { continue };
                if let Ok(decrypted) = decrypt_report(key, &payload)
                    && newest.is_none_or(|n| decrypted.timestamp > n.timestamp)
                {
                    newest = Some(decrypted);
                }
            }
        }
        Ok(newest)
    }
}

/// What the Linux beacon service advertises: public keys only, no secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicKeys {
    /// Seconds each key is advertised before moving to the next.
    pub rotate_secs: u64,
    /// Base64 28-byte advertised keys.
    pub keys: Vec<String>,
}

impl PublicKeys {
    /// The key to advertise at `unix_secs`.
    pub fn current(&self, unix_secs: u64) -> Option<&str> {
        if self.keys.is_empty() {
            return None;
        }
        let slot = unix_secs / self.rotate_secs.max(1);
        Some(&self.keys[(slot % self.keys.len() as u64) as usize])
    }
}
