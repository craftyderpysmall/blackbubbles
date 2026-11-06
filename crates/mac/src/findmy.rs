//! Combines iCloud Find My devices with BlackBubbles beacons into one list.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bb_findmy::anisette::{Provider, Remote};
use bb_findmy::{beacons, gsa, icloud};
use bb_proto::{ApiError, DeviceKind, ErrorCode, FindMyDevice, Location, Response};

const DEVICE_TTL: Duration = Duration::from_secs(60);
const BEACON_TTL: Duration = Duration::from_secs(5 * 60);

type Cache = Mutex<Option<(Instant, Vec<FindMyDevice>)>>;

pub struct FindMy {
    dir: PathBuf,
    session: Mutex<Option<icloud::Session>>,
    devices: Cache,
    beacons: Cache,
}

pub fn anisette_provider(data_dir: &Path) -> Option<Box<dyn Provider>> {
    if let Ok(url) = std::fs::read_to_string(data_dir.join("anisette_url")) {
        return Some(Box::new(Remote::new(url.trim())));
    }
    #[cfg(target_os = "macos")]
    return crate::anisette::Native::new().ok().map(|n| Box::new(n) as Box<dyn Provider>);
    #[cfg(not(target_os = "macos"))]
    None
}

fn cached(cache: &Cache, ttl: Duration, fetch: impl FnOnce() -> Result<Vec<FindMyDevice>, String>) -> Result<Vec<FindMyDevice>, String> {
    if let Some((at, list)) = &*cache.lock().unwrap()
        && at.elapsed() < ttl
    {
        return Ok(list.clone());
    }
    let list = fetch()?;
    *cache.lock().unwrap() = Some((Instant::now(), list.clone()));
    Ok(list)
}

impl FindMy {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.to_path_buf(),
            session: Mutex::new(None),
            devices: Mutex::new(None),
            beacons: Mutex::new(None),
        }
    }

    fn apple_devices(&self) -> Result<Vec<FindMyDevice>, String> {
        let mut guard = self.session.lock().unwrap();
        if guard.is_none() {
            *guard = Some(icloud::Session::open(&self.dir.join("icloud")).map_err(|e| e.to_string())?);
        }
        let session = guard.as_mut().unwrap();
        if !session.is_signed_in() {
            return Err("Find My is not signed in; run `bbd findmy-login <apple id>` on the Mac".into());
        }
        let devices = session.devices().map_err(|e| format!("{e:#}"))?;
        Ok(devices
            .into_iter()
            .map(|d| FindMyDevice {
                id: d.id,
                name: d.name,
                kind: DeviceKind::Apple,
                model: d.model,
                battery: d.battery,
                location: d.location.map(|(lat, lon, accuracy_m, at)| Location { lat, lon, accuracy_m, at }),
            })
            .collect())
    }

    fn beacon_devices(&self) -> Result<Vec<FindMyDevice>, String> {
        let list = beacons::Registry::new(&self.dir).list();
        if list.is_empty() {
            return Ok(vec![]);
        }
        let auth = gsa::SearchPartyAuth::load(&self.dir.join("icloud/searchparty.json")).map_err(|e| e.to_string())?;
        let provider = anisette_provider(&self.dir).ok_or("no anisette source for beacon lookups")?;
        let mut out = Vec::new();
        for beacon in list {
            let location = beacon.locate(&*provider, &auth).map_err(|e| format!("{}: {e:#}", beacon.name))?;
            out.push(FindMyDevice {
                id: beacon.id.clone(),
                name: beacon.name.clone(),
                kind: DeviceKind::Beacon,
                model: "BlackBubbles beacon".into(),
                battery: None,
                location: location.map(|r| Location {
                    lat: r.lat,
                    lon: r.lon,
                    accuracy_m: f32::from(r.accuracy_m),
                    at: r.timestamp * 1000,
                }),
            });
        }
        Ok(out)
    }

    pub fn list(&self) -> Result<Response, ApiError> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        match cached(&self.devices, DEVICE_TTL, || self.apple_devices()) {
            Ok(list) => out.extend(list),
            Err(e) => errors.push(e),
        }
        match cached(&self.beacons, BEACON_TTL, || self.beacon_devices()) {
            Ok(list) => out.extend(list),
            Err(e) => errors.push(e),
        }
        if out.is_empty() && !errors.is_empty() {
            return Err(ApiError::new(ErrorCode::Unsupported, errors.join("; ")));
        }
        Ok(Response::FindMyDevices(out))
    }

    pub fn register_beacon(&self, id: &str, name: &str, keys: &[Vec<u8>]) -> Result<Response, ApiError> {
        beacons::Registry::new(&self.dir)
            .register(id, name, keys)
            .map_err(|e| ApiError::new(ErrorCode::BadRequest, format!("{e:#}")))?;
        *self.beacons.lock().unwrap() = None;
        Ok(Response::Ok)
    }
}
