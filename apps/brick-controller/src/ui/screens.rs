// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Screen composition: the render model and the drawing order for each screen.

use super::theme::{hint_text, outer_style, Theme};
use super::widgets::{
    draw_hold, draw_key, draw_modal, draw_status, footer, item, key_width, loading_bar, rows_at,
    status_bar_width, Loading, Modal, Row,
};
use super::{circle, round_rect, s, ERROR, HINT_SIZE, ITEM_SIZE, SMALL_SIZE, TITLE_SIZE};
use crate::i18n::{fill, tr, Key, Lang};
use crate::platform::StatusInfo;
use brick::canvas::Canvas;
use brick::text::Fonts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Splash {
        message: String,
    },
    Controller {
        host: String,
        profile: &'static str,
    },
    Menu {
        rows: Vec<Row>,
    },
    Profile {
        rows: Vec<Row>,
    },
    Pairing {
        identity: String,
        controller_type: &'static str,
        reconnecting: Option<String>,
    },
    Error {
        title: String,
        message: String,
        steps: Vec<String>,
        countdown: Option<String>,
    },
}

/// Draw one frame. A modal or a hold overlay can sit on top of any screen.
///
/// Layer order is deliberate: background, then the view, then the blocking
/// modal, then the disconnect hold. A modal therefore hides the screen beneath
/// it without the caller having to clear or swap views.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    view: &View,
    status: &StatusInfo,
    modal: Option<&Modal>,
    hold: Option<f32>,
    phase: f32,
) {
    canvas.clear(theme.background);
    match view {
        View::Splash { message } => draw_splash(canvas, fonts, theme, lang, message, phase),
        View::Controller { host, profile } => {
            header(canvas, fonts, theme, status, tr(lang, Key::Connected));
            draw_controller(canvas, fonts, theme, lang, host, profile);
            disconnect_hint(canvas, fonts, theme, lang);
        }
        View::Menu { rows } => {
            header(canvas, fonts, theme, status, tr(lang, Key::Devices));
            rows_at(canvas, fonts, theme, rows, 120);
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", tr(lang, Key::Quit))],
                &[
                    item("X", tr(lang, Key::Forget)),
                    item("A", tr(lang, Key::Connect)),
                ],
            );
            if rows.is_empty() {
                fonts.draw(
                    canvas,
                    tr(lang, Key::NoPairedDevice),
                    s(canvas, 30),
                    s(canvas, 180),
                    SMALL_SIZE,
                    hint_text(theme),
                );
            }
        }
        View::Profile { rows } => {
            header(canvas, fonts, theme, status, tr(lang, Key::ControllerType));
            rows_at(canvas, fonts, theme, rows, 120);
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", tr(lang, Key::Cancel))],
                &[item("A", tr(lang, Key::Choose))],
            );
        }
        View::Pairing {
            identity,
            controller_type,
            reconnecting,
        } => {
            header(canvas, fonts, theme, status, tr(lang, Key::Pairing));
            draw_pairing(
                canvas,
                fonts,
                theme,
                lang,
                identity,
                controller_type,
                reconnecting.as_deref(),
                phase,
            );
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", tr(lang, Key::Back))],
                &[item("X", tr(lang, Key::ChangeControllerType))],
            );
        }
        View::Error {
            title,
            message,
            steps,
            countdown,
        } => {
            header(canvas, fonts, theme, status, title);
            draw_error(
                canvas,
                fonts,
                theme,
                lang,
                message,
                steps,
                countdown.as_deref(),
            );
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", tr(lang, Key::Quit))],
                &[item("A", tr(lang, Key::Retry))],
            );
        }
    }

    if let Some(modal) = modal {
        draw_modal(canvas, fonts, theme, lang, modal, phase);
    }
    if let Some(progress) = hold {
        draw_hold(canvas, fonts, theme, lang, progress);
    }
}

fn header(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, status: &StatusInfo, title: &str) {
    let margin = s(canvas, 30);
    let status_w = status_bar_width(canvas, fonts, status);
    let max_w = canvas.width() - margin * 2 - status_w - s(canvas, 20);
    let title = fonts.ellipsize(title, TITLE_SIZE, max_w.max(1));
    fonts.draw(
        canvas,
        &title,
        margin,
        s(canvas, 85),
        TITLE_SIZE,
        theme.text,
    );
    draw_status(canvas, fonts, theme, status);
}

fn draw_splash(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    message: &str,
    phase: f32,
) {
    let centre = canvas.width() / 2;
    fonts.draw_centred(
        canvas,
        tr(lang, Key::AppName),
        centre,
        canvas.height() / 2 - s(canvas, 34),
        TITLE_SIZE,
        theme.text,
    );
    fonts.draw_centred(
        canvas,
        message,
        centre,
        canvas.height() / 2 + s(canvas, 24),
        HINT_SIZE,
        hint_text(theme),
    );
    let bar_w = s(canvas, 420);
    let bar_h = s(canvas, 18);
    loading_bar(
        canvas,
        theme,
        centre - bar_w / 2,
        canvas.height() / 2 + s(canvas, 90),
        bar_w,
        bar_h,
        Loading::Indeterminate(phase),
    );
}

/// The connected view: who is linked, which identity it sees, and how to
/// disconnect or wake the panel.
///
/// It intentionally has no event list: the pad belongs to the host once the
/// link is up, and the screen only needs the host, the profile and the
/// disconnect hint.
fn draw_controller(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    host: &str,
    profile: &str,
) {
    let left = s(canvas, 30);
    let max_w = canvas.width() - left * 2;
    let top = canvas.height() / 2 - s(canvas, 120);
    let host = fonts.ellipsize(host, ITEM_SIZE, max_w);
    fonts.draw(canvas, &host, left, top, ITEM_SIZE, theme.text);
    fonts.draw(
        canvas,
        &fill(tr(lang, Key::ProfileLine), &[("profile", profile)]),
        left,
        top + s(canvas, 72),
        HINT_SIZE,
        hint_text(theme),
    );

    let mut y = top + s(canvas, 180);
    for line in fonts.wrap(tr(lang, Key::ScreenOffHint), HINT_SIZE, max_w, 3) {
        fonts.draw(canvas, &line, left, y, HINT_SIZE, hint_text(theme));
        y += s(canvas, 48);
    }
}

/// The pairing screen: what the host should see, and how to change it.
///
/// `identity` is the Bluetooth name the adapter advertises, while
/// `controller_type` is the user-facing profile label. They differ by design:
/// a host lists the emulated controller name, not the app's profile name.
#[allow(clippy::too_many_arguments)]
fn draw_pairing(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    identity: &str,
    controller_type: &str,
    reconnecting: Option<&str>,
    phase: f32,
) {
    let centre = canvas.width() / 2;
    let bar_w = s(canvas, 520);
    let bar_h = s(canvas, 18);
    let hint_line = s(canvas, HINT_SIZE as i32);
    let block_gap = s(canvas, 64);
    let reconnect_h = if reconnecting.is_some() {
        s(canvas, 44)
    } else {
        0
    };

    // Centre the block between the header and the footer. The identity is the
    // only large, main-colour line; the type and waiting labels share the same
    // hint size and colour. The waiting block starts 64 px below the type
    // label's line box, and the bar follows the waiting label.
    let header_bottom = s(canvas, 105);
    let footer_top = canvas.height() - s(canvas, 30) - s(canvas, 90);
    let type_baseline = s(canvas, 130);
    let lower_top = type_baseline + hint_line + block_gap;
    let block_h = lower_top + reconnect_h + s(canvas, 20) + bar_h;
    let start = (header_bottom + footer_top) / 2 - block_h / 2 + s(canvas, 12);

    fonts.draw_centred(
        canvas,
        tr(lang, Key::VisibleAs),
        centre,
        start,
        SMALL_SIZE,
        hint_text(theme),
    );
    fonts.draw_centred(
        canvas,
        identity,
        centre,
        start + s(canvas, 82),
        TITLE_SIZE,
        theme.highlight,
    );
    fonts.draw_centred(
        canvas,
        &fill(tr(lang, Key::ControllerLine), &[("type", controller_type)]),
        centre,
        start + type_baseline,
        HINT_SIZE,
        hint_text(theme),
    );

    let lower_y = start + lower_top;
    if let Some(host) = reconnecting {
        fonts.draw_centred(
            canvas,
            &fill(tr(lang, Key::ReconnectingTo), &[("host", host)]),
            centre,
            lower_y,
            SMALL_SIZE,
            hint_text(theme),
        );
    }
    let wait_y = lower_y + reconnect_h;
    fonts.draw_centred(
        canvas,
        tr(lang, Key::WaitingForHost),
        centre,
        wait_y,
        HINT_SIZE,
        hint_text(theme),
    );
    loading_bar(
        canvas,
        theme,
        centre - bar_w / 2,
        wait_y + s(canvas, 20),
        bar_w,
        bar_h,
        Loading::Indeterminate(phase),
    );
}

/// The recovery screen shown when Bluetooth cannot be taken over.
///
/// The numbered steps are deliberately actionable without a terminal: the app
/// auto-closes so NextUI can restore its own Bluetooth state, hence the
/// countdown at the bottom.
#[allow(clippy::too_many_arguments)]
fn draw_error(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    lang: Lang,
    message: &str,
    steps: &[String],
    countdown: Option<&str>,
) {
    let left = s(canvas, 30);
    let icon_x = left + s(canvas, 56);
    let icon_y = s(canvas, 208);
    circle(canvas, icon_x, icon_y, s(canvas, 56), ERROR);
    circle(canvas, icon_x, icon_y, s(canvas, 50), theme.background);
    fonts.draw_centred(
        canvas,
        "!",
        icon_x,
        icon_y + s(canvas, 28),
        TITLE_SIZE,
        ERROR,
    );
    fonts.draw(
        canvas,
        tr(lang, Key::BluetoothUnavailable),
        left + s(canvas, 140),
        icon_y - s(canvas, 8),
        ITEM_SIZE,
        ERROR,
    );
    fonts.draw(
        canvas,
        message,
        left + s(canvas, 140),
        icon_y + s(canvas, 34),
        SMALL_SIZE,
        hint_text(theme),
    );
    let mut y = s(canvas, 340);
    for (i, step) in steps.iter().enumerate() {
        fonts.draw(
            canvas,
            &format!("{}. {step}", i + 1),
            left,
            y,
            SMALL_SIZE,
            theme.text,
        );
        y += s(canvas, 50);
    }
    if let Some(countdown) = countdown {
        fonts.draw(
            canvas,
            countdown,
            left,
            canvas.height() - s(canvas, 154),
            SMALL_SIZE,
            hint_text(theme),
        );
    }
}

fn disconnect_hint(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, lang: Lang) {
    let margin = s(canvas, 30);
    let gap = s(canvas, 15);
    let pad = s(canvas, 15);
    let h = s(canvas, 90);
    let y = canvas.height() - margin - h;
    let label = tr(lang, Key::DisconnectHold);
    let plus = "+";
    let menu_w = key_width(canvas, fonts, "Menu");
    let start_w = key_width(canvas, fonts, "Start");
    let plus_w = fonts.measure(plus, HINT_SIZE);
    let width =
        pad * 2 + menu_w + gap + plus_w + gap + start_w + gap + fonts.measure(label, HINT_SIZE);
    let outer = outer_style(theme);
    round_rect(canvas, margin, y, width, h, h / 2, outer.fill);

    let mut x = margin + pad;
    x += draw_key(canvas, fonts, theme, "Menu", x, y + pad);
    x += gap;
    fonts.draw_centred(
        canvas,
        plus,
        x + plus_w / 2,
        y + h / 2 + s(canvas, 12),
        HINT_SIZE,
        outer.text,
    );
    x += plus_w + gap;
    x += draw_key(canvas, fonts, theme, "Start", x, y + pad);
    x += gap;
    fonts.draw(
        canvas,
        label,
        x,
        y + h / 2 + s(canvas, 12),
        HINT_SIZE,
        outer.text,
    );
}
