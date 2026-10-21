// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;

use commonplace_core::{Error, Library};

/// The open library, shared by every window in the process.
pub struct State {
    library: RefCell<Library>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
}

impl State {
    pub fn new(library: Library) -> Self {
        Self { library: RefCell::new(library), listeners: RefCell::default() }
    }

    pub fn read<T>(&self, f: impl FnOnce(&Library) -> T) -> T {
        f(&self.library.borrow())
    }

    /// Changes the library, then tells every listener if anything changed.
    pub fn write<T>(&self, f: impl FnOnce(&mut Library) -> Result<T, Error>) -> Result<T, Error> {
        let before = self.library.borrow().commits();
        let result = f(&mut self.library.borrow_mut());
        if result.is_ok() && self.library.borrow().commits() != before {
            for listener in self.listeners.borrow().iter() {
                listener();
            }
        }
        result
    }

    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(listener));
    }
}
