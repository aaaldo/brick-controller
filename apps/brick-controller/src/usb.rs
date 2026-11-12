// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Wired USB HID transport for the PC profile.
//!
//! The stock kernel provides FunctionFS but not `f_hid`, and its vendor
//! configfs implementation accepts only one gadget: a second `mkdir` collides
//! on the internal `android0` name. The transport therefore repurposes the ADB
//! gadget `g1` for the session and restores its identity and function links on
//! exit.
//!
//! The configfs/FunctionFS code is Linux-only; the request handling, the
//! descriptor blocks and the `g1` snapshot format stay pure so they are tested
//! on the host.

pub mod control;
pub mod ffs;
pub mod snapshot;

#[cfg(target_os = "linux")]
mod gadget;

#[cfg(target_os = "linux")]
pub use gadget::restore_after_crash;

#[cfg(target_os = "linux")]
pub use gadget::{Event, UsbHid};

/// pid.codes assigns vendor ID `0x1209` to open-source projects.
#[cfg(target_os = "linux")]
pub const USB_VENDOR: u16 = 0x1209;

/// pid.codes Test PID `0x0002` — development only.
///
/// Test PIDs are official and need no request, but they must never identify a
/// release. Replace this constant with the assigned product PID before
/// building a release pak; hosts cache the pair and would otherwise see a new
/// device after the upgrade.
#[cfg(target_os = "linux")]
pub const USB_PRODUCT: u16 = 0x0002;
