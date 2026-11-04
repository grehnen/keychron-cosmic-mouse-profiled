# mouse-profiled

Automatically switches your Keychron G6 HE 8K mouse to another profile depending on which app has focus on the COSMIC desktop.

- `mouse-profiled` — watches the focused window and switches the profile per `apps.conf`.
- `mouse-profile N` — switches to profile N (0 = first). Handy for keyboard shortcuts.
- `mouse-profile configure` — creates or updates `apps.conf` (profile count, which devices to switch).
- `mouse-profile devices` — lists the connected Keychron mice it can switch.

## Install

Requires `cargo` ([rustup.rs](https://rustup.rs)).

```sh
./install.sh
```

It builds and installs both binaries, adds a udev rule for mouse access (asks for sudo), runs `mouse-profile configure`, and starts the `mouse-profiled` user service. Safe to re-run; `./install.sh --uninstall` removes everything except `apps.conf`.

<details>
<summary>Manual install</summary>

The udev rule must be numbered below 73; replug the mouse/receiver after adding it.

```sh
echo 'KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3434", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/70-keychron.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

cargo install --path . --locked
mouse-profile configure

mkdir -p ~/.config/systemd/user
cp systemd/mouse-profiled.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now mouse-profiled
```

</details>

## Configure

Edit `~/.config/mouse-profile/apps.conf`. Changes apply on the next focus change.

```ini
profiles = 2         # number of profiles ("Levels" in the launcher, 1–5; default 2)
device = 3434:d05a   # only switch this device (repeatable; omit to switch every Keychron mouse)
default = 0          # profile for every app not listed

steam_app_730 = 1    # app_id = profile index
```

The mouse may have a different USB ID wired than through the receiver; `mouse-profile configure` lists both and writes the `device` lines for you.

To find an app ID, run `journalctl --user -u mouse-profiled -f` and click between windows: each focus change prints `focus: <app_id> -> profile N`. Steam games usually show up as `steam_app_<appid>`.

## How it works

The profile command was captured from the Keychron web launcher's WebHID traffic:

- It goes to the vendor HID interface (usage page `0xFFC1`), found by scanning `/sys/class/hidraw/*/device/report_descriptor`.
- Output report `0xB5`, 20-byte payload `0E <profile_index> <profile_count> 00…`.
- The focused window comes from COSMIC's `ext-foreign-toplevel-list` + `zcosmic-toplevel-info` protocols.

If a firmware update breaks switching, re-capture it: hook `HIDDevice.prototype.sendReport` in the launcher's DevTools console and switch profiles.

## Compatibility

Tested on the G6 HE 8K, through the TurboLink 8K receiver and wired. Other Keychron mice with the `0xFFC1` interface probably work if their launcher sends the same `B5 0E <index> <count>` command. Without `device` lines, every such device gets the command, so add them if you have other Keychron gear.

## Troubleshooting

- **`Permission denied` / launcher stuck loading:** `getfacl /dev/hidrawN` should show `user:<you>:rw-`. If not, the udev rule is missing or numbered ≥ 73.
- **`no matching Keychron mouse connected`:** check `mouse-profile devices`, and that any `device` lines match its IDs.
- **Profile doesn't change for an app:** the app ID in the journal must match the key in `apps.conf` exactly.
- **Daemon not running:** `systemctl --user status mouse-profiled`.
