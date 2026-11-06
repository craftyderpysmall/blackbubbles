//! Find My for BlackBubbles.
//!
//! - [`icloud`]: your Apple devices, via the iCloud web Find My API.
//! - [`keys`], [`report`], [`gsa`], [`beacons`]: BLE Offline-Finding beacons (e.g. a Linux
//!   PC), located by nearby Apple devices and decrypted here.
//!
//! Everything here talks to undocumented Apple endpoints and may break without notice.

pub mod anisette;
pub mod beacons;
pub mod gsa;
pub mod icloud;
pub mod keys;
pub mod report;
pub mod srp;

use std::path::Path;

/// Write a file readable only by the current user.
pub(crate) fn write_private(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)?;
    Ok(())
}
