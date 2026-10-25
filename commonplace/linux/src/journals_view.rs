// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Answer, Date, Journal, Note, NoteId, Question, QuestionKind, format_time};
use gtk::{cairo, gio, glib};

use crate::prompts::local_millis;
use crate::rows::{format_date, today};
use crate::state::State;

/// The journals this device hides from the timeline, by identifier, separated by commas.
const HIDDEN: &str = "linux.journals.hidden";
/// The index of the range the timeline covers, in `RANGES`.
const RANGE: &str = "linux.journals.range";

/// The ranges of days the timeline offers, with how many days back each reaches.
const RANGES: [(&str, Option<i32>); 6] = [
    ("Today", Some(1)),
    ("Last 7 Days", Some(7)),
    ("Last 30 Days", Some(30)),
    ("Last 90 Days", Some(90)),
    ("Last Year", Some(365)),
    ("All Time", None),
];

/// Each journal color's light and dark shades, from the dataviz skill's validated categorical
/// palette where it has the hue. Charts draw in their journal's color alone.
pub fn journal_color(name: &str) -> (&'static str, &'static str) {
    match name {
        "green" => ("#008300", "#008300"),
        "yellow" => ("#eda100", "#c98500"),
        "orange" => ("#eb6834", "#d95926"),
        "red" => ("#e34948", "#e66767"),
        "purple" => ("#6250d6", "#9085e9"),
        "brown" => ("#8f6a45", "#b08a63"),
        "gray" => ("#7a7a74", "#9a9a93"),
        _ => ("#2a78d6", "#3987e5"),
    }
}

fn shade<'a>(colors: (&'a str, &'a str)) -> &'a str {
    if adw::StyleManager::default().is_dark() { colors.1 } else { colors.0 }
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let channel = |range: std::ops::Range<usize>| {
        f64::from(u8::from_str_radix(&hex[range], 16).unwrap_or(0)) / 255.0
    };
    (channel(1..3), channel(3..5), channel(5..7))
}

type OpenHandler = Rc<dyn Fn(NoteId)>;

/// One entry as the timeline shows it.
struct Shown {
    note: Note,
    at: i64,
    journal: Note,
}

/// The journals tab: every entry in the journals that show, grouped by day, newest first, with a
/// button per journal to show or hide it, and charts while one journal shows alone.
pub struct JournalsView {
    pub widget: gtk::Box,
    charts: gtk::Box,
    chips: gtk::Box,
    /// Says how to see charts while more than one journal shows.
    hint: gtk::Label,
    /// The journals menu, which starts a journal or opens one to edit.
    menu: gio::Menu,
    empty: adw::StatusPage,
    list: gtk::ListBox,
    /// The entry each row of the list opens, or `None` for a day's heading.
    row_ids: RefCell<Vec<Option<NoteId>>>,
    on_open: RefCell<Option<OpenHandler>>,
    on_new: RefCell<Option<Rc<dyn Fn()>>>,
    query: RefCell<String>,
    range: gtk::DropDown,
    scrolled: gtk::ScrolledWindow,
    state: Rc<State>,
}

impl JournalsView {
    pub fn new(state: Rc<State>) -> Rc<Self> {
        let chips = gtk::Box::builder().spacing(6).build();
        let range = gtk::DropDown::from_strings(&RANGES.map(|(name, _)| name));
        let saved = state.device().setting(RANGE).and_then(|index| u32::try_from(index).ok());
        range.set_selected(saved.filter(|&index| (index as usize) < RANGES.len()).unwrap_or(1));
        let menu = gio::Menu::new();
        let menu_button = gtk::MenuButton::builder()
            .label("Journals")
            .tooltip_text("Start a journal, or edit one")
            .menu_model(&menu)
            .build();
        let show_label = gtk::Label::builder().label("Show").css_classes(["dimmed"]).build();
        let controls =
            gtk::Box::builder().spacing(6).halign(gtk::Align::Center).hexpand(true).build();
        let chips_scroller = gtk::ScrolledWindow::builder()
            .child(&chips)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .build();
        controls.append(&menu_button);
        controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        controls.append(&show_label);
        controls.append(&chips_scroller);
        controls.append(&range);
        let charts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .visible(false)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let hint = gtk::Label::builder()
            .label("Show one journal alone to chart its answers")
            .xalign(0.0)
            .css_classes(["caption", "dimmed"])
            .visible(false)
            .build();
        content.append(&hint);
        content.append(&charts);
        content.append(&list);
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(800).child(&content).build())
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name(crate::rows::JOURNAL_ICON)
            .vexpand(true)
            .visible(false)
            .build();
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bar = gtk::Box::builder().css_classes(["toolbar"]).build();
        bar.append(&controls);
        widget.append(&bar);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&scrolled);
        widget.append(&empty);
        let this = Rc::new(Self {
            widget,
            charts,
            chips,
            empty,
            hint,
            menu,
            list,
            row_ids: RefCell::default(),
            on_open: RefCell::default(),
            on_new: RefCell::default(),
            query: RefCell::default(),
            range,
            scrolled,
            state,
        });
        this.range.connect_selected_notify(glib::clone!(
            #[weak]
            this,
            move |range| {
                this.state.device().set_setting(RANGE, i64::from(range.selected()));
                this.show();
            }
        ));
        let actions = gio::SimpleActionGroup::new();
        let new = gio::SimpleAction::new("new", None);
        new.connect_activate(glib::clone!(
            #[weak]
            this,
            move |_, _| {
                let handler = this.on_new.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            }
        ));
        actions.add_action(&new);
        let edit = gio::SimpleAction::new("edit", Some(glib::VariantTy::STRING));
        edit.connect_activate(glib::clone!(
            #[weak]
            this,
            move |_, parameter| {
                let id = parameter
                    .and_then(|value| value.get::<String>())
                    .and_then(|id| id.parse().ok());
                let handler = this.on_open.borrow().clone();
                if let (Some(id), Some(handler)) = (id, handler) {
                    handler(id);
                }
            }
        ));
        actions.add_action(&edit);
        this.widget.insert_action_group("journals", Some(&actions));
        this.list.connect_row_activated(glib::clone!(
            #[weak]
            this,
            move |_, row| {
                let id = this.row_ids.borrow().get(row.index() as usize).copied().flatten();
                let handler = this.on_open.borrow().clone();
                if let (Some(id), Some(handler)) = (id, handler) {
                    handler(id);
                }
            }
        ));
        adw::StyleManager::default().connect_dark_notify(glib::clone!(
            #[weak]
            this,
            move |_| this.show()
        ));
        this
    }

    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        self.on_open.replace(Some(Rc::new(f)));
    }

    /// Calls `f` when the owner asks for a new journal.
    pub fn connect_new(&self, f: impl Fn() + 'static) {
        self.on_new.replace(Some(Rc::new(f)));
    }

    /// Shows the entries that match `query`.
    pub fn refresh(self: &Rc<Self>, query: &str) {
        self.query.replace(query.to_owned());
        self.show();
    }

    fn hidden(&self) -> HashSet<String> {
        let text = self.state.device().text_setting(HIDDEN).unwrap_or_default();
        text.split(',').filter(|id| !id.is_empty()).map(str::to_owned).collect()
    }

    fn set_hidden(&self, journal: NoteId, hidden: bool) {
        let mut set = self.hidden();
        if hidden {
            set.insert(journal.to_string());
        } else {
            set.remove(&journal.to_string());
        }
        let mut list: Vec<String> = set.into_iter().collect();
        list.sort();
        self.state.device().set_text_setting(HIDDEN, &list.join(","));
    }

    /// The journals that show: those in the libraries that show, less those hidden here.
    fn shown_journals(&self) -> Vec<Note> {
        let hidden = self.hidden();
        let journals = self.state.read(|spaces| spaces.journals());
        journals.into_iter().filter(|journal| !hidden.contains(&journal.id.to_string())).collect()
    }

    fn show(self: &Rc<Self>) {
        let journals = self.state.read(|spaces| spaces.journals());
        self.fill_chips(&journals);
        let shown = self.shown_journals();
        let start = RANGES[self.range.selected() as usize % RANGES.len()]
            .1
            .map(|days| today().add_days(1 - days));
        let since = start.map(|start| local_millis(start, 0));
        let query = self.query.borrow().clone();
        let in_trash = self.state.read(|spaces| spaces.notes_in_trash());
        let mut entries: Vec<Shown> = self
            .state
            .read(|spaces| spaces.notes())
            .into_iter()
            .filter(|note| !in_trash.contains(&note.id) && note.matches(&query))
            .filter_map(|note| {
                let entry = note.entry.as_ref()?;
                let journal = shown.iter().find(|journal| journal.id == entry.journal)?.clone();
                let at = entry.at;
                since.is_none_or(|since| at >= since).then_some(Shown { note, at, journal })
            })
            .collect();
        entries.sort_by_key(|shown| std::cmp::Reverse(shown.at));
        self.fill_charts(&shown, &entries, start);
        let chartable = shown
            .iter()
            .flat_map(|journal| journal.journal.iter())
            .any(|fields| fields.current_questions().any(|question| charts(&question.kind)));
        self.hint.set_visible(shown.len() > 1 && chartable && !entries.is_empty());
        self.fill_list(&entries);
        let (icon, title, description) = if journals.is_empty() {
            (crate::rows::JOURNAL_ICON, "No Journals", "Start one with New Journal")
        } else if !query.trim().is_empty() {
            ("system-search-symbolic", "No Entries Found", "Try other words or a longer range")
        } else {
            (crate::rows::JOURNAL_ICON, "No Entries", "Write one with commonplace capture journal")
        };
        let blank = entries.is_empty() && !self.charts.is_visible();
        self.empty.set_icon_name(Some(icon));
        self.empty.set_title(title);
        self.empty.set_description(Some(description));
        self.empty.set_visible(blank);
        self.scrolled.set_visible(!blank);
    }

    fn fill_chips(self: &Rc<Self>, journals: &[Note]) {
        self.menu.remove_all();
        self.menu.append(Some("_New Journal…"), Some("journals.new"));
        let edits = gio::Menu::new();
        for journal in journals {
            let name = journal.label().unwrap_or_else(|| "Untitled journal".to_owned());
            let item = gio::MenuItem::new(Some(&format!("Edit {name}")), None);
            item.set_action_and_target_value(
                Some("journals.edit"),
                Some(&journal.id.to_string().to_variant()),
            );
            edits.append_item(&item);
        }
        self.menu.append_section(None, &edits);
        while let Some(child) = self.chips.first_child() {
            self.chips.remove(&child);
        }
        let hidden = self.hidden();
        for journal in journals {
            let name = journal.label().unwrap_or_else(|| "Untitled journal".to_owned());
            let color = journal.journal.as_ref().map_or("blue", |fields| fields.color.as_str());
            let content = gtk::Box::builder().spacing(6).build();
            content.append(&dot(journal_color(color)));
            content.append(&gtk::Label::new(Some(&name)));
            let button = gtk::ToggleButton::builder()
                .child(&content)
                .active(!hidden.contains(&journal.id.to_string()))
                .tooltip_text(format!("Show or hide {name}"))
                .build();
            let id = journal.id;
            button.connect_toggled(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |button| {
                    this.set_hidden(id, !button.is_active());
                    this.show();
                }
            ));
            self.chips.append(&button);
        }
    }

    fn fill_list(&self, entries: &[Shown]) {
        self.list.remove_all();
        let mut ids = Vec::new();
        let mut day: Option<(i32, i32, i32)> = None;
        for shown in entries {
            let Ok(time) = glib::DateTime::from_unix_local(shown.at.div_euclid(1000)) else {
                continue;
            };
            let ymd = time.ymd();
            if day != Some(ymd) {
                day = Some(ymd);
                let date = Date::new(ymd.0 as u16, ymd.1 as u8, ymd.2 as u8);
                let label = gtk::Label::builder()
                    .label(date.map(format_date).unwrap_or_default())
                    .xalign(0.0)
                    .css_classes(["heading"])
                    .margin_top(12)
                    .margin_bottom(6)
                    .margin_start(12)
                    .build();
                let row = gtk::ListBoxRow::builder().child(&label).activatable(false).build();
                self.list.append(&row);
                ids.push(None);
            }
            let row = gtk::ListBoxRow::builder().child(&entry_row(shown, &time)).build();
            self.list.append(&row);
            ids.push(Some(shown.note.id));
        }
        self.list.set_visible(!entries.is_empty());
        self.row_ids.replace(ids);
    }

    /// Charts each of the one journal's questions that charts, over the days from `start`, or from
    /// its first entry, until today.
    fn fill_charts(&self, journals: &[Note], entries: &[Shown], start: Option<Date>) {
        while let Some(child) = self.charts.first_child() {
            self.charts.remove(&child);
        }
        let [journal] = journals else {
            self.charts.set_visible(false);
            return;
        };
        let Some(fields) = &journal.journal else {
            self.charts.set_visible(false);
            return;
        };
        let color = shade(journal_color(&fields.color)).to_owned();
        let mut any = false;
        for question in fields.current_questions() {
            let answers: Vec<(i64, &Answer)> = entries
                .iter()
                .filter_map(|shown| {
                    let answer = shown.note.entry.as_ref()?.answers.get(&question.id)?;
                    Some((shown.at, answer))
                })
                .collect();
            if answers.is_empty() {
                continue;
            }
            let chart = match &question.kind {
                kind if kind.is_numeric() => {
                    let points: Vec<(i64, f64)> = answers
                        .iter()
                        .filter_map(|(at, answer)| Some((*at, answer.number()?)))
                        .collect();
                    line_chart(question, points, &color).upcast::<gtk::Widget>()
                }
                QuestionKind::Choice { .. } | QuestionKind::YesNo => {
                    let first = answers.iter().filter_map(|(at, _)| local_date(*at)).min();
                    let Some(first) = start.or(first) else { continue };
                    day_grid(question, &answers, first, today(), &color).upcast()
                }
                _ => continue,
            };
            let title = gtk::Label::builder()
                .label(question.prompt.trim())
                .xalign(0.0)
                .css_classes(["heading"])
                .build();
            let card = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .css_classes(["card"])
                .build();
            title.set_margin_top(12);
            title.set_margin_start(12);
            card.append(&title);
            card.append(&chart);
            self.charts.append(&card);
            any = true;
        }
        self.charts.set_visible(any);
    }
}

/// A row for one entry: its time and journal, the start of its text, and its answers.
fn entry_row(shown: &Shown, time: &glib::DateTime) -> gtk::Box {
    let fields = shown.journal.journal.clone().unwrap_or_default();
    let name = shown.journal.label().unwrap_or_else(|| "Untitled journal".to_owned());
    let minute = (time.hour() * 60 + time.minute()) as u16;
    let header = gtk::Box::builder().spacing(6).build();
    header.append(&dot(journal_color(&fields.color)));
    header.append(
        &gtk::Label::builder()
            .label(format!("{} · {name}", format_time(minute)))
            .xalign(0.0)
            .css_classes(["caption", "dimmed"])
            .build(),
    );
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&header);
    let text = preview(&shown.note.body);
    if !text.is_empty() {
        content.append(
            &gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .wrap(true)
                .lines(4)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
        );
    }
    let answers = shown.note.entry.as_ref().map(|entry| summary(&fields, &entry.answers));
    if let Some(answers) = answers.filter(|answers| !answers.is_empty()) {
        content.append(
            &gtk::Label::builder()
                .label(&answers)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["dimmed"])
                .build(),
        );
    }
    content
}

/// The entry's text without its headings, which the journal's template wrote.
fn preview(body: &str) -> String {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    lines.join("\n")
}

/// The answers as text, in the order of the journal's questions.
pub fn summary(journal: &Journal, answers: &BTreeMap<String, Answer>) -> String {
    let parts: Vec<String> = journal
        .questions
        .iter()
        .filter_map(|question| Some((question, answers.get(&question.id)?)))
        .map(|(question, answer)| {
            format!("{} {}", question.prompt.trim(), describe(question, answer))
        })
        .collect();
    parts.join(" · ")
}

/// An answer as the owner reads it.
fn describe(question: &Question, answer: &Answer) -> String {
    match (&question.kind, answer) {
        (QuestionKind::Number { unit: Some(unit), .. }, Answer::Number(value)) => {
            format!("{value} {unit}")
        }
        (QuestionKind::Duration, Answer::Number(minutes)) => {
            let minutes = *minutes as i64;
            match (minutes / 60, minutes % 60) {
                (0, minutes) => format!("{minutes} min"),
                (hours, 0) => format!("{hours} h"),
                (hours, minutes) => format!("{hours} h {minutes} min"),
            }
        }
        (QuestionKind::Choice { options, .. }, Answer::Choices(ids)) => {
            let labels: Vec<&str> = ids
                .iter()
                .filter_map(|id| options.iter().find(|option| &option.id == id))
                .map(|option| option.label.as_str())
                .collect();
            labels.join(", ")
        }
        _ => answer.to_string(),
    }
}

/// A small dot in a journal's color.
pub fn dot(colors: (&'static str, &'static str)) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |_, cr, width, height| {
        let (r, g, b) = rgb(shade(colors));
        cr.set_source_rgb(r, g, b);
        let radius = 5.0_f64.min(f64::from(width.min(height)) / 2.0);
        cr.arc(f64::from(width) / 2.0, f64::from(height) / 2.0, radius, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });
    area
}

const CHART_HEIGHT: i32 = 140;
const PAD_LEFT: f64 = 48.0;
const PAD_RIGHT: f64 = 16.0;
const PAD_TOP: f64 = 8.0;
const PAD_BOTTOM: f64 = 24.0;

/// The ink color the widget's text uses, at `alpha`.
fn ink(widget: &gtk::Widget, alpha: f64) -> (f64, f64, f64, f64) {
    let color = widget.color();
    (f64::from(color.red()), f64::from(color.green()), f64::from(color.blue()), alpha)
}

fn date_label(at: i64) -> String {
    glib::DateTime::from_unix_local(at.div_euclid(1000))
        .ok()
        .and_then(|time| time.format("%b %-d").ok())
        .map(String::from)
        .unwrap_or_default()
}

/// A line over time through one question's numeric answers, with a tooltip for the nearest point.
fn line_chart(question: &Question, mut points: Vec<(i64, f64)>, color: &str) -> gtk::DrawingArea {
    points.sort_by_key(|(at, _)| *at);
    let area = gtk::DrawingArea::builder()
        .content_height(CHART_HEIGHT)
        .hexpand(true)
        .has_tooltip(true)
        .build();
    let (first, last) = (points.first().map_or(0, |p| p.0), points.last().map_or(0, |p| p.0));
    let (mut low, mut high) =
        points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (_, v)| (lo.min(*v), hi.max(*v)));
    if let QuestionKind::Scale { min, max, .. } = question.kind {
        (low, high) = (min as f64, max as f64);
    }
    if (high - low).abs() < f64::EPSILON {
        (low, high) = (low - 1.0, high + 1.0);
    }
    let is_time = question.kind == QuestionKind::TimeOfDay;
    let format_value = move |value: f64| {
        if is_time {
            format_time(value.round().clamp(0.0, 1439.0) as u16)
        } else {
            format!("{}", (value * 100.0).round() / 100.0)
        }
    };
    let place = move |width: f64, height: f64, at: i64, value: f64| {
        let span = (last - first).max(1) as f64;
        let x = if last == first {
            PAD_LEFT + (width - PAD_LEFT - PAD_RIGHT) / 2.0
        } else {
            PAD_LEFT + (at - first) as f64 / span * (width - PAD_LEFT - PAD_RIGHT)
        };
        let y = PAD_TOP + (high - value) / (high - low) * (height - PAD_TOP - PAD_BOTTOM);
        (x, y)
    };
    let (r, g, b) = rgb(color);
    let drawn = points.clone();
    area.set_draw_func(move |area, cr, width, height| {
        let (width, height) = (f64::from(width), f64::from(height));
        let (ir, ig, ib, _) = ink(area.upcast_ref(), 1.0);
        cr.set_line_width(1.0);
        cr.set_source_rgba(ir, ig, ib, 0.15);
        for value in [low, high] {
            let (_, y) = place(width, height, first, value);
            cr.move_to(PAD_LEFT, y.round() + 0.5);
            cr.line_to(width - PAD_RIGHT, y.round() + 0.5);
        }
        let _ = cr.stroke();
        cr.set_source_rgba(ir, ig, ib, 0.6);
        cr.set_font_size(11.0);
        for value in [low, high] {
            let (_, y) = place(width, height, first, value);
            text_right(cr, &format_value(value), PAD_LEFT - 8.0, y + 4.0);
        }
        cr.move_to(PAD_LEFT, height - 6.0);
        let _ = cr.show_text(&date_label(first));
        if last != first {
            text_right(cr, &date_label(last), width - PAD_RIGHT, height - 6.0);
        }
        cr.set_source_rgb(r, g, b);
        cr.set_line_width(2.0);
        cr.set_line_join(cairo::LineJoin::Round);
        for (index, (at, value)) in drawn.iter().enumerate() {
            let (x, y) = place(width, height, *at, *value);
            if index == 0 { cr.move_to(x, y) } else { cr.line_to(x, y) }
        }
        let _ = cr.stroke();
        for (at, value) in &drawn {
            let (x, y) = place(width, height, *at, *value);
            cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });
    area.connect_query_tooltip(move |area, x, _, _, tooltip| {
        let (width, height) = (f64::from(area.width()), f64::from(area.height()));
        let nearest = points.iter().min_by(|a, b| {
            let distance = |point: &(i64, f64)| {
                (place(width, height, point.0, point.1).0 - f64::from(x)).abs()
            };
            distance(a).total_cmp(&distance(b))
        });
        let Some((at, value)) = nearest else {
            return false;
        };
        let when = glib::DateTime::from_unix_local(at.div_euclid(1000))
            .ok()
            .and_then(|time| time.format("%b %-d, %H:%M").ok())
            .map(String::from)
            .unwrap_or_default();
        tooltip.set_text(Some(&format!("{when}: {}", format_value(*value))));
        true
    });
    area
}

/// Whether the Journals tab charts a kind of question.
fn charts(kind: &QuestionKind) -> bool {
    kind.is_numeric() || matches!(kind, QuestionKind::Choice { .. } | QuestionKind::YesNo)
}

/// The local date of a timestamp.
fn local_date(at: i64) -> Option<Date> {
    let time = glib::DateTime::from_unix_local(at.div_euclid(1000)).ok()?;
    Date::new(time.year() as u16, time.month() as u8, time.day_of_month() as u8)
}

/// What a day grid draws in one cell.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Empty,
    /// A hollow ring, for a no.
    Ring,
    /// A filled dot, for a yes or an option picked.
    Dot,
}

const GRID_ROW: f64 = 22.0;
const GRID_LABELS: f64 = 96.0;
const GRID_COUNTS: f64 = 64.0;

/// A grid with a column for each day from `first` to `last`, in the journal's color. A choice
/// question gets a row per option, with a dot on each day that picked it. A yes or no question
/// gets one row, with a dot for yes and a ring for no.
fn day_grid(
    question: &Question,
    answers: &[(i64, &Answer)],
    first: Date,
    last: Date,
    color: &str,
) -> gtk::Box {
    let mut days = vec![first];
    while let Some(&day) = days.last()
        && day < last
    {
        days.push(day.add_days(1));
    }
    let column = |at: i64| local_date(at).and_then(|date| days.iter().position(|day| *day == date));
    let mut rows: Vec<(String, Vec<Mark>, String)> = Vec::new();
    let caption;
    match &question.kind {
        QuestionKind::Choice { options, .. } => {
            for option in options {
                let mut marks = vec![Mark::Empty; days.len()];
                for (at, answer) in answers {
                    if let (Answer::Choices(ids), Some(index)) = (answer, column(*at))
                        && ids.contains(&option.id)
                    {
                        marks[index] = Mark::Dot;
                    }
                }
                let count = marks.iter().filter(|mark| **mark == Mark::Dot).count();
                if option.retired.is_none() || count > 0 {
                    rows.push((option.label.clone(), marks, count.to_string()));
                }
            }
            caption = "A dot for each day that picked the option".to_owned();
        }
        _ => {
            let mut marks = vec![Mark::Empty; days.len()];
            for (at, answer) in answers {
                if let (Answer::Bool(yes), Some(index)) = (answer, column(*at)) {
                    // A day with both answers shows the yes.
                    if *yes || marks[index] == Mark::Empty {
                        marks[index] = if *yes { Mark::Dot } else { Mark::Ring };
                    }
                }
            }
            let yes = marks.iter().filter(|mark| **mark == Mark::Dot).count();
            let answered = marks.iter().filter(|mark| **mark != Mark::Empty).count();
            caption = format!("A dot for yes and a ring for no · Yes on {yes} of {answered} days");
            rows.push(("Yes".to_owned(), marks, format!("{yes} of {answered}")));
        }
    }
    let yes_no = question.kind == QuestionKind::YesNo;
    let height = (GRID_ROW * rows.len() as f64 + 22.0).ceil() as i32;
    let area =
        gtk::DrawingArea::builder().content_height(height).hexpand(true).has_tooltip(true).build();
    let (r, g, b) = rgb(color);
    let geometry =
        move |width: f64, count: usize| (width - GRID_LABELS - GRID_COUNTS) / count.max(1) as f64;
    let drawn = rows.clone();
    let day_count = days.len();
    let (first_label, last_label) = (format_date(first), format_date(last));
    area.set_draw_func(move |area, cr, width, height| {
        let width = f64::from(width);
        let cell = geometry(width, day_count);
        let radius = (cell.min(GRID_ROW) / 2.0 - 1.5).clamp(1.5, 6.0);
        let (ir, ig, ib, _) = ink(area.upcast_ref(), 1.0);
        cr.set_font_size(11.0);
        for (row, (label, marks, count)) in drawn.iter().enumerate() {
            let middle = GRID_ROW * row as f64 + GRID_ROW / 2.0;
            cr.set_source_rgba(ir, ig, ib, 0.7);
            cr.move_to(12.0, middle + 4.0);
            let _ = cr.show_text(label);
            text_right(cr, count, width - 12.0, middle + 4.0);
            cr.set_source_rgba(ir, ig, ib, 0.08);
            for index in 0..marks.len() {
                let x = GRID_LABELS + cell * (index as f64 + 0.5);
                cr.arc(x, middle, 1.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
            cr.set_source_rgb(r, g, b);
            cr.set_line_width(2.0);
            for (index, mark) in marks.iter().enumerate() {
                let x = GRID_LABELS + cell * (index as f64 + 0.5);
                match mark {
                    Mark::Dot => {
                        cr.arc(x, middle, radius, 0.0, std::f64::consts::TAU);
                        let _ = cr.fill();
                    }
                    Mark::Ring => {
                        cr.arc(x, middle, (radius - 1.0).max(1.0), 0.0, std::f64::consts::TAU);
                        let _ = cr.stroke();
                    }
                    Mark::Empty => {}
                }
            }
        }
        cr.set_source_rgba(ir, ig, ib, 0.6);
        let baseline = f64::from(height) - 6.0;
        cr.move_to(GRID_LABELS, baseline);
        let _ = cr.show_text(&first_label);
        if day_count > 1 {
            text_right(cr, &last_label, width - GRID_COUNTS, baseline);
        }
    });
    area.connect_query_tooltip(move |area, x, y, _, tooltip| {
        let cell = geometry(f64::from(area.width()), days.len());
        let index = ((f64::from(x) - GRID_LABELS) / cell).floor();
        let row = (f64::from(y) / GRID_ROW).floor();
        if index < 0.0 || row < 0.0 {
            return false;
        }
        let (Some(day), Some((label, marks, _))) =
            (days.get(index as usize), rows.get(row as usize))
        else {
            return false;
        };
        let state = match (yes_no, marks[index as usize]) {
            (true, Mark::Dot) => "Yes",
            (true, Mark::Ring) => "No",
            (false, Mark::Dot) => "Picked",
            (_, _) => "No answer",
        };
        tooltip.set_text(Some(&format!("{}\n{label}: {state}", format_date(*day))));
        true
    });
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 6);
    holder.append(&area);
    holder.append(
        &gtk::Label::builder()
            .label(caption)
            .xalign(0.0)
            .css_classes(["caption", "dimmed"])
            .margin_start(12)
            .margin_bottom(12)
            .build(),
    );
    holder
}

fn text_right(cr: &cairo::Context, text: &str, right: f64, baseline: f64) {
    let width = cr.text_extents(text).map_or(0.0, |extents| extents.x_advance());
    cr.move_to(right - width, baseline);
    let _ = cr.show_text(text);
}
