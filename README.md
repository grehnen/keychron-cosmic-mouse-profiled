# mouse-profiled

Switches the Keychron G6 HE 8K mouse profile automatically based on which app is focused (COSMIC desktop).

- `bin/mouse-profile N` — switches the mouse to profile N (0 = first, 1 = second, …).
- `mouse-profiled` (this Rust project) — watches the focused window and runs `mouse-profile` when the profile should change.

## Install

1. Let your user access the mouse (one-time, needs sudo). The rule file must be numbered below 73:

   ```sh
   echo 'KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3434", MODE="0660", TAG+="uaccess"' \
     | sudo tee /etc/udev/rules.d/70-keychron.rules
   sudo udevadm control --reload-rules && sudo udevadm trigger
   ```

   Then replug the mouse/receiver.

2. Put the switch script on your `PATH` and test it:

   ```sh
   ln -s "$PWD/bin/mouse-profile" ~/.local/bin/mouse-profile
   mouse-profile 1
   ```

3. Build and start the daemon:

   ```sh
   cargo install --path .
   systemctl --user daemon-reload
   systemctl --user enable --now mouse-profiled
   ```

   The service file is at `~/.config/systemd/user/mouse-profiled.service`.

## Configure

Edit `~/.config/mouse-profile/apps.conf`:

```ini
default = 0          # profile for every app not listed

steam_app_730 = 1    # app_id = profile index
```

Changes take effect on the next focus change; no restart needed.

### Finding app IDs

Follow the daemon log and click between windows:

```sh
journalctl --user -u mouse-profiled -f
```

Each focus change prints `focus: <app_id> -> profile N`. Copy the `<app_id>` into `apps.conf`. Steam games usually show up as `steam_app_<appid>`.

## How it works

The profile command was captured from the Keychron web launcher (launcher.keychron.com) by logging its WebHID traffic:

- Interface: the TurboLink receiver's vendor HID collection (usage page `0xFFC1`), found by scanning `/sys/class/hidraw/*/device/report_descriptor`.
- Output report ID `0xB5`, 20-byte payload: `0E <profile_index> <profile_count> 00…`.
- `<profile_count>` matches "Levels" in the launcher (default 2). Override with `MOUSE_PROFILE_COUNT=N`.

The daemon uses COSMIC's `ext-foreign-toplevel-list` + `zcosmic-toplevel-info` protocols to find the window with the `Activated` state.

If a firmware update breaks switching, re-capture: hook `HIDDevice.prototype.sendReport` in the launcher's DevTools console, switch profiles, and compare the logged reports.

## Troubleshooting

- **`Permission denied` / launcher stuck loading:** check `getfacl /dev/hidrawN` shows `user:<you>:rw-`. If not, the udev rule is missing or numbered ≥ 73.
- **`vendor interface not found`:** the receiver/cable isn't plugged in, or the device name changed — check `grep -h HID_NAME /sys/class/hidraw/*/device/uevent`.
- **Profile doesn't change for an app:** confirm the app ID in the journal matches the key in `apps.conf` exactly.
- **Daemon not running:** `systemctl --user status mouse-profiled`.
