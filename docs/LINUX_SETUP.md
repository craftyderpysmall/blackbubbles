# Setting up Linux

## Install

On Arch, using the included PKGBUILD:

```sh
git clone https://github.com/craftyderpysmall/blackbubbles
cd blackbubbles/packaging/linux && makepkg -si
```

From source on any distro:

```sh
sudo pacman -S --needed rust nodejs npm webkit2gtk-4.1 gtk3 libsoup3 noto-fonts-emoji dbus alsa-lib libxkbcommon wayland wl-clipboard wtype   # Arch
# Debian/Ubuntu: sudo apt install cargo nodejs npm libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev fonts-noto-color-emoji libdbus-1-dev libasound2-dev libxkbcommon-dev libwayland-dev wl-clipboard wtype
(cd ui && npm ci && npm run build)
cargo build --release -p bb-client --features custom-protocol   # embeds ui/dist
sudo install -m755 target/release/blackbubbles /usr/bin/
```

The window is a Tauri app on the system WebKitGTK. Install a colour emoji font (Noto Color Emoji above): it renders every emoji sequence. A bundled subset is only a fallback, and it can't draw emoji written with a variation selector such as ✈️.

## Let Linux find the Mac by name

`bbd pair` often prints the Mac's local name, like `my-mac.local`. Many Linux systems, Arch included, can't look those names up out of the box. BlackBubbles then fails with:

```
connecting to my-mac.local:7878: failed to lookup address information: Name or service not known
```

Fix it once (Arch):

```sh
sudo pacman -S --needed avahi nss-mdns
sudo systemctl enable --now avahi-daemon
sudo sed -i 's/^hosts: mymachines resolve/hosts: mymachines mdns_minimal [NOTFOUND=return] resolve/' /etc/nsswitch.conf
getent hosts my-mac.local      # should print the Mac's IP
```

On Debian/Ubuntu, `libnss-mdns` is normally installed already. If not, run `sudo apt install libnss-mdns`, which also edits `/etc/nsswitch.conf`.

If your `hosts:` line looks different from the one the `sed` command expects, add `mdns_minimal [NOTFOUND=return]` by hand, just before `resolve` (or before `dns`). Then restart BlackBubbles; you don't need to pair again.

**Other options:**
- **Use the Mac's IP address:** on the Mac, run `bbd pair --host 192.168.x.y` and pair with that string. This breaks if the router gives the Mac a different IP, so reserve one for it in your router's DHCP settings.
- **Use Tailscale on both machines:** pair with the Mac's Tailscale name or IP. This also works away from home.

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
