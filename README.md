# BlackBubbles

A fast, native bridge to your Apple ecosystem for **Linux and macOS** desktops, built in Rust: an [egui](https://github.com/emilk/egui) client plus a small daemon on your Mac.

> Status: early development. Nothing works yet.

## Features (planned)

- **Messages**: iMessage and SMS, read and send, using the Mac's `chat.db` and AppleScript.
- **Photos**: browse your library through PhotoKit.
- **Notes**: read and edit Apple Notes.
- **Calendar**: events and reminders through EventKit.
- **Voice Memos**: list and play recordings.
- **Find My**: your Apple devices, plus the Linux machine itself. A Bluetooth Low Energy (BLE) Offline-Finding beacon makes it findable by nearby Apple devices. Its location shows on BlackBubbles' map, because Apple's own Find My app can't list non-Apple hardware.
- **Built-in autofill**: a port of [linuxautofill](https://github.com/craftyderpysmall/linuxautofill). It reads verification codes and links from iMessage/SMS (via the Mac), Gmail IMAP, or an iPhone Shortcut, and puts them on the clipboard with click-to-paste. Works on Wayland, X11 and macOS.

## Layout

```
crates/proto     shared wire protocol
crates/server    macOS daemon
crates/client    egui desktop app (macOS + Linux)
crates/autofill  verification code/link autofill
crates/beacon    Linux BLE Offline-Finding advertiser
packaging/       launchd, systemd, PKGBUILD, .app
```

## Caveats

Find My support depends on undocumented Apple endpoints, which may break at any time. It is implemented clean-room from public research.

## License

MIT
