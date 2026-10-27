// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Date, Note, NoteId, Question, format_time, parse_loose_time};
use gtk::glib;

use crate::answers::AnswerForm;
use crate::journals_view::{dot, journal_color};
use crate::prompts::local_millis;
use crate::rows::{connect_day_clicked, day_picker, format_date};
use crate::state::State;

type OpenHandler = Rc<dyn Fn(NoteId)>;

/// The details sidebar's fields for an entry: its journal, when it happened, and its answers.
pub struct EntryPanel {
    pub widget: gtk::Box,
    current: Cell<Option<NoteId>>,
    day: gtk::MenuButton,
    calendar: gtk::Calendar,
    form: RefCell<Option<Rc<AnswerForm>>>,
    form_holder: gtk::Box,
    /// The journal's questions that the form shows, so it's rebuilt only when they change.
    form_questions: RefCell<Vec<Question>>,
    journal: gtk::Button,
    journal_display: RefCell<Option<(String, String)>>,
    journal_id: Cell<Option<NoteId>>,
    on_open: RefCell<Option<OpenHandler>>,
    prompted: gtk::Label,
    state: Rc<State>,
    time: gtk::Entry,
}

impl EntryPanel {
    pub fn new(state: Rc<State>) -> Rc<Self> {
        let journal =
            gtk::Button::builder().css_classes(["flat"]).halign(gtk::Align::Start).build();
        let (day, calendar) = day_picker("Day", None);
        let time = gtk::Entry::builder().placeholder_text("HH:MM").width_chars(6).build();
        let when = gtk::Box::builder().spacing(6).build();
        when.append(&day);
        when.append(&time);
        let prompted =
            gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption", "dimmed"]).build();
        let form_holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let fields = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| {
            gtk::Label::builder().label(text).xalign(0.0).css_classes(["dimmed"]).build()
        };
        fields.attach(&label("Journal"), 0, 0, 1, 1);
        fields.attach(&journal, 1, 0, 1, 1);
        fields.attach(&label("When"), 0, 1, 1, 1);
        fields.attach(&when, 1, 1, 1, 1);
        fields.attach(&prompted, 0, 2, 2, 1);
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .visible(false)
            .build();
        widget.append(&fields);
        widget.append(&form_holder);
        let this = Rc::new(Self {
            widget,
            current: Cell::new(None),
            day,
            calendar,
            form: RefCell::default(),
            form_holder,
            form_questions: RefCell::default(),
            journal,
            journal_display: RefCell::default(),
            journal_id: Cell::new(None),
            on_open: RefCell::default(),
            prompted,
            state,
            time,
        });
        this.journal.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| {
                let handler = this.on_open.borrow().clone();
                if let (Some(handler), Some(journal)) = (handler, this.journal_id.get()) {
                    handler(journal);
                }
            }
        ));
        connect_day_clicked(
            &this.day,
            &this.calendar,
            glib::clone!(
                #[weak]
                this,
                move |date| this.set_when(Some(date), None)
            ),
        );
        this.time.connect_activate(glib::clone!(
            #[weak]
            this,
            move |_| this.save_time()
        ));
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak]
            this,
            move |_| this.save_time()
        ));
        this.time.add_controller(focus);
        this
    }

    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    /// Shows the fields of `note` if it's an entry, and hides the panel otherwise.
    pub fn show(self: &Rc<Self>, note: Option<&Note>, editable: bool) {
        let entry = note.and_then(|note| note.entry.clone());
        let id = entry.as_ref().and(note.map(|note| note.id));
        let changed_note = self.current.get() != id;
        self.current.set(id);
        self.widget.set_visible(entry.is_some());
        self.widget.set_sensitive(editable);
        let (Some(entry), Some(id)) = (entry, id) else {
            return;
        };
        let journal = self.state.read(|spaces| spaces.note(entry.journal));
        self.journal_id.set(Some(entry.journal));
        let name =
            journal.as_ref().and_then(Note::label).unwrap_or_else(|| "Missing journal".to_owned());
        let color = journal
            .as_ref()
            .and_then(|note| note.journal.as_ref())
            .map_or("blue", |fields| fields.color.as_str());
        let display = (name, color.to_owned());
        if self.journal_display.borrow().as_ref() != Some(&display) {
            let content = gtk::Box::builder().spacing(6).build();
            content.append(&dot(journal_color(color)));
            content.append(&gtk::Label::new(Some(&display.0)));
            self.journal.set_child(Some(&content));
            self.journal_display.replace(Some(display));
        }
        if let Ok(time) = glib::DateTime::from_unix_local(entry.at.div_euclid(1000)) {
            let date = Date::new(time.year() as u16, time.month() as u8, time.day_of_month() as u8);
            if let Some(date) = date {
                self.day.set_label(&format_date(date));
            }
            self.time.remove_css_class("error");
            // The entry's text child holds the focus, never the entry itself.
            let typing = self.time.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN);
            if !typing || changed_note {
                self.time.set_text(&format_time((time.hour() * 60 + time.minute()) as u16));
            }
        }
        match entry
            .prompted
            .and_then(|at| glib::DateTime::from_unix_local(at.div_euclid(1000)).ok())
        {
            Some(time) => {
                let at = format_time((time.hour() * 60 + time.minute()) as u16);
                self.prompted.set_label(&format!("Answers the {at} prompt"));
                self.prompted.set_visible(true);
            }
            None => self.prompted.set_visible(false),
        }
        let questions: Vec<Question> = journal
            .and_then(|journal| journal.journal)
            .map(|fields| fields.questions)
            .unwrap_or_default();
        if changed_note || *self.form_questions.borrow() != questions {
            let fields =
                commonplace_core::Journal { questions: questions.clone(), ..Default::default() };
            let form = AnswerForm::new(&fields);
            form.connect_changed(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |question, answer| {
                    if let Err(error) =
                        this.state.write(|spaces| spaces.set_answer(id, question, answer.as_ref()))
                    {
                        eprintln!("commonplace: {error}");
                    }
                }
            ));
            if let Some(old) = self.form.replace(Some(form.clone())) {
                self.form_holder.remove(&old.widget);
            }
            self.form_holder.append(&form.widget);
            self.form_questions.replace(questions);
            form.fill(&entry.answers);
        } else if let Some(form) = self.form.borrow().as_ref()
            && form.answers() != entry.answers
            && !form.widget.has_focus()
            && form.widget.focus_child().is_none()
        {
            form.fill(&entry.answers);
        }
    }

    /// Moves the entry to the time typed in the time field, or marks the field as wrong.
    fn save_time(&self) {
        match parse_loose_time(self.time.text()) {
            Some(minute) => self.set_when(None, Some(minute)),
            None => self.time.add_css_class("error"),
        }
    }

    /// Moves the entry to `date` or to `minute`, keeping the part not given.
    fn set_when(&self, date: Option<Date>, minute: Option<u16>) {
        let Some(id) = self.current.get() else {
            return;
        };
        let Some(entry) = self.state.read(|spaces| spaces.note(id)).and_then(|note| note.entry)
        else {
            return;
        };
        let Ok(time) = glib::DateTime::from_unix_local(entry.at.div_euclid(1000)) else {
            return;
        };
        let old_date = Date::new(time.year() as u16, time.month() as u8, time.day_of_month() as u8);
        let Some(date) = date.or(old_date) else {
            return;
        };
        let minute = minute.unwrap_or((time.hour() * 60 + time.minute()) as u16);
        let at = local_millis(date, minute);
        if let Err(error) = self.state.write(|spaces| spaces.set_entry_at(id, at)) {
            eprintln!("commonplace: {error}");
        }
    }
}
