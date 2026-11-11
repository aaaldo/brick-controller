// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The status bar's platform readings: battery, charging, Wi-Fi and clock.
//!
//! These are the only sysfs and libc reads the UI needs. Keeping them here
//! lets `ui` draw a plain [`StatusInfo`] value and keeps host-build previews
//! independent of the device paths.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusInfo {
    pub clock: String,
    pub battery: Option<u8>,
    pub charging: bool,
    pub wifi: bool,
    pub bluetooth: bool,
}

impl StatusInfo {
    pub fn read(bluetooth: bool) -> Self {
        Self {
            clock: clock_text(),
            battery: read_battery(),
            charging: read_charging(),
            wifi: read_wifi(),
            bluetooth,
        }
    }
}

fn read_sysfs(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
}

fn read_battery() -> Option<u8> {
    [
        "/sys/class/power_supply/axp2202-battery/capacity",
        "/sys/class/power_supply/battery/capacity",
    ]
    .iter()
    .find_map(|path| read_sysfs(path)?.parse::<u8>().ok())
    .filter(|value| *value <= 100)
}

fn read_charging() -> bool {
    read_sysfs("/sys/class/power_supply/axp2202-usb/online").as_deref() == Some("1")
        || read_sysfs("/sys/class/power_supply/ac/online").as_deref() == Some("1")
}

fn read_wifi() -> bool {
    if let Some(state) = read_sysfs("/sys/class/net/wlan0/operstate") {
        return state == "up";
    }
    // Older firmware may not expose operstate; the link-quality column is
    // the next best signal that the radio is associated.
    std::fs::read_to_string("/proc/net/wireless")
        .map(|text| {
            text.lines().skip(2).any(|line| {
                let mut fields = line.split_whitespace();
                let Some(name) = fields.next() else {
                    return false;
                };
                if !name.starts_with("wlan") {
                    return false;
                }
                fields
                    .nth(1)
                    .and_then(|quality| quality.trim_end_matches('.').parse::<f32>().ok())
                    .is_some_and(|quality| quality > 0.0)
            })
        })
        .unwrap_or(false)
}

fn clock_text() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "--:--".into();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}
