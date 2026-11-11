// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Application-level tests: view-model navigation and the preview renderer.
//! Protocol tests live next to their profile module.

use super::settings::Settings;
use super::view_model::{move_cursor, nav_dir, Dir, MenuRow};
use crate::bluetooth::{bluez, Address};
use crate::input::Hat;
use crate::platform::StatusInfo;
use crate::profiles::Profile;
use crate::ui::{self, Modal, Row, Theme, View};
use brick::canvas::Canvas;
use brick::png;
use brick::text::Fonts;
use std::collections::HashMap;
use std::fs;

fn preview_status(bluetooth: bool) -> StatusInfo {
    StatusInfo {
        clock: "14:32".into(),
        battery: Some(76),
        charging: false,
        wifi: true,
        bluetooth,
    }
}

fn preview_menu_rows(linked: bool) -> Vec<Row> {
    let mut rows = vec![
        Row::new(
            "MacBook Pro",
            if linked {
                "PC · Connected"
            } else {
                "PC · Offline"
            },
            true,
        ),
        Row::new("iPhone", "Xbox · Offline", false),
    ];
    rows.push(Row::new("Pair a new device", "PC", false));
    rows
}

/// `BRICK_CONTROLLER_PREVIEW=dir cargo test preview` writes every screen
/// as a PNG, so the layout can be checked without the device.
#[test]
fn preview_every_screen() {
    let Ok(dir) = std::env::var("BRICK_CONTROLLER_PREVIEW") else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let theme = Theme::fallback();
    let mut fonts = Fonts::load(&[]);
    let screens: Vec<(&str, View, StatusInfo)> = vec![
        (
            "splash",
            View::Splash {
                message: "Starting Bluetooth…".into(),
            },
            preview_status(false),
        ),
        (
            "controller",
            View::Controller {
                host: "MacBook Pro".into(),
                profile: "PC",
            },
            preview_status(true),
        ),
        (
            "menu",
            View::Menu {
                rows: preview_menu_rows(true),
            },
            preview_status(true),
        ),
        (
            "menu-idle",
            View::Menu {
                rows: preview_menu_rows(false),
            },
            preview_status(true),
        ),
        (
            "profile",
            View::Profile {
                rows: Profile::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, profile)| Row::new(profile.label(), "", i == 0))
                    .collect(),
            },
            preview_status(true),
        ),
        (
            "pairing",
            View::Pairing {
                identity: "Brick Controller".into(),
                controller_type: "PC",
                reconnecting: None,
            },
            preview_status(true),
        ),
        (
            "error",
            View::Error {
                title: "Bluetooth off".into(),
                message: "Brick Controller could not start Bluetooth.".into(),
                steps: vec![
                    "Open NextUI Settings and turn Bluetooth on.".into(),
                    "Close Brick Controller and open it again.".into(),
                ],
                countdown: Some("Closing in 60 s.".into()),
            },
            preview_status(false),
        ),
    ];
    for (name, view, status) in screens {
        let mut canvas = Canvas::in_memory(1024, 768);
        ui::draw(
            &mut canvas,
            &mut fonts,
            &theme,
            &view,
            &status,
            None,
            None,
            0.35,
        );
        png::write_png(
            &format!("{dir}/{name}.png"),
            1024,
            768,
            &canvas.snapshot_rgb(),
        )
        .unwrap();
    }

    let modal = Modal {
        title: "Forget MacBook Pro?".into(),
        message: "You will need to pair it again from the host.".into(),
        confirm: "Forget",
        destructive: true,
        loading: false,
    };
    let mut canvas = Canvas::in_memory(1024, 768);
    ui::draw(
        &mut canvas,
        &mut fonts,
        &theme,
        &View::Menu {
            rows: preview_menu_rows(true),
        },
        &preview_status(true),
        Some(&modal),
        None,
        0.35,
    );
    png::write_png(
        &format!("{dir}/confirm.png"),
        1024,
        768,
        &canvas.snapshot_rgb(),
    )
    .unwrap();

    let connecting = Modal {
        title: "Connecting to MacBook Pro".into(),
        message: String::new(),
        confirm: "",
        destructive: false,
        loading: true,
    };
    let mut canvas = Canvas::in_memory(1024, 768);
    ui::draw(
        &mut canvas,
        &mut fonts,
        &theme,
        &View::Menu {
            rows: preview_menu_rows(true),
        },
        &preview_status(true),
        Some(&connecting),
        None,
        0.35,
    );
    png::write_png(
        &format!("{dir}/connecting.png"),
        1024,
        768,
        &canvas.snapshot_rgb(),
    )
    .unwrap();

    let controller = View::Controller {
        host: "MacBook Pro".into(),
        profile: "PC",
    };
    let mut canvas = Canvas::in_memory(1024, 768);
    ui::draw(
        &mut canvas,
        &mut fonts,
        &theme,
        &controller,
        &preview_status(true),
        None,
        Some(0.72),
        0.35,
    );
    png::write_png(
        &format!("{dir}/hold.png"),
        1024,
        768,
        &canvas.snapshot_rgb(),
    )
    .unwrap();
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
