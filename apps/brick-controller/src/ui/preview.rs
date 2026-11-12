// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Deterministic screen previews for visual review. Only compiled for tests.

use super::{draw, Modal, Row, Theme, View};
use crate::i18n::{fill, tr, Key, Lang};
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
    let status = if linked {
        tr(Lang::En, Key::Connected)
    } else {
        tr(Lang::En, Key::Offline)
    };
    let offline = tr(Lang::En, Key::Offline);
    let mut rows = vec![
        Row::new("MacBook Pro", format!("PC · {status}"), true),
        Row::new("iPhone", format!("Xbox · {offline}"), false),
    ];
    rows.push(Row::new(tr(Lang::En, Key::PairNew), "PC", false));
    rows.push(Row::new(
        tr(Lang::En, Key::WiredConnection),
        tr(Lang::En, Key::WiredDetail),
        false,
    ));
    rows
}

/// The menu with the wired row selected, to check that `X Forget` disappears
/// when the selection has no stored host.
fn preview_wired_rows() -> Vec<Row> {
    let mut rows = preview_menu_rows(true);
    for row in &mut rows {
        row.selected = false;
    }
    if let Some(last) = rows.last_mut() {
        last.selected = true;
    }
    rows
}

/// `BRICK_CONTROLLER_PREVIEW=dir cargo test preview` writes every screen
/// as a PNG, so the layout can be checked without the device.
///
/// `BRICK_CONTROLLER_PREVIEW_THEME` optionally points to a `nextval.elf`
/// JSON captured from the device; without it the fallback palette is used.
#[test]
fn preview_every_screen() {
    let Ok(dir) = std::env::var("BRICK_CONTROLLER_PREVIEW") else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let theme = std::env::var_os("BRICK_CONTROLLER_PREVIEW_THEME")
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|json| Theme::from_json(&json))
        .unwrap_or_else(Theme::fallback);
    let mut fonts = Fonts::load(&[]);
    let screens: Vec<(&str, View, StatusInfo)> = vec![
        (
            "splash",
            View::Splash {
                message: tr(Lang::En, Key::StartingBluetooth).into(),
            },
            preview_status(false),
        ),
        (
            "controller",
            View::Controller {
                host: "MacBook Pro".into(),
                profile: "PC",
                wired: false,
            },
            preview_status(true),
        ),
        (
            "menu",
            View::Menu {
                rows: preview_menu_rows(true),
                can_forget: true,
            },
            preview_status(true),
        ),
        (
            "menu-idle",
            View::Menu {
                rows: preview_menu_rows(false),
                can_forget: true,
            },
            preview_status(true),
        ),
        (
            "menu-wired",
            View::Menu {
                rows: preview_wired_rows(),
                can_forget: false,
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
                identity: "Brick Controller (PC)".into(),
                controller_type: "PC",
                reconnecting: None,
            },
            preview_status(true),
        ),
        (
            "wired-waiting",
            View::WiredWaiting {
                identity: Profile::Pc.bt_name().into(),
                controller_type: Profile::Pc.label(),
            },
            preview_status(true),
        ),
        (
            "wired-connected",
            View::Controller {
                host: tr(Lang::En, Key::WiredHost).into(),
                profile: Profile::Pc.label(),
                wired: true,
            },
            preview_status(true),
        ),
        (
            "error",
            View::Error {
                title: tr(Lang::En, Key::ErrorTitle).into(),
                message: tr(Lang::En, Key::BluetoothActivationError).into(),
                back: false,
            },
            preview_status(false),
        ),
        (
            "unsupported",
            View::Unsupported {
                title: tr(Lang::En, Key::UnsupportedDeviceTitle).into(),
                message: tr(Lang::En, Key::UnsupportedDeviceMessage).into(),
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
            Lang::En,
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
        title: fill(tr(Lang::En, Key::ForgetTitle), &[("name", "MacBook Pro")]),
        message: tr(Lang::En, Key::ForgetMessage).into(),
        confirm: tr(Lang::En, Key::Forget),
        destructive: true,
        loading: false,
    };
    let mut canvas = Canvas::in_memory(1024, 768);
    draw(
        &mut canvas,
        &mut fonts,
        &theme,
        Lang::En,
        &View::Menu {
            rows: preview_menu_rows(true),
            can_forget: true,
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

    let connecting = Modal::loading(fill(
        tr(Lang::En, Key::ConnectingTo),
        &[("name", "MacBook Pro")],
    ));
    let mut canvas = Canvas::in_memory(1024, 768);
    draw(
        &mut canvas,
        &mut fonts,
        &theme,
        Lang::En,
        &View::Menu {
            rows: preview_menu_rows(true),
            can_forget: true,
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

    let changing = Modal::loading(tr(Lang::En, Key::ChangingControllerType));
    let mut canvas = Canvas::in_memory(1024, 768);
    draw(
        &mut canvas,
        &mut fonts,
        &theme,
        Lang::En,
        &View::Profile {
            rows: Profile::ALL
                .iter()
                .enumerate()
                .map(|(i, profile)| Row::new(profile.label(), "", i == 0))
                .collect(),
        },
        &preview_status(true),
        Some(&changing),
        None,
        0.35,
    );
    png::write_png(
        &format!("{dir}/changing-type.png"),
        1024,
        768,
        &canvas.snapshot_rgb(),
    )
    .unwrap();

    let returning = Modal::loading(tr(Lang::En, Key::ReturningToDevices));
    let mut canvas = Canvas::in_memory(1024, 768);
    draw(
        &mut canvas,
        &mut fonts,
        &theme,
        Lang::En,
        &View::Menu {
            rows: Vec::new(),
            can_forget: false,
        },
        &preview_status(true),
        Some(&returning),
        None,
        0.35,
    );
    png::write_png(
        &format!("{dir}/returning.png"),
        1024,
        768,
        &canvas.snapshot_rgb(),
    )
    .unwrap();

    let controller = View::Controller {
        host: "MacBook Pro".into(),
        profile: "PC",
        wired: false,
    };
    let mut canvas = Canvas::in_memory(1024, 768);
    draw(
        &mut canvas,
        &mut fonts,
        &theme,
        Lang::En,
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
