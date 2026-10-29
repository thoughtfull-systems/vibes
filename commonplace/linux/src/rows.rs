// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{
    Action, ActionChange, ActionDates, ActionEntry, ActionList, ActionState, ActionStatus, Date,
    Dated, Note, NoteId, Spaces, Style, expand_links, inline_pieces,
};
use gtk::glib;

use crate::state::State;

type OpenHandler = Rc<dyn Fn(NoteId)>;

/// A titled group of notes in a `NoteList`.
pub struct Section {
    pub entries: Vec<ActionEntry>,
    /// Whether each open action's row has a button that queues it for planning.
    pub plan: bool,
    /// A time to show in each row.
    pub stamp: Option<Stamp>,
    /// Whether each action's row shows its star, as a button that takes the star away.
    pub star: bool,
    pub title: &'static str,
}

/// Which of a note's times a row shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stamp {
    Archived,
    Created,
    Modified,
    Trashed,
}

/// Notes in sections, each a boxed list under its title, or a status page when there are none.
pub struct NoteList {
    content: gtk::Box,
    empty: adw::StatusPage,
    on_open: Rc<RefCell<Option<OpenHandler>>>,
    state: Rc<State>,
    pub widget: gtk::Stack,
}

impl NoteList {
    pub fn new(state: Rc<State>) -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(800).child(&content).build())
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let empty = adw::StatusPage::new();
        let widget = gtk::Stack::new();
        widget.add_named(&scrolled, Some("list"));
        widget.add_named(&empty, Some("empty"));
        Self { content, empty, on_open: Rc::default(), state, widget }
    }

    /// Calls `f` when the user activates a note's row.
    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    /// Sets what the status page says when there are no notes to show.
    pub fn set_empty(&self, icon: &str, title: &str, description: &str) {
        self.empty.set_icon_name(Some(icon));
        self.empty.set_title(title);
        self.empty.set_description(Some(description));
    }

    /// Shows `sections`, leaving out the empty ones.
    pub fn show(&self, sections: Vec<Section>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let sections: Vec<Section> =
            sections.into_iter().filter(|section| !section.entries.is_empty()).collect();
        self.widget.set_visible_child_name(if sections.is_empty() { "empty" } else { "list" });
        for section in sections {
            if !section.title.is_empty() {
                self.content.append(
                    &gtk::Label::builder()
                        .label(section.title)
                        .xalign(0.0)
                        .css_classes(["heading"])
                        .margin_top(12)
                        .margin_bottom(6)
                        .build(),
                );
            }
            let list = gtk::ListBox::builder()
                .css_classes(["boxed-list"])
                .selection_mode(gtk::SelectionMode::None)
                .build();
            for entry in &section.entries {
                let star = section.star.then(|| {
                    star_button(&self.state, entry.note.id, entry.note.starred.is_some()).upcast()
                });
                let row = stamped_note_row(&self.state, entry, true, section.stamp, star, None);
                row.set_margin_start(12);
                row.set_margin_end(12);
                let buttons =
                    gtk::Box::builder().spacing(6).hexpand(true).halign(gtk::Align::End).build();
                let open = entry.note.action.as_ref().filter(|action| action.is_open());
                if let Some(action) = open.filter(|_| section.plan) {
                    buttons.append(&plan_button(
                        &self.state,
                        entry.note.id,
                        action.planning.is_some(),
                    ));
                }
                if buttons.first_child().is_some() {
                    row.append(&buttons);
                }
                list.append(&row);
            }
            let ids: Vec<NoteId> = section.entries.iter().map(|entry| entry.note.id).collect();
            let on_open = self.on_open.clone();
            list.connect_row_activated(move |_, row| {
                let handler = on_open.borrow().clone();
                if let (Some(&id), Some(handler)) = (ids.get(row.index() as usize), handler) {
                    handler(id);
                }
            });
            self.content.append(&list);
        }
    }
}

/// A toggle that adds an open action to the planning queue, or takes it out.
fn plan_button(state: &Rc<State>, id: NoteId, queued: bool) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .label("Plan")
        .active(queued)
        .valign(gtk::Align::Center)
        .tooltip_text(plan_tooltip(queued))
        .css_classes(["flat"])
        .build();
    button.connect_toggled(glib::clone!(
        #[strong]
        state,
        move |button| {
            let queued = button.is_active();
            if let Err(error) =
                state.write(|library| library.change_action(id, ActionChange::Planning(queued)))
            {
                eprintln!("commonplace: {error}");
            }
        }
    ));
    button
}

fn plan_tooltip(queued: bool) -> &'static str {
    if queued { "Take out of the planning queue" } else { "Add to the planning queue" }
}

/// A toggle that stars a note, or takes its star away. Starred notes lead their tabs.
pub fn star_button(state: &Rc<State>, id: NoteId, starred: bool) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .icon_name("starred-symbolic")
        .active(starred)
        .valign(gtk::Align::Center)
        .tooltip_text(star_tooltip(starred))
        .css_classes(["star"])
        .build();
    button.connect_toggled(glib::clone!(
        #[strong]
        state,
        move |button| {
            let starred = button.is_active();
            button.set_tooltip_text(Some(star_tooltip(starred)));
            if let Err(error) = state.write(|library| library.set_starred(id, starred)) {
                eprintln!("commonplace: {error}");
            }
        }
    ));
    button
}

fn star_tooltip(starred: bool) -> &'static str {
    if starred { "Remove the star" } else { "Star" }
}

/// A row for a note in a list. An action's row has a checkbox that closes or reopens it, and a line
/// naming the actions it belongs to if `show_path` is set.
pub fn note_row(state: &Rc<State>, entry: &ActionEntry, show_path: bool) -> gtk::Box {
    stamped_note_row(state, entry, show_path, None, None, None)
}

/// The grid columns of a step's row, which `step_row` lays out.
pub const STEP_CHECK_COLUMN: i32 = 1;
pub const STEP_TITLE_COLUMN: i32 = 2;
pub const STEP_MENU_COLUMN: i32 = 3;

/// A row for a step in an action's steps list, like `note_row`, with `handle` before the
/// indentation, `edit` right after the title, and `menu` at the end of the title's line. The row's
/// grid holds them on its first row, in the columns the constants above name, with the title and
/// `edit` in a box together.
pub fn step_row(
    state: &Rc<State>,
    entry: &ActionEntry,
    handle: &gtk::Widget,
    edit: &gtk::Widget,
    menu: &gtk::Widget,
) -> gtk::Box {
    let row = stamped_note_row(state, entry, false, None, Some(menu.clone()), Some(edit.clone()));
    let grid = row.first_child().and_downcast::<gtk::Grid>().expect("a note row holds a grid");
    grid.insert_column(0);
    grid.attach(handle, 0, 0, 1, 1);
    if let Some(check) = grid.child_at(STEP_CHECK_COLUMN, 0) {
        check.set_margin_start(row.margin_start());
    }
    row.set_margin_start(0);
    row
}

/// A row for a note, like `note_row`, that also shows the time `stamp` names, `title_end` at the
/// end of the title's line, and `title_after` right after the title.
fn stamped_note_row(
    state: &Rc<State>,
    entry: &ActionEntry,
    show_path: bool,
    stamp: Option<Stamp>,
    title_end: Option<gtk::Widget>,
    title_after: Option<gtk::Widget>,
) -> gtk::Box {
    let note = &entry.note;
    let title = gtk::Label::builder()
        .label(state.read(|spaces| title_markup(spaces, note)))
        .use_markup(true)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let stamp_text = stamp.map(|stamp| match stamp {
        Stamp::Archived => {
            format!("Archived {}", format_time(note.archived.unwrap_or(note.modified)))
        }
        Stamp::Created => format!("Created {}", format_time(note.created)),
        Stamp::Modified => format!("Modified {}", format_time(note.modified)),
        Stamp::Trashed => {
            format!("Trashed {}", format_time(note.trashed.unwrap_or(note.modified)))
        }
    });
    let summary = match (&note.action, stamp_text) {
        (Some(action), stamp) => {
            let summary = action_summary(state, entry, action);
            [summary, stamp.unwrap_or_default()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
        }
        (None, Some(stamp)) => stamp,
        (None, None) => format_time(note.modified),
    };
    // An action's summary already says whether it's archived.
    let summary =
        if note.action.is_none() && note.archived.is_some() && stamp != Some(Stamp::Archived) {
            format!("Archived · {summary}")
        } else {
            summary
        };
    let journal =
        note.entry.as_ref().and_then(|fields| state.read(|spaces| spaces.note(fields.journal)));
    let summary = match &journal {
        Some(journal) => {
            let name = journal.label().unwrap_or_else(|| "Untitled journal".to_owned());
            format!("{name} · {summary}")
        }
        None => summary,
    };
    let path = if show_path && !entry.parents.is_empty() {
        let labels: Vec<String> =
            parent_labels(state, &entry.parents).into_iter().map(|(_, label)| label).collect();
        labels.join(" › ")
    } else {
        String::new()
    };
    let row = gtk::Box::builder()
        .spacing(8)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(16 * entry.depth as i32)
        .build();
    // A checkbox has no baseline, so a grid row centers it on the title's line instead.
    let text = gtk::Grid::builder().column_spacing(8).row_spacing(2).build();
    let column = 1;
    if let Some(action) = &note.action {
        text.attach(&action_check(state, note.id, action), 0, 0, 1, 1);
        if action.project {
            title.add_css_class("project-title");
        }
        if action.state == ActionState::Done {
            row.add_css_class("dimmed");
        }
    } else {
        text.attach(&kind_icon(note, journal.as_ref()), 0, 0, 1, 1);
    }
    match title_after {
        Some(after) => {
            // A box keeps `after` next to the title. In the grid, it would follow the widest line.
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 1);
            line.append(&title);
            after.set_valign(gtk::Align::Center);
            line.append(&after);
            text.attach(&line, column, 0, 1, 1);
        }
        None => text.attach(&title, column, 0, 1, 1),
    }
    if let Some(widget) = title_end {
        text.set_hexpand(true);
        widget.set_hexpand(true);
        widget.set_halign(gtk::Align::End);
        widget.set_valign(gtk::Align::Center);
        text.attach(&widget, column + 1, 0, 1, 1);
    }
    for (line, index) in [summary, path].into_iter().filter(|line| !line.is_empty()).zip(1..) {
        let label = gtk::Label::builder()
            .label(line)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["caption", "dimmed"])
            .build();
        text.attach(&label, column, index, 1, 1);
    }
    match entry.dates.status {
        ActionStatus::DueSoon => {
            title.add_css_class("heading");
            title.add_css_class("warning");
        }
        ActionStatus::Normal => {}
        ActionStatus::Overdue => {
            title.add_css_class("heading");
            title.add_css_class("error");
        }
        ActionStatus::Upcoming => row.add_css_class("dimmed"),
    }
    row.append(&text);
    row
}

/// The app's own icons for memos and journals, which the tabs and the note rows share.
pub const MEMO_ICON: &str = "systems.thoughtfull.Commonplace-memo-symbolic";
pub const JOURNAL_ICON: &str = "systems.thoughtfull.Commonplace-journal-symbolic";

/// The icon that tells a memo, a journal, and an entry apart: the Memos and Journals tabs' icons,
/// and a dot in an entry's journal's color.
fn kind_icon(note: &Note, journal: Option<&Note>) -> gtk::Widget {
    if note.entry.is_some() {
        let color = journal
            .and_then(|journal| journal.journal.as_ref())
            .map_or("blue", |fields| fields.color.as_str());
        return crate::journals_view::dot(crate::journals_view::journal_color(color)).upcast();
    }
    let icon = if note.journal.is_some() { JOURNAL_ICON } else { MEMO_ICON };
    gtk::Image::builder()
        .icon_name(icon)
        .valign(gtk::Align::Center)
        .css_classes(["dimmed"])
        .build()
        .upcast()
}

/// A checkbox that marks an action done, or reopens it if it's closed.
fn action_check(state: &Rc<State>, id: NoteId, action: &Action) -> gtk::CheckButton {
    let check = gtk::CheckButton::builder()
        .active(action.state == ActionState::Done)
        .valign(gtk::Align::Center)
        .tooltip_text("Done")
        .css_classes(["row-check"])
        .build();
    let open = action.is_open();
    check.connect_toggled(glib::clone!(
        #[strong]
        state,
        move |_| {
            let new_state = if open { ActionState::Done } else { ActionState::Open };
            if let Err(error) =
                state.write(|library| library.change_action(id, ActionChange::State(new_state)))
            {
                eprintln!("commonplace: {error}");
            }
        }
    ));
    check
}

/// A line that sums up an action's list, dates, and project state. The action's checkbox shows
/// whether it's done or archived.
fn action_summary(state: &State, entry: &ActionEntry, action: &Action) -> String {
    let mut parts = Vec::new();
    if entry.note.archived.is_some() {
        parts.push("Archived".to_owned());
    }
    if action.project {
        parts.push(if entry.stalled { "Stalled project" } else { "Project" }.to_owned());
    }
    match action.list {
        ActionList::Active => {}
        ActionList::Inbox => parts.push("Inbox".to_owned()),
        ActionList::Maybe => parts.push("Maybe".to_owned()),
        ActionList::Someday => parts.push("Someday".to_owned()),
        ActionList::Waiting => parts.push(match &action.waiting_for {
            Some(who) => format!("Waiting for {who}"),
            None => "Waiting".to_owned(),
        }),
    }
    if let Some(due) = entry.dates.due {
        let overdue = action.is_open() && due.date < today();
        let word = if overdue { "Overdue since" } else { "Due" };
        parts.push(describe_date(state, word, due, entry.note.id));
    }
    if let Some(start) = entry.dates.start.filter(|start| start.date > today()) {
        parts.push(describe_date(state, "Starts", start, entry.note.id));
    }
    parts.join(" · ")
}

/// Describes a date that applies to the action `id`, naming the project it comes from if it isn't
/// the action's own.
pub fn describe_date(state: &State, word: &str, dated: Dated, id: NoteId) -> String {
    let date = format!("{word} {}", format_date(dated.date));
    if dated.from == id {
        return date;
    }
    match state.read(|library| library.note(dated.from)) {
        Some(project) => {
            format!("{date} from {}", state.read(|spaces| display_label(spaces, &project)))
        }
        None => date,
    }
}

/// An entry for a note outside an action view, with its action status if it's an action.
pub fn entry(
    note: Note,
    dates: &HashMap<NoteId, ActionDates>,
    paths: &HashMap<NoteId, Vec<NoteId>>,
) -> ActionEntry {
    let dates = dates.get(&note.id).copied().unwrap_or_default();
    let parents = paths.get(&note.id).cloned().unwrap_or_default();
    ActionEntry { dates, depth: 0, note, parents, stalled: false }
}

/// The labels of the actions in `parents`, with their identifiers.
pub fn parent_labels(state: &State, parents: &[NoteId]) -> Vec<(NoteId, String)> {
    state.read(|library| {
        parents
            .iter()
            .filter_map(|&id| library.note(id).map(|note| (id, display_label(library, &note))))
            .collect()
    })
}

/// A button whose popover holds a calendar, with `extra` below it.
pub fn day_picker(label: &str, extra: Option<&gtk::Widget>) -> (gtk::MenuButton, gtk::Calendar) {
    let calendar = gtk::Calendar::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.append(&calendar);
    if let Some(extra) = extra {
        content.append(extra);
    }
    let popover = gtk::Popover::builder().child(&content).build();
    let button =
        gtk::MenuButton::builder().label(label).use_underline(true).popover(&popover).build();
    (button, calendar)
}

/// Closes the popover of `button` and calls `on_pick` with the day when the user clicks a day in
/// `calendar`, even the day already selected.
pub fn connect_day_clicked(
    button: &gtk::MenuButton,
    calendar: &gtk::Calendar,
    on_pick: impl Fn(Date) + 'static,
) {
    // GtkCalendar selects a day when the button goes down, so the popover closes on the release
    // instead, leaving the calendar to finish the click.
    let click = gtk::GestureClick::new();
    click.connect_released(glib::clone!(
        #[weak]
        button,
        #[weak]
        calendar,
        move |_, _, x, y| {
            let on_day = calendar
                .pick(x, y, gtk::PickFlags::DEFAULT)
                .is_some_and(|picked| picked.has_css_class("day-number"));
            if let Some(day) = calendar_day(&calendar).filter(|_| on_day) {
                button.popdown();
                on_pick(day);
            }
        }
    ));
    calendar.add_controller(click);
}

/// The day the calendar has selected.
pub fn calendar_day(calendar: &gtk::Calendar) -> Option<Date> {
    let day = calendar.date();
    Date::new(day.year() as u16, day.month() as u8, day.day_of_month() as u8)
}

pub fn today() -> Date {
    let now = glib::DateTime::now_local().expect("the local time is known");
    Date::new(now.year() as u16, now.month() as u8, now.day_of_month() as u8)
        .expect("GLib's dates are valid")
}

pub fn format_date(date: Date) -> String {
    let time = glib::DateTime::from_local(
        i32::from(date.year()),
        i32::from(date.month()),
        i32::from(date.day()),
        0,
        0,
        0.0,
    );
    let format = if today().year() == date.year() { "%a %b %-d" } else { "%b %-d, %Y" };
    time.and_then(|time| time.format(format)).map(String::from).unwrap_or_else(|_| date.to_string())
}

/// The note's label as Pango markup that renders its inline Markdown, or a stand-in naming its kind
/// if its body has no text.
fn title_markup(spaces: &Spaces, note: &Note) -> String {
    let Some(label) = shown_label(spaces, note) else {
        return glib::markup_escape_text(&display_label(spaces, note)).to_string();
    };
    let markup = markdown_markup(&label);
    if markup.trim().is_empty() {
        glib::markup_escape_text(&display_label(spaces, note)).to_string()
    } else {
        markup
    }
}

pub fn markdown_markup(body: &str) -> String {
    inline_pieces(body)
        .into_iter()
        .map(|piece| {
            let mut markup = glib::markup_escape_text(&piece.text).to_string();
            for style in piece.styles {
                markup = match style {
                    Style::Code => format!("<tt>{markup}</tt>"),
                    Style::Emphasis => format!("<i>{markup}</i>"),
                    Style::Highlight => {
                        format!("<span background=\"#f5d42e\" bgalpha=\"45%\">{markup}</span>")
                    }
                    Style::Strikethrough => format!("<s>{markup}</s>"),
                    Style::Strong => format!("<b>{markup}</b>"),
                    _ => markup,
                };
            }
            markup
        })
        .collect()
}

/// The note's label as plain text, without its Markdown, or a stand-in naming its kind if its body
/// has no text.
pub fn display_label(spaces: &Spaces, note: &Note) -> String {
    if let Some(label) = shown_label(spaces, note) {
        let text: String = inline_pieces(&label).into_iter().map(|piece| piece.text).collect();
        if !text.trim().is_empty() {
            return text;
        }
    }
    note.label().unwrap_or_else(|| {
        match &note.action {
            Some(action) if action.project => "Empty project",
            Some(_) => "Empty task",
            None if note.entry.is_some() => "Empty entry",
            None => "Empty memo",
        }
        .to_owned()
    })
}

/// The note's label, with each link in it that has no label showing its note's title.
fn shown_label(spaces: &Spaces, note: &Note) -> Option<String> {
    note.label().map(|label| expand_links(&label, |id| spaces.link_title(id)))
}

pub fn format_time(milliseconds: i64) -> String {
    let (Ok(time), Ok(now)) = (
        glib::DateTime::from_unix_local(milliseconds.div_euclid(1000)),
        glib::DateTime::now_local(),
    ) else {
        return String::new();
    };
    let format = if time.ymd() == now.ymd() {
        "%H:%M"
    } else if time.year() == now.year() {
        "%b %-d"
    } else {
        "%Y-%m-%d"
    };
    time.format(format).map(String::from).unwrap_or_default()
}
