// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Reusable widgets: list rows, footer key pills, modals, loading bar and the
//! status pill. The screen composition lives in `screens.rs`.

use super::theme::{hint_text, key_text, mix, outer_style, Theme};
use super::{circle, round_rect, s, stroke, HINT_SIZE, ITEM_SIZE, KEY_SIZE, ROW_H, SMALL_SIZE};
use crate::i18n::{tr, Key, Lang};
use crate::platform::StatusInfo;
use brick::canvas::{Canvas, Rgb};
use brick::text::Fonts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub trailing: String,
    pub selected: bool,
}

impl Row {
    pub fn new(label: impl Into<String>, trailing: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            trailing: trailing.into(),
            selected,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterItem {
    pub key: &'static str,
    pub label: &'static str,
}

pub const fn item(key: &'static str, label: &'static str) -> FooterItem {
    FooterItem { key, label }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modal {
    pub title: String,
    pub message: String,
    pub confirm: &'static str,
    pub destructive: bool,
    /// Indeterminate loading popup.
    pub loading: bool,
}

impl Modal {
    /// A blocking popup with an indeterminate bar and no actions.
    ///
    /// The session changes it when the work finishes; the user cannot dismiss
    /// it. Used for every in-app transition that must not be interrupted.
    pub fn loading(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: String::new(),
            confirm: "",
            destructive: false,
            loading: true,
        }
    }
}

/// Draw the visible rows as one continuous list.
///
/// Scrolling is decided by `view_for`, which keeps the selected row in a
/// five-row window. The renderer only lays out what it is given, so it stays
/// independent of host counts and cursor movement.
pub(super) fn rows_at(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    rows: &[Row],
    top: i32,
) {
    let left = s(canvas, 30);
    let width = canvas.width() - left * 2;
    let row_h = s(canvas, ROW_H);
    for (i, row) in rows.iter().enumerate() {
        let y = s(canvas, top) + i as i32 * row_h;
        if row.selected {
            round_rect(canvas, left, y, width, row_h, row_h / 2, theme.highlight);
        }
        let colour = if row.selected {
            theme.highlighted_text
        } else {
            theme.text
        };
        let trailing_colour = if row.selected {
            theme.highlighted_text
        } else {
            hint_text(theme)
        };
        let mut label_w = width - s(canvas, 60);
        if !row.trailing.is_empty() {
            label_w -= fonts.measure(&row.trailing, SMALL_SIZE) + s(canvas, 30);
        }
        let label = fonts.ellipsize(&row.label, ITEM_SIZE, label_w.max(1));
        fonts.draw(
            canvas,
            &label,
            left + s(canvas, 30),
            y + row_h / 2 + s(canvas, 16),
            ITEM_SIZE,
            colour,
        );
        if !row.trailing.is_empty() {
            let tw = fonts.measure(&row.trailing, SMALL_SIZE);
            fonts.draw(
                canvas,
                &row.trailing,
                left + width - s(canvas, 30) - tw,
                y + row_h / 2 + s(canvas, 10),
                SMALL_SIZE,
                trailing_colour,
            );
        }
    }
}

/// Draw the bottom action bar: one group flush left, one flush right.
///
/// Each group is a single pill containing key pills and labels, which is how
/// NextUI groups related actions. Empty groups are omitted rather than drawn
/// as blank pills.
pub(super) fn footer(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    left: &[FooterItem],
    right: &[FooterItem],
) {
    let margin = s(canvas, 30);
    let y = canvas.height() - margin - s(canvas, 90);
    if !left.is_empty() {
        draw_group(canvas, fonts, theme, left, margin, y, false);
    }
    if !right.is_empty() {
        let w = group_width(canvas, fonts, right);
        draw_group(
            canvas,
            fonts,
            theme,
            right,
            canvas.width() - margin - w,
            y,
            true,
        );
    }
}

pub(super) fn group_width(canvas: &Canvas, fonts: &mut Fonts, items: &[FooterItem]) -> i32 {
    let gap = s(canvas, 15);
    let outer = s(canvas, 15);
    let mut width = outer * 2;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            width += gap;
        }
        width += key_width(canvas, fonts, item.key) + gap;
        width += fonts.measure(item.label, HINT_SIZE) + gap;
    }
    width
}

pub(super) fn key_width(canvas: &Canvas, fonts: &mut Fonts, key: &str) -> i32 {
    if key.chars().count() == 1 {
        s(canvas, 60)
    } else {
        fonts.measure(key, KEY_SIZE) + s(canvas, 30)
    }
}

/// Draw one key pill and return its width.
///
/// Single-character keys get a fixed square-ish width, while names such as
/// "Menu" and "Start" are measured from the font. The baseline is computed
/// from real glyph bounds: the NextUI face and the embedded fallback have
/// different ascent/descent, so a fixed offset would centre one and not the
/// other.
pub(super) fn draw_key(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    key: &str,
    x: i32,
    y: i32,
) -> i32 {
    let width = key_width(canvas, fonts, key);
    let size = if key.chars().count() == 1 {
        HINT_SIZE
    } else {
        KEY_SIZE
    };
    round_rect(
        canvas,
        x,
        y,
        width,
        s(canvas, 60),
        s(canvas, 30),
        theme.highlight,
    );
    let baseline = fonts.centred_baseline(key, size, y + s(canvas, 30));
    fonts.draw_centred(canvas, key, x + width / 2, baseline, size, key_text(theme));
    width
}

pub(super) fn draw_group(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    items: &[FooterItem],
    x: i32,
    y: i32,
    _right: bool,
) -> i32 {
    let width = group_width(canvas, fonts, items);
    let h = s(canvas, 90);
    let outer = outer_style(theme);
    round_rect(canvas, x, y, width, h, h / 2, outer.fill);
    let gap = s(canvas, 15);
    let mut cx = x + gap;
    for item in items {
        cx += draw_key(canvas, fonts, theme, item.key, cx, y + gap) + gap;
        fonts.draw(
            canvas,
            item.label,
            cx,
            y + s(canvas, 45) + s(canvas, 12),
            HINT_SIZE,
            outer.text,
        );
        cx += fonts.measure(item.label, HINT_SIZE) + gap;
    }
    width
}

/// Draw a blocking modal over a dimmed screen.
///
/// A loading modal owns the screen until the session changes it: it draws no
/// actions, because the only way out is the connection succeeding or its
/// timeout expiring. A confirmation modal draws Cancel and Confirm as a pair,
/// with the destructive action on the right.
pub(super) fn draw_modal(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    modal: &Modal,
    phase: f32,
) {
    let w = canvas.width();
    let h = canvas.height();
    canvas.blend_rect(0, 0, w, h, Rgb(0, 0, 0), 180);
    let box_w = s(canvas, 780);
    let surface = mix(theme.background, theme.text, 0.07);

    if modal.loading {
        let box_h = s(canvas, 280);
        let box_x = (w - box_w) / 2;
        let box_y = (h - box_h) / 2;
        round_rect(canvas, box_x, box_y, box_w, box_h, s(canvas, 30), surface);
        let title = fonts.ellipsize(&modal.title, ITEM_SIZE, box_w - s(canvas, 80));
        fonts.draw_centred(
            canvas,
            &title,
            w / 2,
            box_y + s(canvas, 120),
            ITEM_SIZE,
            theme.text,
        );
        let bar_w = s(canvas, 520);
        let bar_h = s(canvas, 18);
        loading_bar(
            canvas,
            theme,
            (w - bar_w) / 2,
            box_y + s(canvas, 190),
            bar_w,
            bar_h,
            Loading::Indeterminate(phase),
        );
        return;
    }

    let lines = fonts.wrap(&modal.message, SMALL_SIZE, box_w - s(canvas, 80), 3);
    let line_h = s(canvas, 44);
    let box_h = s(canvas, 170) + lines.len() as i32 * line_h + s(canvas, 110);
    let box_x = (w - box_w) / 2;
    let box_y = (h - box_h) / 2;
    round_rect(canvas, box_x, box_y, box_w, box_h, s(canvas, 30), surface);
    fonts.draw_centred(
        canvas,
        &modal.title,
        w / 2,
        box_y + s(canvas, 74),
        ITEM_SIZE,
        theme.text,
    );
    for (i, line) in lines.iter().enumerate() {
        fonts.draw_centred(
            canvas,
            line,
            w / 2,
            box_y + s(canvas, 126) + i as i32 * line_h,
            SMALL_SIZE,
            hint_text(theme),
        );
    }
    let confirm = if modal.confirm.is_empty() {
        tr(lang, Key::Confirm)
    } else {
        modal.confirm
    };
    let cancel = [item("B", tr(lang, Key::Cancel))];
    let accept = [item("A", confirm)];
    let gap = s(canvas, 15);
    let cancel_w = group_width(canvas, fonts, &cancel);
    let actions_w = cancel_w + gap + group_width(canvas, fonts, &accept);
    let actions_x = (w - actions_w) / 2;
    let actions_y = box_y + box_h - s(canvas, 116);
    draw_group(canvas, fonts, theme, &cancel, actions_x, actions_y, false);
    draw_group(
        canvas,
        fonts,
        theme,
        &accept,
        actions_x + cancel_w + gap,
        actions_y,
        true,
    );
    let _ = modal.destructive;
}

#[derive(Clone, Copy)]
pub(super) enum Loading {
    /// Known duration: the main-colour fill grows from the left.
    Determinate(f32),
    /// Unknown duration: a main-colour segment sweeps back and forth.
    Indeterminate(f32),
}

/// The single loading indicator, shared by the splash, the connecting popup
/// and the disconnect hold.
///
/// The fill is always `theme.highlight` (NextUI's color1). The track is a
/// low-contrast mix of background and text rather than a fixed grey, so it
/// remains visible on both light and dark themes.
pub(super) fn loading_bar(
    canvas: &mut Canvas,
    theme: &Theme,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    loading: Loading,
) {
    let track = mix(theme.background, theme.text, 0.15);
    round_rect(canvas, x, y, width, height, height / 2, track);
    match loading {
        Loading::Determinate(progress) => {
            let filled = (width as f32 * progress.clamp(0.0, 1.0)) as i32;
            if filled > 0 {
                round_rect(
                    canvas,
                    x,
                    y,
                    filled.max(height),
                    height,
                    height / 2,
                    theme.highlight,
                );
            }
        }
        Loading::Indeterminate(phase) => {
            let segment = (width as f32 * 0.32) as i32;
            let travel = (width - segment).max(1);
            let t = (phase.rem_euclid(1.0) * 2.0).min(2.0);
            let t = if t <= 1.0 { t } else { 2.0 - t };
            let segment_x = x + (travel as f32 * t) as i32;
            round_rect(
                canvas,
                segment_x,
                y,
                segment,
                height,
                height / 2,
                theme.highlight,
            );
        }
    }
}

/// Overlay shown while Menu+Start is held to disconnect.
///
/// The progression is determinate because the hold has a known duration; the
/// HID report is already neutral at this point, so the overlay is purely a
/// visual confirmation that the chord is doing something.
pub(super) fn draw_hold(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    progress: f32,
) {
    let w = canvas.width();
    let h = canvas.height();
    canvas.blend_rect(0, 0, w, h, Rgb(0, 0, 0), 150);
    let box_w = s(canvas, 820);
    let box_h = s(canvas, 260);
    let box_x = (w - box_w) / 2;
    let box_y = (h - box_h) / 2;
    round_rect(
        canvas,
        box_x,
        box_y,
        box_w,
        box_h,
        s(canvas, 30),
        mix(theme.background, theme.text, 0.07),
    );
    fonts.draw_centred(
        canvas,
        tr(lang, Key::HoldTitle),
        w / 2,
        box_y + s(canvas, 82),
        ITEM_SIZE,
        theme.text,
    );
    fonts.draw_centred(
        canvas,
        tr(lang, Key::HoldMessage),
        w / 2,
        box_y + s(canvas, 140),
        SMALL_SIZE,
        hint_text(theme),
    );
    let bar_w = s(canvas, 700);
    let bar_h = s(canvas, 18);
    loading_bar(
        canvas,
        theme,
        (w - bar_w) / 2,
        box_y + s(canvas, 194),
        bar_w,
        bar_h,
        Loading::Determinate(progress),
    );
}

/// Draw the right-aligned status pill: link, Wi-Fi, battery and clock.
///
/// Only the indicators that are actually known are drawn. The pill width is
/// measured first so the header title can be ellipsized against the remaining
/// space instead of running underneath it.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_status(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    status: &StatusInfo,
) {
    let width = status_bar_width(canvas, fonts, status);
    let margin = s(canvas, 30);
    let h = s(canvas, 90);
    let y = s(canvas, 15);
    let x = canvas.width() - margin - width;
    round_rect(canvas, x, y, width, h, h / 2, theme.accent);
    let colour = theme.hint;
    let mut cx = x + s(canvas, 24);
    let cy = y + h / 2;
    if status.bluetooth {
        draw_bt(canvas, cx + s(canvas, 20), cy, colour);
        cx += s(canvas, 40) + s(canvas, 18);
    }
    if status.wifi {
        draw_wifi(canvas, cx + s(canvas, 20), cy, colour);
        cx += s(canvas, 40) + s(canvas, 18);
    }
    if let Some(level) = status.battery {
        draw_battery(
            canvas,
            cx,
            cy,
            level,
            status.charging,
            colour,
            mix(colour, Rgb(0, 0, 0), 0.55),
        );
    }
    let clock_w = fonts.measure(&status.clock, HINT_SIZE);
    fonts.draw(
        canvas,
        &status.clock,
        canvas.width() - margin - s(canvas, 24) - clock_w,
        cy + s(canvas, 12),
        HINT_SIZE,
        colour,
    );
}

pub(super) fn status_bar_width(canvas: &Canvas, fonts: &mut Fonts, status: &StatusInfo) -> i32 {
    let gap = s(canvas, 18);
    let mut width = s(canvas, 24) * 2;
    if status.bluetooth {
        width += s(canvas, 40) + gap;
    }
    if status.wifi {
        width += s(canvas, 40) + gap;
    }
    if status.battery.is_some() {
        width += s(canvas, 44) + gap;
    }
    width + fonts.measure(&status.clock, HINT_SIZE)
}

fn draw_bt(canvas: &mut Canvas, cx: i32, cy: i32, colour: Rgb) {
    let half = s(canvas, 16);
    stroke(
        canvas,
        (cx, cy - half),
        (cx, cy + half),
        s(canvas, 3),
        colour,
    );
    stroke(
        canvas,
        (cx, cy - half),
        (cx + half * 2 / 3, cy - half / 3),
        s(canvas, 3),
        colour,
    );
    stroke(
        canvas,
        (cx + half * 2 / 3, cy - half / 3),
        (cx, cy + half / 3),
        s(canvas, 3),
        colour,
    );
    stroke(
        canvas,
        (cx, cy + half),
        (cx + half * 2 / 3, cy + half / 3),
        s(canvas, 3),
        colour,
    );
    stroke(
        canvas,
        (cx + half * 2 / 3, cy + half / 3),
        (cx, cy - half / 3),
        s(canvas, 3),
        colour,
    );
}

fn draw_wifi(canvas: &mut Canvas, cx: i32, cy: i32, colour: Rgb) {
    circle(canvas, cx, cy + s(canvas, 12), s(canvas, 4), colour);
    for radius in [s(canvas, 10), s(canvas, 18), s(canvas, 26)] {
        let steps = 18;
        for i in 0..=steps {
            let angle = (-135.0 + 90.0 * i as f32 / steps as f32).to_radians();
            let x = cx + (angle.cos() * radius as f32) as i32;
            let y = cy + s(canvas, 12) + (angle.sin() * radius as f32) as i32;
            circle(canvas, x, y, s(canvas, 2), colour);
        }
    }
}

fn draw_battery(
    canvas: &mut Canvas,
    x: i32,
    cy: i32,
    level: u8,
    charging: bool,
    colour: Rgb,
    empty: Rgb,
) {
    let w = s(canvas, 40);
    let h = s(canvas, 22);
    let y = cy - h / 2;
    round_rect(canvas, x, y, w, h, s(canvas, 4), colour);
    round_rect(
        canvas,
        x + s(canvas, 3),
        y + s(canvas, 3),
        w - s(canvas, 6),
        h - s(canvas, 6),
        s(canvas, 2),
        empty,
    );
    let fill = (w - s(canvas, 6)) as f32 * level as f32 / 100.0;
    if fill > 0.0 {
        round_rect(
            canvas,
            x + s(canvas, 3),
            y + s(canvas, 3),
            fill as i32,
            h - s(canvas, 6),
            s(canvas, 2),
            colour,
        );
    }
    round_rect(
        canvas,
        x + w,
        cy - s(canvas, 6),
        s(canvas, 4),
        s(canvas, 12),
        s(canvas, 2),
        colour,
    );
    if charging {
        let bx = x + w / 2;
        stroke(
            canvas,
            (bx - s(canvas, 4), cy - s(canvas, 6)),
            (bx + s(canvas, 4), cy),
            s(canvas, 2),
            colour,
        );
    }
}
