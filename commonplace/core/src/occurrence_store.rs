// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::{HashMap, HashSet};

use loro::{
    ContainerTrait, Frontiers, ID, JsonMapOp, JsonOpContent, LoroDoc, LoroMap, LoroValue,
    VersionVector,
};

use crate::library::{VERSION, integer, loro_error, string};
use crate::recurring::{
    HandledOccurrence, MovedOccurrence, OccurrenceNote, OccurrenceReach, OccurrenceState,
    OccurrenceTarget, Occurrences, StepOccurrence,
};
use crate::{Date, Error, Library, Note, NoteId, OccurrenceId};

pub(crate) const GENERATIONS: &str = "recurrence_generations";
pub(crate) const HANDLED: &str = "recurrence_handled";
pub(crate) const MOVED: &str = "recurrence_moved";
pub(crate) const NOTES: &str = "recurrence_notes";
pub(crate) const SETTLED: &str = "recurrence_settled";
pub(crate) const STEPS: &str = "step_occurrences";

type LegacyRecords = HashSet<(&'static str, String)>;

pub(crate) fn last_generation(doc: &LoroDoc, series: NoteId) -> Option<u64> {
    let prefix = format!("{series}/");
    doc.get_map(GENERATIONS).keys().filter_map(|key| key.strip_prefix(&prefix)?.parse().ok()).max()
}

pub(crate) fn remember_generation(
    doc: &LoroDoc,
    series: NoteId,
    generation: u64,
) -> Result<bool, Error> {
    let map = doc.get_map(GENERATIONS);
    let key = format!("{series}/{generation}");
    if map.get(&key).is_some() {
        return Ok(false);
    }
    map.insert(&key, true).map_err(loro_error)?;
    Ok(true)
}

pub(crate) fn record_key(series: NoteId, key: OccurrenceId) -> String {
    format!("{series}/{key}")
}

pub(crate) fn settlement_key(series: NoteId, key: OccurrenceId, order: u64, peer: u64) -> String {
    format!("{series}/{key}/{order}/{peer}")
}

pub(crate) fn next_handling_order(doc: &LoroDoc, series: NoteId) -> u64 {
    let prefix = format!("{series}/");
    let mut order = 0;
    for root in [HANDLED, SETTLED] {
        let LoroValue::Map(records) = doc.get_map(root).get_value() else { continue };
        for (_, value) in records.iter().filter(|(key, _)| key.starts_with(&prefix)) {
            if let Some(map) = fields(value) {
                order = order.max(number(map, "order").max(0) as u64);
            }
        }
    }
    (doc.len_ops() as u64).max(order.saturating_add(1))
}

pub(crate) fn fields(value: &LoroValue) -> Option<&loro::LoroMapValue> {
    match value {
        LoroValue::Map(map) => Some(map),
        _ => None,
    }
}

pub(crate) fn text(map: &loro::LoroMapValue, key: &str) -> Option<String> {
    map.get(key).and_then(|value| string(value.clone()))
}

pub(crate) fn date(map: &loro::LoroMapValue, key: &str) -> Option<Date> {
    text(map, key)?.parse().ok()
}

pub(crate) fn number(map: &loro::LoroMapValue, key: &str) -> i64 {
    map.get(key).and_then(|value| integer(value.clone())).unwrap_or(0)
}

pub(crate) fn target(map: &loro::LoroMapValue) -> Option<OccurrenceTarget> {
    Some(OccurrenceTarget {
        origin: date(map, "origin").or_else(|| date(map, "date"))?,
        key: text(map, "key")?.parse().ok()?,
        date: date(map, "date")?,
        cycle: map.get("cycle").and_then(|value| integer(value.clone())?.try_into().ok()),
    })
}

pub(crate) fn target_fields(
    scope: OccurrenceId,
    target: OccurrenceTarget,
) -> Vec<(String, LoroValue)> {
    let mut fields = vec![
        ("date".into(), target.date.to_string().into()),
        ("identity_version".into(), 7.into()),
        ("key".into(), target.key.to_string().into()),
        ("origin".into(), target.origin.to_string().into()),
        ("scope".into(), scope.to_string().into()),
    ];
    if let Some(cycle) = target.cycle {
        fields.push(("cycle".into(), i64::from(cycle).into()));
    }
    fields
}

pub(crate) fn insert_preserving(
    map: &LoroMap,
    key: &str,
    updates: Vec<(String, LoroValue)>,
) -> Result<(), Error> {
    let mut old: HashMap<String, LoroValue> = map
        .get(key)
        .and_then(|value| fields(&value.get_deep_value()).cloned())
        .map(|map| map.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
        .unwrap_or_default();
    old.extend(updates);
    map.insert(key, LoroValue::Map(old.into_iter().collect())).map_err(loro_error)
}

impl Library {
    pub(crate) fn copy_occurrences(&self, source: &Self, id: NoteId) -> Result<(), Error> {
        let prefix = format!("{id}/");
        for root in [GENERATIONS, HANDLED, MOVED, NOTES, SETTLED, STEPS] {
            let LoroValue::Map(records) = source.doc.get_map(root).get_value() else { continue };
            for (key, value) in records.iter().filter(|(key, _)| key.starts_with(&prefix)) {
                self.doc.get_map(root).insert(key, value.clone()).map_err(loro_error)?;
            }
        }
        Ok(())
    }

    pub(crate) fn hydrate_occurrences(&self, notes: &mut [Note]) {
        let indices: HashMap<NoteId, usize> =
            notes.iter().enumerate().map(|(index, note)| (note.id, index)).collect();
        for root in [HANDLED, MOVED, NOTES, SETTLED, STEPS] {
            let LoroValue::Map(records) = self.doc.get_map(root).get_value() else { continue };
            for (key, value) in records.iter() {
                let Some((series, _)) = key.split_once('/') else { continue };
                let Some(index) = series.parse().ok().and_then(|id| indices.get(&id)) else {
                    continue;
                };
                let Some(action) = notes[*index].action.as_mut() else { continue };
                let Some(map) = fields(value) else { continue };
                let Some(target) = target(map) else { continue };
                let Some(scope) = text(map, "scope").and_then(|text| text.parse().ok()) else {
                    continue;
                };
                if root == STEPS {
                    action.occurrences.push(StepOccurrence {
                        at: number(map, "at"),
                        date: target.date,
                        key: Some(target.key),
                        series: text(map, "series").and_then(|text| text.parse().ok()),
                    });
                    continue;
                }
                let Some(recur) = action.recur.as_mut() else { continue };
                let active = scope == recur.scope;
                match root {
                    HANDLED | SETTLED => {
                        let state = match text(map, "state").as_deref() {
                            Some("done") => OccurrenceState::Done,
                            Some("skipped") => OccurrenceState::Skipped,
                            Some("open") => OccurrenceState::Open,
                            _ => continue,
                        };
                        let handled = HandledOccurrence {
                            at: number(map, "at"),
                            date: target.date,
                            day: date(map, "day").unwrap_or(target.date),
                            defer: date(map, "defer"),
                            due: date(map, "due"),
                            early: matches!(map.get("early"), Some(LoroValue::Bool(true))),
                            key: target.key,
                            on: date(map, "on").unwrap_or(target.date),
                            order: number(map, "order") as u64,
                            peer: text(map, "peer").and_then(|text| text.parse().ok()).unwrap_or(0),
                            scope,
                            state,
                            target,
                        };
                        if root == SETTLED {
                            if active
                                && let Some(reach) =
                                    OccurrenceReach::from_value(&handled, map.get("reach"))
                            {
                                recur.reaches.push(reach);
                            }
                            continue;
                        }
                        if !active {
                            if state != OccurrenceState::Open {
                                recur.past.push(handled);
                            }
                        } else if state == OccurrenceState::Open {
                            recur.reopened.push(handled);
                        } else {
                            recur.handled.push(handled);
                        }
                    }
                    MOVED if active => {
                        if let Some(to) = date(map, "to") {
                            recur.moved.push(MovedOccurrence {
                                date: target.date,
                                key: target.key,
                                rule: text(map, "rule"),
                                scope,
                                start: date(map, "start"),
                                target,
                                to,
                            });
                        }
                    }
                    NOTES => {
                        if let Some(note) = text(map, "note").and_then(|text| text.parse().ok()) {
                            recur.notes.push(OccurrenceNote {
                                date: target.date,
                                key: target.key,
                                note,
                                target,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        for note in notes {
            if let Some(action) = &mut note.action {
                action.occurrences.sort_by_key(|entry| (entry.date, entry.key));
            }
            let Some(recur) = note.action.as_mut().and_then(|action| action.recur.as_mut()) else {
                continue;
            };
            recur.handled.sort_by_key(|entry| (entry.date, entry.key));
            recur.moved.sort_by_key(|entry| (entry.date, entry.key));
            recur.notes.sort_by_key(|entry| (entry.date, entry.key));
            recur.reaches.sort_by_key(|entry| entry.date);
        }
    }

    fn migrate_recurrence_history(
        &self,
        since: &VersionVector,
    ) -> Result<(bool, LegacyRecords), Error> {
        let actions: HashMap<_, _> = self
            .notes()
            .into_iter()
            .filter_map(|note| Some((self.action_map(note.id).ok()?.id(), note.id)))
            .collect();
        let roots: HashMap<_, _> = [HANDLED, MOVED, NOTES, SETTLED, STEPS]
            .into_iter()
            .map(|root| (self.doc.get_map(root).id(), root))
            .collect();
        let mut changed = false;
        let mut writes: HashMap<_, ID> = HashMap::new();
        let history =
            self.doc.export_json_updates_without_peer_compression(since, &self.doc.oplog_vv());
        for change in history.changes {
            for op in change.ops {
                let JsonOpContent::Map(JsonMapOp::Insert { key, value }) = op.content else {
                    continue;
                };
                if key == "recurrence_generation"
                    && let Some(&series) = actions.get(&op.container)
                    && let Some(generation) = integer(value).filter(|&value| value >= 0)
                {
                    changed |= remember_generation(&self.doc, series, generation as u64)?;
                }
                let Some(&root) = roots.get(&op.container) else { continue };
                let records = self.doc.get_map(root);
                let Some(value) = records.get(&key).map(|value| value.get_deep_value()) else {
                    continue;
                };
                let Some(map) = fields(&value) else { continue };
                if map.contains_key("identity_version")
                    || records.get_last_editor(&key) != Some(change.id.peer)
                {
                    continue;
                }
                let id = ID::new(change.id.peer, op.counter);
                writes
                    .entry((root, key))
                    .and_modify(|old| *old = ID::new(id.peer, old.counter.max(id.counter)))
                    .or_insert(id);
            }
        }
        let mut legacy = HashSet::new();
        if writes.is_empty() {
            return Ok((changed, legacy));
        }
        let writer = self.doc.fork();
        for ((root, key), id) in writes {
            // The merged meta version can't identify the format of a concurrent record's writer.
            writer.checkout(&Frontiers::from(id)).map_err(loro_error)?;
            let version = writer
                .get_map("meta")
                .get("version")
                .and_then(|value| integer(value.get_deep_value()))
                .unwrap_or(0);
            if version <= 6 {
                legacy.insert((root, key.clone()));
            }
        }
        Ok((changed, legacy))
    }

    fn migrate_recurrence_scopes(&self, legacy: &LegacyRecords) -> Result<bool, Error> {
        let series: HashMap<_, _> = self
            .notes()
            .into_iter()
            .filter_map(|note| {
                let recur = note.action?.recur?;
                Some((note.id, (recur.legacy_scope()?, recur)))
            })
            .collect();
        let mut changed = false;
        for root in [HANDLED, MOVED, NOTES, SETTLED, STEPS] {
            let records = self.doc.get_map(root);
            let LoroValue::Map(values) = records.get_value() else { continue };
            for (key, value) in values.iter() {
                if !legacy.contains(&(root, key.clone())) {
                    continue;
                }
                let Some((owner, tail)) = key.split_once('/') else { continue };
                let Some(owner) = owner.parse().ok() else { continue };
                let Some(map) = fields(value) else { continue };
                let id = if root == STEPS {
                    let Some(id) = text(map, "series").and_then(|id| id.parse().ok()) else {
                        continue;
                    };
                    id
                } else {
                    owner
                };
                let Some((scope, recur)) = series.get(&id) else { continue };
                if text(map, "scope").and_then(|text| text.parse().ok()) != Some(*scope) {
                    continue;
                }
                let Some(old) = target(map) else { continue };
                let target = recur.target_from(old.date, old.cycle, old.origin);
                let new_key = if root == SETTLED {
                    let Some((_, suffix)) = tail.split_once('/') else { continue };
                    format!("{owner}/{}/{suffix}", target.key)
                } else {
                    record_key(owner, target.key)
                };
                if records.get(&new_key).is_none() {
                    let mut values: Vec<_> =
                        map.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                    values.extend(target_fields(recur.scope, target));
                    insert_preserving(&records, &new_key, values)?;
                }
                records.delete(key).map_err(loro_error)?;
                changed = true;
            }
        }
        Ok(changed)
    }

    pub(crate) fn migrate_recurrence(&self, since: &VersionVector) -> Result<bool, Error> {
        let (mut changed, legacy) = self.migrate_recurrence_history(since)?;
        changed |= self.migrate_recurrence_scopes(&legacy)?;
        let meta = self.doc.get_map("meta");
        if meta.get("version").and_then(|value| integer(value.get_deep_value())) != Some(VERSION) {
            meta.insert("version", VERSION).map_err(loro_error)?;
            changed = true;
        }
        let raw = self.notes();
        let days_off = self.days_off_set();
        for note in &raw {
            let Some(recur) = note.action.as_ref().and_then(|action| action.recur.as_ref()) else {
                continue;
            };
            let map = self.recur_map(note.id)?;
            if map.get("calendar").is_some() {
                continue;
            }
            let LoroValue::Map(old) = map.get_deep_value() else { continue };
            let calendar = recur.calendar_value();
            let legacy_dates = |key: &str| -> Vec<(Date, LoroValue)> {
                old.get(key)
                    .and_then(fields)
                    .into_iter()
                    .flat_map(|map| map.iter())
                    .filter_map(|(key, value)| Some((key.parse().ok()?, value.clone())))
                    .collect()
            };
            let mut handled = legacy_dates("handled");
            handled.sort_by_key(|(date, value)| {
                let at = fields(value).map_or(0, |map| number(map, "at"));
                let day = fields(value).and_then(|map| crate::occurrence_store::date(map, "day"));
                (at, day, *date)
            });
            let mut targets = HashMap::new();
            let mut origin = recur.settings.start;
            for (index, (date, value)) in handled.iter().enumerate() {
                let cycle = (recur.settings.missed == crate::Missed::After).then_some(index as u32);
                let nominal = recur.legacy_nominal(*date, &days_off);
                targets.insert(*date, recur.target_from(nominal, cycle, origin));
                origin = fields(value)
                    .and_then(|map| crate::occurrence_store::date(map, "day"))
                    .unwrap_or(*date)
                    .max(recur.settings.start);
            }
            let mut future: Vec<Date> = ["moved", "notes"]
                .into_iter()
                .flat_map(|key| legacy_dates(key).into_iter().map(|(date, _)| date))
                .filter(|date| !targets.contains_key(date))
                .collect();
            future.sort_unstable();
            future.dedup();
            for date in future {
                let cycle = (recur.settings.missed == crate::Missed::After).then_some(
                    handled.len() as u32 + targets.len().saturating_sub(handled.len()) as u32,
                );
                targets.insert(
                    date,
                    recur.target_from(recur.legacy_nominal(date, &days_off), cycle, origin),
                );
                origin = date.max(recur.settings.start);
            }
            for (old_key, root, field) in [("moved", MOVED, "to"), ("notes", NOTES, "note")] {
                for (date, value) in legacy_dates(old_key) {
                    let target = targets[&date];
                    let mut values = target_fields(recur.scope, target);
                    if let Some(old) = fields(&value) {
                        values.extend(old.iter().map(|(key, value)| (key.clone(), value.clone())));
                    } else {
                        values.push((field.into(), value));
                    }
                    if root == MOVED {
                        if !values.iter().any(|(key, _)| key == "rule") {
                            values.push(("rule".into(), recur.settings.rule.to_string().into()));
                        }
                        if !values.iter().any(|(key, _)| key == "start") {
                            values.push(("start".into(), recur.settings.start.to_string().into()));
                        }
                    }
                    let key = record_key(note.id, target.key);
                    if self.doc.get_map(root).get(&key).is_none() {
                        insert_preserving(&self.doc.get_map(root), &key, values)?;
                    }
                }
            }
            let projection = self.note(note.id).and_then(|note| note.action?.recur);
            let projection = projection.as_ref().unwrap_or(recur);
            for (date, value) in handled {
                let Some(old) = fields(&value) else { continue };
                let target = targets[&date];
                let mut values: Vec<_> =
                    old.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                values.extend(target_fields(recur.scope, target));
                let on = legacy_dates("moved")
                    .into_iter()
                    .find(|(key, _)| *key == date)
                    .and_then(|(_, value)| {
                        fields(&value)
                            .and_then(|map| crate::occurrence_store::date(map, "to"))
                            .or_else(|| string(value)?.parse().ok())
                    })
                    .unwrap_or_else(|| recur.legacy_on(date, &days_off));
                values.push(("on".into(), on.to_string().into()));
                for (key, offset) in [
                    ("defer", Some(recur.settings.defer.unwrap_or(0))),
                    ("due", recur.settings.due),
                ] {
                    if let Some(offset) = offset {
                        values.push((key.into(), on.add_days(offset).to_string().into()));
                    }
                }
                values.push(("order".into(), number(old, "at").max(0).into()));
                values.push(("peer".into(), "0".into()));
                if !old.contains_key("reach") {
                    let day = crate::occurrence_store::date(old, "day").unwrap_or(date);
                    let start = on.add_days(recur.settings.defer.unwrap_or(0));
                    let reach = Occurrences::new(projection, &days_off, day).reach(
                        target,
                        day.max(start),
                        number(old, "at").max(0) as u64,
                        0,
                    );
                    values.push(("reach".into(), reach.to_value()));
                }
                let key = record_key(note.id, target.key);
                if self.doc.get_map(HANDLED).get(&key).is_none() {
                    insert_preserving(&self.doc.get_map(HANDLED), &key, values)?;
                }
            }
            if let Some(action) = &note.action
                && action.state == crate::ActionState::Done
            {
                let fields = self.action_map(note.id)?;
                fields
                    .insert("recurrence_stopped", action.closed.unwrap_or_else(|| self.now()))
                    .map_err(loro_error)?;
                fields.insert("state", "open").map_err(loro_error)?;
                fields.delete("closed").map_err(loro_error)?;
            }
            map.insert("calendar", calendar).map_err(loro_error)?;
            self.action_map(note.id)?.insert("recurrence_generation", 0).map_err(loro_error)?;
            remember_generation(&self.doc, note.id, 0)?;
            changed = true;
        }
        let hierarchy = self.hierarchy();
        for note in &raw {
            let Some(action) = &note.action else { continue };
            let legacy: Vec<_> =
                action.occurrences.iter().filter(|entry| entry.key.is_none()).collect();
            if legacy.is_empty() {
                continue;
            }
            let series = hierarchy
                .ancestors(note.id)
                .filter_map(|id| {
                    let recur =
                        raw.iter().find(|note| note.id == id)?.action.as_ref()?.recur.as_ref()?;
                    Some(recur)
                })
                .last();
            if let Some(recur) = series {
                for entry in legacy {
                    let nominal = recur.legacy_nominal(entry.date, &days_off);
                    let mut active = self.note(recur.series).and_then(|note| note.action?.recur);
                    if let Some(active) = &mut active {
                        active.stopped = None;
                    }
                    let target = active
                        .as_ref()
                        .and_then(|active| {
                            active
                                .handled
                                .iter()
                                .find(|handled| handled.date == nominal)
                                .map(|handled| handled.target)
                        })
                        .or_else(|| {
                            active
                                .as_ref()
                                .and_then(|active| {
                                    Occurrences::new(active, &days_off, Date::EPOCH).current()
                                })
                                .filter(|current| current.date == nominal)
                                .map(|current| current.target)
                        })
                        .unwrap_or_else(|| recur.target(nominal, None));
                    let mut values = target_fields(recur.scope, target);
                    values.extend([
                        ("at".into(), entry.at.into()),
                        ("series".into(), recur.series.to_string().into()),
                    ]);
                    let key = record_key(note.id, target.key);
                    if self.doc.get_map(STEPS).get(&key).is_none() {
                        insert_preserving(&self.doc.get_map(STEPS), &key, values)?;
                    }
                }
                self.action_map(note.id)?.delete("occurrences").map_err(loro_error)?;
                changed = true;
            }
        }
        let LoroValue::Map(records) = self.doc.get_map(HANDLED).get_value() else {
            return Ok(changed);
        };
        let settled = self.doc.get_map(SETTLED);
        for (key, value) in records.iter() {
            let Some((series, _)) = key.split_once('/') else { continue };
            let Some(series) = series.parse().ok() else { continue };
            let Some(map) = fields(value) else { continue };
            if !matches!(text(map, "state").as_deref(), Some("done" | "skipped"))
                || map.get("reach").and_then(fields).is_none()
            {
                continue;
            }
            let Some(target) = target(map) else { continue };
            let order = number(map, "order") as u64;
            let peer = text(map, "peer").and_then(|text| text.parse().ok()).unwrap_or(0);
            let key = settlement_key(series, target.key, order, peer);
            if settled.get(&key).is_none() {
                settled.insert(&key, value.clone()).map_err(loro_error)?;
                changed = true;
            }
        }
        Ok(changed)
    }
}
