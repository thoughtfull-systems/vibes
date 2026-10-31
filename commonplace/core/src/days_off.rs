// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Days off: holidays and vacations, which recurring actions and journal schedules can skip or move
//! past, as the library format spec defines them.

use loro::{LoroMap, LoroValue};

use crate::actions::Date;
use crate::library::{loro_error, string};
use crate::recur::{Frequency, MONTH_NAMES, Rule, describe_rule};
use crate::{Error, Library, NoteId};

const DAYS_OFF: &str = "days_off";
const ARCHIVED: &str = "archived";
const FIRST: &str = "first";
const KIND: &str = "kind";
const LAST: &str = "last";
const NAME: &str = "name";
const RULE: &str = "rule";

/// How many days after its last day a schedule of days off drops out of the lists.
const HIDE_AFTER: i32 = 7;

/// The kinds of days off, each a category of schedules.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum DaysOffKind {
    Holidays,
    Vacations,
}

impl DaysOffKind {
    pub const ALL: [Self; 2] = [Self::Holidays, Self::Vacations];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Holidays => "holidays",
            Self::Vacations => "vacations",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Holidays => "Holidays",
            Self::Vacations => "Vacations",
        }
    }
}

/// What a recurring action or journal schedule does with a date that falls on a day off.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum DayOffEffect {
    #[default]
    Ignore,
    /// Drops the date.
    Skip,
    /// Moves the date to the next day that isn't a day off.
    Move,
}

impl DayOffEffect {
    pub(crate) fn parse(text: Option<String>) -> Self {
        match text.as_deref() {
            Some("skip") => Self::Skip,
            Some("move") => Self::Move,
            _ => Self::Ignore,
        }
    }

    pub(crate) fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Ignore => None,
            Self::Skip => Some("skip"),
            Self::Move => Some("move"),
        }
    }
}

/// What a recurring action or journal schedule does on holidays and on vacations.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct DaysOffUse {
    pub holidays: DayOffEffect,
    pub vacations: DayOffEffect,
}

impl DaysOffUse {
    pub fn effect(&self, kind: DaysOffKind) -> DayOffEffect {
        match kind {
            DaysOffKind::Holidays => self.holidays,
            DaysOffKind::Vacations => self.vacations,
        }
    }

    pub(crate) fn from_map(map: &loro::LoroMapValue) -> Self {
        let effect = |kind: DaysOffKind| {
            DayOffEffect::parse(map.get(kind.key()).and_then(|v| string(v.clone())))
        };
        Self { holidays: effect(DaysOffKind::Holidays), vacations: effect(DaysOffKind::Vacations) }
    }

    pub(crate) fn write(&self, map: &LoroMap) -> Result<(), Error> {
        for kind in DaysOffKind::ALL {
            match self.effect(kind).as_str() {
                Some(effect) => map.insert(kind.key(), effect),
                None => map.delete(kind.key()),
            }
            .map_err(loro_error)?;
        }
        Ok(())
    }
}

/// One schedule of days off, such as Christmas or a trip: a span of days from `first` to `last`,
/// which comes back on each date `rule` produces from `first`, if it has one.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct DaysOff {
    pub id: String,
    pub kind: DaysOffKind,
    pub name: String,
    pub first: Date,
    pub last: Date,
    pub rule: Option<Rule>,
    /// A rule this version can't read, such as one from a later version, kept as written so that
    /// saving the schedule doesn't lose it. Until a version reads it, the schedule covers only its
    /// first span. An editor that changes how it repeats clears it.
    pub unread_rule: Option<String>,
    /// The day its owner archived it, from which it no longer applies.
    pub archived: Option<Date>,
}

impl DaysOff {
    pub fn new(kind: DaysOffKind, name: &str, first: Date) -> Self {
        let id = NoteId::generate().to_string();
        Self {
            id,
            kind,
            name: name.trim().to_owned(),
            first,
            last: first,
            rule: None,
            unread_rule: None,
            archived: None,
        }
    }

    fn from_value(id: &str, value: &LoroValue) -> Option<Self> {
        let LoroValue::Map(map) = value else {
            return None;
        };
        let text = |key: &str| map.get(key).and_then(|v| string(v.clone()));
        let kind =
            DaysOffKind::ALL.into_iter().find(|kind| text(KIND).as_deref() == Some(kind.key()))?;
        let date = |key: &str| text(key).and_then(|t| t.parse::<Date>().ok());
        let first = date(FIRST)?;
        let last = date(LAST).filter(|last| *last >= first).unwrap_or(first);
        let written = text(RULE);
        let rule = written.as_deref().and_then(Rule::parse);
        let unread_rule = written.filter(|_| rule.is_none());
        Some(Self {
            id: id.to_owned(),
            kind,
            name: text(NAME).unwrap_or_default(),
            first,
            last,
            rule,
            unread_rule,
            archived: date(ARCHIVED),
        })
    }

    fn to_value(&self) -> LoroValue {
        let mut fields: Vec<(String, LoroValue)> = vec![
            (KIND.into(), self.kind.key().into()),
            (NAME.into(), self.name.trim().into()),
            (FIRST.into(), self.first.to_string().into()),
        ];
        if self.last > self.first {
            fields.push((LAST.into(), self.last.to_string().into()));
        }
        let rule = self.rule.as_ref().map(Rule::to_string).or_else(|| self.unread_rule.clone());
        if let Some(rule) = rule {
            fields.push((RULE.into(), rule.into()));
        }
        if let Some(archived) = self.archived {
            fields.push((ARCHIVED.into(), archived.to_string().into()));
        }
        LoroValue::Map(fields.into())
    }

    fn span(&self) -> i32 {
        self.last.days_since(self.first).max(0)
    }

    /// Whether `date` is one of these days off.
    pub fn covers(&self, date: Date) -> bool {
        self.span_at(date).is_some()
    }

    fn span_at(&self, date: Date) -> Option<(Date, Date)> {
        if self.archived.is_some_and(|archived| date >= archived) || date < self.first {
            return None;
        }
        let (first, last) = match &self.rule {
            None => (self.first, self.last),
            Some(rule)
                if rule.frequency == Frequency::Daily
                    && rule.interval <= self.span() as u32 + 1
                    && rule.weekdays.is_empty()
                    && rule.month_days.is_empty()
                    && rule.months.is_empty()
                    && rule.positions.is_empty() =>
            {
                let limit = Date::new(9998, 12, 31)?;
                let end = rule.until.unwrap_or(limit).min(limit);
                if end < self.first {
                    return None;
                }
                let index = (end.days_since(self.first) as u32 / rule.interval)
                    .min(rule.count.map_or(u32::MAX, |count| count - 1));
                let last = self.first.add_days((index * rule.interval) as i32);
                (self.first, last.add_days(self.span()))
            }
            Some(rule) => {
                let first = rule
                    .all_dates(self.first, date.add_days(-self.span()).max(self.first))
                    .take_while(|&start| start <= date)
                    .last()?;
                (first, first.add_days(self.span()))
            }
        };
        let last = self.archived.map_or(last, |archived| last.min(archived.add_days(-1)));
        (date <= last).then_some((first, last))
    }

    /// The last day off, or nothing if the schedule comes back forever.
    pub fn end(&self) -> Option<Date> {
        match &self.rule {
            None if self.unread_rule.is_some() => None,
            None => Some(self.last),
            Some(rule) if rule.until.is_none() && rule.count.is_none() => None,
            Some(rule) => {
                let last = rule.all_dates(self.first, self.first).last()?;
                Some(last.add_days(self.span()))
            }
        }
    }

    /// Whether the lists leave the schedule out, because its owner archived it or its last day
    /// passed a week ago. It still applies to the days it covers.
    pub fn is_hidden(&self, today: Date) -> bool {
        self.archived.is_some() || self.end().is_some_and(|end| end.add_days(HIDE_AFTER) <= today)
    }

    /// When the days off fall, such as "Dec 21, 2026 – Jan 2, 2027" or "Yearly on December 25".
    pub fn describe(&self) -> String {
        match &self.rule {
            Some(rule) => describe_rule(rule, self.first),
            None if self.unread_rule.is_some() => "Custom repeat".to_owned(),
            None if self.first == self.last => format_long_date(self.first),
            None => format!("{} – {}", format_long_date(self.first), format_long_date(self.last)),
        }
    }
}

/// A date as "Dec 21, 2026".
fn format_long_date(date: Date) -> String {
    let month = &MONTH_NAMES[usize::from(date.month() - 1)][..3];
    format!("{month} {}, {}", date.day(), date.year())
}

/// Every schedule of days off in a library, to test dates against.
#[derive(Default)]
pub(crate) struct DaysOffSet {
    schedules: Vec<DaysOff>,
}

impl DaysOffSet {
    fn is_off(&self, kind: DaysOffKind, date: Date) -> bool {
        self.schedules.iter().any(|days| days.kind == kind && days.covers(date))
    }

    /// Whether a rule that uses days off as `uses` says drops `date`.
    pub(crate) fn skips(&self, uses: &DaysOffUse, date: Date) -> bool {
        DaysOffKind::ALL
            .into_iter()
            .any(|kind| uses.effect(kind) == DayOffEffect::Skip && self.is_off(kind, date))
    }

    /// The date that `date` moves to under `uses`: the next day that isn't a day off of a kind the
    /// rule moves past or skips, if `date` falls on a day off it moves past, and `date` otherwise.
    pub(crate) fn moved(&self, uses: &DaysOffUse, date: Date) -> Date {
        let moves = DaysOffKind::ALL
            .into_iter()
            .any(|kind| uses.effect(kind) == DayOffEffect::Move && self.is_off(kind, date));
        if !moves {
            return date;
        }
        let mut day = date;
        while let Some(last) = self
            .schedules
            .iter()
            .filter(|days| uses.effect(days.kind) != DayOffEffect::Ignore)
            .filter_map(|days| days.span_at(day).map(|(_, last)| last))
            .max()
        {
            if last.year() >= 9999 {
                return date;
            }
            day = last.add_days(1);
        }
        day
    }

    /// The beginning of the uninterrupted days off immediately before `date`, or `date` itself.
    pub(crate) fn before(&self, uses: &DaysOffUse, date: Date) -> Date {
        let mut day = date;
        while let Some(first) = self
            .schedules
            .iter()
            .filter(|days| uses.effect(days.kind) != DayOffEffect::Ignore)
            .filter_map(|days| days.span_at(day.add_days(-1)).map(|(first, _)| first))
            .min()
        {
            day = first;
        }
        day
    }

    pub(crate) fn applies(uses: &DaysOffUse) -> bool {
        *uses != DaysOffUse::default()
    }
}

impl Library {
    /// Every schedule of days off, hidden or not, holidays first, each kind in order of first day.
    pub fn days_off(&self) -> Vec<DaysOff> {
        let LoroValue::Map(entries) = self.doc.get_map(DAYS_OFF).get_value() else {
            return Vec::new();
        };
        let mut all: Vec<DaysOff> =
            entries.iter().filter_map(|(id, value)| DaysOff::from_value(id, value)).collect();
        let kind = |days: &DaysOff| DaysOffKind::ALL.iter().position(|kind| *kind == days.kind);
        all.sort_by(|a, b| kind(a).cmp(&kind(b)).then(a.first.cmp(&b.first)).then(a.id.cmp(&b.id)));
        all
    }

    pub(crate) fn days_off_set(&self) -> DaysOffSet {
        DaysOffSet { schedules: self.days_off() }
    }

    /// Adds a schedule of days off, or replaces the one with the same identifier and kind.
    pub fn set_days_off(&mut self, days: &DaysOff) -> Result<(), Error> {
        self.check_writable()?;
        for note in self.notes() {
            self.preserve_occurrence_reaches(note.id)?;
        }
        // Each schedule sits in the root map, not a map per kind: two devices that each made the
        // first map of a kind would make two containers under one key, and sync keeps only one.
        self.doc.get_map(DAYS_OFF).insert(&days.id, days.to_value()).map_err(loro_error)?;
        self.commit()
    }

    /// Archives a schedule of days off from the day after `today`, so it still covers today, or
    /// takes it out of the archive. Schedules are never deleted, so the days off that shaped past
    /// dates keep shaping them.
    pub fn archive_days_off(
        &mut self,
        kind: DaysOffKind,
        id: &str,
        archived: bool,
        today: Date,
    ) -> Result<(), Error> {
        let Some(mut days) = self.days_off().into_iter().find(|d| d.kind == kind && d.id == id)
        else {
            return Ok(());
        };
        days.archived = archived.then(|| today.add_days(1));
        self.set_days_off(&days)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn covers_spans_that_come_back() {
        let mut trip = DaysOff::new(DaysOffKind::Vacations, "Beach trip", date("2026-12-21"));
        trip.last = date("2027-01-02");
        assert!(trip.covers(date("2026-12-21")));
        assert!(trip.covers(date("2027-01-02")));
        assert!(!trip.covers(date("2027-01-03")));
        assert_eq!(trip.describe(), "Dec 21, 2026 – Jan 2, 2027");
        assert!(!trip.is_hidden(date("2027-01-08")));
        assert!(trip.is_hidden(date("2027-01-09")));

        let mut thanksgiving =
            DaysOff::new(DaysOffKind::Holidays, "Thanksgiving", date("2026-11-26"));
        thanksgiving.last = date("2026-11-27");
        thanksgiving.rule = Rule::parse("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH");
        assert!(thanksgiving.covers(date("2027-11-25")));
        assert!(thanksgiving.covers(date("2027-11-26")));
        assert!(!thanksgiving.covers(date("2027-11-27")));
        assert_eq!(thanksgiving.end(), None);
        assert!(!thanksgiving.is_hidden(date("2040-01-01")));
        thanksgiving.archived = Some(date("2027-01-01"));
        assert!(thanksgiving.covers(date("2026-11-26")));
        assert!(!thanksgiving.covers(date("2027-11-25")));
        assert!(thanksgiving.is_hidden(date("2027-01-01")));
    }

    #[test]
    fn touching_daily_spans_respect_count_until_and_archive_boundaries() {
        let mut days = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-01-01"));
        days.last = date("2026-01-03");
        for rule in ["FREQ=DAILY;INTERVAL=3;COUNT=2", "FREQ=DAILY;INTERVAL=3;UNTIL=20260105"] {
            days.rule = Rule::parse(rule);
            assert!(days.covers(date("2026-01-01")));
            assert!(days.covers(date("2026-01-06")));
            assert!(!days.covers(date("2026-01-07")));
            assert_eq!(days.end(), Some(date("2026-01-06")));
            days.archived = Some(date("2026-01-05"));
            assert!(days.covers(date("2026-01-04")));
            assert!(!days.covers(date("2026-01-05")));
            days.archived = None;
        }
    }

    #[test]
    fn skips_and_moves_past_days_off() {
        let mut library = Library::in_memory();
        let mut christmas = DaysOff::new(DaysOffKind::Holidays, "Christmas", date("2026-12-25"));
        christmas.rule = Rule::parse("FREQ=YEARLY");
        library.set_days_off(&christmas).unwrap();
        let mut trip = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-12-26"));
        trip.last = date("2026-12-27");
        library.set_days_off(&trip).unwrap();
        assert_eq!(library.days_off().len(), 2);
        let set = library.days_off_set();
        let both = DaysOffUse { holidays: DayOffEffect::Move, vacations: DayOffEffect::Skip };
        assert!(set.skips(&both, date("2026-12-26")));
        assert!(!set.skips(&both, date("2026-12-25")));
        assert_eq!(set.moved(&both, date("2026-12-25")), date("2026-12-28"));
        let holidays = DaysOffUse { holidays: DayOffEffect::Move, ..DaysOffUse::default() };
        assert_eq!(set.moved(&holidays, date("2026-12-25")), date("2026-12-26"));
        assert_eq!(set.moved(&DaysOffUse::default(), date("2026-12-25")), date("2026-12-25"));
        library
            .archive_days_off(DaysOffKind::Holidays, &christmas.id, true, date("2027-01-01"))
            .unwrap();
        let set = library.days_off_set();
        assert_eq!(set.moved(&holidays, date("2027-12-25")), date("2027-12-25"));
        assert_eq!(set.moved(&holidays, date("2026-12-25")), date("2026-12-26"));
    }

    #[test]
    fn first_schedules_added_on_two_devices_both_survive_sync() {
        let mut laptop = Library::in_memory();
        let mut phone = Library::in_memory();
        phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
        let christmas = DaysOff::new(DaysOffKind::Holidays, "Christmas", date("2026-12-25"));
        laptop.set_days_off(&christmas).unwrap();
        let thanksgiving = DaysOff::new(DaysOffKind::Holidays, "Thanksgiving", date("2026-11-26"));
        phone.set_days_off(&thanksgiving).unwrap();
        let updates = phone.updates_since(&laptop.version()).unwrap().unwrap();
        laptop.merge_updates(&updates, false).unwrap();
        let names: Vec<String> = laptop.days_off().into_iter().map(|days| days.name).collect();
        assert_eq!(names, ["Thanksgiving", "Christmas"]);
    }

    #[test]
    fn archiving_keeps_today_and_unread_rules_survive_saves() {
        let mut library = Library::in_memory();
        let mut christmas = DaysOff::new(DaysOffKind::Holidays, "Christmas", date("2026-12-25"));
        christmas.rule = Rule::parse("FREQ=YEARLY");
        library.set_days_off(&christmas).unwrap();
        library
            .archive_days_off(DaysOffKind::Holidays, &christmas.id, true, date("2026-12-25"))
            .unwrap();
        let archived = library.days_off().remove(0);
        assert!(archived.covers(date("2026-12-25")));
        assert!(!archived.covers(date("2027-12-25")));

        let mut later = DaysOff::new(DaysOffKind::Holidays, "Later", date("2026-12-31"));
        later.unread_rule = Some("FREQ=YEARLY;BYWEEKNO=1".to_owned());
        library.set_days_off(&later).unwrap();
        let read = library.days_off().into_iter().find(|days| days.id == later.id).unwrap();
        assert_eq!(read.unread_rule, later.unread_rule);
        assert_eq!(read.rule, None);
        assert!(read.covers(date("2026-12-31")));
        assert_eq!(read.describe(), "Custom repeat");
        assert!(!read.is_hidden(date("2030-01-01")));
        library
            .archive_days_off(DaysOffKind::Holidays, &later.id, true, date("2027-01-01"))
            .unwrap();
        let read = library.days_off().into_iter().find(|days| days.id == later.id).unwrap();
        assert_eq!(read.unread_rule, later.unread_rule);
    }
}
