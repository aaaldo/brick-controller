// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Theme colours, NextUI colour loading and shared colour helpers.
//!
//! `Theme` keeps NextUI's `color1`..`color7` roles; the rest of the UI uses
//! the semantic names (`highlight`, `text`, `hint`, ...) so a theme change does
//! not turn into a search for colour numbers.

use brick::canvas::Rgb;
use std::process::Command;

/// NextUI's colour slots, loaded from `nextval.elf` when it is available.
///
/// The fields keep NextUI's numeric names (`color1`..`color7`) in their doc
/// comments because that is how themes are authored. Prefer the role names in
/// drawing code: `highlight` is the selected surface, `text` is body copy, and
/// so on. This keeps a theme swap from turning into a search for colour
/// numbers.
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
        nextval_json()
            .map(|json| Self::from_json(&json))
            .unwrap_or_else(Self::fallback)
    }

    /// Build a theme from the JSON returned by NextUI's `nextval.elf`.
    ///
    /// Only the colour keys are read. The preview test uses this with a JSON
    /// captured from the device so its screenshots match the active palette.
    pub fn from_json(json: &str) -> Self {
        let mut theme = Self::fallback();
        if let Some(color) = json_color(json, "color1") {
            theme.highlight = color;
        }
        if let Some(color) = json_color(json, "color2") {
            theme.accent = color;
        }
        if let Some(color) = json_color(json, "color3") {
            theme.button_label = color;
        }
        if let Some(color) = json_color(json, "color4") {
            theme.text = color;
        }
        if let Some(color) = json_color(json, "color5") {
            theme.highlighted_text = color;
        }
        if let Some(color) = json_color(json, "color6") {
            theme.hint = color;
        }
        if let Some(color) = json_color(json, "color7").or_else(|| json_color(json, "bgcolor")) {
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
pub(super) struct PillStyle {
    pub(super) fill: Rgb,
    pub(super) text: Rgb,
}

pub(super) fn outer_style(theme: &Theme) -> PillStyle {
    PillStyle {
        fill: theme.accent,
        text: theme.hint,
    }
}

pub(super) fn key_text(theme: &Theme) -> Rgb {
    theme.button_label
}

pub(super) fn hint_text(theme: &Theme) -> Rgb {
    theme.hint
}

pub(super) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nextui_colours_fill_the_theme_roles() {
        let json = r##"{
            "color1": "0xCBA6F7FF",
            "color2": "0x181825FF",
            "color3": "0x1E1E2EFF",
            "color4": "0xCDD6F4FF",
            "color5": "0x1E1E2EFF",
            "color6": "0xA6ADC8FF",
            "color7": "0x1E1E2EFF"
        }"##;
        let theme = Theme::from_json(json);
        assert_eq!(theme.highlight, Rgb(0xcb, 0xa6, 0xf7));
        assert_eq!(theme.accent, Rgb(0x18, 0x18, 0x25));
        assert_eq!(theme.button_label, Rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(theme.text, Rgb(0xcd, 0xd6, 0xf4));
        assert_eq!(theme.highlighted_text, Rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(theme.hint, Rgb(0xa6, 0xad, 0xc8));
        assert_eq!(theme.background, Rgb(0x1e, 0x1e, 0x2e));
    }
}
