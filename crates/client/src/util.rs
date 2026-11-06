use bb_proto::Timestamp;
use chrono::{DateTime, Local};

pub fn local(ms: Timestamp) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&Local))
}

/// "14:05" today, "Tue" this week, else "Mar 3".
pub fn short_time(ms: Timestamp) -> String {
    let Some(t) = local(ms) else { return String::new() };
    let now = Local::now();
    let days = (now.date_naive() - t.date_naive()).num_days();
    match days {
        0 => t.format("%H:%M").to_string(),
        1..=6 => t.format("%a").to_string(),
        _ if t.format("%Y").to_string() == now.format("%Y").to_string() => t.format("%b %-d").to_string(),
        _ => t.format("%b %-d, %Y").to_string(),
    }
}

pub fn long_time(ms: Timestamp) -> String {
    local(ms).map(|t| t.format("%a %b %-d, %Y · %H:%M").to_string()).unwrap_or_default()
}

pub fn ago(ms: Timestamp) -> String {
    let secs = (chrono::Utc::now().timestamp_millis() - ms) / 1000;
    match secs {
        ..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

pub fn duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

pub fn bytes(n: u64) -> String {
    match n {
        ..1024 => format!("{n} B"),
        ..1_048_576 => format!("{:.0} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// Save downloaded bytes into the user's Downloads folder without overwriting.
pub fn save_download(name: &str, data: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let dir = directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&dir)?;
    let clean: String = name.chars().filter(|c| !c.is_control() && *c != '/' && *c != '\\').collect();
    let clean = if clean.trim().is_empty() { "download".to_string() } else { clean };
    let mut path = dir.join(&clean);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{n} {clean}"));
        n += 1;
    }
    std::fs::write(&path, data)?;
    Ok(path)
}
