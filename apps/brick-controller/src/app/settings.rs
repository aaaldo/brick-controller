// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Persistent host/settings state: which hosts are paired, which controller
//! identity each one cached, and the identity to offer a new host.
//!
//! The file formats are deliberately tiny (`ADDR=key` lines and a single
//! `mode` word). They survive a firmware update and can be inspected over
//! ADB without a parser.

use super::view_model::MenuRow;
use crate::bluetooth::{bluez, Address};
use crate::paths::data_dir;
use crate::profiles::Profile;
use std::collections::HashMap;
use std::path::PathBuf;

fn mode_path() -> PathBuf {
    data_dir().join("mode")
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

/// The menu's data: paired hosts and the identity a new host would see.
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
    pub(crate) error: Option<String>,
}

impl Settings {
    pub fn load(bus: &bluez::Bus, pairing: Profile) -> Self {
        match bus.paired_hosts() {
            Ok(hosts) => {
                eprintln!("menu: {} paired host(s)", hosts.len());
                Self {
                    hosts,
                    profiles: load_profiles(),
                    pairing,
                    error: None,
                }
            }
            Err(error) => {
                eprintln!("menu: {error}");
                Self {
                    hosts: Vec::new(),
                    profiles: load_profiles(),
                    pairing,
                    error: Some(error),
                }
            }
        }
    }

    /// Devices ends with pairing and the wired PC mode, even while a host is
    /// connected: connecting, pairing and switching transport are explicit
    /// choices on this screen.
    pub fn rows(&self) -> Vec<MenuRow> {
        let mut rows: Vec<MenuRow> = (0..self.hosts.len()).map(MenuRow::Host).collect();
        rows.push(MenuRow::Pair);
        rows.push(MenuRow::Wired);
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
