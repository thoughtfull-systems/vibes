// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{ActionEntry, ActionView, Date, Note, NoteId};
use gtk::glib;

use crate::rows::{
    NoteList, Section, Stamp, connect_day_clicked, day_picker, entry, format_date, today,
};
use crate::state::State;

/// Which notes a `Browser` lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    All,
    Memos,
    Trash,
}

impl Scope {
    /// The orders the scope offers, in the order the picker lists them.
    fn orders(self) -> &'static [Order] {
        match self {
            Self::All => {
                &[Order::Created, Order::Modified, Order::Due, Order::Scheduled, Order::Archived]
            }
            Self::Memos => &[Order::Created, Order::Modified],
            Self::Trash => &[Order::Trashed, Order::Created, Order::Modified],
        }
    }

    fn default_order(self) -> Order {
        match self {
            Self::All | Self::Memos => Order::Created,
            Self::Trash => Order::Trashed,
        }
    }

    /// The prefix of the settings that keep the scope's filters.
    fn settings(self) -> &'static str {
        match self {
            Self::All => "linux.filter.all",
            Self::Memos => "linux.filter.memos",
            Self::Trash => "linux.filter.trash",
        }
    }
}

/// What a `Browser` orders notes by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Order {
    /// When a note was archived. Ordering by it lists only archived notes, so it doubles as a way
    /// to find the archive.
    Archived,
    Created,
    /// An action's own due date, not one it inherits.
    Due,
    Modified,
    /// An action's own start date, not one it inherits.
    Scheduled,
    Trashed,
}

impl Order {
    /// The number the settings store for the order. Changing one breaks saved settings.
    fn code(self) -> i64 {
        match self {
            Self::Created => 0,
            Self::Modified => 1,
            Self::Due => 2,
            Self::Scheduled => 3,
            Self::Trashed => 4,
            Self::Archived => 5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Archived => "Archived",
            Self::Created => "Created",
            Self::Due => "Due",
            Self::Modified => "Modified",
            Self::Scheduled => "Scheduled",
            Self::Trashed => "Trashed",
        }
    }

    fn stamp(self) -> Option<Stamp> {
        match self {
            Self::Archived => Some(Stamp::Archived),
            Self::Created => Some(Stamp::Created),
            Self::Modified => Some(Stamp::Modified),
            Self::Trashed => Some(Stamp::Trashed),
            // An action's row already shows its dates.
            Self::Due | Self::Scheduled => None,
        }
    }

    /// The value `note` sorts by, and its day, or `None` if the note has no such date.
    fn key(self, note: &Note) -> Option<(i64, Date)> {
        let time = |milliseconds: i64| local_day(milliseconds).map(|day| (milliseconds, day));
        let date = |date: Date| {
            let key = i64::from(date.year()) * 10_000
                + i64::from(date.month()) * 100
                + i64::from(date.day());
            (key, date)
        };
        match self {
            Self::Archived => note.archived.and_then(time),
            Self::Created => time(note.created),
            Self::Modified => time(note.modified),
            Self::Trashed => time(note.trashed.unwrap_or(note.modified)),
            Self::Due => note.action.as_ref().and_then(|action| action.due).map(date),
            Self::Scheduled => note.action.as_ref().and_then(|action| action.defer).map(date),
        }
    }
}

/// A tab's notes that match the search, ordered by one of their dates. With a day picked, the
/// list starts there and runs in the order's direction. The device keeps the filters.
pub struct Browser {
    /// The day the list starts from, if the user picked one.
    from: Cell<Option<Date>>,
    from_button: gtk::MenuButton,
    list: NoteList,
    /// Set while the browser changes its controls itself, so their signals don't echo back.
    loading: Cell<bool>,
    newest_first: gtk::ToggleButton,
    order: gtk::DropDown,
    query: RefCell<String>,
    reset: gtk::Button,
    scope: Scope,
    state: Rc<State>,
    pub widget: gtk::Box,
}

impl Browser {
    pub fn new(state: Rc<State>, scope: Scope) -> Rc<Self> {
        let list = NoteList::new(state.clone());
        let labels: Vec<&str> = scope.orders().iter().map(|order| order.label()).collect();
        let order = gtk::DropDown::from_strings(&labels);
        let newest_first = gtk::ToggleButton::new();
        let any_day = gtk::Button::with_mnemonic("_Any day");
        let (from_button, calendar) = day_picker("Any day", Some(any_day.upcast_ref()));
        let reset = gtk::Button::with_mnemonic("_Reset");
        let dimmed = |text: &str| gtk::Label::builder().label(text).css_classes(["dimmed"]).build();
        let controls = gtk::Box::builder().spacing(6).halign(gtk::Align::Center).build();
        controls.append(&dimmed("Order by"));
        controls.append(&order);
        controls.append(&newest_first);
        controls.append(&dimmed("From"));
        controls.append(&from_button);
        controls.append(&reset);
        let bar = gtk::Box::builder().css_classes(["toolbar"]).build();
        controls.set_hexpand(true);
        bar.append(&controls);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&bar);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&list.widget);

        let browser = Rc::new(Self {
            from: Cell::new(None),
            from_button,
            list,
            loading: Cell::new(false),
            newest_first,
            order,
            query: RefCell::default(),
            reset,
            scope,
            state,
            widget,
        });
        browser.load();
        connect_day_clicked(
            &browser.from_button,
            &calendar,
            glib::clone!(
                #[weak]
                browser,
                move |day| browser.set_from(Some(day))
            ),
        );
        any_day.connect_clicked(glib::clone!(
            #[weak]
            browser,
            move |_| {
                browser.from_button.popdown();
                browser.set_from(None);
            }
        ));
        browser.order.connect_selected_notify(glib::clone!(
            #[weak]
            browser,
            move |_| browser.changed()
        ));
        browser.newest_first.connect_toggled(glib::clone!(
            #[weak]
            browser,
            move |_| browser.changed()
        ));
        browser.reset.connect_clicked(glib::clone!(
            #[weak]
            browser,
            move |_| {
                browser.set_controls(scope.default_order(), true, None);
                browser.changed();
            }
        ));
        browser
    }

    /// Calls `f` when the user activates a note's row.
    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        self.list.connect_open(f);
    }

    /// Lists the notes that match `query` again.
    pub fn refresh(&self, query: &str) {
        *self.query.borrow_mut() = query.to_owned();
        self.show();
    }

    fn selected_order(&self) -> Order {
        let orders = self.scope.orders();
        orders.get(self.order.selected() as usize).copied().unwrap_or(orders[0])
    }

    /// Shows the filters saved on the device, or the defaults.
    fn load(&self) {
        let setting = |name: &str| {
            let key = format!("{}.{name}", self.scope.settings());
            self.state.device().setting(&key)
        };
        let order = setting("order")
            .and_then(|code| self.scope.orders().iter().copied().find(|o| o.code() == code))
            .unwrap_or(self.scope.default_order());
        let newest_first = setting("newest_first").is_none_or(|value| value != 0);
        let from = setting("from").and_then(|value| {
            let date = |part: i64| u8::try_from(part).ok();
            Date::new(
                u16::try_from(value / 10_000).ok()?,
                date(value / 100 % 100)?,
                date(value % 100)?,
            )
        });
        self.set_controls(order, newest_first, from);
    }

    fn set_controls(&self, order: Order, newest_first: bool, from: Option<Date>) {
        self.loading.set(true);
        let index = self.scope.orders().iter().position(|listed| *listed == order).unwrap_or(0);
        self.order.set_selected(index as u32);
        self.newest_first.set_active(newest_first);
        self.loading.set(false);
        self.from.set(from);
        self.from_button.set_label(&from.map_or_else(|| "Any day".to_owned(), format_date));
    }

    fn set_from(&self, day: Option<Date>) {
        self.set_controls(self.selected_order(), self.newest_first.is_active(), day);
        self.changed();
    }

    /// Saves the filters after the user changes them, and lists the notes again.
    fn changed(&self) {
        if self.loading.get() {
            return;
        }
        let from = self.from.get().map_or(0, |day| {
            i64::from(day.year()) * 10_000 + i64::from(day.month()) * 100 + i64::from(day.day())
        });
        for (name, value) in [
            ("from", from),
            ("newest_first", i64::from(self.newest_first.is_active())),
            ("order", self.selected_order().code()),
        ] {
            let key = format!("{}.{name}", self.scope.settings());
            self.state.device().set_setting(&key, value);
        }
        self.show();
    }

    fn show(&self) {
        let order = self.selected_order();
        let newest_first = self.newest_first.is_active();
        let (icon, tooltip) = if newest_first {
            ("view-sort-descending-symbolic", "Latest first")
        } else {
            ("view-sort-ascending-symbolic", "Earliest first")
        };
        self.newest_first.set_icon_name(icon);
        self.newest_first.set_tooltip_text(Some(tooltip));
        let from = self.from.get();
        self.reset
            .set_sensitive(order != self.scope.default_order() || !newest_first || from.is_some());

        let query = self.query.borrow().clone();
        let scope = self.scope;
        let (notes, dates, paths, starred_actions) = self.state.read(|library| {
            let in_trash = library.notes_in_trash();
            let notes: Vec<Note> = library
                .notes()
                .into_iter()
                .filter(|note| match scope {
                    Scope::All => !in_trash.contains(&note.id),
                    Scope::Memos => {
                        note.is_memo() && note.archived.is_none() && !in_trash.contains(&note.id)
                    }
                    Scope::Trash => note.is_trashed(),
                })
                .filter(|note| note.matches(&query))
                .collect();
            let starred_actions: HashSet<NoteId> = library
                .action_view(ActionView::Starred, today())
                .into_iter()
                .map(|entry| entry.note.id)
                .collect();
            (notes, library.action_dates(today()), library.action_paths(), starred_actions)
        });
        // Notes without the order's date follow the rest, newest first, except in the archive.
        let (mut dated, undated): (Vec<_>, Vec<_>) = notes
            .into_iter()
            .map(|note| (order.key(&note), note))
            .partition(|(key, _)| key.is_some());
        if let Some(from) = from {
            dated.retain(|(key, _)| {
                key.is_some_and(|(_, day)| if newest_first { day <= from } else { day >= from })
            });
        }
        dated.sort_by_key(|(key, _)| key.map(|(value, _)| value));
        if newest_first {
            dated.reverse();
        }
        let mut undated =
            if from.is_some() || order == Order::Archived { Vec::new() } else { undated };
        undated.sort_by_key(|(_, note)| std::cmp::Reverse(note.created));
        let entries: Vec<ActionEntry> =
            dated.into_iter().chain(undated).map(|(_, note)| entry(note, &dates, &paths)).collect();
        // An action counts as starred only while it and the projects above it are open.
        let (starred, others): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| {
            let note = &entry.note;
            scope != Scope::Trash
                && note.starred.is_some()
                && note.archived.is_none()
                && (note.action.is_none() || starred_actions.contains(&note.id))
        });

        let filtered = !query.trim().is_empty() || from.is_some();
        let (icon, title, description) = match (scope, filtered) {
            (Scope::All, false) => {
                ("document-edit-symbolic", "No notes", "Capture a memo or an action to start")
            }
            (Scope::Memos, false) => {
                (crate::rows::MEMO_ICON, "No memos", "Capture one with commonplace capture memo")
            }
            (Scope::Trash, false) => {
                ("user-trash-symbolic", "Trash is empty", "Notes you trash wait here")
            }
            (Scope::All, true) => {
                ("system-search-symbolic", "No notes found", "Try other words or another day")
            }
            (Scope::Memos, true) => {
                ("system-search-symbolic", "No memos found", "Try other words or another day")
            }
            (Scope::Trash, true) => (
                "system-search-symbolic",
                "Nothing found in the trash",
                "Try other words or another day",
            ),
        };
        self.list.set_empty(icon, title, description);
        let others_title = match (starred.is_empty(), scope) {
            (true, _) => "",
            (false, Scope::Memos) => "Other memos",
            (false, _) => "Other notes",
        };
        let stamp = order.stamp();
        self.list.show(vec![
            Section { entries: starred, plan: false, stamp, star: true, title: "Starred" },
            Section { entries: others, plan: false, stamp, star: false, title: others_title },
        ]);
    }
}

/// The local calendar day of a timestamp in milliseconds.
fn local_day(milliseconds: i64) -> Option<Date> {
    let time = glib::DateTime::from_unix_local(milliseconds.div_euclid(1000)).ok()?;
    Date::new(time.year() as u16, time.month() as u8, time.day_of_month() as u8)
}
