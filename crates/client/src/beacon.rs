//! Turn this PC into a Find My beacon: generate keys, register the private halves with
//! the Mac (which fetches and decrypts reports), and write the public halves for the
//! root advertiser service.

use anyhow::{Result, bail};
use bb_findmy::beacons::PublicKeys;
use bb_findmy::keys::KeyPair;
use bb_proto::{Request, Response};

use crate::config::{self, BeaconConfig};
use crate::conn::Client;

/// Keys rotate every 15 minutes through this many slots, so a passer-by can't follow
/// one fixed address for long.
const KEYS: usize = 96;
const ROTATE_SECS: u64 = 15 * 60;

pub async fn setup(client: &Client, name: &str) -> Result<BeaconConfig> {
    let pairs: Vec<KeyPair> = (0..KEYS).map(|_| KeyPair::generate()).collect();
    let id = format!("beacon-{}", hex::encode(rand::random::<[u8; 6]>()));
    let private_keys: Vec<Vec<u8>> = pairs.iter().map(KeyPair::private_bytes).collect();
    match client.request(Request::RegisterBeacon { id: id.clone(), name: name.into(), private_keys: private_keys.clone() }).await {
        Ok(Response::Ok) => {}
        Ok(other) => bail!("unexpected reply {other:?}"),
        Err(e) => bail!("{}", e.message),
    }
    let public = PublicKeys { rotate_secs: ROTATE_SECS, keys: pairs.iter().map(|p| p.public_key().to_base64()).collect() };
    let path = config::beacon_public_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, serde_json::to_vec_pretty(&public)?)?;
    Ok(BeaconConfig { id, name: name.into(), private_keys: private_keys.iter().map(hex::encode).collect(), rotate_secs: ROTATE_SECS })
}
