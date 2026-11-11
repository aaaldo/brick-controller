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
    None => "0.1.0",
};

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
    let mut start_in_menu = false;
    // No host is reached until the user asks for it from Devices.
    let mut connect = None;
    loop {
        save_mode(profile);
        match session(profile, start_in_menu, connect, &screen, &mut pad)? {
            SessionOutcome::Quit => break,
            SessionOutcome::SwitchProfile {
                profile: next,
                connect: next_connect,
            } => {
                profile = next;
                connect = next_connect;
                eprintln!("profile: {}", profile.label());
                // A profile change comes from the menu; reopen it across the
                // Bluetooth restart.
                start_in_menu = true;
            }
            SessionOutcome::Retry => {
                connect = None;
                start_in_menu = true;
            }
        }
    }
    drop(screen);
    Ok(())
}
