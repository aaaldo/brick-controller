// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The `g1` snapshot written before the wired session changes the ADB gadget.
//!
//! The format is line-based because it is written to `/tmp` and read back by
//! the `restore` subcommand after a crash. Values are kept verbatim apart from
//! trailing newlines, so an identity string or an attribute value prints back
//! exactly as configfs reported it.

/// The parts of `g1` the wired session replaces.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Configfs attribute paths relative to `g1`, and their values.
    pub attrs: Vec<(String, String)>,
    /// String descriptor paths relative to `g1`, and their values.
    pub strings: Vec<(String, String)>,
    /// Function names linked into `configs/c.1`.
    pub links: Vec<String>,
}

impl Snapshot {
    /// Parse a marker file. Unknown lines are ignored so a newer build can
    /// leave extra records without breaking an older restore path.
    pub fn parse(text: &str) -> Self {
        let mut snapshot = Self::default();
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((kind, rest)) = line.split_once(' ') else {
                continue;
            };
            match kind {
                "attr" | "string" => {
                    let Some((path, value)) = rest.split_once('=') else {
                        continue;
                    };
                    let entry = (path.to_string(), value.to_string());
                    if kind == "attr" {
                        snapshot.attrs.push(entry);
                    } else {
                        snapshot.strings.push(entry);
                    }
                }
                "link" => snapshot.links.push(rest.to_string()),
                _ => {}
            }
        }
        snapshot
    }

    /// Render the marker. Attribute order is kept so the file is stable and a
    /// diff only shows a real change.
    pub fn render(&self) -> String {
        let mut text = String::from("# brick-controller usb snapshot v1\n");
        for (path, value) in &self.attrs {
            text.push_str(&format!("attr {path}={value}\n"));
        }
        for (path, value) in &self.strings {
            text.push_str(&format!("string {path}={value}\n"));
        }
        for name in &self.links {
            text.push_str(&format!("link {name}\n"));
        }
        text
    }

    /// True when nothing was captured. A corrupt or empty marker must not make
    /// the crash restore rewrite an identity it never saved.
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty() && self.strings.is_empty() && self.links.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_round_trips() {
        let snapshot = Snapshot {
            attrs: vec![
                ("idVendor".into(), "0x18d1".into()),
                ("configs/c.1/MaxPower".into(), "500".into()),
            ],
            strings: vec![("strings/0x409/product".into(), "TRIMUI ADB".into())],
            links: vec!["ffs.adb".into()],
        };
        assert_eq!(Snapshot::parse(&snapshot.render()), snapshot);
    }

    #[test]
    fn a_value_may_contain_an_equals_sign() {
        let snapshot = Snapshot {
            strings: vec![("strings/0x409/product".into(), "A=B".into())],
            ..Snapshot::default()
        };
        let parsed = Snapshot::parse(&snapshot.render());
        assert_eq!(
            parsed.strings,
            vec![("strings/0x409/product".into(), "A=B".into())]
        );
    }

    #[test]
    fn unknown_records_and_blank_lines_are_ignored() {
        let parsed =
            Snapshot::parse("# comment\n\nattr idVendor=0x18d1\nvendor something\nlink ffs.adb\n");
        assert_eq!(parsed.attrs, vec![("idVendor".into(), "0x18d1".into())]);
        assert_eq!(parsed.links, vec!["ffs.adb".to_string()]);
        assert!(!parsed.is_empty());
    }

    #[test]
    fn an_empty_marker_is_reported_as_empty() {
        assert!(Snapshot::parse("# nothing\n").is_empty());
    }
}
