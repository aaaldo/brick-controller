// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The renderer thread and the frame it receives.
//!
//! Drawing a frame takes long enough on this CPU that doing it between reading
//! the pad and sending the report put every redraw in the input path. The
//! session therefore only posts the latest frame here; a frame still drawing
//! when several more arrive is followed by the newest only.

use super::{draw, Modal, Theme, View};
use crate::platform::StatusInfo;
use brick::canvas::Canvas;
use brick::text::Fonts;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What the screen should show next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub view: View,
    pub status: StatusInfo,
    pub modal: Option<Modal>,
    /// Chord progress, quantised so an unchanged hold does not redraw.
    pub hold: Option<u8>,
}

impl Frame {
    /// Splash and connection popup use an indeterminate bar; the renderer
    /// keeps redrawing them so its segment moves.
    fn animates(&self) -> bool {
        matches!(&self.view, View::Splash { .. })
            || self.modal.as_ref().is_some_and(|modal| modal.loading)
    }
}

/// The screen, drawn on a thread of its own.
pub struct Renderer {
    slot: Arc<(Mutex<Option<Option<Frame>>>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The shortest time between two frames. The screen is a status display; the
/// gamepad is what has to be quick. Twenty frames a second keeps the pressed
/// controls readable while leaving the CPU mostly to the HID loop.
const FRAME_EVERY: Duration = Duration::from_millis(50);
/// Redraw cadence for an indeterminate loading bar.
const ANIMATION_EVERY: Duration = Duration::from_millis(120);
/// One complete back-and-forth sweep of the indeterminate segment.
const LOADING_PHASE: Duration = Duration::from_millis(1500);

/// `gettid` is Linux-only; on a host build the process id is enough for the
/// priority call, which only ever runs on the device anyway.
#[cfg(target_os = "linux")]
fn current_tid() -> i32 {
    unsafe { libc::syscall(libc::SYS_gettid) as i32 }
}

#[cfg(not(target_os = "linux"))]
fn current_tid() -> i32 {
    unsafe { libc::getpid() }
}

fn draw_frame(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    frame: &Frame,
    phase: f32,
    drawn: &mut u32,
) {
    let started = Instant::now();
    draw(
        canvas,
        fonts,
        theme,
        &frame.view,
        &frame.status,
        frame.modal.as_ref(),
        frame.hold.map(|step| step as f32 / 20.0),
        phase,
    );
    let _ = canvas.flush();
    // A few, to know what a frame costs on the device.
    *drawn += 1;
    if *drawn <= 5 {
        eprintln!(
            "screen: frame {} took {} ms",
            *drawn,
            started.elapsed().as_millis()
        );
    }
}

impl Renderer {
    pub fn spawn(mut canvas: Canvas, mut fonts: Fonts, theme: Theme) -> Self {
        let slot: Arc<(Mutex<Option<Option<Frame>>>, Condvar)> = Arc::default();
        let shared = Arc::clone(&slot);
        let thread = std::thread::spawn(move || {
            // Below the I/O thread, so a frame never delays a report.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, current_tid() as _, 10) };
            let (lock, ready) = &*shared;
            let mut drawn = 0u32;
            let mut current: Option<Frame> = None;
            let animation_start = Instant::now();
            loop {
                let next = {
                    let mut pending = lock.lock().unwrap();
                    if pending.is_none() {
                        if current.as_ref().is_some_and(Frame::animates) {
                            let (guard, _) = ready.wait_timeout(pending, ANIMATION_EVERY).unwrap();
                            pending = guard;
                            if pending.is_none() {
                                drop(pending);
                                if let Some(frame) = current.as_ref() {
                                    let phase = (animation_start.elapsed().as_secs_f32()
                                        / LOADING_PHASE.as_secs_f32())
                                    .fract();
                                    draw_frame(
                                        &mut canvas,
                                        &mut fonts,
                                        &theme,
                                        frame,
                                        phase,
                                        &mut drawn,
                                    );
                                }
                                continue;
                            }
                        } else {
                            while pending.is_none() {
                                pending = ready.wait(pending).unwrap();
                            }
                        }
                    }
                    pending.take().unwrap()
                };
                let started = Instant::now();
                match next {
                    None => {
                        canvas.clear(theme.background);
                        let _ = canvas.flush();
                        return;
                    }
                    Some(frame) => {
                        let phase = (animation_start.elapsed().as_secs_f32()
                            / LOADING_PHASE.as_secs_f32())
                        .fract();
                        draw_frame(&mut canvas, &mut fonts, &theme, &frame, phase, &mut drawn);
                        current = Some(frame);
                    }
                }
                if let Some(rest) = FRAME_EVERY.checked_sub(started.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        });
        Self {
            slot,
            thread: Some(thread),
        }
    }

    pub fn show(&self, frame: Frame) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(Some(frame));
        ready.notify_one();
    }
}

impl Drop for Renderer {
    /// Blank the screen and wait for it, so the launcher never comes back
    /// over a half-drawn frame.
    fn drop(&mut self) {
        let (lock, ready) = &*self.slot;
        *lock.lock().unwrap() = Some(None);
        ready.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
