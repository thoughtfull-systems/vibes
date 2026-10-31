// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Journals, their entries, and their prompts, as the library format spec defines them.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use loro::{LoroMap, LoroMovableList, LoroValue, UpdateOptions, ValueOrContainer};

use crate::actions::Date;
use crate::days_off::{DaysOffKind, DaysOffSet, DaysOffUse};
use crate::library::{MODIFIED, integer, loro_error, string};
use crate::recur::Rule;
use crate::{Error, Library, NoteId, wikilink};

pub const JOURNAL_KIND: &str = "journal";
pub const ENTRY_KIND: &str = "entry";

pub(crate) const JOURNAL: &str = "journal";
pub(crate) const ENTRY: &str = "entry";
const ANSWERS: &str = "answers";
const AT: &str = "at";
const COLOR: &str = "color";
const HANDLED: &str = "handled";
const PROMPT: &str = "prompt";
const PROMPTED: &str = "prompted";
const QUESTIONS: &str = "questions";
const SCHEDULES: &str = "schedules";

/// The colors a journal can take, in the order a picker offers them.
pub const COLORS: [&str; 8] =
    ["blue", "green", "yellow", "orange", "red", "purple", "brown", "gray"];

/// A journal's fields.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Journal {
    pub color: String,
    /// Every question, retired ones included, in order.
    pub questions: Vec<Question>,
    /// Every schedule, in order of identifier, which is the order they were made in.
    pub schedules: Vec<Schedule>,
    /// What happened to each prompt, by its key.
    pub handled: HashMap<String, Handled>,
}

impl Journal {
    fn from_value(value: Option<&LoroValue>) -> Self {
        let Some(LoroValue::Map(map)) = value else {
            return Self { color: COLORS[0].to_owned(), ..Self::default() };
        };
        let color = map
            .get(COLOR)
            .and_then(|v| string(v.clone()))
            .filter(|color| COLORS.contains(&color.as_str()))
            .unwrap_or_else(|| COLORS[0].to_owned());
        let questions = match map.get(QUESTIONS) {
            Some(LoroValue::List(list)) => list.iter().filter_map(Question::from_value).collect(),
            _ => Vec::new(),
        };
        let mut schedules: Vec<Schedule> = match map.get(SCHEDULES) {
            Some(LoroValue::Map(schedules)) => {
                schedules.iter().filter_map(|(id, value)| Schedule::from_value(id, value)).collect()
            }
            _ => Vec::new(),
        };
        schedules.sort_by(|a, b| a.id.cmp(&b.id));
        let handled = match map.get(HANDLED) {
            Some(LoroValue::Map(handled)) => handled
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), Handled::from_value(value)?)))
                .collect(),
            _ => HashMap::new(),
        };
        Self { color, questions, schedules, handled }
    }

    /// The questions that aren't retired, in order.
    pub fn current_questions(&self) -> impl Iterator<Item = &Question> {
        self.questions.iter().filter(|question| question.retired.is_none())
    }

    pub fn question(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|question| question.id == id)
    }

    /// The body a new entry starts with: a heading for each current text question.
    pub fn template(&self) -> String {
        self.current_questions()
            .filter(|question| question.kind == QuestionKind::Text)
            .map(|question| format!("## {}\n\n", question.prompt.trim()))
            .collect()
    }

    /// The prompt to show now, if any: the latest of today's prompts that has come due and that
    /// nobody has handled, or whose snooze has run out. `minute` is the local time of day, in
    /// minutes since midnight, `now` the current timestamp, and `local` turns a local date and
    /// minute into a timestamp.
    pub(crate) fn due_prompt(
        &self,
        journal: NoteId,
        today: Date,
        minute: u16,
        now: i64,
        local: impl Fn(Date, u16) -> i64,
        days_off: &DaysOffSet,
    ) -> Option<Prompt> {
        self.schedules
            .iter()
            .flat_map(|schedule| {
                schedule.minutes_on(journal, today, days_off).into_iter().map(|at| Prompt {
                    journal,
                    key: prompt_key(&schedule.id, today, at),
                    date: today,
                    minute: at,
                })
            })
            .filter_map(|prompt| {
                let due = match self.handled.get(&prompt.key) {
                    None if prompt.minute <= minute => local(today, prompt.minute),
                    Some(Handled::Snoozed { until }) if *until <= now => *until,
                    _ => return None,
                };
                Some((due, prompt))
            })
            .max_by(|(a, first), (b, second)| a.cmp(b).then(first.key.cmp(&second.key)))
            .map(|(_, prompt)| prompt)
    }
}

/// One question a journal asks.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Question {
    pub id: String,
    pub prompt: String,
    pub kind: QuestionKind,
    /// When the owner removed the question, which keeps its answers' labels.
    pub retired: Option<i64>,
}

impl Question {
    pub fn new(prompt: &str, kind: QuestionKind) -> Self {
        Self { id: NoteId::generate().to_string(), prompt: prompt.to_owned(), kind, retired: None }
    }

    fn from_value(value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        let text = |key: &str| map.get(key).and_then(|v| string(v.clone()));
        let number = |key: &str| match map.get(key) {
            Some(LoroValue::Double(value)) => Some(*value),
            Some(LoroValue::I64(value)) => Some(*value as f64),
            _ => None,
        };
        let kind = match text("type")?.as_str() {
            "text" => QuestionKind::Text,
            "short_text" => QuestionKind::ShortText,
            "number" => QuestionKind::Number {
                unit: text("unit").filter(|unit| !unit.trim().is_empty()),
                min: number("min"),
                max: number("max"),
            },
            "scale" => QuestionKind::Scale {
                min: map.get("min").and_then(|v| integer(v.clone())).unwrap_or(1),
                max: map.get("max").and_then(|v| integer(v.clone())).unwrap_or(5),
                low: text("low").filter(|label| !label.trim().is_empty()),
                high: text("high").filter(|label| !label.trim().is_empty()),
            },
            "choice" => QuestionKind::Choice {
                multiple: matches!(map.get("multiple"), Some(LoroValue::Bool(true))),
                options: match map.get("options") {
                    Some(LoroValue::List(list)) => {
                        list.iter().filter_map(ChoiceOption::from_value).collect()
                    }
                    _ => Vec::new(),
                },
            },
            "yes_no" => QuestionKind::YesNo,
            "duration" => QuestionKind::Duration,
            "time" => QuestionKind::TimeOfDay,
            _ => return None,
        };
        Some(Self {
            id: text("id")?,
            prompt: text("prompt").unwrap_or_default(),
            kind,
            retired: map.get("retired").and_then(|v| integer(v.clone())),
        })
    }

    fn to_value(&self) -> LoroValue {
        let mut fields: Vec<(String, LoroValue)> = vec![
            ("id".into(), self.id.as_str().into()),
            ("prompt".into(), self.prompt.as_str().into()),
            ("type".into(), self.kind.name().into()),
        ];
        if let Some(retired) = self.retired {
            fields.push(("retired".into(), retired.into()));
        }
        match &self.kind {
            QuestionKind::Number { unit, min, max } => {
                if let Some(unit) = unit {
                    fields.push(("unit".into(), unit.as_str().into()));
                }
                if let Some(min) = min {
                    fields.push(("min".into(), LoroValue::Double(*min)));
                }
                if let Some(max) = max {
                    fields.push(("max".into(), LoroValue::Double(*max)));
                }
            }
            QuestionKind::Scale { min, max, low, high } => {
                fields.push(("min".into(), (*min).into()));
                fields.push(("max".into(), (*max).into()));
                if let Some(low) = low {
                    fields.push(("low".into(), low.as_str().into()));
                }
                if let Some(high) = high {
                    fields.push(("high".into(), high.as_str().into()));
                }
            }
            QuestionKind::Choice { multiple, options } => {
                fields.push(("multiple".into(), (*multiple).into()));
                let options: Vec<LoroValue> = options.iter().map(ChoiceOption::to_value).collect();
                fields.push(("options".into(), LoroValue::List(options.into())));
            }
            _ => {}
        }
        LoroValue::Map(fields.into())
    }
}

/// What kind of answer a question takes.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum QuestionKind {
    /// Prose, written in the entry's body under a heading.
    Text,
    /// One line, kept in the entry's answers.
    ShortText,
    Number {
        unit: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
    },
    /// A whole number between `min` and `max`, with optional labels for the ends.
    Scale {
        min: i64,
        max: i64,
        low: Option<String>,
        high: Option<String>,
    },
    Choice {
        multiple: bool,
        options: Vec<ChoiceOption>,
    },
    YesNo,
    /// A number of minutes.
    Duration,
    /// A time of day, written `HH:MM`.
    TimeOfDay,
}

impl QuestionKind {
    /// The name the format stores for the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ShortText => "short_text",
            Self::Number { .. } => "number",
            Self::Scale { .. } => "scale",
            Self::Choice { .. } => "choice",
            Self::YesNo => "yes_no",
            Self::Duration => "duration",
            Self::TimeOfDay => "time",
        }
    }

    /// Whether a chart can draw its answers as a line over time.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Number { .. } | Self::Scale { .. } | Self::Duration | Self::TimeOfDay)
    }

    /// The answer that typed `text` gives to a number, duration, or time of day question, or
    /// `None` if it isn't a valid one or the kind takes no typed answer.
    pub fn parse_answer(&self, text: &str) -> Option<Answer> {
        let text = text.trim();
        match self {
            Self::Number { min, max, .. } => {
                let value = text.parse::<f64>().ok().filter(|value| value.is_finite())?;
                let fits = min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max);
                fits.then_some(Answer::Number(value))
            }
            Self::Duration => {
                parse_duration(text).map(|minutes| Answer::Number(f64::from(minutes)))
            }
            Self::TimeOfDay => {
                parse_loose_time(text).map(|minutes| Answer::Text(format_time(minutes)))
            }
            _ => None,
        }
    }

    /// The hint that an empty field shows for a typed answer.
    pub fn placeholder(&self) -> String {
        match self {
            Self::Number { min: Some(min), max: Some(max), .. } => format!("{min} to {max}"),
            Self::Number { min: Some(min), max: None, .. } => format!("At least {min}"),
            Self::Number { min: None, max: Some(max), .. } => format!("At most {max}"),
            Self::Duration => "Minutes, or hours:minutes".to_owned(),
            Self::TimeOfDay => "HH:MM".to_owned(),
            _ => String::new(),
        }
    }
}

/// One of a choice question's options.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
    pub retired: Option<i64>,
}

impl ChoiceOption {
    pub fn new(label: &str) -> Self {
        Self { id: NoteId::generate().to_string(), label: label.to_owned(), retired: None }
    }

    fn from_value(value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        Some(Self {
            id: map.get("id").and_then(|v| string(v.clone()))?,
            label: map.get("label").and_then(|v| string(v.clone())).unwrap_or_default(),
            retired: map.get("retired").and_then(|v| integer(v.clone())),
        })
    }

    fn to_value(&self) -> LoroValue {
        let mut fields: Vec<(String, LoroValue)> = vec![
            ("id".into(), self.id.as_str().into()),
            ("label".into(), self.label.as_str().into()),
        ];
        if let Some(retired) = self.retired {
            fields.push(("retired".into(), retired.into()));
        }
        LoroValue::Map(fields.into())
    }
}

/// When a journal prompts.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Schedule {
    pub id: String,
    /// The days the schedule prompts on.
    pub rule: Rule,
    /// The date the rule counts from.
    pub start: Date,
    pub days_off: DaysOffUse,
    pub times: Times,
}

/// The date a schedule with no `start` counts from, so that its rule covers every date.
const NO_START: Date = Date::EPOCH;

impl Schedule {
    pub fn new(rule: Rule, start: Date, times: Times) -> Self {
        let id = NoteId::generate().to_string();
        Self { id, rule, start, days_off: DaysOffUse::default(), times }
    }

    /// Reads a stored schedule, or nothing if it's malformed or one `set_schedule` would refuse.
    fn from_value(id: &str, value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        let minutes = |key: &str| map.get(key).and_then(|v| string(v.clone())).and_then(parse_time);
        let count = |key: &str| {
            map.get(key).and_then(|v| integer(v.clone())).and_then(|n| u16::try_from(n).ok())
        };
        let times = match map.get("kind").and_then(|v| string(v.clone()))?.as_str() {
            "fixed" => {
                let mut times: Vec<u16> = match map.get("times") {
                    Some(LoroValue::List(list)) => {
                        list.iter().filter_map(|v| string(v.clone()).and_then(parse_time)).collect()
                    }
                    _ => Vec::new(),
                };
                times.sort_unstable();
                times.dedup();
                Times::Fixed(times)
            }
            "random" => Times::Random {
                start: minutes("start")?,
                end: minutes("end")?,
                mean: count("mean").filter(|&mean| mean > 0)?,
                min_gap: count("min_gap").unwrap_or(0),
            },
            _ => return None,
        };
        let rule = map.get("rule").and_then(|v| string(v.clone())).and_then(|r| Rule::parse(&r))?;
        let start = match map.get("from").and_then(|v| string(v.clone())) {
            Some(text) => text.parse().ok()?,
            None => NO_START,
        };
        let days_off = DaysOffUse::from_map(map);
        let schedule = Self { id: id.to_owned(), rule, start, days_off, times };
        schedule.is_valid().then_some(schedule)
    }

    fn to_value(&self) -> LoroValue {
        let mut fields: Vec<(String, LoroValue)> =
            vec![("rule".into(), self.rule.to_string().into())];
        if self.start != NO_START {
            fields.push(("from".into(), self.start.to_string().into()));
        }
        for kind in DaysOffKind::ALL {
            if let Some(effect) = self.days_off.effect(kind).as_str() {
                fields.push((kind.key().into(), effect.into()));
            }
        }
        match &self.times {
            Times::Fixed(times) => {
                fields.push(("kind".into(), "fixed".into()));
                let times: Vec<LoroValue> =
                    times.iter().map(|&time| format_time(time).into()).collect();
                fields.push(("times".into(), LoroValue::List(times.into())));
            }
            Times::Random { start, end, mean, min_gap } => {
                fields.push(("kind".into(), "random".into()));
                fields.push(("start".into(), format_time(*start).into()));
                fields.push(("end".into(), format_time(*end).into()));
                fields.push(("mean".into(), i64::from(*mean).into()));
                fields.push(("min_gap".into(), i64::from(*min_gap).into()));
            }
        }
        LoroValue::Map(fields.into())
    }

    /// Whether the rule is one the spec allows, every time falls within a day, and a random
    /// schedule has a window and a mean gap.
    pub fn is_valid(&self) -> bool {
        self.rule.is_valid()
            && match &self.times {
                Times::Fixed(times) => !times.is_empty() && times.iter().all(|at| *at < 1440),
                &Times::Random { start, end, mean, min_gap } => {
                    start < end && end < 1440 && mean > 0 && mean <= 1440 && min_gap <= 1440
                }
            }
    }

    /// Whether the schedule prompts on `date`: the rule gives the date, unless a day off moves it
    /// away, or the rule gives an earlier date that a day off moves to it.
    fn prompts_on(&self, date: Date, days_off: &DaysOffSet) -> bool {
        let uses = &self.days_off;
        if !DaysOffSet::applies(uses) {
            return self.rule.all_dates(self.start, date).next() == Some(date);
        }
        let end = self.rule.end(self.start);
        let given = |day: Date| {
            let skip = Box::new(|d| days_off.skips(uses, d));
            self.rule.iterate(self.start, day, skip, end).next() == Some(day)
        };
        if given(date) && days_off.moved(uses, date) == date {
            return true;
        }
        let first = days_off.before(uses, date).max(self.start);
        let skip = Box::new(|day| days_off.skips(uses, day));
        self.rule
            .iterate(self.start, first, skip, end)
            .take_while(|&day| day < date)
            .any(|day| days_off.moved(uses, day) == date)
    }

    /// The local times, in minutes since midnight, at which the schedule prompts on `date`.
    pub(crate) fn minutes_on(
        &self,
        journal: NoteId,
        date: Date,
        days_off: &DaysOffSet,
    ) -> Vec<u16> {
        if !self.prompts_on(date, days_off) {
            return Vec::new();
        }
        match &self.times {
            Times::Fixed(times) => times.clone(),
            &Times::Random { start, end, mean, min_gap } => {
                let mut random = SplitMix64::new(fnv1a(&format!("{journal} {} {date}", self.id)));
                let mut minutes = Vec::new();
                let mut free = start;
                for minute in start..end {
                    if minute < free {
                        continue;
                    }
                    if random.next().is_multiple_of(u64::from(mean)) {
                        minutes.push(minute);
                        free = minute.saturating_add(min_gap.max(1));
                    }
                }
                minutes
            }
        }
    }
}

/// The times of day a schedule prompts at.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Times {
    /// These local times, in minutes since midnight.
    Fixed(Vec<u16>),
    /// Random times from `start` until `end`, in minutes since midnight. After each prompt, the
    /// next waits at least `min_gap` minutes, and each minute after that has a one in `mean`
    /// chance of a prompt.
    Random { start: u16, end: u16, mean: u16, min_gap: u16 },
}

/// What happened to a prompt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Handled {
    Answered { entry: NoteId },
    Skipped,
    Snoozed { until: i64 },
}

impl Handled {
    fn from_value(value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        match map.get("state").and_then(|v| string(v.clone()))?.as_str() {
            "answered" => Some(Self::Answered {
                entry: map.get("entry").and_then(|v| string(v.clone()))?.parse().ok()?,
            }),
            "skipped" => Some(Self::Skipped),
            "snoozed" => {
                Some(Self::Snoozed { until: map.get("until").and_then(|v| integer(v.clone()))? })
            }
            _ => None,
        }
    }

    fn to_value(self, at: i64) -> LoroValue {
        let fields: Vec<(String, LoroValue)> = match self {
            Self::Answered { entry } => vec![
                ("state".into(), "answered".into()),
                ("entry".into(), entry.to_string().into()),
            ],
            Self::Skipped => vec![("state".into(), "skipped".into())],
            Self::Snoozed { until } => {
                vec![("state".into(), "snoozed".into()), ("until".into(), until.into())]
            }
        };
        let mut fields = fields;
        fields.push(("at".into(), at.into()));
        LoroValue::Map(fields.into())
    }
}

/// One time a journal asks its questions.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Prompt {
    pub journal: NoteId,
    pub key: String,
    pub date: Date,
    /// The local time it came due, in minutes since midnight.
    pub minute: u16,
}

/// The key of the prompt that a schedule makes on `date` at `minute`.
pub fn prompt_key(schedule: &str, date: Date, minute: u16) -> String {
    format!("{schedule}/{date}/{}", format_time(minute))
}

/// The date in a prompt key.
fn key_date(key: &str) -> Option<Date> {
    key.split('/').nth(1)?.parse().ok()
}

/// An entry's fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub journal: NoteId,
    /// When the entry happened, as a timestamp.
    pub at: i64,
    /// The key of the prompt it answers.
    pub prompt: Option<String>,
    /// When that prompt came due.
    pub prompted: Option<i64>,
    /// The answers to every question but the text ones, by question identifier.
    pub answers: BTreeMap<String, Answer>,
}

impl Entry {
    fn from_value(value: Option<&LoroValue>, created: i64) -> Option<Self> {
        let Some(LoroValue::Map(map)) = value else {
            return None;
        };
        let answers = match map.get(ANSWERS) {
            Some(LoroValue::Map(answers)) => answers
                .iter()
                .filter_map(|(id, value)| Some((id.clone(), Answer::from_value(value)?)))
                .collect(),
            _ => BTreeMap::new(),
        };
        Some(Self {
            journal: map.get(JOURNAL).and_then(|v| string(v.clone()))?.parse().ok()?,
            at: map.get(AT).and_then(|v| integer(v.clone())).unwrap_or(created),
            prompt: map.get(PROMPT).and_then(|v| string(v.clone())),
            prompted: map.get(PROMPTED).and_then(|v| integer(v.clone())),
            answers,
        })
    }
}

/// An answer to a question that isn't a text question.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Answer {
    /// A number, scale, or duration answer.
    Number(f64),
    /// A short text or time of day answer.
    Text(String),
    Bool(bool),
    /// The identifiers of the options picked.
    Choices(Vec<String>),
}

impl Answer {
    fn from_value(value: &LoroValue) -> Option<Self> {
        match value {
            LoroValue::Double(value) => Some(Self::Number(*value)),
            LoroValue::I64(value) => Some(Self::Number(*value as f64)),
            LoroValue::String(_) => string(value.clone()).map(Self::Text),
            LoroValue::Bool(value) => Some(Self::Bool(*value)),
            LoroValue::List(list) => {
                Some(Self::Choices(list.iter().filter_map(|v| string(v.clone())).collect()))
            }
            _ => None,
        }
    }

    fn to_value(&self) -> LoroValue {
        match self {
            Self::Number(value) if value.fract() == 0.0 && value.abs() < 1e15 => {
                LoroValue::I64(*value as i64)
            }
            Self::Number(value) => LoroValue::Double(*value),
            Self::Text(text) => text.as_str().into(),
            Self::Bool(value) => (*value).into(),
            Self::Choices(ids) => {
                LoroValue::List(ids.iter().map(|id| id.as_str().into()).collect::<Vec<_>>().into())
            }
        }
    }

    /// The answer as a number to chart, for a number, scale, duration, or time of day.
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Text(text) => parse_time(text).map(f64::from),
            _ => None,
        }
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(text) => f.write_str(text),
            Self::Bool(true) => f.write_str("Yes"),
            Self::Bool(false) => f.write_str("No"),
            Self::Choices(ids) => f.write_str(&ids.join(", ")),
        }
    }
}

/// Reads `HH:MM` as minutes since midnight.
pub fn parse_time(text: impl AsRef<str>) -> Option<u16> {
    let (hours, minutes) = text.as_ref().split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    let (hours, minutes): (u16, u16) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// Reads `HH:MM`, or `H:MM` with a one-digit hour, as minutes since midnight.
pub fn parse_loose_time(text: impl AsRef<str>) -> Option<u16> {
    let text = text.as_ref().trim();
    match text.split_once(':') {
        Some((hours, _)) if hours.len() == 1 => parse_time(format!("0{text}")),
        _ => parse_time(text),
    }
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

/// Writes minutes since midnight as `HH:MM`.
pub fn format_time(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The 64-bit FNV-1a hash of `text`'s UTF-8 bytes.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Sebastiano Vigna's SplitMix64 generator, which the spec fixes so every device draws the same
/// random prompt times.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

pub(crate) fn journal_from_value(value: Option<&LoroValue>) -> Journal {
    Journal::from_value(value)
}

pub(crate) fn entry_from_value(value: Option<&LoroValue>, created: i64) -> Option<Entry> {
    Entry::from_value(value, created)
}

impl Library {
    pub fn create_journal(&mut self, name: &str) -> Result<NoteId, Error> {
        self.check_writable()?;
        let id = self.insert_note(&format!("{}\n", name.trim()), JOURNAL_KIND)?;
        let journal =
            self.note_map(id)?.insert_container(JOURNAL, LoroMap::new()).map_err(loro_error)?;
        journal.insert(COLOR, COLORS[0]).map_err(loro_error)?;
        journal.insert_container(QUESTIONS, LoroMovableList::new()).map_err(loro_error)?;
        journal.insert_container(SCHEDULES, LoroMap::new()).map_err(loro_error)?;
        journal.insert_container(HANDLED, LoroMap::new()).map_err(loro_error)?;
        self.commit()?;
        Ok(id)
    }

    /// Every journal, by its identifier, outside the trash or in it.
    pub fn journals(&self) -> Vec<crate::Note> {
        self.notes().into_iter().filter(|note| note.journal.is_some()).collect()
    }

    pub fn set_journal_color(&mut self, id: NoteId, color: &str) -> Result<(), Error> {
        self.check_writable()?;
        if !COLORS.contains(&color) {
            return Ok(());
        }
        self.journal_map(id)?.insert(COLOR, color).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Adds a question at the end of the journal's questions.
    pub fn add_question(&mut self, id: NoteId, question: &Question) -> Result<(), Error> {
        self.check_writable()?;
        let list = self.questions_list(id)?;
        list.push(question.to_value()).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Replaces the question with the same identifier.
    pub fn set_question(&mut self, id: NoteId, question: &Question) -> Result<(), Error> {
        self.check_writable()?;
        let list = self.questions_list(id)?;
        let Some(index) = question_index(&list, &question.id) else {
            return Err(Error::Malformed(id));
        };
        // Answers are typed by their question, so a new type would strand the old answers.
        let old = self
            .note(id)
            .and_then(|note| note.journal)
            .and_then(|journal| journal.question(&question.id).map(|old| old.kind.name()));
        if old.is_some_and(|old| old != question.kind.name()) {
            return Err(Error::QuestionTypeChanged);
        }
        list.set(index, question.to_value()).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Moves a question up or down one place among the questions.
    pub fn move_question(&mut self, id: NoteId, question: &str, up: bool) -> Result<(), Error> {
        self.check_writable()?;
        let list = self.questions_list(id)?;
        let Some(index) = question_index(&list, question) else {
            return Err(Error::Malformed(id));
        };
        let target = if up { index.checked_sub(1) } else { Some(index + 1) };
        let Some(target) = target.filter(|&target| target < list.len()) else {
            return Ok(());
        };
        list.mov(index, target).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Adds a schedule, or replaces the one with the same identifier.
    pub fn set_schedule(&mut self, id: NoteId, schedule: &Schedule) -> Result<(), Error> {
        self.check_writable()?;
        if !schedule.is_valid() {
            return Err(Error::InvalidSchedule);
        }
        self.editable_schedules(id)?
            .insert(&schedule.id, schedule.to_value())
            .map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    pub fn remove_schedule(&mut self, id: NoteId, schedule: &str) -> Result<(), Error> {
        self.check_writable()?;
        self.editable_schedules(id)?.delete(schedule).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// The journal's prompt to show now, if any, as [`Journal::due_prompt`] finds it, with the
    /// library's days off applied.
    pub fn due_prompt(
        &self,
        journal: NoteId,
        today: Date,
        minute: u16,
        now: i64,
        local: impl Fn(Date, u16) -> i64,
    ) -> Option<Prompt> {
        let fields = self.note(journal)?.journal?;
        fields.due_prompt(journal, today, minute, now, local, &self.days_off_set())
    }

    /// The local times, in minutes since midnight, at which a journal's schedule prompts on
    /// `date`, with the library's days off applied.
    pub fn schedule_minutes(&self, journal: NoteId, schedule: &Schedule, date: Date) -> Vec<u16> {
        schedule.minutes_on(journal, date, &self.days_off_set())
    }

    /// Records what happened to a prompt, and forgets the records of prompts from before
    /// yesterday, which never show again.
    pub fn handle_prompt(
        &mut self,
        journal: NoteId,
        key: &str,
        handled: Handled,
        today: Date,
    ) -> Result<(), Error> {
        self.check_writable()?;
        self.record_handled(journal, key, handled, today)?;
        self.commit()
    }

    fn record_handled(
        &self,
        journal: NoteId,
        key: &str,
        handled: Handled,
        today: Date,
    ) -> Result<(), Error> {
        let map = self.handled_map(journal)?;
        map.insert(key, handled.to_value(self.now())).map_err(loro_error)?;
        let mut stale = Vec::new();
        map.for_each(|key, _| {
            if key_date(key).is_none_or(|date| date < today.add_days(-1)) {
                stale.push(key.to_owned());
            }
        });
        for key in stale {
            map.delete(&key).map_err(loro_error)?;
        }
        Ok(())
    }

    /// Creates an entry in `journal`, answering `prompt` if it's given, with the prompt's key and
    /// the time it came due.
    pub fn create_entry(
        &mut self,
        journal: NoteId,
        body: &str,
        at: i64,
        prompt: Option<(&str, i64)>,
        answers: &BTreeMap<String, Answer>,
        today: Date,
    ) -> Result<NoteId, Error> {
        self.check_writable()?;
        let id = self.insert_entry(journal, body, at, prompt, answers)?;
        if let Some((key, _)) = prompt {
            self.record_handled(journal, key, Handled::Answered { entry: id }, today)?;
        }
        self.commit()?;
        Ok(id)
    }

    fn insert_entry(
        &mut self,
        journal: NoteId,
        body: &str,
        at: i64,
        prompt: Option<(&str, i64)>,
        answers: &BTreeMap<String, Answer>,
    ) -> Result<NoteId, Error> {
        if self.note(journal).and_then(|note| note.journal).is_none() {
            return Err(Error::NotAJournal(journal));
        }
        let id = self.insert_note(body, ENTRY_KIND)?;
        let entry =
            self.note_map(id)?.insert_container(ENTRY, LoroMap::new()).map_err(loro_error)?;
        entry.insert(JOURNAL, journal.to_string()).map_err(loro_error)?;
        entry.insert(AT, at).map_err(loro_error)?;
        if let Some((key, prompted)) = prompt {
            entry.insert(PROMPT, key).map_err(loro_error)?;
            entry.insert(PROMPTED, prompted).map_err(loro_error)?;
        }
        let map = entry.insert_container(ANSWERS, LoroMap::new()).map_err(loro_error)?;
        for (question, answer) in answers {
            map.insert(question, answer.to_value()).map_err(loro_error)?;
        }
        Ok(id)
    }

    pub fn set_entry_at(&mut self, id: NoteId, at: i64) -> Result<(), Error> {
        self.check_writable()?;
        self.entry_map(id)?.insert(AT, at).map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Sets an entry's answer to a question, or removes it.
    pub fn set_answer(
        &mut self,
        id: NoteId,
        question: &str,
        answer: Option<&Answer>,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let entry = self.entry_map(id)?;
        let answers = match entry.get(ANSWERS) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        };
        let answers = match answers {
            Some(answers) => answers,
            None => entry.insert_container(ANSWERS, LoroMap::new()).map_err(loro_error)?,
        };
        match answer {
            Some(answer) => answers.insert(question, answer.to_value()),
            None => answers.delete(question),
        }
        .map_err(loro_error)?;
        self.touch(id)?;
        self.commit()
    }

    /// Turns a memo into an entry in `journal`, in one commit: the entry takes the memo's body,
    /// the memo's body becomes a link to the entry, and the memo goes to the archive.
    pub fn convert_memo(&mut self, memo: NoteId, journal: NoteId) -> Result<NoteId, Error> {
        self.check_writable()?;
        let note = self.note(memo).ok_or(Error::NotFound(memo))?;
        if !note.is_memo() {
            return Err(Error::NotAMemo(memo));
        }
        let entry = self.insert_entry(journal, &note.body, note.created, None, &BTreeMap::new())?;
        self.body_text(memo)?
            .update(&format!("{}\n", wikilink(entry, None)), UpdateOptions::default())
            .map_err(loro_error)?;
        let map = self.note_map(memo)?;
        let now = self.now();
        map.insert(MODIFIED, now).map_err(loro_error)?;
        map.insert(crate::library::ARCHIVED, now).map_err(loro_error)?;
        self.commit()?;
        Ok(entry)
    }

    /// The identifiers of a journal's entries, outside the trash or in it.
    pub fn journal_entries(&self, journal: NoteId) -> Vec<NoteId> {
        self.notes()
            .into_iter()
            .filter(|note| note.entry.as_ref().is_some_and(|entry| entry.journal == journal))
            .map(|note| note.id)
            .collect()
    }

    /// The journal an entry belongs to, for an entry whose journal is in this library.
    pub(crate) fn entry_journal(&self, id: NoteId) -> Option<NoteId> {
        let journal = self.note(id)?.entry?.journal;
        self.note_map(journal).is_ok().then_some(journal)
    }

    fn touch(&self, id: NoteId) -> Result<(), Error> {
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        Ok(())
    }

    fn journal_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.note_map(id)?.get(JOURNAL) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotAJournal(id))
    }

    fn entry_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.note_map(id)?.get(ENTRY) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::Malformed(id))
    }

    fn questions_list(&self, id: NoteId) -> Result<LoroMovableList, Error> {
        let journal = self.journal_map(id)?;
        match journal.get(QUESTIONS) {
            Some(ValueOrContainer::Container(container)) => {
                container.into_movable_list().map_err(|_| Error::Malformed(id))
            }
            _ => journal.insert_container(QUESTIONS, LoroMovableList::new()).map_err(loro_error),
        }
    }

    fn schedules_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        child_map(&self.journal_map(id)?, SCHEDULES, id)
    }

    /// The journal's schedules, after deleting those that `Schedule::from_value` skips. The apps
    /// never show those, so changing the schedules is the only chance to remove them.
    fn editable_schedules(&self, id: NoteId) -> Result<LoroMap, Error> {
        let map = self.schedules_map(id)?;
        if let LoroValue::Map(entries) = map.get_value() {
            for (key, value) in entries.iter() {
                if Schedule::from_value(key, value).is_none() {
                    map.delete(key).map_err(loro_error)?;
                }
            }
        }
        Ok(map)
    }

    fn handled_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        child_map(&self.journal_map(id)?, HANDLED, id)
    }
}

fn child_map(parent: &LoroMap, key: &str, id: NoteId) -> Result<LoroMap, Error> {
    match parent.get(key) {
        Some(ValueOrContainer::Container(container)) => {
            container.into_map().map_err(|_| Error::Malformed(id))
        }
        _ => parent.insert_container(key, LoroMap::new()).map_err(loro_error),
    }
}

fn question_index(list: &LoroMovableList, id: &str) -> Option<usize> {
    (0..list.len()).find(|&index| {
        list.get(index)
            .and_then(|value| match value {
                ValueOrContainer::Value(value) => Question::from_value(&value),
                ValueOrContainer::Container(_) => None,
            })
            .is_some_and(|question| question.id == id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn reads_durations() {
        assert_eq!(parse_duration("90"), Some(90));
        assert_eq!(parse_duration("1:30"), Some(90));
        assert_eq!(parse_duration("1h30m"), Some(90));
        assert_eq!(parse_duration("2h"), Some(120));
        assert_eq!(parse_duration("45m"), Some(45));
        assert_eq!(parse_duration("1:75"), None);
        assert_eq!(parse_duration("1.5"), None);
        assert_eq!(parse_duration("soon"), None);
    }

    #[test]
    fn parses_typed_answers() {
        let number = QuestionKind::Number { unit: None, min: Some(0.0), max: Some(10.0) };
        assert_eq!(number.parse_answer(" 2.5 "), Some(Answer::Number(2.5)));
        assert_eq!(number.parse_answer("11"), None);
        assert_eq!(
            QuestionKind::Number { unit: None, min: None, max: None }.parse_answer("NaN"),
            None
        );
        assert_eq!(QuestionKind::Duration.parse_answer("1:30"), Some(Answer::Number(90.0)));
        assert_eq!(
            QuestionKind::TimeOfDay.parse_answer("9:05"),
            Some(Answer::Text("09:05".into()))
        );
        assert_eq!(QuestionKind::TimeOfDay.parse_answer("25:00"), None);
        assert_eq!(QuestionKind::YesNo.parse_answer("yes"), None);
        assert_eq!(number.placeholder(), "0 to 10");
    }

    #[test]
    fn schedules_follow_rules_and_days_off() {
        let mut library = Library::in_memory();
        let journal = library.create_journal("Log").unwrap();
        let workdays = Rule::parse("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR").unwrap();
        let mut schedule = Schedule::new(workdays, Date::EPOCH, Times::Fixed(vec![600]));
        library.set_schedule(journal, &schedule).unwrap();
        let read = library.note(journal).unwrap().journal.unwrap().schedules;
        assert_eq!(read, [schedule.clone()]);
        let minutes = |library: &Library, schedule: &Schedule, day: &str| {
            library.schedule_minutes(journal, schedule, date(day))
        };
        // 2026-10-24 is a Saturday.
        assert!(minutes(&library, &schedule, "2026-10-24").is_empty());
        assert_eq!(minutes(&library, &schedule, "2026-10-26"), [600]);
        let mut holiday =
            crate::DaysOff::new(DaysOffKind::Holidays, "Founders' day", date("2026-10-26"));
        holiday.rule = Rule::parse("FREQ=YEARLY");
        library.set_days_off(&holiday).unwrap();
        assert_eq!(minutes(&library, &schedule, "2026-10-26"), [600]);
        schedule.days_off.holidays = crate::DayOffEffect::Skip;
        assert!(minutes(&library, &schedule, "2026-10-26").is_empty());
        assert!(minutes(&library, &schedule, "2026-10-27").len() == 1);
        schedule.days_off.holidays = crate::DayOffEffect::Move;
        library.set_schedule(journal, &schedule).unwrap();
        let read = library.note(journal).unwrap().journal.unwrap().schedules;
        assert_eq!(read, [schedule.clone()]);
        assert!(minutes(&library, &schedule, "2026-10-26").is_empty());
        assert_eq!(minutes(&library, &schedule, "2026-10-27"), [600]);
    }

    #[test]
    fn moved_prompts_pass_days_off_they_skip() {
        let mut library = Library::in_memory();
        let journal = library.create_journal("Log").unwrap();
        let mondays = Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap();
        let mut schedule = Schedule::new(mondays, Date::EPOCH, Times::Fixed(vec![600]));
        schedule.days_off.holidays = crate::DayOffEffect::Move;
        schedule.days_off.vacations = crate::DayOffEffect::Skip;
        // 2026-10-26 is a Monday.
        let holiday = crate::DaysOff::new(DaysOffKind::Holidays, "Holiday", date("2026-10-26"));
        library.set_days_off(&holiday).unwrap();
        let mut trip = crate::DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-10-27"));
        trip.last = date("2026-10-28");
        library.set_days_off(&trip).unwrap();
        let minutes = |day: &str| library.schedule_minutes(journal, &schedule, date(day));
        assert!(minutes("2026-10-26").is_empty());
        assert!(minutes("2026-10-27").is_empty());
        assert_eq!(minutes("2026-10-29"), [600]);
    }

    #[test]
    fn refuses_schedules_outside_the_day() {
        let mut library = Library::in_memory();
        let journal = library.create_journal("Log").unwrap();
        for times in [
            Times::Fixed(Vec::new()),
            Times::Fixed(vec![1440]),
            Times::Random { start: 600, end: 600, mean: 30, min_gap: 0 },
            Times::Random { start: 600, end: 1440, mean: 30, min_gap: 0 },
            Times::Random { start: 600, end: 700, mean: 0, min_gap: 0 },
        ] {
            let schedule = Schedule::new(Rule::default(), Date::EPOCH, times);
            assert!(matches!(
                library.set_schedule(journal, &schedule),
                Err(Error::InvalidSchedule)
            ));
        }
        let fixed = Schedule::new(Rule::default(), Date::EPOCH, Times::Fixed(vec![0, 1439]));
        library.set_schedule(journal, &fixed).unwrap();
        // Another device or an earlier version can still store a schedule that set_schedule
        // refuses.
        let backwards = Schedule::new(
            Rule::default(),
            Date::EPOCH,
            Times::Random { start: 700, end: 600, mean: 30, min_gap: 0 },
        );
        let schedules = library.schedules_map(journal).unwrap();
        schedules.insert(&backwards.id, backwards.to_value()).unwrap();
        let read = library.note(journal).unwrap().journal.unwrap().schedules;
        assert_eq!(read, std::slice::from_ref(&fixed));
        library.set_schedule(journal, &fixed).unwrap();
        assert!(schedules.get(&backwards.id).is_none());
    }

    #[test]
    fn random_times_depend_only_on_their_seed() {
        let journal = NoteId::from_uuid(uuid::Uuid::from_u128(1));
        let schedule = Schedule {
            id: "s".to_owned(),
            rule: Rule::default(),
            start: Date::EPOCH,
            days_off: DaysOffUse::default(),
            times: Times::Random { start: 9 * 60, end: 17 * 60, mean: 45, min_gap: 20 },
        };
        let first = schedule.minutes_on(journal, date("2026-10-26"), &DaysOffSet::default());
        assert_eq!(first, schedule.minutes_on(journal, date("2026-10-26"), &DaysOffSet::default()));
        assert_ne!(first, schedule.minutes_on(journal, date("2026-10-27"), &DaysOffSet::default()));
        assert!(!first.is_empty());
        assert!(first.iter().all(|&minute| (9 * 60..17 * 60).contains(&minute)));
        assert!(first.windows(2).all(|pair| pair[1] - pair[0] >= 20));
    }

    #[test]
    fn fixes_the_generator() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
        let mut random = SplitMix64::new(1_234_567);
        assert_eq!(random.next(), 6_457_827_717_110_365_317);
        assert_eq!(random.next(), 3_203_168_211_198_807_973);
    }

    fn work_log(library: &mut Library) -> (NoteId, Schedule) {
        let journal = library.create_journal("Work log").unwrap();
        let schedule =
            Schedule::new(Rule::default(), Date::EPOCH, Times::Fixed(vec![10 * 60, 14 * 60]));
        library.set_schedule(journal, &schedule).unwrap();
        (journal, schedule)
    }

    #[test]
    fn shows_the_latest_unhandled_prompt() {
        let mut library = Library::in_memory();
        let (journal, schedule) = work_log(&mut library);
        let today = date("2026-10-26");
        let local = |_: Date, minute: u16| i64::from(minute) * 60_000;
        let due = |library: &Library, minute: u16| {
            library.due_prompt(journal, today, minute, i64::from(minute) * 60_000, local)
        };
        assert_eq!(due(&library, 9 * 60), None);
        let morning = due(&library, 11 * 60).unwrap();
        assert_eq!(morning.key, prompt_key(&schedule.id, today, 10 * 60));
        let afternoon = due(&library, 15 * 60).unwrap();
        assert_eq!(afternoon.minute, 14 * 60);
        let until = i64::from(15 * 60 + 15) * 60_000;
        library.handle_prompt(journal, &afternoon.key, Handled::Snoozed { until }, today).unwrap();
        assert_eq!(due(&library, 15 * 60).unwrap().key, morning.key);
        assert_eq!(due(&library, 15 * 60 + 15).unwrap().key, afternoon.key);
        library.handle_prompt(journal, &afternoon.key, Handled::Skipped, today).unwrap();
        library.handle_prompt(journal, &morning.key, Handled::Skipped, today).unwrap();
        assert_eq!(due(&library, 16 * 60), None);
    }

    #[test]
    fn answering_a_prompt_records_it() {
        let mut library = Library::in_memory();
        let (journal, _) = work_log(&mut library);
        let today = date("2026-10-26");
        let stale = prompt_key("old", date("2026-10-20"), 600);
        library.handle_prompt(journal, &stale, Handled::Skipped, date("2026-10-20")).unwrap();
        let mood =
            Question::new("Mood", QuestionKind::Scale { min: 1, max: 5, low: None, high: None });
        library.add_question(journal, &mood).unwrap();
        let answers = BTreeMap::from([(mood.id.clone(), Answer::Number(4.0))]);
        let key = prompt_key("s", today, 600);
        let entry = library
            .create_entry(journal, "## Notes\n\nShipped it.\n", 5, Some((&key, 4)), &answers, today)
            .unwrap();
        let note = library.note(entry).unwrap();
        assert_eq!(note.kind, ENTRY_KIND);
        assert_eq!(note.label().as_deref(), Some("Shipped it."));
        let fields = note.entry.unwrap();
        assert_eq!(fields.journal, journal);
        assert_eq!((fields.at, fields.prompted), (5, Some(4)));
        assert_eq!(fields.answers.get(&mood.id), Some(&Answer::Number(4.0)));
        let handled = library.note(journal).unwrap().journal.unwrap().handled;
        assert_eq!(handled.get(&key), Some(&Handled::Answered { entry }));
        assert!(!handled.contains_key(&stale));
    }

    #[test]
    fn edits_questions_by_identifier() {
        let mut library = Library::in_memory();
        let (journal, _) = work_log(&mut library);
        let done = Question::new("What did you do?", QuestionKind::Text);
        let blockers = Question::new("Blockers", QuestionKind::Text);
        library.add_question(journal, &done).unwrap();
        library.add_question(journal, &blockers).unwrap();
        library.move_question(journal, &blockers.id, true).unwrap();
        let mut renamed = done.clone();
        renamed.prompt = "Done".to_owned();
        library.set_question(journal, &renamed).unwrap();
        let fields = library.note(journal).unwrap().journal.unwrap();
        let prompts: Vec<&str> = fields.questions.iter().map(|q| q.prompt.as_str()).collect();
        assert_eq!(prompts, ["Blockers", "Done"]);
        assert_eq!(fields.template(), "## Blockers\n\n## Done\n\n");
        let mut retired = renamed;
        retired.retired = Some(1);
        library.set_question(journal, &retired).unwrap();
        let mut retyped = blockers.clone();
        retyped.kind = QuestionKind::ShortText;
        assert!(matches!(library.set_question(journal, &retyped), Err(Error::QuestionTypeChanged)));
        let fields = library.note(journal).unwrap().journal.unwrap();
        assert_eq!(fields.current_questions().count(), 1);
        assert_eq!(fields.questions.len(), 2);
    }

    #[test]
    fn converts_a_memo_into_an_entry() {
        let mut library = Library::in_memory();
        let (journal, _) = work_log(&mut library);
        let memo = library.create_memo("Fixed the build").unwrap();
        let created = library.note(memo).unwrap().created;
        let entry = library.convert_memo(memo, journal).unwrap();
        let note = library.note(entry).unwrap();
        assert_eq!(note.body, "Fixed the build");
        assert_eq!(note.entry.unwrap().at, created);
        let memo = library.note(memo).unwrap();
        assert_eq!(memo.body, format!("{}\n", wikilink(entry, None)));
        assert!(memo.archived.is_some());
        assert!(matches!(library.convert_memo(memo.id, memo.id), Err(Error::NotAJournal(_))));
        let task = library.create_task("x", None).unwrap();
        assert!(matches!(library.convert_memo(task, journal), Err(Error::NotAMemo(_))));
    }

    #[test]
    fn entries_follow_their_journal_through_the_trash() {
        let mut library = Library::in_memory();
        let (journal, _) = work_log(&mut library);
        let entry = library
            .create_entry(journal, "x", 1, None, &BTreeMap::new(), date("2026-10-26"))
            .unwrap();
        library.trash(journal).unwrap();
        assert!(library.is_in_trash(entry));
        assert!(library.notes_in_trash().contains(&entry));
        library.restore(entry).unwrap();
        assert!(!library.is_in_trash(journal));
        library.trash(journal).unwrap();
        library.delete(journal).unwrap();
        assert_eq!(library.note(entry), None);
        assert_eq!(library.deleted_label(entry).as_deref(), Some("x"));
    }
}
