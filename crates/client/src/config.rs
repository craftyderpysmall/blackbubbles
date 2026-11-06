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
    pub notifications: Notifications,
    pub beacon: Option<BeaconConfig>,
}

/// What gets a desktop notification. Shown in the process that owns autofill (the
/// daemon if it runs, otherwise the window).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub enabled: bool,
    // Messages
    pub messages: bool,
    pub imessage: bool,
    pub sms: bool,
    pub show_sender: bool,
    pub show_preview: bool,
    /// Skip texts that only carry a verification code (autofill shows those).
    pub skip_codes: bool,
    /// Don't notify while the BlackBubbles window is focused.
    pub only_when_unfocused: bool,
    pub muted_chats: Vec<String>,
    // Calendar
    pub calendar_reminders: bool,
    pub reminder_minutes: u32,
    // Library changes
    pub photos: bool,
    pub notes: bool,
    pub voice_memos: bool,
    // Find My
    pub low_battery: bool,
    pub low_battery_percent: u8,
    pub beacon_located: bool,
    // Connection
    pub connection: bool,
    // Quiet hours (local time, HH:MM; may wrap past midnight)
    pub quiet_hours: bool,
    pub quiet_start: String,
    pub quiet_end: String,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: true,
            messages: true,
            imessage: true,
            sms: true,
            show_sender: true,
            show_preview: true,
            skip_codes: true,
            only_when_unfocused: true,
            muted_chats: vec![],
            calendar_reminders: true,
            reminder_minutes: 10,
            photos: false,
            notes: false,
            voice_memos: false,
            low_battery: true,
            low_battery_percent: 20,
            beacon_located: false,
            connection: true,
            quiet_hours: false,
            quiet_start: "22:00".into(),
            quiet_end: "07:00".into(),
        }
    }
}

impl Notifications {
    pub fn in_quiet_hours(&self, now: chrono::NaiveTime) -> bool {
        if !self.quiet_hours {
            return false;
        }
        let parse = |s: &str| chrono::NaiveTime::parse_from_str(s.trim(), "%H:%M").ok();
        match (parse(&self.quiet_start), parse(&self.quiet_end)) {
            (Some(start), Some(end)) if start <= end => now >= start && now < end,
            (Some(start), Some(end)) => now >= start || now < end,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    #[test]
    fn quiet_hours_wrap_midnight() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let mut n = Notifications { quiet_hours: true, ..Default::default() };
        assert!(n.in_quiet_hours(t(23, 0)));
        assert!(n.in_quiet_hours(t(6, 59)));
        assert!(!n.in_quiet_hours(t(7, 0)));
        assert!(!n.in_quiet_hours(t(12, 0)));
        n.quiet_start = "13:00".into();
        n.quiet_end = "14:00".into();
        assert!(n.in_quiet_hours(t(13, 30)));
        assert!(!n.in_quiet_hours(t(23, 0)));
        n.quiet_hours = false;
        assert!(!n.in_quiet_hours(t(13, 30)));
    }
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
