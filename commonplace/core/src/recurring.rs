// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use loro::{LoroMap, LoroValue, ValueOrContainer};

use crate::actions::{ACTION, ActionState, CLOSED, Date, Hierarchy, STATE as ACTION_STATE};
use crate::days_off::{DaysOffSet, DaysOffUse};
use crate::library::{MODIFIED, integer, loro_error, string};
use crate::occurrence_store::{
    self as store, date, fields, insert_preserving, number, record_key, text,
};
use crate::recur::Rule;
use crate::{Error, Library, Note, NoteId, OccurrenceId, wikilink};

pub(crate) const RECUR: &str = "recur";
pub(crate) const OCCURRENCES: &str = "occurrences";
const MAX_COUNT: u32 = 99;

impl Library {
    pub fn set_recurrence(
        &mut self,
        id: NoteId,
        settings: Option<RecurrenceSettings>,
        today: Date,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let action = self.action_map(id)?;
        let before = self.note(id).and_then(|note| note.action).ok_or(Error::NotAnAction(id))?;
        let old = before.recur.as_ref();
        if old.map(|recur| &recur.settings) == settings.as_ref() {
            return Ok(());
        }
        let hierarchy = self.hierarchy();
        let ignored = hierarchy
            .ancestors(id)
            .any(|ancestor| self.note(ancestor).and_then(|note| note.action?.recur).is_some());
        match settings {
            Some(settings) => {
                if !settings.rule.is_valid() {
                    return Err(Error::InvalidRule);
                }
                let replaced = old.is_some_and(|old| {
                    old.settings.start != settings.start
                        || !old.settings.rule.same_calendar(&settings.rule)
                        || (old.settings.missed == Missed::After)
                            != (settings.missed == Missed::After)
                });
                let previous = action
                    .get("recurrence_generation")
                    .and_then(|value| integer(value.get_deep_value()))
                    .and_then(|value| u64::try_from(value).ok())
                    .max(store::last_generation(&self.doc, id));
                let generation = if let Some(old) = old.filter(|_| !replaced) {
                    old.generation
                } else if let Some(previous) = previous {
                    previous
                        .checked_add(1)
                        .filter(|&value| value <= i64::MAX as u64)
                        .ok_or_else(|| Error::Loro("recurrence generation overflow".into()))?
                } else {
                    0
                };
                let not_before = if replaced {
                    today.max(settings.start)
                } else {
                    old.map_or(settings.start, |old| old.not_before)
                };
                let recur = Recurrence::new(id, settings.clone(), generation, not_before);
                if old.is_none() && action.get("recurrence_stopped").is_some() {
                    action.delete("recurrence_stopped").map_err(loro_error)?;
                }
                let map = child_map(&action, RECUR)?;
                if old.is_none_or(|old| old.calendar_value() != recur.calendar_value()) {
                    let LoroValue::Map(calendar) = recur.calendar_value() else { unreachable!() };
                    insert_preserving(
                        &map,
                        "calendar",
                        calendar.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
                    )?;
                }
                action.insert("recurrence_generation", generation as i64).map_err(loro_error)?;
                store::remember_generation(&self.doc, id, generation)?;
                for (key, offset, previous) in [
                    ("defer", settings.defer, old.and_then(|old| old.settings.defer)),
                    ("due", settings.due, old.and_then(|old| old.settings.due)),
                ] {
                    if old.is_none() || offset != previous {
                        match offset {
                            Some(offset) => map.insert(key, i64::from(offset)),
                            None => map.delete(key),
                        }
                        .map_err(loro_error)?;
                    }
                }
                settings.days_off.write_changes(&map, old.map(|old| &old.settings.days_off))?;
                if old.is_none() && !ignored {
                    let days_off = self.days_off_set();
                    let occurrences = Occurrences::new(&recur, &days_off, today);
                    if let Some(current) =
                        occurrences.current().or_else(|| occurrences.nominal_current())
                    {
                        for step in hierarchy.descendants(id) {
                            if let Some(fields) = self.note(step).and_then(|note| note.action)
                                && fields.state == ActionState::Done
                            {
                                self.store_step(
                                    step,
                                    OccurrenceContext { series: id, target: current.target },
                                    true,
                                    fields.closed,
                                )?;
                                set_state(&self.action_map(step)?, None)?;
                            }
                        }
                        if before.state == ActionState::Done {
                            self.store_handled(
                                id,
                                current.target,
                                OccurrenceState::Done,
                                before.closed.unwrap_or_else(|| self.now()),
                                today,
                            )?;
                            set_state(&action, None)?;
                        }
                    }
                }
            }
            None => {
                if old.is_none() {
                    return Ok(());
                }
                if !ignored {
                    let current = self.current_occurrence(id, today);
                    let context = self.note_on(id, today).and_then(|note| note.action?.occurrence);
                    if let Some(current) = current {
                        for (key, date) in [("defer", current.defer), ("due", current.due)] {
                            match date {
                                Some(date) => action.insert(key, date.to_string()),
                                None => action.delete(key),
                            }
                            .map_err(loro_error)?;
                        }
                    } else if before.state == ActionState::Open
                        && self.occurrence_summary(id, today).is_some_and(|summary| {
                            matches!(summary.state, SeriesState::Exhausted | SeriesState::Stopped)
                        })
                    {
                        let last = old.and_then(|old| {
                            old.stopped.or_else(|| old.latest_handling().map(|entry| entry.at))
                        });
                        set_state(&action, Some(last.unwrap_or_else(|| self.now())))?;
                    }
                    if let Some(context) = context {
                        for step in hierarchy.descendants(id) {
                            // Removing this repeat activates the nested repeat instead.
                            let nested = std::iter::once(step)
                                .chain(hierarchy.ancestors(step))
                                .take_while(|&ancestor| ancestor != id)
                                .any(|ancestor| {
                                    self.note(ancestor)
                                        .and_then(|note| note.action?.recur)
                                        .is_some()
                                });
                            if nested {
                                continue;
                            }
                            let Some(fields) = self.note(step).and_then(|note| note.action) else {
                                continue;
                            };
                            let closed = fields
                                .occurrences
                                .iter()
                                .find(|entry| {
                                    entry.key == Some(context.target.key)
                                        && entry.series == Some(id)
                                })
                                .map(|entry| entry.at);
                            set_state(&self.action_map(step)?, closed)?;
                        }
                    }
                }
                action.delete(RECUR).map_err(loro_error)?;
                action.delete("recurrence_stopped").map_err(loro_error)?;
            }
        }
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    fn occurrence_target_on(
        &self,
        id: NoteId,
        date: Date,
        today: Date,
    ) -> Result<OccurrenceTarget, Error> {
        let recur =
            self.note(id).and_then(|note| note.action?.recur).ok_or(Error::NotRecurring(id))?;
        if let Some(current) = self.current_occurrence(id, today)
            && (current.date == date || current.on == date)
        {
            return Ok(current.target);
        }
        if let Some(entry) = recur
            .handled
            .iter()
            .chain(&recur.reopened)
            .filter(|entry| entry.date == date)
            .max_by_key(|entry| entry.target.cycle)
        {
            return Ok(entry.target);
        }
        if recur.settings.missed == Missed::After {
            if let Some(entry) =
                self.occurrences(id, today, 0, MAX_COUNT).iter().find(|entry| entry.date == date)
            {
                return Ok(entry.target);
            }
            let origin = recur
                .latest_handling()
                .map_or(recur.settings.start, |entry| entry.day.max(recur.settings.start));
            let mut rule = recur.settings.rule.clone();
            rule.count = None;
            if rule.all_dates(origin, date).next() == Some(date) {
                let cycle = recur
                    .handled
                    .iter()
                    .chain(&recur.reopened)
                    .filter_map(|entry| entry.target.cycle)
                    .max()
                    .map_or(0, |cycle| cycle + 1);
                return Ok(recur.target_from(date, Some(cycle), origin));
            }
            return Err(Error::InvalidOccurrence);
        }
        Ok(recur.target(recur.legacy_nominal(date, &self.days_off_set()), None))
    }

    fn validate_target(&self, id: NoteId, target: OccurrenceTarget) -> Result<Recurrence, Error> {
        let recur =
            self.note(id).and_then(|note| note.action?.recur).ok_or(Error::NotRecurring(id))?;
        if recur.target(target.date, target.cycle).key != target.key
            || (recur.settings.missed == Missed::After) != target.cycle.is_some()
        {
            return Err(Error::InvalidOccurrence);
        }
        let known =
            recur.handled.iter().chain(&recur.reopened).any(|entry| entry.key == target.key);
        if !known {
            if target.cycle.is_some() {
                let mut rule = recur.settings.rule.clone();
                rule.count = None;
                if target.date < recur.not_before
                    || rule.all_dates(target.origin, target.date).next() != Some(target.date)
                {
                    return Err(Error::InvalidOccurrence);
                }
                if recur
                    .settings
                    .rule
                    .count
                    .is_some_and(|count| recur.handled.len() >= count as usize)
                {
                    return Err(Error::InvalidOccurrence);
                }
            } else if target.date < recur.not_before
                || recur
                    .settings
                    .rule
                    .slots(recur.settings.start, target.date, Box::new(|_| None))
                    .next()
                    .is_none_or(|slot| slot.nominal != target.date)
            {
                return Err(Error::InvalidOccurrence);
            }
        }
        Ok(recur)
    }

    fn store_handled(
        &self,
        id: NoteId,
        target: OccurrenceTarget,
        state: OccurrenceState,
        at: i64,
        today: Date,
    ) -> Result<(), Error> {
        let recur = self.validate_target(id, target)?;
        let days_off = self.days_off_set();
        let occurrences = Occurrences::new(&recur, &days_off, today);
        let occurrence = occurrences
            .current()
            .filter(|current| current.target.key == target.key)
            .unwrap_or_else(|| occurrences.occurrence_target(target, OccurrenceStatus::Upcoming));
        let order = store::next_handling_order(&self.doc, id);
        let peer = self.doc.peer_id();
        let early = today < occurrence.defer.unwrap_or(occurrence.on);
        let reach = occurrences.reach(
            target,
            today.max(occurrence.defer.unwrap_or(occurrence.on)),
            order,
            peer,
        );
        let mut values = store::target_fields(recur.scope, target);
        values.extend([
            ("at".into(), at.into()),
            ("day".into(), today.to_string().into()),
            ("early".into(), early.into()),
            ("on".into(), occurrence.on.to_string().into()),
            ("order".into(), (order as i64).into()),
            ("peer".into(), peer.to_string().into()),
            ("reach".into(), reach.to_value()),
            (
                "state".into(),
                match state {
                    OccurrenceState::Done => "done",
                    OccurrenceState::Skipped => "skipped",
                    OccurrenceState::Open => "open",
                }
                .into(),
            ),
        ]);
        for (key, date) in [("defer", occurrence.defer), ("due", occurrence.due)] {
            values.push((key.into(), date.map_or(LoroValue::Null, |date| date.to_string().into())));
        }
        if state != OccurrenceState::Open {
            let key = store::settlement_key(id, target.key, order, peer);
            insert_preserving(&self.doc.get_map(store::SETTLED), &key, values.clone())?;
        }
        insert_preserving(&self.doc.get_map(store::HANDLED), &record_key(id, target.key), values)
    }

    pub fn handle_occurrence_target(
        &mut self,
        id: NoteId,
        target: OccurrenceTarget,
        state: Option<OccurrenceState>,
        today: Date,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let recur = self.validate_target(id, target)?;
        if state.is_some_and(|state| {
            state != OccurrenceState::Open
                && recur.handled.iter().any(|entry| entry.key == target.key && entry.state == state)
        }) {
            return Ok(());
        }
        self.store_handled(id, target, state.unwrap_or(OccurrenceState::Open), self.now(), today)?;
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    pub fn handle_occurrence(
        &mut self,
        id: NoteId,
        date: Date,
        state: Option<OccurrenceState>,
        today: Date,
    ) -> Result<(), Error> {
        let target = self.occurrence_target_on(id, date, today)?;
        self.handle_occurrence_target(id, target, state, today)
    }

    pub fn move_occurrence_target(
        &mut self,
        id: NoteId,
        target: OccurrenceTarget,
        to: Option<Date>,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let recur = self.validate_target(id, target)?;
        let map = self.doc.get_map(store::MOVED);
        let key = record_key(id, target.key);
        if let Some(to) = to {
            let mut values = store::target_fields(recur.scope, target);
            values.extend([
                ("rule".into(), recur.settings.rule.to_string().into()),
                ("start".into(), recur.settings.start.to_string().into()),
                ("to".into(), to.to_string().into()),
            ]);
            insert_preserving(&map, &key, values)?;
        } else {
            map.delete(&key).map_err(loro_error)?;
        }
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    pub fn move_occurrence(
        &mut self,
        id: NoteId,
        date: Date,
        to: Option<Date>,
    ) -> Result<(), Error> {
        let target = self.occurrence_target_on(id, date, date)?;
        self.move_occurrence_target(id, target, to)
    }

    pub fn write_occurrence_note_target(
        &mut self,
        id: NoteId,
        target: OccurrenceTarget,
    ) -> Result<NoteId, Error> {
        self.check_writable()?;
        let recur = self.validate_target(id, target)?;
        let title = self.note(id).and_then(|note| note.label()).unwrap_or_default();
        let link = wikilink(id, (!title.is_empty()).then_some(title.as_str()));
        let body = format!("{title} on {}\n\n{link}\n", target.date);
        let memo = self.insert_note(body.trim_start(), crate::MEMO)?;
        let mut values = store::target_fields(recur.scope, target);
        values.push(("note".into(), memo.to_string().into()));
        insert_preserving(&self.doc.get_map(store::NOTES), &record_key(id, target.key), values)?;
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()?;
        Ok(memo)
    }

    pub fn write_occurrence_note(&mut self, id: NoteId, date: Date) -> Result<NoteId, Error> {
        let target = self.occurrence_target_on(id, date, date)?;
        self.write_occurrence_note_target(id, target)
    }

    pub(crate) fn with_occurrences<T>(
        &self,
        id: NoteId,
        today: Date,
        keep_note: impl Fn(NoteId) -> bool,
        read: impl FnOnce(&Occurrences<'_>) -> T,
    ) -> Option<T> {
        let action = self.note(id)?.action?;
        let recur = action.recur?;
        let days_off = self.days_off_set();
        let mut occurrences = Occurrences::new(&recur, &days_off, today);
        occurrences.ignored = self
            .hierarchy()
            .ancestors(id)
            .any(|ancestor| self.note(ancestor).and_then(|note| note.action?.recur).is_some());
        occurrences.notes.retain(|_, note| keep_note(*note));
        Some(read(&occurrences))
    }

    pub fn current_occurrence(&self, id: NoteId, today: Date) -> Option<Occurrence> {
        self.with_occurrences(id, today, |note| !self.is_deleted(note), |o| o.current())?
    }

    pub fn occurrence_summary(&self, id: NoteId, today: Date) -> Option<OccurrenceSummary> {
        self.with_occurrences(id, today, |note| !self.is_deleted(note), |o| o.summary())
    }

    pub fn occurrences(
        &self,
        id: NoteId,
        today: Date,
        past: u32,
        upcoming: u32,
    ) -> Vec<Occurrence> {
        self.with_occurrences(
            id,
            today,
            |note| !self.is_deleted(note),
            |o| o.history(past as usize, upcoming as usize),
        )
        .unwrap_or_default()
    }

    pub fn preview_dates(
        &self,
        settings: &RecurrenceSettings,
        from: Date,
        count: u32,
    ) -> Vec<Date> {
        if !settings.rule.is_valid() {
            return Vec::new();
        }
        let recur = Recurrence::new(NoteId::generate(), settings.clone(), 0, settings.start);
        let days_off = self.days_off_set();
        let occurrences = Occurrences::new(&recur, &days_off, from);
        let first = settings.rule.period_start(days_off.before(&settings.days_off, from));
        occurrences
            .dates(first.max(settings.start))
            .filter_map(|date| occurrences.on(date))
            .filter(|&date| date >= from)
            .take(count as usize)
            .collect()
    }

    fn store_step(
        &self,
        id: NoteId,
        context: OccurrenceContext,
        done: bool,
        at: Option<i64>,
    ) -> Result<(), Error> {
        let recur = self.validate_target(context.series, context.target)?;
        let map = self.doc.get_map(store::STEPS);
        let key = record_key(id, context.target.key);
        if done {
            let mut values = store::target_fields(recur.scope, context.target);
            values.extend([
                ("at".into(), at.unwrap_or_else(|| self.now()).into()),
                ("series".into(), context.series.to_string().into()),
            ]);
            insert_preserving(&map, &key, values)
        } else {
            map.delete(&key).map_err(loro_error)
        }
    }

    pub fn check_action_in(
        &mut self,
        id: NoteId,
        done: bool,
        today: Date,
        context: Option<OccurrenceContext>,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let action = self.note(id).and_then(|note| note.action).ok_or(Error::NotAnAction(id))?;
        let ancestor = self
            .hierarchy()
            .ancestors(id)
            .filter(|&ancestor| self.note(ancestor).and_then(|note| note.action?.recur).is_some())
            .last();
        if let Some(context) = context {
            if ancestor.unwrap_or(id) != context.series {
                return Err(Error::InvalidOccurrence);
            }
            if context.series == id {
                return self.handle_occurrence_target(
                    id,
                    context.target,
                    done.then_some(OccurrenceState::Done),
                    today,
                );
            }
            self.store_step(id, context, done, None)?;
            self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
            return self.commit();
        }
        if ancestor.is_some() || action.recur.as_ref().is_some_and(|recur| recur.stopped.is_none())
        {
            return Err(Error::InvalidOccurrence);
        }
        self.change_action(
            id,
            crate::ActionChange::State(if done { ActionState::Done } else { ActionState::Open }),
        )
    }

    pub fn check_action(&mut self, id: NoteId, done: bool, today: Date) -> Result<(), Error> {
        let raw = self.note(id).and_then(|note| note.action).ok_or(Error::NotAnAction(id))?;
        let mut context = self.note_on(id, today).and_then(|note| note.action?.occurrence);
        if context.is_none()
            && !done
            && let Some(recur) = raw.recur
            && recur.stopped.is_none()
            && let Some(last) = recur.latest_handling()
        {
            context = Some(OccurrenceContext { series: id, target: last.target });
        }
        if context.is_none()
            && self
                .hierarchy()
                .ancestors(id)
                .any(|ancestor| self.note(ancestor).and_then(|note| note.action?.recur).is_some())
        {
            return Ok(());
        }
        self.check_action_in(id, done, today, context)
    }

    pub(crate) fn resolve_recurrence(
        &self,
        notes: &mut HashMap<NoteId, Note>,
        today: Date,
    ) -> HashMap<NoteId, Standing> {
        self.resolve_recurrence_in(notes, today, &self.hierarchy())
    }

    fn resolve_recurrence_in(
        &self,
        notes: &mut HashMap<NoteId, Note>,
        today: Date,
        hierarchy: &Hierarchy,
    ) -> HashMap<NoteId, Standing> {
        let recurring: Vec<_> = notes
            .values()
            .filter(|note| note.action.as_ref().is_some_and(|action| action.recur.is_some()))
            .map(|note| note.id)
            .collect();
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
            let plain_closed = action.closed;
            action.due = None;
            action.defer = None;
            action.state =
                if recur.stopped.is_some() { ActionState::Done } else { ActionState::Open };
            action.closed = recur.stopped;
            let waiting = current.map_or(0, |current| occurrences.waiting(&current));
            if let Some(current) = current {
                action.due = current.due;
                action.defer = current.defer;
                action.occurrence = Some(OccurrenceContext { series: id, target: current.target });
            } else if occurrences.summary().state == SeriesState::Exhausted {
                action.state = ActionState::Done;
                action.closed = recur.latest_handling().map(|entry| entry.at).or(plain_closed);
                action.occurrence = recur
                    .latest_handling()
                    .map(|entry| OccurrenceContext { series: id, target: entry.target });
            }
            standings.insert(id, Standing { waiting });
        }
        let contexts: HashMap<_, _> = standings
            .keys()
            .filter_map(|id| Some((*id, notes.get(id)?.action.as_ref()?.occurrence?)))
            .collect();
        for (&id, note) in notes.iter_mut() {
            let Some(ancestor) = hierarchy.ancestors(id).find(|id| standings.contains_key(id))
            else {
                continue;
            };
            let Some(action) = note.action.as_mut() else { continue };
            action.recur = None;
            action.state = ActionState::Open;
            action.closed = None;
            if let Some(context) = contexts.get(&ancestor) {
                action.occurrence = Some(*context);
                if let Some(done) = action.occurrences.iter().find(|entry| {
                    entry.key == Some(context.target.key) && entry.series == Some(ancestor)
                }) {
                    action.state = ActionState::Done;
                    action.closed = Some(done.at);
                }
            }
        }
        standings
    }

    pub fn notes_on(&self, notes: Vec<Note>, today: Date) -> Vec<Note> {
        let hierarchy = self.hierarchy();
        let mut all: HashMap<_, _> = notes.iter().map(|note| (note.id, note.clone())).collect();
        for ancestor in notes.iter().flat_map(|note| hierarchy.ancestors(note.id)) {
            if let std::collections::hash_map::Entry::Vacant(entry) = all.entry(ancestor)
                && let Some(note) = self.note(ancestor)
            {
                entry.insert(note);
            }
        }
        self.resolve_recurrence_in(&mut all, today, &hierarchy);
        notes.into_iter().map(|note| all.get(&note.id).cloned().unwrap_or(note)).collect()
    }

    pub fn note_on(&self, id: NoteId, today: Date) -> Option<Note> {
        self.notes_on(vec![self.note(id)?], today).into_iter().next()
    }

    pub(crate) fn action_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.note_map(id)?.get(ACTION) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotAnAction(id))
    }

    pub(crate) fn recur_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.action_map(id)?.get(RECUR) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotRecurring(id))
    }
}

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

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Missed {
    All,
    #[default]
    Latest,
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
    Open,
    Skipped,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceTarget {
    pub origin: Date,
    pub key: OccurrenceId,
    pub date: Date,
    pub cycle: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceContext {
    pub series: NoteId,
    pub target: OccurrenceTarget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct HandledOccurrence {
    pub at: i64,
    pub date: Date,
    pub day: Date,
    pub defer: Option<Date>,
    pub due: Option<Date>,
    pub early: bool,
    pub key: OccurrenceId,
    pub on: Date,
    pub order: u64,
    pub peer: u64,
    pub scope: OccurrenceId,
    pub state: OccurrenceState,
    pub target: OccurrenceTarget,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct MovedOccurrence {
    pub date: Date,
    pub key: OccurrenceId,
    pub rule: Option<String>,
    pub scope: OccurrenceId,
    pub start: Option<Date>,
    pub target: OccurrenceTarget,
    pub to: Date,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceNote {
    pub date: Date,
    pub key: OccurrenceId,
    pub note: NoteId,
    pub target: OccurrenceTarget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct StepOccurrence {
    pub at: i64,
    pub date: Date,
    pub key: Option<OccurrenceId>,
    pub series: Option<NoteId>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct RecurrenceSettings {
    pub rule: Rule,
    pub start: Date,
    pub missed: Missed,
    pub due: Option<i32>,
    pub defer: Option<i32>,
    pub days_off: DaysOffUse,
}

impl RecurrenceSettings {
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

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Recurrence {
    pub settings: RecurrenceSettings,
    pub series: NoteId,
    pub scope: OccurrenceId,
    pub generation: u64,
    pub not_before: Date,
    pub stopped: Option<i64>,
    pub handled: Vec<HandledOccurrence>,
    pub moved: Vec<MovedOccurrence>,
    pub notes: Vec<OccurrenceNote>,
    pub past: Vec<HandledOccurrence>,
    pub reopened: Vec<HandledOccurrence>,
    pub reaches: Vec<OccurrenceReach>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceReach {
    pub date: Date,
    pub extra: Vec<Date>,
    pub on: Date,
    pub order: u64,
    pub peer: u64,
    pub pending: Vec<Date>,
    pub through: Date,
}

impl OccurrenceReach {
    pub(crate) fn from_value(entry: &HandledOccurrence, value: Option<&LoroValue>) -> Option<Self> {
        let map = fields(value?)?;
        let dates = |key| {
            let mut dates: Vec<Date> = match map.get(key) {
                Some(LoroValue::List(values)) => {
                    values.iter().filter_map(|value| string(value.clone())?.parse().ok()).collect()
                }
                _ => Vec::new(),
            };
            dates.sort_unstable();
            dates.dedup();
            dates
        };
        Some(Self {
            date: entry.date,
            extra: dates("extra"),
            on: date(map, "on")?,
            order: entry.order,
            peer: entry.peer,
            pending: dates("pending"),
            through: date(map, "through")?,
        })
    }

    pub(crate) fn to_value(&self) -> LoroValue {
        let dates = |dates: &[Date]| -> LoroValue {
            LoroValue::List(dates.iter().map(|date| date.to_string().into()).collect())
        };
        LoroValue::Map(
            vec![
                ("extra".into(), dates(&self.extra)),
                ("on".into(), self.on.to_string().into()),
                ("pending".into(), dates(&self.pending)),
                ("through".into(), self.through.to_string().into()),
            ]
            .into(),
        )
    }

    fn misses(&self, date: Date) -> bool {
        (date <= self.through && self.pending.binary_search(&date).is_err())
            || self.extra.binary_search(&date).is_ok()
    }
}

impl Recurrence {
    fn new(
        series: NoteId,
        settings: RecurrenceSettings,
        generation: u64,
        not_before: Date,
    ) -> Self {
        let kind = if settings.missed == Missed::After { "after" } else { "calendar" };
        let scope = OccurrenceId::derive(
            series,
            &format!("{generation}|{kind}|{}|{}", settings.start, settings.rule.calendar_text()),
        );
        Self {
            settings,
            series,
            scope,
            generation,
            not_before,
            stopped: None,
            handled: Vec::new(),
            moved: Vec::new(),
            notes: Vec::new(),
            past: Vec::new(),
            reopened: Vec::new(),
            reaches: Vec::new(),
        }
    }

    pub(crate) fn from_value(
        value: Option<&LoroValue>,
        series: NoteId,
        stopped: Option<i64>,
    ) -> Option<Self> {
        let map = fields(value?)?;
        let calendar = map.get("calendar").and_then(fields).unwrap_or(map);
        let offset = |key| map.get(key).and_then(|value| integer(value.clone())?.try_into().ok());
        let settings = RecurrenceSettings {
            rule: Rule::parse(&text(calendar, "rule")?)?,
            start: date(calendar, "start")?,
            missed: Missed::parse(text(calendar, "missed").or_else(|| text(map, "missed"))),
            due: offset("due"),
            defer: offset("defer"),
            days_off: DaysOffUse::from_map(map),
        };
        let not_before = date(calendar, "not_before").unwrap_or(settings.start);
        let mut recur =
            Self::new(series, settings, number(calendar, "generation") as u64, not_before);
        recur.stopped = stopped;
        Some(recur)
    }

    pub(crate) fn calendar_value(&self) -> LoroValue {
        LoroValue::Map(
            vec![
                ("generation".into(), (self.generation as i64).into()),
                ("missed".into(), self.settings.missed.as_str().into()),
                ("not_before".into(), self.not_before.to_string().into()),
                ("rule".into(), self.settings.rule.to_string().into()),
                ("start".into(), self.settings.start.to_string().into()),
            ]
            .into(),
        )
    }

    pub(crate) fn target(&self, date: Date, cycle: Option<u32>) -> OccurrenceTarget {
        self.target_from(date, cycle, self.settings.start)
    }

    pub(crate) fn legacy_scope(&self) -> Option<OccurrenceId> {
        let mut settings = self.settings.clone();
        if settings.rule.frequency != crate::Frequency::Weekly
            || settings.rule.interval != 1
            || settings.rule.positions.is_empty()
            || settings.rule.week_start == 0
        {
            return None;
        }
        settings.rule.week_start = 0;
        Some(Self::new(self.series, settings, self.generation, self.not_before).scope)
    }

    pub(crate) fn target_from(
        &self,
        date: Date,
        cycle: Option<u32>,
        origin: Date,
    ) -> OccurrenceTarget {
        let slot = cycle.map_or_else(|| date.to_string(), |cycle| format!("cycle:{cycle}"));
        OccurrenceTarget {
            origin,
            key: OccurrenceId::derive(self.series, &format!("{}|{slot}", self.scope)),
            date,
            cycle,
        }
    }

    pub(crate) fn legacy_nominal(&self, selected: Date, days: &DaysOffSet) -> Date {
        if self.settings.rule.positions.is_empty() {
            return selected;
        }
        let from = self.settings.rule.period_start(selected);
        let skip = Box::new(|date| days.skipped_through(&self.settings.days_off, date));
        self.settings
            .rule
            .slots(self.settings.start, from, skip)
            .take_while(|slot| slot.nominal <= self.settings.rule.period_end(selected))
            .find(|slot| slot.on == Some(selected))
            .map_or(selected, |slot| slot.nominal)
    }

    pub(crate) fn legacy_on(&self, date: Date, days: &DaysOffSet) -> Date {
        days.moved(&self.settings.days_off, date).unwrap_or(date)
    }

    fn latest_handling(&self) -> Option<&HandledOccurrence> {
        self.handled
            .iter()
            .max_by_key(|entry| (entry.order, entry.peer, entry.at, entry.day, entry.key))
    }
}

pub(crate) fn step_occurrences(value: Option<&LoroValue>) -> Vec<StepOccurrence> {
    let Some(LoroValue::Map(map)) = value else { return Vec::new() };
    let mut entries: Vec<_> = map
        .iter()
        .filter_map(|(date, at)| {
            Some(StepOccurrence {
                at: integer(at.clone())?,
                date: date.parse().ok()?,
                key: None,
                series: None,
            })
        })
        .collect();
    entries.sort_by_key(|entry| entry.date);
    entries
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum OccurrenceStatus {
    Current,
    Upcoming,
    Done,
    Skipped,
    Missed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum SeriesState {
    Active,
    Blocked,
    Exhausted,
    Ignored,
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Occurrence {
    pub target: OccurrenceTarget,
    pub date: Date,
    pub on: Date,
    pub defer: Option<Date>,
    pub due: Option<Date>,
    pub editable: bool,
    pub status: OccurrenceStatus,
    pub handled: Option<HandledOccurrence>,
    pub moved: bool,
    pub note: Option<NoteId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct OccurrenceSummary {
    pub current: Option<Occurrence>,
    pub missed: u32,
    pub state: SeriesState,
    pub waiting: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Standing {
    pub(crate) waiting: u32,
}

struct Cutoff {
    from: Date,
    reaches: Vec<OccurrenceReach>,
}

pub(crate) struct Occurrences<'a> {
    recur: &'a Recurrence,
    days_off: &'a DaysOffSet,
    today: Date,
    stopped: bool,
    ignored: bool,
    moved: HashMap<Date, Date>,
    handled: HashMap<OccurrenceId, HandledOccurrence>,
    notes: HashMap<OccurrenceId, NoteId>,
    on_cache: RefCell<HashMap<Date, Option<Date>>>,
    cutoff: OnceCell<Cutoff>,
}

impl<'a> Occurrences<'a> {
    pub(crate) fn new(recur: &'a Recurrence, days_off: &'a DaysOffSet, today: Date) -> Self {
        Self {
            recur,
            days_off,
            today,
            stopped: recur.stopped.is_some(),
            ignored: false,
            moved: recur.moved.iter().map(|entry| (entry.date, entry.to)).collect(),
            handled: recur.handled.iter().map(|entry| (entry.key, *entry)).collect(),
            notes: recur.notes.iter().map(|entry| (entry.key, entry.note)).collect(),
            on_cache: RefCell::default(),
            cutoff: OnceCell::new(),
        }
    }

    fn settings(&self) -> &RecurrenceSettings {
        &self.recur.settings
    }

    fn nominal_current(&self) -> Option<Occurrence> {
        let available = DaysOffSet::default();
        Occurrences::new(self.recur, &available, self.today).current()
    }

    fn skipped_through(&self, date: Date) -> Option<Date> {
        let last = self.days_off.skipped_through(&self.settings().days_off, date)?;
        Some(
            self.moved
                .keys()
                .filter(|&&moved| moved > date && moved <= last)
                .min()
                .map_or(last, |moved| moved.add_days(-1)),
        )
    }

    fn dates(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        self.settings()
            .rule
            .slots(
                self.settings().start,
                from.max(self.recur.not_before),
                Box::new(|date| self.skipped_through(date)),
            )
            .filter(|slot| slot.on.is_some() || self.moved.contains_key(&slot.nominal))
            .map(|slot| slot.nominal)
    }

    fn on(&self, nominal: Date) -> Option<Date> {
        if let Some(&date) = self.moved.get(&nominal) {
            return Some(date);
        }
        if let Some(&date) = self.on_cache.borrow().get(&nominal) {
            return date;
        }
        let selected = self.settings().rule.slot_on(self.settings().start, nominal, &|date| {
            self.days_off.skipped_through(&self.settings().days_off, date)
        });
        let on =
            selected.and_then(|selected| self.days_off.moved(&self.settings().days_off, selected));
        self.on_cache.borrow_mut().insert(nominal, on);
        on
    }

    fn started(&self, date: Date) -> Option<Date> {
        self.on(date).map(|on| on.add_days(self.settings().defer.unwrap_or(0)))
    }

    fn occurrence_target(&self, target: OccurrenceTarget, status: OccurrenceStatus) -> Occurrence {
        let handled = self
            .handled
            .get(&target.key)
            .copied()
            .or_else(|| self.recur.past.iter().find(|entry| entry.key == target.key).copied());
        let moved = self.recur.moved.iter().find(|entry| entry.key == target.key);
        let on = handled
            .map(|entry| entry.on)
            .or_else(|| moved.map(|entry| entry.to))
            .or_else(|| {
                if target.cycle.is_some() {
                    self.settings()
                        .rule
                        .slot_on(target.origin, target.date, &|date| {
                            self.days_off.skipped_through(&self.settings().days_off, date)
                        })
                        .and_then(|date| self.days_off.moved(&self.settings().days_off, date))
                } else {
                    self.on(target.date)
                }
            })
            .unwrap_or(target.date);
        let (defer, due) = handled.map_or_else(
            || {
                (
                    Some(on.add_days(self.settings().defer.unwrap_or(0))),
                    self.settings().due.map(|offset| on.add_days(offset)),
                )
            },
            |entry| (entry.defer, entry.due),
        );
        let status = match handled.map(|entry| entry.state) {
            Some(OccurrenceState::Done) => OccurrenceStatus::Done,
            Some(OccurrenceState::Skipped) => OccurrenceStatus::Skipped,
            _ => status,
        };
        Occurrence {
            target,
            date: target.date,
            on,
            defer,
            due,
            status,
            handled,
            editable: self.recur.target(target.date, target.cycle).key == target.key,
            moved: moved.is_some(),
            note: self.notes.get(&target.key).copied(),
        }
    }

    fn occurrence(&self, date: Date, status: OccurrenceStatus) -> Occurrence {
        self.occurrence_target(self.recur.target(date, None), status)
    }

    pub(crate) fn reach(
        &self,
        target: OccurrenceTarget,
        on: Date,
        order: u64,
        peer: u64,
    ) -> OccurrenceReach {
        let through = on.add_days(-self.settings().defer.unwrap_or(0));
        let end = self.settings().rule.period_end(through);
        let rule = &self.settings().rule;
        let nominal = rule
            .slots(self.settings().start, self.recur.not_before, Box::new(|_| None))
            .map(|slot| slot.nominal)
            .take_while(|&date| date <= end);
        let moved = self.moved.keys().copied().filter(|&date| {
            date >= self.recur.not_before
                && rule
                    .slots(self.settings().start, date, Box::new(|_| None))
                    .next()
                    .is_some_and(|slot| slot.nominal == date)
        });
        let mut candidates: Vec<_> = nominal.chain(moved).collect();
        candidates.sort_unstable();
        candidates.dedup();
        let through = if rule.count.is_some() || rule.until.is_some() {
            candidates
                .iter()
                .copied()
                .filter(|&date| date <= through)
                .max()
                .unwrap_or(self.settings().start.add_days(-1))
        } else {
            through
        };
        let pending = candidates
            .iter()
            .copied()
            .filter(|&date| date <= through && self.started(date).is_none_or(|start| start > on))
            .collect();
        let extra = candidates
            .into_iter()
            .filter(|&date| date > through && self.started(date).is_some_and(|start| start <= on))
            .collect();
        OccurrenceReach { date: target.date, extra, on, order, peer, pending, through }
    }

    fn cutoff(&self) -> &Cutoff {
        self.cutoff.get_or_init(|| {
            if self.settings().missed != Missed::Latest {
                return Cutoff { from: self.settings().start, reaches: Vec::new() };
            }
            let reaches: Vec<_> =
                self.recur.reaches.iter().filter(|reach| reach.on <= self.today).cloned().collect();
            let mut from = self.settings().start;
            for reach in &reaches {
                let boundary = reach.pending.first().copied().unwrap_or(reach.through.add_days(1));
                from = from.max(self.settings().rule.period_start(boundary));
            }
            for entry in &self.recur.reopened {
                from = from.min(entry.date)
            }
            Cutoff { from, reaches }
        })
    }

    fn is_missed(&self, date: Date) -> bool {
        let key = self.recur.target(date, None).key;
        let reopened = self.recur.reopened.iter().find(|entry| entry.key == key);
        self.cutoff().reaches.iter().any(|reach| {
            reach.misses(date)
                && reopened
                    .is_none_or(|entry| (entry.order, entry.peer) <= (reach.order, reach.peer))
        })
    }

    fn unhandled(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        self.dates(from)
            .filter(|&date| !self.handled.contains_key(&self.recur.target(date, None).key))
    }

    fn open(&self, from: Date) -> impl Iterator<Item = Date> + '_ {
        self.unhandled(from.max(self.cutoff().from)).filter(|&date| !self.is_missed(date))
    }

    fn gives(&self, date: Date) -> bool {
        self.dates(date).next() == Some(date)
    }

    fn moved_open(&self) -> impl Iterator<Item = Date> + '_ {
        self.moved.keys().copied().filter(|&date| {
            self.gives(date)
                && !self.handled.contains_key(&self.recur.target(date, None).key)
                && !self.is_missed(date)
        })
    }

    fn soonest(&self) -> Option<Date> {
        let mut best = self.moved_open().filter_map(|date| Some((self.started(date)?, date))).min();
        for date in self.open(self.settings().start) {
            let first = self
                .settings()
                .rule
                .period_start(date)
                .add_days(self.settings().defer.unwrap_or(0));
            if best.is_some_and(|(started, _)| first > started) {
                break;
            }
            if let Some(started) = self.started(date) {
                let candidate = (started, date);
                best = Some(best.map_or(candidate, |best| best.min(candidate)));
            } else if best.is_none() {
                break;
            }
        }
        best.map(|(_, date)| date)
    }

    fn latest_started(&self) -> Option<Date> {
        let last = self.today.add_days(-self.settings().defer.unwrap_or(0));
        let end = self.settings().rule.period_end(last);
        let scan = |from| {
            self.open(from)
                .take_while(|&date| date <= end)
                .filter_map(|date| Some((self.started(date)?, date)))
                .filter(|&(start, _)| start <= self.today)
                .max()
        };
        let window =
            self.settings().rule.period_start(last.add_days(-31)).max(self.settings().start);
        let walked = scan(window).or_else(|| scan(self.settings().start));
        let moved = self
            .moved_open()
            .filter_map(|date| Some((self.started(date)?, date)))
            .filter(|&(start, _)| start <= self.today)
            .max();
        walked.max(moved).map(|(_, date)| date)
    }

    fn after_target(&self, latest: Option<&HandledOccurrence>) -> Option<OccurrenceTarget> {
        let settings = self.settings();
        if settings.rule.count.is_some_and(|count| self.recur.handled.len() as u32 >= count) {
            return None;
        }
        let reopened = self.recur.reopened.iter().min_by_key(|entry| entry.target.cycle);
        let cycle = reopened.and_then(|entry| entry.target.cycle).unwrap_or_else(|| {
            self.recur
                .handled
                .iter()
                .chain(&self.recur.reopened)
                .filter_map(|entry| entry.target.cycle)
                .max()
                .map_or(0, |cycle| cycle + 1)
        });
        let key = self.recur.target(settings.start, Some(cycle)).key;
        let mut rule = settings.rule.clone();
        rule.count = None;
        let origin = reopened.map_or_else(
            || latest.map_or(settings.start, |entry| entry.day.max(settings.start)),
            |entry| entry.target.origin,
        );
        let from = reopened
            .map_or_else(
                || latest.map_or(origin, |entry| entry.day.add_days(1).max(origin)),
                |entry| entry.target.date,
            )
            .max(self.recur.not_before);
        if let Some(moved) = self.recur.moved.iter().find(|entry| entry.key == key)
            && rule.all_dates(origin, moved.date).next() == Some(moved.date)
        {
            return Some(moved.target);
        }
        let slot = rule
            .slots(
                origin,
                from,
                Box::new(|date| self.days_off.skipped_through(&settings.days_off, date)),
            )
            .find(|slot| {
                slot.on.and_then(|date| self.days_off.moved(&settings.days_off, date)).is_some()
            })?;
        Some(self.recur.target_from(slot.nominal, Some(cycle), origin))
    }

    pub(crate) fn current(&self) -> Option<Occurrence> {
        if self.stopped || self.ignored {
            return None;
        }
        if self.settings().missed == Missed::After {
            let target = self.after_target(self.recur.latest_handling())?;
            return Some(self.occurrence_target(target, OccurrenceStatus::Current));
        }
        let date = if self.settings().missed == Missed::Latest {
            self.latest_started().or_else(|| self.soonest())?
        } else {
            self.soonest()?
        };
        Some(self.occurrence(date, OccurrenceStatus::Current))
    }

    fn is_past(&self, date: Date, current: Option<&Occurrence>) -> bool {
        self.is_missed(date)
            || (self.settings().missed == Missed::Latest
                && self.started(date).is_some_and(|start| {
                    start <= self.today
                        && current.is_none_or(|current| {
                            (start, date) < (current.defer.unwrap_or(current.on), current.date)
                        })
                }))
    }

    fn waiting(&self, current: &Occurrence) -> u32 {
        if self.settings().missed != Missed::All {
            return 0;
        }
        let offset = self.settings().due.unwrap_or(0);
        let last = self.today.add_days(-offset);
        let end = self.settings().rule.period_end(last);
        let due = |date| {
            date != current.date
                && self.on(date).is_some_and(|on| on.add_days(offset) <= self.today)
        };
        let mut count = self
            .moved_open()
            .filter(|&date| date > end && due(date))
            .take(MAX_COUNT as usize)
            .count() as u32;
        for date in self.open(self.settings().start).take_while(|&date| date <= end) {
            if due(date) {
                count += 1
            }
            if count >= MAX_COUNT {
                break;
            }
        }
        count.min(MAX_COUNT)
    }

    fn past_dates(&self, current: Option<&Occurrence>, count: usize) -> Vec<Date> {
        if self.settings().missed != Missed::Latest || count == 0 {
            return Vec::new();
        }
        let end = self
            .settings()
            .rule
            .period_end(self.today.add_days(-self.settings().defer.unwrap_or(0)));
        let moved: Vec<_> = self
            .moved
            .keys()
            .copied()
            .filter(|&date| {
                self.gives(date)
                    && !self.handled.contains_key(&self.recur.target(date, None).key)
                    && self.started(date).is_some_and(|start| start <= self.today)
                    && self.is_past(date, current)
            })
            .collect();
        let mut span = 64i32;
        loop {
            let from = if span >= end.days_since(self.settings().start) {
                self.settings().start
            } else {
                end.add_days(-span)
            };
            let mut found: Vec<_> = self
                .unhandled(from)
                .take_while(|&date| date <= end)
                .filter(|&date| {
                    self.started(date).is_some_and(|start| start <= self.today)
                        && self.is_past(date, current)
                })
                .chain(moved.iter().copied())
                .collect();
            found.sort_unstable();
            found.dedup();
            found.sort_by_key(|&date| std::cmp::Reverse((self.started(date), date)));
            if found.len() >= count || from <= self.settings().start {
                found.truncate(count);
                return found;
            }
            span = span.saturating_mul(4);
        }
    }

    pub(crate) fn history(&self, past: usize, upcoming: usize) -> Vec<Occurrence> {
        let current = self.current();
        let mut earlier: Vec<_> = self
            .recur
            .handled
            .iter()
            .chain(&self.recur.past)
            .filter(|entry| entry.on <= self.today)
            .map(|entry| self.occurrence_target(entry.target, OccurrenceStatus::Done))
            .collect();
        earlier.extend(
            self.past_dates(current.as_ref(), past)
                .into_iter()
                .map(|date| self.occurrence(date, OccurrenceStatus::Missed)),
        );
        earlier.sort_by_key(|occurrence| {
            std::cmp::Reverse((
                occurrence.on,
                occurrence.date,
                occurrence.target.cycle,
                occurrence.target.key,
            ))
        });
        earlier.truncate(past);
        let mut later = Vec::new();
        if let Some(current) = current {
            later.push(current);
            if self.settings().missed == Missed::After {
                let mut last = current;
                let mut projected = self.recur.clone();
                for _ in 0..upcoming {
                    let entry = HandledOccurrence {
                        at: 0,
                        date: last.date,
                        day: last.on.max(self.today),
                        defer: last.defer,
                        due: last.due,
                        early: false,
                        key: last.target.key,
                        on: last.on,
                        order: 0,
                        peer: 0,
                        scope: self.recur.scope,
                        state: OccurrenceState::Done,
                        target: last.target,
                    };
                    projected.reopened.retain(|reopened| reopened.key != entry.key);
                    projected.handled.push(entry);
                    let next = Occurrences::new(&projected, self.days_off, self.today);
                    let Some(target) = next.after_target(Some(&entry)) else { break };
                    last = self.occurrence_target(target, OccurrenceStatus::Upcoming);
                    later.push(last);
                }
            } else if upcoming > 0 {
                let mut next: Vec<_> = self
                    .moved_open()
                    .filter(|&date| date != current.date && !self.is_past(date, Some(&current)))
                    .map(|date| self.occurrence(date, OccurrenceStatus::Upcoming))
                    .collect();
                next.sort_by_key(|occurrence| (occurrence.on, occurrence.date));
                next.truncate(upcoming);
                for date in self.open(self.settings().start) {
                    if next.len() == upcoming
                        && self.settings().rule.period_start(date) > next[upcoming - 1].on
                    {
                        break;
                    }
                    if date == current.date
                        || self.moved.contains_key(&date)
                        || self.is_past(date, Some(&current))
                    {
                        continue;
                    }
                    if self.on(date).is_none() {
                        break;
                    }
                    next.push(self.occurrence(date, OccurrenceStatus::Upcoming));
                    next.sort_by_key(|occurrence| (occurrence.on, occurrence.date));
                    next.truncate(upcoming);
                }
                later.extend(next);
            }
            if self.settings().missed != Missed::After {
                later.extend(
                    self.moved_open()
                        .filter(|&date| date < current.date && !self.is_past(date, Some(&current)))
                        .map(|date| self.occurrence(date, OccurrenceStatus::Upcoming)),
                );
            }
        }
        later.extend(
            self.recur
                .handled
                .iter()
                .chain(&self.recur.past)
                .filter(|entry| entry.on > self.today)
                .map(|entry| self.occurrence_target(entry.target, OccurrenceStatus::Done)),
        );
        later.sort_by_key(|occurrence| {
            (
                occurrence.status != OccurrenceStatus::Current,
                occurrence.on,
                occurrence.date,
                occurrence.target.cycle,
                occurrence.target.key,
            )
        });
        later.dedup_by_key(|occurrence| occurrence.target.key);
        later.extend(earlier);
        later
    }

    pub(crate) fn summary(&self) -> OccurrenceSummary {
        let current = self.current();
        let state = if self.ignored {
            SeriesState::Ignored
        } else if self.stopped {
            SeriesState::Stopped
        } else if current.is_some() {
            SeriesState::Active
        } else {
            if self.nominal_current().is_none() {
                SeriesState::Exhausted
            } else {
                SeriesState::Blocked
            }
        };
        OccurrenceSummary {
            current,
            missed: self.past_dates(current.as_ref(), MAX_COUNT as usize).len() as u32,
            state,
            waiting: current.map_or(0, |current| self.waiting(&current)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionView, DayOffEffect, DaysOff, DaysOffKind};

    #[test]
    fn occurrence_identifier_vectors() {
        let id = "01j8000000e008000000000000".parse().unwrap();
        let start = date("2026-11-01");
        for (rule, missed, generation, slot, cycle, scope, key) in [
            (
                "FREQ=DAILY;INTERVAL=1;WKST=SU;COUNT=3",
                Missed::Latest,
                0,
                start,
                None,
                "5t711daa7qf144d6cpekwspv9q",
                "5zgfvn6n7a367669ys6yhbtvft",
            ),
            (
                "BYDAY=TU,MO,TU;FREQ=WEEKLY;WKST=SU;INTERVAL=2;UNTIL=20270101",
                Missed::All,
                0,
                date("2026-11-02"),
                None,
                "3ak4zczn8np349jkk2fpa3ya7g",
                "3dms9ff48wyzwekr2x2y6smhb9",
            ),
            (
                "FREQ=MONTHLY;BYDAY=1TU,2MO,-1MO,-1MO;WKST=SU",
                Missed::Latest,
                2,
                date("2026-11-09"),
                None,
                "1rm15c2w8cp9cp05qv68shr80b",
                "2mjzc397hcg3qepdfkypf734xx",
            ),
            (
                "FREQ=DAILY;INTERVAL=7;COUNT=4",
                Missed::After,
                3,
                start,
                Some(0),
                "7jqrad31qpdmxd7s5wk9a2np7k",
                "0cdqgh32y95tq3ysp3h9r6ak95",
            ),
        ] {
            let mut settings = RecurrenceSettings::new(Rule::parse(rule).unwrap(), start);
            settings.missed = missed;
            let recur = Recurrence::new(id, settings, generation, start);
            assert_eq!(recur.scope.to_string(), scope);
            assert_eq!(recur.target(slot, cycle).key.to_string(), key);
        }
    }

    #[test]
    fn handling_order_survives_a_backwards_clock_change() {
        let mut future_clock = Library::in_memory_at(i64::MAX);
        let start = date("2026-11-01");
        let id = future_clock.create_task("Water", None).unwrap();
        let mut settings =
            RecurrenceSettings::new(Rule::parse("FREQ=DAILY;INTERVAL=7").unwrap(), start);
        settings.missed = Missed::After;
        future_clock.set_recurrence(id, Some(settings), start).unwrap();
        future_clock.check_action(id, true, start).unwrap();
        let mut normal_clock = Library::in_memory();
        normal_clock.import_snapshot(&future_clock.export_snapshot().unwrap()).unwrap();
        normal_clock.check_action(id, true, start.add_days(1)).unwrap();
        assert_eq!(normal_clock.current_occurrence(id, start).unwrap().on, start.add_days(8));
    }

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
        let id = weekly(&mut library, Missed::Latest);
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

    #[test]
    fn handled_entries_keep_their_snapshot_and_unknown_fields_before_an_edit() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        let mut settings = library.note(id).unwrap().action.unwrap().recur.unwrap().settings;
        settings.defer = Some(-14);
        let today = date("2026-10-05");
        library.set_recurrence(id, Some(settings.clone()), today).unwrap();
        let key = "2026-10-19";
        library.handle_occurrence(id, date(key), Some(OccurrenceState::Done), today).unwrap();
        let target = library.current_occurrence(id, today).unwrap().target;
        let key = record_key(id, library.occurrence_target_on(id, date(key), today).unwrap().key);
        let handled = library.doc.get_map(store::HANDLED);
        let LoroValue::Map(values) = handled.get_value() else { panic!() };
        let LoroValue::Map(value) = &values[&key] else { panic!() };
        let mut legacy: Vec<_> = value
            .iter()
            .filter(|(key, _)| key.as_str() != "reach")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        legacy.push(("future".into(), "preserved".into()));
        handled.insert(&key, LoroValue::Map(legacy.into())).unwrap();
        library.commit().unwrap();
        settings.defer = Some(0);
        library.set_recurrence(id, Some(settings), date("2026-10-06")).unwrap();
        assert_eq!(library.current_occurrence(id, today).unwrap().target, target);
        let LoroValue::Map(values) = handled.get_value() else { panic!() };
        let LoroValue::Map(value) = &values[&key] else { panic!() };
        assert_eq!(value.get("future"), Some(&LoroValue::from("preserved")));
    }

    #[test]
    fn moving_an_occurrence_preserves_unknown_override_fields() {
        let mut library = Library::in_memory();
        let id = weekly(&mut library, Missed::All);
        let date = date("2026-10-05");
        library.move_occurrence(id, date, Some(date.add_days(1))).unwrap();
        let moved = library.doc.get_map(store::MOVED);
        let key = record_key(id, library.occurrence_target_on(id, date, date).unwrap().key);
        let Some(value) = moved.get(&key) else { panic!() };
        let LoroValue::Map(value) = value.get_deep_value() else { panic!() };
        let mut fields: Vec<_> =
            value.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
        fields.push(("future".into(), "preserved".into()));
        moved.insert(&key, LoroValue::Map(fields.into())).unwrap();
        library.commit().unwrap();
        library.move_occurrence(id, date, Some(date.add_days(2))).unwrap();
        let Some(value) = moved.get(&key) else { panic!() };
        let LoroValue::Map(value) = value.get_deep_value() else { panic!() };
        assert_eq!(value.get("future"), Some(&LoroValue::from("preserved")));
    }
}
