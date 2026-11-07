//! Apple "GrandSlam" (GSA) sign-in and the Offline-Finding report service.
//!
//! Flow: SRP over plist to `gsa.apple.com/grandslam/GsService2` → decrypt `spd` → trusted
//! device 2FA if asked → `setup.icloud.com/setup/iosbuddy/loginDelegates` for the
//! `searchPartyToken` → `gateway.icloud.com/acsnservice/fetch` for encrypted reports.
//! This follows the publicly documented protocol (OpenHaystack / macless-haystack research);
//! no code from those projects is used.

use std::path::Path;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::anisette::{self, DeviceIds, Provider};
use crate::srp;

const GSA_URL: &str = "https://gsa.apple.com/grandslam/GsService2";

/// What we keep between runs (file mode 600). The password is never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchPartyAuth {
    pub dsid: String,
    pub search_party_token: String,
    pub ids: DeviceIds,
}

impl SearchPartyAuth {
    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path).context("not signed in for beacons; run `bbd findmy-login --beacons`")?)?)
    }
}

fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("akd/1.0 CFNetwork/978.0.7 Darwin/18.7.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

fn gsa_request(client: &reqwest::blocking::Client, provider: &dyn Provider, ids: &DeviceIds, params: Dictionary) -> Result<Dictionary> {
    let mut cpd = Dictionary::new();
    cpd.insert("bootstrap".into(), true.into());
    cpd.insert("icscrec".into(), true.into());
    cpd.insert("pbe".into(), false.into());
    cpd.insert("prkgen".into(), true.into());
    cpd.insert("svct".into(), "iCloud".into());
    for (k, v) in anisette::full_headers(provider, ids)? {
        cpd.insert(k, v.into());
    }
    let mut request = params;
    request.insert("cpd".into(), cpd.into());
    let mut header = Dictionary::new();
    header.insert("Version".into(), "1.0.1".into());
    let mut body = Dictionary::new();
    body.insert("Header".into(), header.into());
    body.insert("Request".into(), request.into());
    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, &Value::Dictionary(body))?;

    let reply = client
        .post(GSA_URL)
        .header("Content-Type", "text/x-xml-plist")
        .header("Accept", "*/*")
        .header("X-MMe-Client-Info", anisette::CLIENT_INFO)
        .body(bytes)
        .send()?
        .bytes()?;
    let value: Value = plist::from_bytes(&reply).context("GSA reply is not a plist")?;
    let response = value
        .into_dictionary()
        .and_then(|mut d| d.remove("Response"))
        .and_then(Value::into_dictionary)
        .context("GSA reply has no Response")?;
    if let Some(status) = response.get("Status").and_then(Value::as_dictionary) {
        let code = status.get("ec").and_then(Value::as_signed_integer).unwrap_or(0);
        if code != 0 {
            let message = status.get("em").and_then(Value::as_string).unwrap_or("unknown error");
            bail!("Apple sign-in failed ({code}): {message}");
        }
    }
    Ok(response)
}

fn data(dict: &Dictionary, key: &str) -> Result<Vec<u8>> {
    dict.get(key).and_then(Value::as_data).map(<[u8]>::to_vec).with_context(|| format!("GSA reply lacks {key}"))
}

fn session_derived(key: &[u8], label: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    mac.update(label.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn decrypt_spd(session_key: &[u8], spd: &[u8]) -> Result<Dictionary> {
    let key = session_derived(session_key, "extra data key:");
    let iv = &session_derived(session_key, "extra data iv:")[..16];
    let plain = cbc::Decryptor::<aes::Aes256>::new_from_slices(&key, iv)?
        .decrypt_padded_vec_mut::<Pkcs7>(spd)
        .map_err(|_| anyhow::anyhow!("could not decrypt GSA session data"))?;
    let mut doc = br#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0">"#.to_vec();
    doc.extend_from_slice(&plain);
    doc.extend_from_slice(b"</plist>");
    plist::from_bytes::<Value>(&doc)?.into_dictionary().context("spd is not a dictionary")
}

fn authenticate(
    client: &reqwest::blocking::Client,
    provider: &dyn Provider,
    ids: &DeviceIds,
    apple_id: &str,
    password: &str,
    code_prompt: &mut dyn FnMut(&str) -> Result<String>,
    attempt: u32,
) -> Result<Dictionary> {
    let srp_client = srp::Client::new(apple_id);
    let mut init = Dictionary::new();
    init.insert("A2k".into(), Value::Data(srp_client.public_a()));
    init.insert("ps".into(), Value::Array(vec!["s2k".into(), "s2k_fo".into()]));
    init.insert("u".into(), apple_id.into());
    init.insert("o".into(), "init".into());
    let challenge = gsa_request(client, provider, ids, init)?;

    let protocol = srp::Protocol::parse(challenge.get("sp").and_then(Value::as_string).unwrap_or(""))?;
    let salt = data(&challenge, "s")?;
    let iterations = challenge.get("i").and_then(Value::as_unsigned_integer).context("GSA reply lacks i")? as u32;
    let password_key = srp::derive_password(password, &salt, iterations, protocol);
    let proof = srp_client.process_challenge(&salt, &data(&challenge, "B")?, &password_key)?;

    let mut complete = Dictionary::new();
    complete.insert("c".into(), challenge.get("c").cloned().context("GSA reply lacks c")?);
    complete.insert("M1".into(), Value::Data(proof.m1.to_vec()));
    complete.insert("u".into(), apple_id.into());
    complete.insert("o".into(), "complete".into());
    let done = gsa_request(client, provider, ids, complete)?;
    if data(&done, "M2")? != proof.m2 {
        bail!("Apple's server proof did not verify");
    }
    let spd = decrypt_spd(&proof.key, &data(&done, "spd")?)?;

    let second_factor = done
        .get("Status")
        .and_then(Value::as_dictionary)
        .and_then(|s| s.get("au"))
        .and_then(Value::as_string)
        .map(str::to_owned);
    match second_factor.as_deref() {
        None => Ok(spd),
        Some(_) if attempt > 0 => bail!("Apple still asks for a second factor after verification"),
        Some("trustedDeviceSecondaryAuth") => {
            trusted_device_2fa(client, provider, ids, &spd, code_prompt)?;
            authenticate(client, provider, ids, apple_id, password, code_prompt, attempt + 1)
        }
        Some(other) => bail!("unsupported second factor {other:?}; enable trusted-device 2FA"),
    }
}

fn trusted_device_2fa(
    client: &reqwest::blocking::Client,
    provider: &dyn Provider,
    ids: &DeviceIds,
    spd: &Dictionary,
    code_prompt: &mut dyn FnMut(&str) -> Result<String>,
) -> Result<()> {
    let adsid = spd.get("adsid").and_then(Value::as_string).context("spd lacks adsid")?;
    let idms = spd.get("GsIdmsToken").and_then(Value::as_string).context("spd lacks GsIdmsToken")?;
    let identity = B64.encode(format!("{adsid}:{idms}"));
    let anisette = anisette::full_headers(provider, ids)?;
    let with_headers = |mut builder: reqwest::blocking::RequestBuilder, plist: bool| {
        let kind = if plist { "text/x-xml-plist" } else { "application/json" };
        builder = builder
            .header("Content-Type", kind)
            .header("Accept", kind)
            .header("User-Agent", "Xcode")
            .header("X-Apple-Identity-Token", &identity)
            .header("X-Apple-App-Info", "com.apple.gs.xcode.auth")
            .header("X-Xcode-Version", "11.2 (11B41)");
        for (k, v) in &anisette {
            builder = builder.header(k, v);
        }
        builder
    };

    let push = with_headers(client.get("https://gsa.apple.com/auth/verify/trusteddevice"), true).send()?;
    if !push.status().is_success() {
        eprintln!("warning: Apple didn't push a code ({}).", push.status());
    }
    let mut sms: Option<(i64, String)> = None;
    loop {
        let prompt = match &sms {
            Some((_, number)) => format!("Enter the 6-digit code texted to {number}: "),
            None => "Enter the new 6-digit code from your Apple device, or type sms to get it by text: ".to_string(),
        };
        let answer = code_prompt(&prompt)?;
        if answer.trim().eq_ignore_ascii_case("sms") && sms.is_none() {
            let auth: serde_json::Value = with_headers(client.get("https://gsa.apple.com/auth"), false).send()?.json()?;
            let phone = auth
                .pointer("/trustedPhoneNumbers/0")
                .and_then(|p| Some((p.get("id")?.as_i64()?, p.get("numberWithDialCode").and_then(serde_json::Value::as_str).unwrap_or("your phone").to_string())))
                .context("Apple didn't list a trusted phone number for this account")?;
            let reply = with_headers(client.put("https://gsa.apple.com/auth/verify/phone/"), false)
                .json(&serde_json::json!({ "phoneNumber": { "id": phone.0 }, "mode": "sms" }))
                .send()?;
            if !reply.status().is_success() {
                bail!("Apple couldn't send the text ({})", reply.status());
            }
            sms = Some(phone);
            continue;
        }
        let code = answer.trim();
        let reply = match &sms {
            None => with_headers(client.get("https://gsa.apple.com/grandslam/GsService2/validate"), true).header("security-code", code).send()?,
            Some((id, _)) => with_headers(client.post("https://gsa.apple.com/auth/verify/phone/securitycode"), false)
                .json(&serde_json::json!({ "phoneNumber": { "id": id }, "securityCode": { "code": code }, "mode": "sms" }))
                .send()?,
        };
        if reply.status().is_success() {
            return Ok(());
        }
        eprintln!("2FA code was rejected ({}); try again.", reply.status());
    }
}

/// Sign in and fetch the search-party token used for beacon reports.
pub fn login(
    provider: &dyn Provider,
    apple_id: &str,
    password: &str,
    mut code_prompt: impl FnMut(&str) -> Result<String>,
) -> Result<SearchPartyAuth> {
    let client = http()?;
    let ids = DeviceIds::generate();
    let spd = authenticate(&client, provider, &ids, apple_id, password, &mut code_prompt, 0)?;
    let adsid = spd.get("adsid").and_then(Value::as_string).context("spd lacks adsid")?;
    let pet = spd
        .get("t")
        .and_then(Value::as_dictionary)
        .and_then(|t| t.get("com.apple.gs.idms.pet"))
        .and_then(Value::as_dictionary)
        .and_then(|p| p.get("token"))
        .and_then(Value::as_string)
        .context("sign-in did not return a PET token")?;

    let mut delegates = Dictionary::new();
    delegates.insert("com.apple.mobileme".into(), Dictionary::new().into());
    let mut body = Dictionary::new();
    body.insert("apple-id".into(), apple_id.into());
    body.insert("delegates".into(), delegates.into());
    body.insert("password".into(), pet.into());
    body.insert("client-id".into(), ids.user_id.clone().into());
    let mut bytes = Vec::new();
    plist::to_writer_xml(&mut bytes, &Value::Dictionary(body))?;

    let mut builder = client
        .post("https://setup.icloud.com/setup/iosbuddy/loginDelegates")
        .basic_auth(apple_id, Some(pet))
        .header("X-Apple-ADSID", adsid)
        .header("User-Agent", "com.apple.iCloudHelper/282 CFNetwork/1408.0.4 Darwin/22.5.0")
        .body(bytes);
    for (k, v) in anisette::full_headers(provider, &ids)? {
        builder = builder.header(k, v);
    }
    let reply: Value = plist::from_bytes(&builder.send()?.bytes()?)?;
    let service = reply
        .as_dictionary()
        .and_then(|d| d.get("delegates"))
        .and_then(Value::as_dictionary)
        .and_then(|d| d.get("com.apple.mobileme"))
        .and_then(Value::as_dictionary)
        .and_then(|d| d.get("service-data"))
        .and_then(Value::as_dictionary)
        .context("loginDelegates reply lacks com.apple.mobileme service data")?;
    let token = service
        .get("tokens")
        .and_then(Value::as_dictionary)
        .and_then(|t| t.get("searchPartyToken"))
        .and_then(Value::as_string)
        .context("no searchPartyToken; is Find My enabled on this account?")?;
    let dsid = match service.get("dsid") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Integer(i)) => i.to_string(),
        _ => bail!("loginDelegates reply lacks dsid"),
    };
    Ok(SearchPartyAuth { dsid, search_party_token: token.into(), ids })
}

#[derive(Debug, Deserialize)]
pub struct EncryptedReport {
    pub id: String,
    pub payload: String,
    #[serde(rename = "datePublished", default)]
    pub date_published: i64,
}

/// Fetch encrypted reports for hashed key ids over the last `days` days.
pub fn fetch_reports(provider: &dyn Provider, auth: &SearchPartyAuth, hashed_ids: &[String], days: i64) -> Result<Vec<EncryptedReport>> {
    #[derive(Deserialize)]
    struct Reply {
        results: Vec<EncryptedReport>,
    }
    let end = chrono::Utc::now().timestamp_millis();
    let body = serde_json::json!({
        "search": [{ "startDate": end - days * 86_400_000, "endDate": end, "ids": hashed_ids }]
    });
    let mut builder = http()?
        .post("https://gateway.icloud.com/acsnservice/fetch")
        .basic_auth(&auth.dsid, Some(&auth.search_party_token))
        .json(&body);
    for (k, v) in anisette::full_headers(provider, &auth.ids)? {
        builder = builder.header(k, v);
    }
    let reply = builder.send()?;
    if reply.status() == reqwest::StatusCode::UNAUTHORIZED {
        bail!("beacon session expired; run `bbd findmy-login --beacons` again");
    }
    let reply: Reply = reply.error_for_status()?.json()?;
    Ok(reply.results)
}

/// Decode a base64 report payload.
pub fn payload_bytes(report: &EncryptedReport) -> Result<Vec<u8>> {
    Ok(B64.decode(&report.payload)?)
}
