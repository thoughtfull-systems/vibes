// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Answer, Journal, QuestionKind, format_time, parse_time};
use gtk::glib;

type Changed = Rc<RefCell<Option<Box<dyn Fn(&str, Option<Answer>)>>>>;
type Reader = Box<dyn Fn() -> Option<Answer>>;
type Writer = Box<dyn Fn(Option<&Answer>)>;

/// One question's widgets, and how to read and fill them.
struct Field {
    question: String,
    get: Reader,
    set: Writer,
}

/// A form with a row for each of a journal's current questions that isn't a text question, which
/// the entry's body holds instead.
pub struct AnswerForm {
    pub widget: gtk::Grid,
    fields: Vec<Field>,
    changed: Changed,
    /// Set while the form fills itself, so filling it doesn't report changes.
    filling: Rc<Cell<bool>>,
}

impl AnswerForm {
    pub fn new(journal: &Journal) -> Rc<Self> {
        let widget = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let changed: Changed = Rc::default();
        let filling = Rc::new(Cell::new(false));
        let mut fields = Vec::new();
        for (row, question) in journal
            .current_questions()
            .filter(|question| question.kind != QuestionKind::Text)
            .enumerate()
        {
            // The report needs the field's reader, which exists only once the input does.
            let reader: Rc<RefCell<Option<Reader>>> = Rc::default();
            let report: Rc<dyn Fn()> = Rc::new(glib::clone!(
                #[strong]
                changed,
                #[strong]
                filling,
                #[strong]
                reader,
                #[strong(rename_to = id)]
                question.id,
                move || {
                    if filling.get() {
                        return;
                    }
                    let answer = reader.borrow().as_ref().and_then(|get| get());
                    if let Some(changed) = changed.borrow().as_ref() {
                        changed(&id, answer);
                    }
                }
            ));
            let (input, get, set) = input(&question.kind, report);
            reader.replace(Some(get));
            let label = gtk::Label::builder()
                .label(question.prompt.trim())
                .xalign(0.0)
                .wrap(true)
                .css_classes(["dimmed"])
                .build();
            widget.attach(&label, 0, row as i32, 1, 1);
            widget.attach(&input, 1, row as i32, 1, 1);
            let get = Box::new(move || reader.borrow().as_ref().and_then(|get| get()));
            fields.push(Field { question: question.id.clone(), get, set });
        }
        widget.set_visible(!fields.is_empty());
        Rc::new(Self { widget, fields, changed, filling })
    }

    /// The answers given, by question identifier.
    pub fn answers(&self) -> BTreeMap<String, Answer> {
        self.fields
            .iter()
            .filter_map(|field| Some((field.question.clone(), (field.get)()?)))
            .collect()
    }

    /// Fills the form with `answers`, without reporting changes.
    pub fn fill(&self, answers: &BTreeMap<String, Answer>) {
        self.filling.set(true);
        for field in &self.fields {
            (field.set)(answers.get(&field.question));
        }
        self.filling.set(false);
    }

    /// Calls `f` with the question and its new answer whenever the owner changes an answer.
    pub fn connect_changed(&self, f: impl Fn(&str, Option<Answer>) + 'static) {
        self.changed.replace(Some(Box::new(f)));
    }
}

type Input = (gtk::Widget, Reader, Writer);

/// The widget for one kind of question, with functions that read and fill it.
fn input(kind: &QuestionKind, report: Rc<dyn Fn()>) -> Input {
    match kind {
        QuestionKind::Text | QuestionKind::ShortText => {
            let entry = gtk::Entry::builder().hexpand(true).build();
            entry.connect_changed(move |_| report());
            let get = entry.clone();
            let set = entry.clone();
            (
                entry.upcast(),
                Box::new(move || {
                    let text = get.text().trim().to_owned();
                    (!text.is_empty()).then_some(Answer::Text(text))
                }),
                Box::new(move |answer| {
                    set.set_text(&answer.map(Answer::to_string).unwrap_or_default())
                }),
            )
        }
        QuestionKind::Number { unit, min, max } => {
            let placeholder = match (min, max) {
                (Some(min), Some(max)) => format!("{min} to {max}"),
                (Some(min), None) => format!("At least {min}"),
                (None, Some(max)) => format!("At most {max}"),
                (None, None) => String::new(),
            };
            let (min, max) = (*min, *max);
            parsed_entry(unit.as_deref(), &placeholder, report, move |text| {
                let value: f64 = text.parse().ok()?;
                let fits = min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max);
                fits.then_some(Answer::Number(value))
            })
        }
        QuestionKind::Duration => parsed_entry(None, "Minutes, or hours:minutes", report, |text| {
            parse_duration(text).map(|minutes| Answer::Number(f64::from(minutes)))
        }),
        QuestionKind::TimeOfDay => parsed_entry(None, "HH:MM", report, |text| {
            parse_loose_time(text).map(|minutes| Answer::Text(format_time(minutes)))
        }),
        QuestionKind::Scale { min, max, low, high } => {
            let row = gtk::Box::builder().spacing(6).build();
            if let Some(low) = low {
                row.append(&gtk::Label::builder().label(low).css_classes(["dimmed"]).build());
            }
            let choices: Vec<(String, String)> =
                (*min..=*max).map(|value| (value.to_string(), value.to_string())).collect();
            let (buttons, get, set) = exclusive(&choices, report);
            row.append(&buttons);
            if let Some(high) = high {
                row.append(&gtk::Label::builder().label(high).css_classes(["dimmed"]).build());
            }
            (
                row.upcast(),
                Box::new(move || get().and_then(|value| value.parse().ok()).map(Answer::Number)),
                Box::new(move |answer| {
                    set(answer.and_then(Answer::number).map(|value| (value as i64).to_string()));
                }),
            )
        }
        QuestionKind::YesNo => {
            let choices =
                [("yes".to_owned(), "Yes".to_owned()), ("no".to_owned(), "No".to_owned())];
            let (buttons, get, set) = exclusive(&choices, report);
            (
                buttons.upcast(),
                Box::new(move || get().map(|value| Answer::Bool(value == "yes"))),
                Box::new(move |answer| {
                    set(match answer {
                        Some(Answer::Bool(true)) => Some("yes".to_owned()),
                        Some(Answer::Bool(false)) => Some("no".to_owned()),
                        _ => None,
                    });
                }),
            )
        }
        QuestionKind::Choice { multiple: false, options } => {
            let choices: Vec<(String, String)> = options
                .iter()
                .filter(|option| option.retired.is_none())
                .map(|option| (option.id.clone(), option.label.clone()))
                .collect();
            let (buttons, get, set) = exclusive(&choices, report);
            (
                buttons.upcast(),
                Box::new(move || get().map(|id| Answer::Choices(vec![id]))),
                Box::new(move |answer| {
                    set(match answer {
                        Some(Answer::Choices(ids)) => ids.first().cloned(),
                        _ => None,
                    });
                }),
            )
        }
        QuestionKind::Choice { multiple: true, options } => {
            let row = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .max_children_per_line(4)
                .build();
            let checks: Rc<Vec<(String, gtk::CheckButton)>> = Rc::new(
                options
                    .iter()
                    .filter(|option| option.retired.is_none())
                    .map(|option| {
                        let check = gtk::CheckButton::with_label(&option.label);
                        let report = report.clone();
                        check.connect_toggled(move |_| report());
                        row.append(&check);
                        (option.id.clone(), check)
                    })
                    .collect(),
            );
            let get = checks.clone();
            (
                row.upcast(),
                Box::new(move || {
                    let ids: Vec<String> = get
                        .iter()
                        .filter(|(_, check)| check.is_active())
                        .map(|(id, _)| id.clone())
                        .collect();
                    (!ids.is_empty()).then_some(Answer::Choices(ids))
                }),
                Box::new(move |answer| {
                    let picked = match answer {
                        Some(Answer::Choices(ids)) => ids.clone(),
                        _ => Vec::new(),
                    };
                    for (id, check) in checks.iter() {
                        check.set_active(picked.contains(id));
                    }
                }),
            )
        }
    }
}

/// An entry whose text `parse` turns into an answer, marked as an error while it can't.
fn parsed_entry(
    unit: Option<&str>,
    placeholder: &str,
    report: Rc<dyn Fn()>,
    parse: impl Fn(&str) -> Option<Answer> + 'static,
) -> Input {
    let entry = gtk::Entry::builder().placeholder_text(placeholder).hexpand(true).build();
    let parse = Rc::new(parse);
    let check = parse.clone();
    entry.connect_changed(move |entry| {
        let text = entry.text();
        let text = text.trim();
        if text.is_empty() || check(text).is_some() {
            entry.remove_css_class("error");
        } else {
            entry.add_css_class("error");
        }
        report();
    });
    let widget: gtk::Widget = match unit {
        Some(unit) => {
            let row = gtk::Box::builder().spacing(6).build();
            row.append(&entry);
            row.append(&gtk::Label::builder().label(unit).css_classes(["dimmed"]).build());
            row.upcast()
        }
        None => entry.clone().upcast(),
    };
    let get = entry.clone();
    (
        widget,
        Box::new(move || parse(get.text().trim())),
        Box::new(move |answer| {
            let text = match answer {
                Some(Answer::Number(value)) => value.to_string(),
                Some(answer) => answer.to_string(),
                None => String::new(),
            };
            entry.set_text(&text);
        }),
    )
}

type Exclusive = (gtk::Box, Box<dyn Fn() -> Option<String>>, Box<dyn Fn(Option<String>)>);

/// A row of buttons of which at most one is on, so clicking the one that's on turns it off and
/// leaves the question unanswered.
fn exclusive(choices: &[(String, String)], report: Rc<dyn Fn()>) -> Exclusive {
    let row = gtk::Box::builder().spacing(6).css_classes(["linked"]).build();
    let buttons: Rc<Vec<(String, gtk::ToggleButton)>> = Rc::new(
        choices
            .iter()
            .map(|(id, label)| {
                let button = gtk::ToggleButton::with_label(label);
                row.append(&button);
                (id.clone(), button)
            })
            .collect(),
    );
    let updating = Rc::new(Cell::new(false));
    for (index, (_, button)) in buttons.iter().enumerate() {
        let others = Rc::downgrade(&buttons);
        let updating = updating.clone();
        let report = report.clone();
        button.connect_toggled(move |button| {
            if updating.get() {
                return;
            }
            if button.is_active()
                && let Some(others) = others.upgrade()
            {
                updating.set(true);
                for (other, (_, sibling)) in others.iter().enumerate() {
                    if other != index {
                        sibling.set_active(false);
                    }
                }
                updating.set(false);
            }
            report();
        });
    }
    let get = buttons.clone();
    (
        row,
        Box::new(move || {
            get.iter().find(|(_, button)| button.is_active()).map(|(id, _)| id.clone())
        }),
        Box::new(move |picked| {
            updating.set(true);
            for (id, button) in buttons.iter() {
                button.set_active(picked.as_deref() == Some(id.as_str()));
            }
            updating.set(false);
        }),
    )
}

/// Reads a duration in minutes from `90`, `1:30`, `1h30m`, `1h`, or `30m`.
fn parse_duration(text: &str) -> Option<u16> {
    let text = text.trim().to_lowercase();
    if let Some((hours, minutes)) = text.split_once(':') {
        let (hours, minutes): (u16, u16) = (hours.parse().ok()?, minutes.parse().ok()?);
        return (minutes < 60).then(|| hours.checked_mul(60)?.checked_add(minutes)).flatten();
    }
    if let Ok(minutes) = text.parse() {
        return Some(minutes);
    }
    let (hours, rest) = match text.split_once('h') {
        Some((hours, rest)) => (hours.trim().parse::<u16>().ok()?, rest.trim()),
        None => (0, text.as_str()),
    };
    let minutes = match rest.strip_suffix('m') {
        Some(minutes) => minutes.trim().parse::<u16>().ok()?,
        None if rest.is_empty() => 0,
        None => return None,
    };
    hours.checked_mul(60)?.checked_add(minutes)
}

/// Reads a time of day from `HH:MM` or `H:MM`.
fn parse_loose_time(text: &str) -> Option<u16> {
    let text = text.trim();
    match text.split_once(':') {
        Some((hours, _)) if hours.len() == 1 => parse_time(format!("0{text}")),
        _ => parse_time(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_durations() {
        assert_eq!(parse_duration("90"), Some(90));
        assert_eq!(parse_duration("1:30"), Some(90));
        assert_eq!(parse_duration("1h30m"), Some(90));
        assert_eq!(parse_duration("2h"), Some(120));
        assert_eq!(parse_duration("45m"), Some(45));
        assert_eq!(parse_duration("1:75"), None);
        assert_eq!(parse_duration("soon"), None);
    }

    #[test]
    fn reads_loose_times() {
        assert_eq!(parse_loose_time("9:05"), Some(9 * 60 + 5));
        assert_eq!(parse_loose_time("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_loose_time("24:00"), None);
    }
}
