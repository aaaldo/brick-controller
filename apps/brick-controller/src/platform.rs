// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! The Brick's hardware: evdev input, backlight, rumble motor and status
//! readings.
//!
//! This is the imperative shell. Everything here may touch the filesystem,
//! libc or sysfs; profiles and the pure `input` module never do. A missing
//! device is a no-op, not an error, because the same binary runs on models
//! with different peripherals.

pub mod display;
pub mod input_trimui;
pub mod rumble;
pub mod status;

pub use status::StatusInfo;
