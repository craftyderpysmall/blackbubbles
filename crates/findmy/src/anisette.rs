//! Anisette: the device-attestation headers Apple's auth servers require.
//!
//! On a Mac they come from the system (see `bb-mac`'s AOSKit provider). Anywhere else,
//! point at a self-hosted anisette server (e.g. `dadoum/anisette-v3-server`).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};

pub type Headers = BTreeMap<String, String>;

/// Supplies the two one-time-password headers, `X-Apple-I-MD` and `X-Apple-I-MD-M`.
pub trait Provider: Send + Sync {
    fn otp_headers(&self) -> Result<Headers>;
}

/// A JSON anisette server (`GET` returns `{"X-Apple-I-MD": ..., "X-Apple-I-MD-M": ...}`).
pub struct Remote {
    pub url: String,
    client: reqwest::blocking::Client,
}

impl Remote {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into(), client: reqwest::blocking::Client::new() }
    }
}

impl Provider for Remote {
    fn otp_headers(&self) -> Result<Headers> {
        let all: BTreeMap<String, serde_json::Value> =
            self.client.get(&self.url).send()?.error_for_status()?.json().context("anisette server reply")?;
        let mut out = Headers::new();
        for key in ["X-Apple-I-MD", "X-Apple-I-MD-M"] {
            let value = all.get(key).and_then(|v| v.as_str()).with_context(|| format!("anisette reply lacks {key}"))?;
            out.insert(key.into(), value.into());
        }
        Ok(out)
    }
}

/// Stable per-install identifiers sent alongside the OTP headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIds {
    pub user_id: String,
    pub device_id: String,
}

impl DeviceIds {
    pub fn generate() -> Self {
        Self {
            user_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            device_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
        }
    }
}

pub const CLIENT_INFO: &str =
    "<MacBookPro18,3> <Mac OS X;13.4.1;22F8> <com.apple.AOSKit/282 (com.apple.dt.Xcode/3594.4.19)>";

/// OTP headers plus the metadata headers that go with them.
pub fn full_headers(provider: &dyn Provider, ids: &DeviceIds) -> Result<Headers> {
    let mut headers = provider.otp_headers()?;
    let now = chrono::Utc::now();
    headers.insert("X-Apple-I-Client-Time".into(), now.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    headers.insert("X-Apple-I-TimeZone".into(), "UTC".into());
    headers.insert("loc".into(), "en_US".into());
    headers.insert("X-Apple-Locale".into(), "en_US".into());
    headers.insert("X-Apple-I-MD-RINFO".into(), "17106176".into());
    headers.insert(
        "X-Apple-I-MD-LU".into(),
        base64::engine::general_purpose::STANDARD.encode(ids.user_id.as_bytes()),
    );
    headers.insert("X-Mme-Device-Id".into(), ids.device_id.clone());
    headers.insert("X-Apple-I-SRL-NO".into(), "0".into());
    headers.insert("X-Mme-Client-Info".into(), CLIENT_INFO.into());
    Ok(headers)
}
