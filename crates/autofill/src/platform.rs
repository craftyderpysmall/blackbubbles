//! Clipboard, notifications, typing and URL opening per desktop.
//!
//! - Wayland: `wl-copy`/`wl-paste` (work on every compositor, including GNOME), typing via
//!   `wtype` (wlroots/KDE) or `ydotool` (GNOME, needs the ydotoold service).
//! - X11: arboard for the clipboard, XTest (enigo) for typing.
//! - macOS: arboard (NSPasteboard) and Notification Center. Clicking a notification
//!   activates BlackBubbles, so instead of typing the code is left on the clipboard.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Clip {
    pub mime: String,
    pub data: Vec<u8>,
}

pub trait Platform: Send + Sync {
    /// The current clipboard, or `None` if empty or owned by a password manager.
    fn snapshot(&self) -> Option<Clip>;
    fn copy(&self, text: &str);
    fn current_text(&self) -> Option<String>;
    /// Put back a snapshot; `None` clears the clipboard.
    fn restore(&self, clip: Option<&Clip>);
    /// Show a notification; blocks until it closes. Returns true if `action` was clicked.
    fn notify(&self, summary: &str, body: &str, action: Option<&str>, timeout_secs: u64) -> bool;
    fn can_type(&self) -> bool;
    fn type_text(&self, text: &str);
    fn open_url(&self, url: &str) {
        let _ = open::that_detached(url);
    }
}

fn which(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

fn run_with_stdin(cmd: &mut Command, data: &[u8]) -> bool {
    let Ok(mut child) = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(data);
    }
    child.wait().is_ok_and(|s| s.success())
}

fn output(cmd: &mut Command) -> Option<Vec<u8>> {
    let out = cmd.stderr(Stdio::null()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// Pick the right platform for this session.
pub fn detect() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    return Box::new(MacOs::new());
    #[cfg(not(target_os = "macos"))]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wl-copy") && which("wl-paste") {
            Box::new(Wayland)
        } else {
            Box::new(X11::new())
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn xdg_notify(summary: &str, body: &str, action: Option<&str>, timeout_secs: u64) -> bool {
    // Escaped because notification daemons render markup and the sender is untrusted.
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut n = notify_rust::Notification::new();
    n.appname("BlackBubbles")
        .summary(&escape(summary))
        .body(&escape(body))
        .timeout(notify_rust::Timeout::Milliseconds((timeout_secs * 1000) as u32));
    if let Some(label) = action {
        n.action("default", label);
    }
    let Ok(handle) = n.show() else { return false };
    if action.is_none() {
        return false;
    }
    let mut clicked = false;
    handle.wait_for_action(|a| clicked = a == "default");
    clicked
}

// --- Pasting files into Messages ------------------------------------------------------

/// A file on the clipboard, for pasting into a chat.
pub struct PastedFile {
    pub name: String,
    pub data: Vec<u8>,
}

/// Image types a chat can take, best first, with the extension for the file name.
#[cfg(not(target_os = "macos"))]
const PASTE_IMAGES: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
    ("image/heic", "heic"),
    ("image/tiff", "tiff"),
    ("image/bmp", "bmp"),
];

/// Largest file taken from the clipboard (Messages' own attachment limit is about this).
#[cfg(not(target_os = "macos"))]
const PASTE_MAX: u64 = 100 * 1024 * 1024;

/// Files copied in a file manager (`text/uri-list` with `file://` URIs), or else one image
/// (a screenshot, "Copy Image" in a browser). Empty if neither, or a password manager owns
/// the clipboard. WebKitGTK doesn't hand clipboard images to the page, hence this.
pub fn clipboard_files() -> Vec<PastedFile> {
    #[cfg(target_os = "macos")]
    return macos_clipboard_image().into_iter().collect();
    #[cfg(not(target_os = "macos"))]
    {
        type Get = fn(&str) -> Option<Vec<u8>>;
        let (list, get): (Vec<&str>, Get) =
            if std::env::var_os("WAYLAND_DISPLAY").is_some() && which("wl-paste") {
                (vec!["wl-paste", "--list-types"], |mime| output(Command::new("wl-paste").args(["--no-newline", "--type", mime])))
            } else if which("xclip") {
                (vec!["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"], |mime| {
                    output(Command::new("xclip").args(["-selection", "clipboard", "-t", mime, "-o"]))
                })
            } else {
                return Vec::new();
            };
        let Some(types) = output(Command::new(list[0]).args(&list[1..])).and_then(|t| String::from_utf8(t).ok()) else {
            return Vec::new();
        };
        let types: Vec<&str> = types.lines().map(str::trim).collect();
        if types.contains(&"x-kde-passwordManagerHint") {
            return Vec::new();
        }
        if types.contains(&"text/uri-list") {
            let files: Vec<PastedFile> = get("text/uri-list")
                .and_then(|t| String::from_utf8(t).ok())
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.trim().strip_prefix("file://").map(percent_decode))
                .filter_map(|path| read_pasted(std::path::Path::new(&path)))
                .collect();
            if !files.is_empty() {
                return files;
            }
        }
        PASTE_IMAGES
            .iter()
            .find(|(mime, _)| types.contains(mime))
            .and_then(|(mime, ext)| get(mime).map(|data| PastedFile { name: format!("Pasted image.{ext}"), data }))
            .filter(|f| !f.data.is_empty())
            .into_iter()
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
fn read_pasted(path: &std::path::Path) -> Option<PastedFile> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > PASTE_MAX {
        return None;
    }
    Some(PastedFile { name: path.file_name()?.to_string_lossy().into_owned(), data: std::fs::read(path).ok()? })
}

/// `%20` → space, etc. (file URIs from file managers).
#[cfg(not(target_os = "macos"))]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// macOS: the clipboard as PNG, via AppleScript (`«data PNGf89504E…»`).
#[cfg(target_os = "macos")]
fn macos_clipboard_image() -> Option<PastedFile> {
    let out = output(Command::new("/usr/bin/osascript").args(["-e", "the clipboard as «class PNGf»"]))?;
    let text = String::from_utf8(out).ok()?;
    let hex = text.trim().strip_prefix("«data PNGf")?.strip_suffix('»')?;
    let data: Option<Vec<u8>> = (0..hex.len() / 2).map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()).collect();
    Some(PastedFile { name: "Pasted image.png".into(), data: data? })
}

#[cfg(all(test, not(target_os = "macos")))]
mod paste_tests {
    #[test]
    fn percent_decoding() {
        assert_eq!(super::percent_decode("/home/a/My%20Pics/caf%C3%A9.png"), "/home/a/My Pics/café.png");
        assert_eq!(super::percent_decode("/x/100%"), "/x/100%");
        assert_eq!(super::percent_decode("/x/%2"), "/x/%2");
    }
}

// --- Wayland --------------------------------------------------------------------------

pub struct Wayland;

/// Clipboard types worth saving, in order of preference.
const MIME_PREFERENCE: &[&str] = &["text/uri-list", "text/plain;charset=utf-8", "text/plain", "UTF8_STRING", "image/png"];

impl Platform for Wayland {
    fn snapshot(&self) -> Option<Clip> {
        let types = String::from_utf8(output(Command::new("wl-paste").arg("--list-types"))?).ok()?;
        let types: Vec<&str> = types.lines().collect();
        // KeePassXC and friends mark copied secrets; never keep those around.
        if types.contains(&"x-kde-passwordManagerHint") {
            return None;
        }
        let mime = MIME_PREFERENCE.iter().find(|m| types.contains(m))?;
        let data = output(Command::new("wl-paste").args(["--no-newline", "--type", mime]))?;
        Some(Clip { mime: mime.to_string(), data })
    }

    fn copy(&self, text: &str) {
        // Through stdin so the code never appears in the process list.
        run_with_stdin(&mut Command::new("wl-copy"), text.as_bytes());
    }

    fn current_text(&self) -> Option<String> {
        String::from_utf8(output(Command::new("wl-paste").args(["--no-newline", "--type", "text/plain"]))?).ok()
    }

    fn restore(&self, clip: Option<&Clip>) {
        match clip {
            Some(clip) => {
                run_with_stdin(Command::new("wl-copy").args(["--type", &clip.mime]), &clip.data);
            }
            None => {
                let _ = Command::new("wl-copy").arg("--clear").status();
            }
        }
    }

    fn notify(&self, summary: &str, body: &str, action: Option<&str>, timeout_secs: u64) -> bool {
        #[cfg(not(target_os = "macos"))]
        return xdg_notify(summary, body, action, timeout_secs);
        #[cfg(target_os = "macos")]
        {
            let _ = (summary, body, action, timeout_secs);
            false
        }
    }

    fn can_type(&self) -> bool {
        which("wtype") || which("ydotool")
    }

    fn type_text(&self, text: &str) {
        if which("wtype") && Command::new("wtype").args(["--", text]).status().is_ok_and(|s| s.success()) {
            return;
        }
        if which("ydotool") {
            let _ = Command::new("ydotool").args(["type", "--", text]).status();
        }
    }
}

// --- arboard-based (X11 and macOS) ----------------------------------------------------

/// Holds the clipboard open: on X11 the owner must stay alive to serve pastes.
struct Board(Mutex<Option<arboard::Clipboard>>);

impl Board {
    fn new() -> Self {
        Self(Mutex::new(arboard::Clipboard::new().ok()))
    }
    fn with<T>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
        let mut guard = self.0.lock().unwrap();
        if guard.is_none() {
            *guard = arboard::Clipboard::new().ok();
        }
        f(guard.as_mut()?)
    }
    fn snapshot(&self) -> Option<Clip> {
        self.with(|c| c.get_text().ok()).map(|t| Clip { mime: "text/plain".into(), data: t.into_bytes() })
    }
    fn copy(&self, text: &str) {
        self.with(|c| c.set_text(text).ok());
    }
    fn current_text(&self) -> Option<String> {
        self.with(|c| c.get_text().ok())
    }
    fn restore(&self, clip: Option<&Clip>) {
        match clip {
            Some(clip) => self.copy(&String::from_utf8_lossy(&clip.data)),
            None => {
                self.with(|c| c.clear().ok());
            }
        }
    }
}

fn enigo_type(text: &str) {
    use enigo::{Enigo, Keyboard, Settings};
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.text(text);
    }
}

#[cfg(not(target_os = "macos"))]
pub struct X11 {
    board: Board,
}

#[cfg(not(target_os = "macos"))]
impl X11 {
    pub fn new() -> Self {
        Self { board: Board::new() }
    }
}

#[cfg(not(target_os = "macos"))]
impl Platform for X11 {
    fn snapshot(&self) -> Option<Clip> {
        self.board.snapshot()
    }
    fn copy(&self, text: &str) {
        self.board.copy(text)
    }
    fn current_text(&self) -> Option<String> {
        self.board.current_text()
    }
    fn restore(&self, clip: Option<&Clip>) {
        self.board.restore(clip)
    }
    fn notify(&self, summary: &str, body: &str, action: Option<&str>, timeout_secs: u64) -> bool {
        xdg_notify(summary, body, action, timeout_secs)
    }
    fn can_type(&self) -> bool {
        std::env::var_os("DISPLAY").is_some()
    }
    fn type_text(&self, text: &str) {
        enigo_type(text)
    }
}

#[cfg(target_os = "macos")]
pub struct MacOs {
    board: Board,
}

#[cfg(target_os = "macos")]
impl MacOs {
    pub fn new() -> Self {
        let bundle = std::env::var("BLACKBUBBLES_BUNDLE_ID").unwrap_or_else(|_| "com.craftyderpysmall.blackbubbles".into());
        if mac_notification_sys::set_application(&bundle).is_err() {
            let _ = mac_notification_sys::set_application("com.apple.Terminal");
        }
        Self { board: Board::new() }
    }
}

#[cfg(target_os = "macos")]
impl Platform for MacOs {
    fn snapshot(&self) -> Option<Clip> {
        self.board.snapshot()
    }
    fn copy(&self, text: &str) {
        self.board.copy(text)
    }
    fn current_text(&self) -> Option<String> {
        self.board.current_text()
    }
    fn restore(&self, clip: Option<&Clip>) {
        self.board.restore(clip)
    }
    fn notify(&self, summary: &str, body: &str, action: Option<&str>, _timeout_secs: u64) -> bool {
        let mut options = mac_notification_sys::Notification::new();
        options.wait_for_click(action.is_some());
        matches!(
            mac_notification_sys::send_notification(summary, None, body, Some(&options)),
            Ok(mac_notification_sys::NotificationResponse::Click)
        )
    }
    fn can_type(&self) -> bool {
        false
    }
    fn type_text(&self, text: &str) {
        enigo_type(text)
    }
}

/// A plain informational notification (no actions, returns immediately).
pub fn simple_notify(summary: &str, body: &str) {
    #[cfg(not(target_os = "macos"))]
    {
        let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let _ = notify_rust::Notification::new().appname("BlackBubbles").summary(&escape(summary)).body(&escape(body)).show();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = mac_notification_sys::send_notification(summary, None, body, None);
    }
}
