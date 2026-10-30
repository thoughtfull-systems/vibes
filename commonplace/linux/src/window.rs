// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{ActionEntry, ActionStatus, ActionView, NoteId};
use gtk::{gdk, gio, glib};

use crate::browse::{Browser, Scope};
use crate::inbox::InboxView;
use crate::journals_view::JournalsView;
use crate::layout;
use crate::libraries;
use crate::note_page::NotePage;
use crate::rows::{NoteList, Section, today};
use crate::state::{Space, State};

/// How long a closed action stays in the review's recently done section.
const RECENT_DAYS: i64 = 7;

/// The ways to work with actions, each a list in the actions tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Execute,
    Inbox,
    Plan,
    Review,
}

// In the order the action mode menu lists them.
const MODES: [(&str, &str, Mode); 4] = [
    ("Execute", icon!("execute"), Mode::Execute),
    ("Plan", icon!("edit"), Mode::Plan),
    ("Review", icon!("review"), Mode::Review),
    ("Inbox", icon!("inbox"), Mode::Inbox),
];

/// A listing the window can show: the memos, the actions in a mode, every note, or the trash.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Listing {
    Actions(Mode),
    AllNotes,
    Journals,
    Memos,
    Trash,
}

/// A place in the window's history: a listing, or a note opened from one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Location {
    List(Listing),
    Note(NoteId, Listing),
}

impl Location {
    fn listing(self) -> Listing {
        match self {
            Self::List(listing) | Self::Note(_, listing) => listing,
        }
    }

    /// The location as the text the `linux.screen` setting holds: the listing's name, followed by
    /// a slash and the note's identifier if a note is open.
    fn encode(self) -> String {
        let name = |listing| match listing {
            Listing::Actions(Mode::Execute) => "actions.execute",
            Listing::Actions(Mode::Inbox) => "actions.inbox",
            Listing::Actions(Mode::Plan) => "actions.plan",
            Listing::Actions(Mode::Review) => "actions.review",
            Listing::AllNotes => "all",
            Listing::Journals => "journals",
            Listing::Memos => "memos",
            Listing::Trash => "trash",
        };
        match self {
            Self::List(listing) => name(listing).to_owned(),
            Self::Note(id, listing) => format!("{}/{id}", name(listing)),
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let (name, note) = match text.split_once('/') {
            Some((name, note)) => (name, Some(note.parse::<NoteId>().ok()?)),
            None => (text, None),
        };
        let listing = match name {
            "actions.execute" => Listing::Actions(Mode::Execute),
            "actions.inbox" => Listing::Actions(Mode::Inbox),
            "actions.plan" => Listing::Actions(Mode::Plan),
            "actions.review" => Listing::Actions(Mode::Review),
            "all" => Listing::AllNotes,
            "journals" => Listing::Journals,
            "memos" => Listing::Memos,
            "trash" => Listing::Trash,
            _ => return None,
        };
        Some(match note {
            Some(id) => Self::Note(id, listing),
            None => Self::List(listing),
        })
    }
}

fn remove_missing_notes(
    current: Location,
    back: &mut Vec<Location>,
    forward: &mut Vec<Location>,
    exists: impl Fn(NoteId) -> bool,
) -> Option<Location> {
    let valid = |location: &Location| match location {
        Location::List(_) => true,
        Location::Note(id, _) => exists(*id),
    };
    back.retain(valid);
    forward.retain(valid);
    if valid(&current) {
        None
    } else {
        Some(back.pop().unwrap_or(Location::List(current.listing())))
    }
}

/// The setting that holds the screen the user last viewed, so the window can return to it.
const SCREEN: &str = "linux.screen";

/// The main window: memos, actions, every note, and the trash in tabs under one search bar. A note
/// opens in place of the tabs' content.
pub struct Window {
    actions: NoteList,
    /// Shows the actions in a list, or the inbox's own view.
    actions_content: gtk::Stack,
    actions_results: NoteList,
    actions_stack: gtk::Stack,
    all_notes: Rc<Browser>,
    journals: Rc<JournalsView>,
    /// Locations to return to, most recent last.
    back: RefCell<Vec<Location>>,
    back_button: gtk::Button,
    forward: RefCell<Vec<Location>>,
    inbox: Rc<InboxView>,
    location: Cell<Location>,
    /// Shows the tabs or the open note.
    main: gtk::Stack,
    memos: Rc<Browser>,
    modes: gtk::DropDown,
    note: Rc<NotePage>,
    /// Set while the window shows a location itself, so the tabs' and modes' change signals don't
    /// record it.
    restoring: Cell<bool>,
    search: gtk::SearchEntry,
    /// Whether the search bar holds text.
    searching: Cell<bool>,
    /// A button for each library, which shows or hides its notes.
    spaces_bar: gtk::Box,
    /// The libraries the space buttons stand for, as they last showed.
    spaces_shown: RefCell<Vec<Space>>,
    state: Rc<State>,
    switcher: adw::ViewSwitcher,
    tabs: adw::ViewStack,
    toasts: adw::ToastOverlay,
    trash: Rc<Browser>,
    window: adw::ApplicationWindow,
}

impl Window {
    pub fn new(app: &adw::Application, state: Rc<State>) -> Rc<Self> {
        let toasts = adw::ToastOverlay::new();
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Commonplace")
            .default_width(960)
            .default_height(640)
            .width_request(360)
            .height_request(294)
            .content(&toasts)
            .build();

        let memos = Browser::new(state.clone(), Scope::Memos);
        let all_notes = Browser::new(state.clone(), Scope::All);
        let journals = JournalsView::new(state.clone());
        let actions = NoteList::new(state.clone());
        let modes = gtk::DropDown::from_strings(&MODES.map(|(name, _, _)| name));
        let execute = MODES.iter().position(|(_, _, mode)| *mode == Mode::Execute).unwrap_or(0);
        modes.set_selected(execute as u32);
        let mode_bar =
            gtk::Box::builder().css_classes(["toolbar"]).halign(gtk::Align::Center).build();
        mode_bar.append(&modes);
        let sizes = layout::load(&state);
        let inbox = InboxView::new(state.clone(), toasts.clone());
        // The inbox sorts one action at a time rather than listing them.
        let actions_content = gtk::Stack::new();
        actions_content.add_named(&actions.widget, Some("list"));
        actions_content.add_named(&inbox.widget, Some("inbox"));
        let actions_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        actions_page.append(&mode_bar);
        actions_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        actions_content.set_vexpand(true);
        actions_page.append(&actions_content);
        let actions_results = NoteList::new(state.clone());
        actions_results.set_empty(icon!("search"), "No actions found", "Try other words");
        let actions_stack = gtk::Stack::new();
        actions_stack.add_named(&actions_page, Some("list"));
        actions_stack.add_named(&actions_results.widget, Some("results"));

        let tabs = adw::ViewStack::new();
        let trash = Browser::new(state.clone(), Scope::Trash);
        // The order of the tabs.
        for (page, name, title, icon) in [
            (memos.widget.upcast_ref::<gtk::Widget>(), "memos", "Memos", crate::rows::MEMO_ICON),
            (actions_stack.upcast_ref(), "actions", "Actions", icon!("actions")),
            (journals.widget.upcast_ref(), "journals", "Journals", crate::rows::JOURNAL_ICON),
            (all_notes.widget.upcast_ref(), "all", "All notes", icon!("all-notes")),
            (trash.widget.upcast_ref(), "trash", "Trash", icon!("trash")),
        ] {
            tabs.add_titled_with_icon(page, Some(name), title, icon);
        }

        // Hiding the button would shift the space buttons beside it, so it only fades.
        let back_button = gtk::Button::builder()
            .icon_name(icon!("back"))
            .tooltip_text("Back")
            .opacity(0.0)
            .sensitive(false)
            .build();
        let spaces_bar = gtk::Box::builder().spacing(6).visible(false).build();
        let switcher =
            adw::ViewSwitcher::builder().stack(&tabs).policy(adw::ViewSwitcherPolicy::Wide).build();
        let search = gtk::SearchEntry::builder().placeholder_text("Search").hexpand(true).build();
        let title = gtk::Box::builder().spacing(12).build();
        title.append(&switcher);
        title.append(&search);
        let header = adw::HeaderBar::builder()
            .title_widget(
                &adw::Clamp::builder().maximum_size(800).hexpand(true).child(&title).build(),
            )
            .build();
        header.pack_start(&back_button);
        header.pack_start(&spaces_bar);
        let menu = gio::Menu::new();
        menu.append(Some("_Libraries"), Some("win.libraries"));
        menu.append(Some("_Sync"), Some("win.sync"));
        header.pack_end(
            &gtk::MenuButton::builder()
                .icon_name(icon!("menu"))
                .tooltip_text("Main menu")
                .menu_model(&menu)
                .primary(true)
                .build(),
        );

        let note = NotePage::new(state.clone(), window.clone(), toasts.clone());
        let main = gtk::Stack::new();
        main.add_named(&tabs, Some("tabs"));
        main.add_named(&note.widget, Some("note"));
        let banner = adw::Banner::new(
            "A newer version of Commonplace wrote this library, so it’s read-only",
        );
        banner.set_revealed(state.read(|library| library.is_read_only()));
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&banner);
        toolbar.set_content(Some(&main));
        toasts.set_child(Some(&toolbar));

        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            600.0,
            adw::LengthUnit::Sp,
        ));
        breakpoint.add_setter(
            &switcher,
            "policy",
            Some(&adw::ViewSwitcherPolicy::Narrow.to_value()),
        );
        window.add_breakpoint(breakpoint.clone());

        let this = Rc::new(Self {
            actions,
            actions_content,
            actions_results,
            actions_stack,
            all_notes,
            back: RefCell::default(),
            back_button,
            forward: RefCell::default(),
            inbox,
            journals,
            location: Cell::new(Location::List(Listing::Memos)),
            main,
            memos,
            modes,
            note,
            restoring: Cell::new(false),
            search,
            searching: Cell::new(false),
            spaces_bar,
            spaces_shown: RefCell::default(),
            state,
            switcher,
            tabs,
            toasts,
            trash,
            window,
        });
        this.note.set_details_width(sizes.details);
        this.connect_signals();
        this.add_actions();
        this.refresh();
        this.restore_screen();
        this
    }

    /// Shows the Sync dialog, opening `invite` if there is one.
    pub fn open_sync(&self, invite: Option<&str>) {
        crate::sync::present_dialog(&self.state, &self.window, invite);
    }

    pub fn present(&self) {
        self.window.present();
    }

    /// Closes the window, which hides it instead while the app runs in the tray.
    pub fn close(&self) {
        self.window.close();
    }

    pub fn set_hide_on_close(&self, hide: bool) {
        self.window.set_hide_on_close(hide);
    }

    /// Hides the window if it has the focus on the current workspace, and otherwise shows it there.
    pub fn toggle(&self) {
        // Not every compositor reports a window on another workspace as suspended, but none
        // leaves it focused, and a click on the tray doesn't take the focus from the window.
        let focused = self
            .window
            .surface()
            .and_downcast::<gdk::Toplevel>()
            .is_some_and(|toplevel| toplevel.state().contains(gdk::ToplevelState::FOCUSED));
        if self.window.is_visible() && focused {
            self.window.close();
            return;
        }
        // Compositors put a window that maps again on the current workspace.
        self.window.set_visible(false);
        self.window.present();
    }

    pub fn connect_destroy(&self, f: impl Fn() + 'static) {
        self.window.connect_destroy(move |_| f());
    }

    /// Opens a note in place of the listing it's opened from.
    pub fn open(&self, id: NoteId) {
        if self.state.read(|library| library.note(id)).is_none() {
            self.toast("That note doesn’t exist");
            return;
        }
        self.go(Location::Note(id, self.location.get().listing()));
        self.note.focus_editor();
    }

    fn showing(&self, name: &str) -> bool {
        self.main.visible_child_name().as_deref() == Some(name)
    }

    /// Shows `location`, recording the current location in the history.
    fn go(&self, location: Location) {
        let current = self.location.get();
        if location == current {
            return;
        }
        self.back.borrow_mut().push(current);
        self.forward.borrow_mut().clear();
        self.show(location);
    }

    /// Leaves the open note for the listing it came from, which shows the search results if
    /// there's a search.
    fn return_to_listing(&self) {
        self.go(Location::List(self.location.get().listing()));
    }

    fn go_back(&self) {
        if !self.step(&self.back, &self.forward)
            && let Location::Note(_, listing) = self.location.get()
        {
            self.forward.borrow_mut().push(self.location.get());
            self.show(Location::List(listing));
        }
    }

    fn update_back_button(&self) {
        let available =
            !self.back.borrow().is_empty() || matches!(self.location.get(), Location::Note(_, _));
        self.back_button.set_opacity(if available { 1.0 } else { 0.0 });
        self.back_button.set_sensitive(available);
    }

    /// Moves to the most recent location in `from` whose note still exists, and remembers the
    /// current location in `to`. Returns whether there was one.
    fn step(&self, from: &RefCell<Vec<Location>>, to: &RefCell<Vec<Location>>) -> bool {
        loop {
            let location = from.borrow_mut().pop();
            let Some(location) = location else {
                self.update_back_button();
                return false;
            };
            if let Location::Note(id, _) = location
                && self.state.read(|library| library.note(id)).is_none()
            {
                continue;
            }
            to.borrow_mut().push(self.location.get());
            self.show(location);
            return true;
        }
    }

    fn remove_missing_notes(&self) {
        let next = self.state.read(|spaces| {
            remove_missing_notes(
                self.location.get(),
                &mut self.back.borrow_mut(),
                &mut self.forward.borrow_mut(),
                |id| spaces.note(id).is_some(),
            )
        });
        if let Some(next) = next {
            self.show(next);
        } else {
            self.update_back_button();
        }
    }

    fn cycle_tab(&self, backwards: bool) {
        let pages = self.tabs.pages();
        let count = pages.n_items();
        let current = (0..count).find(|&index| pages.is_selected(index)).unwrap_or(0);
        let next = if backwards { (current + count - 1) % count } else { (current + 1) % count };
        pages.select_item(next, true);
    }

    fn show(&self, location: Location) {
        self.restoring.set(true);
        let listing = location.listing();
        match listing {
            Listing::Actions(mode) => {
                self.tabs.set_visible_child_name("actions");
                let index = MODES.iter().position(|(_, _, listed)| *listed == mode);
                if let Some(index) = index {
                    self.modes.set_selected(index as u32);
                }
            }
            Listing::AllNotes => self.tabs.set_visible_child_name("all"),
            Listing::Journals => self.tabs.set_visible_child_name("journals"),
            Listing::Memos => self.tabs.set_visible_child_name("memos"),
            Listing::Trash => self.tabs.set_visible_child_name("trash"),
        }
        match location {
            Location::List(_) => {
                if self.showing("note") {
                    self.note.close();
                }
                self.main.set_visible_child_name("tabs");
                let tab = self.tabs.visible_child().and_then(|page| self.tabs.page(&page).title());
                let title =
                    tab.map_or_else(|| "Commonplace".to_owned(), |tab| crate::window_title(&tab));
                self.window.set_title(Some(&title));
            }
            Location::Note(id, _) => {
                self.main.set_visible_child_name("note");
                self.note.show(id);
            }
        }
        self.location.set(location);
        self.update_back_button();
        self.restoring.set(false);
        self.state.device().set_text_setting(SCREEN, &location.encode());
    }

    /// Returns to the screen the user last viewed on this device, or to the memos if that screen's
    /// note is gone.
    fn restore_screen(&self) {
        let saved = self.state.device().text_setting(SCREEN);
        let location = saved
            .and_then(|text| Location::decode(&text))
            .filter(|location| match location {
                Location::Note(id, _) => self.state.read(|library| library.note(*id)).is_some(),
                Location::List(_) => true,
            })
            .unwrap_or(Location::List(Listing::Memos));
        self.show(location);
    }

    /// The listing the tab bar and the actions sidebar show as selected.
    fn tab(&self) -> Listing {
        match self.tabs.visible_child_name().as_deref() {
            Some("actions") => Listing::Actions(self.mode()),
            Some("all") => Listing::AllNotes,
            Some("journals") => Listing::Journals,
            Some("trash") => Listing::Trash,
            _ => Listing::Memos,
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        self.state.connect_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move || {
                this.remove_missing_notes();
                this.refresh();
                this.note.refresh();
            }
        ));
        for list in [&self.actions, &self.actions_results] {
            list.connect_open(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |id| this.open(id)
            ));
        }
        self.journals.connect_open(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |id| this.open(id)
        ));
        self.journals.connect_new(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move || this.new_journal()
        ));
        for browser in [&self.memos, &self.all_notes, &self.trash] {
            browser.connect_open(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |id| this.open(id)
            ));
        }
        self.note.connect_leave(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move || this.return_to_listing()
        ));
        self.back_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.go_back()
        ));
        self.note.connect_open(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |id| this.open(id)
        ));
        self.tabs.connect_visible_child_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                if !this.restoring.get() {
                    this.go(Location::List(this.tab()));
                }
            }
        ));
        // Clicking the tab that's already selected changes nothing in the stack, but still means
        // leaving the open note for that tab. The release comes before the tabs switch, so the
        // check waits until another tab has had its chance to take over.
        let switcher_click =
            gtk::GestureClick::builder().propagation_phase(gtk::PropagationPhase::Capture).build();
        switcher_click.connect_released(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _, _, _| {
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    this,
                    move || this.go(Location::List(this.tab()))
                ));
            }
        ));
        self.switcher.add_controller(switcher_click);
        self.modes.connect_selected_notify(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.refresh_actions();
                if !this.restoring.get() {
                    this.go(Location::List(Listing::Actions(this.mode())));
                }
            }
        ));
        self.search.connect_search_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |search| {
                let searching = !search.text().is_empty();
                // Starting a search from a note means looking for another note.
                if searching && !this.searching.replace(searching) && this.showing("note") {
                    this.go(Location::List(this.tab()));
                }
                this.searching.set(searching);
                this.refresh_search();
            }
        ));
        self.search.connect_stop_search(|search| search.set_text(""));
        // Typing over the whole text deletes it and inserts the new text as one change, so the
        // entry never reports being empty. Deleting everything counts as clearing the search.
        if let Some(text) = self.search.delegate().and_downcast::<gtk::Text>() {
            text.connect_delete_text(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |text, start, end| {
                    let length = text.text().chars().count() as i32;
                    if start <= 0 && (end < 0 || end >= length) {
                        this.searching.set(false);
                    }
                }
            ));
        }

        // The editor uses Alt+Left/Right to move words and Ctrl+Tab to move focus unless the
        // window handles them first.
        let navigation_keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        navigation_keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
                let control = gdk::ModifierType::CONTROL_MASK;
                let shift = gdk::ModifierType::SHIFT_MASK;
                if matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab)
                    && (modifiers == control || modifiers == control | shift)
                {
                    this.cycle_tab(modifiers.contains(shift) || key == gdk::Key::ISO_Left_Tab);
                    return glib::Propagation::Stop;
                }
                let alt = modifiers == gdk::ModifierType::ALT_MASK;
                let stepped = match key {
                    gdk::Key::Back => this.step(&this.back, &this.forward),
                    gdk::Key::Left if alt => this.step(&this.back, &this.forward),
                    gdk::Key::Forward => this.step(&this.forward, &this.back),
                    gdk::Key::Right if alt => this.step(&this.forward, &this.back),
                    _ => return glib::Propagation::Proceed,
                };
                if stepped { glib::Propagation::Stop } else { glib::Propagation::Proceed }
            }
        ));
        self.window.add_controller(navigation_keys);
        let history_buttons = gtk::GestureClick::builder()
            .button(0)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        history_buttons.connect_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, _, _| {
                let stepped = match gesture.current_button() {
                    8 => this.step(&this.back, &this.forward),
                    9 => this.step(&this.forward, &this.back),
                    _ => false,
                };
                if stepped {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        ));
        self.window.add_controller(history_buttons);
        self.window.connect_close_request(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                this.note.save_now();
                this.inbox.save_now();
                let details = this.note.details_width();
                layout::save(&this.state, layout::Sizes { details });
                glib::Propagation::Proceed
            }
        ));
    }

    fn add_actions(self: &Rc<Self>) {
        let follow_link = gio::SimpleAction::new("follow-link", None);
        follow_link.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                if this.showing("note") {
                    this.note.follow_link_at_cursor();
                }
            }
        ));
        let insert_link = gio::SimpleAction::new("insert-link", None);
        insert_link.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                if this.showing("note") {
                    this.note.insert_link();
                }
            }
        ));
        let save = gio::SimpleAction::new("save", None);
        save.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.note.save_now()
        ));
        let search = gio::SimpleAction::new("search", None);
        search.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                this.search.grab_focus();
                this.search.select_region(0, -1);
            }
        ));
        let toggle_checkbox = gio::SimpleAction::new("toggle-checkbox", None);
        toggle_checkbox.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                if this.showing("note") {
                    this.note.toggle_checkbox_at_cursor();
                }
            }
        ));
        let toggle_space = gio::SimpleAction::new("toggle-space", Some(glib::VariantTy::INT32));
        toggle_space.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, parameter| {
                let index = parameter.and_then(|value| value.get::<i32>()).unwrap_or(0) - 1;
                let spaces = this.state.spaces();
                if spaces.len() > 1
                    && let Some(space) = usize::try_from(index).ok().and_then(|i| spaces.get(i))
                {
                    this.state.set_shown(&space.key, !space.shown);
                }
            }
        ));
        let libraries = gio::SimpleAction::new("libraries", None);
        libraries.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| libraries::present_dialog(&this.state, &this.window)
        ));
        let sync = gio::SimpleAction::new("sync", None);
        sync.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.open_sync(None)
        ));
        for action in
            [follow_link, insert_link, libraries, save, search, sync, toggle_checkbox, toggle_space]
        {
            self.window.add_action(&action);
        }
    }

    /// Shows a button for each library while there's more than one, and asks for the name of any
    /// library this device hasn't named.
    fn refresh_spaces(self: &Rc<Self>) {
        let spaces = self.state.spaces();
        if *self.spaces_shown.borrow() == spaces {
            return;
        }
        let named = |list: &[Space]| -> Vec<(String, String)> {
            list.iter().map(|space| (space.key.clone(), space.name.clone())).collect()
        };
        let same_buttons = named(&self.spaces_shown.borrow()) == named(&spaces);
        *self.spaces_shown.borrow_mut() = spaces.clone();
        if same_buttons {
            let mut child = self.spaces_bar.first_child();
            for space in &spaces {
                if let Some(button) =
                    child.as_ref().and_then(|c| c.downcast_ref::<gtk::ToggleButton>())
                {
                    button.set_active(space.shown);
                }
                child = child.and_then(|c| c.next_sibling());
            }
            return;
        }
        while let Some(child) = self.spaces_bar.first_child() {
            self.spaces_bar.remove(&child);
        }
        for (index, space) in spaces.iter().enumerate() {
            let button = gtk::ToggleButton::builder()
                .label(&space.name)
                .active(space.shown)
                .tooltip_text(format!("Show or Hide {} (Alt+{})", space.name, index + 1))
                .build();
            let key = space.key.clone();
            button.connect_toggled(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |button| {
                    if this.state.read(|spaces| spaces.is_shown(&key)) != button.is_active() {
                        this.state.set_shown(&key, button.is_active());
                    }
                }
            ));
            self.spaces_bar.append(&button);
        }
        self.spaces_bar.set_visible(spaces.len() > 1);
        libraries::name_unnamed(&self.state, &self.window);
    }

    fn mode(&self) -> Mode {
        let index = self.modes.selected();
        MODES.get(index as usize).map_or(Mode::Execute, |(_, _, mode)| *mode)
    }

    fn refresh(self: &Rc<Self>) {
        self.refresh_spaces();
        self.refresh_actions();
        self.refresh_search();
    }

    fn refresh_actions(&self) {
        if self.mode() == Mode::Inbox {
            self.actions_content.set_visible_child_name("inbox");
            self.inbox.refresh();
            return;
        }
        self.actions_content.set_visible_child_name("list");
        let today = today();
        let view = |view| self.state.read(|library| library.action_view(view, today));
        let sections = match self.mode() {
            Mode::Execute => {
                self.actions.set_empty(
                    icon!("execute"),
                    "Nothing to do next",
                    "Starred actions and tasks on the active list show up here",
                );
                let next = view(ActionView::Next);
                let status = |status| {
                    next.iter().filter(|entry| entry.dates.status == status).cloned().collect()
                };
                vec![
                    Section { star: true, ..section("Starred", view(ActionView::Starred), false) },
                    section("Overdue", status(ActionStatus::Overdue), false),
                    section("Due soon", status(ActionStatus::DueSoon), false),
                    section("Active", status(ActionStatus::Normal), false),
                    section("Starting soon", status(ActionStatus::Upcoming), false),
                ]
            }
            Mode::Plan => {
                self.actions.set_empty(
                    icon!("edit"),
                    "Nothing to plan",
                    "Actions queued for planning and stalled projects show up here",
                );
                let queue = view(ActionView::Planning);
                let mut stalled = view(ActionView::Stalled);
                let queued: Vec<NoteId> = queue.iter().map(|entry| entry.note.id).collect();
                stalled.retain(|entry| !queued.contains(&entry.note.id));
                vec![
                    section("Planning queue", queue, true),
                    section("Stalled projects", stalled, true),
                ]
            }
            Mode::Inbox => unreachable!("the inbox has its own view"),
            Mode::Review => {
                self.actions.set_empty(
                    icon!("review"),
                    "Nothing to review",
                    "Projects, and waiting, scheduled, someday, and maybe actions show up here",
                );
                let since = glib::real_time() / 1000 - RECENT_DAYS * 24 * 60 * 60 * 1000;
                let mut done = view(ActionView::Closed);
                done.retain(|entry| {
                    entry.note.action.as_ref().and_then(|action| action.closed) >= Some(since)
                });
                // An action in the planning queue waits in Plan instead.
                let unqueued = |view| {
                    let mut entries: Vec<ActionEntry> =
                        self.state.read(|library| library.action_view(view, today));
                    entries.retain(|entry| {
                        entry.note.action.as_ref().is_none_or(|action| action.planning.is_none())
                    });
                    entries
                };
                vec![
                    section("Projects", unqueued(ActionView::Projects), true),
                    section("Waiting for", unqueued(ActionView::Waiting), true),
                    section("Scheduled", unqueued(ActionView::Scheduled), true),
                    section("Someday", unqueued(ActionView::Someday), true),
                    section("Maybe", unqueued(ActionView::Maybe), true),
                    section("Recently done", done, false),
                ]
            }
        };
        self.actions.show(sections);
    }

    /// Lists the notes in every tab that match the search. The actions tab shows its matches in
    /// place of its listing while there's a search, and the others filter their lists.
    fn refresh_search(&self) {
        let query = self.search.text();
        self.memos.refresh(&query);
        self.all_notes.refresh(&query);
        self.journals.refresh(&query);
        self.trash.refresh(&query);
        let searching = !query.trim().is_empty();
        self.actions_stack.set_visible_child_name(if searching { "results" } else { "list" });
        if !searching {
            return;
        }
        let actions: Vec<ActionEntry> = self
            .state
            .read(|library| library.action_view(ActionView::All, today()))
            .into_iter()
            .filter(|entry| entry.note.matches(&query))
            .map(|entry| ActionEntry { depth: 0, ..entry })
            .collect();
        self.actions_results.show(vec![section("", actions, false)]);
    }

    /// Asks for a name, and a library if there's more than one, and opens the new journal.
    fn new_journal(self: &Rc<Self>) {
        let name = gtk::Entry::builder().placeholder_text("Name").activates_default(true).build();
        let spaces = self.state.spaces();
        let names: Vec<&str> = spaces.iter().map(|space| space.name.as_str()).collect();
        let library = gtk::DropDown::from_strings(&names);
        let start = self.state.capture_space();
        library
            .set_selected(spaces.iter().position(|space| space.key == start).unwrap_or(0) as u32);
        library.set_visible(spaces.len() > 1);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.append(&name);
        content.append(&library);
        let dialog = adw::AlertDialog::new(Some("New journal"), None);
        dialog.add_responses(&[("cancel", "_Cancel"), ("create", "_Create")]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        dialog.set_extra_child(Some(&content));
        dialog.connect_response(
            Some("create"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    let text = name.text().trim().to_owned();
                    let Some(space) = spaces.get(library.selected() as usize) else {
                        return;
                    };
                    if text.is_empty() {
                        return;
                    }
                    match this.state.create_journal(&space.key, &text) {
                        Ok(id) => this.open(id),
                        Err(error) => this.toast(&error.to_string()),
                    }
                }
            ),
        );
        dialog.present(Some(&self.window));
    }

    fn toast(&self, title: &str) {
        self.toasts.add_toast(adw::Toast::new(title));
    }
}

fn section(title: &'static str, entries: Vec<ActionEntry>, plan: bool) -> Section {
    Section { entries, plan, stamp: None, star: false, title }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs an isolated GTK Broadway display and temporary sample library"]
    fn header_back_traverses_notes_and_listings() {
        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("broadway"));
        let dir = std::path::PathBuf::from(
            std::env::var_os("COMMONPLACE_LIBRARY").expect("a temporary sample library"),
        );
        assert!(dir.starts_with(std::env::temp_dir()));
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id(format!(
                "systems.thoughtfull.CommonplaceAgent.History{}",
                std::process::id()
            ))
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let mut spaces = commonplace_core::Spaces::open(&dir).unwrap();
        let key = spaces.keys().first().unwrap().clone();
        let first = spaces.create_memo(&key, "History first").unwrap();
        let second = spaces.create_memo(&key, "History second").unwrap();
        let state = Rc::new(State::new(spaces, crate::device::Device::open(&dir)));
        let window = Window::new(&app, state.clone());
        window.present();
        crate::journals_view::check_chart_hover();
        window.note.check_details_and_trash(second);
        assert_eq!(window.modes.selected(), 0);
        let list = Location::List(Listing::Memos);
        assert_eq!(window.location.get(), list);
        assert!(!window.back_button.is_sensitive());

        window.open(first);
        window.open(second);
        window.back_button.emit_clicked();
        assert_eq!(window.location.get(), Location::Note(first, Listing::Memos));
        assert!(window.showing("note"));
        assert!(window.step(&window.forward, &window.back));
        assert_eq!(window.location.get(), Location::Note(second, Listing::Memos));
        window.back_button.emit_clicked();
        window.back_button.emit_clicked();
        assert_eq!(window.location.get(), list);
        assert!(window.showing("tabs"));
        assert!(!window.back_button.is_sensitive());
        assert_eq!(window.back_button.opacity(), 0.0);

        window.tabs.set_visible_child_name("journals");
        assert_eq!(window.location.get(), Location::List(Listing::Journals));
        assert!(window.back_button.is_sensitive());
        window.back_button.emit_clicked();
        assert_eq!(window.location.get(), list);
        assert!(!window.back_button.is_sensitive());

        window.open(first);
        window.open(second);
        state
            .write(|spaces| {
                spaces.trash(first)?;
                spaces.delete(first)
            })
            .unwrap();
        window.back_button.emit_clicked();
        assert_eq!(window.location.get(), list);
        assert!(!window.back_button.is_sensitive());

        let saved = Location::Note(second, Listing::Journals);
        window.show(saved);
        window.close();
        drop(window);
        let restored = Window::new(&app, state);
        assert_eq!(restored.location.get(), saved);
        assert!(restored.back.borrow().is_empty());
        assert!(restored.back_button.is_sensitive());
        restored.back_button.emit_clicked();
        assert_eq!(restored.location.get(), Location::List(Listing::Journals));
        assert!(!restored.back_button.is_sensitive());
        assert!(restored.step(&restored.forward, &restored.back));
        assert_eq!(restored.location.get(), saved);
        restored.close();
    }

    #[test]
    fn deleting_notes_prunes_both_directions_and_returns_to_previous_note() {
        let deleted = NoteId::generate();
        let previous = Location::Note(NoteId::generate(), Listing::Memos);
        let next = Location::Note(NoteId::generate(), Listing::AllNotes);
        let current = Location::Note(deleted, Listing::Trash);
        let list = Location::List(Listing::Memos);
        let mut back = vec![list, current, previous, current];
        let mut forward = vec![current, next, current];

        let destination =
            remove_missing_notes(current, &mut back, &mut forward, |id| id != deleted);

        assert_eq!(destination, Some(previous));
        assert_eq!(back, vec![list]);
        assert_eq!(forward, vec![next]);
    }

    #[test]
    fn deleting_without_history_returns_to_the_notes_listing() {
        let current = Location::Note(NoteId::generate(), Listing::Trash);
        let mut back = vec![current];
        let mut forward = vec![current];

        let destination = remove_missing_notes(current, &mut back, &mut forward, |_| false);

        assert_eq!(destination, Some(Location::List(Listing::Trash)));
        assert!(back.is_empty());
        assert!(forward.is_empty());
    }

    #[test]
    fn deleting_another_note_preserves_the_current_location_and_history_order() {
        let deleted = NoteId::generate();
        let current = Location::Note(NoteId::generate(), Listing::Memos);
        let list = Location::List(Listing::Memos);
        let removed = Location::Note(deleted, Listing::Trash);
        let mut back = vec![list, removed, current];
        let mut forward = vec![removed, list];

        let destination =
            remove_missing_notes(current, &mut back, &mut forward, |id| id != deleted);

        assert_eq!(destination, None);
        assert_eq!(back, vec![list, current]);
        assert_eq!(forward, vec![list]);
    }
}
