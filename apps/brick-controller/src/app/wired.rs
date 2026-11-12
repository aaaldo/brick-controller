// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The wired PC session: USB HID instead of Bluetooth.
//!
//! This is a sibling of `session.rs`, not a branch inside it. The Bluetooth
//! session is stopped before this module runs and restarted after it, so the
//! wired path has no access to `bluez`, hosts or the L2CAP channels. It owns
//! the panel timer, the Menu+Start disconnect and the neutral-report rules
//! that the Bluetooth loop implements for its own screens.
//!
//! Only the Linux build compiles this module: the transport repurposes the
//! ADB gadget and mounts FunctionFS.

use super::view_model::{wired_controller, wired_waiting};
use super::{DISCONNECT_HOLD, SCREEN_OFF_AFTER};
use crate::i18n::{tr, Key, Lang};
use crate::input::{Button, PadState};
use crate::platform::display;
use crate::platform::input_trimui::Pad;
use crate::platform::StatusInfo;
use crate::profiles::{generic_hid, Schedule};
use crate::ui::{Frame, Renderer, View};
use crate::usb::{Event, UsbHid};
use std::os::unix::io::AsRawFd;
use std::time::Instant;

/// The two screens the wired session can show. Host errors are handled by the
/// outer retry loop, not by a third variant.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WiredScreen {
    Waiting,
    Connected,
}

/// The parts of a frame that decide whether a redraw is needed.
type DrawKey = (View, StatusInfo, Option<u8>);

pub(crate) fn session(screen: &Renderer, pad: &mut Pad) -> Result<(), String> {
    loop {
        match run(screen, pad) {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("wired: {error}");
                if !show_error(screen, pad) {
                    return Ok(());
                }
            }
        }
    }
}

/// Run one wired attempt: take over `g1`, then serve the host until the user
/// leaves or the transport fails.
fn run(screen: &Renderer, pad: &mut Pad) -> Result<(), String> {
    let lang = Lang::En;
    let mut backlight = display::Backlight::open();
    screen.show(Frame {
        lang,
        view: wired_waiting(),
        status: StatusInfo::read(true),
        modal: None,
        hold: None,
    });

    let mut usb = UsbHid::take_over(generic_hid::NAME, generic_hid::DESCRIPTOR)
        .map_err(|error| format!("USB activation: {error}"))?;

    // The PC profile declares its cadence; reusing it keeps the USB report
    // rate identical to the Bluetooth one.
    let report_gap = match generic_hid::SPEC.schedule {
        Schedule::OnChange { min_gap } => min_gap,
        Schedule::Periodic { every } => every,
    };

    let mut previous = pad.mapper.state;
    let mut last_sent: Option<[u8; generic_hid::REPORT_LEN]> = None;
    let mut sent_at = Instant::now();
    let mut enabled = false;
    let mut state = WiredScreen::Waiting;
    let mut screen_off = false;
    let mut screen_off_at: Option<Instant> = None;
    let mut disconnect_since: Option<Instant> = None;
    let mut combo_seen = false;
    let mut wake_used = false;
    // The A that opened this mode may still be held; the host must not see it
    // as a fresh gamepad press when the interface comes up.
    let mut resume_after_release = pad.mapper.state != PadState::default();
    let mut drawn: Option<DrawKey> = None;

    loop {
        let pending = enabled && last_sent != Some(generic_hid::report(&pad.mapper.state));
        // A dark panel saves the animation, not the report cadence: an active
        // interface still wakes on input and honours the profile gap.
        let wait = if pending {
            report_gap
                .saturating_sub(sent_at.elapsed())
                .as_millis()
                .max(1) as i32
        } else if enabled {
            100
        } else if screen_off {
            250
        } else {
            100
        };

        let mut fds = [
            libc::pollfd {
                fd: pad.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: usb.ep0_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, wait) };
        if rc < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("poll: {error}"));
        }

        if fds[0].revents & libc::POLLIN != 0 {
            pad.read().map_err(|error| format!("pad: {error}"))?;
        }
        // A quick Menu+Start tap may be released before this poll.
        let combo_tapped = pad.mapper.take_combo_tap();

        loop {
            match usb
                .read_event()
                .map_err(|error| format!("USB event: {error}"))?
            {
                Some(Event::Enable) => {
                    eprintln!("wired: USB host enabled");
                    enabled = true;
                    state = WiredScreen::Connected;
                    screen_off_at = Some(Instant::now() + SCREEN_OFF_AFTER);
                    if screen_off {
                        backlight.on();
                        screen_off = false;
                    }
                    // The host configured the interface: a neutral report comes
                    // before any real state, even if a button is still held.
                    let neutral = generic_hid::report(&PadState::default());
                    usb.send_report(&neutral)
                        .map_err(|error| format!("USB report: {error}"))?;
                    last_sent = Some(neutral);
                    sent_at = Instant::now();
                    drawn = None;
                }
                Some(Event::Disable) => {
                    eprintln!("wired: USB host disabled");
                    enabled = false;
                    state = WiredScreen::Waiting;
                    screen_off_at = None;
                    disconnect_since = None;
                    combo_seen = false;
                    wake_used = false;
                    last_sent = None;
                    if screen_off {
                        backlight.on();
                        screen_off = false;
                    }
                    drawn = None;
                }
                Some(Event::Setup(setup)) => {
                    // Answer a control read with the report the host already
                    // owns, not the raw pad: during enumeration the A used to
                    // open this mode may still be held.
                    let report =
                        last_sent.unwrap_or_else(|| generic_hid::report(&PadState::default()));
                    usb.handle_setup(&setup, &report)
                        .map_err(|error| format!("USB control: {error}"))?;
                }
                Some(Event::Bind | Event::Unbind | Event::Suspend | Event::Resume) => {}
                None => break,
            }
        }

        let now = pad.mapper.state;
        let b_pressed = now.pressed(Button::South) && !previous.pressed(Button::South);
        let combos = now.pressed(Button::Home) && now.pressed(Button::Start);
        let combo_was = previous.pressed(Button::Home) && previous.pressed(Button::Start);

        // Leaving the waiting screen is the only interface action here; the
        // connected screen belongs to the host except for the disconnect hold.
        if b_pressed && state == WiredScreen::Waiting {
            return Ok(());
        }

        // Interacting with the chord keeps the panel on while it is held,
        // except for the hold that just woke it: that one leaves the
        // ten-second cycle armed.
        if !screen_off && combos && !wake_used {
            screen_off_at = None;
        }

        if screen_off {
            if combos || combo_tapped {
                combo_seen = true;
                if !combo_was {
                    backlight.on();
                    screen_off = false;
                    wake_used = true;
                    screen_off_at = Some(Instant::now() + SCREEN_OFF_AFTER);
                    drawn = None;
                }
            } else {
                combo_seen = false;
            }
        } else if !wake_used {
            if combos {
                combo_seen = true;
                let since = disconnect_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= DISCONNECT_HOLD && state == WiredScreen::Connected {
                    if enabled {
                        let neutral = generic_hid::report(&PadState::default());
                        let _ = usb.send_report(&neutral);
                    }
                    return Ok(());
                }
            } else {
                disconnect_since = None;
            }
            if !combos {
                combo_seen = false;
            }
        }
        if !combos {
            wake_used = false;
        }

        if !screen_off
            && state == WiredScreen::Connected
            && enabled
            && screen_off_at.is_some_and(|deadline| Instant::now() >= deadline)
        {
            backlight.off();
            screen_off = true;
            screen_off_at = None;
            drawn = None;
        }

        // The pad belongs to the host once the interface is up. Interface
        // buttons and the disconnect chord stay neutral, and so does the
        // period until every button used before the link came up is released.
        if state == WiredScreen::Waiting {
            resume_after_release = true;
        }
        if resume_after_release && now == PadState::default() {
            resume_after_release = false;
        }
        let report_state = if state == WiredScreen::Waiting || combo_seen || resume_after_release {
            PadState::default()
        } else {
            now
        };

        if enabled {
            let report = generic_hid::report(&report_state);
            if last_sent != Some(report) && sent_at.elapsed() >= report_gap {
                usb.send_report(&report)
                    .map_err(|error| format!("USB report: {error}"))?;
                last_sent = Some(report);
                sent_at = Instant::now();
            }
        }

        if screen_off {
            previous = now;
            continue;
        }

        let status = StatusInfo::read(true);
        let view = match state {
            WiredScreen::Waiting => wired_waiting(),
            WiredScreen::Connected => wired_controller(lang),
        };
        let hold = if combo_seen && state == WiredScreen::Connected {
            disconnect_since.map(|since| {
                ((since.elapsed().as_secs_f32() / DISCONNECT_HOLD.as_secs_f32()).clamp(0.0, 1.0)
                    * 20.0) as u8
            })
        } else {
            None
        };
        let key = (view.clone(), status.clone(), hold);
        if drawn.as_ref() != Some(&key) {
            screen.show(Frame {
                lang,
                view,
                status,
                modal: None,
                hold,
            });
            drawn = Some(key);
        }

        previous = now;
    }
}

/// Show the USB recovery screen until the user retries or goes back.
///
/// The technical error stays in the log; the screen shows the same actionable
/// message as a Bluetooth takeover failure. `true` means retry, `false` means
/// return to Devices.
fn show_error(screen: &Renderer, pad: &mut Pad) -> bool {
    let lang = Lang::En;
    let mut previous = pad.mapper.state;
    screen.show(Frame {
        lang,
        view: View::Error {
            title: tr(lang, Key::ErrorTitle).into(),
            message: tr(lang, Key::UsbActivationError).into(),
            back: true,
        },
        status: StatusInfo::read(true),
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
            let retry = now.pressed(Button::East) && !previous.pressed(Button::East);
            let back = now.pressed(Button::South) && !previous.pressed(Button::South);
            previous = now;
            if retry {
                return true;
            }
            if back {
                return false;
            }
        }
    }
}
