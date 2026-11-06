use std::path::{Path, PathBuf};

use bb_proto::{ApiError, Response, Timestamp};
use rusqlite::{Connection, OpenFlags};

/// Seconds between the Unix epoch and Apple's 2001-01-01 epoch.
pub const APPLE_EPOCH_SECS: i64 = 978_307_200;

/// Messages stores nanoseconds since 2001 (older macOS: seconds).
pub fn apple_nanos_to_unix_ms(value: i64) -> Timestamp {
    let ms_since_2001 = if value.abs() > 1_000_000_000_000 { value / 1_000_000 } else { value * 1000 };
    ms_since_2001 + APPLE_EPOCH_SECS * 1000
}

pub fn unix_ms_to_apple_nanos(ms: Timestamp) -> i64 {
    (ms - APPLE_EPOCH_SECS * 1000) * 1_000_000
}

/// Core Data stores fractional seconds since 2001.
pub fn apple_secs_to_unix_ms(value: f64) -> Timestamp {
    ((value + APPLE_EPOCH_SECS as f64) * 1000.0) as Timestamp
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

pub fn open_readonly(path: &Path) -> Result<Connection, ApiError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|err| {
            ApiError::new(
                bb_proto::ErrorCode::PermissionDenied,
                format!(
                    "cannot open {} ({err}). Grant bbd Full Disk Access in System Settings → Privacy & Security.",
                    path.display()
                ),
            )
        })
}

/// Columns of a table, for coping with schema differences between macOS versions.
pub fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else { return vec![] };
    stmt.query_map([], |row| row.get::<_, String>(1))
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub fn sql_err(err: rusqlite::Error) -> ApiError {
    ApiError::internal(format!("database error: {err}"))
}

/// Serve `len` bytes at `offset` from a file.
pub fn read_blob(path: &Path, offset: u64, len: u32, mime: Option<String>) -> Result<Response, ApiError> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|e| ApiError::not_found(format!("{} ({e})", path.display())))?;
    let total = file.metadata().map_err(ApiError::internal)?.len();
    file.seek(SeekFrom::Start(offset.min(total))).map_err(ApiError::internal)?;
    let want = u64::from(len.min(bb_proto::MAX_BLOB_CHUNK)).min(total.saturating_sub(offset)) as usize;
    let mut data = vec![0u8; want];
    file.read_exact(&mut data).map_err(ApiError::internal)?;
    Ok(Response::Blob { data, total, mime_type: mime })
}

/// Run an AppleScript with arguments passed as `argv` (never interpolated into the source).
pub fn osascript(script: &str, args: &[&str]) -> Result<String, ApiError> {
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .args(args)
        .output()
        .map_err(|e| ApiError::internal(format!("osascript: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(ApiError::internal(format!(
            "AppleScript failed: {}. Allow bbd to control the app in System Settings → Privacy & Security → Automation.",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// Convert a file with a macOS tool into the cache, reusing earlier conversions.
pub fn cached_conversion(cache: &Path, key: &str, ext: &str, convert: impl FnOnce(&Path) -> std::io::Result<bool>) -> Result<PathBuf, ApiError> {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let out = cache.join(format!("{safe}.{ext}"));
    if out.exists() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache).map_err(ApiError::internal)?;
    let tmp = cache.join(format!("{safe}.tmp.{ext}"));
    match convert(&tmp) {
        Ok(true) => {
            std::fs::rename(&tmp, &out).map_err(ApiError::internal)?;
            Ok(out)
        }
        Ok(false) => Err(ApiError::internal("conversion failed")),
        Err(e) => Err(ApiError::internal(format!("conversion failed: {e}"))),
    }
}

pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_time_conversions() {
        // 2023-01-01T00:00:00Z
        let unix_ms = 1_672_531_200_000;
        let nanos = unix_ms_to_apple_nanos(unix_ms);
        assert_eq!(apple_nanos_to_unix_ms(nanos), unix_ms);
        assert_eq!(apple_nanos_to_unix_ms(nanos / 1_000_000_000), unix_ms);
        assert_eq!(apple_secs_to_unix_ms(694_224_000.0), unix_ms);
    }
}
