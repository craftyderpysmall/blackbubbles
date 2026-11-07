# BlackBubbles

Your Apple world on **Linux and macOS** desktops: Messages, Photos, Notes, Calendar, Voice Memos and Find My. It is a [Tauri](https://tauri.app) app (Rust core, React UI on the system webview) plus a small Rust daemon on your Mac.

It is not based on BlueBubbles. BlackBubbles has its own server, protocol and client.

> **Status:** early, and in daily use on one Mac. Messages (with contact names), Notes and Find My run against real data; Photos, Calendar and Reminders are tested against the mock server but not yet on a real Mac. Expect rough edges.

## Features

| | What you get | How it works on the Mac |
|---|---|---|
| **Messages** | iMessage and SMS chats with contact names and group events, history, sending text and files (drag & drop), images inline, Save As | Reads `chat.db` directly and sends through AppleScript |
| **Photos** | Your library and albums in a fast thumbnail grid, full-size viewer, save to disk | PhotoKit, rendered to JPEG (no HEIC on the client) |
| **Notes** | Folders, search, read, edit, new and delete. Password-locked notes open read-only with your Notes password (passcode-locked ones can't) | Reads `NoteStore.sqlite` and decrypts locked notes locally; edits go through AppleScript |
| **Calendar** | Month grid, agenda, create and delete events; reminders with a date show on their day | EventKit |
| **Reminders** | Today / Scheduled / All / Completed, your lists, add, tick off, delete; timed reminders alert on your iPhone too | EventKit (the Reminders app's store) |
| **Voice Memos** | List, play, seek, save | Recordings database and audio files |
| **Find My** | Your Apple devices on a map, optionally with your family's | iCloud Find My |
| **Autofill** | Verification codes and links go straight to your clipboard | Built-in [linuxautofill](https://github.com/craftyderpysmall/linuxautofill), see below |

### Services

Settings → Services chooses which of Messages, Photos, Notes, Calendar, Reminders, Voice Memos and Find My BlackBubbles shows (all on by default). A hidden service loses its tab and its notifications; autofill is unaffected.

### Notifications

Settings → Notifications decides exactly what reaches your desktop:
- **Messages:** iMessage and SMS separately, with or without the sender and a preview. You can skip code-only texts (autofill already shows those), stay quiet while the window is focused, and mute single conversations from their header.
- **Other alerts:** calendar reminders N minutes before an event, Reminders when they're due, low battery on any Find My device, new photos, notes or voice memos, and the Mac connection dropping or coming back.
- **Quiet hours:** for example 22:00–07:00.

Notifications come from whichever BlackBubbles process owns autofill (the background daemon if it runs, otherwise the window). The daemon picks up setting changes live.

### Autofill (linuxautofill, built in)

This is a faithful Rust port of linuxautofill's detection, and its whole test suite passes unchanged. It also keeps linuxautofill's delivery rules:
- A repeated code is ignored for 120 seconds.
- Your previous clipboard is saved, unless a password manager put it there, and restored afterwards.
- Nothing is typed or opened unless you click the notification.

Codes come from three places:
1. **Every iMessage/SMS the Mac receives.** No iPhone Shortcut is needed if Text Message Forwarding is on.
2. **Gmail over IMAP.** It checks every 0.5 seconds, doesn't mark mail as read, and also finds verification links.
3. **The iPhone Shortcut endpoint** (`POST /message`, same contract as linuxautofill). Use it when the Mac is offline.

| Desktop | Clipboard | Click to paste | Notification |
|---|---|---|---|
| Wayland (Hyprland, Sway, KDE, GNOME) | `wl-copy`/`wl-paste` | `wtype`, or `ydotool` on GNOME | freedesktop, with actions |
| X11 | native | XTest | freedesktop, with actions |
| macOS | NSPasteboard | the code stays on the clipboard; press ⌘V | Notification Center |

## Design

The UI follows the "Neutral" desktop theme:
- **Look:** monochrome dark and shadcn-style. Colour is used only for state (status dots, low battery, email-address warnings).
- **Shape:** 16 px cards, 10 px controls and pill buttons, with one inverted white primary action per surface.
- **Type and icons:** Inter and Lucide icons are bundled (OFL and ISC licences). Emoji render in colour everywhere, including flags, skin tones and ZWJ sequences, using the system colour emoji font (Apple Color Emoji, or Noto Color Emoji, a Linux package dependency). A bundled Noto subset is the fallback.
- **Motion:** quick decelerating enter and accelerating exit animations.
- **Responsive:** the rail collapses to icons, and Messages, Notes and Find My go single-pane on narrow windows. Calendar stacks its agenda under the month and shrinks events to dots.

## Layout

```
crates/proto     wire protocol (postcard over WebSocket)
crates/net       pinned self-signed TLS, pairing strings
crates/server    bbd, the daemon on your Mac (--mock runs anywhere)
crates/mac       macOS data access: chat.db, NoteStore, Voice Memos, EventKit, PhotoKit, AOSKit
crates/findmy    iCloud Find My sign-in (Apple SRP) and device locations
crates/autofill  linuxautofill port: detection, IMAP, HTTP, per-desktop delivery
crates/client    the Tauri app's Rust side (blackbubbles: window, daemon, pair)
ui/              the React + Tailwind UI (Vite)
packaging/       launchd agent, .app bundling, systemd units, PKGBUILD
```

## Getting started

1. **Mac:** follow [docs/MAC_SETUP.md](docs/MAC_SETUP.md).
2. **Linux:** follow [docs/LINUX_SETUP.md](docs/LINUX_SETUP.md). To reach the Mac by its `.local` name, Linux needs `nss-mdns`; that guide covers it.
3. **macOS client:** install Node.js, run `packaging/macos/make-app.sh` and open the resulting `BlackBubbles.app`.

You can try the client without a Mac by running the mock server:

```sh
cargo run -p bb-server -- pair --host 127.0.0.1      # prints a pairing string
cargo run -p bb-server -- run --mock --mock-codes 20 # fake data + a fake code text every 20 s
(cd ui && npm ci && npm run build)
cargo run -p bb-client --features custom-protocol    # Settings → paste the string → Pair
```

## Security

- **Connection:** the client pins the server's self-signed certificate (SHA-256) from the pairing string, so no CA is trusted.
- **Pairing codes:** they expire after 10 minutes, work once, and are compared in constant time.
- **Device tokens:** the server stores only their hashes. `bbd devices` lists paired devices and `bbd revoke` removes one.
- **Secrets:** the client config holds secrets and must be mode 600, as with linuxautofill. Server secrets are stored the same way.
- **Remote access:** use [Tailscale](https://tailscale.com) or another VPN rather than port forwarding.

## Caveats

- **Find My** uses undocumented Apple endpoints, implemented clean-room from public research (OpenHaystack, macless-haystack). It can break whenever Apple changes them.
- **Messages** are sent through AppleScript, so tapbacks, edits and typing indicators aren't supported.

## License

MIT
