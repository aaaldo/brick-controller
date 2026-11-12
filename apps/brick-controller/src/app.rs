// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The application shell: bootstrap the renderer and run one session after
//! another as the user switches profile or retries.

mod session;
mod settings;
#[cfg(test)]
mod tests;
mod view_model;

use crate::platform::input_trimui::{pad_path, Pad};
use crate::ui::{Renderer, Theme};
use brick::canvas::Canvas;
use brick::text::Fonts;
use session::{session, SessionOutcome};
use settings::{fallback_profile, profile_for_host, save_mode, saved_host};

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

pub(crate) fn run() -> Result<(), String> {
    eprintln!("brick-controller {VERSION}");
    let canvas = Canvas::open("/dev/fb0").map_err(|e| format!("framebuffer: {e}"))?;
    let theme = Theme::load();
    let fonts = Fonts::load(&crate::ui::font_paths());
    let mut pad = Pad::open(&pad_path()).map_err(|e| format!("pad: {e}"))?;
    let screen = Renderer::spawn(canvas, fonts, theme);
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
        }
    }
    drop(screen);
    Ok(())
}
