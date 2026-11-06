//! `~/.config/blackbubbles/config.toml` (macOS: `~/Library/Application Support/BlackBubbles`).
//!
//! Holds the device token and autofill secrets, so like linuxautofill it must be mode 600.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: Option<ServerConfig>,
    pub autofill: bb_autofill::Config,
    pub beacon: Option<BeaconConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Pinned SHA-256 of the server certificate (hex).
    pub fingerprint: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeaconConfig {
    pub id: String,
    pub name: String,
    /// Hex P-224 private keys. Only the public halves go to the beacon service.
    pub private_keys: Vec<String>,
    pub rotate_secs: u64,
}

pub fn dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("BLACKBUBBLES_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(directories::ProjectDirs::from("", "", "BlackBubbles").context("no config directory")?.config_dir().to_path_buf())
}

pub fn path() -> Result<PathBuf> {
    Ok(dir()?.join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = path()?;
        let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Self::default()) };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)?.permissions().mode();
            if mode & 0o077 != 0 {
                bail!("{} holds secrets but is readable by others; run: chmod 600 {}", path.display(), path.display());
            }
        }
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        bb_net::write_private(&path, toml::to_string_pretty(self)?.as_bytes())
    }
}

/// Public keys file for the root beacon service (no secrets).
pub fn beacon_public_path() -> Result<PathBuf> {
    Ok(dir()?.join("beacon-public.json"))
}
