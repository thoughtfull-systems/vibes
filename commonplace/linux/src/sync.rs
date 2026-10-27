// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Sync with other devices, as decision 0013 describes, and the dialog that sets it up.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::{Arc, mpsc};

use adw::prelude::*;
use commonplace_core::Error;
use commonplace_core::sync::{Config, Host, Status, Syncer};
use gtk::{gio, glib};

use crate::libraries::ask_name;
use crate::state::{Space, State};

thread_local! {
    /// The state that sync reads and writes, which only the main thread may touch.
    static STATE: RefCell<Weak<State>> = RefCell::default();
}

/// Starts syncing the libraries in `dir`.
pub fn start(state: &Rc<State>, dir: &Path) {
    STATE.with(|cell| *cell.borrow_mut() = Rc::downgrade(state));
    match Syncer::start(dir, Arc::new(MainThread(glib::MainContext::default()))) {
        Ok(syncer) => state.set_syncer(syncer),
        Err(error) => eprintln!("commonplace: couldn't start sync: {error}"),
    }
}

/// Hands each of sync's calls to the main thread, and waits for the answer.
struct MainThread(glib::MainContext);

impl MainThread {
    fn run<T: Send + 'static>(
        &self,
        call: impl FnOnce(&State) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (sender, receiver) = mpsc::channel();
        self.0.invoke(move || {
            let result = match STATE.with(|cell| cell.borrow().upgrade()) {
                Some(state) => call(&state),
                None => Err(closing()),
            };
            let _ = sender.send(result);
        });
        receiver.recv().unwrap_or_else(|_| Err(closing()))
    }
}

impl Host for MainThread {
    fn version(&self, library: &str) -> Result<Vec<u8>, Error> {
        let library = library.to_owned();
        self.run(move |state| {
            state.read(|spaces| {
                Ok(spaces.library(&library).ok_or(Error::NoSuchLibrary(library.clone()))?.version())
            })
        })
    }

    fn updates_since(&self, library: &str, version: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let (library, version) = (library.to_owned(), version.to_owned());
        self.run(move |state| {
            state.read(|spaces| {
                spaces
                    .library(&library)
                    .ok_or(Error::NoSuchLibrary(library.clone()))?
                    .updates_since(&version)
            })
        })
    }

    fn merge_updates(&self, library: &str, updates: &[u8], repair: bool) -> Result<(), Error> {
        let (library, updates) = (library.to_owned(), updates.to_owned());
        self.run(move |state| {
            state.write(|spaces| spaces.library_mut(&library)?.merge_updates(&updates, repair))
        })
    }

    fn changed(&self) {
        self.0.invoke(|| {
            if let Some(state) = STATE.with(|cell| cell.borrow().upgrade()) {
                state.notify_sync();
            }
        });
    }
}

fn closing() -> Error {
    Error::Sync("Commonplace is closing".to_owned())
}

/// What the dialog last built its rows from, so that a change of status alone keeps them.
#[derive(PartialEq)]
struct Shown {
    config: Config,
    spaces: Vec<Space>,
}

struct Dialog {
    devices: adw::PreferencesGroup,
    dialog: adw::PreferencesDialog,
    offers: adw::PreferencesGroup,
    rows: RefCell<Vec<(adw::PreferencesGroup, gtk::Widget)>>,
    shown: RefCell<Option<Shown>>,
    state: Rc<State>,
    statuses: RefCell<Vec<(String, adw::ExpanderRow)>>,
}

pub fn present_dialog(state: &Rc<State>, parent: &impl IsA<gtk::Widget>) {
    let Some(syncer) = state.syncer() else {
        let alert = adw::AlertDialog::new(
            Some("Sync Isn't Running"),
            Some("Commonplace couldn't start sync. Its error went to standard error."),
        );
        alert.add_response("close", "_Close");
        alert.present(Some(parent));
        return;
    };
    let config = syncer.config();
    let dialog = adw::PreferencesDialog::builder().title("Sync").build();
    let page = adw::PreferencesPage::new();
    dialog.add(&page);

    let address = syncer.address();
    let this_device = adw::PreferencesGroup::builder()
        .title("This Device")
        .description("Enter this address on another device to pair the two")
        .build();
    let address_row = adw::ActionRow::builder()
        .title("Address")
        .subtitle(&address)
        .subtitle_selectable(true)
        .css_classes(["property"])
        .build();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text("Copy Address")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    copy.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |button| {
            button.clipboard().set_text(&address);
            dialog.add_toast(adw::Toast::new("Address copied"));
        }
    ));
    address_row.add_suffix(&copy);
    this_device.add(&address_row);
    page.add(&this_device);

    let options = adw::PreferencesGroup::new();
    let automatic = adw::SwitchRow::builder()
        .title("Sync Automatically")
        .subtitle("After each change, and every few minutes")
        .active(config.automatic)
        .build();
    automatic.connect_active_notify(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        move |row| {
            if let Some(syncer) = state.syncer()
                && let Err(error) = syncer.set_automatic(row.is_active())
            {
                dialog.add_toast(adw::Toast::new(&error.to_string()));
            }
        }
    ));
    options.add(&automatic);
    let relay = adw::EntryRow::builder()
        .title("Relay Server (Empty for Public Relays)")
        .text(config.relay.clone().unwrap_or_default())
        .show_apply_button(true)
        .build();
    relay.connect_apply(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        move |row| {
            let Some(syncer) = state.syncer() else {
                return;
            };
            let message = match syncer.set_relay(Some(&row.text())) {
                Ok(()) => "The relay takes effect when Commonplace next starts".to_owned(),
                Err(error) => error.to_string(),
            };
            dialog.add_toast(adw::Toast::new(&message));
        }
    ));
    options.add(&relay);
    let sync_now = adw::ButtonRow::builder().title("Sync Now").build();
    sync_now.connect_activated(glib::clone!(
        #[weak]
        state,
        move |_| {
            if let Some(syncer) = state.syncer() {
                syncer.sync_now();
            }
        }
    ));
    options.add(&sync_now);
    page.add(&options);

    let offers = adw::PreferencesGroup::builder()
        .title("Offers")
        .description("Libraries that other devices share with this one")
        .build();
    page.add(&offers);
    let devices = adw::PreferencesGroup::builder()
        .title("Devices")
        .description("Devices that this one syncs with, once each has the other's address")
        .build();
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add Device")
        .css_classes(["flat"])
        .build();
    devices.set_header_suffix(Some(&add));
    page.add(&devices);

    let this = Rc::new(Dialog {
        devices,
        dialog: dialog.clone(),
        offers,
        rows: RefCell::default(),
        shown: RefCell::default(),
        state: state.clone(),
        statuses: RefCell::default(),
    });
    this.add_actions();
    add.connect_clicked(glib::clone!(
        #[weak]
        this,
        move |_| this.ask_device()
    ));
    this.fill();
    let weak = Rc::downgrade(&this);
    state.connect_sync_changed(move || match weak.upgrade() {
        Some(this) => {
            this.fill();
            true
        }
        None => false,
    });
    // Rebuilding the rows also follows changes to the libraries, such as a new one's name.
    state.connect_changed(glib::clone!(
        #[weak]
        this,
        move || this.fill()
    ));
    let open = RefCell::new(Some(this));
    dialog.connect_closed(move |_| drop(open.take()));
    dialog.present(Some(parent));
}

impl Dialog {
    fn syncer(&self) -> Option<&Syncer> {
        self.state.syncer()
    }

    fn report(&self, result: Result<(), Error>) {
        if let Err(error) = result {
            self.dialog.add_toast(adw::Toast::new(&error.to_string()));
        }
    }

    fn add_actions(self: &Rc<Self>) {
        let actions = gio::SimpleActionGroup::new();
        let accept = gio::SimpleAction::new("accept", Some(glib::VariantTy::STRING_ARRAY));
        accept.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, value| {
                let Some([share, device, library]) = strings(value) else {
                    return;
                };
                if !library.is_empty() {
                    let result =
                        this.syncer().map_or(Ok(()), |s| s.accept(&share, &device, &library));
                    this.report(result);
                    return;
                }
                ask_name(
                    &this.dialog,
                    "New Library",
                    "Name it for this device",
                    "",
                    glib::clone!(
                        #[weak]
                        this,
                        move |name| {
                            let result = this.state.create_space(&name).and_then(|library| {
                                this.syncer()
                                    .map_or(Ok(()), |s| s.accept(&share, &device, &library))
                            });
                            this.report(result);
                        }
                    ),
                );
            }
        ));
        let decline = gio::SimpleAction::new("decline", Some(glib::VariantTy::STRING_ARRAY));
        decline.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, value| {
                if let Some([share, device]) = strings(value) {
                    let result = this.syncer().map_or(Ok(()), |s| s.decline(&share, &device));
                    this.report(result);
                }
            }
        ));
        actions.add_action(&accept);
        actions.add_action(&decline);
        self.dialog.insert_action_group("sync", Some(&actions));
    }

    /// Rebuilds the rows if the configuration or the libraries changed, and updates each device's
    /// status either way.
    fn fill(self: &Rc<Self>) {
        let Some(syncer) = self.syncer() else {
            return;
        };
        let shown = Shown { config: syncer.config(), spaces: self.state.spaces() };
        if self.shown.borrow().as_ref() != Some(&shown) {
            self.rebuild(&shown);
            *self.shown.borrow_mut() = Some(shown);
        }
        for (device, row) in self.statuses.borrow().iter() {
            row.set_subtitle(&describe(&syncer.status(device)));
        }
    }

    fn rebuild(self: &Rc<Self>, shown: &Shown) {
        let expanded: HashSet<String> = self
            .statuses
            .borrow_mut()
            .drain(..)
            .filter(|(_, row)| row.is_expanded())
            .map(|(device, _)| device)
            .collect();
        for (group, row) in self.rows.borrow_mut().drain(..) {
            group.remove(&row);
        }
        let config = &shown.config;
        let name_of = |device: &str| {
            config.device(device).map_or_else(|| short(device), |device| device.name.clone())
        };
        for offer in config.open_offers() {
            let row = adw::ActionRow::builder()
                .title(name_of(&offer.device))
                .subtitle("Offers a library")
                .build();
            let menu = gio::Menu::new();
            let item = |label: &str, library: &str| {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(
                    Some("sync.accept"),
                    Some(&[offer.share.as_str(), offer.device.as_str(), library].to_variant()),
                );
                menu.append_item(&item);
            };
            match config.share(&offer.share) {
                Some(share) => {
                    let name = space_name(&shown.spaces, &share.library);
                    item(&format!("Sync With {name}"), &share.library);
                }
                None => {
                    item("New Library…", "");
                    for space in &shown.spaces {
                        item(&format!("Merge Into {}", space.name), &space.key);
                    }
                }
            }
            row.add_suffix(
                &gtk::MenuButton::builder()
                    .label("Accept")
                    .menu_model(&menu)
                    .valign(gtk::Align::Center)
                    .css_classes(["suggested-action"])
                    .build(),
            );
            let decline = gtk::Button::builder()
                .label("Decline")
                .valign(gtk::Align::Center)
                .action_name("sync.decline")
                .action_target(&[offer.share.as_str(), offer.device.as_str()].to_variant())
                .build();
            row.add_suffix(&decline);
            self.offers.add(&row);
            self.rows.borrow_mut().push((self.offers.clone(), row.upcast()));
        }
        self.offers.set_visible(config.open_offers().next().is_some());

        for device in &config.devices {
            let row = adw::ExpanderRow::builder()
                .title(&device.name)
                .expanded(expanded.contains(&device.address))
                .build();
            let name = adw::EntryRow::builder()
                .title("Name")
                .text(&device.name)
                .show_apply_button(true)
                .build();
            let address = device.address.clone();
            name.connect_apply(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |row| {
                    let name = row.text();
                    if !name.trim().is_empty() {
                        let result = this
                            .syncer()
                            .map_or(Ok(()), |s| s.rename_device(&address, name.trim()));
                        this.report(result);
                    }
                }
            ));
            row.add_row(&name);
            for space in &shown.spaces {
                let shared =
                    config.shares_with(&device.address).any(|share| share.library == space.key);
                let share = adw::SwitchRow::builder()
                    .title(format!("Share {}", space.name))
                    .active(shared)
                    .build();
                let (address, library) = (device.address.clone(), space.key.clone());
                share.connect_active_notify(glib::clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |row| {
                        let Some(syncer) = this.syncer() else {
                            return;
                        };
                        let result = if row.is_active() {
                            syncer.share(&library, &address)
                        } else {
                            syncer.unshare(&library, &address)
                        };
                        this.report(result);
                    }
                ));
                row.add_row(&share);
            }
            let remove = adw::ButtonRow::builder()
                .title("Remove Device")
                .css_classes(["destructive-action"])
                .build();
            let address = device.address.clone();
            remove.connect_activated(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let result = this.syncer().map_or(Ok(()), |s| s.remove_device(&address));
                    this.report(result);
                }
            ));
            row.add_row(&remove);
            self.devices.add(&row);
            self.rows.borrow_mut().push((self.devices.clone(), row.clone().upcast()));
            self.statuses.borrow_mut().push((device.address.clone(), row));
        }
        if config.devices.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No devices yet")
                .subtitle("Add another device by its address")
                .build();
            self.devices.add(&row);
            self.rows.borrow_mut().push((self.devices.clone(), row.upcast()));
        }
    }

    fn ask_device(self: &Rc<Self>) {
        let address = gtk::Entry::builder().placeholder_text("Address").build();
        let name = gtk::Entry::builder().placeholder_text("Name").activates_default(true).build();
        let fields =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12).build();
        fields.append(&address);
        fields.append(&name);
        let dialog = adw::AlertDialog::new(
            Some("Add Device"),
            Some("Enter the address that the other device's Sync dialog shows, and a name for it"),
        );
        dialog.set_extra_child(Some(&fields));
        dialog.add_responses(&[("cancel", "_Cancel"), ("add", "_Add")]);
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");
        let update = glib::clone!(
            #[weak]
            dialog,
            #[weak]
            address,
            #[weak]
            name,
            move || {
                let ready = !address.text().trim().is_empty() && !name.text().trim().is_empty();
                dialog.set_response_enabled("add", ready);
            }
        );
        update();
        address.connect_changed(glib::clone!(
            #[strong]
            update,
            move |_| update()
        ));
        name.connect_changed(move |_| update());
        dialog.connect_response(
            Some("add"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[weak]
                address,
                #[weak]
                name,
                move |_, _| {
                    let result = this.syncer().map_or(Ok(()), |syncer| {
                        syncer.add_device(&address.text(), name.text().trim()).map(drop)
                    });
                    this.report(result);
                }
            ),
        );
        dialog.present(Some(&self.dialog));
        address.grab_focus();
    }
}

/// The strings in an action's target, if it holds exactly `N` of them.
fn strings<const N: usize>(value: Option<&glib::Variant>) -> Option<[String; N]> {
    value?.get::<Vec<String>>()?.try_into().ok()
}

fn describe(status: &Status) -> String {
    if status.syncing {
        return "Syncing…".to_owned();
    }
    if let Some(error) = &status.error {
        return format!("Couldn't sync: {error}");
    }
    let synced = status
        .synced
        .and_then(|at| glib::DateTime::from_unix_local(at.div_euclid(1000)).ok())
        .and_then(|time| time.format("%b %-d, %H:%M").ok());
    match synced {
        Some(time) => format!("Synced {time}"),
        None => "Not synced yet".to_owned(),
    }
}

fn space_name(spaces: &[Space], key: &str) -> String {
    spaces.iter().find(|space| space.key == key).map_or_else(|| short(key), |s| s.name.clone())
}

/// The start of a long identifier, enough to tell it apart from others.
fn short(identifier: &str) -> String {
    identifier.chars().take(8).collect()
}
