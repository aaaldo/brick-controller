// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The NextUI-like screens: theme, status bar, list rows, footers, dialogs.
//!
//! The layout follows Apostrophe's NextUI metrics (MIT): a 1024-wide reference,
//! a 3x device scale on the Brick, 30 px screen padding, 90 px pills, a 120 px
//! header/footer and the NextUI colour keys from `nextval.elf`.

use brick::canvas::{Canvas, Rgb};
use brick::text::Fonts;
use std::collections::HashMap;
use std::process::Command;

pub const OK: Rgb = Rgb(0xb8, 0xbb, 0x26);
pub const WARN: Rgb = Rgb(0xfa, 0xbd, 0x2f);
pub const ERROR: Rgb = Rgb(0xfb, 0x49, 0x34);

const HINT_SIZE: f32 = 36.0;
const SMALL_SIZE: f32 = 30.0;
const ITEM_SIZE: f32 = 48.0;
const KEY_SIZE: f32 = 27.0;
const TITLE_SIZE: f32 = 72.0;
const ROW_H: i32 = 90;

/// NextUI's colour slots, loaded from `nextval.elf` when it is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Main Color (color1): selected row and inner button surface.
    pub highlight: Rgb,
    /// Primary Accent Color (color2): outer footer/status surfaces and accents.
    pub accent: Rgb,
    /// Secondary Accent Color (color3): label on the main button surface.
    pub button_label: Rgb,
    /// List Text (color4): normal text.
    pub text: Rgb,
    /// List Text Selected (color5): text on a selected surface.
    pub highlighted_text: Rgb,
    /// Hint info Color (color6): secondary text, footer labels, status icons.
    pub hint: Rgb,
    /// Background Color (color7).
    pub background: Rgb,
}

impl Theme {
    pub fn load() -> Self {
        let mut theme = Self::fallback();
        let Some(json) = nextval_json() else {
            return theme;
        };
        if let Some(color) = json_color(&json, "color1") {
            theme.highlight = color;
        }
        if let Some(color) = json_color(&json, "color2") {
            theme.accent = color;
        }
        if let Some(color) = json_color(&json, "color3") {
            theme.button_label = color;
        }
        if let Some(color) = json_color(&json, "color4") {
            theme.text = color;
        }
        if let Some(color) = json_color(&json, "color5") {
            theme.highlighted_text = color;
        }
        if let Some(color) = json_color(&json, "color6") {
            theme.hint = color;
        }
        if let Some(color) = json_color(&json, "color7").or_else(|| json_color(&json, "bgcolor")) {
            theme.background = color;
        }
        theme
    }

    /// The Gruvbox-like palette the app used before, kept as a safe fallback.
    pub const fn fallback() -> Self {
        Self {
            highlight: Rgb(0xeb, 0xdb, 0xb2),
            accent: Rgb(0xb3, 0x54, 0x0b),
            button_label: Rgb(0x1d, 0x20, 0x21),
            text: Rgb(0xeb, 0xdb, 0xb2),
            highlighted_text: Rgb(0x1d, 0x20, 0x21),
            hint: Rgb(0xbd, 0xae, 0x93),
            background: Rgb(0x1d, 0x20, 0x21),
        }
    }
}

#[derive(Clone, Copy)]
struct PillStyle {
    fill: Rgb,
    text: Rgb,
}

fn outer_style(theme: &Theme) -> PillStyle {
    PillStyle {
        fill: theme.accent,
        text: theme.hint,
    }
}

fn key_text(theme: &Theme) -> Rgb {
    theme.button_label
}

fn hint_text(theme: &Theme) -> Rgb {
    theme.hint
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    Rgb(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

/// The nextval.elf JSON is a flat object of colour strings; a full JSON
/// dependency would be more code than this.
fn nextval_json() -> Option<String> {
    let mut paths = Vec::new();
    if let Ok(system) = std::env::var("SYSTEM_PATH") {
        paths.push(format!("{system}/bin/nextval.elf"));
    }
    paths.push("/mnt/SDCARD/.system/tg5040/bin/nextval.elf".into());
    for path in paths {
        if !std::path::Path::new(&path).is_file() {
            continue;
        }
        let mut command = Command::new(&path);
        // NextUI runs nextval.elf with its SDL runtime and shared data dir in
        // the environment. A pak launched directly over ADB does not inherit
        // those, so make the call self-contained.
        let mut ld = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        for dir in ["/usr/trimui/lib", "/mnt/SDCARD/.system/lib"] {
            if !ld.split(':').any(|part| part == dir) {
                if !ld.is_empty() {
                    ld.push(':');
                }
                ld.push_str(dir);
            }
        }
        if !ld.is_empty() {
            command.env("LD_LIBRARY_PATH", ld);
        }
        if std::env::var_os("SHARED_USERDATA_PATH").is_none() {
            command.env("SHARED_USERDATA_PATH", "/mnt/SDCARD/.userdata/shared");
        }
        if std::env::var_os("SDCARD_PATH").is_none() {
            command.env("SDCARD_PATH", "/mnt/SDCARD");
        }
        let Ok(output) = command.output() else {
            continue;
        };
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            if text.contains("color1") {
                return Some(text);
            }
        }
    }
    None
}

fn json_color(json: &str, key: &str) -> Option<Rgb> {
    let pattern = format!("\"{key}\"");
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let first = rest.find('"')?;
    let rest = &rest[first + 1..];
    let second = rest.find('"')?;
    parse_hex(&rest[..second])
}

fn parse_hex(text: &str) -> Option<Rgb> {
    let hex = text.trim().trim_start_matches('#').trim_start_matches("0x");
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// NextUI font search order, with the previous theme font as a last resort.
pub fn font_paths() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(system) = std::env::var("SYSTEM_PATH") {
        paths.push(format!("{system}/res/font1.ttf"));
        paths.push(format!("{system}/res/font2.ttf"));
        paths.push(format!("{system}/res/font.ttf"));
    }
    paths.extend([
        "/mnt/SDCARD/.system/res/font1.ttf".into(),
        "/mnt/SDCARD/.system/res/font2.ttf".into(),
        "/mnt/SDCARD/.system/res/font.ttf".into(),
        "/mnt/SDCARD/Themes/SPRUCE/nunwen.ttf".into(),
    ]);
    paths
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusInfo {
    pub clock: String,
    pub battery: Option<u8>,
    pub charging: bool,
    pub wifi: bool,
    pub bluetooth: bool,
}

impl StatusInfo {
    pub fn read(bluetooth: bool) -> Self {
        Self {
            clock: clock_text(),
            battery: read_battery(),
            charging: read_charging(),
            wifi: read_wifi(),
            bluetooth,
        }
    }
}

fn read_sysfs(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
}

fn read_battery() -> Option<u8> {
    [
        "/sys/class/power_supply/axp2202-battery/capacity",
        "/sys/class/power_supply/battery/capacity",
    ]
    .iter()
    .find_map(|path| read_sysfs(path)?.parse::<u8>().ok())
    .filter(|value| *value <= 100)
}

fn read_charging() -> bool {
    read_sysfs("/sys/class/power_supply/axp2202-usb/online").as_deref() == Some("1")
        || read_sysfs("/sys/class/power_supply/ac/online").as_deref() == Some("1")
}

fn read_wifi() -> bool {
    if let Some(state) = read_sysfs("/sys/class/net/wlan0/operstate") {
        return state == "up";
    }
    // Older firmware may not expose operstate; the link-quality column is
    // the next best signal that the radio is associated.
    std::fs::read_to_string("/proc/net/wireless")
        .map(|text| {
            text.lines().skip(2).any(|line| {
                let mut fields = line.split_whitespace();
                let Some(name) = fields.next() else {
                    return false;
                };
                if !name.starts_with("wlan") {
                    return false;
                }
                fields
                    .nth(1)
                    .and_then(|quality| quality.trim_end_matches('.').parse::<f32>().ok())
                    .is_some_and(|quality| quality > 0.0)
            })
        })
        .unwrap_or(false)
}

fn clock_text() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "--:--".into();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub trailing: String,
    pub selected: bool,
    pub chevron: bool,
}

impl Row {
    pub fn new(label: impl Into<String>, trailing: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            trailing: trailing.into(),
            selected,
            chevron: false,
        }
    }

    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Splash {
        message: String,
    },
    Controller {
        host: String,
        profile: &'static str,
        connected: bool,
        events: Vec<String>,
        countdown: Option<String>,
    },
    Menu {
        rows: Vec<Row>,
    },
    Host {
        title: String,
        rows: Vec<Row>,
        note: String,
    },
    Profile {
        rows: Vec<Row>,
        show_note: bool,
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
pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    view: &View,
    status: &StatusInfo,
    modal: Option<&Modal>,
    hold: Option<f32>,
) {
    canvas.clear(theme.background);
    match view {
        View::Splash { message } => draw_splash(canvas, fonts, theme, message),
        View::Controller {
            host,
            profile,
            connected,
            events,
            countdown,
        } => {
            header(canvas, fonts, theme, status, "Brick Controller");
            draw_controller(
                canvas,
                fonts,
                theme,
                host,
                profile,
                *connected,
                events,
                countdown.as_deref(),
            );
            footer(canvas, fonts, theme, &[item("F1", "Devices")], &[]);
        }
        View::Menu { rows } => {
            header(canvas, fonts, theme, status, "Devices");
            rows_at(canvas, fonts, theme, rows, 120);
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", "Quit")],
                &[item("A", "Select")],
            );
            if rows.is_empty() {
                let text = "No paired device.";
                fonts.draw(
                    canvas,
                    text,
                    s(canvas, 30),
                    s(canvas, 180),
                    SMALL_SIZE,
                    hint_text(theme),
                );
            }
        }
        View::Host { title, rows, note } => {
            header(canvas, fonts, theme, status, title);
            rows_at(canvas, fonts, theme, rows, 120);
            if !note.is_empty() {
                draw_wrapped(
                    canvas,
                    fonts,
                    hint_text(theme),
                    SMALL_SIZE,
                    note,
                    s(canvas, 30),
                    s(canvas, 410),
                    canvas.width() - s(canvas, 60),
                    3,
                );
            }
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", "Back")],
                &[item("A", "Select")],
            );
        }
        View::Profile { rows, show_note } => {
            header(canvas, fonts, theme, status, "Controller type");
            rows_at(canvas, fonts, theme, rows, 120);
            if *show_note {
                draw_wrapped(
                    canvas,
                    fonts,
                    hint_text(theme),
                    SMALL_SIZE,
                    "Changing the controller type requires the host to forget the old pairing.",
                    s(canvas, 30),
                    s(canvas, 410),
                    canvas.width() - s(canvas, 60),
                    3,
                );
            }
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", "Cancel")],
                &[item("A", "Choose")],
            );
        }
        View::Pairing {
            identity,
            controller_type,
            reconnecting,
        } => {
            header(canvas, fonts, theme, status, "Pairing");
            draw_pairing(
                canvas,
                fonts,
                theme,
                identity,
                controller_type,
                reconnecting.as_deref(),
            );
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", "Back"), item("X", "Change controller type")],
                &[],
            );
        }
        View::Error {
            title,
            message,
            steps,
            countdown,
        } => {
            header(canvas, fonts, theme, status, title);
            draw_error(canvas, fonts, theme, message, steps, countdown.as_deref());
            footer(
                canvas,
                fonts,
                theme,
                &[item("B", "Quit")],
                &[item("A", "Retry")],
            );
        }
    }

    if let Some(modal) = modal {
        draw_modal(canvas, fonts, theme, modal);
    }
    if let Some(progress) = hold {
        draw_hold(canvas, fonts, theme, progress);
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

fn draw_splash(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, message: &str) {
    let centre = canvas.width() / 2;
    fonts.draw_centred(
        canvas,
        "Brick Controller",
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
    let cy = canvas.height() / 2 + s(canvas, 100);
    circle(canvas, centre, cy, s(canvas, 28), theme.accent);
    circle(canvas, centre, cy, s(canvas, 22), theme.background);
}

#[allow(clippy::too_many_arguments)]
fn draw_controller(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    host: &str,
    profile: &str,
    connected: bool,
    events: &[String],
    countdown: Option<&str>,
) {
    let left = s(canvas, 30);
    let top = s(canvas, 132);
    let status = if connected { "Connected" } else { "Waiting" };
    let status_colour = if connected { OK } else { WARN };
    circle(
        canvas,
        left + s(canvas, 10),
        top + s(canvas, 20),
        s(canvas, 10),
        status_colour,
    );
    fonts.draw(
        canvas,
        status,
        left + s(canvas, 34),
        top + s(canvas, 44),
        TITLE_SIZE,
        status_colour,
    );
    let host = fonts.ellipsize(host, ITEM_SIZE, canvas.width() - s(canvas, 60));
    fonts.draw(
        canvas,
        &host,
        left,
        top + s(canvas, 104),
        ITEM_SIZE,
        theme.text,
    );
    fonts.draw(
        canvas,
        &format!("Profile {profile} · Bluetooth HID"),
        left,
        top + s(canvas, 146),
        SMALL_SIZE,
        hint_text(theme),
    );

    let events_top = s(canvas, 330);
    fonts.draw(
        canvas,
        "RECENT EVENTS",
        left,
        events_top,
        SMALL_SIZE,
        hint_text(theme),
    );
    if events.is_empty() {
        fonts.draw(
            canvas,
            "Press a button…",
            left,
            events_top + s(canvas, 46),
            ITEM_SIZE,
            hint_text(theme),
        );
    } else {
        let visible = events.len().min(5);
        let start = events.len() - visible;
        for (i, line) in events[start..].iter().enumerate() {
            fonts.draw(
                canvas,
                line,
                left,
                events_top + s(canvas, 46) + i as i32 * s(canvas, 42),
                SMALL_SIZE,
                theme.text,
            );
        }
    }

    let hint_y = canvas.height() - s(canvas, 154);
    if let Some(countdown) = countdown {
        fonts.draw(
            canvas,
            "Hold F1+F2 for Devices.",
            left,
            canvas.height() - s(canvas, 190),
            SMALL_SIZE,
            hint_text(theme),
        );
        fonts.draw(
            canvas,
            countdown,
            left,
            hint_y,
            SMALL_SIZE,
            hint_text(theme),
        );
    } else {
        fonts.draw(
            canvas,
            "Hold F1+F2 for Devices.",
            left,
            hint_y,
            SMALL_SIZE,
            hint_text(theme),
        );
    }
}

fn draw_pairing(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
    identity: &str,
    controller_type: &str,
    reconnecting: Option<&str>,
) {
    let centre = canvas.width() / 2;
    let max_w = canvas.width() - s(canvas, 120);
    let mut y = s(canvas, 150);
    fonts.draw_centred(
        canvas,
        "VISIBLE AS",
        centre,
        y,
        SMALL_SIZE,
        hint_text(theme),
    );
    y += s(canvas, 82);
    fonts.draw_centred(canvas, identity, centre, y, TITLE_SIZE, theme.accent);
    y += s(canvas, 48);
    fonts.draw_centred(
        canvas,
        &format!("{controller_type} controller"),
        centre,
        y,
        HINT_SIZE,
        hint_text(theme),
    );
    y += s(canvas, 64);
    let instructions = "On your PC, Mac, phone or tablet:\nopen Bluetooth settings and select this controller.\nNo PIN is required.";
    for line in fonts.wrap(instructions, ITEM_SIZE, max_w, 3) {
        fonts.draw_centred(canvas, &line, centre, y, ITEM_SIZE, theme.text);
        y += s(canvas, 56);
    }
    if let Some(host) = reconnecting {
        fonts.draw_centred(
            canvas,
            &format!("Reconnecting to {host}…"),
            centre,
            y + s(canvas, 8),
            SMALL_SIZE,
            hint_text(theme),
        );
    }
    let wait_y = canvas.height() - s(canvas, 230);
    let wait_w = s(canvas, 520);
    let wait_h = s(canvas, 72);
    let wait_x = centre - wait_w / 2;
    let waiting = PillStyle {
        fill: theme.accent,
        text: theme.button_label,
    };
    round_rect(
        canvas,
        wait_x,
        wait_y,
        wait_w,
        wait_h,
        wait_h / 2,
        waiting.fill,
    );
    fonts.draw_centred(
        canvas,
        "Waiting for a host…",
        centre,
        wait_y + wait_h / 2 + s(canvas, 12),
        SMALL_SIZE,
        waiting.text,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_error(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    theme: &Theme,
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
        "Bluetooth unavailable",
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

fn rows_at(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, rows: &[Row], top: i32) {
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
        if row.chevron {
            label_w -= s(canvas, 38);
        } else if !row.trailing.is_empty() {
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
        if row.chevron {
            draw_chevron(
                canvas,
                left + width - s(canvas, 42),
                y + row_h / 2,
                s(canvas, 28),
                trailing_colour,
            );
        } else if !row.trailing.is_empty() {
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

fn draw_chevron(canvas: &mut Canvas, x: i32, cy: i32, size: i32, colour: Rgb) {
    stroke(
        canvas,
        (x, cy - size),
        (x + size / 2, cy),
        s(canvas, 4),
        colour,
    );
    stroke(
        canvas,
        (x + size / 2, cy),
        (x, cy + size),
        s(canvas, 4),
        colour,
    );
}

fn footer(
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

fn group_width(canvas: &Canvas, fonts: &mut Fonts, items: &[FooterItem]) -> i32 {
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

fn key_width(canvas: &Canvas, fonts: &mut Fonts, key: &str) -> i32 {
    if key.chars().count() == 1 {
        s(canvas, 60)
    } else {
        fonts.measure(key, KEY_SIZE) + s(canvas, 30)
    }
}

fn draw_group(
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
    let key_fill = theme.highlight;
    let key_colour = key_text(theme);
    round_rect(canvas, x, y, width, h, h / 2, outer.fill);
    let gap = s(canvas, 15);
    let mut cx = x + gap;
    for item in items {
        let kw = key_width(canvas, fonts, item.key);
        let key_size = if item.key.chars().count() == 1 {
            HINT_SIZE
        } else {
            KEY_SIZE
        };
        round_rect(
            canvas,
            cx,
            y + gap,
            kw,
            s(canvas, 60),
            s(canvas, 30),
            key_fill,
        );
        fonts.draw_centred(
            canvas,
            item.key,
            cx + kw / 2,
            y + gap + s(canvas, 30) + s(canvas, 14),
            key_size,
            key_colour,
        );
        cx += kw + gap;
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

fn draw_modal(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, modal: &Modal) {
    let w = canvas.width();
    let h = canvas.height();
    canvas.blend_rect(0, 0, w, h, Rgb(0, 0, 0), 180);
    let box_w = s(canvas, 780);
    let lines = fonts.wrap(&modal.message, SMALL_SIZE, box_w - s(canvas, 80), 3);
    let line_h = s(canvas, 44);
    let box_h = s(canvas, 170) + lines.len() as i32 * line_h + s(canvas, 110);
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
        "Confirm"
    } else {
        modal.confirm
    };
    let cancel = [item("B", "Cancel")];
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

fn draw_hold(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, progress: f32) {
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
        "Keep holding to open devices…",
        w / 2,
        box_y + s(canvas, 72),
        ITEM_SIZE,
        theme.text,
    );
    fonts.draw_centred(
        canvas,
        "Input is paused while both function keys are held.",
        w / 2,
        box_y + s(canvas, 118),
        SMALL_SIZE,
        hint_text(theme),
    );
    let bar_w = s(canvas, 700);
    let bar_h = s(canvas, 18);
    let bar_x = (w - bar_w) / 2;
    let bar_y = box_y + s(canvas, 178);
    round_rect(
        canvas,
        bar_x,
        bar_y,
        bar_w,
        bar_h,
        bar_h / 2,
        mix(theme.background, theme.text, 0.15),
    );
    let filled = (bar_w as f32 * progress.clamp(0.0, 1.0)) as i32;
    if filled > 0 {
        round_rect(
            canvas,
            bar_x,
            bar_y,
            filled.max(bar_h),
            bar_h,
            bar_h / 2,
            theme.accent,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_wrapped(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    colour: Rgb,
    size: f32,
    text: &str,
    x: i32,
    y: i32,
    max_w: i32,
    max_lines: usize,
) {
    for (i, line) in fonts.wrap(text, size, max_w, max_lines).iter().enumerate() {
        fonts.draw(
            canvas,
            line,
            x,
            y + i as i32 * (size as i32 + s(canvas, 8)),
            size,
            colour,
        );
    }
}

fn draw_status(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, status: &StatusInfo) {
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

fn status_bar_width(canvas: &Canvas, fonts: &mut Fonts, status: &StatusInfo) -> i32 {
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
