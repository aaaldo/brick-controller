// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Application state that becomes a drawable `ui::View`.
//!
//! The UI must not know about hosts, profiles or the pairing flow. This module
//! is the seam: it turns `Screen`, `Settings` and the active `Link` into plain
//! labels and rows, and resolves modal titles once.

use super::session::Link;
use super::settings::Settings;
use crate::bluetooth::Address;
use crate::profiles::Profile;
use crate::ui::{Modal, Row, View};

/// One focusable line in the main menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRow {
    Host(usize),
    Pair,
}

/// The screen currently displayed above the Bluetooth session.
///
/// This is application state, not a render model: `view_for` turns it into a
/// `ui::View` so the drawing code never needs to know about hosts, profiles or
/// the pairing flow.
#[derive(Clone, PartialEq, Eq)]
pub enum Screen {
    Controller,
    Menu { cursor: usize },
    Profile { choice: Profile },
    Pairing { back: bool },
}

/// A modal waiting for A or B.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalState {
    Forget(usize),
    Connecting(Address),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Collapse a diagonal hat to its dominant cardinal direction.
///
/// The hat reports eight positions, but a list only moves on four. Treating a
/// diagonal as one of its two sides keeps a diagonal roll from producing two
/// moves at once; the side chosen here is kept stable so navigation does not
/// alternate between up and right on alternate frames.
pub fn primary_dir(hat: crate::input::Hat) -> Option<Dir> {
    use crate::input::Hat;
    match hat {
        Hat::Up | Hat::UpRight | Hat::UpLeft => Some(Dir::Up),
        Hat::Right => Some(Dir::Right),
        Hat::DownRight | Hat::Down | Hat::DownLeft => Some(Dir::Down),
        Hat::Left => Some(Dir::Left),
        Hat::Centred => None,
    }
}

/// Report a direction only when the dominant cardinal direction changed.
///
/// Holding a diagonal would otherwise repeat the same direction every frame;
/// this makes one physical movement produce one navigation step.
pub fn nav_dir(previous: crate::input::Hat, current: crate::input::Hat) -> Option<Dir> {
    let from = primary_dir(previous);
    let to = primary_dir(current);
    if from == to {
        None
    } else {
        to
    }
}

pub fn move_cursor(cursor: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    ((cursor as isize + delta).rem_euclid(len as isize)) as usize
}

/// Turn application state into the drawable `ui::View`.
///
/// The list windows are computed here rather than in the UI: Devices scrolls
/// to keep five rows visible, and every row carries the profile the host
/// cached. Keeping this mapping outside `ui` lets the renderer be tested with
/// synthetic views and no Bluetooth.
pub fn view_for(
    screen: &Screen,
    settings: &Settings,
    link: Option<&Link>,
    profile: Profile,
    target: Option<Address>,
) -> View {
    match screen {
        Screen::Controller => View::Controller {
            host: link.map(|l| l.name.clone()).unwrap_or_default(),
            profile: profile.label(),
        },
        Screen::Menu { cursor } => {
            let rows = settings.rows();
            let cursor = (*cursor).min(rows.len().saturating_sub(1));
            let start = cursor.saturating_sub(4);
            let rows = rows
                .iter()
                .enumerate()
                .skip(start)
                .take(5)
                .map(|(index, row)| match row {
                    MenuRow::Host(host_index) => {
                        let host = &settings.hosts[*host_index];
                        let status = if host.connected {
                            "Connected"
                        } else {
                            "Offline"
                        };
                        Row::new(
                            host.name.clone(),
                            format!("{} · {status}", settings.profile_for(host.addr).label()),
                            index == cursor,
                        )
                    }
                    MenuRow::Pair => Row::new(
                        "Pair a new device",
                        settings.pairing.label(),
                        index == cursor,
                    ),
                })
                .collect();
            View::Menu { rows }
        }
        Screen::Profile { choice } => {
            let rows = Profile::ALL
                .iter()
                .map(|profile| Row::new(profile.label(), "", *profile == *choice))
                .collect();
            View::Profile { rows }
        }
        Screen::Pairing { .. } => View::Pairing {
            identity: settings.pairing.bt_name().into(),
            controller_type: settings.pairing.label(),
            reconnecting: target.map(|addr| addr.to_string()),
        },
    }
}

/// Build the blocking popup for the modal state.
///
/// The host name is resolved here, once, so the UI never needs to know how to
/// find a host by address.
pub fn modal_view(modal: &ModalState, settings: &Settings) -> Modal {
    match modal {
        ModalState::Forget(index) => {
            let name = settings
                .hosts
                .get(*index)
                .map(|host| host.name.clone())
                .unwrap_or_else(|| "this controller".into());
            Modal {
                title: format!("Forget {name}?"),
                message: "You will need to pair it again from the host.".into(),
                confirm: "Forget",
                destructive: true,
                loading: false,
            }
        }
        ModalState::Connecting(addr) => {
            let name = settings
                .hosts
                .iter()
                .find(|host| host.addr == *addr)
                .map(|host| host.name.clone())
                .unwrap_or_else(|| addr.to_string());
            Modal {
                title: format!("Connecting to {name}"),
                message: String::new(),
                confirm: "",
                destructive: false,
                loading: true,
            }
        }
    }
}
