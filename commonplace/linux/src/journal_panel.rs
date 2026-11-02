// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{
    COLORS, ChoiceOption, Date, Journal, Note, NoteId, Question, QuestionKind, Rule, Schedule,
    Times, describe_rule, format_time, parse_loose_time, parse_time,
};
use gtk::glib;

use crate::journals_view::{dot, journal_color};
use crate::repeat::{DaysOffMenus, RepeatChoice, RepeatEditor, RepeatMenu, present_repeat_dialog};
use crate::rows::today;
use crate::state::State;

/// The question types in the order the picker lists them, with their names.
const KINDS: [(&str, &str); 8] = [
    ("Text", "text"),
    ("Short text", "short_text"),
    ("Number", "number"),
    ("Scale", "scale"),
    ("Choice", "choice"),
    ("Yes or no", "yes_no"),
    ("Duration", "duration"),
    ("Time of day", "time"),
];

/// Each option's identifier, if it has one already, and its entry.
type Options = Rc<RefCell<Vec<(Option<String>, gtk::Entry)>>>;

/// The details sidebar's fields for a journal: whether it prompts here, its color, its questions,
/// and its schedules.
pub struct JournalPanel {
    pub widget: gtk::Box,
    color: gtk::DropDown,
    current: Cell<Option<NoteId>>,
    /// Set while the panel fills its widgets, so filling them doesn't change the journal.
    loading: Cell<bool>,
    prompts: gtk::Switch,
    questions: gtk::ListBox,
    schedules: gtk::ListBox,
    state: Rc<State>,
    /// What the lists last showed, so a refresh that changes nothing leaves them alone.
    shown: RefCell<Option<Journal>>,
}

impl JournalPanel {
    pub fn new(state: Rc<State>) -> Rc<Self> {
        let prompts = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let names: Vec<String> = COLORS.iter().map(|color| capitalized(color)).collect();
        let color =
            gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(name) = item.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            let Some(color) = COLORS.get(item.position() as usize) else {
                return;
            };
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.append(&dot(journal_color(color)));
            row.append(&gtk::Label::new(Some(&name.string())));
            item.set_child(Some(&row));
        });
        color.set_factory(Some(&factory));
        color.set_list_factory(Some(&factory));
        let fields = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| {
            gtk::Label::builder().label(text).xalign(0.0).css_classes(["dimmed"]).build()
        };
        fields.attach(&label("Prompt here?"), 0, 0, 1, 1);
        fields.attach(&prompts, 1, 0, 1, 1);
        fields.attach(&label("Color"), 0, 1, 1, 1);
        fields.attach(&color, 1, 1, 1, 1);
        prompts.set_halign(gtk::Align::Start);
        prompts
            .set_tooltip_text(Some("Send this journal’s prompts as notifications on this device"));
        let questions = gtk::ListBox::builder().css_classes(["boxed-list"]).build();
        let schedules = gtk::ListBox::builder().css_classes(["boxed-list"]).build();
        let add_question =
            gtk::Button::builder().label("Add question").halign(gtk::Align::Start).build();
        let add_schedule =
            gtk::Button::builder().label("Add schedule").halign(gtk::Align::Start).build();
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .visible(false)
            .build();
        widget.append(&fields);
        widget.append(&heading("Questions"));
        widget.append(&questions);
        widget.append(&add_question);
        widget.append(&heading("Schedules"));
        widget.append(&schedules);
        widget.append(&add_schedule);
        let this = Rc::new(Self {
            widget,
            color,
            current: Cell::new(None),
            loading: Cell::new(false),
            prompts,
            questions,
            schedules,
            state,
            shown: RefCell::new(None),
        });
        this.prompts.connect_active_notify(glib::clone!(
            #[weak]
            this,
            move |switch| {
                if let (false, Some(id)) = (this.loading.get(), this.current.get()) {
                    this.state.set_prompts_here(id, switch.is_active());
                }
            }
        ));
        this.color.connect_selected_notify(glib::clone!(
            #[weak]
            this,
            move |dropdown| {
                let color = COLORS.get(dropdown.selected() as usize);
                if let (false, Some(id), Some(color)) =
                    (this.loading.get(), this.current.get(), color)
                {
                    this.report(this.state.write(|spaces| spaces.set_journal_color(id, color)));
                }
            }
        ));
        add_question.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |button| this.edit_question(button, None)
        ));
        add_schedule.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |button| this.edit_schedule(button, None)
        ));
        this
    }

    /// Shows the fields of `note` if it's a journal, and hides the panel otherwise.
    pub fn show(self: &Rc<Self>, note: Option<&Note>, editable: bool) {
        let journal = note.and_then(|note| note.journal.clone());
        self.current.set(journal.as_ref().and(note.map(|note| note.id)));
        self.widget.set_visible(journal.is_some());
        self.widget.set_sensitive(editable);
        let (Some(journal), Some(note)) = (journal, note) else {
            self.shown.replace(None);
            return;
        };
        self.loading.set(true);
        self.prompts.set_active(self.state.prompts_here(note.id));
        let color = COLORS.iter().position(|color| *color == journal.color).unwrap_or(0);
        self.color.set_selected(color as u32);
        self.loading.set(false);
        let mut comparable = journal.clone();
        comparable.handled.clear();
        if self.shown.borrow().as_ref() == Some(&comparable) {
            return;
        }
        self.fill_questions(&journal);
        self.fill_schedules(&journal);
        self.shown.replace(Some(comparable));
    }

    fn fill_questions(self: &Rc<Self>, journal: &Journal) {
        self.questions.remove_all();
        let current: Vec<&Question> = journal.current_questions().collect();
        if current.is_empty() {
            self.questions.append(&placeholder("No questions"));
        }
        for (index, question) in current.iter().enumerate() {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(question.prompt.trim()))
                .subtitle(describe_question(question))
                .build();
            let up = icon_button(icon!("up"), "Move up");
            up.set_sensitive(index > 0);
            let down = icon_button(icon!("down"), "Move down");
            down.set_sensitive(index + 1 < current.len());
            let edit = icon_button(icon!("edit"), "Edit");
            let remove = icon_button(icon!("trash"), "Remove");
            for button in [&up, &down, &edit, &remove] {
                row.add_suffix(button);
            }
            let id = question.id.clone();
            let question = (*question).clone();
            for (button, up) in [(&up, true), (&down, false)] {
                let id = id.clone();
                button.connect_clicked(glib::clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |_| {
                        if let Some(journal) = this.current.get() {
                            this.report(
                                this.state.write(|spaces| spaces.move_question(journal, &id, up)),
                            );
                        }
                    }
                ));
            }
            let editing = question.clone();
            edit.connect_clicked(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |button| this.edit_question(button, Some(editing.clone()))
            ));
            remove.connect_clicked(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let Some(journal) = this.current.get() else {
                        return;
                    };
                    let mut retired = question.clone();
                    retired.retired = Some(glib::real_time() / 1000);
                    this.report(this.state.write(|spaces| spaces.set_question(journal, &retired)));
                }
            ));
            self.questions.append(&row);
        }
    }

    fn fill_schedules(self: &Rc<Self>, journal: &Journal) {
        self.schedules.remove_all();
        if journal.schedules.is_empty() {
            self.schedules.append(&placeholder("Takes entries only when you write one"));
        }
        for schedule in &journal.schedules {
            let row = adw::ActionRow::builder()
                .title(describe_rule(&schedule.rule, schedule.start))
                .subtitle(describe_times(&schedule.times))
                .build();
            let edit = icon_button(icon!("edit"), "Edit");
            let remove = icon_button(icon!("trash"), "Remove");
            row.add_suffix(&edit);
            row.add_suffix(&remove);
            let editing = schedule.clone();
            edit.connect_clicked(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |button| this.edit_schedule(button, Some(editing.clone()))
            ));
            let id = schedule.id.clone();
            remove.connect_clicked(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    if let Some(journal) = this.current.get() {
                        this.report(
                            this.state.write(|spaces| spaces.remove_schedule(journal, &id)),
                        );
                    }
                }
            ));
            self.schedules.append(&row);
        }
    }

    /// Opens a dialog that adds a question, or edits `question`.
    fn edit_question(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>, question: Option<Question>) {
        let Some(journal) = self.current.get() else {
            return;
        };
        let editor = QuestionEditor::new(question.as_ref());
        let adding = question.is_none();
        let dialog = adw::AlertDialog::new(
            Some(if adding { "Add question" } else { "Edit question" }),
            None,
        );
        dialog.add_responses(&[
            ("cancel", "_Cancel"),
            ("save", if adding { "_Add" } else { "_Save" }),
        ]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        dialog.set_extra_child(Some(&editor.widget));
        dialog.connect_response(
            Some("save"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    let Some(edited) = editor.question(question.as_ref()) else {
                        return;
                    };
                    this.report(this.state.write(|spaces| {
                        if adding {
                            spaces.add_question(journal, &edited)
                        } else {
                            spaces.set_question(journal, &edited)
                        }
                    }));
                }
            ),
        );
        dialog.present(Some(parent));
    }

    /// Opens a dialog that adds a schedule, or edits `schedule`.
    fn edit_schedule(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>, schedule: Option<Schedule>) {
        let Some(journal) = self.current.get() else {
            return;
        };
        let editor = ScheduleEditor::new(schedule.as_ref(), &self.state, journal);
        let adding = schedule.is_none();
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
        dialog.set_extra_child(Some(&editor.widget));
        dialog.connect_response(
            Some("save"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    let Some(edited) = editor.schedule(schedule.as_ref()) else {
                        return;
                    };
                    this.report(this.state.write(|spaces| spaces.set_schedule(journal, &edited)));
                }
            ),
        );
        dialog.present(Some(parent));
    }

    fn report(&self, result: Result<(), commonplace_core::Error>) {
        if let Err(error) = result {
            eprintln!("commonplace: {error}");
        }
    }
}

/// The widgets that edit one question.
struct QuestionEditor {
    widget: gtk::Box,
    prompt: gtk::Entry,
    kind: gtk::DropDown,
    unit: gtk::Entry,
    min: gtk::Entry,
    max: gtk::Entry,
    low: gtk::Entry,
    high: gtk::Entry,
    multiple: gtk::Switch,
    options: Options,
}

impl QuestionEditor {
    fn new(question: Option<&Question>) -> Rc<Self> {
        let prompt = gtk::Entry::builder()
            .placeholder_text("Question")
            .activates_default(true)
            .text(question.map(|question| question.prompt.as_str()).unwrap_or_default())
            .build();
        let kind = gtk::DropDown::from_strings(&KINDS.map(|(label, _)| label));
        let name = question.map_or("text", |question| question.kind.name());
        kind.set_selected(KINDS.iter().position(|(_, kind)| *kind == name).unwrap_or(0) as u32);
        if question.is_some() {
            kind.set_sensitive(false);
            kind.set_tooltip_text(Some(
                "A question keeps its type. Add a new question for another type.",
            ));
        }
        let entry = |placeholder: &str| gtk::Entry::builder().placeholder_text(placeholder).build();
        let (unit, min, max) = (entry("Unit, such as kg"), entry("Least"), entry("Greatest"));
        let (low, high) = (entry("Label for the least"), entry("Label for the greatest"));
        let multiple = gtk::Switch::builder().halign(gtk::Align::Start).build();
        let options: Options = Rc::default();
        let options_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        match question.map(|question| &question.kind) {
            Some(QuestionKind::Number { unit: u, min: lo, max: hi }) => {
                unit.set_text(u.as_deref().unwrap_or_default());
                min.set_text(&lo.map(|v| v.to_string()).unwrap_or_default());
                max.set_text(&hi.map(|v| v.to_string()).unwrap_or_default());
            }
            Some(QuestionKind::Scale { min: lo, max: hi, low: l, high: h }) => {
                min.set_text(&lo.to_string());
                max.set_text(&hi.to_string());
                low.set_text(l.as_deref().unwrap_or_default());
                high.set_text(h.as_deref().unwrap_or_default());
            }
            Some(QuestionKind::Choice { multiple: m, options: existing }) => {
                multiple.set_active(*m);
                for option in existing.iter().filter(|option| option.retired.is_none()) {
                    add_option(&options_box, &options, Some(option.id.clone()), &option.label);
                }
            }
            _ => {}
        }
        let add = gtk::Button::builder().label("Add option").halign(gtk::Align::Start).build();
        add.connect_clicked(glib::clone!(
            #[weak]
            options_box,
            #[strong]
            options,
            move |_| add_option(&options_box, &options, None, "")
        ));
        let grid = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| gtk::Label::builder().label(text).xalign(0.0).build();
        grid.attach(&label("Type"), 0, 0, 1, 1);
        grid.attach(&kind, 1, 0, 1, 1);
        let number_rows = [
            (label("Unit"), unit.clone()),
            (label("Least"), min.clone()),
            (label("Greatest"), max.clone()),
        ];
        let scale_rows = [(label("Low label"), low.clone()), (label("High label"), high.clone())];
        let mut row = 1;
        for (label, entry) in number_rows.iter().chain(scale_rows.iter()) {
            grid.attach(label, 0, row, 1, 1);
            grid.attach(entry, 1, row, 1, 1);
            row += 1;
        }
        let multiple_label = label("Pick several?");
        grid.attach(&multiple_label, 0, row, 1, 1);
        grid.attach(&multiple, 1, row, 1, 1);
        let choice_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        choice_box.append(&options_box);
        choice_box.append(&add);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 12);
        widget.append(&prompt);
        widget.append(&grid);
        widget.append(&choice_box);
        let update = glib::clone!(
            #[weak]
            choice_box,
            #[weak]
            multiple,
            #[weak]
            multiple_label,
            move |selected: u32| {
                let name = KINDS[selected as usize].1;
                for (label, entry) in &number_rows {
                    let shown = name == "number" || (name == "scale" && label.label() != "Unit");
                    label.set_visible(shown);
                    entry.set_visible(shown);
                }
                for (label, entry) in &scale_rows {
                    label.set_visible(name == "scale");
                    entry.set_visible(name == "scale");
                }
                multiple.set_visible(name == "choice");
                multiple_label.set_visible(name == "choice");
                choice_box.set_visible(name == "choice");
            }
        );
        update(kind.selected());
        kind.connect_selected_notify(move |kind| update(kind.selected()));
        Rc::new(Self { widget, prompt, kind, unit, min, max, low, high, multiple, options })
    }

    /// The question the widgets describe, keeping `existing`'s identifier and retired options.
    fn question(&self, existing: Option<&Question>) -> Option<Question> {
        let prompt = self.prompt.text().trim().to_owned();
        if prompt.is_empty() {
            return None;
        }
        let text =
            |entry: &gtk::Entry| Some(entry.text().trim().to_owned()).filter(|t| !t.is_empty());
        let number = |entry: &gtk::Entry| text(entry).and_then(|t| t.parse::<f64>().ok());
        let kind = match KINDS[self.kind.selected() as usize].1 {
            "short_text" => QuestionKind::ShortText,
            "number" => QuestionKind::Number {
                unit: text(&self.unit),
                min: number(&self.min),
                max: number(&self.max),
            },
            "scale" => {
                let min = number(&self.min).map_or(1, |v| v as i64);
                let max = number(&self.max).map_or(5, |v| v as i64).max(min + 1);
                QuestionKind::Scale { min, max, low: text(&self.low), high: text(&self.high) }
            }
            "choice" => {
                let mut options: Vec<ChoiceOption> = Vec::new();
                for (id, entry) in self.options.borrow().iter() {
                    let Some(label) = text(entry) else { continue };
                    options.push(match id {
                        Some(id) => ChoiceOption { id: id.clone(), label, retired: None },
                        None => ChoiceOption::new(&label),
                    });
                }
                if let Some(QuestionKind::Choice { options: old, .. }) = existing.map(|q| &q.kind) {
                    let now = glib::real_time() / 1000;
                    for option in old {
                        if !options.iter().any(|kept| kept.id == option.id) {
                            options.push(ChoiceOption {
                                retired: option.retired.or(Some(now)),
                                ..option.clone()
                            });
                        }
                    }
                }
                QuestionKind::Choice { multiple: self.multiple.is_active(), options }
            }
            "yes_no" => QuestionKind::YesNo,
            "duration" => QuestionKind::Duration,
            "time" => QuestionKind::TimeOfDay,
            _ => QuestionKind::Text,
        };
        Some(match existing {
            Some(existing) => Question { prompt, kind, ..existing.clone() },
            None => Question::new(&prompt, kind),
        })
    }
}

fn add_option(holder: &gtk::Box, options: &Options, id: Option<String>, label: &str) {
    let entry = gtk::Entry::builder().text(label).placeholder_text("Option").hexpand(true).build();
    let remove = icon_button(icon!("remove"), "Remove option");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.append(&entry);
    row.append(&remove);
    holder.append(&row);
    options.borrow_mut().push((id, entry.clone()));
    remove.connect_clicked(glib::clone!(
        #[weak]
        holder,
        #[weak]
        row,
        #[strong]
        options,
        #[strong]
        entry,
        move |_| {
            options.borrow_mut().retain(|(_, kept)| *kept != entry);
            holder.remove(&row);
        }
    ));
    if label.is_empty() {
        entry.grab_focus();
    }
}

/// The widgets that edit one schedule.
struct ScheduleEditor {
    widget: gtk::Box,
    rule: RefCell<Rule>,
    start: RefCell<Date>,
    repeat: Rc<RepeatMenu>,
    days_off: DaysOffMenus,
    random: gtk::Switch,
    times: gtk::Entry,
    start_time: gtk::Entry,
    end: gtk::Entry,
    mean: gtk::SpinButton,
    min_gap: gtk::SpinButton,
}

impl ScheduleEditor {
    fn new(schedule: Option<&Schedule>, state: &Rc<State>, journal: NoteId) -> Rc<Self> {
        let workdays = || Rule::parse("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR").unwrap_or_default();
        let rule = schedule.map_or_else(workdays, |schedule| schedule.rule.clone());
        let start = schedule.map_or_else(today, |schedule| schedule.start);
        let uses = schedule.map(|schedule| schedule.days_off).unwrap_or_default();
        let repeat = RepeatMenu::new(false);
        let random = gtk::Switch::builder().halign(gtk::Align::Start).build();
        let times = gtk::Entry::builder().placeholder_text("10:00, 14:00, 17:00").build();
        let start_time = gtk::Entry::builder().placeholder_text("09:00").text("09:00").build();
        let end = gtk::Entry::builder().placeholder_text("17:00").text("17:00").build();
        let mean = gtk::SpinButton::with_range(1.0, 720.0, 5.0);
        mean.set_value(60.0);
        let min_gap = gtk::SpinButton::with_range(0.0, 720.0, 5.0);
        min_gap.set_value(20.0);
        match schedule.map(|schedule| &schedule.times) {
            Some(Times::Fixed(list)) => {
                let list: Vec<String> = list.iter().map(|&minute| format_time(minute)).collect();
                times.set_text(&list.join(", "));
            }
            Some(Times::Random { start: s, end: e, mean: m, min_gap: g }) => {
                random.set_active(true);
                start_time.set_text(&format_time(*s));
                end.set_text(&format_time(*e));
                mean.set_value(f64::from(*m));
                min_gap.set_value(f64::from(*g));
            }
            None => times.set_text("10:00, 14:00, 17:00"),
        }
        let grid = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| gtk::Label::builder().label(text).xalign(0.0).build();
        grid.attach(&label("Repeats"), 0, 0, 1, 1);
        grid.attach(&repeat.button, 1, 0, 1, 1);
        grid.attach(&label("Random times?"), 0, 1, 1, 1);
        grid.attach(&random, 1, 1, 1, 1);
        let times_label = label("Times");
        grid.attach(&times_label, 0, 2, 1, 1);
        grid.attach(&times, 1, 2, 1, 1);
        let random_rows = [
            (label("From"), start_time.clone().upcast::<gtk::Widget>()),
            (label("Until"), end.clone().upcast()),
            (label("Minutes apart, on average"), mean.clone().upcast()),
            (label("Minutes apart, at least"), min_gap.clone().upcast()),
        ];
        for (row, (label, widget)) in random_rows.iter().enumerate() {
            grid.attach(label, 0, row as i32 + 3, 1, 1);
            grid.attach(widget, 1, row as i32 + 3, 1, 1);
        }
        grid.attach(&gtk::Separator::new(gtk::Orientation::Horizontal), 0, 7, 2, 1);
        let days_off = DaysOffMenus::new();
        days_off.show(uses);
        for ((name, menu), row) in days_off.rows().zip(8..) {
            grid.attach(&label(name), 0, row, 1, 1);
            grid.attach(menu, 1, row, 1, 1);
        }
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 12);
        widget.append(&grid);
        let update = glib::clone!(
            #[weak]
            times,
            #[weak]
            times_label,
            move |random: bool| {
                times.set_visible(!random);
                times_label.set_visible(!random);
                for (label, widget) in &random_rows {
                    label.set_visible(random);
                    widget.set_visible(random);
                }
            }
        );
        update(random.is_active());
        random.connect_active_notify(move |random| update(random.is_active()));
        let editor = Rc::new(Self {
            widget,
            rule: RefCell::new(rule),
            start: RefCell::new(start),
            repeat,
            days_off,
            random,
            times,
            start_time,
            end,
            mean,
            min_gap,
        });
        editor.show_rule();
        let state = state.clone();
        editor.repeat.connect_chosen(glib::clone!(
            #[weak]
            editor,
            move |choice| match choice {
                RepeatChoice::Rule(rule) => {
                    *editor.rule.borrow_mut() = rule;
                    editor.show_rule();
                }
                RepeatChoice::None => {}
                RepeatChoice::Custom => {
                    let rule = editor.rule.borrow().clone();
                    let start = *editor.start.borrow();
                    let uses = editor.days_off.uses();
                    let preview = crate::days_off::preview(&state, Some(journal), uses);
                    let custom = RepeatEditor::new(Some(&rule), start, true, preview);
                    present_repeat_dialog(
                        &editor.widget,
                        custom,
                        glib::clone!(
                            #[weak]
                            editor,
                            move |values| {
                                *editor.rule.borrow_mut() = values.rule;
                                *editor.start.borrow_mut() = values.start;
                                editor.show_rule();
                            }
                        ),
                    );
                }
            }
        ));
        editor
    }

    fn show_rule(self: &Rc<Self>) {
        let start = *self.start.borrow();
        self.repeat.show(Some(&self.rule.borrow()), start, start);
    }

    /// The schedule the widgets describe, keeping `existing`'s identifier.
    fn schedule(&self, existing: Option<&Schedule>) -> Option<Schedule> {
        let rule = self.rule.borrow().clone();
        let start = *self.start.borrow();
        let days_off = self.days_off.uses();
        let times = if self.random.is_active() {
            let start = parse_time(self.start_time.text().trim())?;
            let end = parse_time(self.end.text().trim())?;
            if end <= start {
                return None;
            }
            Times::Random {
                start,
                end,
                mean: self.mean.value_as_int().max(1) as u16,
                min_gap: self.min_gap.value_as_int().max(0) as u16,
            }
        } else {
            let mut list: Vec<u16> = self
                .times
                .text()
                .split([',', ' '])
                .filter(|part| !part.is_empty())
                .filter_map(parse_loose_time)
                .collect();
            list.sort_unstable();
            list.dedup();
            if list.is_empty() {
                return None;
            }
            Times::Fixed(list)
        };
        Some(match existing {
            Some(existing) => Schedule { rule, start, days_off, times, ..existing.clone() },
            None => Schedule { days_off, ..Schedule::new(rule, start, times) },
        })
    }
}

/// A short description of a question's type and its limits.
fn describe_question(question: &Question) -> String {
    let name =
        KINDS.iter().find(|(_, kind)| *kind == question.kind.name()).map_or("", |(label, _)| label);
    match &question.kind {
        QuestionKind::Number { unit: Some(unit), .. } => format!("{name}, in {unit}"),
        QuestionKind::Scale { min, max, .. } => format!("{name} from {min} to {max}"),
        QuestionKind::Choice { multiple, options } => {
            let labels: Vec<&str> = options
                .iter()
                .filter(|option| option.retired.is_none())
                .map(|option| option.label.as_str())
                .collect();
            let pick = if *multiple { "any of" } else { "one of" };
            format!("{name}: {pick} {}", labels.join(", "))
        }
        _ => name.to_owned(),
    }
}

/// The times a schedule prompts at, as words.
pub fn describe_times(times: &Times) -> String {
    match times {
        Times::Fixed(list) => {
            let list: Vec<String> = list.iter().map(|&minute| format_time(minute)).collect();
            format!("At {}", list.join(", "))
        }
        Times::Random { start, end, mean, .. } => format!(
            "About every {mean} minutes from {} until {}",
            format_time(*start),
            format_time(*end)
        ),
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).margin_top(6).build()
}

fn placeholder(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .css_classes(["dimmed"])
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build()
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build()
}
