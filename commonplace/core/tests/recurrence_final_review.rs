// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    Date, DayOffEffect, DaysOff, DaysOffKind, Ends, Error, Library, Missed, OccurrenceState,
    RecurrenceSettings, RepeatForm, Rule,
};
use loro::{Container, ExportMode, LoroDoc, LoroMap, ValueOrContainer};

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

fn copy(source: &Library) -> Library {
    let mut result = Library::in_memory();
    result.import_snapshot(&source.export_snapshot().unwrap()).unwrap();
    result
}

#[test]
fn invalid_offsets_return_an_error_without_changing_the_action() {
    for offset in [i32::MIN, -1000, 1000, i32::MAX] {
        for due in [false, true] {
            let start = day("2026-11-01");
            let mut library = Library::in_memory();
            let id = library.create_task("Repeat", None).unwrap();
            let mut settings = RecurrenceSettings::new(Rule::default(), start);
            if due {
                settings.due = Some(offset);
            } else {
                settings.defer = Some(offset);
            }
            assert!(matches!(
                library.set_recurrence(id, Some(settings), start),
                Err(Error::InvalidOffset)
            ));
            assert!(library.note(id).unwrap().action.unwrap().recur.is_none());
        }
    }
}

#[test]
fn inclusive_offset_limits_produce_dates_in_both_directions() {
    let start = day("2026-11-01");
    for offset in [-999, 999] {
        let mut library = Library::in_memory();
        let id = library.create_task("Repeat", None).unwrap();
        let mut settings = RecurrenceSettings::new(Rule::default(), start);
        settings.defer = Some(offset);
        settings.due = Some(offset);
        settings.missed = Missed::All;
        library.set_recurrence(id, Some(settings), start).unwrap();
        let current = library.current_occurrence(id, start).unwrap();
        assert_eq!(current.defer, Some(start.add_days(offset)));
        assert_eq!(current.due, current.defer);
    }
}

#[test]
fn invalid_stored_offsets_stay_in_the_document_but_do_not_affect_scheduling() {
    let start = day("2026-11-01");
    let mut library = Library::in_memory();
    let id = library.create_task("Repeat", None).unwrap();
    library
        .set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), start)), start)
        .unwrap();
    let doc = LoroDoc::new();
    doc.import(&library.export_snapshot().unwrap()).unwrap();
    let map = |parent: &LoroMap, key: &str| match parent.get(key) {
        Some(ValueOrContainer::Container(Container::Map(map))) => map,
        _ => panic!("missing map {key}"),
    };
    let recur = map(&map(&map(&doc.get_map("notes"), &id.to_string()), "action"), "recur");
    recur.insert("defer", i32::MIN).unwrap();
    recur.insert("due", i32::MAX).unwrap();
    library.import_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    let current = library.current_occurrence(id, start).unwrap();
    assert_eq!(current.defer, Some(start));
    assert_eq!(current.due, None);
    let saved = LoroDoc::new();
    saved.import(&library.export_snapshot().unwrap()).unwrap();
    let recur = map(&map(&map(&saved.get_map("notes"), &id.to_string()), "action"), "recur");
    assert_eq!(recur.get("defer").unwrap().get_deep_value(), i32::MIN.into());
    assert_eq!(recur.get("due").unwrap().get_deep_value(), i32::MAX.into());
}

#[test]
fn changing_only_the_ending_preserves_original_calendar_selectors() {
    let start = day("2026-10-06");
    for text in [
        "FREQ=MONTHLY",
        "FREQ=MONTHLY;BYDAY=6MO",
        "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU;WKST=MO",
        "FREQ=YEARLY",
    ] {
        let rule = Rule::parse(text).unwrap();
        let mut form = RepeatForm::from_rule(&rule, start).unwrap();
        assert_eq!(form.apply_to(&rule, start), rule);
        form.ends = Ends::After { count: 20 };
        let updated = form.apply_to(&rule, start);
        assert_eq!(updated, Rule { count: Some(20), ..rule.clone() });
        form.interval += 1;
        assert_eq!(form.apply_to(&rule, start), form.rule());
    }
}

#[test]
fn a_loro_winning_reopen_is_not_settled_by_the_losing_completion() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Repeat", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=1").unwrap(), start);
    seed.set_recurrence(id, Some(settings), start).unwrap();
    let target = seed.current_occurrence(id, start).unwrap().target;
    let mut left = copy(&seed);
    let mut right = copy(&seed);
    for index in 0..150 {
        let mut branch = copy(&seed);
        branch.set_setting(&format!("parallel-{index}"), index).unwrap();
        left.merge_updates(&branch.export_snapshot().unwrap(), false).unwrap();
    }
    for index in 0..50 {
        right.set_setting("serial", index).unwrap();
    }
    left.handle_occurrence_target(id, target, Some(OccurrenceState::Done), start).unwrap();
    right.handle_occurrence_target(id, target, Some(OccurrenceState::Done), start).unwrap();
    right.handle_occurrence_target(id, target, None, start).unwrap();
    let left_updates = left.export_snapshot().unwrap();
    let right_updates = right.export_snapshot().unwrap();
    left.merge_updates(&right_updates, false).unwrap();
    right.merge_updates(&left_updates, false).unwrap();
    for replica in [&left, &right] {
        let recur = replica.note(id).unwrap().action.unwrap().recur.unwrap();
        assert_eq!(recur.reopened.len(), 1, "the reopen must win the record conflict");
        assert_eq!(replica.current_occurrence(id, start).map(|o| o.target), Some(target));
    }
    assert_eq!(left.occurrences(id, start, 10, 1), right.occurrences(id, start, 10, 1));
}

#[test]
fn paginating_history_keeps_the_same_most_recent_rows() {
    let start = day("2026-01-01");
    let handled = day("2026-05-11");
    for offset in [-20, 0, 20] {
        let today = day("2026-05-12").add_days(offset.max(0));
        let mut library = Library::in_memory();
        let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Day off", start);
        library.set_days_off(&holiday).unwrap();
        let id = library.create_task("Daily", None).unwrap();
        let mut settings = RecurrenceSettings::new(Rule::default(), start);
        settings.rule.count = Some(200);
        settings.days_off.holidays = DayOffEffect::Skip;
        library.set_recurrence(id, Some(settings.clone()), start).unwrap();
        library
            .handle_occurrence(id, handled, Some(OccurrenceState::Done), handled.add_days(-1))
            .unwrap();
        holiday.last = handled.add_days(-1);
        library.set_days_off(&holiday).unwrap();
        settings.days_off.holidays = DayOffEffect::Move;
        settings.defer = Some(offset);
        library.set_recurrence(id, Some(settings), handled).unwrap();
        let history = |past| {
            library
                .occurrences(id, today, past, 0)
                .into_iter()
                .filter(|entry| !entry.upcoming)
                .map(|entry| (entry.date, entry.on, entry.status))
                .collect::<Vec<_>>()
        };
        let wide = history(200);
        for past in [1, 2, 20, 25, 60, 99] {
            assert_eq!(
                history(past),
                wide.iter().copied().take(past as usize).collect::<Vec<_>>(),
                "offset {offset}, past {past}"
            );
        }
    }
}
