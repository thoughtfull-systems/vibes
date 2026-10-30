// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Sync with other devices, as decision 0013 describes, and the dialog that sets it up.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::{Arc, mpsc};

use adw::prelude::*;
use commonplace_core::Error;
use commonplace_core::sync::{
    Config, Host, Nearby, Outgoing, OutgoingState, Request, Status, Syncer,
};
use gtk::{gio, glib};

use crate::libraries::ask_name;
use crate::state::{Space, State};

const START_FAILED: &str = "Commonplace couldn't start sync. Its error went to standard error.";

thread_local! {
    /// Keeps the subscription to logind's sleep signal, which ends when dropped.
    static SLEEP: RefCell<Option<gio::SignalSubscription>> = RefCell::default();
    /// The state that sync reads and writes, which only the main thread may touch.
    static STATE: RefCell<Weak<State>> = RefCell::default();
}

/// Starts syncing the libraries in `dir`.
pub fn start(state: &Rc<State>, dir: &Path) {
    STATE.with(|cell| *cell.borrow_mut() = Rc::downgrade(state));
    match Syncer::start(dir, Arc::new(MainThread(glib::MainContext::default()))) {
        Ok(syncer) => {
            if syncer.name().is_empty()
                && let Err(error) = syncer.set_name(&glib::host_name())
            {
                eprintln!("commonplace: couldn't name this device: {error}");
            }
            state.set_syncer(syncer);
        }
        Err(error) => eprintln!("commonplace: couldn't start sync: {error}"),
    }
}

/// Starts sync again, so a change that takes effect only at start, such as the relay, applies now,
/// then calls `done`.
pub fn restart(state: &Rc<State>, done: impl FnOnce() + 'static) {
    let old = state.take_syncer();
    let state = Rc::downgrade(state);
    glib::spawn_future_local(async move {
        // Dropping a syncer waits up to seconds for its connections to close, which would freeze
        // the window, and the new one can't listen until the old one stops.
        let _ = gio::spawn_blocking(move || drop(old)).await;
        if let Some(state) = state.upgrade() {
            start(&state, &crate::library_dir());
            done();
        }
    });
}

/// Tells sync to reach other devices again whenever the network changes or the system wakes.
pub fn watch_connectivity(state: &Rc<State>) {
    let state = Rc::downgrade(state);
    let reconnect = move || {
        if let Some(state) = state.upgrade()
            && let Some(syncer) = state.syncer()
        {
            syncer.reconnect();
        }
    };
    let on_network = reconnect.clone();
    gio::NetworkMonitor::default().connect_network_changed(move |_, available| {
        if available {
            on_network();
        }
    });
    let bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(error) => {
            eprintln!("commonplace: couldn't watch for the system waking: {error}");
            return;
        }
    };
    let subscription = bus.subscribe_to_signal(
        Some("org.freedesktop.login1"),
        Some("org.freedesktop.login1.Manager"),
        Some("PrepareForSleep"),
        Some("/org/freedesktop/login1"),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if signal.parameters.get::<(bool,)>() == Some((false,)) {
                reconnect();
            }
        },
    );
    SLEEP.with(|cell| cell.replace(Some(subscription)));
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
    nearby: adw::PreferencesGroup,
    nearby_rows: RefCell<Vec<gtk::Widget>>,
    nearby_shown: RefCell<Option<Vec<Nearby>>>,
    offers: adw::PreferencesGroup,
    /// The pairing this device asked for, and the alert that shows its code while it waits.
    outgoing: RefCell<Option<(Outgoing, Option<adw::AlertDialog>)>>,
    /// The alert for each request to pair that waits for an answer, by the requester's address.
    requests: RefCell<Vec<(String, adw::AlertDialog)>>,
    rows: RefCell<Vec<(adw::PreferencesGroup, gtk::Widget)>>,
    shown: RefCell<Option<Shown>>,
    state: Rc<State>,
    statuses: RefCell<Vec<(String, adw::ExpanderRow)>>,
}

/// Shows the Sync dialog, and opens `invite`, a link from another device, if there is one.
pub fn present_dialog(state: &Rc<State>, parent: &impl IsA<gtk::Widget>, invite: Option<&str>) {
    let Some(syncer) = state.syncer() else {
        let alert = adw::AlertDialog::new(Some("Sync isn't running"), Some(START_FAILED));
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
        .title("This device")
        .description("Devices that pair with this one know it by this name")
        .build();
    let name =
        adw::EntryRow::builder().title("Name").text(syncer.name()).show_apply_button(true).build();
    name.connect_apply(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        move |row| {
            if let Some(syncer) = state.syncer()
                && let Err(error) = syncer.set_name(&row.text())
            {
                dialog.add_toast(adw::Toast::new(&error.to_string()));
            }
        }
    ));
    this_device.add(&name);
    let address_row = adw::ActionRow::builder()
        .title("Address, for Pairing by Hand")
        .subtitle(grouped(&address))
        .subtitle_selectable(true)
        .css_classes(["property"])
        .build();
    let copy = gtk::Button::builder()
        .icon_name(icon!("copy"))
        .tooltip_text("Copy address")
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
        .title("Sync automatically")
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
        .title("Relay server (empty for public relays)")
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
            if let Err(error) = syncer.set_relay(Some(&row.text())) {
                dialog.add_toast(adw::Toast::new(&error.to_string()));
                return;
            }
            // Restarting can't stop a syncer that something still holds.
            drop(syncer);
            let public = row.text().trim().is_empty();
            restart(
                &state,
                glib::clone!(
                    #[weak]
                    state,
                    #[weak]
                    dialog,
                    move || {
                        let message = match state.syncer() {
                            Some(syncer) => {
                                // The dialog looks for nearby devices while it's open.
                                let _ = syncer.start_nearby();
                                if public {
                                    "Sync uses the public relays"
                                } else {
                                    "Sync uses this relay"
                                }
                            }
                            None => START_FAILED,
                        };
                        dialog.add_toast(adw::Toast::new(message));
                    }
                ),
            );
        }
    ));
    options.add(&relay);
    let sync_now = adw::ButtonRow::builder().title("Sync now").build();
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

    let nearby = adw::PreferencesGroup::builder()
        .title("Pair a device")
        .description("Devices on this network show here while their Sync screen is open")
        .build();
    page.add(&nearby);
    let other_ways = adw::PreferencesGroup::new();
    let show_invite = adw::ButtonRow::builder().title("Show invite").build();
    let open_invite = adw::ButtonRow::builder().title("Open invite").build();
    let enter_address = adw::ButtonRow::builder().title("Enter address").build();
    other_ways.add(&show_invite);
    other_ways.add(&open_invite);
    other_ways.add(&enter_address);
    page.add(&other_ways);

    let offers = adw::PreferencesGroup::builder()
        .title("Offers")
        .description("Libraries that other devices share with this one")
        .build();
    page.add(&offers);
    let devices = adw::PreferencesGroup::builder()
        .title("Devices")
        .description("Devices that this one syncs with")
        .build();
    page.add(&devices);
    page.add(&options);

    let this = Rc::new(Dialog {
        devices,
        dialog: dialog.clone(),
        nearby,
        nearby_rows: RefCell::default(),
        nearby_shown: RefCell::default(),
        offers,
        outgoing: RefCell::default(),
        requests: RefCell::default(),
        rows: RefCell::default(),
        shown: RefCell::default(),
        state: state.clone(),
        statuses: RefCell::default(),
    });
    this.add_actions();
    show_invite.connect_activated(glib::clone!(
        #[weak]
        this,
        move |_| this.show_invite()
    ));
    open_invite.connect_activated(glib::clone!(
        #[weak]
        this,
        move |_| this.ask_invite()
    ));
    enter_address.connect_activated(glib::clone!(
        #[weak]
        this,
        move |_| this.ask_device()
    ));
    if let Err(error) = syncer.start_nearby() {
        dialog.add_toast(adw::Toast::new(&format!("Can't look for nearby devices: {error}")));
    }
    if let Some(invite) = invite {
        this.report(syncer.open_invite(invite));
    }
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
    dialog.connect_closed(glib::clone!(
        #[weak]
        state,
        move |_| {
            if let Some(syncer) = state.syncer() {
                syncer.stop_nearby();
                syncer.clear_outgoing();
            }
            if let Some(this) = open.take() {
                this.close_alerts();
            }
        }
    ));
    dialog.present(Some(parent));
}

impl Dialog {
    fn syncer(&self) -> Option<Rc<Syncer>> {
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
                    "New library",
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
        let nearby = syncer.nearby();
        if self.nearby_shown.borrow().as_ref() != Some(&nearby) {
            self.show_nearby(&nearby);
            *self.nearby_shown.borrow_mut() = Some(nearby);
        }
        self.show_requests(&syncer.requests());
        self.show_outgoing(syncer.outgoing());
    }

    fn show_nearby(self: &Rc<Self>, nearby: &[Nearby]) {
        for row in self.nearby_rows.borrow_mut().drain(..) {
            self.nearby.remove(&row);
        }
        for device in nearby {
            let row = adw::ActionRow::builder()
                .title(&device.name)
                .subtitle("On this network")
                .activatable(true)
                .build();
            let pair = gtk::Button::builder()
                .label("Pair")
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();
            let address = device.address.clone();
            let start = glib::clone!(
                #[weak(rename_to = this)]
                self,
                move || {
                    let result = this.syncer().map_or(Ok(()), |s| s.pair_nearby(&address));
                    this.report(result);
                }
            );
            pair.connect_clicked(glib::clone!(
                #[strong]
                start,
                move |_| start()
            ));
            row.connect_activated(move |_| start());
            row.add_suffix(&pair);
            self.nearby.add(&row);
            self.nearby_rows.borrow_mut().push(row.upcast());
        }
        if nearby.is_empty() {
            let row = adw::ActionRow::builder()
                .title("Looking for devices on this network…")
                .subtitle("Open Sync on the other device too")
                .build();
            row.add_prefix(&adw::Spinner::new());
            self.nearby.add(&row);
            self.nearby_rows.borrow_mut().push(row.upcast());
        }
    }

    /// Asks the owner about each new request to pair, and closes the questions that expired.
    fn show_requests(self: &Rc<Self>, requests: &[Request]) {
        let mut shown = self.requests.borrow_mut();
        shown.retain(|(address, alert)| {
            let waiting = requests.iter().any(|request| &request.address == address);
            if !waiting {
                alert.force_close();
            }
            waiting
        });
        for request in requests {
            if shown.iter().any(|(address, _)| *address == request.address) {
                continue;
            }
            let alert = adw::AlertDialog::new(
                Some(&format!("Pair With {}?", request.name)),
                Some(&format!(
                    "Pair only if {} shows the code {}.",
                    request.name,
                    spaced(&request.code)
                )),
            );
            alert.add_responses(&[("decline", "_Decline"), ("pair", "_Pair")]);
            alert.set_response_appearance("pair", adw::ResponseAppearance::Suggested);
            alert.set_close_response("decline");
            let address = request.address.clone();
            alert.connect_response(
                None,
                glib::clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |_, response| {
                        if let Some(syncer) = this.syncer() {
                            syncer.answer(&address, response == "pair");
                        }
                    }
                ),
            );
            alert.present(Some(&self.dialog));
            shown.push((request.address.clone(), alert));
        }
    }

    /// Shows the code while the other device's owner decides, and how the pairing ended.
    fn show_outgoing(&self, outgoing: Option<Outgoing>) {
        let mut shown = self.outgoing.borrow_mut();
        if shown.as_ref().map(|(outgoing, _)| outgoing) == outgoing.as_ref() {
            return;
        }
        if let Some((_, Some(alert))) = shown.take() {
            alert.force_close();
        }
        let Some(outgoing) = outgoing else {
            return;
        };
        let alert = match &outgoing.state {
            OutgoingState::Waiting => outgoing.code.as_ref().map(|code| {
                let alert = adw::AlertDialog::new(
                    Some("Accept on the other device"),
                    Some(&format!("Check that it shows the code {}.", spaced(code))),
                );
                alert.add_response("close", "_Hide");
                alert.present(Some(&self.dialog));
                alert
            }),
            OutgoingState::Paired(name) => {
                self.dialog.add_toast(adw::Toast::new(&format!("Paired with {name}")));
                None
            }
            OutgoingState::Failed(reason) => {
                self.dialog.add_toast(adw::Toast::new(&format!("Couldn't pair: {reason}")));
                None
            }
        };
        *shown = Some((outgoing, alert));
    }

    fn close_alerts(&self) {
        for (_, alert) in self.requests.borrow_mut().drain(..) {
            alert.force_close();
        }
        if let Some((_, Some(alert))) = self.outgoing.borrow_mut().take() {
            alert.force_close();
        }
    }

    /// Shows an invite as a QR code and a link, with a switch for each library to share.
    fn show_invite(self: &Rc<Self>) {
        let picture = gtk::Picture::builder()
            .can_shrink(false)
            .halign(gtk::Align::Center)
            .css_classes(["card"])
            .build();
        let link = gtk::Label::builder()
            .selectable(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Char)
            .css_classes(["monospace", "caption"])
            .build();
        // Pango marks a word broken across lines with a hyphen, which reads as part of the link.
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrInt::new_insert_hyphens(false));
        link.set_attributes(Some(&attributes));
        let copy = gtk::Button::builder()
            .label("_Copy link")
            .use_underline(true)
            .halign(gtk::Align::Center)
            .css_classes(["pill"])
            .build();
        let shares = adw::PreferencesGroup::builder()
            .title("Share")
            .description("Libraries to offer the device that uses this invite")
            .build();
        let switches: Rc<RefCell<Vec<(String, adw::SwitchRow)>>> = Rc::default();
        for space in self.state.spaces() {
            let row = adw::SwitchRow::builder().title(&space.name).build();
            shares.add(&row);
            switches.borrow_mut().push((space.key, row));
        }
        let refresh = Rc::new(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            picture,
            #[weak]
            link,
            #[strong]
            switches,
            move || {
                let libraries: Vec<String> = switches
                    .borrow()
                    .iter()
                    .filter(|(_, row)| row.is_active())
                    .map(|(key, _)| key.clone())
                    .collect();
                let Some(syncer) = this.syncer() else {
                    return;
                };
                match syncer.invite(&libraries) {
                    Ok(invite) => {
                        picture.set_paintable(qr_texture(&invite).as_ref());
                        link.set_text(&invite);
                    }
                    Err(error) => this.report(Err(error)),
                }
            }
        ));
        for (_, row) in switches.borrow().iter() {
            row.connect_active_notify(glib::clone!(
                #[strong]
                refresh,
                move |_| refresh()
            ));
        }
        refresh();
        copy.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            link,
            move |button| {
                button.clipboard().set_text(&link.text());
                this.dialog.add_toast(adw::Toast::new("Invite link copied"));
            }
        ));
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        content.append(
            &gtk::Label::builder()
                .label(concat!(
                    "Scan this code with a phone, or open the link on the other device. ",
                    "It works once, for 15 minutes.",
                ))
                .wrap(true)
                .justify(gtk::Justification::Center)
                .build(),
        );
        content.append(&picture);
        content.append(&link);
        content.append(&copy);
        content.append(&shares);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .child(&content)
                .build(),
        ));
        let invite =
            adw::Dialog::builder().title("Invite a device").content_width(420).child(&view).build();
        invite.present(Some(&self.dialog));
    }

    /// Asks for an invite link from another device, and opens it.
    fn ask_invite(self: &Rc<Self>) {
        let entry = gtk::Entry::builder()
            .placeholder_text("commonplace:pair/…")
            .activates_default(true)
            .build();
        let alert = adw::AlertDialog::new(
            Some("Open invite"),
            Some("Paste the link that the other device's Show invite gives"),
        );
        alert.set_extra_child(Some(&entry));
        alert.add_responses(&[("cancel", "_Cancel"), ("open", "_Open")]);
        alert.set_response_appearance("open", adw::ResponseAppearance::Suggested);
        alert.set_default_response(Some("open"));
        alert.set_close_response("cancel");
        alert.set_response_enabled("open", false);
        entry.connect_changed(glib::clone!(
            #[weak]
            alert,
            move |entry| alert.set_response_enabled("open", Syncer::is_invite(&entry.text()))
        ));
        alert.connect_response(
            Some("open"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                #[weak]
                entry,
                move |_, _| {
                    let result = this.syncer().map_or(Ok(()), |s| s.open_invite(&entry.text()));
                    this.report(result);
                }
            ),
        );
        alert.present(Some(&self.dialog));
        entry.grab_focus();
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
                    item("New library…", "");
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
                .title("Remove device")
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
                .subtitle("Pair one above")
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
            Some("Add device"),
            Some("Enter the address that the other device's Sync screen shows, and a name for it"),
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

/// A QR code for `text`, black on white, with the quiet zone that scanners need around it.
fn qr_texture(text: &str) -> Option<gtk::gdk::Texture> {
    const SCALE: usize = 6;
    const QUIET: usize = 4;
    let (width, modules) = commonplace_core::sync::qr_code(text).ok()?;
    let side = (width + 2 * QUIET) * SCALE;
    let mut pixels = vec![0xff; side * side];
    for (index, dark) in modules.into_iter().enumerate() {
        if !dark {
            continue;
        }
        let (x, y) = ((index % width + QUIET) * SCALE, (index / width + QUIET) * SCALE);
        for row in y..y + SCALE {
            pixels[row * side + x..row * side + x + SCALE].fill(0);
        }
    }
    let texture = gtk::gdk::MemoryTexture::new(
        side as i32,
        side as i32,
        gtk::gdk::MemoryFormat::G8,
        &glib::Bytes::from_owned(pixels),
        side,
    );
    Some(texture.upcast())
}

/// An address in groups of eight characters, which wrap between groups and are easier to compare.
fn grouped(address: &str) -> String {
    let groups: Vec<&str> =
        address.as_bytes().chunks(8).filter_map(|group| std::str::from_utf8(group).ok()).collect();
    groups.join(" ")
}

/// A six-digit code split in two halves, as people read it aloud.
fn spaced(code: &str) -> String {
    match code.split_at_checked(3) {
        Some((first, second)) => format!("{first} {second}"),
        None => code.to_owned(),
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
