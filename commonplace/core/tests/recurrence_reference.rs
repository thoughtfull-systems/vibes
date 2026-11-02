// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::{BTreeMap, BTreeSet};

use commonplace_core::{
    ActionChange, ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, NoteId,
    Occurrence, OccurrenceInactiveReason, OccurrenceState, OccurrenceStatus, OccurrenceTarget,
    RecurrenceSettings, Rule, SeriesState,
};

fn first() -> Date {
    "2026-10-01".parse().unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Calendar {
    Daily,
    Monthly,
    MonthlyOrdinal { ordinal: i8, weekday: u8 },
    MonthlyPosition { position: i8 },
    Weekly,
    Yearly,
}

impl Calendar {
    fn rule(self, count: u32) -> Rule {
        let text = match self {
            Self::Daily => "FREQ=DAILY;INTERVAL=3".to_owned(),
            Self::Monthly => "FREQ=MONTHLY".to_owned(),
            Self::MonthlyOrdinal { ordinal, weekday } => {
                let days = if ordinal > 0 { "1,2,3,4,5,6,7" } else { "-1,-2,-3,-4,-5,-6,-7" };
                let weekday = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][weekday as usize];
                format!("FREQ=MONTHLY;BYMONTHDAY={days};BYDAY={ordinal}{weekday}")
            }
            Self::MonthlyPosition { position } => {
                format!("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS={position}")
            }
            Self::Weekly => "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;WKST=SU".to_owned(),
            Self::Yearly => "FREQ=YEARLY".to_owned(),
        };
        Rule::parse(&format!("{text};COUNT={count}")).unwrap()
    }

    fn dates(self, origin: Date, count: usize) -> Vec<Date> {
        match self {
            Self::Daily => (0..count).map(|index| origin.add_days(index as i32 * 3)).collect(),
            Self::Monthly | Self::MonthlyOrdinal { .. } | Self::MonthlyPosition { .. } => {
                let month = i32::from(origin.year()) * 12 + i32::from(origin.month()) - 1;
                (0..count * 2 + 24)
                    .filter_map(|index| {
                        let next = month + index as i32;
                        let year = (next / 12) as u16;
                        let month = (next % 12 + 1) as u8;
                        match self {
                            Self::MonthlyOrdinal { ordinal, weekday } => {
                                let last = Date::days_in_month(year, month);
                                (1..=last).filter_map(|day| Date::new(year, month, day)).find(
                                    |day| {
                                        day.weekday() == weekday
                                            && *day >= origin
                                            && if ordinal > 0 {
                                                day.day() <= 7
                                            } else {
                                                day.day() > last - 7
                                            }
                                    },
                                )
                            }
                            Self::MonthlyPosition { position } => {
                                let dates: Vec<_> = (1..=Date::days_in_month(year, month))
                                    .filter_map(|day| Date::new(year, month, day))
                                    .filter(|day| day.weekday() < 5 && *day >= origin)
                                    .collect();
                                pick(&dates, position)
                            }
                            _ => Date::new(year, month, origin.day()),
                        }
                    })
                    .take(count)
                    .collect()
            }
            Self::Weekly => {
                let sunday = origin.add_days(-i32::from((origin.weekday() + 1) % 7));
                (0..count * 14 + 14)
                    .map(|days| origin.add_days(days as i32))
                    .filter(|&day| {
                        (day.weekday() == 0 || day.weekday() == 3)
                            && (day.days_since(sunday) / 7) % 2 == 0
                    })
                    .take(count)
                    .collect()
            }
            Self::Yearly => (0..count * 8 + 8)
                .filter_map(|index| {
                    Date::new(origin.year() + index as u16, origin.month(), origin.day())
                })
                .take(count)
                .collect(),
        }
    }
}

fn pick(dates: &[Date], position: i8) -> Option<Date> {
    let index = if position > 0 {
        (position - 1) as usize
    } else {
        dates.len().checked_sub(position.unsigned_abs() as usize)?
    };
    dates.get(index).copied()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Key {
    Calendar(Date),
    Cycle(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Slot {
    key: Key,
    date: Date,
}

impl Slot {
    fn of(occurrence: Occurrence) -> Self {
        Self {
            key: occurrence.target.cycle.map_or(Key::Calendar(occurrence.date), Key::Cycle),
            date: occurrence.date,
        }
    }
}

#[derive(Clone)]
struct Completion {
    activation: Date,
    day: Date,
    origin: Date,
    settled: BTreeSet<Date>,
    sequence: u32,
    slot: Slot,
}

struct Reference {
    calendar: Calendar,
    closed: BTreeMap<Key, Completion>,
    count: u32,
    defer: i32,
    due: i32,
    effect: DayOffEffect,
    moved: BTreeMap<Key, (Date, Date)>,
    not_before: Date,
    policy: Missed,
    reopened: BTreeMap<Key, (Slot, u32, Date)>,
    sequence: u32,
    settlements: Vec<Completion>,
    start: Date,
    steps: BTreeSet<Key>,
}

impl Reference {
    fn new(calendar: Calendar, policy: Missed) -> Self {
        Self {
            calendar,
            closed: BTreeMap::new(),
            count: 8,
            defer: 0,
            due: 0,
            effect: DayOffEffect::Ignore,
            moved: BTreeMap::new(),
            not_before: first(),
            policy,
            reopened: BTreeMap::new(),
            sequence: 0,
            settlements: Vec::new(),
            start: first(),
            steps: BTreeSet::new(),
        }
    }

    fn settings(&self) -> RecurrenceSettings {
        let mut settings = RecurrenceSettings::new(self.calendar.rule(self.count), self.start);
        settings.defer = Some(self.defer);
        settings.due = Some(self.due);
        settings.missed = self.policy;
        settings.days_off.vacations = self.effect;
        settings
    }

    fn reset(&mut self, not_before: Date) {
        self.closed.clear();
        self.moved.clear();
        self.not_before = not_before;
        self.reopened.clear();
        self.settlements.clear();
        self.steps.clear();
    }

    fn on(&self, slot: Slot) -> Option<Date> {
        if let Some((_, to)) = self.moved.get(&slot.key) {
            return Some(*to);
        }
        let date = if let Calendar::MonthlyPosition { position } = self.calendar
            && self.effect == DayOffEffect::Skip
        {
            let origin = match slot.key {
                Key::Calendar(_) => self.start,
                Key::Cycle(_) => self.reopened.get(&slot.key).map_or_else(
                    || {
                        self.closed
                            .values()
                            .max_by_key(|entry| entry.sequence)
                            .map_or(self.start, |entry| entry.day.max(self.start))
                    },
                    |(_, _, origin)| *origin,
                ),
            };
            let dates: Vec<_> = (1..=Date::days_in_month(slot.date.year(), slot.date.month()))
                .filter_map(|day| Date::new(slot.date.year(), slot.date.month(), day))
                .filter(|day| {
                    *day >= origin
                        && day.weekday() < 5
                        && !(4..=7).contains(&day.days_since(first()))
                })
                .collect();
            pick(&dates, position)?
        } else {
            slot.date
        };
        if (4..=7).contains(&date.days_since(first())) {
            match self.effect {
                DayOffEffect::Ignore => Some(date),
                DayOffEffect::Move => Some(first().add_days(8)),
                DayOffEffect::Skip => None,
            }
        } else {
            Some(date)
        }
    }

    fn started(&self, slot: Slot) -> Option<Date> {
        self.on(slot).map(|on| on.add_days(self.defer))
    }

    fn missed(&self, slot: Slot, today: Date) -> bool {
        self.settled_on(slot, today).is_some()
    }

    fn settled_on(&self, slot: Slot, today: Date) -> Option<Date> {
        if self.policy != Missed::Latest {
            return None;
        }
        self.settlements
            .iter()
            .filter(|entry| {
                entry.activation <= today
                    && entry.settled.contains(&slot.date)
                    && self.reopened.get(&slot.key).is_none_or(|(_, seq, _)| *seq <= entry.sequence)
            })
            .map(|entry| entry.activation)
            .min()
    }

    fn open(&self, today: Date) -> Vec<Slot> {
        self.calendar
            .dates(self.start, self.count as usize)
            .into_iter()
            .filter(|&date| date >= self.not_before)
            .map(|date| Slot { key: Key::Calendar(date), date })
            .filter(|slot| {
                self.on(*slot).is_some()
                    && !self.closed.contains_key(&slot.key)
                    && !self.missed(*slot, today)
            })
            .collect()
    }

    fn current(&self, today: Date) -> Option<Slot> {
        if self.policy == Missed::After {
            if self.closed.len() >= self.count as usize {
                return None;
            }
            if let Some((slot, _, origin)) = self.reopened.values().next() {
                let candidates = self.calendar.dates(*origin, 100);
                if self.moved.contains_key(&slot.key) && candidates.contains(&slot.date) {
                    return Some(*slot);
                }
                return candidates
                    .into_iter()
                    .filter(|&date| date >= slot.date)
                    .map(|date| Slot { key: slot.key, date })
                    .find(|slot| self.on(*slot).is_some());
            }
            let latest = self.closed.values().max_by_key(|entry| entry.sequence);
            let origin = latest.map_or(self.start, |entry| entry.day.max(self.start));
            let from = latest
                .map_or(origin, |entry| entry.day.add_days(1).max(origin))
                .max(self.not_before);
            let cycle = self
                .closed
                .keys()
                .chain(self.reopened.keys())
                .filter_map(|key| match key {
                    Key::Cycle(cycle) => Some(*cycle),
                    _ => None,
                })
                .max()
                .map_or(0, |cycle| cycle + 1);
            let key = Key::Cycle(cycle);
            let candidates = self.calendar.dates(origin, 100);
            if let Some(&(date, _)) = self.moved.get(&key)
                && candidates.contains(&date)
            {
                return Some(Slot { key, date });
            }
            return candidates
                .into_iter()
                .filter(|&date| date >= from)
                .map(|date| Slot { key, date })
                .find(|slot| self.on(*slot).is_some());
        }
        let open = self.open(today);
        if self.policy == Missed::Latest
            && let Some(slot) = open
                .iter()
                .copied()
                .filter(|slot| self.started(*slot).is_some_and(|start| start <= today))
                .max_by_key(|slot| (self.started(*slot), slot.date))
        {
            return Some(slot);
        }
        open.into_iter().min_by_key(|slot| (self.started(*slot), slot.date))
    }

    fn handle(&mut self, slot: Slot, today: Date) {
        self.sequence += 1;
        let activation = today.max(self.started(slot).unwrap());
        let settled = self
            .calendar
            .dates(self.start, self.count as usize)
            .into_iter()
            .filter(|&date| {
                date >= self.not_before
                    && self
                        .started(Slot { key: Key::Calendar(date), date })
                        .is_some_and(|start| start <= activation)
            })
            .collect();
        let origin = self.reopened.get(&slot.key).map_or_else(
            || {
                self.closed
                    .values()
                    .max_by_key(|entry| entry.sequence)
                    .map_or(self.start, |entry| entry.day.max(self.start))
            },
            |(_, _, origin)| *origin,
        );
        self.reopened.remove(&slot.key);
        let completion =
            Completion { activation, day: today, origin, settled, sequence: self.sequence, slot };
        self.settlements.push(completion.clone());
        self.closed.insert(slot.key, completion);
    }

    fn check(&self, library: &Library, id: NoteId, step: NoteId, today: Date) {
        let current = self.current(today);
        let actual = library.current_occurrence(id, today);
        assert_eq!(actual.map(Slot::of), current, "{today}: {:?}", self.calendar);
        if let Some(slot) = current {
            let occurrence = actual.unwrap();
            assert_eq!(Some(occurrence.on), self.on(slot));
            assert_eq!(occurrence.defer, self.started(slot));
            assert_eq!(occurrence.due, self.on(slot).map(|on| on.add_days(self.due)));
            let action = library.note_on(step, today).unwrap().action.unwrap();
            assert_eq!(action.state == ActionState::Done, self.steps.contains(&slot.key));
            assert_eq!(action.occurrence.unwrap().target, occurrence.target);
        }
        let waiting = if self.policy == Missed::All {
            self.open(today)
                .into_iter()
                .filter(|slot| {
                    Some(*slot) != current && self.on(*slot).unwrap().add_days(self.due) <= today
                })
                .count()
        } else {
            0
        };
        assert_eq!(library.occurrence_summary(id, today).unwrap().waiting, waiting as u32);
        let history = library.occurrences(id, today, 100, 20);
        let keys: BTreeSet<_> = history.iter().map(|occurrence| occurrence.target.key).collect();
        assert_eq!(keys.len(), history.len());
        assert_eq!(
            history.iter().find(|o| o.status == OccurrenceStatus::Current).copied().map(Slot::of),
            current
        );
        for entry in self.closed.values() {
            assert!(history.iter().any(|occurrence| Slot::of(*occurrence) == entry.slot
                && occurrence.status == OccurrenceStatus::Done));
        }
        if self.policy != Missed::After {
            for slot in self.open(today) {
                let superseded = self.policy == Missed::Latest
                    && self.started(slot).is_some_and(|start| start <= today)
                    && current.is_some_and(|current| {
                        (self.started(slot), slot.date) < (self.started(current), current.date)
                    });
                if !superseded {
                    assert!(
                        history.iter().any(|occurrence| Slot::of(*occurrence) == slot),
                        "missing open {slot:?} on {today}"
                    );
                }
            }
            let nominal = self.calendar.dates(self.start, self.count as usize);
            let candidates: BTreeSet<_> = nominal
                .iter()
                .copied()
                .chain(self.settlements.iter().flat_map(|entry| entry.settled.iter().copied()))
                .collect();
            let mut expected_missed = BTreeMap::new();
            for date in candidates {
                let slot = Slot { key: Key::Calendar(date), date };
                if self.closed.contains_key(&slot.key) {
                    continue;
                }
                let settled = self.settled_on(slot, today);
                let superseded = self.policy == Missed::Latest
                    && date >= self.not_before
                    && nominal.contains(&date)
                    && self.started(slot).is_some_and(|start| {
                        start <= today
                            && current.is_none_or(|current| {
                                (start, date) < (self.started(current).unwrap(), current.date)
                            })
                    });
                if settled.is_some() || superseded {
                    expected_missed.insert(slot.key, settled);
                }
            }
            let actual_missed: BTreeMap<_, _> = history
                .iter()
                .filter(|entry| entry.status == OccurrenceStatus::Missed)
                .map(|entry| {
                    assert!(!entry.upcoming);
                    (Slot::of(*entry).key, entry.settled)
                })
                .collect();
            assert_eq!(
                actual_missed, expected_missed,
                "missed history on {today}: {:?} {:?}",
                self.calendar, self.policy
            );
            assert_eq!(
                library.occurrence_summary(id, today).unwrap().missed as usize,
                expected_missed.len()
            );
        }
        for (key, (slot, _, _)) in &self.reopened {
            let entry = history
                .iter()
                .find(|entry| entry.editable && Slot::of(**entry).key == *key)
                .unwrap_or_else(|| panic!("missing reopened {key:?} on {today}"));
            if self.policy == Missed::After {
                if self.closed.len() >= self.count as usize {
                    assert_eq!(entry.inactive_reason, Some(OccurrenceInactiveReason::CycleLimit));
                }
            } else if self.missed(*slot, today) {
                assert_eq!(entry.status, OccurrenceStatus::Missed);
            } else if !self.calendar.dates(self.start, self.count as usize).contains(&slot.date) {
                assert_eq!(entry.inactive_reason, Some(OccurrenceInactiveReason::Bounds));
            } else if self.on(*slot).is_none() {
                assert_eq!(entry.inactive_reason, Some(OccurrenceInactiveReason::DaysOff));
            }
        }
    }
}

fn next(seed: &mut u64) -> u32 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (*seed >> 32) as u32
}

fn copied(library: &Library) -> Library {
    let mut peer = Library::in_memory();
    peer.merge_updates(&library.export_snapshot().unwrap(), false).unwrap();
    peer
}

fn merge(left: &mut Library, right: &mut Library) {
    if let Some(bytes) = right.updates_since(&left.version()).unwrap() {
        left.merge_updates(&bytes, false).unwrap();
    }
    if let Some(bytes) = left.updates_since(&right.version()).unwrap() {
        right.merge_updates(&bytes, false).unwrap();
    }
}

fn handled_target(library: &Library, id: NoteId, slot: Slot) -> OccurrenceTarget {
    library
        .note(id)
        .unwrap()
        .action
        .unwrap()
        .recur
        .unwrap()
        .handled
        .into_iter()
        .find(|entry| entry.target.cycle.map_or(Key::Calendar(entry.date), Key::Cycle) == slot.key)
        .unwrap()
        .target
}

#[test]
fn generated_transitions_match_a_bounded_reference() {
    for pattern in 0..6 {
        for policy in [Missed::After, Missed::All, Missed::Latest] {
            for initial_seed in 0..16 {
                let calendar = match pattern {
                    0 => Calendar::Daily,
                    1 => Calendar::Monthly,
                    2 => Calendar::MonthlyOrdinal {
                        ordinal: if initial_seed % 2 == 0 { 1 } else { -1 },
                        weekday: (initial_seed / 2 % 7) as u8,
                    },
                    3 => Calendar::MonthlyPosition {
                        position: [1, 2, -1, -2][initial_seed as usize % 4],
                    },
                    4 => Calendar::Weekly,
                    _ => Calendar::Yearly,
                };
                let mut seed = initial_seed;
                let mut model = Reference::new(calendar, policy);
                let mut library = Library::in_memory();
                let id = library.create_task("Routine", None).unwrap();
                let step = library.create_task("Step", Some(id)).unwrap();
                let mut vacation =
                    DaysOff::new(DaysOffKind::Vacations, "Trip", first().add_days(4));
                vacation.last = first().add_days(7);
                library.set_days_off(&vacation).unwrap();
                library.set_recurrence(id, Some(model.settings()), first()).unwrap();
                for tick in 0..32 {
                    let today = first().add_days(tick);
                    model.check(&library, id, step, today);
                    let current = library.current_occurrence(id, today);
                    match next(&mut seed) % 11 {
                        0 => {
                            if let Some(current) = current {
                                let to = today.add_days((next(&mut seed) % 13) as i32 - 4);
                                library
                                    .move_occurrence_target(id, current.target, Some(to))
                                    .unwrap();
                                model.moved.insert(Slot::of(current).key, (current.date, to));
                            }
                        }
                        1 => {
                            if let Some(current) = current {
                                library
                                    .handle_occurrence_target(
                                        id,
                                        current.target,
                                        Some(OccurrenceState::Done),
                                        today,
                                    )
                                    .unwrap();
                                model.handle(Slot::of(current), today);
                            }
                        }
                        2 => {
                            if let Some((&key, entry)) = model.closed.iter().next() {
                                let slot = entry.slot;
                                let origin = entry.origin;
                                let target = handled_target(&library, id, slot);
                                library.handle_occurrence_target(id, target, None, today).unwrap();
                                model.closed.remove(&key);
                                model.sequence += 1;
                                model.reopened.insert(key, (slot, model.sequence, origin));
                            }
                        }
                        3 => {
                            model.defer = (next(&mut seed) % 7) as i32 - 3;
                            model.due = (next(&mut seed) % 9) as i32 - 3;
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                        }
                        4 => {
                            model.effect = match next(&mut seed) % 3 {
                                0 => DayOffEffect::Ignore,
                                1 => DayOffEffect::Skip,
                                _ => DayOffEffect::Move,
                            };
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                        }
                        5 => {
                            if let Some(current) = current {
                                let done = next(&mut seed).is_multiple_of(2);
                                library.check_action(step, done, today).unwrap();
                                if done {
                                    model.steps.insert(Slot::of(current).key);
                                } else {
                                    model.steps.remove(&Slot::of(current).key);
                                }
                            }
                        }
                        6 => {
                            let mut peer = copied(&library);
                            let mut remote = model.settings();
                            remote.days_off.vacations = DayOffEffect::Ignore;
                            model.due += 1;
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                            peer.set_recurrence(id, Some(remote), today).unwrap();
                            model.effect = DayOffEffect::Ignore;
                            merge(&mut library, &mut peer);
                            model.check(&peer, id, step, today);
                        }
                        7 => {
                            model.count = next(&mut seed) % 8 + 3;
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                        }
                        8 => {
                            let policy = if next(&mut seed).is_multiple_of(2) {
                                Missed::All
                            } else {
                                Missed::Latest
                            };
                            if model.policy == Missed::After {
                                model.reset(today.max(model.start));
                            }
                            model.policy = policy;
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                        }
                        9 => {
                            model.calendar = if next(&mut seed).is_multiple_of(2) {
                                Calendar::Daily
                            } else {
                                Calendar::Monthly
                            };
                            model.start = today.add_days(1);
                            model.reset(model.start);
                            library.set_recurrence(id, Some(model.settings()), today).unwrap();
                        }
                        _ => {
                            library
                                .change_action(id, ActionChange::State(ActionState::Done))
                                .unwrap();
                            assert_eq!(
                                library.occurrence_summary(id, today).unwrap().state,
                                SeriesState::Stopped
                            );
                            library
                                .change_action(id, ActionChange::State(ActionState::Open))
                                .unwrap();
                        }
                    }
                    model.check(&library, id, step, today);
                }
            }
        }
    }
}
