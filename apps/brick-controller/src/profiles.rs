// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Controller identities and their report codecs.
//!
//! Each profile lives in its own module and exposes a [`ProfileSpec`] plus a
//! [`Codec`] implementation. [`Controller`] joins the two and is the only
//! profile type the session needs to know about.
//!
//! The set is closed on purpose: dispatch is an enum, not a trait object, so
//! adding a profile is a compile-time change and every match stays exhaustive.
//! See the **Architecture** section of `AGENTS.md`.

pub mod codec;
pub mod dualshock4;
pub mod generic_hid;
pub mod spec;
pub mod switch_pro;
pub mod xbox;

pub use codec::{Controller, LinkContext};
pub use spec::{Profile, Schedule};
