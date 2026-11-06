# Setting up Linux

## Install

On Arch, using the included PKGBUILD:

```sh
git clone https://github.com/craftyderpysmall/blackbubbles
cd blackbubbles/packaging/linux && makepkg -si
```

From source on any distro:

```sh
sudo pacman -S --needed rust dbus alsa-lib libxkbcommon wayland wl-clipboard wtype   # Arch
# Debian/Ubuntu: sudo apt install cargo libdbus-1-dev libasound2-dev libxkbcommon-dev libwayland-dev wl-clipboard wtype
cargo build --release -p bb-client -p bb-beacon
sudo install -m755 target/release/blackbubbles target/release/bb-beacon /usr/bin/
```

## Pair

Run `bbd pair` on the Mac (see [MAC_SETUP.md](MAC_SETUP.md)), then either:

```sh
blackbubbles pair 'blackbubbles://…'
```

or open BlackBubbles → Settings, paste the string, and click Pair.

## Autofill

Autofill is on by default for iMessage/SMS codes. Configure the other sources in Settings, or in `~/.config/blackbubbles/config.toml`, which must be mode 600:

```toml
[autofill]
clear_after = 60            # seconds until the clipboard is restored
extra_keywords = ["clave"]  # extra words that mark a code

[autofill.http]             # iPhone Shortcut endpoint
port = 8787
token = "…"                 # Settings → "Enable with a new token" generates one

[[autofill.gmail]]
address = "you@gmail.com"
app_password = "abcd efgh ijkl mnop"
```

The `[autofill]` section uses the same keys as linuxautofill's config, so you can move your settings over and uninstall linuxautofill.

**iPhone Shortcut:** in the Shortcuts app, create an automation:
- Trigger: *Message* → *Message Contains* a single space → *Run Immediately*.
- Action: *Get Contents of URL* → `http://<this-pc>.local:8787/message`, method POST.
- Header: `Authorization: Bearer <token>`.
- Body: JSON `{"text": Shortcut Input}`.
- Open the port to your LAN (for example `sudo ufw allow from 192.168.0.0/16 to any port 8787`).

**Keep autofill running when the window is closed:**

```sh
systemctl --user enable --now blackbubbles   # runs `blackbubbles daemon`
```

Only one process autofills at a time: if the daemon is running, the window leaves autofill to it.

**Click to paste** needs `wtype` (Hyprland, Sway, KDE) or `ydotool` with `ydotoold` running (GNOME). Without either, the code is still copied; you paste it yourself.

## Make this PC findable

1. BlackBubbles → Settings → **Make this PC findable**, or run `blackbubbles beacon-setup`. This generates 96 rotating keys, gives the private halves to your Mac, and writes the public halves to `~/.config/blackbubbles/beacon-public.json`.
2. Start the advertiser. It needs Bluetooth on and raw HCI access, which the unit grants through capabilities rather than root:
   ```sh
   sudo systemctl enable --now blackbubbles-beacon@$USER
   journalctl -u blackbubbles-beacon@$USER -f     # "advertising XX:XX:… (legacy|extended)"
   ```
3. Make sure the Mac has run `bbd findmy-login … --beacons`.

Reports show up after an Apple device has been near the PC, usually within minutes in a populated area. The PC then appears as **This PC** in the Find My tab.

To check what is being broadcast without root, run `bb-beacon show --keys ~/.config/blackbubbles/beacon-public.json`.
