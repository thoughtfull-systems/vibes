// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, OccurrenceState,
    RecurrenceSettings, Rule, Schedule, Times,
};
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

#[test]
fn journals_prompt_on_the_holiday_adjusted_first_workday() {
    let mut library = Library::in_memory();
    let journal = library.create_journal("Log").unwrap();
    let start = day("2026-11-01");
    let mut schedule = Schedule::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1").unwrap(),
        start,
        Times::Fixed(vec![600]),
    );
    schedule.days_off.holidays = DayOffEffect::Skip;
    library.set_days_off(&DaysOff::new(DaysOffKind::Holidays, "Off", day("2026-11-02"))).unwrap();
    library.set_schedule(journal, &schedule).unwrap();
    assert_eq!(library.schedule_minutes(journal, &schedule, day("2026-11-03")), [600]);
}

#[test]
fn weekly_position_week_start_is_part_of_identity() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let start = day("2026-11-01");
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=WEEKLY;BYDAY=MO,SU;BYSETPOS=1;WKST=MO").unwrap(),
        start,
    );
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    let before = library.note(id).unwrap().action.unwrap().recur.unwrap().scope;
    settings.rule.week_start = 6;
    library.set_recurrence(id, Some(settings), start).unwrap();
    let after = library.note(id).unwrap().action.unwrap().recur.unwrap().scope;
    assert_ne!(before, after);
}

#[test]
fn unavailable_slots_survive_later_settlement_and_calendar_edits() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let start = day("2026-11-01");
    let today = day("2026-11-02");
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=2").unwrap(), start);
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    library.set_days_off(&DaysOff::new(DaysOffKind::Holidays, "Off", start)).unwrap();
    library.check_action(id, true, today).unwrap();
    settings.days_off.holidays = DayOffEffect::Ignore;
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert!(library.current_occurrence(id, today).is_some());
}

#[test]
fn after_preview_next_cycle_survives_recompleting_a_reopened_cycle() {
    let mut library = Library::in_memory();
    let id = library.create_task("Water", None).unwrap();
    let start = day("2026-11-01");
    let mut settings = RecurrenceSettings::new(Rule::default(), start);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), start).unwrap();
    let first = library.current_occurrence(id, start).unwrap();
    library.check_action(id, true, start).unwrap();
    library.check_action(id, true, start.add_days(1)).unwrap();
    library.handle_occurrence_target(id, first.target, None, start.add_days(1)).unwrap();
    let expected_next = library.occurrences(id, start.add_days(1), 10, 1)[1];
    let memo = library.write_occurrence_note_target(id, expected_next.target).unwrap();
    library.check_action(id, true, start.add_days(1)).unwrap();
    let actual_next = library.current_occurrence(id, start.add_days(1)).unwrap();
    assert_eq!(actual_next.target.key, expected_next.target.key);
    assert_eq!(actual_next.note, Some(memo));
}

#[test]
fn settlement_does_not_reach_moves_outside_current_bounds() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let start = day("2026-11-01");
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=10").unwrap(), start);
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    library.move_occurrence(id, start.add_days(9), Some(start)).unwrap();
    settings.rule.count = Some(2);
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    library.check_action(id, true, start.add_days(1)).unwrap();
    settings.rule.count = Some(10);
    library.set_recurrence(id, Some(settings), start.add_days(1)).unwrap();
    library.move_occurrence(id, start.add_days(9), Some(start.add_days(20))).unwrap();
    for n in 2..9 {
        library.check_action(id, true, start.add_days(n)).unwrap();
    }
    assert_eq!(library.current_occurrence(id, start.add_days(9)).unwrap().date, start.add_days(9));
}

#[test]
fn until_rejects_trailing_time_parts() {
    for until in ["2026110", "202611020", "20261102junk", "20261102T000000Z"] {
        assert!(Rule::parse(&format!("FREQ=DAILY;UNTIL={until}")).is_none());
    }
    assert!(Rule::parse("FREQ=DAILY;UNTIL=20261102").is_some());
}

#[test]
fn archiving_days_off_preserves_unknown_fields() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let off = DaysOff::new(DaysOffKind::Holidays, "Off", start);
    library.set_days_off(&off).unwrap();
    let doc = LoroDoc::new();
    doc.import(&library.export_snapshot().unwrap()).unwrap();
    let root = doc.get_map("days_off");
    let LoroValue::Map(value) = root.get(&off.id).unwrap().get_deep_value() else { panic!() };
    let mut fields: Vec<_> = value.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    fields.push(("future".to_string(), "keep".into()));
    root.insert(&off.id, LoroValue::Map(fields.into())).unwrap();
    doc.commit();
    library.import_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    library.archive_days_off(off.kind, &off.id, true, start).unwrap();
    let doc = LoroDoc::new();
    doc.import(&library.export_snapshot().unwrap()).unwrap();
    let LoroValue::Map(value) = doc.get_map("days_off").get(&off.id).unwrap().get_deep_value()
    else {
        panic!()
    };
    assert_eq!(value.get("future"), Some(&"keep".into()));
}

#[test]
fn migration_does_not_settle_a_legacy_move_that_had_not_started() {
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let doc = LoroDoc::new();
    doc.import(&seed.export_snapshot().unwrap()).unwrap();
    doc.get_map("meta").insert("version", 4).unwrap();
    let action = map(&map(&doc.get_map("notes"), &id.to_string()), "action");
    let recur = action.insert_container("recur", LoroMap::new()).unwrap();
    recur.insert("rule", "FREQ=DAILY;COUNT=2").unwrap();
    recur.insert("start", "2026-11-01").unwrap();
    let handled = recur.insert_container("handled", LoroMap::new()).unwrap();
    handled
        .insert(
            "2026-11-02",
            LoroValue::Map(
                vec![
                    ("at".into(), 123.into()),
                    ("day".into(), "2026-11-02".into()),
                    ("state".into(), "done".into()),
                ]
                .into(),
            ),
        )
        .unwrap();
    let moved = recur.insert_container("moved", LoroMap::new()).unwrap();
    moved.insert("2026-11-01", "2026-11-30").unwrap();
    doc.commit();
    let mut library = Library::in_memory();
    library.import_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(library.current_occurrence(id, day("2026-11-30")).unwrap().on, day("2026-11-30"));
}

#[test]
fn extending_bounds_does_not_inherit_settlement_from_outside_them() {
    let start = day("2026-11-01");
    for bound in ["COUNT=2", "UNTIL=20261102"] {
        let mut library = Library::in_memory();
        let id = library.create_task("Routine", None).unwrap();
        let settings =
            RecurrenceSettings::new(Rule::parse(&format!("FREQ=DAILY;{bound}")).unwrap(), start);
        library.set_recurrence(id, Some(settings.clone()), start).unwrap();
        library.check_action(id, true, start.add_days(9)).unwrap();
        let mut extended = settings;
        extended.rule = Rule::parse("FREQ=DAILY;COUNT=3").unwrap();
        library.set_recurrence(id, Some(extended), start.add_days(9)).unwrap();
        assert_eq!(
            library.current_occurrence(id, start.add_days(9)).unwrap().date,
            start.add_days(2)
        );
    }
}

#[test]
fn after_previews_walk_each_reopened_cycle_before_allocating_new_cycles() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=4").unwrap(), start);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), start).unwrap();
    let mut closed = Vec::new();
    for offset in 0..3 {
        closed.push(library.current_occurrence(id, start.add_days(offset)).unwrap().target);
        library.check_action(id, true, start.add_days(offset)).unwrap();
    }
    for target in &closed[..2] {
        library.handle_occurrence_target(id, *target, None, start.add_days(2)).unwrap();
    }
    let preview = library.occurrences(id, start.add_days(2), 0, 10);
    assert_eq!(
        preview.iter().map(|entry| entry.target.cycle).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(3)]
    );
    for occurrence in preview {
        let today = start.add_days(2).max(occurrence.on);
        assert_eq!(library.current_occurrence(id, today).unwrap().target, occurrence.target);
        library.check_action(id, true, today).unwrap();
    }
    assert!(library.current_occurrence(id, start.add_days(10)).is_none());
}

#[test]
fn version_six_weekly_records_migrate_together_and_merge_once() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let step = seed.create_task("Step", Some(id)).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO,SU;BYSETPOS=1").unwrap(), start);
    settings.missed = Missed::All;
    seed.set_recurrence(id, Some(settings), start).unwrap();
    let target = seed.current_occurrence(id, start).unwrap().target;
    seed.check_action(step, true, start).unwrap();
    seed.move_occurrence_target(id, target, Some(start.add_days(2))).unwrap();
    let memo = seed.write_occurrence_note_target(id, target).unwrap();
    seed.handle_occurrence_target(id, target, Some(OccurrenceState::Done), start).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 6).unwrap();
    let action = map(&map(&legacy.get_map("notes"), &id.to_string()), "action");
    let recur = map(&action, "recur");
    let LoroValue::Map(calendar) = recur.get("calendar").unwrap().get_deep_value() else {
        panic!()
    };
    let mut calendar: Vec<_> = calendar
        .iter()
        .filter(|(key, _)| key.as_str() != "rule")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    calendar.push(("rule".into(), "FREQ=WEEKLY;BYDAY=MO,SU;BYSETPOS=1;WKST=SU".into()));
    recur.insert("calendar", LoroValue::Map(calendar.into())).unwrap();
    for root in [
        "recurrence_handled",
        "recurrence_moved",
        "recurrence_notes",
        "recurrence_settled",
        "step_occurrences",
    ] {
        let records = legacy.get_map(root);
        let key = records.keys().into_iter().next().unwrap();
        let LoroValue::Map(value) = records.get(&key).unwrap().get_deep_value() else { panic!() };
        let mut fields: Vec<_> =
            value.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
        fields.push(("future".into(), "keep".into()));
        records.insert(&key, LoroValue::Map(fields.into())).unwrap();
    }
    legacy.commit();
    let bytes = legacy.export(ExportMode::Snapshot).unwrap();
    let mut laptop = Library::in_memory();
    laptop.import_snapshot(&bytes).unwrap();
    let mut phone = Library::in_memory();
    phone.import_snapshot(&bytes).unwrap();
    laptop.merge_updates(&phone.updates_since(&laptop.version()).unwrap().unwrap(), false).unwrap();
    let recurrence = laptop.note(id).unwrap().action.unwrap().recur.unwrap();
    let migrated = recurrence.handled[0].target;
    assert_ne!(migrated.key, target.key);
    assert_eq!(recurrence.handled.len(), 1);
    assert_eq!(recurrence.reaches.len(), 1);
    let doc = LoroDoc::new();
    doc.import(&laptop.export_snapshot().unwrap()).unwrap();
    assert_eq!(doc.get_map("meta").get("version").unwrap().get_deep_value(), 7.into());
    for root in [
        "recurrence_handled",
        "recurrence_moved",
        "recurrence_notes",
        "recurrence_settled",
        "step_occurrences",
    ] {
        let LoroValue::Map(records) = doc.get_map(root).get_deep_value() else { panic!() };
        assert_eq!(records.len(), 1);
        let LoroValue::Map(value) = records.values().next().unwrap() else { panic!() };
        assert_eq!(value.get("future"), Some(&"keep".into()));
        assert_eq!(value.get("scope"), Some(&recurrence.scope.to_string().into()));
        assert_eq!(value.get("key"), Some(&migrated.key.to_string().into()));
    }
    laptop.handle_occurrence_target(id, migrated, None, start).unwrap();
    let current = laptop.current_occurrence(id, start).unwrap();
    assert_eq!(current.target, migrated);
    assert_eq!(current.on, start.add_days(2));
    assert_eq!(current.note, Some(memo));
    assert_eq!(laptop.note_on(step, start).unwrap().action.unwrap().state, ActionState::Done);
    let before = laptop.occurrences(id, start, 10, 3);
    phone.import_snapshot(&laptop.export_snapshot().unwrap()).unwrap();
    assert_eq!(phone.occurrences(id, start, 10, 3), before);
}
