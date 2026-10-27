// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use crate::state::State;

const DETAILS: &str = "linux.sidebar.details";

/// The widths the user last gave the sidebars, kept on the device, which knows its screen.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub details: i32,
}

impl Default for Sizes {
    fn default() -> Self {
        Self { details: 260 }
    }
}

/// Reads the saved sizes, using the defaults for any that are missing.
pub fn load(state: &State) -> Sizes {
    let defaults = Sizes::default();
    let width = |key, default| {
        state.device().setting(key).and_then(|width| i32::try_from(width).ok()).unwrap_or(default)
    };
    Sizes { details: width(DETAILS, defaults.details) }
}

pub fn save(state: &State, sizes: Sizes) {
    state.device().set_setting(DETAILS, i64::from(sizes.details));
}
