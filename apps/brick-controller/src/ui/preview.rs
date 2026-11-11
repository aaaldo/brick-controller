// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Deterministic screen previews for visual review. Only compiled for tests.

use super::{draw, Modal, Row, Theme, View};
use crate::platform::StatusInfo;
use crate::profiles::Profile;
use brick::canvas::Canvas;
use brick::png;
use brick::text::Fonts;
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
        draw(
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
    draw(
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
    draw(
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
    draw(
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
