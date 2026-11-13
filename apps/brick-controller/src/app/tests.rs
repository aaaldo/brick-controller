// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Application-level tests: view-model navigation and settings.
//! Protocol tests live next to their profile module; screen previews live in
//! `ui/preview.rs`.

use super::settings::{Preferences, RumbleStrength, Settings};
use super::view_model::{
    changing_type_modal, move_cursor, nav_dir, returning_to_devices_modal, view_for,
    wired_controller, wired_waiting, Dir, MenuRow, Screen, SettingRow,
};
use super::{start_after_switch, StartScreen};
use crate::bluetooth::{bluez, Address};
use crate::i18n::Lang;
use crate::input::{DpadMode, Hat};
use crate::profiles::Profile;
use crate::ui::View;
use std::collections::HashMap;

fn settings_with_one_host() -> Settings {
    Settings {
        hosts: vec![bluez::Host {
            addr: Address([0x11; 6]),
            name: "Studio Mac".into(),
            connected: true,
        }],
        profiles: HashMap::new(),
        pairing: Profile::Pc,
        preferences: Preferences::default(),
        error: None,
    }
}

#[test]
fn the_dpad_maps_to_one_direction_at_a_time() {
    assert_eq!(nav_dir(Hat::Centred, Hat::Right), Some(Dir::Right));
    assert_eq!(nav_dir(Hat::Right, Hat::DownRight), Some(Dir::Down));
    assert_eq!(nav_dir(Hat::UpRight, Hat::Right), Some(Dir::Right));
    assert_eq!(nav_dir(Hat::UpRight, Hat::Up), None);
    assert_eq!(nav_dir(Hat::Up, Hat::Centred), None);
}

#[test]
fn cursors_wrap_and_an_empty_list_is_safe() {
    assert_eq!(move_cursor(0, 3, 1), 1);
    assert_eq!(move_cursor(2, 3, 1), 0);
    assert_eq!(move_cursor(0, 3, -1), 2);
    assert_eq!(move_cursor(4, 0, 1), 0);
}

#[test]
fn the_menu_keeps_pairing_wired_and_settings_while_a_host_is_connected() {
    let rows = settings_with_one_host().rows();
    assert!(rows.iter().any(|row| matches!(row, MenuRow::Host(0))));
    assert!(rows.contains(&MenuRow::Pair));
    assert!(rows.contains(&MenuRow::Wired));
    assert_eq!(rows.last(), Some(&MenuRow::Settings));
}

#[test]
fn forget_is_offered_only_for_a_paired_host() {
    let settings = settings_with_one_host();
    let can_forget = |cursor| {
        let view = view_for(
            &Screen::Menu { cursor },
            &settings,
            None,
            Profile::Pc,
            None,
            Lang::En,
        );
        match view {
            View::Menu { can_forget, .. } => can_forget,
            other => panic!("expected a menu, got {other:?}"),
        }
    };
    assert!(can_forget(0), "the host row may be forgotten");
    assert!(!can_forget(1), "Pairing has no identity to forget");
    assert!(!can_forget(2), "the wired mode has no identity to forget");
    assert!(!can_forget(3), "Settings has no identity to forget");
}

#[test]
fn only_settings_opens_instead_of_connecting() {
    let settings = settings_with_one_host();
    let open = |cursor| {
        let view = view_for(
            &Screen::Menu { cursor },
            &settings,
            None,
            Profile::Pc,
            None,
            Lang::En,
        );
        match view {
            View::Menu { open, .. } => open,
            other => panic!("expected a menu, got {other:?}"),
        }
    };
    assert!(!open(0), "a host row connects");
    assert!(!open(1), "Pairing still uses the Connect label");
    assert!(!open(2), "the wired row starts a connection");
    assert!(open(3), "Settings opens with the Open label");
}

#[test]
fn the_wired_views_name_the_pc_identity_and_usb_transport() {
    match wired_waiting() {
        View::WiredWaiting {
            identity,
            controller_type,
        } => {
            assert_eq!(identity, "Brick Controller (PC)");
            assert_eq!(controller_type, "PC");
        }
        other => panic!("expected the wired waiting view, got {other:?}"),
    }
    match wired_controller(Lang::En) {
        View::Controller {
            host,
            profile,
            wired,
        } => {
            assert_eq!(host, "PC (USB)");
            assert_eq!(profile, "PC");
            assert!(wired);
        }
        other => panic!("expected a controller view, got {other:?}"),
    }
}

#[test]
fn returning_to_devices_shows_a_loading_popup() {
    let modal = returning_to_devices_modal(Lang::En);
    assert!(modal.loading);
    assert_eq!(modal.title, "Returning to Devices…");
}

#[test]
fn preferences_default_to_the_hat_and_strong_rumble() {
    let preferences = Preferences::default();
    assert_eq!(preferences.dpad, DpadMode::Dpad);
    assert_eq!(preferences.rumble, RumbleStrength::Strong);
}

#[test]
fn preferences_round_trip_and_ignore_unknown_values() {
    let preferences = Preferences::from_text("dpad=stick\nrumble=low\nfuture=value\n");
    assert_eq!(preferences.dpad, DpadMode::LeftStick);
    assert_eq!(preferences.rumble, RumbleStrength::Low);
    assert_eq!(preferences.to_text(), "dpad=stick\nrumble=low\n");

    let fallback = Preferences::from_text("dpad=sideways\nrumble=MAX\n");
    assert_eq!(fallback, Preferences::default());
}

#[test]
fn settings_rows_show_the_current_values() {
    let settings = Settings {
        hosts: Vec::new(),
        profiles: HashMap::new(),
        pairing: Profile::Pc,
        preferences: Preferences {
            dpad: DpadMode::LeftStick,
            rumble: RumbleStrength::Low,
        },
        error: None,
    };
    let view = view_for(
        &Screen::Settings { cursor: 1 },
        &settings,
        None,
        Profile::Pc,
        None,
        Lang::En,
    );
    let View::Settings { rows } = view else {
        panic!("expected the Settings view");
    };
    assert_eq!(rows[0].label, "D-Pad Mode");
    assert_eq!(rows[0].trailing, "Left Stick");
    assert!(!rows[0].selected);
    assert_eq!(rows[1].label, "Rumble strength");
    assert_eq!(rows[1].trailing, "Low");
    assert!(rows[1].selected);
}

#[test]
fn rumble_strength_scales_the_host_request() {
    assert_eq!(RumbleStrength::Off.scaled(255), 0);
    assert_eq!(RumbleStrength::Low.scaled(255), 76);
    assert_eq!(RumbleStrength::Mid.scaled(255), 153);
    assert_eq!(RumbleStrength::Strong.scaled(255), 229);
    assert_eq!(RumbleStrength::Strong.scaled(0), 0);
}

#[test]
fn settings_values_cycle_and_wrap() {
    let mut preferences = Preferences::default();
    preferences.cycle(SettingRow::Dpad, 1);
    assert_eq!(preferences.dpad, DpadMode::LeftStick);
    preferences.cycle(SettingRow::Dpad, 1);
    assert_eq!(preferences.dpad, DpadMode::Dpad);

    preferences.cycle(SettingRow::Rumble, -1);
    assert_eq!(preferences.rumble, RumbleStrength::Mid);
    for _ in 0..4 {
        preferences.cycle(SettingRow::Rumble, 1);
    }
    assert_eq!(preferences.rumble, RumbleStrength::Mid, "four steps wrap");
}

#[test]
fn a_profile_key_round_trips() {
    for profile in Profile::ALL {
        assert_eq!(Profile::from_key(profile.key()), Some(profile));
    }
    assert_eq!(Profile::from_key(" xbox\n"), Some(Profile::Xbox));
    assert_eq!(Profile::from_key("PC"), Some(Profile::Pc));
    assert_eq!(Profile::from_key("PlayStation"), Some(Profile::PlayStation));
    assert_eq!(Profile::from_key("ds4"), None);
}

#[test]
fn a_chosen_profile_reopens_pairing() {
    assert_eq!(start_after_switch(true), StartScreen::Pairing);
    assert_eq!(start_after_switch(false), StartScreen::Devices);
}

#[test]
fn changing_the_controller_type_shows_a_loading_popup() {
    let modal = changing_type_modal(Lang::En);
    assert!(modal.loading);
    assert_eq!(modal.title, "Changing controller type…");
}
