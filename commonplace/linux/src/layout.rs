// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use crate::state::State;

const ACTIONS: &str = "linux.sidebar.actions";
const DETAILS: &str = "linux.sidebar.details";
const OUTLINE: &str = "linux.sidebar.outline";

/// The widths the user last gave the sidebars, kept on the device, which knows its screen.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub actions: i32,
    pub details: i32,
    pub outline: i32,
}

impl Default for Sizes {
    fn default() -> Self {
        Self { actions: 160, details: 260, outline: 220 }
    }
}

/// Reads the saved sizes, using the defaults for any that are missing.
pub fn load(state: &State) -> Sizes {
    let defaults = Sizes::default();
    let width = |key, default| {
        state.device().setting(key).and_then(|width| i32::try_from(width).ok()).unwrap_or(default)
    };
    Sizes {
        actions: width(ACTIONS, defaults.actions),
        details: width(DETAILS, defaults.details),
        outline: width(OUTLINE, defaults.outline),
    }
}

pub fn save(state: &State, sizes: Sizes) {
    for (key, width) in
        [(ACTIONS, sizes.actions), (DETAILS, sizes.details), (OUTLINE, sizes.outline)]
    {
        state.device().set_setting(key, i64::from(width));
    }
}
