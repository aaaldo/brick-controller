// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Application-level tests: view-model navigation and settings.
//! Protocol tests live next to their profile module; screen previews live in
//! `ui/preview.rs`.

use super::settings::Settings;
use super::view_model::{changing_type_modal, move_cursor, nav_dir, Dir, MenuRow};
use super::{start_after_switch, StartScreen};
use crate::bluetooth::{bluez, Address};
use crate::i18n::Lang;
use crate::input::Hat;
use crate::profiles::Profile;
use std::collections::HashMap;

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
fn the_menu_keeps_pairing_while_a_host_is_connected() {
    let settings = Settings {
        hosts: vec![bluez::Host {
            addr: Address([0x11; 6]),
            name: "Studio Mac".into(),
            connected: true,
        }],
        profiles: HashMap::new(),
        pairing: Profile::Pc,
        error: None,
    };
    let rows = settings.rows();
    assert!(rows.iter().any(|row| matches!(row, MenuRow::Host(0))));
    assert_eq!(rows.last(), Some(&MenuRow::Pair));
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
