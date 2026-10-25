// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;

use commonplace_core::{Error, NoteId, Spaces};

use crate::device::Device;

/// The prefix of the settings this app kept in the library before spaces, and keeps on the device
/// now.
const CARRIED_SETTINGS: &str = "linux.";

/// The identifier of the journal that capture last saved into.
const CAPTURE_JOURNAL: &str = "linux.capture.journal";

/// The key of the library that capture last saved into.
const CAPTURE_SPACE: &str = "linux.capture.space";

/// A library as the space buttons show it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Space {
    pub key: String,
    pub name: String,
    pub shown: bool,
}

/// The open libraries and this device's settings, shared by every window in the process.
pub struct State {
    device: Device,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
    spaces: RefCell<Spaces>,
}

impl State {
    pub fn new(mut spaces: Spaces, device: Device) -> Self {
        if let Some(key) = spaces.migrated() {
            carry_settings(&spaces, key, &device);
        }
        for key in spaces.keys() {
            spaces.set_shown(&key, device.is_shown(&key));
        }
        Self { device, listeners: RefCell::default(), spaces: RefCell::new(spaces) }
    }

    pub fn read<T>(&self, f: impl FnOnce(&Spaces) -> T) -> T {
        f(&self.spaces.borrow())
    }

    /// Changes the libraries, then tells every listener if anything changed.
    pub fn write<T>(&self, f: impl FnOnce(&mut Spaces) -> Result<T, Error>) -> Result<T, Error> {
        let before = self.spaces.borrow().commits();
        let result = f(&mut self.spaces.borrow_mut());
        if result.is_ok() && self.spaces.borrow().commits() != before {
            self.notify();
        }
        result
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(listener));
    }

    fn notify(&self) {
        for listener in self.listeners.borrow().iter() {
            listener();
        }
    }

    /// Every library, in order of name, which is the order of the space buttons.
    pub fn spaces(&self) -> Vec<Space> {
        let spaces = self.spaces.borrow();
        let mut list: Vec<Space> = spaces
            .keys()
            .into_iter()
            .map(|key| Space {
                name: self.device.name(&key).unwrap_or_else(|| "Unnamed".to_owned()),
                shown: spaces.is_shown(&key),
                key,
            })
            .collect();
        list.sort_by(|a, b| {
            a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.key.cmp(&b.key))
        });
        list
    }

    /// The libraries this device hasn't named yet.
    pub fn unnamed(&self) -> Vec<String> {
        let keys = self.spaces.borrow().keys();
        keys.into_iter().filter(|key| self.device.name(key).is_none()).collect()
    }

    pub fn set_name(&self, key: &str, name: &str) {
        self.device.set_name(key, name);
        self.notify();
    }

    pub fn set_shown(&self, key: &str, shown: bool) {
        self.device.set_shown(key, shown);
        let _ = self.write(|spaces| {
            spaces.set_shown(key, shown);
            Ok(())
        });
    }

    /// Creates a library with this device's name for it.
    pub fn create_space(&self, name: &str) -> Result<String, Error> {
        let key = self.spaces.borrow_mut().create()?;
        self.device.set_name(&key, name);
        self.notify();
        Ok(key)
    }

    pub fn remove_space(&self, key: &str) -> Result<(), Error> {
        self.spaces.borrow_mut().remove(key)?;
        self.device.forget(key);
        self.notify();
        Ok(())
    }

    pub fn merge_space(&self, source: &str, target: &str) -> Result<(), Error> {
        self.spaces.borrow_mut().merge(source, target)?;
        self.device.forget(source);
        self.notify();
        Ok(())
    }

    /// The library that capture saves into first: the last one it saved into, or the first.
    pub fn capture_space(&self) -> String {
        let saved = self.device.text_setting(CAPTURE_SPACE);
        let spaces = self.spaces();
        saved
            .filter(|key| spaces.iter().any(|space| &space.key == key))
            .unwrap_or_else(|| spaces[0].key.clone())
    }

    pub fn set_capture_space(&self, key: &str) {
        self.device.set_text_setting(CAPTURE_SPACE, key);
    }

    /// Whether a journal prompts on this device.
    pub fn prompts_here(&self, journal: NoteId) -> bool {
        self.device.prompts(&journal.to_string())
    }

    pub fn set_prompts_here(&self, journal: NoteId, prompts: bool) {
        self.device.set_prompts(&journal.to_string(), prompts);
        self.notify();
    }

    /// Creates a journal, which prompts on the device that creates it.
    pub fn create_journal(&self, space: &str, name: &str) -> Result<NoteId, Error> {
        let id = self.write(|spaces| spaces.create_journal(space, name))?;
        self.device.set_prompts(&id.to_string(), true);
        Ok(id)
    }

    /// The journal that capture last saved an entry into.
    pub fn capture_journal(&self) -> Option<NoteId> {
        self.device.text_setting(CAPTURE_JOURNAL)?.parse().ok()
    }

    pub fn set_capture_journal(&self, id: NoteId) {
        self.device.set_text_setting(CAPTURE_JOURNAL, &id.to_string());
    }
}

/// Copies the settings that a library kept for this app onto the device.
fn carry_settings(spaces: &Spaces, key: &str, device: &Device) {
    let Some(library) = spaces.library(key) else {
        return;
    };
    for setting in library.setting_keys() {
        if !setting.starts_with(CARRIED_SETTINGS) {
            continue;
        }
        if let Some(value) = library.setting(&setting) {
            device.set_setting(&setting, value);
        } else if let Some(value) = library.text_setting(&setting) {
            device.set_text_setting(&setting, &value);
        }
    }
}
