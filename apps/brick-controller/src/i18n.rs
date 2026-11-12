// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Every string the interface can show, in one place.
//!
//! The catalogue is compiled in: the pak is self-contained and the UI must not
//! depend on a translation file being present on the SD card. [`Key`] is a
//! closed set, so the exhaustive match below makes a missing string a compile
//! error, and [`Key::ALL`] lets the tests check the whole table at once.
//!
//! Only English ships today. [`Lang`] and the `lang` parameter are the seam for
//! the next languages: adding one is a new arm in [`tr`], not a change to every
//! caller. Strings with a variable use `{name}` placeholders and [`fill`],
//! because word order differs between languages.
//!
//! Logs, Bluetooth identities, profile brands and the labels printed on the
//! handheld's buttons deliberately stay in English.

/// A UI language.
///
/// Kept as an enum rather than a string so a typo cannot select an empty table.
/// English is the only variant for now; the value travels with `Frame` so the
/// renderer already knows which table to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
}

/// A user-visible string. Adding a key requires an English value in [`tr`].
macro_rules! keys {
    ($($key:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        // The wired transport and its strings are only compiled on the device;
        // the host build still compiles the menu and view code.
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        pub enum Key {
            $($key),+
        }

        impl Key {
            /// Every key, for the completeness test.
            #[cfg(test)]
            pub const ALL: &'static [Self] = &[$(Self::$key),+];
        }
    };
}

keys! {
    AppName,
    Connected,
    Devices,
    ControllerType,
    ChangingControllerType,
    Pairing,
    Quit,
    Forget,
    Connect,
    Cancel,
    Choose,
    Back,
    ChangeControllerType,
    Retry,
    NoPairedDevice,
    ProfileLine,
    VisibleAs,
    ControllerLine,
    ReconnectingTo,
    WaitingForHost,
    DisconnectHold,
    ScreenOffHint,
    Confirm,
    HoldTitle,
    HoldMessage,
    Offline,
    PairNew,
    WiredConnection,
    WiredDetail,
    WiredHost,
    ProfileLineUsb,
    WaitingForUsbHost,
    ReturningToDevices,
    ThisController,
    ForgetTitle,
    ForgetMessage,
    ConnectingTo,
    StartingBluetooth,
    ErrorTitle,
    BluetoothActivationError,
    UsbActivationError,
    UnsupportedDeviceTitle,
    UnsupportedDeviceMessage,
}

/// The string for `key` in `lang`.
pub fn tr(lang: Lang, key: Key) -> &'static str {
    // One language today. When a second one lands this becomes a match on
    // `lang`, and the compiler points at every arm that needs a translation.
    let Lang::En = lang;
    english(key)
}

/// Replace every `{name}` placeholder in `template` with its argument.
///
/// A small helper rather than `format!`: the template is chosen at runtime,
/// and future languages need to move the placeholders around the sentence.
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

fn english(key: Key) -> &'static str {
    match key {
        Key::AppName => "Brick Controller",
        Key::Connected => "Connected",
        Key::Devices => "Devices",
        Key::ControllerType => "Controller type",
        Key::ChangingControllerType => "Changing controller type…",
        Key::Pairing => "Pairing",
        Key::Quit => "Quit",
        Key::Forget => "Forget",
        Key::Connect => "Connect",
        Key::Cancel => "Cancel",
        Key::Choose => "Choose",
        Key::Back => "Back",
        Key::ChangeControllerType => "Change controller type",
        Key::Retry => "Retry",
        Key::NoPairedDevice => "No paired device.",
        Key::ProfileLine => "Profile {profile} · Bluetooth HID",
        Key::VisibleAs => "VISIBLE AS",
        Key::ControllerLine => "{type} controller",
        Key::ReconnectingTo => "Reconnecting to {host}…",
        Key::WaitingForHost => "Waiting for a host…",
        Key::DisconnectHold => "Disconnect (Hold)",
        Key::ScreenOffHint => {
            "Screen will switch off after 10s. Press Menu+Start once to wake it up"
        }
        Key::Confirm => "Confirm",
        Key::HoldTitle => "Keep holding to disconnect…",
        Key::HoldMessage => "Input is paused while Menu and Start are held.",
        Key::Offline => "Offline",
        Key::PairNew => "Pair a new device",
        Key::WiredConnection => "Wired connection",
        Key::WiredDetail => "PC · USB",
        Key::WiredHost => "PC (USB)",
        Key::ProfileLineUsb => "Profile {profile} · USB HID",
        Key::WaitingForUsbHost => "Waiting for USB host…",
        Key::ReturningToDevices => "Returning to Devices…",
        Key::ThisController => "this controller",
        Key::ForgetTitle => "Forget {name}?",
        Key::ForgetMessage => "You will need to pair it again from the host.",
        Key::ConnectingTo => "Connecting to {name}",
        Key::StartingBluetooth => "Starting Bluetooth…",
        Key::ErrorTitle => "Error",
        Key::BluetoothActivationError => {
            "A problem occurred during Bluetooth activation. Please try again."
        }
        Key::UsbActivationError => {
            "A problem occurred during USB activation. Please try again."
        }
        Key::UnsupportedDeviceTitle => "Unsupported device",
        Key::UnsupportedDeviceMessage => {
            "Brick Controller supports the TrimUI Brick and Brick Pro only. Smart Pro support is planned."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_english_text() {
        for key in Key::ALL {
            assert!(
                !tr(Lang::En, *key).is_empty(),
                "{key:?} has no English text"
            );
        }
    }

    #[test]
    fn fill_replaces_every_occurrence() {
        assert_eq!(
            fill("{name} and {name}", &[("name", "Brick")]),
            "Brick and Brick"
        );
        assert_eq!(
            fill("no placeholder", &[("name", "Brick")]),
            "no placeholder"
        );
    }

    #[test]
    fn templates_keep_their_placeholders() {
        let templates = [
            (Key::ProfileLine, "{profile}"),
            (Key::ProfileLineUsb, "{profile}"),
            (Key::ControllerLine, "{type}"),
            (Key::ReconnectingTo, "{host}"),
            (Key::ForgetTitle, "{name}"),
            (Key::ConnectingTo, "{name}"),
        ];
        for (key, placeholder) in templates {
            let text = tr(Lang::En, key);
            assert!(text.contains(placeholder), "{key:?} lost {placeholder}");
            assert_eq!(
                fill(text, &[(placeholder.trim_matches(['{', '}']), "value")]),
                text.replace(placeholder, "value")
            );
        }
    }
}
