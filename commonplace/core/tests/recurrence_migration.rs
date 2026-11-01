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
