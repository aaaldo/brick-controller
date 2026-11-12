// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The application shell: bootstrap the renderer and run one session after
//! another as the user switches profile or retries.

mod session;
mod settings;
#[cfg(test)]
mod tests;
mod view_model;
#[cfg(target_os = "linux")]
mod wired;

use crate::i18n::{tr, Key, Lang};
use crate::input::Button;
use crate::platform::input_trimui::{pad_path, Pad};
use crate::platform::model::Model;
use crate::platform::StatusInfo;
use crate::ui::{Frame, Renderer, Theme, View};
use brick::canvas::Canvas;
use brick::text::Fonts;
use session::{session, SessionOutcome};
use settings::{fallback_profile, profile_for_host, save_mode, saved_host};
use std::os::unix::io::AsRawFd;
use std::time::Duration;

/// How long Menu+Start must be held to leave the controller view.
pub(crate) const DISCONNECT_HOLD: Duration = Duration::from_millis(1000);

/// After a host connects, the panel stays on this long, then goes dark until
/// Menu+Start is pressed once. Shared by the Bluetooth and wired sessions.
pub(crate) const SCREEN_OFF_AFTER: Duration = Duration::from_secs(10);

/// The release, as `package.sh` builds it; "dev" for any other build.
const VERSION: &str = match option_env!("BRICK_CONTROLLER_VERSION") {
    Some(v) => v,
    None => "dev",
};

/// Where a session starts after the Bluetooth daemon has been restarted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartScreen {
    /// Show the splash while Bluetooth starts.
    Splash,
    /// Go straight to the Devices list.
    Devices,
    /// Go straight to Pairing, so a newly chosen identity can be paired.
    Pairing,
    /// Show the "Returning to Devices…" popup while Bluetooth restarts.
    Restarting,
}

/// The screen to reopen after a profile switch.
///
/// A type chosen from Pairing returns there; a Connect that needed another
/// identity returns to Devices, where the connection flow resumes.
fn start_after_switch(pairing: bool) -> StartScreen {
    if pairing {
        StartScreen::Pairing
    } else {
        StartScreen::Devices
    }
}

#[cfg(target_os = "linux")]
fn run_wired(screen: &Renderer, pad: &mut Pad) -> Result<(), String> {
    wired::session(screen, pad)
}

/// The host build has no configfs or FunctionFS. It only compiles the menu and
/// view code, so reaching this arm is a development mistake, not a device path.
#[cfg(not(target_os = "linux"))]
fn run_wired(_screen: &Renderer, _pad: &mut Pad) -> Result<(), String> {
    Err("USB wired mode is only available on the device".into())
}

pub(crate) fn run() -> Result<(), String> {
    eprintln!("brick-controller {VERSION}");
    let canvas = Canvas::open("/dev/fb0").map_err(|e| format!("framebuffer: {e}"))?;
    let theme = Theme::load();
    let fonts = Fonts::load(&crate::ui::font_paths());
    let mut pad = Pad::open(&pad_path()).map_err(|e| format!("pad: {e}"))?;
    let screen = Renderer::spawn(canvas, fonts, theme);
    // Refuse a model with no layout before touching Bluetooth: the Smart Pro
    // shares tg5040 with the Brick but has a 1280x720 panel, and a cropped
    // controller screen would hide the reason nothing works.
    if !Model::current().is_supported() {
        show_unsupported(&screen, &mut pad);
        return Ok(());
    }
    let mut profile = saved_host()
        .map(profile_for_host)
        .unwrap_or_else(fallback_profile);
    let mut start = StartScreen::Splash;
    // No host is reached until the user asks for it from Devices.
    let mut connect = None;
    loop {
        save_mode(profile);
        match session(profile, start, connect, &screen, &mut pad)? {
            SessionOutcome::Quit => break,
            SessionOutcome::SwitchProfile {
                profile: next,
                connect: next_connect,
                pairing,
            } => {
                profile = next;
                connect = next_connect;
                eprintln!("profile: {}", profile.label());
                // A type chosen from Pairing must reopen Pairing so the new
                // identity can be paired; a connect with a different profile
                // reopens Devices and then connects to the host.
                start = start_after_switch(pairing);
            }
            SessionOutcome::Retry => {
                connect = None;
                start = StartScreen::Devices;
            }
            SessionOutcome::Wired => {
                run_wired(&screen, &mut pad)?;
                // The wired mode did not change the Bluetooth profile or the
                // last host; Devices reopens with a fresh Bluetooth session.
                connect = None;
                start = StartScreen::Restarting;
            }
        }
    }
    drop(screen);
    Ok(())
}

/// Explain that this model has no layout yet and wait for B.
///
/// This runs before any Bluetooth takeover, so the handheld keeps NextUI's
/// daemon and the user gets an actionable message instead of a cropped screen.
fn show_unsupported(screen: &Renderer, pad: &mut Pad) {
    let lang = Lang::En;
    let mut previous = pad.mapper.state;
    screen.show(Frame {
        lang,
        view: View::Unsupported {
            title: tr(lang, Key::UnsupportedDeviceTitle).into(),
            message: tr(lang, Key::UnsupportedDeviceMessage).into(),
        },
        status: StatusInfo::read(false),
        modal: None,
        hold: None,
    });
    loop {
        let mut fd = libc::pollfd {
            fd: pad.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut fd, 1, 200) };
        if fd.revents & libc::POLLIN != 0 && pad.read().is_ok() {
            let now = pad.mapper.state;
            let b = now.pressed(Button::South) && !previous.pressed(Button::South);
            previous = now;
            if b {
                return;
            }
        }
    }
}
