// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Bluetooth transport: the daemon takeover, the L2CAP HID channels and the
//! SDP record.
//!
//! Nothing here knows a controller identity. The session passes the Bluetooth
//! name and report descriptor in, and profiles only ever see [`Address`] and
//! report bytes. This keeps the protocol modules testable without a radio.

pub mod address;
pub mod bluez;
pub mod l2cap;
pub mod sdp;

pub use address::Address;
