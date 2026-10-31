// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Recurring actions: one action whose rule produces occurrences, with a record of the occurrences
//! its owner handled, as decision 0006 and the library format spec define them.

use std::cell::OnceCell;
use std::collections::HashMap;

use loro::{LoroMap, LoroMapValue, LoroValue, ValueOrContainer};

use crate::actions::{ACTION, Action, ActionState, CLOSED, Date, Hierarchy, STATE as ACTION_STATE};
use crate::days_off::{DaysOffSet, DaysOffUse};
use crate::library::{MODIFIED, integer, loro_error, string};
use crate::recur::{End, Rule};
use crate::{Error, Library, Note, NoteId, wikilink};

pub(crate) const RECUR: &str = "recur";
pub(crate) const OCCURRENCES: &str = "occurrences";
const AT: &str = "at";
const DAY: &str = "day";
const DEFER: &str = "defer";
const DUE: &str = "due";
const EARLY: &str = "early";
const HANDLED: &str = "handled";
const MISSED: &str = "missed";
const MOVED: &str = "moved";
const NOTES: &str = "notes";
const RULE: &str = "rule";
const START: &str = "start";
const STATE: &str = "state";

/// How many waiting or missed occurrences a count reaches before it stops.
const MAX_COUNT: u32 = 99;

/// What happens to occurrences their owner didn't handle in time.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Missed {
    /// Every occurrence counts, so missed ones wait their turn, as bills do.
    All,
    /// Only the latest occurrence that has come into view counts, as a weekly review does.
    #[default]
    Latest,
    /// The rule starts again from the day its owner handled the last occurrence, as watering a
    /// plant does.
    After,
}

impl Missed {
    fn parse(text: Option<String>) -> Self {
        match text.as_deref() {
            Some("all") => Self::All,
            Some("after") => Self::After,
            _ => Self::Latest,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Latest => "latest",
            Self::After => "after",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum OccurrenceState {
    Done,
    Skipped,
}

/// An occurrence its owner closed or skipped.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct HandledOccurrence {
    /// The date the rule gave the occurrence, which names it.
    pub date: Date,
    pub state: OccurrenceState,
    pub at: i64,
    /// The local day its owner handled it.
    pub day: Date,
    /// Whether its owner handled it before its start date, which leaves the unhandled occurrences
    /// before it waiting instead of missed.
    pub early: bool,
}

/// An occurrence its owner moved to another date.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct MovedOccurrence {
    pub date: Date,
    pub to: Date,
}

/// A note written for one occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceNote {
    pub date: Date,
    pub note: NoteId,
}

/// A step's completion in one occurrence of the recurring action it belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct StepOccurrence {
    pub date: Date,
    pub at: i64,
}

/// The parts of a recurring action its owner sets.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct RecurrenceSettings {
    pub rule: Rule,
    /// The date the rule counts from.
    pub start: Date,
    pub missed: Missed,
    /// The due date's offset in days from each occurrence's date, if occurrences have one.
    pub due: Option<i32>,
    /// The start date's offset in days from each occurrence's date. Without one, each occurrence
    /// starts on its date, so it never shows before its date.
    pub defer: Option<i32>,
    pub days_off: DaysOffUse,
}

impl RecurrenceSettings {
    /// Settings for `rule` from `start`, starting and due on each occurrence's date, keeping only
    /// the latest missed occurrence, and ignoring days off.
    pub fn new(rule: Rule, start: Date) -> Self {
        Self {
            rule,
            start,
            missed: Missed::default(),
            due: Some(0),
            defer: Some(0),
            days_off: DaysOffUse::default(),
        }
    }
}

/// A recurring action's fields.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Recurrence {
    pub settings: RecurrenceSettings,
    pub handled: Vec<HandledOccurrence>,
    pub moved: Vec<MovedOccurrence>,
    pub notes: Vec<OccurrenceNote>,
}

impl Recurrence {
    pub(crate) fn from_value(value: Option<&LoroValue>) -> Option<Self> {
        let Some(LoroValue::Map(map)) = value else {
            return None;
        };
        let text = |key: &str| map.get(key).and_then(|v| string(v.clone()));
        let offset = |key: &str| {
            map.get(key).and_then(|v| integer(v.clone())).and_then(|n| i32::try_from(n).ok())
        };
        let settings = RecurrenceSettings {
            rule: Rule::parse(&text(RULE)?)?,
            start: text(START)?.parse().ok()?,
            missed: Missed::parse(text(MISSED)),
            due: offset(DUE),
            defer: offset(DEFER),
            days_off: DaysOffUse::from_map(map),
        };
        let handled = entries(map, HANDLED, |date, value| {
            let LoroValue::Map(entry) = value else { return None };
            let state = match entry.get(STATE).and_then(|v| string(v.clone())).as_deref() {
                Some("done") => OccurrenceState::Done,
                Some("skipped") => OccurrenceState::Skipped,
                _ => return None,
            };
            let at = entry.get(AT).and_then(|v| integer(v.clone())).unwrap_or(0);
            let day = entry
                .get(DAY)
                .and_then(|v| string(v.clone()))
                .and_then(|t| t.parse().ok())
                .unwrap_or(date);
            let early = matches!(entry.get(EARLY), Some(LoroValue::Bool(true)));
            Some(HandledOccurrence { date, state, at, day, early })
        });
        let moved = entries(map, MOVED, |date, value| {
            let to = string(value.clone())?.parse().ok()?;
            Some(MovedOccurrence { date, to })
        });
        let notes = entries(map, NOTES, |date, value| {
            let note = string(value.clone())?.parse().ok()?;
            Some(OccurrenceNote { date, note })
        });
        Some(Self { settings, handled, moved, notes })
    }

    fn latest_handled(&self) -> Option<&HandledOccurrence> {
        self.handled.iter().max_by_key(|entry| entry.date)
    }
}

/// The entries of a map in `map` under `key`, keyed by date, in order of date.
fn entries<T>(
    map: &LoroMapValue,
    key: &str,
    read: impl Fn(Date, &LoroValue) -> Option<T>,
) -> Vec<T> {
    let Some(LoroValue::Map(entries)) = map.get(key) else {
        return Vec::new();
    };
    let mut found: Vec<(Date, T)> = entries
        .iter()
        .filter_map(|(date, value)| {
            let date: Date = date.parse().ok()?;
            Some((date, read(date, value)?))
        })
        .collect();
    found.sort_by_key(|(date, _)| *date);
    found.into_iter().map(|(_, entry)| entry).collect()
}

pub(crate) fn step_occurrences(value: Option<&LoroValue>) -> Vec<StepOccurrence> {
    let Some(LoroValue::Map(map)) = value else {
        return Vec::new();
    };
    let mut found: Vec<StepOccurrence> = map
        .iter()
        .filter_map(|(date, at)| {
            Some(StepOccurrence { date: date.parse().ok()?, at: integer(at.clone())? })
        })
        .collect();
    found.sort_by_key(|occurrence| occurrence.date);
    found
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum OccurrenceStatus {
    /// The occurrence the action stands for now.
    Current,
    Upcoming,
    Done,
    Skipped,
    /// Unhandled, and no longer counting.
    Missed,
}

/// One occurrence of a recurring action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Occurrence {
    /// The date the rule gave the occurrence, which names it.
    pub date: Date,
    /// The date it falls on, after its owner or days off moved it.
    pub on: Date,
    pub defer: Option<Date>,
    pub due: Option<Date>,
    pub status: OccurrenceStatus,
    pub handled: Option<HandledOccurrence>,
    /// Whether its owner moved it.
    pub moved: bool,
    pub note: Option<NoteId>,
}

/// Works out a recurring action's occurrences as they stand on one day.
pub(crate) struct Occurrences<'a> {
    recur: &'a Recurrence,
    days_off: &'a DaysOffSet,
    today: Date,
    moved: HashMap<Date, Date>,
    handled: HashMap<Date, HandledOccurrence>,
    notes: HashMap<Date, NoteId>,
    /// Where a rule with a `count` ends, when days off can skip some of its dates.
    end: Option<End>,
    cutoff: OnceCell<Cutoff>,
}

/// Which unhandled occurrences count as missed.
struct Cutoff {
    /// Every unhandled occurrence before it.
    from: Date,
    /// For each counted handled occurrence, in order of date, its date and the latest reach among
    /// it and those after it: an unhandled occurrence before it that came into view by then.
    reaches: Vec<(Date, Date)>,
}

impl<'a> Occurrences<'a> {
    pub(crate) fn new(recur: &'a Recurrence, days_off: &'a DaysOffSet, today: Date) -> Self {
        let settings = &recur.settings;
        Self {
            recur,
            days_off,
            today,
            moved: recur.moved.iter().map(|moved| (moved.date, moved.to)).collect(),
            handled: recur.handled.iter().map(|entry| (entry.date, *entry)).collect(),
            notes: recur.notes.iter().map(|note| (note.date, note.note)).collect(),
            end: DaysOffSet::applies(&settings.days_off)
                .then(|| settings.rule.end(settings.start))
                .flatten(),
            cutoff: OnceCell::new(),
        }
    }

    fn settings(&self) -> &RecurrenceSettings {
        &self.recur.settings
    }

    /// The dates the rule gives occurrences on or after `from`.
    fn dates(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        let settings = self.settings();
        let uses = settings.days_off;
        let days_off = self.days_off;
        if DaysOffSet::applies(&uses) {
            let skip = Box::new(move |date| days_off.skips(&uses, date));
            settings.rule.iterate(settings.start, from, skip, self.end)
        } else {
            settings.rule.all_dates(settings.start, from)
        }
    }

    fn occurrence(&self, date: Date, status: OccurrenceStatus) -> Occurrence {
        let settings = self.settings();
        let moved = self.moved.get(&date).copied();
        let on = self.on(date);
        let handled = self.handled.get(&date).copied();
        let status = match handled.map(|entry| entry.state) {
            Some(OccurrenceState::Done) => OccurrenceStatus::Done,
            Some(OccurrenceState::Skipped) => OccurrenceStatus::Skipped,
            None => status,
        };
        Occurrence {
            date,
            on,
            defer: Some(on.add_days(settings.defer.unwrap_or(0))),
            due: settings.due.map(|days| on.add_days(days)),
            status,
            handled,
            moved: moved.is_some(),
            note: self.notes.get(&date).copied(),
        }
    }

    /// The date the occurrence named by `date` falls on, after its owner or days off moved it.
    fn on(&self, date: Date) -> Date {
        let uses = &self.settings().days_off;
        self.moved.get(&date).copied().unwrap_or_else(|| {
            if DaysOffSet::applies(uses) { self.days_off.moved(uses, date) } else { date }
        })
    }

    /// The day an occurrence comes into view: its start date, or the date it falls on.
    fn view(occurrence: &Occurrence) -> Date {
        occurrence.defer.unwrap_or(occurrence.on)
    }

    /// Whether an occurrence named by `date` and handled on `day` is handled before its start date.
    fn early(&self, date: Date, day: Date) -> bool {
        day < self.started(date)
    }

    fn started(&self, date: Date) -> Date {
        Self::view(&self.occurrence(date, OccurrenceStatus::Upcoming))
    }

    /// The day by which an occurrence must have come into view for `entry` to miss it, if `entry`
    /// misses any. Handling an occurrence misses the unhandled ones before it that had come into
    /// view by the day its owner handled it. One handled early misses nothing under `all`, and
    /// under `latest` misses those in view on the day it started, once that day has come.
    fn reach(&self, entry: &HandledOccurrence) -> Option<Date> {
        if !entry.early {
            return Some(entry.day);
        }
        let started = self.started(entry.date);
        (self.settings().missed != Missed::All && started <= self.today).then_some(started)
    }

    fn cutoff(&self) -> &Cutoff {
        self.cutoff.get_or_init(|| {
            let settings = self.settings();
            let mut reaches: Vec<(Date, Date)> = self
                .recur
                .handled
                .iter()
                .filter_map(|entry| Some((entry.date, self.reach(entry)?)))
                .collect();
            for i in (0..reaches.len().saturating_sub(1)).rev() {
                reaches[i].1 = reaches[i].1.max(reaches[i + 1].1);
            }
            let mut from = settings.start;
            for entry in self.recur.handled.iter().rev() {
                if entry.date.add_days(1) <= from {
                    break;
                }
                let Some(reach) = self.reach(entry) else { continue };
                let unseen = |date: Date| {
                    date < entry.date
                        && !self.handled.contains_key(&date)
                        && self.started(date) > reach
                };
                // Dates this far before `reach` came into view by then, unless days off moved them
                // more than a month, so only the occurrences their owner moved need checking.
                let walk = reach.add_days(-settings.defer.unwrap_or(0) - 31).max(settings.start);
                let walked =
                    self.dates(walk).take_while(|&date| date < entry.date).find(|&d| unseen(d));
                let moved = self.moved.keys().copied().filter(|&date| {
                    date >= settings.start && date < walk && unseen(date) && self.gives(date)
                });
                let after = walked.into_iter().chain(moved).min().unwrap_or(entry.date.add_days(1));
                from = from.max(after);
            }
            Cutoff { from, reaches }
        })
    }

    /// Whether the unhandled occurrence named by `date` counts as missed because a later handled
    /// occurrence reached it.
    fn is_missed(&self, date: Date) -> bool {
        let cutoff = self.cutoff();
        if date < cutoff.from {
            return true;
        }
        let later = cutoff.reaches.partition_point(|&(handled, _)| handled <= date);
        cutoff.reaches.get(later).is_some_and(|&(_, reach)| self.started(date) <= reach)
    }

    /// Whether the unhandled occurrence named by `date` is past when `current` stands: missed, or,
    /// under `latest`, superseded by the later `current`, once it has started.
    fn is_past(&self, date: Date, current: Option<&Occurrence>) -> bool {
        self.is_missed(date)
            || (self.settings().missed == Missed::Latest
                && current.is_none_or(|current| date < current.date)
                && self.started(date) <= self.today)
    }

    /// The unhandled occurrences on or after `from` that don't count as missed.
    fn open(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        let from = from.max(self.cutoff().from);
        self.unhandled(from).filter(move |&date| !self.is_missed(date))
    }

    /// Whether the rule gives an occurrence named by `date`.
    fn gives(&self, date: Date) -> bool {
        self.dates(date).next() == Some(date)
    }

    fn unhandled(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        self.dates(from).filter(|date| !self.handled.contains_key(date))
    }

    /// The open occurrences after `by` that their owner moved, which a walk of the rule's dates
    /// that stops at `by` doesn't reach, however early they now fall.
    fn moved_beyond(&self, by: Date) -> impl Iterator<Item = Date> + '_ {
        self.moved.keys().copied().filter(move |&date| {
            date > by
                && date >= self.cutoff().from
                && !self.handled.contains_key(&date)
                && !self.is_missed(date)
                && self.gives(date)
        })
    }

    /// The open occurrence, from `first` on, that starts soonest, the earliest by date among ties.
    fn soonest(&self, first: Date) -> Option<Date> {
        let defer = self.settings().defer.unwrap_or(0);
        let mut soonest: Option<Date> = None;
        let mut walked = Vec::new();
        for date in self.open(first) {
            // Days off only move occurrences later, so no later date can start sooner, unless its
            // owner moved it, which `moved_beyond` finds.
            if soonest.is_some_and(|soonest| date.add_days(defer) > soonest) {
                break;
            }
            let started = self.started(date);
            soonest = Some(soonest.map_or(started, |soonest| soonest.min(started)));
            walked.push((started, date));
        }
        let last = walked.last().map_or(first, |&(_, date)| date);
        let moved = self.moved_beyond(last).map(|date| (self.started(date), date));
        walked.into_iter().chain(moved).min().map(|(_, date)| date)
    }

    /// The latest open occurrence, from `first` on, that has started.
    fn latest_started(&self, first: Date) -> Option<Date> {
        let today = self.today;
        // Days off only move occurrences later, so none after this has started, unless its owner
        // moved it, which `moved_beyond` finds.
        let last = today.add_days(-self.settings().defer.unwrap_or(0));
        let scan = |from: Date| {
            self.open(from).take_while(|&date| date <= last).filter(|&d| self.started(d) <= today)
        };
        // Most often one has started in the last month, so look there before the whole backlog.
        let window = first.max(last.add_days(-31));
        let walked =
            scan(window).last().or_else(|| (window > first).then(|| scan(first).last())?);
        let moved = self.moved_beyond(last).filter(|&date| self.started(date) <= today).max();
        walked.max(moved)
    }

    /// The occurrence the action stands for, or nothing once the rule has ended: under `all`, the
    /// open one that starts first, and under `latest`, the latest open one that has started, or
    /// else the one that starts soonest. Neither is the first by date when its owner postponed
    /// that one.
    pub(crate) fn current(&self) -> Option<Occurrence> {
        let settings = self.settings();
        let date = match settings.missed {
            Missed::After => self.next_after(self.recur.latest_handled(), 0)?,
            Missed::All | Missed::Latest => {
                let first = self.open(settings.start).next()?;
                let latest = (settings.missed == Missed::Latest)
                    .then(|| self.latest_started(first))
                    .flatten();
                latest.or_else(|| self.soonest(first))?
            }
        };
        Some(self.occurrence(date, OccurrenceStatus::Current))
    }

    /// For the `after` behavior, the occurrence after `latest`: the rule's first date after the
    /// day its owner handled it, counting from that day, or the rule's first date if nothing is
    /// handled yet. `projected` is how many occurrences a preview assumes handled beyond the
    /// record, which count toward the rule's `COUNT`.
    fn next_after(&self, latest: Option<&HandledOccurrence>, projected: usize) -> Option<Date> {
        let settings = self.settings();
        let mut rule = settings.rule.clone();
        if let Some(count) = rule.count.take()
            && self.recur.handled.len() + projected >= count as usize
        {
            return None;
        }
        let uses = settings.days_off;
        let skips = DaysOffSet::applies(&uses);
        let days_off = self.days_off;
        let skip = move |date| skips && days_off.skips(&uses, date);
        match latest {
            Some(entry) => {
                let day = entry.day.max(settings.start);
                rule.dates(day, day.add_days(1), skip).find(|date| !self.handled.contains_key(date))
            }
            None => rule.dates(settings.start, settings.start, skip).next(),
        }
    }

    /// How many open occurrences besides `current` have already fallen due, for the `all`
    /// behavior, in which they wait their turn.
    pub(crate) fn waiting(&self, current: &Occurrence) -> u32 {
        if self.settings().missed != Missed::All {
            return 0;
        }
        let today = self.today;
        let mut count = self
            .moved_beyond(today)
            .filter(|&date| date != current.date && self.on(date) <= today)
            .count() as u32;
        for date in self.open(self.settings().start) {
            if date > today || count >= MAX_COUNT {
                break;
            }
            if date != current.date && self.on(date) <= today {
                count += 1;
            }
        }
        count.min(MAX_COUNT)
    }

    /// How many unhandled occurrences after the cutoff count as missed: those before `current`
    /// that are past, or, once the rule has ended, those up to today.
    fn missed(&self, current: Option<&Occurrence>) -> u32 {
        if self.settings().missed == Missed::After {
            return 0;
        }
        let end = current.map_or(self.today.add_days(1), |current| current.date);
        self.unhandled(self.cutoff().from)
            .take_while(|date| *date < end)
            .filter(|&date| self.started(date) <= self.today && self.is_past(date, current))
            .take(MAX_COUNT as usize)
            .count() as u32
    }

    /// The latest `past` unhandled occurrences on or before `end` that are past when `current`
    /// stands, latest first.
    fn latest_past(&self, end: Date, current: Option<&Occurrence>, past: usize) -> Vec<Date> {
        let start = self.settings().start;
        // Walks back in growing spans, so a rule that started long ago doesn't walk every date.
        let mut span: i32 = 64;
        loop {
            let from = if end.days() - start.days() <= i64::from(span) {
                start
            } else {
                end.add_days(-span)
            };
            let mut found: Vec<Date> = self
                .unhandled(from)
                .take_while(|date| *date <= end)
                .filter(|&date| self.started(date) <= self.today && self.is_past(date, current))
                .collect();
            if found.len() >= past || from <= start {
                found.reverse();
                found.truncate(past);
                return found;
            }
            span *= 4;
        }
    }

    /// The current occurrence, then `upcoming` more after it, any handled ahead of time among them,
    /// and any still open before it, in the order they fall, then the latest `past` occurrences
    /// before it, latest first.
    pub(crate) fn history(&self, past: usize, upcoming: usize) -> Vec<Occurrence> {
        let settings = self.settings();
        let today = self.today;
        let current = self.current();
        let mut earlier: Vec<Occurrence> = self
            .recur
            .handled
            .iter()
            .filter(|entry| current.is_none_or(|current| entry.date < current.date))
            .map(|entry| self.occurrence(entry.date, OccurrenceStatus::Missed))
            .collect();
        if settings.missed != Missed::After {
            let end = current.map_or(today, |current| current.date.add_days(-1)).min(today);
            let missed = self.latest_past(end, current.as_ref(), past);
            earlier.extend(
                missed.into_iter().map(|date| self.occurrence(date, OccurrenceStatus::Missed)),
            );
        }
        earlier.sort_by_key(|occurrence| std::cmp::Reverse(occurrence.date));
        earlier.truncate(past);
        let mut later = Vec::new();
        if let Some(current) = current {
            later.push(current);
            match settings.missed {
                Missed::After => {
                    // Each occurrence counts from the day its owner handles it, so the preview
                    // assumes each is handled on its date, or today if that has passed.
                    let mut last = current;
                    for projected in 1..=upcoming {
                        let entry = HandledOccurrence {
                            date: last.date,
                            state: OccurrenceState::Done,
                            at: 0,
                            day: last.on.max(today),
                            early: false,
                        };
                        let Some(date) = self.next_after(Some(&entry), projected) else { break };
                        last = self.occurrence(date, OccurrenceStatus::Upcoming);
                        later.push(last);
                    }
                }
                _ => later.extend(
                    self.open(current.date.add_days(1))
                        .take(upcoming)
                        .map(|date| self.occurrence(date, OccurrenceStatus::Upcoming)),
                ),
            }
            let last = (later.len() > upcoming).then(|| later[later.len() - 1].date);
            later.extend(
                self.recur
                    .handled
                    .iter()
                    .filter(|entry| entry.date > current.date)
                    .filter(|entry| last.is_none_or(|last| entry.date < last))
                    .map(|entry| self.occurrence(entry.date, OccurrenceStatus::Upcoming)),
            );
            if settings.missed != Missed::After {
                // Occurrences before the current one by date that aren't past, such as postponed
                // ones, can still become current.
                // Days off move an occurrence at most a year, so one dated earlier than that can
                // only start after the current one if its owner moved it.
                let from = self.cutoff().from;
                let window = from.max(current.date.add_days(-366));
                let walked = self.unhandled(window).take_while(|date| *date < current.date);
                let moved = self.moved.keys().copied().filter(|&date| {
                    date >= from
                        && date < window
                        && !self.handled.contains_key(&date)
                        && self.gives(date)
                });
                let open = walked.chain(moved).filter(|&date| !self.is_past(date, Some(&current)));
                later.extend(open.map(|date| self.occurrence(date, OccurrenceStatus::Upcoming)));
            }
            later.sort_by_key(|occurrence| {
                (occurrence.status != OccurrenceStatus::Current, occurrence.on, occurrence.date)
            });
        }
        later.extend(earlier);
        later
    }
}

/// What a recurring action stands for on a day: its current occurrence, if the rule hasn't ended,
/// and how many more occurrences wait behind it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Standing {
    pub(crate) current: Option<Occurrence>,
    pub(crate) waiting: u32,
}

/// Where a recurring action stands on a day, for its details: its current occurrence, if the rule
/// hasn't ended, how many occurrences since the latest handled one it missed, and how many more
/// wait behind the current one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceSummary {
    pub current: Option<Occurrence>,
    pub missed: u32,
    pub waiting: u32,
}

impl Library {
    /// Makes an action recur with `settings`, or, with nothing, stops it recurring.
    ///
    /// An action that starts recurring while done counts its first occurrence as done, and its
    /// done steps as done in that occurrence only. An action that stops recurring keeps the dates
    /// of its current occurrence as its own, and its steps' states in it, or, once its rule has
    /// ended, counts as done.
    pub fn set_recurrence(
        &mut self,
        id: NoteId,
        settings: Option<RecurrenceSettings>,
        today: Date,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        let action = self.action_map(id)?;
        let before = self.note(id).and_then(|note| note.action).ok_or(Error::NotAnAction(id))?;
        let steps: Vec<(LoroMap, Action)> = self
            .hierarchy()
            .descendants(id)
            .into_iter()
            .filter_map(|step| Some((self.action_map(step).ok()?, self.note(step)?.action?)))
            .collect();
        match settings {
            Some(settings) => {
                if !settings.rule.is_valid() {
                    return Err(Error::InvalidRule);
                }
                let recur = child_map(&action, RECUR)?;
                // Two devices that each create one of these maps for their first entry would
                // each make a container under the same key, and sync keeps only one.
                for key in [HANDLED, MOVED, NOTES] {
                    child_map(&recur, key)?;
                }
                for (step, _) in &steps {
                    child_map(step, OCCURRENCES)?;
                }
                recur.insert(RULE, settings.rule.to_string()).map_err(loro_error)?;
                recur.insert(START, settings.start.to_string()).map_err(loro_error)?;
                recur.insert(MISSED, settings.missed.as_str()).map_err(loro_error)?;
                for (key, offset) in [(DUE, settings.due), (DEFER, settings.defer)] {
                    match offset {
                        Some(days) => recur.insert(key, i64::from(days)),
                        None => recur.delete(key),
                    }
                    .map_err(loro_error)?;
                }
                settings.days_off.write(&recur)?;
                if before.recur.is_none()
                    && let Some(current) = self.current_occurrence(id, today)
                {
                    let key = current.date.to_string();
                    for (step, fields) in &steps {
                        if fields.state == ActionState::Done {
                            let at = fields.closed.unwrap_or_else(|| self.now());
                            child_map(step, OCCURRENCES)?.insert(&key, at).map_err(loro_error)?;
                            set_state(step, None)?;
                        }
                    }
                    if before.state == ActionState::Done {
                        let at = before.closed.unwrap_or_else(|| self.now());
                        let entry =
                            self.handled_entry(id, current.date, OccurrenceState::Done, at, today);
                        child_map(&recur, HANDLED)?.insert(&key, entry).map_err(loro_error)?;
                        set_state(&action, None)?;
                    }
                }
            }
            None => {
                match self.current_occurrence(id, today) {
                    Some(current) => {
                        for (key, date) in [(DUE, current.due), (DEFER, current.defer)] {
                            match date {
                                Some(date) => action.insert(key, date.to_string()),
                                None => action.delete(key),
                            }
                            .map_err(loro_error)?;
                        }
                        for (step, fields) in &steps {
                            let done = fields.occurrences.iter().find(|o| o.date == current.date);
                            if fields.state == ActionState::Open
                                && let Some(done) = done
                            {
                                set_state(step, Some(done.at))?;
                            }
                        }
                    }
                    None if before.state == ActionState::Open => {
                        let last = before
                            .recur
                            .as_ref()
                            .and_then(|recur| recur.handled.iter().map(|entry| entry.at).max());
                        set_state(&action, Some(last.unwrap_or_else(|| self.now())))?;
                    }
                    None => {}
                }
                action.delete(RECUR).map_err(loro_error)?;
            }
        }
        note.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// Closes or skips the occurrence named by `date`, or, with nothing, reopens it. `today` is the
    /// local day, which an action that starts again from its last completion counts from.
    pub fn handle_occurrence(
        &mut self,
        id: NoteId,
        date: Date,
        state: Option<OccurrenceState>,
        today: Date,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let handled = child_map(&self.recur_map(id)?, HANDLED)?;
        match state {
            Some(state) => {
                let entry = self.handled_entry(id, date, state, self.now(), today);
                handled.insert(&date.to_string(), entry)
            }
            None => handled.delete(&date.to_string()),
        }
        .map_err(loro_error)?;
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// The record of the recurring action `id`'s occurrence named by `date`, handled as `state` at
    /// `at` on `today`.
    fn handled_entry(
        &self,
        id: NoteId,
        date: Date,
        state: OccurrenceState,
        at: i64,
        today: Date,
    ) -> LoroValue {
        let state = match state {
            OccurrenceState::Done => "done",
            OccurrenceState::Skipped => "skipped",
        };
        let mut entry: Vec<(String, LoroValue)> = vec![
            (STATE.into(), state.into()),
            (AT.into(), at.into()),
            (DAY.into(), today.to_string().into()),
        ];
        // Recorded now, so later changes to the start offset or days off don't turn a timely
        // completion into an early one and bring back occurrences already missed.
        let recur = self.note(id).and_then(|note| note.action?.recur);
        let days_off = self.days_off_set();
        if recur.is_some_and(|recur| Occurrences::new(&recur, &days_off, today).early(date, today))
        {
            entry.push((EARLY.into(), true.into()));
        }
        LoroValue::Map(entry.into())
    }

    /// Moves the occurrence named by `date` to `to`, or, with nothing, back to its own date.
    pub fn move_occurrence(
        &mut self,
        id: NoteId,
        date: Date,
        to: Option<Date>,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let moved = child_map(&self.recur_map(id)?, MOVED)?;
        match to {
            Some(to) if to != date => moved.insert(&date.to_string(), to.to_string()),
            _ => moved.delete(&date.to_string()),
        }
        .map_err(loro_error)?;
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// Writes a memo for the occurrence named by `date`, titled with the action and the date and
    /// linking to the action, records it, and returns its identifier.
    pub fn write_occurrence_note(&mut self, id: NoteId, date: Date) -> Result<NoteId, Error> {
        self.check_writable()?;
        let recur = self.recur_map(id)?;
        let title = self.note(id).and_then(|note| note.label()).unwrap_or_default();
        let link = wikilink(id, (!title.is_empty()).then_some(title.as_str()));
        let body = format!("{title} on {date}\n\n{link}\n");
        let memo = self.insert_note(body.trim_start(), crate::MEMO)?;
        child_map(&recur, NOTES)?
            .insert(&date.to_string(), memo.to_string())
            .map_err(loro_error)?;
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()?;
        Ok(memo)
    }

    /// The occurrence a recurring action stands for on `today`, or nothing if it doesn't recur or
    /// its rule has ended.
    pub fn current_occurrence(&self, id: NoteId, today: Date) -> Option<Occurrence> {
        let recur = self.note(id)?.action?.recur?;
        let days_off = self.days_off_set();
        Occurrences::new(&recur, &days_off, today).current()
    }

    /// Where a recurring action stands on `today`, or nothing if it doesn't recur.
    pub fn occurrence_summary(&self, id: NoteId, today: Date) -> Option<OccurrenceSummary> {
        let recur = self.note(id)?.action?.recur?;
        let days_off = self.days_off_set();
        let occurrences = Occurrences::new(&recur, &days_off, today);
        let current = occurrences.current();
        Some(OccurrenceSummary {
            current,
            missed: occurrences.missed(current.as_ref()),
            waiting: current.map_or(0, |current| occurrences.waiting(&current)),
        })
    }

    /// `notes` as of `today`, with each recurring action and its steps as `note_on` gives them.
    pub fn notes_on(&self, notes: Vec<Note>, today: Date) -> Vec<Note> {
        if !notes.iter().any(|note| note.action.is_some()) {
            return notes;
        }
        let hierarchy = self.hierarchy();
        let ancestors: Vec<NoteId> = notes
            .iter()
            .filter(|note| note.action.is_some())
            .flat_map(|note| hierarchy.ancestors(note.id))
            .collect();
        let mut all: HashMap<NoteId, Note> =
            notes.iter().map(|note| (note.id, note.clone())).collect();
        for ancestor in ancestors {
            if !all.contains_key(&ancestor)
                && let Some(note) = self.note(ancestor)
            {
                all.insert(ancestor, note);
            }
        }
        self.resolve_recurrence_in(&mut all, today, &hierarchy);
        // Looked up by position, not by draining `all`, so a note listed twice comes back twice.
        notes.into_iter().map(|note| all.get(&note.id).cloned().unwrap_or(note)).collect()
    }

    /// A recurring action's current occurrence and its `upcoming` occurrences after it, in order,
    /// then its latest `past` occurrences, latest first.
    pub fn occurrences(
        &self,
        id: NoteId,
        today: Date,
        past: u32,
        upcoming: u32,
    ) -> Vec<Occurrence> {
        let Some(recur) = self.note(id).and_then(|note| note.action).and_then(|a| a.recur) else {
            return Vec::new();
        };
        let days_off = self.days_off_set();
        Occurrences::new(&recur, &days_off, today).history(past as usize, upcoming as usize)
    }

    /// The dates the first `count` occurrences on or after `from` would fall on under `settings`,
    /// with the library's days off, for an editor to preview a rule.
    pub fn preview_dates(
        &self,
        settings: &RecurrenceSettings,
        from: Date,
        count: u32,
    ) -> Vec<Date> {
        let recur = Recurrence {
            settings: settings.clone(),
            handled: Vec::new(),
            moved: Vec::new(),
            notes: Vec::new(),
        };
        let days_off = self.days_off_set();
        let occurrences = Occurrences::new(&recur, &days_off, from);
        occurrences
            .dates(from.max(settings.start))
            .take(count as usize)
            .map(|date| occurrences.occurrence(date, OccurrenceStatus::Upcoming).on)
            .collect()
    }

    /// Ticks or unticks an action's checkbox. For a recurring action, ticking closes its current
    /// occurrence, and unticking one whose rule has ended reopens its last occurrence. For a step
    /// of a recurring action, it ticks the step for the current occurrence only, so the next
    /// occurrence starts with every step open. Otherwise it marks the action done or open.
    pub fn check_action(&mut self, id: NoteId, done: bool, today: Date) -> Result<(), Error> {
        let note = self.note(id).ok_or(Error::NotFound(id))?;
        let action = note.action.ok_or(Error::NotAnAction(id))?;
        let hierarchy = self.hierarchy();
        // The outermost, since `resolve_recurrence` ignores a repeat nested in another.
        let recurring_ancestor = hierarchy
            .ancestors(id)
            .filter(|&ancestor| {
                self.note(ancestor).and_then(|n| n.action).is_some_and(|a| a.recur.is_some())
            })
            .last();
        if let Some(ancestor) = recurring_ancestor
            && action.state == ActionState::Open
            && let Some(current) = self.current_occurrence(ancestor, today)
        {
            self.check_writable()?;
            let occurrences = child_map(&self.action_map(id)?, OCCURRENCES)?;
            let key = current.date.to_string();
            if done { occurrences.insert(&key, self.now()) } else { occurrences.delete(&key) }
                .map_err(loro_error)?;
            self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
            return self.commit();
        }
        if recurring_ancestor.is_none()
            && let Some(recur) = &action.recur
            && action.state == ActionState::Open
        {
            match self.current_occurrence(id, today) {
                Some(current) if done => {
                    let done = Some(OccurrenceState::Done);
                    return self.handle_occurrence(id, current.date, done, today);
                }
                // A finished rule shows the action done, so unticking it reopens the last
                // occurrence its owner handled.
                None if !done => {
                    if let Some(last) = recur.latest_handled() {
                        return self.handle_occurrence(id, last.date, None, today);
                    }
                }
                _ => {}
            }
        }
        let state = if done { ActionState::Done } else { ActionState::Open };
        self.change_action(id, crate::ActionChange::State(state))
    }

    /// Replaces each recurring action's own dates with its current occurrence's, counts a recurring
    /// action whose rule has ended as done, and marks each step of a recurring action done only if
    /// it's done in the current occurrence. Returns where each recurring action stands.
    pub(crate) fn resolve_recurrence(
        &self,
        notes: &mut HashMap<NoteId, Note>,
        today: Date,
    ) -> HashMap<NoteId, Standing> {
        let recurs =
            |note: &Note| note.action.as_ref().is_some_and(|action| action.recur.is_some());
        if !notes.values().any(recurs) {
            return HashMap::new();
        }
        self.resolve_recurrence_in(notes, today, &self.hierarchy())
    }

    /// Resolves recurrence as `resolve_recurrence` does, with the library's `hierarchy`.
    fn resolve_recurrence_in(
        &self,
        notes: &mut HashMap<NoteId, Note>,
        today: Date,
        hierarchy: &Hierarchy,
    ) -> HashMap<NoteId, Standing> {
        let recurring: Vec<NoteId> = notes
            .values()
            .filter(|note| note.action.as_ref().is_some_and(|action| action.recur.is_some()))
            .map(|note| note.id)
            .collect();
        if recurring.is_empty() {
            return HashMap::new();
        }
        let days_off = self.days_off_set();
        let mut standings = HashMap::new();
        for &id in &recurring {
            let nested = hierarchy.ancestors(id).any(|ancestor| recurring.contains(&ancestor));
            let Some(action) = notes.get_mut(&id).and_then(|note| note.action.as_mut()) else {
                continue;
            };
            if nested {
                action.recur = None;
                continue;
            }
            let Some(recur) = &action.recur else { continue };
            let occurrences = Occurrences::new(recur, &days_off, today);
            let current = occurrences.current();
            let waiting = current.map_or(0, |current| occurrences.waiting(&current));
            let last_closed = recur.handled.iter().map(|entry| entry.at).max();
            match current {
                Some(current) => {
                    action.due = current.due;
                    action.defer = current.defer;
                }
                None if action.state == ActionState::Open => {
                    action.state = ActionState::Done;
                    action.closed = last_closed.or(action.closed);
                }
                None => {}
            }
            standings.insert(id, Standing { current, waiting });
        }
        let steps: Vec<(NoteId, Date)> = notes
            .keys()
            .filter_map(|&id| {
                let ancestor = hierarchy.ancestors(id).find(|a| standings.contains_key(a))?;
                Some((id, standings[&ancestor].current?.date))
            })
            .collect();
        for (id, occurrence) in steps {
            let Some(action) = notes.get_mut(&id).and_then(|note| note.action.as_mut()) else {
                continue;
            };
            action.recur = None;
            if action.state == ActionState::Open
                && let Some(done) = action.occurrences.iter().find(|o| o.date == occurrence)
            {
                action.state = ActionState::Done;
                action.closed = Some(done.at);
            }
        }
        standings
    }

    /// A note as of `today`, with a recurring action's fields replaced as `resolve_recurrence`
    /// does, so the apps show the current occurrence's dates and each step's state in it.
    pub fn note_on(&self, id: NoteId, today: Date) -> Option<Note> {
        let note = self.note(id)?;
        if note.action.is_none() {
            return Some(note);
        }
        let mut notes: HashMap<NoteId, Note> = std::iter::once(note.id)
            .chain(self.hierarchy().ancestors(id))
            .filter_map(|id| self.note(id).map(|note| (id, note)))
            .collect();
        self.resolve_recurrence(&mut notes, today);
        notes.remove(&id)
    }

    pub(crate) fn action_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.note_map(id)?.get(ACTION) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotAnAction(id))
    }

    fn recur_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.action_map(id)?.get(RECUR) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotRecurring(id))
    }
}

/// Marks the action whose map is `action` done at `closed`, or, with nothing, open.
fn set_state(action: &LoroMap, closed: Option<i64>) -> Result<(), Error> {
    match closed {
        Some(at) => {
            action.insert(ACTION_STATE, ActionState::Done.as_str()).map_err(loro_error)?;
            action.insert(CLOSED, at)
        }
        None => {
            action.insert(ACTION_STATE, ActionState::Open.as_str()).map_err(loro_error)?;
            action.delete(CLOSED)
        }
    }
    .map_err(loro_error)
}

fn child_map(parent: &LoroMap, key: &str) -> Result<LoroMap, Error> {
    match parent.get(key) {
        Some(ValueOrContainer::Container(container)) => {
            container.into_map().map_err(|_| Error::Loro(format!("{key} isn't a map")))
        }
        _ => parent.insert_container(key, LoroMap::new()).map_err(loro_error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionView, DayOffEffect, DaysOff, DaysOffKind};

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn weekly(library: &mut Library, missed: Missed) -> NoteId {
        let id = library.create_task("Weekly review", None).unwrap();
        let mut settings = RecurrenceSettings::new(
            Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap(),
            date("2026-10-05"),
        );
        settings.missed = missed;
        library.set_recurrence(id, Some(settings), date("2026-10-05")).unwrap();
        id
    }

    fn current(library: &Library, id: NoteId, today: &str) -> Option<String> {
        library.current_occurrence(id, date(today)).map(|o| o.date.to_string())
    }

    #[test]
    fn latest_keeps_only_the_newest_occurrence_in_view() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        assert_eq!(current(&library, id, "2026-10-01").as_deref(), Some("2026-10-05"));
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-10-19"));
        library
            .handle_occurrence(
                id,
                date("2026-10-19"),
                Some(OccurrenceState::Done),
                date("2026-10-21"),
            )
            .unwrap();
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-10-26"));
        let history = library.occurrences(id, date("2026-10-21"), 5, 1);
        let statuses: Vec<(String, OccurrenceStatus)> =
            history.iter().map(|o| (o.date.to_string(), o.status)).collect();
        assert_eq!(
            statuses,
            [
                ("2026-10-26".into(), OccurrenceStatus::Current),
                ("2026-11-02".into(), OccurrenceStatus::Upcoming),
                ("2026-10-19".into(), OccurrenceStatus::Done),
                ("2026-10-12".into(), OccurrenceStatus::Missed),
                ("2026-10-05".into(), OccurrenceStatus::Missed),
            ]
        );
    }

    #[test]
    fn all_keeps_every_occurrence_after_the_latest_handled() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-10-05"));
        let dates = library.action_view(ActionView::All, date("2026-10-21"));
        assert_eq!(dates[0].waiting, 2);
        assert_eq!(dates[0].note.action.as_ref().unwrap().due, Some(date("2026-10-05")));
        library.check_action(id, true, date("2026-10-21")).unwrap();
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-10-12"));
        let handle = |library: &mut Library, day: &str| {
            let skipped = Some(OccurrenceState::Skipped);
            library.handle_occurrence(id, date(day), skipped, date("2026-10-21")).unwrap();
        };
        // Handled before it starts, so the ones before it still wait their turn.
        handle(&mut library, "2026-10-26");
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-10-12"));
        let summary = library.occurrence_summary(id, date("2026-10-21")).unwrap();
        assert_eq!((summary.missed, summary.waiting), (0, 1));
        handle(&mut library, "2026-10-12");
        handle(&mut library, "2026-10-19");
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn after_counts_from_the_last_completion() {
        let mut library = Library::in_memory();
        let id = library.create_task("Water the fern", None).unwrap();
        let mut settings = RecurrenceSettings::new(
            Rule::parse("FREQ=DAILY;INTERVAL=7").unwrap(),
            date("2026-10-01"),
        );
        settings.missed = Missed::After;
        library.set_recurrence(id, Some(settings), date("2026-10-01")).unwrap();
        assert_eq!(current(&library, id, "2026-10-20").as_deref(), Some("2026-10-01"));
        library.check_action(id, true, date("2026-10-20")).unwrap();
        assert_eq!(current(&library, id, "2026-10-20").as_deref(), Some("2026-10-27"));
    }

    #[test]
    fn offsets_moves_and_days_off_set_the_dates() {
        let mut library = Library::in_memory();
        let id = library.create_task("Pay rent", None).unwrap();
        let mut settings = RecurrenceSettings::new(
            Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap(),
            date("2026-12-01"),
        );
        settings.defer = Some(-3);
        settings.days_off.holidays = DayOffEffect::Move;
        let mut holiday = DaysOff::new(DaysOffKind::Holidays, "New year", date("2027-01-01"));
        holiday.rule = Rule::parse("FREQ=YEARLY");
        library.set_days_off(&holiday).unwrap();
        library.set_recurrence(id, Some(settings), date("2026-11-20")).unwrap();
        library.check_action(id, true, date("2026-11-30")).unwrap();
        let next = library.current_occurrence(id, date("2026-12-02")).unwrap();
        assert_eq!((next.date, next.on), (date("2027-01-01"), date("2027-01-02")));
        assert_eq!((next.defer, next.due), (Some(date("2026-12-30")), Some(date("2027-01-02"))));
        library.move_occurrence(id, next.date, Some(date("2027-01-04"))).unwrap();
        let next = library.current_occurrence(id, date("2026-12-02")).unwrap();
        assert_eq!((next.on, next.moved), (date("2027-01-04"), true));
        let note = library.note_on(id, date("2026-12-02")).unwrap();
        assert_eq!(note.action.unwrap().due, Some(date("2027-01-04")));
        library.set_recurrence(id, None, date("2026-12-02")).unwrap();
        let action = library.note(id).unwrap().action.unwrap();
        assert_eq!((action.recur, action.due), (None, Some(date("2027-01-04"))));
    }

    #[test]
    fn steps_start_open_in_each_occurrence() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        let step = library.create_task("Empty the inbox", Some(id)).unwrap();
        let today = date("2026-10-05");
        library.check_action(step, true, today).unwrap();
        assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
        assert_eq!(library.note(step).unwrap().action.unwrap().state, ActionState::Open);
        library.check_action(id, true, today).unwrap();
        assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Open);
        let steps = library.action_subtree(id, today);
        assert_eq!(steps[0].note.action.as_ref().unwrap().state, ActionState::Open);
    }

    #[test]
    fn a_rule_that_ends_closes_the_action() {
        let mut library = Library::in_memory();
        let id = library.create_task("Physical therapy", None).unwrap();
        let settings =
            RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=2").unwrap(), date("2026-10-01"));
        library.set_recurrence(id, Some(settings), date("2026-10-01")).unwrap();
        library.check_action(id, true, date("2026-10-01")).unwrap();
        library.check_action(id, true, date("2026-10-02")).unwrap();
        let closed = library.action_view(ActionView::Closed, date("2026-10-02"));
        assert_eq!(closed.len(), 1);
        assert!(library.action_view(ActionView::All, date("2026-10-02")).is_empty());
    }

    #[test]
    fn writes_notes_for_occurrences() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        let memo = library.write_occurrence_note(id, date("2026-10-05")).unwrap();
        let body = library.note(memo).unwrap().body;
        assert!(body.starts_with("Weekly review on 2026-10-05\n\n[["), "{body}");
        let occurrence = library.current_occurrence(id, date("2026-10-05")).unwrap();
        assert_eq!(occurrence.note, Some(memo));
    }

    #[test]
    fn summaries_count_missed_and_waiting_occurrences() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        let summary = library.occurrence_summary(id, date("2026-10-21")).unwrap();
        assert_eq!((summary.missed, summary.waiting), (0, 2));
        let latest = weekly(&mut library, Missed::Latest);
        let summary = library.occurrence_summary(latest, date("2026-10-21")).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-10-19")));
        assert_eq!((summary.missed, summary.waiting), (2, 0));
        let plain = library.create_task("Plain", None).unwrap();
        assert_eq!(library.occurrence_summary(plain, date("2026-10-21")), None);
    }

    #[test]
    fn after_previews_count_from_today_and_stop_at_count() {
        let mut library = Library::in_memory();
        let id = library.create_task("Water the fern", None).unwrap();
        let mut settings = RecurrenceSettings::new(
            Rule::parse("FREQ=DAILY;INTERVAL=7;COUNT=3").unwrap(),
            date("2026-10-01"),
        );
        settings.missed = Missed::After;
        library.set_recurrence(id, Some(settings), date("2026-10-01")).unwrap();
        let today = date("2026-10-30");
        let history = library.occurrences(id, today, 10, 10);
        let upcoming: Vec<Date> = history
            .iter()
            .filter(|o| o.status == OccurrenceStatus::Upcoming)
            .map(|o| o.on)
            .collect();
        assert_eq!(upcoming, [date("2026-11-06"), date("2026-11-13")]);
        assert_eq!(library.occurrence_summary(id, today).unwrap().waiting, 0);
    }

    #[test]
    fn note_lists_resolve_recurring_steps() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        let step = library.create_task("Empty the inbox", Some(id)).unwrap();
        let today = date("2026-10-05");
        library.check_action(step, true, today).unwrap();
        let raw = library.note(step).unwrap();
        let notes = library.notes_on(vec![raw.clone(), raw], today);
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().all(|note| note.action.as_ref().unwrap().state == ActionState::Done));
    }

    #[test]
    fn unticking_a_finished_action_reopens_its_last_occurrence() {
        let mut library = Library::in_memory();
        let id = library.create_task("Two reviews", None).unwrap();
        let settings = RecurrenceSettings::new(
            Rule::parse("FREQ=WEEKLY;BYDAY=MO;COUNT=2").unwrap(),
            date("2026-10-05"),
        );
        library.set_recurrence(id, Some(settings), date("2026-10-05")).unwrap();
        let today = date("2026-10-12");
        // Only the latest counts, so this closes the second and last occurrence.
        library.check_action(id, true, today).unwrap();
        assert_eq!(current(&library, id, "2026-10-12"), None);
        let state = |library: &Library| library.note_on(id, today).unwrap().action.unwrap().state;
        assert_eq!(state(&library), ActionState::Done);
        library.check_action(id, false, today).unwrap();
        assert_eq!(current(&library, id, "2026-10-12").as_deref(), Some("2026-10-12"));
        assert_eq!(state(&library), ActionState::Open);
    }

    #[test]
    fn latest_passes_over_occurrences_handled_early() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        let today = date("2026-10-21");
        library
            .handle_occurrence(id, date("2026-10-26"), Some(OccurrenceState::Done), today)
            .unwrap();
        let summary = library.occurrence_summary(id, today).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-10-19")));
        assert_eq!(summary.missed, 2);
        let history = library.occurrences(id, today, 5, 2);
        let statuses: Vec<(String, OccurrenceStatus)> =
            history.iter().map(|o| (o.date.to_string(), o.status)).collect();
        assert_eq!(
            statuses,
            [
                ("2026-10-19".into(), OccurrenceStatus::Current),
                ("2026-10-26".into(), OccurrenceStatus::Done),
                ("2026-11-02".into(), OccurrenceStatus::Upcoming),
                ("2026-11-09".into(), OccurrenceStatus::Upcoming),
                ("2026-10-12".into(), OccurrenceStatus::Missed),
                ("2026-10-05".into(), OccurrenceStatus::Missed),
            ]
        );
        library.check_action(id, true, today).unwrap();
        assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn changing_the_start_offset_keeps_missed_occurrences_missed() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        let mut settings = library.note(id).unwrap().action.unwrap().recur.unwrap().settings;
        settings.defer = Some(-3);
        library.set_recurrence(id, Some(settings.clone()), date("2026-10-05")).unwrap();
        // Handled after it started but before its date, leaving 2026-10-05 missed.
        let done = Some(OccurrenceState::Done);
        library.handle_occurrence(id, date("2026-10-12"), done, date("2026-10-10")).unwrap();
        settings.defer = Some(0);
        library.set_recurrence(id, Some(settings), date("2026-10-13")).unwrap();
        assert_eq!(current(&library, id, "2026-10-13").as_deref(), Some("2026-10-19"));
    }

    #[test]
    fn occurrences_moved_earlier_count_from_the_day_they_fall_on() {
        let mut library = Library::in_memory();
        let today = date("2026-10-21");
        let latest = weekly(&mut library, Missed::Latest);
        library.move_occurrence(latest, date("2026-11-30"), Some(date("2026-10-20"))).unwrap();
        assert_eq!(current(&library, latest, "2026-10-21").as_deref(), Some("2026-11-30"));
        // Only the three that have come into view are missed, not the weeks still ahead.
        assert_eq!(library.occurrence_summary(latest, today).unwrap().missed, 3);
        library.check_action(latest, true, today).unwrap();
        assert_eq!(current(&library, latest, "2026-10-21").as_deref(), Some("2026-10-26"));
        let all = weekly(&mut library, Missed::All);
        assert_eq!(library.occurrence_summary(all, today).unwrap().waiting, 2);
        library.move_occurrence(all, date("2026-11-30"), Some(date("2026-10-20"))).unwrap();
        assert_eq!(library.occurrence_summary(all, today).unwrap().waiting, 3);
    }

    #[test]
    fn early_handling_misses_nothing_under_all_and_counts_once_started_under_latest() {
        for (missed, then) in [(Missed::All, "2026-10-05"), (Missed::Latest, "2026-10-26")] {
            let mut library = Library::in_memory();
            let id = weekly(&mut library, missed);
            // 2026-10-05 is forgotten, and each later week is handled the day before.
            let done = Some(OccurrenceState::Done);
            library.handle_occurrence(id, date("2026-10-12"), done, date("2026-10-11")).unwrap();
            library.handle_occurrence(id, date("2026-10-19"), done, date("2026-10-18")).unwrap();
            assert_eq!(current(&library, id, "2026-10-11").as_deref(), Some("2026-10-05"));
            assert_eq!(current(&library, id, "2026-10-21").as_deref(), Some(then));
        }
    }

    #[test]
    fn postponed_occurrences_stay_pending_behind_nearer_ones() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        library.move_occurrence(id, date("2026-10-12"), Some(date("2026-12-01"))).unwrap();
        let today = date("2026-10-27");
        let summary = library.occurrence_summary(id, today).unwrap();
        assert_eq!(
            (summary.current.map(|o| o.date), summary.missed),
            (Some(date("2026-10-26")), 2)
        );
        let history = library.occurrences(id, today, 5, 1);
        let statuses: Vec<(String, OccurrenceStatus)> =
            history.iter().map(|o| (o.date.to_string(), o.status)).collect();
        assert_eq!(
            statuses,
            [
                ("2026-10-26".into(), OccurrenceStatus::Current),
                ("2026-11-02".into(), OccurrenceStatus::Upcoming),
                ("2026-10-12".into(), OccurrenceStatus::Upcoming),
                ("2026-10-19".into(), OccurrenceStatus::Missed),
                ("2026-10-05".into(), OccurrenceStatus::Missed),
            ]
        );
        library.check_action(id, true, today).unwrap();
        assert_eq!(current(&library, id, "2026-10-27").as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn postponed_occurrences_wait_behind_due_ones_under_all() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        library.move_occurrence(id, date("2026-10-05"), Some(date("2026-12-01"))).unwrap();
        let today = date("2026-10-21");
        let summary = library.occurrence_summary(id, today).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-10-12")));
        assert_eq!((summary.missed, summary.waiting), (0, 1));
        let history = library.occurrences(id, today, 5, 0);
        let postponed = history.iter().find(|o| o.date == date("2026-10-05")).unwrap();
        assert_eq!(postponed.status, OccurrenceStatus::Upcoming);
    }

    #[test]
    fn an_occurrence_moved_in_can_be_current_without_waiting_behind_itself() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        for day in ["2026-10-05", "2026-10-12", "2026-10-19"] {
            library.check_action(id, true, date(day)).unwrap();
        }
        library.move_occurrence(id, date("2026-11-30"), Some(date("2026-10-20"))).unwrap();
        let summary = library.occurrence_summary(id, date("2026-10-21")).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-11-30")));
        assert_eq!(summary.waiting, 0);
    }

    #[test]
    fn a_long_backlog_keeps_its_oldest_occurrence_current() {
        let mut library = Library::in_memory();
        let id = library.create_task("Log the day", None).unwrap();
        let mut daily =
            RecurrenceSettings::new(Rule::parse("FREQ=DAILY").unwrap(), date("2024-01-01"));
        daily.missed = Missed::All;
        library.set_recurrence(id, Some(daily), date("2024-01-01")).unwrap();
        let summary = library.occurrence_summary(id, date("2026-10-21")).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2024-01-01")));
        assert_eq!(summary.waiting, MAX_COUNT);
    }

    #[test]
    fn latest_finds_a_started_occurrence_older_than_a_month() {
        let mut library = Library::in_memory();
        let id = library.create_task("Yearly review", None).unwrap();
        let mut yearly =
            RecurrenceSettings::new(Rule::parse("FREQ=YEARLY").unwrap(), date("2020-03-01"));
        yearly.missed = Missed::Latest;
        library.set_recurrence(id, Some(yearly), date("2020-03-01")).unwrap();
        let summary = library.occurrence_summary(id, date("2026-10-31")).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-03-01")));
        assert_eq!(summary.missed, 6);
    }

    #[test]
    fn a_vacation_that_gathers_a_week_keeps_them_in_order() {
        let mut library = Library::in_memory();
        let id = library.create_task("Log the day", None).unwrap();
        let mut daily =
            RecurrenceSettings::new(Rule::parse("FREQ=DAILY").unwrap(), date("2026-10-01"));
        daily.missed = Missed::All;
        daily.days_off.vacations = DayOffEffect::Move;
        let mut trip = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-10-01"));
        trip.last = date("2026-10-07");
        library.set_days_off(&trip).unwrap();
        library.set_recurrence(id, Some(daily), date("2026-10-01")).unwrap();
        let summary = library.occurrence_summary(id, date("2026-10-08")).unwrap();
        let current = summary.current.unwrap();
        assert_eq!((current.date, current.on), (date("2026-10-01"), date("2026-10-08")));
        assert_eq!(summary.waiting, 7);
    }

    #[test]
    fn missed_counts_stop_at_the_cap() {
        let mut library = Library::in_memory();
        let id = library.create_task("Log the day", None).unwrap();
        let mut daily =
            RecurrenceSettings::new(Rule::parse("FREQ=DAILY").unwrap(), date("2025-10-01"));
        daily.missed = Missed::Latest;
        library.set_recurrence(id, Some(daily), date("2025-10-01")).unwrap();
        let summary = library.occurrence_summary(id, date("2026-10-31")).unwrap();
        assert_eq!(summary.current.map(|o| o.date), Some(date("2026-10-31")));
        assert_eq!(summary.missed, MAX_COUNT);
    }

    #[test]
    fn stopping_keeps_this_times_steps_and_a_finished_rule_done() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        let step = library.create_task("Empty the inbox", Some(id)).unwrap();
        let today = date("2026-10-05");
        library.check_action(step, true, today).unwrap();
        library.set_recurrence(id, None, today).unwrap();
        assert_eq!(library.note(step).unwrap().action.unwrap().state, ActionState::Done);

        let twice = library.create_task("Two reviews", None).unwrap();
        let settings = RecurrenceSettings::new(
            Rule::parse("FREQ=WEEKLY;BYDAY=MO;COUNT=2").unwrap(),
            date("2026-10-05"),
        );
        library.set_recurrence(twice, Some(settings), today).unwrap();
        library.check_action(twice, true, date("2026-10-12")).unwrap();
        library.set_recurrence(twice, None, date("2026-10-12")).unwrap();
        assert_eq!(library.note(twice).unwrap().action.unwrap().state, ActionState::Done);
    }

    #[test]
    fn starting_counts_a_done_action_and_its_done_steps_in_the_first_occurrence() {
        let mut library = Library::in_memory();
        let id = library.create_task("Renew passport", None).unwrap();
        let step = library.create_task("Fill in the form", Some(id)).unwrap();
        library.change_action(step, crate::ActionChange::State(ActionState::Done)).unwrap();
        library.change_action(id, crate::ActionChange::State(ActionState::Done)).unwrap();
        let today = date("2026-10-05");
        let yearly = RecurrenceSettings::new(Rule::parse("FREQ=YEARLY").unwrap(), today);
        library.set_recurrence(id, Some(yearly), today).unwrap();
        assert_eq!(library.note(id).unwrap().action.unwrap().state, ActionState::Open);
        assert_eq!(current(&library, id, "2026-10-05").as_deref(), Some("2027-10-05"));
        assert_eq!(library.note(step).unwrap().action.unwrap().state, ActionState::Open);
        let later = date("2027-10-05");
        assert_eq!(library.note_on(step, later).unwrap().action.unwrap().state, ActionState::Open);
    }

    #[test]
    fn each_handled_occurrence_misses_what_it_reached() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::Latest);
        // The first stays pending, and holds back the cutoff, while the second starts in time
        // for only the later of the two handled occurrences to reach it.
        library.move_occurrence(id, date("2026-10-05"), Some(date("2026-12-01"))).unwrap();
        library.move_occurrence(id, date("2026-10-12"), Some(date("2026-10-29"))).unwrap();
        let done = Some(OccurrenceState::Done);
        library.handle_occurrence(id, date("2026-10-19"), done, date("2026-10-20")).unwrap();
        library.handle_occurrence(id, date("2026-10-26"), done, date("2026-10-31")).unwrap();
        assert_eq!(current(&library, id, "2026-10-31").as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn occurrences_postponed_over_a_month_stay_pending() {
        let mut library = Library::in_memory();
        let id = library.create_task("Pay rent", None).unwrap();
        let mut monthly =
            RecurrenceSettings::new(Rule::parse("FREQ=MONTHLY").unwrap(), date("2026-07-01"));
        monthly.missed = Missed::Latest;
        library.set_recurrence(id, Some(monthly), date("2026-07-01")).unwrap();
        library.move_occurrence(id, date("2026-08-01"), Some(date("2026-12-15"))).unwrap();
        library.check_action(id, true, date("2026-10-02")).unwrap();
        let history = library.occurrences(id, date("2026-10-02"), 5, 0);
        let august = history.iter().find(|o| o.date == date("2026-08-01")).unwrap();
        assert_eq!(august.status, OccurrenceStatus::Upcoming);
    }

    #[test]
    fn history_reaches_back_past_its_first_span() {
        let mut library = Library::in_memory();
        let id = library.create_task("Pay rent", None).unwrap();
        let monthly =
            RecurrenceSettings::new(Rule::parse("FREQ=MONTHLY").unwrap(), date("2020-01-01"));
        library.set_recurrence(id, Some(monthly), date("2020-01-01")).unwrap();
        let today = date("2026-10-21");
        let missed = |past: u32| -> Vec<String> {
            let history = library.occurrences(id, today, past, 0);
            history.iter().skip(1).map(|o| o.date.to_string()).collect()
        };
        assert_eq!(missed(3), ["2026-09-01", "2026-08-01", "2026-07-01"]);
        let all = missed(100);
        assert_eq!(all.len(), 81);
        assert_eq!(all.last().map(String::as_str), Some("2020-01-01"));
    }

    #[test]
    fn history_lists_early_occurrences_when_the_rule_ends_soon() {
        let mut library = Library::in_memory();
        let id = library.create_task("Four reviews", None).unwrap();
        let settings = RecurrenceSettings::new(
            Rule::parse("FREQ=WEEKLY;BYDAY=MO;COUNT=4").unwrap(),
            date("2026-10-05"),
        );
        library.set_recurrence(id, Some(settings), date("2026-10-05")).unwrap();
        let today = date("2026-10-21");
        library
            .handle_occurrence(id, date("2026-10-26"), Some(OccurrenceState::Done), today)
            .unwrap();
        let history = library.occurrences(id, today, 5, 5);
        let dates: Vec<String> = history.iter().map(|o| o.date.to_string()).collect();
        assert_eq!(dates, ["2026-10-19", "2026-10-26", "2026-10-12", "2026-10-05"]);
    }

    #[test]
    fn steps_follow_the_outermost_repeat() {
        let mut library = Library::in_memory();
        let outer = weekly(&mut library, Missed::Latest);
        let inner = library.create_task("Monthly part", Some(outer)).unwrap();
        let monthly = RecurrenceSettings::new(
            Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap(),
            date("2026-10-01"),
        );
        let today = date("2026-10-05");
        library.set_recurrence(inner, Some(monthly), today).unwrap();
        let step = library.create_task("Step", Some(inner)).unwrap();
        library.check_action(step, true, today).unwrap();
        assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
    }

    #[test]
    fn occurrences_wait_for_their_date_without_a_start_offset() {
        let mut library = Library::in_memory();
        let id = library.create_task("Weekly review", None).unwrap();
        let mut settings = RecurrenceSettings::new(
            Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap(),
            date("2026-10-05"),
        );
        settings.defer = None;
        library.set_recurrence(id, Some(settings), date("2026-10-05")).unwrap();
        library.check_action(id, true, date("2026-10-05")).unwrap();
        let next = library.current_occurrence(id, date("2026-10-06")).unwrap();
        assert_eq!(next.defer, Some(date("2026-10-12")));
        let scheduled = library.action_view(ActionView::Scheduled, date("2026-10-06"));
        assert!(scheduled.iter().any(|entry| entry.note.id == id));
    }
}
