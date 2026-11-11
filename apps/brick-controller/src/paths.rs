// Copyright 2026 the Brick Controller contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem roots shared by the app and the platform modules.
//!
//! Keeping this in one place avoids a dependency from `platform` back to the
//! session: the display and theme code only need to know where persistent
//! state lives.

use std::path::PathBuf;

const DATA_DIR: &str = "/mnt/SDCARD/.userdata/tg5040/brick-controller";

/// Where the launch wrapper keeps persistent state.
///
/// The environment variables let the packaged app, a local test run and an
/// older Padpod data directory share the same code without writing to the SD
/// card path by default.
pub fn data_dir() -> PathBuf {
    for key in ["BRICK_CONTROLLER_DATA", "PADPOD_DATA"] {
        if let Ok(path) = std::env::var(key) {
            return PathBuf::from(path);
        }
    }
    if let Ok(path) = std::env::var("USERDATA_PATH") {
        return PathBuf::from(path).join("brick-controller");
    }
    PathBuf::from(DATA_DIR)
}
