//! `bb-beacon`: broadcast this PC as an Offline-Finding accessory.
//!
//! Reads only public keys (written by `blackbubbles beacon-setup`), so it can run with
//! raised privileges without access to any secret. Nearby iPhones, iPads and Macs pick up
//! the advertisement and upload encrypted location reports, which the Mac server decrypts.

#![cfg_attr(not(target_os = "linux"), allow(unused))]

mod hci;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bb_findmy::beacons::PublicKeys;
use bb_findmy::keys::AdvertisedKey;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bb-beacon", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Advertise the current key, rotating on schedule (needs CAP_NET_RAW).
    Advertise {
        /// beacon-public.json from `blackbubbles beacon-setup`.
        #[arg(long)]
        keys: PathBuf,
        /// HCI device index (0 for hci0).
        #[arg(long, default_value_t = 0)]
        hci: u16,
    },
    /// Print what would be advertised right now (no privileges needed).
    Show {
        #[arg(long)]
        keys: PathBuf,
    },
}

static STOP: AtomicBool = AtomicBool::new(false);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn load(path: &PathBuf) -> Result<PublicKeys> {
    let keys: PublicKeys = serde_json::from_slice(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?;
    anyhow::ensure!(!keys.keys.is_empty(), "{} has no keys", path.display());
    Ok(keys)
}

fn current(keys: &PublicKeys) -> Result<AdvertisedKey> {
    AdvertisedKey::from_base64(keys.current(now()).context("no keys")?)
}

fn format_address(address: &[u8; 6]) -> String {
    address.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Show { keys } => {
            let keys = load(&keys)?;
            let key = current(&keys)?;
            println!("key slot rotates every {} s across {} keys", keys.rotate_secs, keys.keys.len());
            println!("address  {}", format_address(&key.ble_address()));
            println!("payload  {}", hex::encode(key.advertising_data()));
            println!("lookup   {}", key.hashed_id());
            Ok(())
        }
        Command::Advertise { keys, hci } => advertise(&keys, hci),
    }
}

#[cfg(target_os = "linux")]
fn advertise(path: &PathBuf, dev: u16) -> Result<()> {
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let socket = hci::Socket::open(dev)?;
    let mut active: Option<AdvertisedKey> = None;
    let mut last_apply = 0u64;
    while !STOP.load(Ordering::SeqCst) {
        // Re-read so a new key set from beacon-setup takes effect without a restart.
        let keys = load(path)?;
        let key = current(&keys)?;
        // Re-apply every minute too, in case BlueZ reset the controller.
        if active != Some(key) || now() - last_apply >= 60 {
            let mode = socket.advertise(&key.ble_address(), &key.advertising_data())?;
            if active != Some(key) {
                eprintln!("advertising {} ({mode})", format_address(&key.ble_address()));
            }
            active = Some(key);
            last_apply = now();
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    socket.stop();
    eprintln!("stopped");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn advertise(_path: &PathBuf, _dev: u16) -> Result<()> {
    let _ = &STOP;
    anyhow::bail!("bb-beacon only runs on Linux; Macs are already in Find My")
}
