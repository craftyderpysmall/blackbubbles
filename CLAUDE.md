# BlackBubbles: notes for Claude and the owner

BlackBubbles is a fast bridge from the owner's Apple world to **Linux and macOS** desktops. It does **not** use BlueBubbles: it has its own Rust server (`bbd`) on the Mac, its own protocol, and its own client. The repository is public (MIT): github.com/craftyderpysmall/blackbubbles.

## Client: Tauri v2 + React (migrated 2026-11-06)

The owner chose **React + Vite + Tailwind 4 + shadcn-style components (Radix)** and to **delete the egui client**; both are done. The egui code is only in git history (commit `036d7b4`). Why Tauri: egui couldn't render colour emoji; the webview also gives real text selection, CSS-accurate Neutral styling and system font fallback.

How it fits together:
- **Rust side** (`crates/client`, binary `blackbubbles`): same CLI modes as before (`daemon`, `pair`, `beacon-setup`; no subcommand opens the window). `commands.rs` holds `AppState` and every Tauri command.
- **Requests:** the frontend calls the `request` command with proto JSON (serde's externally tagged enums, e.g. `{"ListChats":{"limit":300,"before":null}}`; unit variants are bare strings like `"ListCalendars"`). Types are mirrored by hand in `ui/src/lib/bb.ts`; **keep them in sync with `crates/proto`**.
- **Events:** server `Event`s are emitted as `server-event` and connection changes as `status`. `ui/src/lib/events.ts` applies them straight to the React Query cache, so nothing polls.
- **Media:** the `bb://` scheme (`blob.rs`) serves `blob/<Kind>/<id>` from chunked `GetBlob` with a 160 MB LRU and a 24-request cap, and proxies OpenFreeMap (`map/<path>`: vector tiles, style, fonts, sprites) with a proper User-Agent and a disk cache (tiles 7 days, style JSON 1 day; stale copies serve offline). MapLibre's `transformRequest` rewrites `tiles.openfreemap.org` URLs to it. The frontend builds URLs with `convertFileSrc(path, "bb")`.
- **Uploads:** attachments go to `send_attachment` as a raw-body invoke (`x-chat`/`x-name` headers). The window sets `dragDropEnabled: false` so HTML5 drag-and-drop gives File objects.
- **Audio:** voice memos play through rodio on its own thread (`audio.rs`), polled by the UI 4×/s while loaded. This avoids depending on GStreamer codecs in WebKitGTK.
- **Emoji:** the font stack prefers a complete system colour emoji font (Apple Color Emoji; Noto Color Emoji is a Linux package dependency). The bundled `@fontsource/noto-color-emoji` subsets are renamed "BlackBubbles Emoji" and only a fallback, because split subsets can't shape emoji + U+FE0F (✈️ ❤️ drew boxes). The mock's reminder "Flight to Lisbon ✈️" is the regression check.
- **Fonts:** Inter, JetBrains Mono and Noto Color Emoji come from `@fontsource` (licences in `ui/licenses/`) (imported in `main.tsx`, not CSS, so the `woff2Only` Vite plugin can drop the `.woff` fallbacks). The emoji font is split by unicode-range, so only used glyphs load.
- **Reconnecting:** `Client::close()` stops the old connection task and the notifier's poll loop. (The egui client leaked both after re-pairing.)

Possible next steps (ask first): a tray icon, a "new conversation" composer, typed proto bindings (e.g. `ts-rs`) instead of the hand-written mirror, and rich note bodies (the protocol only carries plain text today).

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
- **Apple's SQLite stores** are opened with `util::open_readonly`. If a WAL database's `-shm` is gone (the app isn't running), a plain read-only open fails ("unable to open database file"; the owner hit this with Voice Memos). It then falls back to `immutable=1`, which is a snapshot, so callers must not cache that connection.
- **Secrets** live in mode-600 files. The client config is refused if group/other can read it.

## Status

As of 2026-11-06, the owner has a Mac but **hasn't been able to set it up yet**. Everything Mac-side compiles and passes tests in CI (`macos-latest`), but none of it has run against real data yet:
- EventKit and PhotoKit
- AppleScript sending and note edits
- AOSKit anisette
- iCloud and GSA sign-in for Find My. **Owner hit this:** the 2FA pop-up never arrived, and their iOS has no "Get Verification Code" option. `findmy-login` now offers `sms` at both code prompts. When Apple's reply lists no numbers, it uses phone id 1, as pypush does (iCloud: `PUT /appleauth/auth/verify/phone`; GSA: `PUT gsa.apple.com/auth/verify/phone/`), and texts automatically when Apple reports `noTrustedDevices`. Untested against Apple.
- real BLE advertising (needs `sudo`)

When the owner gets Mac access, follow `docs/MAC_SETUP.md` with them and fix what breaks. I'd expect Find My sign-in and the Calendar/Photos permission prompts to break first. Logs are in `~/Library/Logs/bbd.log`.

Verified on Linux against the mock server, **with the Tauri client** (2026-11-06):
- pairing from the CLI and from Settings; re-pairing closes the old connection
- every tab renders, including colour emoji (flags, skin tones, ZWJ, ✈️-style VS16) and the bb:// thumbnails and the vector map
- sending a text: optimistic bubble, then the server echo, and the chat list reorders
- note edit and Ctrl+S save; calendar new-event dialog; photo lightbox with arrow keys
- voice memo play/pause/progress
- the per-chat mute bell (writes `muted_chats`)
- Reminders (protocol 3, mock): lists and smart lists, tick off, quick add, creating one from the Calendar dialog, reminders on calendar days
- locked notes: lock in the list, read-only panel; the server-side edit guard is unit-tested
- Services: hiding Photos removes its tab and writes `[services]`
- Find My: OpenFreeMap map through the proxy, floating list, selecting a device (marker, ring, detail card)
- settings write the mode-600 config
- desktop notifications from the window process; the window becomes secondary while `blackbubbles daemon` holds `primary.lock`
- responsive layouts at 700px (single pane) and 900px (stacked calendar, dot cells)

Verified only with the old egui client; the Rust code is unchanged, but these haven't been re-clicked in the Tauri UI:
- autofill over HTTP and server events, including clipboard restore on SIGTERM
- beacon registration

Not exercised yet:
- the Mac's EventKit Reminders code (`crates/mac/src/reminders.rs`): type-checked for macOS, never run
- Reminders-due desktop notifications (the timing rule is unit-tested, not observed on screen)

Not exercised yet in the Tauri UI:
- attachment send (button and drag-and-drop)
- Save to Downloads
- event delete
- Gmail add/remove
- the Tauri client on macOS (CI builds it, nobody has run it)

## Layout

```
crates/proto     wire protocol: postcard frames, Backend trait (PROTOCOL_VERSION = 4; ListContacts added)
crates/net       self-signed TLS + SHA-256 pinning, pairing URIs, framed WebSocket
crates/server    bbd: pairing/devices, sessions, --mock backend (runs anywhere)
crates/mac       macOS data: chat.db, NoteStore.sqlite, Voice Memos, EventKit, PhotoKit, AOSKit, Find My aggregation
crates/findmy    Offline-Finding keys/advertisement/report crypto, Apple SRP, GSA + iCloud web auth, beacon registry
crates/autofill  detect (port), IMAP (rustls), HTTP endpoint, Deliverer + per-desktop Platform
crates/client    Tauri app `blackbubbles`: commands.rs (bridge), blob.rs (bb:// scheme), audio.rs, conn.rs, notify.rs, config.rs; tauri.conf.json, capabilities/, icons/
ui/              React UI: src/lib (bb.ts bridge, events.ts, app.tsx context, format.ts), src/components (ui.tsx kit, Lightbox, Linkified), src/pages/*
crates/beacon    bb-beacon: raw HCI LE advertising (legacy → extended fallback), key rotation
packaging/       launchd agent + install script, .app script, systemd units, PKGBUILD
docs/            MAC_SETUP.md, LINUX_SETUP.md
```

## Build, test, run

```sh
(cd ui && npm ci && npm run build)          # type-check + bundle the UI into ui/dist
cargo build --workspace --features bb-client/custom-protocol
cargo test --workspace                       # ~48 tests; all must pass
cargo run -p bb-server -- --data-dir /tmp/bbd pair --host 127.0.0.1 --port 17878
cargo run -p bb-server -- --data-dir /tmp/bbd run --mock --mock-codes 20 --bind 127.0.0.1:17878
BLACKBUBBLES_CONFIG_DIR=/tmp/bbc cargo run -p bb-client -- pair 'blackbubbles://…'
BLACKBUBBLES_CONFIG_DIR=/tmp/bbc cargo run -p bb-client --features custom-protocol
```

- **custom-protocol:** without this feature, Tauri runs in dev mode and loads `http://localhost:5173`. For hot reload, run `npm run dev` in `ui/` and `cargo run -p bb-client`. Switching the feature on or off rebuilds the tauri crates, so pick one mode per session. Release builds and CI always use the feature.
- **Bundles:** run `npm run tauri -- build --bundles deb|app` from `ui/`. The script `cd`s to the repo root, because that's where the CLI finds `crates/client/tauri.conf.json`. `packaging/macos/make-app.sh` wraps the `app` bundle.
- **Mock server:** `--mock` serves fake chats, events, notes, memos and photos (generated BMP/WAV), plus Find My devices including a beacon. Writes echo events like the Mac does: a sent message comes back as `NewMessage` after 0.5 s, and note and calendar writes emit their `*Changed` events. `--mock-codes N` sends a fake verification-code text every N seconds.
- **Developer environment variables for the client:**
  - `BLACKBUBBLES_CONFIG_DIR`: use a scratch config.
  - `BLACKBUBBLES_TAB=<messages|photos|notes|calendar|voice|findmy|settings>`: the tab to open on.
  - `BLACKBUBBLES_OPEN_FIRST_CHAT=1`: open the first chat.
  - `BLACKBUBBLES_FORCE_WIDTH=<px>`: caps the UI root's width, to test responsive layouts on a tiling window manager.
- **macOS type-check from Linux:** the system Rust has no rustup. Install rustup into a scratch dir and add `aarch64-apple-darwin`. Point `CC_aarch64_apple_darwin` at a stub script that only creates the `-o` output file. Then run `cargo check --target aarch64-apple-darwin --workspace`. CI's macOS job is the real build-and-link check.
- **Screenshots (owner's desktop is Hyprland):** get the window geometry from `hyprctl clients -j` (class `blackbubbles`), then capture with `grim -g "<x>,<y> <w>x<h>"`. The Hyprland config is Lua, so `hyprctl dispatch` uses `hl.dsp.*` syntax.
- **Driving the UI:** there's no click injection (no ydotool), but `wtype` sends keys to the focused window. **Check `hyprctl activewindow` is `blackbubbles` before every `wtype`.** Tab moves through the rail (9 stops: 8 tabs + status), then the page. The map's markers and attribution links come before the floating Find My list (the first device row is stop 22). The chat composer and the note editor autofocus.
- **Don't** `pkill -f` a pattern that appears in your own command line (it kills the shell). `pkill -x blackbubbles` also kills a running `blackbubbles daemon`.
- **Clipboard caution:** autofill tests touch the owner's real Wayland clipboard. Save it with `wl-paste` first and confirm it was restored afterwards. For UI tests, set `[autofill] enabled = false` in the scratch config.
- **Sound:** mock voice memos are tones. Mute with `pactl set-sink-mute @DEFAULT_SINK@ 1` while testing playback, and unmute afterwards.
- **Notifications:** mako only moves a notification to `makoctl history` once it's dismissed. Use `makoctl list` for the ones still showing, and dismiss only your test notifications by ID.

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
- **Icons:** Lucide (`lucide-react`), the set shadcn uses.
- **Implementation:** the tokens are Tailwind `@theme` values in `ui/src/index.css` (`bg-surface`, `rounded-card`, `ease-decel`, `animate-enter`…). The kit is `ui/src/components/ui.tsx` (Button variants primary/ghost/outline/danger, Input, Switch, Badge, Chip, Card, Dialog…). Use them; don't add ad-hoc colours. The decel/accel curves are the exact beziers from `hyprland.lua`.

## Client features (all ported; keep them working)

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
  - Group events ("You left the conversation", "Alex added Anthony", renames, group photo) are centred lines in the thread and the chat-list preview. chat.db `item_type` 1/2/3 + `group_action_type` → `Message.group_event` (protocol 4). Other item types (location sharing, FaceTime…) are skipped. The mock has a "Weekend trip" group with each kind.
  - Contact names: `bbd` reads `~/Library/Application Support/AddressBook/{,Sources/*/}AddressBook-v22.abcddb` (Full Disk Access covers it) and answers `ListContacts` with handle → name for every handle in chat.db. Matching uses lowercase emails and the last 10 digits of phone numbers (`crates/mac/src/contacts.rs`). The UI's `useNames()` applies them to titles, avatars, sender labels and group events; the notifier too.
  - Save As…: `save_blob_as` opens a native dialog (tauri-plugin-dialog, Rust side, so no JS permission). Attachments have a right-click menu (Open, Save As…). The web view's own image menu is suppressed, because its "Save Image As…" can't save bb:// images.
  - Text selection app-wide is white with black text; inside white surfaces (your iMessage bubbles, `.bb-inverted`) it flips to black with white text.
- **Photos:** album pills, lazy thumbnails with an LRU cache, video and favourite badges, lightbox with arrow keys and Save.
- **Notes:** folder chips, search, plain-text editor (first line is the title), Ctrl/⌘+S, "Edited/Saved" badge, New note. Locked (password-protected) notes show a lock and a read-only panel; their bodies are encrypted with the owner's Notes password, so `bbd` never decrypts them and refuses `UpdateNote` on them (`NoteSummary.locked`).
- **Calendar:** month grid with today as a pill, day agenda, delete. Reminders with a due date show on their day as hollow rings (events are dots) and in the agenda with a checkbox. The "New" dialog switches between Event (title, location, notes, all day, times, calendar) and Reminder (title, notes, date, optional time, list, priority).
- **Reminders:** Today (includes overdue) / Scheduled (grouped by day) / All (grouped by list) / Completed tiles with counts, the owner's lists with colours, quick add (title + optional date/time), tick off (stays visible ~1 s), delete, "Show completed". Completed history is only fetched when shown. Timed reminders get an EventKit alarm so they also alert on the iPhone.
- **Voice Memos:** search, play, seek, save; the player card's primary is play/pause.
- **Find My:** full-bleed MapLibre vector map with OpenFreeMap's `dark` style (no API key; all requests go through the `bb://map/` cache proxy). The device list floats over the map as a translucent panel (hideable). Markers are round device-icon badges; the selected one is inverted with its name and an accuracy ring. A detail card shows last seen, accuracy, battery (coloured only when low), coordinates and "Open in OpenStreetMap". Refreshes every 60 s. CARTO's basemaps were tried and now need an API key; don't go back to inverting light OSM tiles (the owner found it forced).
- **Settings:** sections for Connection/pairing, Sending address help, Services (which Apple services show; all on by default; a hidden service loses its tab and notifications), Notifications (all categories incl. Reminders due, quiet hours, mute list; groups dim when their service is hidden), Autofill (sources, silent mode, Shortcut token, Gmail), Find My for this PC (Linux), About.

Behaviour:
- **Toasts:** bottom-right cards.
- **Primary process:** whichever process holds `primary.lock` in the config dir does autofill and notifications. That's `blackbubbles daemon` if it's running, otherwise the window. The daemon reloads notification settings when the config file changes.

## Working agreements

- **Branch and commits:** commit to `main` and push. CI must be green on Linux and macOS. End commit messages with the `Co-Authored-By` trailer given in the session.
- **Testing:** test logic with fixtures on Linux. Mock anything Apple-side through `bbd --mock`.
- **Check APIs:** read the crate source in `~/.cargo/registry` (or `ui/node_modules`) before using new versions.
  - Tauri is 2.12; 3.0 is in alpha, so stay on 2.
  - rodio 0.23 uses `DeviceSinkBuilder` and `Player`. Its `wav` feature is needed for PCM; `symphonia-wav` alone can't decode it.
  - JS dependencies are pinned exactly in `ui/package.json` with a lockfile. CI runs `npm ci`.
- **Reporting:** be explicit about what was verified versus only compiled. The owner can't test Mac features yet.
- **Questions:** ask before large or irreversible choices, such as the frontend stack or deleting the egui client.
