//! Offline probe for locked-note encryption formats. Runs on Linux against a dump from bbd:
//!
//! ```sh
//! blackbubbles request '{"NoteCryptoDump":{"id":"x-coredata://…/ICNote/p112"}}' > dump.json
//! cargo run --release -p bb-mac --example note_probe -- dump.json   # then type the password
//! ```
//!
//! It checks the password against the account verifier, then tries a large matrix: key
//! derivations × every plausibly-sized blob in the dump as the wrapped key (including byte
//! strings found inside the CloudKit record archives) × every IV/tag pair, using AES-GCM,
//! RFC 3394, ECB and XOR. Only AES-GCM / AES-KW integrity checks count as hits, so it never
//! reports a false positive. It prints constructions and lengths only, never note text.

use std::collections::BTreeMap;
use std::io::BufRead;

use bb_mac::notecrypt::{self, Account, Locked};
use serde_json::Value;

type Blobs = BTreeMap<String, Vec<u8>>;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn collect(prefix: &str, row: &Value, out: &mut Blobs, ints: &mut BTreeMap<String, i64>) {
    let Some(obj) = row.as_object() else { return };
    for (k, v) in obj {
        let name = format!("{prefix}.{k}");
        if let Some(h) = v.get("hex").and_then(|h| h.as_str()) {
            let bytes = hex(h);
            if bytes.len() > 64 {
                embedded(&name, &bytes, out);
            }
            out.insert(name, bytes);
        } else if let Some(n) = v.as_i64() {
            ints.insert(name, n);
        }
    }
}

/// Byte strings inside a binary plist (NSKeyedArchiver'd CKRecord): marker 0x4N (N < 15)
/// or 0x4F 0x10 len. Only sizes that could be a key, IV, tag, salt or wrapped key.
fn embedded(name: &str, b: &[u8], out: &mut Blobs) {
    for i in 0..b.len() {
        let (len, start) = match b[i] {
            0x4C => (12, i + 1),
            0x4F if b.get(i + 1) == Some(&0x10) => (*b.get(i + 2).unwrap_or(&0) as usize, i + 3),
            _ => continue,
        };
        if [12, 16, 24, 32, 40].contains(&len) && start + len <= b.len() {
            out.insert(format!("{name}@{i}"), b[start..start + len].to_vec());
        }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: note_probe dump.json (password on stdin)");
    let text = std::fs::read_to_string(&path).expect("read dump");
    // Accept either the bare dump or `blackbubbles request` output ({"Diagnostics": "<json>"}).
    let mut dump: Value = serde_json::from_str(&text).expect("json");
    if let Some(inner) = dump.get("Diagnostics").and_then(|d| d.as_str()) {
        dump = serde_json::from_str(inner).expect("inner json");
    }
    eprint!("Notes password: ");
    let mut password = String::new();
    std::io::stdin().lock().read_line(&mut password).unwrap();
    let password = password.trim_end_matches(['\n', '\r']);

    let mut blobs = Blobs::new();
    let mut ints = BTreeMap::new();
    collect("note", &dump["note"], &mut blobs, &mut ints);
    collect("data", &dump["note_data"], &mut blobs, &mut ints);
    for section in ["accounts", "account_data"] {
        for (pk, row) in dump[section].as_object().into_iter().flatten() {
            collect(&format!("{section}[{pk}]"), row, &mut blobs, &mut ints);
        }
    }
    let body = blobs.remove("data.ZDATA").expect("no note_data.ZDATA");
    println!("body: {} bytes", body.len());
    for (k, v) in &blobs {
        if v.len() <= 64 {
            println!("  {k}: {} bytes", v.len());
        }
    }
    for (k, v) in &ints {
        if k.contains("CRYPTO") {
            println!("  {k} = {v}");
        }
    }

    // 1. Password check against every account verifier.
    let mut master = None;
    for (k, verifier) in blobs.iter().filter(|(k, _)| k.starts_with("accounts[") && k.ends_with(".ZCRYPTOVERIFIER")) {
        let acct = k.trim_end_matches(".ZCRYPTOVERIFIER");
        let salt = &blobs[&format!("{acct}.ZCRYPTOSALT")];
        let iters = ints[&format!("{acct}.ZCRYPTOITERATIONCOUNT")] as u32;
        let kek = notecrypt::kek(password, salt, iters);
        match notecrypt::unwrap_key(&kek, verifier) {
            Some(m) => {
                println!("password OK for {acct} (master key {} bytes)", m.len());
                master = Some((acct.to_string(), salt.clone(), iters, verifier.clone(), kek, m));
            }
            None => println!("password does NOT unwrap {acct}'s verifier"),
        }
    }
    let Some((_acct, acct_salt, acct_iters, verifier, acct_kek, master)) = master else {
        println!("No account verifier accepted the password; stopping.");
        return;
    };

    // 2. Key candidates.
    let empty = Vec::new();
    let note_salt = blobs.get("note.ZCRYPTOSALT").unwrap_or(&empty).clone();
    let wrapped_db = blobs.get("note.ZCRYPTOWRAPPEDKEY").unwrap_or(&empty).clone();
    let iv_data = blobs.get("data.ZCRYPTOINITIALIZATIONVECTOR").unwrap_or(&empty).clone();
    let tag_data = blobs.get("data.ZCRYPTOTAG").unwrap_or(&empty).clone();
    let account = Account { salt: &acct_salt, iterations: acct_iters, verifier: &verifier };
    let locked = Locked {
        salt: &note_salt,
        iterations: 0,
        wrapped_key: &wrapped_db,
        key_iv: blobs.get("note.ZCRYPTOINITIALIZATIONVECTOR").unwrap_or(&iv_data),
        key_tag: blobs.get("note.ZCRYPTOTAG").unwrap_or(&tag_data),
        iv: &iv_data,
        tag: &tag_data,
        data: &body,
    };
    match notecrypt::decrypt_traced(&locked, Some(&account), password) {
        Ok((plain, how)) => {
            println!("bbd's built-in matrix decrypts it: {how} ({} bytes)", plain.len());
        }
        Err(e) => println!("bbd's built-in matrix: {e:?}"),
    }

    let mut keks = notecrypt::account_keks(password, &acct_kek, &master, &account, &locked);
    let salts: Vec<(&String, &Vec<u8>)> = blobs.iter().filter(|(_, v)| v.len() == 16).collect();
    for (name, s) in &salts {
        for iters in [1u32, 1000, 10_000, acct_iters, 100_000] {
            keks.push((format!("pbkdf2(pw,{name},{iters})"), notecrypt::pbkdf16(password.as_bytes(), s, iters)));
        }
        keks.push((format!("hkdf(master,{name})"), notecrypt::hkdf16(s, &master, b"")));
        keks.push((format!("hkdf(master,info={name})"), notecrypt::hkdf16(b"", &master, s)));
        keks.push((format!("hmac(master,{name})"), notecrypt::hmac16(&master, s)));
        keks.push((format!("raw {name}"), s.to_vec()));
        if let Some(m) = <&[u8; 16]>::try_from(master.as_slice()).ok().and_then(|m| notecrypt::unwrap_key(m, s)) {
            keks.push((format!("kw(master,{name})"), m));
        }
    }
    keks.push(("sha256(pw)".into(), notecrypt::sha16(&[password.as_bytes()])));
    keks.retain(|(_, k)| k.len() == 16);
    println!("{} key candidates", keks.len());

    // 3. Note-key candidates: authenticated unwraps of every wrapped-sized blob.
    let ivs: Vec<(&String, &Vec<u8>)> = blobs.iter().filter(|(_, v)| v.len() == 16 || v.len() == 12).collect();
    let tags: Vec<(&String, &Vec<u8>)> = blobs.iter().filter(|(_, v)| v.len() == 16).collect();
    let wrapped: Vec<(&String, &Vec<u8>)> = blobs.iter().filter(|(_, v)| [16, 24, 32, 40].contains(&v.len())).collect();
    let mut note_keys: Vec<(String, Vec<u8>, bool)> = keks.iter().map(|(n, k)| (n.clone(), k.clone(), false)).collect();
    for (kn, k) in &keks {
        for (wn, w) in &wrapped {
            if let Some(nk) = <&[u8; 16]>::try_from(k.as_slice()).ok().and_then(|k| notecrypt::unwrap_key(k, w)) {
                println!("AUTH kw({kn}, {wn}) → {}-byte key", nk.len());
                note_keys.push((format!("kw({kn},{wn})"), nk, true));
            }
            if w.len() == 16 {
                for (ivn, iv) in &ivs {
                    for (tn, tag) in &tags {
                        if let Some(nk) = notecrypt::gcm_open(k, iv, tag, w) {
                            println!("AUTH gcm({kn}, ct={wn}, iv={ivn}, tag={tn}) → {}-byte key", nk.len());
                            note_keys.push((format!("gcm({kn},{wn},{ivn},{tn})"), nk, true));
                        }
                    }
                }
                for (m, nk) in [("ecb-dec", notecrypt::ecb(k, w, false)), ("ecb-enc", notecrypt::ecb(k, w, true))] {
                    if let Some(nk) = nk {
                        note_keys.push((format!("{m}({kn},{wn})"), nk, false));
                    }
                }
                note_keys.push((format!("xor({kn},{wn})"), k.iter().zip(w.iter()).map(|(a, b)| a ^ b).collect(), false));
            }
        }
    }
    println!("{} note-key candidates; trying the body…", note_keys.len());

    // 4. Body: every note key × IV/tag pairs from crypto columns.
    let body_ivs: Vec<_> = ivs.iter().filter(|(n, _)| n.contains("INITIALIZATIONVECTOR")).collect();
    let body_tags: Vec<_> = tags.iter().filter(|(n, _)| n.contains("TAG")).collect();
    let mut found = false;
    for (name, nk, auth) in &note_keys {
        for (ivn, iv) in &body_ivs {
            for (tn, tag) in &body_tags {
                if let Some(plain) = notecrypt::gcm_open(nk, iv, tag, &body) {
                    let decoded = bb_mac::notes::decode_body(&plain).map(|t| t.chars().count());
                    println!("BODY DECRYPTS: key={name} (key authenticated: {auth}) iv={ivn} tag={tn}; {} bytes, decoded text chars: {decoded:?}", plain.len());
                    found = true;
                }
            }
        }
    }
    if !found {
        println!("No construction decrypted the body.");
    }
}
