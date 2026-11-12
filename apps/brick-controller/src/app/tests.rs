// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Application-level tests: view-model navigation and settings.
//! Protocol tests live next to their profile module; screen previews live in
//! `ui/preview.rs`.

use super::settings::Settings;
use super::view_model::{
    changing_type_modal, move_cursor, nav_dir, returning_to_devices_modal, view_for,
    wired_controller, wired_waiting, Dir, MenuRow, Screen,
};
use super::{start_after_switch, StartScreen};
use crate::bluetooth::{bluez, Address};
use crate::i18n::Lang;
use crate::input::Hat;
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
fn the_menu_keeps_pairing_and_wired_while_a_host_is_connected() {
    let settings = settings_with_one_host();
    let rows = settings.rows();
    assert!(rows.iter().any(|row| matches!(row, MenuRow::Host(0))));
    assert!(rows.contains(&MenuRow::Pair));
    assert_eq!(rows.last(), Some(&MenuRow::Wired));
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
