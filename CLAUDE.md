# BlackBubbles: notes for Claude and the owner

BlackBubbles is a fast bridge from the owner's Apple world to **Linux and macOS** desktops. It does **not** use BlueBubbles: it has its own Rust server (`bbd`) on the Mac, its own protocol, and its own client. The repository is public (MIT): github.com/craftyderpysmall/blackbubbles.

## Next task: move the client to Tauri

The owner wants the desktop client rebuilt with **Tauri v2** to remove egui's limits. The main one is that colour emoji render as boxes. Other things a web UI fixes:
- real text layout and selection
- rich note bodies
- smooth animations and CSS-accurate Neutral styling
- system font fallback

Plan:
1. Ask the owner first:
   - Frontend stack. I'd suggest React + Vite + Tailwind with shadcn/ui, which matches the theme's origin. SolidJS or Svelte are lighter alternatives.
   - Whether to delete `crates/client` (egui) once Tauri reaches feature parity, or keep both.
2. Keep all Rust crates as they are. The Tauri backend (`src-tauri`) should reuse:
   - `bb-net`, the connection (`crates/client/src/conn.rs` logic)
   - `bb-autofill`, the notifier (`notify.rs`) and config (`config.rs`)
   - `bb-findmy`, for beacon setup
   - the `daemon`/`pair`/`beacon-setup` CLI modes, which must keep working
3. Bridge to the frontend through Tauri commands (requests) and events (server `Event`s, connection status). Images and audio go through a custom URI scheme handler (e.g. `bb://blob/<kind>/<id>`) backed by chunked `GetBlob`, so the webview loads them like files.
4. Port every screen with the same behaviour and responsive rules (see *Design* and *Client features* below). Use virtualized lists for chats, photos and messages, since performance is a requirement.
5. Packaging:
   - Linux: WebKitGTK. Update the PKGBUILD dependencies (`webkit2gtk-4.1`).
   - macOS: Tauri's `.app` bundle replaces `packaging/macos/make-app.sh`.
   - Keep the systemd and launchd units.
6. Verify with `bbd run --mock`, screenshots, and CI on Linux and macOS (add the Tauri system deps to `.github/workflows/ci.yml`).

## Standing decisions (don't relitigate)

- **No BlueBubbles code or API.** Our own `bbd` server and protocol.
- **Messages are sent through AppleScript only.**
  - No Private API helper: no injecting into Messages.app, and SIP stays on. The owner decided this.
  - Consequence: no edit, unsend, tapbacks, typing indicators or read receipts. Don't propose a helper again.
- **Sending address:** the owner wants iMessages to go from their **phone number, not email**.
  - BlackBubbles replies from the address each chat already uses, and the Mac's Messages settings decide that address. The setup steps are in `docs/MAC_SETUP.md` and the Settings screen.
  - Each chat reports `my_address`; the UI shows it and flags email with the warning colour.
- **Find My for the Linux PC:** a BLE Offline-Finding beacon (`bb-beacon`) whose location is shown on BlackBubbles' own map. Apple's Find My app can't list non-Apple hardware.
- **Autofill:** a built-in port of [linuxautofill](https://github.com/craftyderpysmall/linuxautofill).
  - Sources: iMessage/SMS from the Mac, Gmail IMAP, and the iPhone Shortcut endpoint.
  - Desktops: Wayland, X11 and macOS.
  - Detection must stay a faithful port; `crates/autofill/tests/detect.rs` is the original suite, copied verbatim.
- **Performance is a requirement:** never block the UI thread.
- **Secrets** live in mode-600 files. The client config is refused if group/other can read it.

## Status

As of 2026-11-06, the owner has a Mac but **hasn't been able to set it up yet**. Everything Mac-side compiles and passes tests in CI (`macos-latest`), but none of it has run against real data yet:
- EventKit and PhotoKit
- AppleScript sending and note edits
- AOSKit anisette
- iCloud and GSA sign-in for Find My
- real BLE advertising (needs `sudo`)

When the owner gets Mac access, follow `docs/MAC_SETUP.md` with them and fix what breaks. I'd expect Find My sign-in and the Calendar/Photos permission prompts to break first. Logs are in `~/Library/Logs/bbd.log`.

Verified end to end on Linux against the mock server:
- pairing (pinned TLS, single-use codes)
- every client tab
- autofill over HTTP and server events, including clipboard restore on SIGTERM
- message notifications and per-chat mute
- beacon registration

## Layout

```
crates/proto     wire protocol: postcard frames, Backend trait (PROTOCOL_VERSION = 2)
crates/net       self-signed TLS + SHA-256 pinning, pairing URIs, framed WebSocket
crates/server    bbd: pairing/devices, sessions, --mock backend (runs anywhere)
crates/mac       macOS data: chat.db, NoteStore.sqlite, Voice Memos, EventKit, PhotoKit, AOSKit, Find My aggregation
crates/findmy    Offline-Finding keys/advertisement/report crypto, Apple SRP, GSA + iCloud web auth, beacon registry
crates/autofill  detect (port), IMAP (rustls), HTTP endpoint, Deliverer + per-desktop Platform
crates/client    current egui app `blackbubbles` (GUI, daemon, pair, beacon-setup), theme.rs/kit.rs, notify.rs
crates/beacon    bb-beacon: raw HCI LE advertising (legacy → extended fallback), key rotation
packaging/       launchd agent + install script, .app script, systemd units, PKGBUILD
docs/            MAC_SETUP.md, LINUX_SETUP.md
```

## Build, test, run

```sh
cargo build --workspace
cargo test --workspace                       # ~45 tests; all must pass
cargo run -p bb-server -- --data-dir /tmp/bbd pair --host 127.0.0.1 --port 17878
cargo run -p bb-server -- --data-dir /tmp/bbd run --mock --mock-codes 20 --bind 127.0.0.1:17878
BLACKBUBBLES_CONFIG_DIR=/tmp/bbc cargo run -p bb-client -- pair 'blackbubbles://…'
BLACKBUBBLES_CONFIG_DIR=/tmp/bbc cargo run -p bb-client
```

- **Mock server:** `--mock` serves fake chats, events, notes, memos and photos (generated BMP/WAV), plus Find My devices including a beacon. `--mock-codes N` sends a fake verification-code text every N seconds.
- **Developer environment variables for the client:**
  - `BLACKBUBBLES_CONFIG_DIR`: use a scratch config.
  - `BLACKBUBBLES_TAB=<name>`: the tab to open on.
  - `BLACKBUBBLES_OPEN_FIRST_CHAT=1`: open the first chat.
  - `BLACKBUBBLES_FORCE_WIDTH=<px>`: test responsive layouts on a tiling window manager.
- **macOS type-check from Linux:** the system Rust has no rustup. Install rustup into a scratch dir and add `aarch64-apple-darwin`. Point `CC_aarch64_apple_darwin` at a stub script that only creates the `-o` output file. Then run `cargo check --target aarch64-apple-darwin --workspace`. CI's macOS job is the real build-and-link check.
- **Screenshots (owner's desktop is Hyprland):** get the window geometry from `hyprctl clients -j`, then capture with `grim -g "<x>,<y> <w>x<h>"`. The Hyprland config is Lua, so `hyprctl dispatch` uses `hl.dsp.*` syntax.
- **Clipboard caution:** autofill tests touch the owner's real Wayland clipboard. Save it with `wl-paste` first and confirm it was restored afterwards.

## Design: the "Neutral" theme

The source of truth is the header of `~/.config/hypr/hyprland.lua`. Re-read it before UI work. Summary:
- **Palette:**
  - Surfaces: bg `#0a0a0a`, surface `#171717`, raised `#212121`.
  - Borders: border `#262626`, border-strong `#404040`.
  - Text: fg `#fafafa`, muted `#a1a1a1`, subtle `#737373`.
  - Primary: `#fafafa` on `#171717`.
  - Semantic: danger `#e5787a`, success `#8ccf9a`, warning `#e6c879`, info `#82a9e0`, magenta `#b49ae6`, cyan `#7cc6cf`.
- **Colour:** monochrome first. Semantic colours only for state (dots, badges), never large fills.
- **Layering:** layering is lightness, so a darker surface never sits on a lighter one. Every card has a 1px border; focus steps up to border-strong, never a coloured border.
- **Shape:** cards 16px, inner controls 10px, buttons/badges fully rounded. **One inverted white primary per surface**; everything else is ghost or outline.
- **Type:** Inter for UI, JetBrainsMono Nerd Font for mono. Headings semibold fg, body muted.
- **Spacing:** 4px grid, 8 inner gap, 16 outer gap and card padding.
- **Motion:** enter decelerating at ~250–350ms, exit accelerating at about half that, scale changes within 94–100%.
- **Depth:** soft, large, offset-down black shadows, not glow.
- **Icons:** Lucide, the set shadcn uses. The fonts are bundled in `crates/client/assets/fonts` with their licences. In a web UI, use `lucide` and Inter directly.

## Client features (keep when porting)

Navigation and layout:
- **Rail:** collapses to icons below about 1040px. The active tab is the primary pill; there's a connection status dot.
- **Responsive breakpoints:**
  - Below 760px: Messages, Notes and Find My go single-pane with a back button.
  - Below 980px: Calendar stacks its agenda under the month, and cells show dots only.
  - Photos: the grid is justified to fill the width.

Screens:
- **Messages:**
  - Chat list with avatars, search and time.
  - Thread grouped by day, with monochrome bubbles: yours are inverted, SMS you sent are outlined, received ones sit on raised.
  - "Load earlier", images inline, other files download.
  - Drag-and-drop to send.
  - Header badges show the service and `my_address` (email flagged), plus a per-chat mute bell.
- **Photos:** album pills, lazy thumbnails with an LRU cache, video and favourite badges, lightbox with arrow keys and Save.
- **Notes:** folder chips, search, plain-text editor (first line is the title), Ctrl/⌘+S, "Edited/Saved" badge, New note.
- **Calendar:** month grid with today as a pill, day agenda, delete, new-event modal (title, location, all day, times, calendar).
- **Voice Memos:** search, play, seek, save; the player card's primary is play/pause.
- **Find My:** device list (battery coloured only when low, beacon badge), OSM map with monochrome pins, refresh every 60 s.
- **Settings:** sections for Connection/pairing, Sending address help, Notifications (all categories, quiet hours, mute list), Autofill (sources, silent mode, Shortcut token, Gmail), Find My for this PC (Linux), About.

Behaviour:
- **Toasts:** bottom-right cards.
- **Primary process:** whichever process holds `primary.lock` in the config dir does autofill and notifications. That's `blackbubbles daemon` if it's running, otherwise the window. The daemon reloads notification settings when the config file changes.

## Working agreements

- **Branch and commits:** commit to `main` and push. CI must be green on Linux and macOS. End commit messages with the `Co-Authored-By` trailer given in the session.
- **Testing:** test logic with fixtures on Linux. Mock anything Apple-side through `bbd --mock`.
- **Check APIs:** read the crate source in `~/.cargo/registry` before using new crate versions. egui 0.37 changed panels (`egui::Panel`) and `App::ui`; rodio 0.23 uses `DeviceSinkBuilder` and `Player`.
- **Reporting:** be explicit about what was verified versus only compiled. The owner can't test Mac features yet.
- **Questions:** ask before large or irreversible choices, such as the frontend stack or deleting the egui client.
