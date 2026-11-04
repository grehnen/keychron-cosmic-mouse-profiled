# mouse-profiled

Automatically switches your Keychron G6 HE 8K mouse to another profile depending on which app has focus on the COSMIC desktop.

It is made up of two small binaries that use only the Rust standard library and `cosmic-client-toolkit`:

- `mouse-profile N` — switches the mouse to profile N (0 = first, 1 = second, …). Bind it to keyboard shortcuts if you like.
- `mouse-profiled` — watches the focused window and switches the profile per `apps.conf`.

## Install

Requires `cargo` ([rustup.rs](https://rustup.rs)).

```sh
./install.sh
```

It is safe to re-run (e.g. after `git pull`). It:

1. writes the udev rule `/etc/udev/rules.d/70-keychron.rules` (asks for sudo, only if missing or changed) and checks you can access the mouse — replug it if told to;
2. builds and installs both binaries with `cargo install` into `~/.cargo/bin`;
3. creates `~/.config/mouse-profile/apps.conf` from `apps.conf.example` if it doesn't exist (never overwrites);
4. installs, enables and (re)starts the `mouse-profiled` systemd user service.

Uninstall with `./install.sh --uninstall` (keeps your `apps.conf`).

### Manual install

1. Let your user access the mouse (one-time, needs sudo). The rule file must be numbered below 73:

   ```sh
   echo 'KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3434", MODE="0660", TAG+="uaccess"' \
     | sudo tee /etc/udev/rules.d/70-keychron.rules
   sudo udevadm control --reload-rules && sudo udevadm trigger
   ```

   Then replug the mouse/receiver.

2. Build, install and test the binaries:

   ```sh
   cargo install --path . --locked
   mouse-profile 1
   ```

3. Install the config and start the daemon:

   ```sh
   mkdir -p ~/.config/mouse-profile ~/.config/systemd/user
   cp -n apps.conf.example ~/.config/mouse-profile/apps.conf
   cp systemd/mouse-profiled.service ~/.config/systemd/user/
   systemctl --user daemon-reload
   systemctl --user enable --now mouse-profiled
   ```

## Configure

Edit `~/.config/mouse-profile/apps.conf`:

```ini
profiles = 2         # number of profiles on the mouse ("Levels" in the launcher; default 2)
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

- Interface: the vendor HID collection (usage page `0xFFC1`), present on both the TurboLink receiver and the mouse itself when wired, found on every connected Keychron device by scanning `/sys/class/hidraw/*/device/report_descriptor`.
- Output report ID `0xB5`, 20-byte payload: `0E <profile_index> <profile_count> 00…`.
- `<profile_count>` matches "Levels" in the launcher (default 2). Set it with `profiles = N` in `apps.conf`.

The daemon uses COSMIC's `ext-foreign-toplevel-list` + `zcosmic-toplevel-info` protocols to find the window with the `Activated` state.

If a firmware update breaks switching, re-capture: hook `HIDDevice.prototype.sendReport` in the launcher's DevTools console, switch profiles, and compare the logged reports.

## Compatibility

Only tested on the Keychron G6 HE 8K with 2 profiles, both through the TurboLink 8K receiver and over a wired USB connection. Nothing is tied to that model: the profile command is sent to every Keychron device (vendor ID `0x3434`) that has the `0xFFC1` vendor interface. Other Keychron mice managed by the same web launcher will probably work, but check first:

- Capture the launcher's profile command (see [How it works](#how-it-works)) and make sure it matches `B5 0E <index> <count>`. Another model could use different bytes.
- Every matching device gets the same command, so a different Keychron model that also has this interface but uses other bytes could receive a command it doesn't understand.
- If you use more than 2 profiles, set `profiles = N` in `apps.conf`.

## Troubleshooting

- **`Permission denied` / launcher stuck loading:** check `getfacl /dev/hidrawN` shows `user:<you>:rw-`. If not, the udev rule is missing or numbered ≥ 73.
- **`vendor interface not found`:** the receiver/cable isn't plugged in, or the device name changed — check `grep -h HID_NAME /sys/class/hidraw/*/device/uevent`.
- **Profile doesn't change for an app:** confirm the app ID in the journal matches the key in `apps.conf` exactly.
- **Daemon not running:** `systemctl --user status mouse-profiled`.
