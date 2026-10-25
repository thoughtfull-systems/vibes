// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::path::{Path, PathBuf};

use gtk::glib;

const FILE: &str = "device.ini";
const HIDDEN: &str = "hidden";
const NAMES: &str = "names";
const PROMPTS: &str = "prompts";
const SETTINGS: &str = "settings";

/// Preferences that belong to this device and never sync: the window's layout and filters, the
/// name and visibility of each library, and which journals prompt here.
pub struct Device {
    file: glib::KeyFile,
    path: PathBuf,
}

impl Device {
    /// Reads the settings kept in `dir`, or starts with none.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(FILE);
        let file = glib::KeyFile::new();
        if path.exists()
            && let Err(error) = file.load_from_file(&path, glib::KeyFileFlags::KEEP_COMMENTS)
        {
            eprintln!("commonplace: couldn't read {}: {error}", path.display());
        }
        Self { file, path }
    }

    pub fn setting(&self, key: &str) -> Option<i64> {
        self.file.int64(SETTINGS, key).ok()
    }

    pub fn set_setting(&self, key: &str, value: i64) {
        if self.setting(key) != Some(value) {
            self.file.set_int64(SETTINGS, key, value);
            self.save();
        }
    }

    pub fn text_setting(&self, key: &str) -> Option<String> {
        self.file.string(SETTINGS, key).ok().map(String::from)
    }

    pub fn set_text_setting(&self, key: &str, value: &str) {
        if self.text_setting(key).as_deref() != Some(value) {
            self.file.set_string(SETTINGS, key, value);
            self.save();
        }
    }

    /// The name this device gives a library, by the library's key.
    pub fn name(&self, key: &str) -> Option<String> {
        self.file.string(NAMES, key).ok().map(String::from).filter(|name| !name.trim().is_empty())
    }

    pub fn set_name(&self, key: &str, name: &str) {
        self.file.set_string(NAMES, key, name.trim());
        self.save();
    }

    pub fn is_shown(&self, key: &str) -> bool {
        !self.file.boolean(HIDDEN, key).unwrap_or(false)
    }

    pub fn set_shown(&self, key: &str, shown: bool) {
        if shown {
            let _ = self.file.remove_key(HIDDEN, key);
        } else {
            self.file.set_boolean(HIDDEN, key, true);
        }
        self.save();
    }

    /// Whether a journal prompts on this device.
    pub fn prompts(&self, journal: &str) -> bool {
        self.file.boolean(PROMPTS, journal).unwrap_or(false)
    }

    pub fn set_prompts(&self, journal: &str, prompts: bool) {
        if prompts {
            self.file.set_boolean(PROMPTS, journal, true);
        } else {
            let _ = self.file.remove_key(PROMPTS, journal);
        }
        self.save();
    }

    /// Forgets a library that no longer exists.
    pub fn forget(&self, key: &str) {
        let _ = self.file.remove_key(NAMES, key);
        let _ = self.file.remove_key(HIDDEN, key);
        self.save();
    }

    fn save(&self) {
        if let Err(error) = self.file.save_to_file(&self.path) {
            eprintln!("commonplace: couldn't save {}: {error}", self.path.display());
        }
    }
}
