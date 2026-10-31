// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! The history of a recurring action: its past and upcoming occurrences, each with a menu that
//! closes, skips, moves, or reopens it, or writes a note for it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Date, NoteId, Occurrence, OccurrenceState, OccurrenceStatus};
use gtk::{gio, glib};

use crate::rows::{format_date, today};
use crate::state::State;

/// How many past occurrences the history shows at first, and how many more each Show earlier adds.
const PAST: u32 = 10;
/// How many occurrences after the current one the history shows.
const UPCOMING: u32 = 5;

type Opener = Rc<dyn Fn(NoteId)>;
/// What an item in an occurrence's menu does.
type Run = Box<dyn Fn(&Rc<History>)>;

struct History {
    dialog: adw::Dialog,
    id: NoteId,
    list: gtk::Box,
    open: Opener,
    past: Cell<u32>,
    state: Rc<State>,
}

/// Opens the history of the recurring action `id`. `open` opens a note written for an occurrence.
pub fn present(state: &Rc<State>, parent: &impl IsA<gtk::Widget>, id: NoteId, open: Opener) {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(520)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .build();
    content.append(&scroll);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    let dialog = adw::Dialog::builder().title("History").content_width(420).child(&toolbar).build();
    let history = Rc::new(History {
        dialog: dialog.clone(),
        id,
        list,
        open,
        past: Cell::new(PAST),
        state: state.clone(),
    });
    let weak = Rc::downgrade(&history);
    state.connect_changed_while(move || {
        let open = weak.upgrade();
        if let Some(history) = &open {
            history.fill();
        }
        open.is_some()
    });
    history.fill();
    // The dialog keeps the history alive until it closes, then lets it go, which also lets go of
    // the dialog the history holds.
    let keeper = RefCell::new(Some(history));
    dialog.connect_closed(move |_| drop(keeper.take()));
    dialog.present(Some(parent));
}

impl History {
    fn fill(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let occurrences = self
            .state
            .read(|spaces| spaces.occurrences(self.id, today(), self.past.get(), UPCOMING));
        let current = occurrences.iter().find(|o| o.status == OccurrenceStatus::Current);
        let current = current.map(|o| o.date);
        let (upcoming, past): (Vec<_>, Vec<_>) = occurrences.into_iter().partition(|o| {
            matches!(o.status, OccurrenceStatus::Current | OccurrenceStatus::Upcoming)
                || current.is_some_and(|current| o.date > current)
        });
        for (title, items) in [("Upcoming", upcoming), ("Past", past.clone())] {
            if items.is_empty() {
                continue;
            }
            let rows = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            for occurrence in &items {
                rows.append(&self.row(occurrence));
            }
            let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
            section.append(
                &gtk::Label::builder().label(title).xalign(0.0).css_classes(["heading"]).build(),
            );
            section.append(&rows);
            self.list.append(&section);
        }
        if past.len() as u32 >= self.past.get() {
            let earlier = gtk::Button::builder()
                .label("Show earlier")
                .halign(gtk::Align::Start)
                .css_classes(["flat"])
                .build();
            earlier.connect_clicked(glib::clone!(
                #[weak(rename_to = history)]
                self,
                move |_| {
                    history.past.set(history.past.get() + PAST);
                    history.fill();
                }
            ));
            self.list.append(&earlier);
        }
        if self.list.first_child().is_none() {
            self.list.append(
                &gtk::Label::builder().label("No occurrences").css_classes(["dimmed"]).build(),
            );
        }
    }

    fn row(self: &Rc<Self>, occurrence: &Occurrence) -> adw::ActionRow {
        let (icon, word) = match occurrence.status {
            OccurrenceStatus::Current => (icon!("repeat"), Some("this time".to_owned())),
            OccurrenceStatus::Upcoming => {
                (icon!("plan"), (occurrence.on <= today()).then(|| "waiting".to_owned()))
            }
            OccurrenceStatus::Done => (
                icon!("actions"),
                occurrence.handled.map(|handled| format!("done {}", format_date(handled.day))),
            ),
            OccurrenceStatus::Skipped => (icon!("remove"), Some("skipped".to_owned())),
            OccurrenceStatus::Missed => (icon!("clear-search"), Some("missed".to_owned())),
        };
        let mut details: Vec<String> = word.into_iter().collect();
        if occurrence.on != occurrence.date {
            details.push(format!("moved from {}", format_date(occurrence.date)));
        }
        if occurrence.note.is_some() {
            details.push("has a note".to_owned());
        }
        let row = adw::ActionRow::builder()
            .title(format_date(occurrence.on))
            .subtitle(details.join(" · "))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        let menu = gio::Menu::new();
        let actions = gio::SimpleActionGroup::new();
        let add = |label: &str, name: &str, run: Run| {
            menu.append(Some(label), Some(&format!("occurrence.{name}")));
            let action = gio::SimpleAction::new(name, None);
            let history = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(history) = history.upgrade() {
                    run(&history);
                }
            });
            actions.add_action(&action);
        };
        let date = occurrence.date;
        let handle = |state: Option<OccurrenceState>| -> Run {
            Box::new(move |history: &Rc<History>| {
                let id = history.id;
                history.report(
                    history
                        .state
                        .write(|spaces| spaces.handle_occurrence(id, date, state, today())),
                );
            })
        };
        match occurrence.status {
            OccurrenceStatus::Done | OccurrenceStatus::Skipped => {
                add("_Reopen", "reopen", handle(None));
            }
            _ => {
                add("_Done", "done", handle(Some(OccurrenceState::Done)));
                add("_Skip", "skip", handle(Some(OccurrenceState::Skipped)));
            }
        }
        if matches!(occurrence.status, OccurrenceStatus::Current | OccurrenceStatus::Upcoming) {
            let on = occurrence.on;
            add(
                "_Move…",
                "move",
                Box::new(move |history: &Rc<History>| history.pick_day(date, on)),
            );
            if occurrence.moved {
                add(
                    "_Put back on its date",
                    "put-back",
                    Box::new(move |history: &Rc<History>| {
                        let id = history.id;
                        history.report(
                            history.state.write(|spaces| spaces.move_occurrence(id, date, None)),
                        );
                    }),
                );
            }
        }
        match occurrence.note {
            Some(note) => add(
                "_Open note",
                "open-note",
                Box::new(move |history: &Rc<History>| {
                    history.dialog.close();
                    (history.open)(note);
                }),
            ),
            None => add(
                "_Write a note",
                "write-note",
                Box::new(move |history: &Rc<History>| {
                    let id = history.id;
                    match history.state.write(|spaces| spaces.write_occurrence_note(id, date)) {
                        Ok(note) => {
                            history.dialog.close();
                            (history.open)(note);
                        }
                        Err(error) => eprintln!("commonplace: {error}"),
                    }
                }),
            ),
        }
        let button = gtk::MenuButton::builder()
            .icon_name(icon!("more"))
            .tooltip_text("Occurrence menu")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row.add_suffix(&button);
        row.insert_action_group("occurrence", Some(&actions));
        row
    }

    /// Asks for the day to move the occurrence named by `date`, which falls on `on`, to.
    fn pick_day(self: &Rc<Self>, date: Date, on: Date) {
        let calendar = gtk::Calendar::new();
        crate::repeat::show_on_calendar(&calendar, on);
        let dialog = adw::AlertDialog::new(Some("Move to"), None);
        dialog.add_responses(&[("cancel", "_Cancel"), ("move", "_Move")]);
        dialog.set_response_appearance("move", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("move"));
        dialog.set_close_response("cancel");
        dialog.set_extra_child(Some(&calendar));
        dialog.connect_response(
            Some("move"),
            glib::clone!(
                #[weak(rename_to = history)]
                self,
                move |_, _| {
                    let Some(to) = crate::rows::calendar_day(&calendar) else { return };
                    let id = history.id;
                    history.report(
                        history.state.write(|spaces| spaces.move_occurrence(id, date, Some(to))),
                    );
                }
            ),
        );
        dialog.present(Some(&self.dialog));
    }

    fn report(&self, result: Result<(), commonplace_core::Error>) {
        if let Err(error) = result {
            eprintln!("commonplace: {error}");
        }
    }
}
