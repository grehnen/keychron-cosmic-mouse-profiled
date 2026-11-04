//! Switches the Keychron G6 HE 8K mouse profile over raw HID.
//!
//! Protocol captured from launcher.keychron.com: output report 0xB5 on the
//! vendor interface (usage page 0xFFC1), payload `0E <index> <profile_count>`
//! padded to 20 bytes.

use std::{fs, io::Write, path::PathBuf};

const KEYCHRON_VENDOR_ID: u32 = 0x3434;
/// `Usage Page (0xFFC1)` as it appears in a HID report descriptor.
const VENDOR_USAGE_PAGE_DESCRIPTOR: [u8; 3] = [0x06, 0xC1, 0xFF];
const PROFILE_REPORT_ID: u8 = 0xB5;
const PROFILE_PAYLOAD_LEN: usize = 20;
const DEFAULT_PROFILE_COUNT: u8 = 2;

/// Number of profiles configured in the launcher ("Levels"); override with `MOUSE_PROFILE_COUNT`.
pub fn profile_count() -> u8 {
    std::env::var("MOUSE_PROFILE_COUNT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PROFILE_COUNT)
}

/// Finds the `/dev/hidrawN` node of the mouse's vendor interface.
pub fn find_vendor_hidraw() -> Option<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir("/sys/class/hidraw").ok()?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    entries.into_iter().find_map(|entry| {
        let device_dir = entry.path().join("device");
        let uevent = fs::read_to_string(device_dir.join("uevent")).ok()?;
        if hid_vendor_id(&uevent)? != KEYCHRON_VENDOR_ID {
            return None;
        }
        let descriptor = fs::read(device_dir.join("report_descriptor")).ok()?;
        descriptor
            .windows(VENDOR_USAGE_PAGE_DESCRIPTOR.len())
            .any(|window| window == VENDOR_USAGE_PAGE_DESCRIPTOR)
            .then(|| PathBuf::from("/dev").join(entry.file_name()))
    })
}

/// Parses the vendor ID from a uevent's `HID_ID=bus:vendor:product` line.
fn hid_vendor_id(uevent: &str) -> Option<u32> {
    let hid_id = uevent.lines().find_map(|line| line.strip_prefix("HID_ID="))?;
    u32::from_str_radix(hid_id.split(':').nth(1)?, 16).ok()
}

pub fn set_profile(index: u8) -> Result<(), String> {
    let count = profile_count();
    if index >= count {
        return Err(format!("profile index {index} out of range (count {count})"));
    }
    let device = find_vendor_hidraw().ok_or("Keychron mouse vendor interface not found")?;
    let mut report = vec![0u8; 1 + PROFILE_PAYLOAD_LEN];
    report[..4].copy_from_slice(&[PROFILE_REPORT_ID, 0x0E, index, count]);
    fs::OpenOptions::new()
        .write(true)
        .open(&device)
        .and_then(|mut hid| hid.write_all(&report))
        .map_err(|err| format!("{}: {err}", device.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vendor_id_from_uevent() {
        let uevent = "DRIVER=hid-generic\nHID_ID=0003:00003434:0000D05A\nHID_NAME=Keychron\n";
        assert_eq!(hid_vendor_id(uevent), Some(0x3434));
    }

    #[test]
    fn missing_hid_id_yields_none() {
        assert_eq!(hid_vendor_id("HID_NAME=Keychron\n"), None);
    }
}
