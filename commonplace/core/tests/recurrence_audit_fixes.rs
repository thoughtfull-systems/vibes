// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, OccurrenceState,
    RecurrenceSettings, Rule, Schedule, Spaces, Times,
};
use loro::{Container, ExportMode, LoroDoc, LoroMap, LoroValue, ValueOrContainer};

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

fn copy(source: &Library) -> Library {
    let mut peer = Library::in_memory();
    peer.merge_updates(&source.export_snapshot().unwrap(), false).unwrap();
    peer
}

#[test]
fn concurrent_calendar_branches_do_not_reuse_a_discarded_generation() {
    let today = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Repeat", None).unwrap();
    seed.set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today).unwrap();
    let mut left = copy(&seed);
    let mut right = copy(&seed);
    let settings = |rule| RecurrenceSettings::new(Rule::parse(rule).unwrap(), today);
    left.set_recurrence(id, Some(settings("FREQ=WEEKLY")), today).unwrap();
    left.set_recurrence(id, Some(settings("FREQ=MONTHLY")), today).unwrap();
    let old_scope = left.note(id).unwrap().action.unwrap().recur.unwrap().scope;
    let old = left.current_occurrence(id, today).unwrap().target;
    left.handle_occurrence_target(id, old, Some(OccurrenceState::Done), today).unwrap();
    for index in 0..50 {
        right.set_body(id, &format!("Repeat {index}")).unwrap();
    }
    right.set_recurrence(id, Some(settings("FREQ=YEARLY")), today).unwrap();
    left.merge_updates(&right.export_snapshot().unwrap(), false).unwrap();
    assert_eq!(left.note(id).unwrap().action.unwrap().recur.unwrap().generation, 1);
    left.set_recurrence(id, Some(settings("FREQ=MONTHLY")), today).unwrap();
    let restarted_scope = left.note(id).unwrap().action.unwrap().recur.unwrap().scope;
    assert_ne!(restarted_scope, old_scope);
    let restarted = left.current_occurrence(id, today).unwrap().target;
    assert_ne!(restarted.key, old.key);
    assert_eq!(restarted.date, today, "an old completion closed the replacement calendar");
}

#[test]
fn current_records_keep_concurrent_week_start_scopes() {
    let today = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Repeat", None).unwrap();
    seed.set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today).unwrap();
    let mut left = copy(&seed);
    let mut right = copy(&seed);
    let settings = |suffix: &str| {
        let mut settings = RecurrenceSettings::new(
            Rule::parse(&format!("FREQ=WEEKLY;BYDAY=MO,SU;BYSETPOS=1{suffix}")).unwrap(),
            today,
        );
        settings.missed = Missed::All;
        settings
    };
    left.set_recurrence(id, Some(settings("")), today).unwrap();
    left.check_action(id, true, today).unwrap();
    for index in 0..50 {
        right.set_body(id, &format!("Repeat {index}")).unwrap();
    }
    right.set_recurrence(id, Some(settings(";WKST=SU")), today).unwrap();
    left.merge_updates(&right.export_snapshot().unwrap(), false).unwrap();
    let recur = left.note(id).unwrap().action.unwrap().recur.unwrap();
    assert_eq!(recur.settings.rule.week_start, 6);
    assert_eq!(
        left.current_occurrence(id, today).unwrap().date,
        today,
        "completion from the losing calendar migrated into the winning one: {recur:?}"
    );
}

#[test]
fn preview_covers_effective_dates_shifted_in_from_before_its_start() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Holiday", start);
    holiday.last = start.add_days(2);
    library.set_days_off(&holiday).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;COUNT=1").unwrap(), start);
    settings.days_off.holidays = DayOffEffect::Move;
    assert_eq!(library.preview_dates(&settings, start.add_days(1), 3), vec![start.add_days(3)]);
}

#[test]
fn removing_an_exhausted_repeat_preserves_its_projected_steps() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    let id = library.create_task("Repeat", None).unwrap();
    let step = library.create_task("Step", Some(id)).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=1").unwrap(), start);
    library.set_recurrence(id, Some(settings), start).unwrap();
    library.check_action(step, true, start).unwrap();
    library.check_action(id, true, start).unwrap();
    assert_eq!(library.note_on(step, start).unwrap().action.unwrap().state, ActionState::Done);
    library.set_recurrence(id, None, start).unwrap();
    assert_eq!(library.note_on(step, start).unwrap().action.unwrap().state, ActionState::Done);
}

#[test]
fn monthly_ordinals_can_limit_selected_month_days_as_the_spec_requires() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    let journal = library.create_journal("Log").unwrap();
    for (text, dates) in [
        (
            "FREQ=MONTHLY;BYMONTHDAY=1,2,3,4,5,6,7;BYDAY=1MO;COUNT=2",
            [day("2026-11-02"), day("2026-12-07")],
        ),
        (
            "FREQ=MONTHLY;BYMONTHDAY=-1,-2,-3,-4,-5,-6,-7;BYDAY=-1MO;COUNT=2",
            [day("2026-11-30"), day("2026-12-28")],
        ),
        (
            "FREQ=YEARLY;BYMONTH=11;BYMONTHDAY=1,2,3,4,5,6,7;BYDAY=1MO;COUNT=2",
            [day("2026-11-02"), day("2027-11-01")],
        ),
        (
            "FREQ=YEARLY;BYMONTHDAY=1,2,3,4,5,6,7;BYDAY=10MO;COUNT=2",
            [day("2028-03-06"), day("2029-03-05")],
        ),
    ] {
        let rule = Rule::parse(text).unwrap();
        let settings = RecurrenceSettings::new(rule.clone(), start);
        assert_eq!(library.preview_dates(&settings, start, 2), dates);
        let schedule = Schedule::new(rule, start, Times::Fixed(vec![600]));
        for date in dates {
            assert_eq!(library.schedule_minutes(journal, &schedule, date), vec![600]);
        }
        assert!(library.schedule_minutes(journal, &schedule, dates[0].add_days(7)).is_empty());
    }
}

fn map(parent: &LoroMap, key: &str) -> LoroMap {
    match parent.get(key) {
        Some(ValueOrContainer::Container(Container::Map(map))) => map,
        _ => panic!("missing map {key}"),
    }
}

#[test]
fn unmarked_version_seven_records_keep_their_calendar_provenance() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO,SU;BYSETPOS=1").unwrap(), start);
    settings.missed = Missed::All;
    seed.set_recurrence(id, Some(settings.clone()), start).unwrap();
    seed.check_action(id, true, start).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 7).unwrap();
    for root in ["recurrence_handled", "recurrence_settled"] {
        let records = legacy.get_map(root);
        let key = records.keys().into_iter().next().unwrap();
        let LoroValue::Map(value) = records.get(&key).unwrap().get_deep_value() else { panic!() };
        let fields = value
            .iter()
            .filter(|(key, _)| key.as_str() != "identity_version")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        records.insert(&key, LoroValue::Map(fields)).unwrap();
    }
    legacy.commit();
    let mut phone = Library::in_memory();
    phone.import_snapshot(&legacy.export(ExportMode::Snapshot).unwrap()).unwrap();
    settings.rule.week_start = 6;
    // Both calendars have generation zero, as concurrent initial activations can.
    let action = map(&map(&legacy.get_map("notes"), &id.to_string()), "action");
    let recur = map(&action, "recur");
    let LoroValue::Map(calendar) = recur.get("calendar").unwrap().get_deep_value() else {
        panic!()
    };
    let mut calendar: Vec<_> =
        calendar.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
    calendar.retain(|(key, _)| key != "rule");
    calendar.push(("rule".into(), settings.rule.to_string().into()));
    recur.insert("calendar", LoroValue::Map(calendar.into())).unwrap();
    legacy.commit();
    let mut laptop = Library::in_memory();
    laptop.import_snapshot(&legacy.export(ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(laptop.current_occurrence(id, start).unwrap().date, start);
    phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
    assert_eq!(phone.current_occurrence(id, start).unwrap().date, start);
    assert_eq!(phone.note(id).unwrap().action.unwrap().recur.unwrap().past.len(), 1);
}

#[test]
fn legacy_generation_history_retains_losing_allocations_after_a_move() {
    let today = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Repeat", None).unwrap();
    let settings = |rule| RecurrenceSettings::new(Rule::parse(rule).unwrap(), today);
    seed.set_recurrence(id, Some(settings("FREQ=DAILY")), today).unwrap();
    let mut left = copy(&seed);
    let mut right = copy(&seed);
    left.set_recurrence(id, Some(settings("FREQ=WEEKLY")), today).unwrap();
    left.set_recurrence(id, Some(settings("FREQ=MONTHLY")), today).unwrap();
    let old = left.current_occurrence(id, today).unwrap().target;
    for index in 0..50 {
        right.set_body(id, &format!("Repeat {index}")).unwrap();
    }
    right.set_recurrence(id, Some(settings("FREQ=YEARLY")), today).unwrap();
    left.merge_updates(&right.export_snapshot().unwrap(), false).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&left.export_snapshot().unwrap()).unwrap();
    let generations = legacy.get_map("recurrence_generations");
    for key in generations.keys() {
        generations.delete(&key).unwrap();
    }
    legacy.get_map("meta").insert("version", 7).unwrap();
    legacy.commit();
    let mut spaces = Spaces::in_memory(&["a", "b"]);
    spaces
        .library_mut("a")
        .unwrap()
        .import_snapshot(&legacy.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    spaces.set_recurrence(id, None, today).unwrap();
    spaces.move_to_space(id, "b").unwrap();
    spaces.set_recurrence(id, Some(settings("FREQ=MONTHLY")), today).unwrap();
    assert_eq!(spaces.note(id).unwrap().action.unwrap().recur.unwrap().generation, 3);
    assert!(spaces.handle_occurrence_target(id, old, Some(OccurrenceState::Done), today).is_err());
}

#[test]
fn previews_filter_effective_dates_within_position_selected_periods() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    library
        .set_days_off(&DaysOff::new(DaysOffKind::Holidays, "Monday off", day("2026-11-02")))
        .unwrap();
    for (position, from, expected) in [
        ("1", "2026-11-03", vec![day("2026-11-03")]),
        ("1", "2026-11-04", Vec::new()),
        ("-1", "2026-11-30", vec![day("2026-11-30")]),
    ] {
        let rule =
            Rule::parse(&format!("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS={position};COUNT=1"))
                .unwrap();
        let mut settings = RecurrenceSettings::new(rule, start);
        settings.days_off.holidays = DayOffEffect::Skip;
        assert_eq!(library.preview_dates(&settings, day(from), 3), expected);
    }
}

#[test]
fn exhausted_conversion_keeps_open_steps_and_nested_repeats() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    let root = library.create_task("Outer", None).unwrap();
    let open = library.create_task("Open", Some(root)).unwrap();
    let inner = library.create_task("Inner", Some(root)).unwrap();
    let nested = library.create_task("Nested", Some(inner)).unwrap();
    library
        .set_recurrence(inner, Some(RecurrenceSettings::new(Rule::default(), start)), start)
        .unwrap();
    library
        .set_recurrence(
            root,
            Some(RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=1").unwrap(), start)),
            start,
        )
        .unwrap();
    library.check_action(nested, true, start).unwrap();
    library.check_action(root, true, start).unwrap();
    library.set_recurrence(root, None, start).unwrap();
    assert_eq!(library.note_on(root, start).unwrap().action.unwrap().state, ActionState::Done);
    for step in [open, nested] {
        assert_eq!(library.note_on(step, start).unwrap().action.unwrap().state, ActionState::Open);
    }
    assert!(library.note(inner).unwrap().action.unwrap().recur.is_some());
}

#[test]
fn migration_leaves_concurrent_handling_writes_in_control() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    seed.set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), start)), start).unwrap();
    let target = seed.current_occurrence(id, start).unwrap().target;
    seed.check_action(id, true, start).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    legacy.get_map("meta").insert("version", 7).unwrap();
    for root in ["recurrence_handled", "recurrence_settled"] {
        let records = legacy.get_map(root);
        let key = records.keys().into_iter().next().unwrap();
        let LoroValue::Map(value) = records.get(&key).unwrap().get_deep_value() else { panic!() };
        records
            .insert(
                &key,
                LoroValue::Map(
                    value
                        .iter()
                        .filter(|(key, _)| key.as_str() != "identity_version")
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                ),
            )
            .unwrap();
    }
    legacy.commit();
    let snapshot = legacy.export(ExportMode::Snapshot).unwrap();
    let mut laptop = Library::in_memory();
    let memo = laptop.create_memo("Other work").unwrap();
    for index in 0..50 {
        laptop.set_body(memo, &format!("Other work {index}")).unwrap();
    }
    laptop.import_snapshot(&snapshot).unwrap();
    let mut phone = Library::in_memory();
    phone.import_snapshot(&snapshot).unwrap();
    phone.handle_occurrence_target(id, target, Some(OccurrenceState::Skipped), start).unwrap();
    laptop.merge_updates(&phone.export_snapshot().unwrap(), false).unwrap();
    assert_eq!(
        laptop.note(id).unwrap().action.unwrap().recur.unwrap().handled[0].state,
        OccurrenceState::Skipped
    );
}
