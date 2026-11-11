// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The NextUI-like screens: theme, status bar, list rows, footers, dialogs.
//!
//! The layout follows Apostrophe's NextUI metrics (MIT): a 1024-wide reference,
//! a 3x device scale on the Brick, 30 px screen padding, 90 px pills, a 120 px
//! header/footer and the NextUI colour keys from `nextval.elf`.
//!
//! The submodules are `theme` (colours), `widgets` (reusable pieces),
//! `screens` (composition), `renderer` (the drawing thread) and `preview`
//! (test-only snapshots). The platform readings (battery, Wi-Fi, clock) live
//! in `platform/status.rs`; the UI only draws the resulting `StatusInfo`.

#[cfg(test)]
mod preview;
pub mod renderer;
pub mod screens;
pub mod theme;
pub mod widgets;

pub use renderer::{Frame, Renderer};
pub use screens::{draw, View};
pub use theme::{font_paths, Theme};
pub use widgets::{Modal, Row};

use brick::canvas::{Canvas, Rgb};
use std::collections::HashMap;

pub const ERROR: Rgb = Rgb(0xfb, 0x49, 0x34);

const HINT_SIZE: f32 = 36.0;
const SMALL_SIZE: f32 = 30.0;
const ITEM_SIZE: f32 = 48.0;
const KEY_SIZE: f32 = 27.0;
const TITLE_SIZE: f32 = 72.0;
const ROW_H: i32 = 90;
const SCREEN_OFF_HINT: &str =
    "Screen will switch off after 10s. Press Menu+Start once to wake it up";

/// Scale a layout value from the 1024-wide reference to the actual panel.
///
/// Every metric in the UI is written against 1024×768, which is the Brick's
/// native resolution; the helper keeps the same layout usable if a future
/// model has a different width. Heights are derived from the same factor so
/// pills stay circular rather than elliptical.
fn s(canvas: &Canvas, value: i32) -> i32 {
    (value as f32 * canvas.width() as f32 / 1024.0).round() as i32
}

fn round_rect_mask(w: i32, h: i32, r: i32) -> Vec<u8> {
    let (w, h) = (w.max(1), h.max(1));
    let r = (r as f32).min(w as f32 / 2.0).min(h as f32 / 2.0);
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut mask = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let qx = ((x as f32 + 0.5 - hw).abs() - (hw - r)).max(0.0);
            let qy = ((y as f32 + 0.5 - hh).abs() - (hh - r)).max(0.0);
            let d = (qx * qx + qy * qy).sqrt() - r;
            mask.push(((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    mask
}

/// Fill a rounded rectangle, caching its antialiasing mask by size and radius.
///
/// Rasterising the mask on every frame would cost more than the rest of the
/// drawing on this CPU. The cache is per thread because the renderer owns one
/// thread and `Canvas` is not shared across threads.
fn round_rect(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, r: i32, colour: Rgb) {
    thread_local! {
        static MASKS: std::cell::RefCell<HashMap<(i32, i32, i32), Vec<u8>>> = std::cell::RefCell::new(HashMap::new());
    }
    MASKS.with(|masks| {
        let mut masks = masks.borrow_mut();
        let mask = masks
            .entry((w, h, r))
            .or_insert_with(|| round_rect_mask(w, h, r));
        canvas.blend_mask(x, y, w.max(1) as usize, mask, colour);
    });
}

fn circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, colour: Rgb) {
    round_rect(canvas, cx - r, cy - r, 2 * r, 2 * r, r, colour);
}

fn stroke(
    canvas: &mut Canvas,
    (x0, y0): (i32, i32),
    (x1, y1): (i32, i32),
    width: i32,
    colour: Rgb,
) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        circle(
            canvas,
            x0 + (x1 - x0) * i / steps,
            y0 + (y1 - y0) * i / steps,
            width / 2,
            colour,
        );
    }
}
