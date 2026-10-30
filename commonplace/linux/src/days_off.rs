// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! The Days off dialog: holidays and vacations, each a list of schedules that recurring actions
//! and journal schedules can skip or move past.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{DaysOff, DaysOffKind, DaysOffUse, RecurrenceSettings, Rule};
use gtk::{gio, glib};

use crate::repeat::{RepeatChoice, RepeatEditor, RepeatMenu, present_repeat_dialog};
use crate::rows::{connect_day_clicked, day_picker, format_date, today};
use crate::state::State;

struct DaysOffDialog {
    dialog: adw::Dialog,
    hidden: gtk::ListBox,
    hidden_toggle: gtk::ToggleButton,
    kind: Cell<DaysOffKind>,
    shown: gtk::ListBox,
    state: Rc<State>,
}

/// Opens the Days off dialog.
pub fn present(state: &Rc<State>, parent: &impl IsA<gtk::Widget>) {
    let kinds = gtk::Box::builder().css_classes(["linked"]).halign(gtk::Align::Center).build();
    let holidays =
        gtk::ToggleButton::builder().label(DaysOffKind::Holidays.name()).active(true).build();
    let vacations =
        gtk::ToggleButton::builder().label(DaysOffKind::Vacations.name()).group(&holidays).build();
    kinds.append(&holidays);
    kinds.append(&vacations);
    let list = || {
        gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build()
    };
    let shown = list();
    let hidden = list();
    let hidden_toggle =
        gtk::ToggleButton::builder().halign(gtk::Align::Start).css_classes(["flat"]).build();
    let add = gtk::Button::builder().label("Add schedule").halign(gtk::Align::Start).build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .build();
    content.append(&kinds);
    content.append(&shown);
    content.append(&hidden_toggle);
    content.append(&hidden);
    content.append(&add);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(560)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    let dialog =
        adw::Dialog::builder().title("Days off").content_width(460).child(&toolbar).build();
    let this = Rc::new(DaysOffDialog {
        dialog: dialog.clone(),
        hidden,
        hidden_toggle: hidden_toggle.clone(),
        kind: Cell::new(DaysOffKind::Holidays),
        shown,
        state: state.clone(),
    });
    for (button, kind) in [(&holidays, DaysOffKind::Holidays), (&vacations, DaysOffKind::Vacations)]
    {
        button.connect_toggled(glib::clone!(
            #[weak]
            this,
            move |button| {
                if button.is_active() {
                    this.kind.set(kind);
                    this.fill();
                }
            }
        ));
    }
    hidden_toggle.connect_toggled(glib::clone!(
        #[weak]
        this,
        move |_| this.fill()
    ));
    add.connect_clicked(glib::clone!(
        #[weak]
        this,
        move |_| this.edit(None)
    ));
    let weak = Rc::downgrade(&this);
    state.connect_changed_while(move || {
        let open = weak.upgrade();
        if let Some(this) = &open {
            this.fill();
        }
        open.is_some()
    });
    this.fill();
    let keeper = RefCell::new(Some(this));
    dialog.connect_closed(move |_| drop(keeper.take()));
    dialog.present(Some(parent));
}

impl DaysOffDialog {
    fn fill(self: &Rc<Self>) {
        let today = today();
        let kind = self.kind.get();
        let all: Vec<(String, DaysOff)> = self
            .state
            .read(|spaces| spaces.days_off())
            .into_iter()
            .filter(|(_, days)| days.kind == kind)
            .collect();
        let (hidden, shown): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|(_, days)| days.is_hidden(today));
        let names = self.state.spaces();
        let several = names.len() > 1;
        for (list, items) in [(&self.shown, &shown), (&self.hidden, &hidden)] {
            list.remove_all();
            for (space, days) in items {
                let library = several
                    .then(|| names.iter().find(|s| &s.key == space).map(|s| s.name.clone()))
                    .flatten();
                list.append(&self.row(space, days, library));
            }
        }
        if shown.is_empty() {
            let empty = match kind {
                DaysOffKind::Holidays => "No holidays",
                DaysOffKind::Vacations => "No vacations",
            };
            self.shown
                .append(&adw::ActionRow::builder().title(empty).css_classes(["dimmed"]).build());
        }
        let open = self.hidden_toggle.is_active();
        self.hidden_toggle.set_visible(!hidden.is_empty());
        let chevron = if open { "▾" } else { "▸" };
        self.hidden_toggle.set_label(&format!("{chevron} Show hidden ({})", hidden.len()));
        self.hidden.set_visible(open && !hidden.is_empty());
    }

    fn row(
        self: &Rc<Self>,
        space: &str,
        days: &DaysOff,
        library: Option<String>,
    ) -> adw::ActionRow {
        let mut subtitle = days.describe();
        if let Some(library) = library {
            subtitle = format!("{library} · {subtitle}");
        }
        if days.archived.is_some() {
            subtitle = format!("Archived · {subtitle}");
        }
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&days.name))
            .subtitle(glib::markup_escape_text(&subtitle))
            .build();
        let menu = gio::Menu::new();
        menu.append(Some("_Edit"), Some("days.edit"));
        let archive = if days.archived.is_some() { "_Unarchive" } else { "_Archive" };
        menu.append(Some(archive), Some("days.archive"));
        let actions = gio::SimpleActionGroup::new();
        let edit = gio::SimpleAction::new("edit", None);
        let editing = (space.to_owned(), days.clone());
        edit.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.edit(Some(editing.clone()))
        ));
        let archive = gio::SimpleAction::new("archive", None);
        let (space, kind, id, archived) =
            (space.to_owned(), days.kind, days.id.clone(), days.archived.is_none());
        archive.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                let result = this
                    .state
                    .write(|spaces| spaces.archive_days_off(&space, kind, &id, archived, today()));
                if let Err(error) = result {
                    eprintln!("commonplace: {error}");
                }
            }
        ));
        actions.add_action(&edit);
        actions.add_action(&archive);
        let button = gtk::MenuButton::builder()
            .icon_name(icon!("more"))
            .tooltip_text("Schedule menu")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        row.add_suffix(&button);
        row.insert_action_group("days", Some(&actions));
        row
    }

    /// Opens a dialog that adds a schedule of the current kind, or edits `existing` in its library.
    fn edit(self: &Rc<Self>, existing: Option<(String, DaysOff)>) {
        let adding = existing.is_none();
        let spaces = self.state.spaces();
        let space =
            existing.as_ref().map_or_else(|| self.state.capture_space(), |(s, _)| s.clone());
        let days = Rc::new(RefCell::new(
            existing
                .map(|(_, days)| days)
                .unwrap_or_else(|| DaysOff::new(self.kind.get(), "", today())),
        ));
        let name = gtk::Entry::builder()
            .text(days.borrow().name.as_str())
            .placeholder_text("Christmas")
            .activates_default(true)
            .hexpand(true)
            .build();
        let (first_button, first_calendar) = day_picker(&format_date(days.borrow().first), None);
        let same = gtk::Button::builder().label("Same day").css_classes(["flat"]).build();
        let (last_button, last_calendar) = day_picker("", Some(same.upcast_ref()));
        let repeat = RepeatMenu::new(true);
        let library = gtk::DropDown::from_strings(
            &spaces.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        );
        library.set_selected(spaces.iter().position(|s| s.key == space).unwrap_or(0) as u32);
        let grid = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| {
            gtk::Label::builder().label(text).xalign(0.0).css_classes(["dimmed"]).build()
        };
        let mut rows: Vec<(&str, gtk::Widget)> = vec![
            ("Name", name.clone().upcast()),
            ("First day", first_button.clone().upcast()),
            ("Last day", last_button.clone().upcast()),
            ("Repeats", repeat.button.clone().upcast()),
        ];
        if adding && spaces.len() > 1 {
            rows.push(("Library", library.clone().upcast()));
        }
        for (index, (text, widget)) in rows.into_iter().enumerate() {
            grid.attach(&label(text), 0, index as i32, 1, 1);
            grid.attach(&widget, 1, index as i32, 1, 1);
        }
        let refresh = {
            let days = days.clone();
            let first_button = first_button.clone();
            let last_button = last_button.clone();
            let repeat = repeat.clone();
            Rc::new(move || {
                let days = days.borrow();
                first_button.set_label(&format_date(days.first));
                let last = if days.last == days.first {
                    "Same day".to_owned()
                } else {
                    format_date(days.last)
                };
                last_button.set_label(&last);
                let unread = days.unread_rule.is_some();
                repeat.show_with(days.rule.as_ref(), unread, days.first, days.first);
            })
        };
        refresh();
        connect_day_clicked(&first_button, &first_calendar, {
            let (days, refresh) = (days.clone(), refresh.clone());
            move |day| {
                let mut edited = days.borrow_mut();
                let span = edited.last.days_since(edited.first);
                edited.first = day;
                edited.last = day.add_days(span);
                drop(edited);
                refresh();
            }
        });
        connect_day_clicked(&last_button, &last_calendar, {
            let (days, refresh) = (days.clone(), refresh.clone());
            move |day| {
                let mut edited = days.borrow_mut();
                edited.last = day.max(edited.first);
                drop(edited);
                refresh();
            }
        });
        same.connect_clicked({
            let (days, refresh, last_button) = (days.clone(), refresh.clone(), last_button.clone());
            move |_| {
                {
                    let mut edited = days.borrow_mut();
                    edited.last = edited.first;
                }
                last_button.popdown();
                refresh();
            }
        });
        let state = self.state.clone();
        repeat.connect_chosen({
            let (days, refresh, grid) = (days.clone(), refresh.clone(), grid.clone());
            move |choice| match choice {
                RepeatChoice::None => {
                    let mut days = days.borrow_mut();
                    days.rule = None;
                    days.unread_rule = None;
                    drop(days);
                    refresh();
                }
                RepeatChoice::Rule(rule) => {
                    days.borrow_mut().rule = Some(rule);
                    refresh();
                }
                RepeatChoice::Custom => {
                    let first = days.borrow().first;
                    let rule = days.borrow().rule.clone();
                    let preview = preview(&state, None, DaysOffUse::default());
                    let editor = RepeatEditor::new(rule.as_ref(), first, false, preview);
                    let (days, refresh) = (days.clone(), refresh.clone());
                    present_repeat_dialog(&grid, editor, move |values| {
                        days.borrow_mut().rule = Some(values.rule);
                        refresh();
                    });
                }
            }
        });
        let dialog = adw::AlertDialog::new(
            Some(if adding { "Add schedule" } else { "Edit schedule" }),
            None,
        );
        dialog.add_responses(&[
            ("cancel", "_Cancel"),
            ("save", if adding { "_Add" } else { "_Save" }),
        ]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        dialog.set_extra_child(Some(&grid));
        dialog.connect_response(
            Some("save"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    let mut saved = days.borrow().clone();
                    saved.name = name.text().trim().to_owned();
                    if saved.name.is_empty() {
                        saved.name = format_date(saved.first);
                    }
                    let space = if adding {
                        spaces
                            .get(library.selected() as usize)
                            .map_or(space.clone(), |s| s.key.clone())
                    } else {
                        space.clone()
                    };
                    if let Err(error) = this.state.write(|s| s.set_days_off(&space, &saved)) {
                        eprintln!("commonplace: {error}");
                    }
                }
            ),
        );
        dialog.present(Some(&self.dialog));
    }
}

/// A preview of a rule's next dates, as `days_off` moves them around the days off of `id`'s
/// library, for the repeat editor.
pub fn preview(
    state: &Rc<State>,
    id: Option<commonplace_core::NoteId>,
    days_off: DaysOffUse,
) -> crate::repeat::Preview {
    let state = Rc::downgrade(state);
    Box::new(move |rule: &Rule, start| {
        let Some(state) = state.upgrade() else { return Vec::new() };
        let mut settings = RecurrenceSettings::new(rule.clone(), start);
        settings.days_off = days_off;
        state.read(|spaces| spaces.preview_dates(id, None, &settings, today().max(start), 3))
    })
}
