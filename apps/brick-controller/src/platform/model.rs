// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Which handheld model the binary is running on.
//!
//! NextUI exports `DEVICE` for every Pak (`brick`, `brickpro`, `smartpro`,
//! `smartpros`, `rg40xxh`). The application only needs the distinction where
//! the hardware behaviour actually differs: the Brick Pro caps its rumble
//! driver at 2.5 V while the Brick runs at 3.3 V. The H700 family is not
//! uniform either: only the RG40XX H is mapped, so an unknown H700 is refused
//! rather than silently falling back to the Brick path.

/// The handheld model reported by NextUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Brick,
    BrickPro,
    SmartPro,
    SmartProS,
    Rg40xxH,
    /// An H700 model this build does not drive.
    H700Unknown,
    /// No reliable identification; use the behaviour common to the Brick.
    Unknown,
}

impl Model {
    /// Parse a `DEVICE` value or a `TRIMUI_MODEL` display name.
    ///
    /// `DEVICE` is the stable identifier; `TRIMUI_MODEL` is the human string
    /// NextUI reads from the vendor launcher and is only a fallback for a
    /// manual launch outside the Pak environment.
    pub fn parse(value: &str) -> Self {
        let value = value.trim().to_ascii_lowercase();
        match value.as_str() {
            "brick" | "trimui brick" => Self::Brick,
            "brickpro" | "brick pro" | "trimui brickpro" | "trimui brick pro" => Self::BrickPro,
            "smartpro" | "smart pro" | "trimui smart pro" => Self::SmartPro,
            "smartpros" | "smart pro s" | "trimui smart pro s" => Self::SmartProS,
            "rg40xxh" | "rg40xx-h" | "rg40xx h" => Self::Rg40xxH,
            _ => Self::Unknown,
        }
    }

    pub fn current() -> Self {
        if let Ok(value) = std::env::var("DEVICE") {
            let model = Self::parse(&value);
            if model != Self::Unknown {
                return model;
            }
        }
        if let Ok(value) = std::env::var("TRIMUI_MODEL") {
            let model = Self::parse(&value);
            if model != Self::Unknown {
                return model;
            }
        }
        // A missing or unknown H700 identifier must not fall back to the
        // Brick path: the H700 needs a different input mapping and framebuffer
        // handling, and the wrong GPIO writes are not safe to guess.
        if std::env::var("PLATFORM").is_ok_and(|value| value.trim().eq_ignore_ascii_case("h700")) {
            return Self::H700Unknown;
        }
        Self::Unknown
    }

    /// Whether this build has a layout and hardware path for the model.
    ///
    /// The Smart Pro shares `tg5040` with the Brick but uses a 1280×720 panel;
    /// until its layout exists, starting would draw a cropped 1024×768
    /// interface and take Bluetooth away from NextUI for nothing. `Unknown`
    /// stays allowed so a manual launch outside NextUI keeps the Brick path,
    /// but an H700 that is not the known RG40XX H is refused.
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::SmartPro | Self::SmartProS | Self::H700Unknown)
    }

    /// The motor voltage to use while this application owns the rumble path.
    ///
    /// The Brick Pro's driver can reach 3.3 V, but NextUI deliberately caps it
    /// at 2.5 V because stronger settings are unpleasant on that shell. The
    /// app bypasses NextUI's `PLAT_setRumble`, so it has to apply the same cap
    /// itself. The Brick and the other models keep their 3.3 V setting.
    pub fn rumble_voltage(self) -> u32 {
        match self {
            Self::BrickPro => 2_500_000,
            _ => 3_300_000,
        }
    }

    /// The idle voltage written back when the app releases the motor.
    pub const fn idle_rumble_voltage() -> u32 {
        3_300_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nextui_device_ids_are_recognised() {
        assert_eq!(Model::parse("brick"), Model::Brick);
        assert_eq!(Model::parse("brickpro"), Model::BrickPro);
        assert_eq!(Model::parse("smartpro"), Model::SmartPro);
        assert_eq!(Model::parse("smartpros"), Model::SmartProS);
        assert_eq!(Model::parse("rg40xxh"), Model::Rg40xxH);
        assert_eq!(Model::parse("rg40xx-h"), Model::Rg40xxH);
        assert_eq!(Model::parse(" unknown "), Model::Unknown);
    }

    #[test]
    fn vendor_model_names_are_a_fallback() {
        assert_eq!(Model::parse("Trimui Brick"), Model::Brick);
        assert_eq!(Model::parse("Trimui Brick Pro"), Model::BrickPro);
        assert_eq!(Model::parse("Trimui Smart Pro S"), Model::SmartProS);
    }

    #[test]
    fn unsupported_models_are_refused() {
        assert!(Model::Brick.is_supported());
        assert!(Model::BrickPro.is_supported());
        assert!(Model::Rg40xxH.is_supported());
        assert!(Model::Unknown.is_supported());
        assert!(!Model::SmartPro.is_supported());
        assert!(!Model::SmartProS.is_supported());
        assert!(!Model::H700Unknown.is_supported());
    }

    #[test]
    fn the_brick_pro_uses_the_lower_motor_voltage() {
        assert_eq!(Model::BrickPro.rumble_voltage(), 2_500_000);
        assert_eq!(Model::Brick.rumble_voltage(), 3_300_000);
        assert_eq!(Model::Unknown.rumble_voltage(), 3_300_000);
    }
}
