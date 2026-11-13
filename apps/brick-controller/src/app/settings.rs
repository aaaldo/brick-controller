// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Persistent host and user settings: which hosts are paired, which controller
//! identity each one cached, the identity to offer a new host, and the two
//! preferences exposed by the Settings screen.
//!
//! The file formats are deliberately tiny (`ADDR=key` lines, a single `mode`
//! word and `key=value` preference lines). They survive a firmware update and
//! can be inspected over ADB without a parser.

use super::view_model::{MenuRow, SettingRow};
use crate::bluetooth::{bluez, Address};
use crate::input::DpadMode;
use crate::paths::data_dir;
use crate::profiles::Profile;
use std::collections::HashMap;
use std::path::PathBuf;

fn mode_path() -> PathBuf {
    data_dir().join("mode")
}

fn preferences_path() -> PathBuf {
    data_dir().join("preferences")
}

fn last_profile() -> Profile {
    std::fs::read_to_string(mode_path())
        .ok()
        .and_then(|text| Profile::from_key(&text))
        .unwrap_or(Profile::Pc)
}

fn save_last_profile(profile: Profile) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(mode_path(), profile.key());
}

fn profiles_path() -> PathBuf {
    data_dir().join("profiles")
}

fn load_profiles() -> HashMap<Address, Profile> {
    let mut profiles = HashMap::new();
    let Ok(text) = std::fs::read_to_string(profiles_path()) else {
        return profiles;
    };
    for line in text.lines() {
        let Some((address, profile)) = line.split_once('=') else {
            continue;
        };
        let Some(address) = Address::parse(address) else {
            continue;
        };
        if let Some(profile) = Profile::from_key(profile) {
            profiles.insert(address, profile);
        }
    }
    profiles
}

/// Persist the host/profile map as `ADDR=key` lines.
///
/// Sorted by address so the file is stable across runs and a diff only shows
/// real changes; unknown keys are ignored when reading, so a file written by a
/// newer build still loads without discarding the hosts it knows.
pub(crate) fn save_profiles(profiles: &HashMap<Address, Profile>) {
    let mut entries: Vec<(&Address, &Profile)> = profiles.iter().collect();
    entries.sort_by_key(|(address, _)| address.to_string());
    let text: String = entries
        .into_iter()
        .map(|(address, profile)| format!("{address}={}\n", profile.key()))
        .collect();
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(profiles_path(), text);
}

/// The identity a host cached at pairing time, or PC when the file has no
/// entry for it.
pub fn profile_for_host(address: Address) -> Profile {
    load_profiles()
        .get(&address)
        .copied()
        .unwrap_or(Profile::Pc)
}

/// Remember the identity a host was actually paired with. Called when a link
/// comes up: a host new to the list keeps the profile the session is using,
/// which is what the host just cached. Existing entries are never rewritten.
pub fn remember_profile(address: Address, profile: Profile) {
    let mut profiles = load_profiles();
    if profiles.contains_key(&address) {
        return;
    }
    profiles.insert(address, profile);
    save_profiles(&profiles);
}

/// The last host. A host caches the controller identity it first paired with,
/// so the profile is kept per host.
pub fn saved_host() -> Option<Address> {
    std::fs::read_to_string(data_dir().join("host"))
        .ok()
        .and_then(|s| Address::parse(&s))
}

pub fn save_host(addr: Option<Address>) {
    let path = data_dir().join("host");
    match addr {
        Some(a) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, a.to_string());
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// How strongly the host's rumble request reaches the single motor.
///
/// The preset scales the whole request rather than capping it, so a game's
/// light and strong vibrations keep their relative order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RumbleStrength {
    Off,
    Low,
    Mid,
    Strong,
}

impl RumbleStrength {
    pub const ALL: [Self; 4] = [Self::Off, Self::Low, Self::Mid, Self::Strong];

    pub const fn percent(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Low => 30,
            Self::Mid => 60,
            Self::Strong => 90,
        }
    }

    /// Apply the preset to a level the host asked for, still in 0-255.
    ///
    /// Integer truncation keeps the result from ever exceeding the requested
    /// level; the existing `LEAST` threshold in `platform::rumble` then turns
    /// the quietest results into silence.
    pub const fn scaled(self, level: u8) -> u8 {
        (level as u16 * self.percent() as u16 / 100) as u8
    }
}

fn rumble_key(strength: RumbleStrength) -> &'static str {
    match strength {
        RumbleStrength::Off => "off",
        RumbleStrength::Low => "low",
        RumbleStrength::Mid => "mid",
        RumbleStrength::Strong => "strong",
    }
}

fn rumble_from_key(text: &str) -> Option<RumbleStrength> {
    match text {
        "off" => Some(RumbleStrength::Off),
        "low" => Some(RumbleStrength::Low),
        "mid" => Some(RumbleStrength::Mid),
        "strong" => Some(RumbleStrength::Strong),
        _ => None,
    }
}

fn dpad_key(mode: DpadMode) -> &'static str {
    match mode {
        DpadMode::Dpad => "dpad",
        DpadMode::LeftStick => "stick",
    }
}

fn dpad_from_key(text: &str) -> Option<DpadMode> {
    match text {
        "dpad" => Some(DpadMode::Dpad),
        "stick" => Some(DpadMode::LeftStick),
        _ => None,
    }
}

fn cycle<T: Copy + PartialEq>(all: &[T], current: T, step: isize) -> T {
    let index = all.iter().position(|value| *value == current).unwrap_or(0) as isize;
    all[(index + step).rem_euclid(all.len() as isize) as usize]
}

/// The two values the Settings screen exposes.
///
/// Defaults match the app's previous bias: the hat is sent as a hat, and the
/// motor uses the strongest preset available (90% of the host's request).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preferences {
    pub(crate) dpad: DpadMode,
    pub(crate) rumble: RumbleStrength,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            dpad: DpadMode::Dpad,
            rumble: RumbleStrength::Strong,
        }
    }
}

impl Preferences {
    pub fn load() -> Self {
        std::fs::read_to_string(preferences_path())
            .map(|text| Self::from_text(&text))
            .unwrap_or_default()
    }

    /// Parse `key=value` lines, ignoring anything a newer build may write.
    pub(crate) fn from_text(text: &str) -> Self {
        let mut preferences = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "dpad" => {
                    if let Some(mode) = dpad_from_key(value.trim()) {
                        preferences.dpad = mode;
                    }
                }
                "rumble" => {
                    if let Some(strength) = rumble_from_key(value.trim()) {
                        preferences.rumble = strength;
                    }
                }
                _ => {}
            }
        }
        preferences
    }

    pub(crate) fn to_text(self) -> String {
        format!(
            "dpad={}\nrumble={}\n",
            dpad_key(self.dpad),
            rumble_key(self.rumble)
        )
    }

    pub fn save(self) {
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(preferences_path(), self.to_text());
    }

    /// Move the selected setting one step, wrapping at both ends.
    pub(crate) fn cycle(&mut self, row: SettingRow, step: isize) {
        match row {
            SettingRow::Dpad => self.dpad = cycle(&DpadMode::ALL, self.dpad, step),
            SettingRow::Rumble => {
                self.rumble = cycle(&RumbleStrength::ALL, self.rumble, step);
            }
        }
    }
}

/// The menu's data: paired hosts, the identity a new host would see, and the
/// preferences behind the Settings screen.
///
/// `pairing` is distinct from the session's active `Profile`: it is the
/// identity offered to the next new host, while `profiles` remembers what each
/// existing host cached at pairing time.
#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    pub(crate) hosts: Vec<bluez::Host>,
    pub(crate) profiles: HashMap<Address, Profile>,
    /// The identity a new host would pair with, when no host is known yet.
    pub(crate) pairing: Profile,
    pub(crate) preferences: Preferences,
    pub(crate) error: Option<String>,
}

impl Settings {
    pub fn load(bus: &bluez::Bus, pairing: Profile) -> Self {
        let preferences = Preferences::load();
        match bus.paired_hosts() {
            Ok(hosts) => {
                eprintln!("menu: {} paired host(s)", hosts.len());
                Self {
                    hosts,
                    profiles: load_profiles(),
                    pairing,
                    preferences,
                    error: None,
                }
            }
            Err(error) => {
                eprintln!("menu: {error}");
                Self {
                    hosts: Vec::new(),
                    profiles: load_profiles(),
                    pairing,
                    preferences,
                    error: Some(error),
                }
            }
        }
    }

    /// Devices always ends with pairing, the wired PC mode and Settings, even
    /// while a host is connected: connecting, pairing, switching transport and
    /// preferences are explicit choices on this screen.
    pub fn rows(&self) -> Vec<MenuRow> {
        let mut rows: Vec<MenuRow> = (0..self.hosts.len()).map(MenuRow::Host).collect();
        rows.push(MenuRow::Pair);
        // Settings stays last: the Settings screen returns to the last row,
        // and transport choices such as a wired row belong above it.
        rows.push(MenuRow::Wired);
        rows.push(MenuRow::Settings);
        rows
    }

    pub fn profile_for(&self, address: Address) -> Profile {
        self.profiles.get(&address).copied().unwrap_or(Profile::Pc)
    }
}

/// The profile used when a host has no recorded identity and no mode file.
pub fn fallback_profile() -> Profile {
    last_profile()
}

/// Persist the profile chosen for the next pairing.
pub fn save_mode(profile: Profile) {
    save_last_profile(profile);
}
