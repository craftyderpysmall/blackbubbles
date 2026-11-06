# BlackBubbles

Your Apple world on **Linux and macOS** desktops: Messages, Photos, Notes, Calendar, Voice Memos and Find My. It is a native [egui](https://github.com/emilk/egui) app plus a small Rust daemon on your Mac.

It is not based on BlueBubbles. BlackBubbles has its own server, protocol and client.

> **Status:** early. The client, protocol and autofill are tested end to end against the mock server. The macOS server compiles and passes its tests in CI but hasn't yet run on a real Mac. Expect rough edges.

## Features

| | What you get | How it works on the Mac |
|---|---|---|
| **Messages** | iMessage and SMS chats, history, sending text and files (drag & drop), images inline | Reads `chat.db` directly and sends through AppleScript |
| **Photos** | Your library and albums in a fast thumbnail grid, full-size viewer, save to disk | PhotoKit, rendered to JPEG (no HEIC on the client) |
| **Notes** | Folders, search, read and edit, new notes | Reads `NoteStore.sqlite`; edits go through AppleScript |
| **Calendar** | Month grid, agenda, create and delete events | EventKit |
| **Voice Memos** | List, play, seek, save | Recordings database and audio files |
| **Find My** | Your Apple devices on a map, plus this Linux PC | iCloud Find My, and Offline-Finding reports for the beacon |
| **Autofill** | Verification codes and links go straight to your clipboard | Built-in [linuxautofill](https://github.com/craftyderpysmall/linuxautofill), see below |

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

### Find My for a Linux PC

Apple's Find My app only lists Apple hardware. BlackBubbles gets as close as it can:
- The Linux PC broadcasts an **Offline-Finding Bluetooth beacon**, the same protocol AirTags use, with keys that rotate every 15 minutes.
- Nearby iPhones, iPads and Macs encrypt their location to the beacon's key and upload it to Apple. This works even when the PC is offline or asleep with Bluetooth on.
- Your Mac fetches those reports and decrypts them.
- The PC then appears as **"This PC"** on the BlackBubbles map next to your Apple devices.

Only public keys reach the beacon service. Private keys live on your client and your Mac.

## Layout

```
crates/proto     wire protocol (postcard over WebSocket)
crates/net       pinned self-signed TLS, pairing strings
crates/server    bbd, the daemon on your Mac (--mock runs anywhere)
crates/mac       macOS data access: chat.db, NoteStore, Voice Memos, EventKit, PhotoKit, AOSKit
crates/findmy    Offline-Finding crypto, Apple SRP, iCloud and GSA auth
crates/autofill  linuxautofill port: detection, IMAP, HTTP, per-desktop delivery
crates/client    the egui app (blackbubbles)
crates/beacon    bb-beacon, the Linux BLE advertiser
packaging/       launchd agent, .app bundling, systemd units, PKGBUILD
```

## Getting started

1. **Mac:** follow [docs/MAC_SETUP.md](docs/MAC_SETUP.md).
2. **Linux:** follow [docs/LINUX_SETUP.md](docs/LINUX_SETUP.md).
3. **macOS client:** run `packaging/macos/make-app.sh` and open the resulting `BlackBubbles.app`.

You can try the client without a Mac by running the mock server:

```sh
cargo run -p bb-server -- pair --host 127.0.0.1      # prints a pairing string
cargo run -p bb-server -- run --mock --mock-codes 20 # fake data + a fake code text every 20 s
cargo run -p bb-client                               # Settings → paste the string → Pair
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
