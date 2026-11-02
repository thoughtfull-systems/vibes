// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{ActionState, Date, Library, OccurrenceStatus};
use loro::{Container, ExportMode, LoroDoc, LoroMap, LoroValue, ValueOrContainer};

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

fn map(parent: &LoroMap, key: &str) -> LoroMap {
    match parent.get(key) {
        Some(ValueOrContainer::Container(Container::Map(map))) => map,
        _ => panic!("missing map {key}"),
    }
}

fn entry(values: &[(&str, LoroValue)]) -> LoroValue {
    LoroValue::Map(values.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect())
}

#[test]
fn version_four_nested_records_migrate_once_without_losing_their_fields() {
    let mut seed = Library::in_memory();
    let id = seed.create_task("Monthly review", None).unwrap();
    let step = seed.create_task("Step", Some(id)).unwrap();
    let memo = seed.create_memo("Review notes").unwrap();
    let doc = LoroDoc::new();
    doc.import(&seed.export_snapshot().unwrap()).unwrap();
    doc.get_map("meta").insert("version", 4).unwrap();
    let action = map(&map(&doc.get_map("notes"), &id.to_string()), "action");
    let recur = action.insert_container("recur", LoroMap::new()).unwrap();
    recur.insert("rule", "FREQ=MONTHLY;COUNT=3").unwrap();
    recur.insert("start", "2026-10-01").unwrap();
    recur.insert("missed", "all").unwrap();
    recur.insert("defer", -2).unwrap();
    recur.insert("due", 3).unwrap();
    let handled = recur.insert_container("handled", LoroMap::new()).unwrap();
    handled
        .insert(
            "2026-10-01",
            entry(&[
                ("at", 123.into()),
                ("day", "2026-10-02".into()),
                ("future", "keep".into()),
                ("state", "done".into()),
            ]),
        )
        .unwrap();
    let moved = recur.insert_container("moved", LoroMap::new()).unwrap();
    moved.insert("2026-10-01", "2026-10-02").unwrap();
    moved.insert("2026-11-01", "2026-11-03").unwrap();
    let notes = recur.insert_container("notes", LoroMap::new()).unwrap();
    notes.insert("2026-11-01", memo.to_string()).unwrap();
    let fields = map(&map(&doc.get_map("notes"), &step.to_string()), "action");
    let steps = fields.insert_container("occurrences", LoroMap::new()).unwrap();
    steps.insert("2026-11-01", 234).unwrap();
    doc.commit();
    let bytes = doc.export(ExportMode::Snapshot).unwrap();
    let mut library = Library::in_memory();
    library.import_snapshot(&bytes).unwrap();
    let today = day("2026-11-01");
    let current = library.current_occurrence(id, today).unwrap();
    assert_eq!(current.on, day("2026-11-03"));
    assert_eq!(current.note, Some(memo));
    assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
    let history = library.occurrences(id, today, 10, 3);
    let completed = history.iter().find(|entry| entry.status == OccurrenceStatus::Done).unwrap();
    assert_eq!(completed.on, day("2026-10-02"));
    assert_eq!(completed.defer, Some(day("2026-09-30")));
    assert_eq!(completed.due, Some(day("2026-10-05")));
    let migrated = LoroDoc::new();
    migrated.import(&library.export_snapshot().unwrap()).unwrap();
    let root = migrated.get_map("recurrence_handled").get_deep_value();
    let LoroValue::Map(root) = root else { panic!() };
    let LoroValue::Map(value) = root.values().next().unwrap() else { panic!() };
    assert_eq!(value.get("future"), Some(&"keep".into()));
    let mut peer = Library::in_memory();
    peer.import_snapshot(&library.export_snapshot().unwrap()).unwrap();
    assert_eq!(peer.current_occurrence(id, today), Some(current));
    assert_eq!(peer.occurrences(id, today, 10, 3), history);
}

#[test]
fn after_migration_matches_step_records_to_their_completed_cycles() {
    let mut seed = Library::in_memory();
    let id = seed.create_task("Water", None).unwrap();
    let step = seed.create_task("Step", Some(id)).unwrap();
    let doc = LoroDoc::new();
    doc.import(&seed.export_snapshot().unwrap()).unwrap();
    doc.get_map("meta").insert("version", 4).unwrap();
    let action = map(&map(&doc.get_map("notes"), &id.to_string()), "action");
    let recur = action.insert_container("recur", LoroMap::new()).unwrap();
    recur.insert("rule", "FREQ=DAILY;INTERVAL=7;COUNT=2").unwrap();
    recur.insert("start", "2026-11-01").unwrap();
    recur.insert("missed", "after").unwrap();
    let handled = recur.insert_container("handled", LoroMap::new()).unwrap();
    for (date, at) in [("2026-11-01", 123), ("2026-11-08", 234)] {
        handled
            .insert(
                date,
                entry(&[("at", at.into()), ("day", date.into()), ("state", "done".into())]),
            )
            .unwrap();
    }
    let fields = map(&map(&doc.get_map("notes"), &step.to_string()), "action");
    let steps = fields.insert_container("occurrences", LoroMap::new()).unwrap();
    steps.insert("2026-11-08", 200).unwrap();
    doc.commit();
    let mut library = Library::in_memory();
    library.import_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    let today = day("2026-11-08");
    library.check_action(id, false, today).unwrap();
    let current = library.current_occurrence(id, today).unwrap();
    assert_eq!(current.target.cycle, Some(1));
    assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
}

#[test]
fn a_legacy_series_stop_moves_out_of_the_plain_completion_fields() {
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let doc = LoroDoc::new();
    doc.import(&seed.export_snapshot().unwrap()).unwrap();
    doc.get_map("meta").insert("version", 4).unwrap();
    let action = map(&map(&doc.get_map("notes"), &id.to_string()), "action");
    action.insert("state", "done").unwrap();
    action.insert("closed", 123).unwrap();
    let recur = action.insert_container("recur", LoroMap::new()).unwrap();
    recur.insert("rule", "FREQ=DAILY").unwrap();
    recur.insert("start", "2026-11-01").unwrap();
    doc.commit();
    let mut library = Library::in_memory();
    library.import_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    let raw = library.note(id).unwrap().action.unwrap();
    assert_eq!(raw.state, ActionState::Open);
    assert_eq!(raw.closed, None);
    assert_eq!(raw.recur.unwrap().stopped, Some(123));
    let start = day("2026-11-01");
    assert_eq!(
        library.occurrence_summary(id, start).unwrap().state,
        commonplace_core::SeriesState::Stopped
    );
    library.check_action(id, false, start).unwrap();
    assert_eq!(library.current_occurrence(id, start).unwrap().date, start);
}

#[test]
fn version_five_settlement_survives_migration_and_reopening() {
    let mut seed = Library::in_memory();
    let start = day("2026-11-01");
    let today = day("2026-11-10");
    let id = seed.create_task("Routine", None).unwrap();
    let settings = commonplace_core::RecurrenceSettings::new(
        commonplace_core::Rule::parse("FREQ=DAILY;COUNT=10").unwrap(),
        start,
    );
    seed.set_recurrence(id, Some(settings), start).unwrap();
    let target = seed.current_occurrence(id, today).unwrap().target;
    seed.check_action(id, true, today).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 5).unwrap();
    let settled = legacy.get_map("recurrence_settled");
    for key in settled.keys() {
        settled.delete(&key).unwrap();
    }
    legacy.commit();
    let bytes = legacy.export(ExportMode::Snapshot).unwrap();
    let mut library = Library::in_memory();
    library.import_snapshot(&bytes).unwrap();
    let migrated = library.export_snapshot().unwrap();
    let doc = LoroDoc::new();
    doc.import(&migrated).unwrap();
    assert_eq!(doc.get_map("recurrence_settled").len(), 1);
    library.import_snapshot(&bytes).unwrap();
    library.handle_occurrence_target(id, target, None, today).unwrap();
    library.move_occurrence(id, start, Some(today.add_days(1))).unwrap();
    assert_eq!(library.current_occurrence(id, today.add_days(1)).unwrap().target, target);
    let mut peer = Library::in_memory();
    peer.import_snapshot(&library.export_snapshot().unwrap()).unwrap();
    assert_eq!(peer.current_occurrence(id, today.add_days(1)).unwrap().target, target);
}

#[test]
fn handling_sequences_include_reopened_records_from_previous_calendars() {
    let mut seed = Library::in_memory();
    let start = day("2026-11-01");
    let id = seed.create_task("Routine", None).unwrap();
    let mut settings = commonplace_core::RecurrenceSettings::new(
        commonplace_core::Rule::parse("FREQ=DAILY;COUNT=2").unwrap(),
        start,
    );
    seed.set_recurrence(id, Some(settings.clone()), start).unwrap();
    let target = seed.current_occurrence(id, start).unwrap().target;
    seed.check_action(id, true, start).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 5).unwrap();
    let handled = legacy.get_map("recurrence_handled");
    let key = handled.keys().into_iter().next().unwrap();
    let LoroValue::Map(value) = handled.get(&key).unwrap().get_deep_value() else { panic!() };
    let mut fields: Vec<_> = value
        .iter()
        .filter(|(key, _)| key.as_str() != "order")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    fields.push(("order".into(), 1_000_000_000_000i64.into()));
    handled.insert(&key, LoroValue::Map(fields.into())).unwrap();
    legacy.commit();
    let mut library = Library::in_memory();
    library.import_snapshot(&legacy.export(ExportMode::Snapshot).unwrap()).unwrap();
    library.handle_occurrence_target(id, target, None, start).unwrap();
    let reopened = library.note(id).unwrap().action.unwrap().recur.unwrap().reopened[0].order;
    settings.rule.interval = 2;
    library.set_recurrence(id, Some(settings), start).unwrap();
    library.check_action(id, true, start).unwrap();
    let completed = library.note(id).unwrap().action.unwrap().recur.unwrap().handled[0].order;
    assert!(completed > reopened);
}

#[test]
fn editing_other_schedules_preserves_unreadable_position_rules() {
    let mut seed = Library::in_memory();
    let id = seed.create_journal("Journal").unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 5).unwrap();
    let journal = map(&map(&legacy.get_map("notes"), &id.to_string()), "journal");
    let schedules = map(&journal, "schedules");
    let original = entry(&[
        ("future", "preserve".into()),
        ("kind", "fixed".into()),
        ("rule", "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=1,-1".into()),
        ("times", LoroValue::List(vec!["10:00".into()].into())),
    ]);
    schedules.insert("old-rule", original.clone()).unwrap();
    legacy.commit();
    let mut library = Library::in_memory();
    library.import_snapshot(&legacy.export(ExportMode::Snapshot).unwrap()).unwrap();
    let schedule = commonplace_core::Schedule::new(
        commonplace_core::Rule::default(),
        day("2026-11-01"),
        commonplace_core::Times::Fixed(vec![600]),
    );
    library.set_schedule(id, &schedule).unwrap();
    let result = LoroDoc::new();
    result.import(&library.export_snapshot().unwrap()).unwrap();
    let journal = map(&map(&result.get_map("notes"), &id.to_string()), "journal");
    assert_eq!(map(&journal, "schedules").get("old-rule").unwrap().get_deep_value(), original);
}
