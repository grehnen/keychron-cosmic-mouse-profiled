//! Switches the Keychron G6 HE 8K mouse profile over raw HID.
//!
//! Protocol captured from launcher.keychron.com: output report 0xB5 on the
//! vendor interface (usage page 0xFFC1), payload `0E <index> <profile_count>`
//! padded to 20 bytes.

pub mod setup;

use std::{
    collections::HashMap,
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
};

const KEYCHRON_VENDOR_ID: u16 = 0x3434;
/// `Usage Page (0xFFC1)` as it appears in a HID report descriptor.
const VENDOR_USAGE_PAGE_DESCRIPTOR: [u8; 3] = [0x06, 0xC1, 0xFF];
const PROFILE_REPORT_ID: u8 = 0xB5;
const PROFILE_PAYLOAD_LEN: usize = 20;
pub const DEFAULT_PROFILE_COUNT: u8 = 2;
/// The launcher allows at most 5 profiles ("Levels").
pub const MAX_PROFILE_COUNT: u8 = 5;

/// Contents of `~/.config/mouse-profile/apps.conf`.
pub struct Config {
    /// Number of profiles configured in the launcher ("Levels"), from `profiles = N`.
    pub profile_count: u8,
    /// Profile for apps without their own rule, from `default = N`.
    pub default_profile: u8,
    /// `app_id = N` rules, keyed by lowercase app ID.
    pub by_app_id: HashMap<String, u8>,
    /// Devices to switch, from `device = vendor:product` lines. Empty means every matching device.
    pub devices: Vec<UsbId>,
}

/// A USB vendor:product ID pair, written as `3434:d05a` in `apps.conf`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
}

impl fmt::Display for UsbId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vendor, self.product)
    }
}

impl UsbId {
    pub fn parse(text: &str) -> Option<Self> {
        let (vendor, product) = text.trim().split_once(':')?;
        Some(Self {
            vendor: u16::from_str_radix(vendor, 16).ok()?,
            product: u16::from_str_radix(product, 16).ok()?,
        })
    }
}

impl Config {
    pub fn profile_for(&self, app_id: &str) -> u8 {
        *self.by_app_id.get(&app_id.to_lowercase()).unwrap_or(&self.default_profile)
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join(".config"));
    base.join("mouse-profile/apps.conf")
}

/// Loads the config, falling back to defaults if the file is missing.
pub fn load_config() -> Config {
    parse_config(&fs::read_to_string(config_path()).unwrap_or_default())
}

/// Format: `profiles = N`, `default = N`, `device = vendor:product` (repeatable), and one
/// `app_id = N` per line; `#` starts a comment.
fn parse_config(text: &str) -> Config {
    let mut config = Config {
        profile_count: DEFAULT_PROFILE_COUNT,
        default_profile: 0,
        by_app_id: HashMap::new(),
        devices: Vec::new(),
    };
    for line in text.lines().map(|l| l.split('#').next().unwrap().trim()) {
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if key == "device" {
            config.devices.extend(UsbId::parse(value));
            continue;
        }
        let Ok(number) = value.parse::<u8>() else { continue };
        match key {
            "profiles" if (1..=MAX_PROFILE_COUNT).contains(&number) => config.profile_count = number,
            "profiles" => {}
            "default" => config.default_profile = number,
            app_id => {
                config.by_app_id.insert(app_id.to_lowercase(), number);
            }
        }
    }
    config
}

/// A hidraw node the profile command can go to.
pub struct MouseInterface {
    /// e.g. `/dev/hidraw2`.
    pub node: PathBuf,
    pub id: UsbId,
    /// Product name, e.g. "Keychron TurboLink 8K".
    pub name: String,
}

/// Finds every Keychron hidraw node with the vendor interface, sorted by node.
/// One device can expose several of these.
pub fn mouse_interfaces() -> Vec<MouseInterface> {
    let Ok(dir) = fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut found: Vec<_> = dir
        .flatten()
        .filter_map(|entry| {
            let device_dir = entry.path().join("device");
            let uevent = fs::read_to_string(device_dir.join("uevent")).ok()?;
            let id = hid_usb_id(&uevent)?;
            if id.vendor != KEYCHRON_VENDOR_ID || !has_vendor_usage_page(&device_dir) {
                return None;
            }
            let name = uevent
                .lines()
                .find_map(|line| line.strip_prefix("HID_NAME="))
                .map(tidy_name)
                .unwrap_or_else(|| "unknown device".into());
            Some(MouseInterface { node: PathBuf::from("/dev").join(entry.file_name()), id, name })
        })
        .collect();
    found.sort_by(|a, b| a.node.cmp(&b.node));
    found
}

fn has_vendor_usage_page(device_dir: &Path) -> bool {
    fs::read(device_dir.join("report_descriptor")).is_ok_and(|descriptor| {
        descriptor
            .windows(VENDOR_USAGE_PAGE_DESCRIPTOR.len())
            .any(|window| window == VENDOR_USAGE_PAGE_DESCRIPTOR)
    })
}

/// HID names repeat the vendor ("Keychron Keychron TurboLink 8K"); drops the duplicate.
fn tidy_name(name: &str) -> String {
    let name = name.trim();
    match name.split_once(' ') {
        Some((first, rest)) if rest.starts_with(&format!("{first} ")) => rest.to_string(),
        _ => name.to_string(),
    }
}

/// Parses the USB ID from a uevent's `HID_ID=bus:vendor:product` line (8 hex digits each).
fn hid_usb_id(uevent: &str) -> Option<UsbId> {
    let hid_id = uevent.lines().find_map(|line| line.strip_prefix("HID_ID="))?;
    let mut fields = hid_id.split(':').skip(1);
    let mut next = || u32::from_str_radix(fields.next()?, 16).ok()?.try_into().ok();
    Some(UsbId { vendor: next()?, product: next()? })
}

/// Sends the profile command to every matching device (see `Config::devices`).
/// Fails if none is found or any write fails.
pub fn set_profile(index: u8, config: &Config) -> Result<(), String> {
    let count = config.profile_count;
    if index >= count {
        return Err(format!("profile index {index} out of range (count {count})"));
    }
    let devices: Vec<PathBuf> = mouse_interfaces()
        .into_iter()
        .filter(|mouse| config.devices.is_empty() || config.devices.contains(&mouse.id))
        .map(|mouse| mouse.node)
        .collect();
    if devices.is_empty() {
        return Err("no matching Keychron mouse connected".into());
    }
    let mut report = vec![0u8; 1 + PROFILE_PAYLOAD_LEN];
    report[..4].copy_from_slice(&[PROFILE_REPORT_ID, 0x0E, index, count]);
    let errors: Vec<String> = devices
        .iter()
        .filter_map(|device| {
            fs::OpenOptions::new()
                .write(true)
                .open(device)
                .and_then(|mut hid| hid.write_all(&report))
                .err()
                .map(|err| format!("{}: {err}", device.display()))
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usb_id_from_uevent() {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00003434:0000D05A\nHID_NAME=Keychron\n";
        assert_eq!(hid_usb_id(uevent), Some(UsbId { vendor: 0x3434, product: 0xd05a }));
    }

    #[test]
    fn tidies_repeated_vendor_in_name() {
        assert_eq!(tidy_name("Keychron Keychron TurboLink 8K"), "Keychron TurboLink 8K");
        assert_eq!(tidy_name("Keychron TurboLink 8K"), "Keychron TurboLink 8K");
        assert_eq!(tidy_name("Keychron"), "Keychron");
    }

    #[test]
    fn formats_usb_id() {
        assert_eq!(UsbId { vendor: 0x3434, product: 0xd05a }.to_string(), "3434:d05a");
    }

    #[test]
    fn missing_hid_id_yields_none() {
        assert_eq!(hid_usb_id("HID_NAME=Keychron\n"), None);
    }

    #[test]
    fn parses_device_lines() {
        let config = parse_config("device = 3434:d05a  # Keychron TurboLink 8K\ndevice = 3434:D0AB\ndevice = nonsense\n");
        assert_eq!(
            config.devices,
            [UsbId { vendor: 0x3434, product: 0xd05a }, UsbId { vendor: 0x3434, product: 0xd0ab }]
        );
        assert!(config.by_app_id.is_empty());
    }

    #[test]
    fn parses_config() {
        let config = parse_config("# comment\nprofiles = 3\ndefault = 1\nGamescope = 2  # inline\njunk\n");
        assert_eq!(config.profile_count, 3);
        assert_eq!(config.profile_for("gamescope"), 2);
        assert_eq!(config.profile_for("firefox"), 1);
    }

    #[test]
    fn empty_config_uses_defaults() {
        let config = parse_config("");
        assert_eq!(config.profile_count, DEFAULT_PROFILE_COUNT);
        assert_eq!(config.profile_for("anything"), 0);
    }

    #[test]
    fn out_of_range_profile_count_is_ignored() {
        assert_eq!(parse_config("profiles = 0").profile_count, DEFAULT_PROFILE_COUNT);
        assert_eq!(parse_config("profiles = 6").profile_count, DEFAULT_PROFILE_COUNT);
        assert_eq!(parse_config("profiles = 5").profile_count, 5);
    }
}
