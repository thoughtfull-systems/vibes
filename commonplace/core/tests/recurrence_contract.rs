// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception
use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, OccurrenceState,
    OccurrenceStatus, RecurrenceSettings, Rule,
};

fn day(value: &str) -> Date {
    value.parse().unwrap()
}

fn copy_library(source: &Library) -> Library {
    let mut result = Library::in_memory();
    result.merge_updates(&source.export_snapshot().unwrap(), false).unwrap();
    result
}

fn merge_both(left: &mut Library, right: &mut Library) {
    let updates = right.updates_since(&left.version()).unwrap().unwrap();
    left.merge_updates(&updates, false).unwrap();
    let updates = left.updates_since(&right.version()).unwrap().unwrap();
    right.merge_updates(&updates, false).unwrap();
}

#[test]
fn completion_relative_cycles_must_not_collide_on_a_calendar_date() {
    let mut library = Library::in_memory();
    let first = day("2026-11-01");
    let id = library.create_task("Water", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY;INTERVAL=7;COUNT=3").unwrap(), first);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), first).unwrap();
    library.check_action(id, true, first).unwrap();
    let second = library.current_occurrence(id, first).unwrap();
    library.move_occurrence(id, second.date, Some(first)).unwrap();
    library.handle_occurrence(id, second.date, Some(OccurrenceState::Done), first).unwrap();
    assert_eq!(library.current_occurrence(id, first).unwrap().on, first.add_days(7));
}

#[test]
fn days_off_edits_must_not_duplicate_a_completed_monthly_slot() {
    let mut library = Library::in_memory();
    let first = day("2026-12-01");
    let handled = day("2026-12-30");
    let id = library.create_task("Last workday", None).unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", day("2026-12-31"));
    library.set_days_off(&holiday).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=1").unwrap(),
        first,
    );
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), first).unwrap();
    assert_eq!(library.current_occurrence(id, handled).unwrap().on, handled);
    library.check_action(id, true, handled).unwrap();
    assert!(library.current_occurrence(id, handled).is_none());
    library.archive_days_off(DaysOffKind::Holidays, &holiday.id, true, handled).unwrap();
    assert!(
        library.current_occurrence(id, handled).is_none(),
        "completed COUNT=1 slot returned as {:?}",
        library.current_occurrence(id, handled)
    );
}

#[test]
fn concurrent_recurrence_creation_must_keep_distinct_completions() {
    let first = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let mut laptop = copy_library(&seed);
    let mut phone = copy_library(&seed);
    let mut settings = RecurrenceSettings::new(Rule::default(), first);
    settings.missed = Missed::All;
    for library in [&mut laptop, &mut phone] {
        library.set_recurrence(id, Some(settings.clone()), first).unwrap();
    }
    laptop.handle_occurrence(id, first, Some(OccurrenceState::Done), first).unwrap();
    phone
        .handle_occurrence(id, first.add_days(1), Some(OccurrenceState::Done), first.add_days(1))
        .unwrap();
    merge_both(&mut laptop, &mut phone);
    for library in [&laptop, &phone] {
        let handled = library.note(id).unwrap().action.unwrap().recur.unwrap().handled;
        assert_eq!(handled.len(), 2, "lost a completion: {handled:?}");
    }
}

#[test]
fn independent_setting_edits_should_merge() {
    let first = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::default(), first);
    seed.set_recurrence(id, Some(settings.clone()), first).unwrap();
    let mut laptop = copy_library(&seed);
    let mut phone = copy_library(&seed);
    let mut left = settings.clone();
    left.due = Some(3);
    let mut right = settings;
    right.days_off.holidays = DayOffEffect::Skip;
    laptop.set_recurrence(id, Some(left), first).unwrap();
    phone.set_recurrence(id, Some(right), first).unwrap();
    merge_both(&mut laptop, &mut phone);
    for library in [&laptop, &phone] {
        let settings = library.note(id).unwrap().action.unwrap().recur.unwrap().settings;
        assert_eq!(settings.due, Some(3));
        assert_eq!(settings.days_off.holidays, DayOffEffect::Skip);
    }
}

#[test]
fn shared_recurrence_keeps_independent_completions() {
    let first = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), first);
    settings.missed = Missed::All;
    seed.set_recurrence(id, Some(settings), first).unwrap();
    let mut laptop = copy_library(&seed);
    let mut phone = copy_library(&seed);
    laptop.handle_occurrence(id, first, Some(OccurrenceState::Done), first).unwrap();
    phone
        .handle_occurrence(id, first.add_days(1), Some(OccurrenceState::Done), first.add_days(1))
        .unwrap();
    merge_both(&mut laptop, &mut phone);
    for library in [&laptop, &phone] {
        assert_eq!(library.note(id).unwrap().action.unwrap().recur.unwrap().handled.len(), 2);
    }
}

#[test]
fn keep_each_one_retains_and_reopens_old_bills() {
    let mut library = Library::in_memory();
    let first = day("2026-01-01");
    let march = day("2026-03-01");
    let id = library.create_task("Bills", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=MONTHLY").unwrap(), first);
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings), first).unwrap();
    library.handle_occurrence(id, first, Some(OccurrenceState::Done), first).unwrap();
    library.handle_occurrence(id, march, Some(OccurrenceState::Done), march).unwrap();
    library.handle_occurrence(id, first, None, march).unwrap();
    assert_eq!(library.current_occurrence(id, march).unwrap().date, first);
    let january = library
        .occurrences(id, march, 10, 5)
        .into_iter()
        .find(|occurrence| occurrence.date == first)
        .unwrap();
    assert_eq!(january.status, OccurrenceStatus::Current);
}

#[test]
fn latest_should_follow_effective_start_order_as_decision_0006_says() {
    let mut library = Library::in_memory();
    let first = day("2026-11-01");
    let today = day("2026-11-10");
    let id = library.create_task("Review", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;COUNT=3").unwrap(), first);
    library.set_recurrence(id, Some(settings), first).unwrap();
    library.move_occurrence(id, first, Some(today)).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().on, today);
}

#[test]
fn an_edited_after_start_is_eligible_without_skipping_an_interval() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Water", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY;INTERVAL=7").unwrap(), today);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    library.check_action(id, true, today).unwrap();
    settings.start = day("2026-12-01");
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().on, settings.start);
}

#[test]
fn an_outer_repeat_does_not_permanently_complete_an_inner_repeats_steps() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let outer = library.create_task("Outer", None).unwrap();
    let inner = library.create_task("Inner", Some(outer)).unwrap();
    let step = library.create_task("Step", Some(inner)).unwrap();
    library
        .set_recurrence(
            inner,
            Some(RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY").unwrap(), today.add_days(1))),
            today,
        )
        .unwrap();
    library
        .set_recurrence(outer, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    library.check_action(step, true, today).unwrap();
    library.set_recurrence(outer, None, today).unwrap();
    assert_eq!(
        library.note_on(step, today.add_days(8)).unwrap().action.unwrap().state,
        ActionState::Open
    );
}

#[test]
fn a_step_checked_after_exhaustion_does_not_complete_future_cycles() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    let step = library.create_task("Step", Some(id)).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=1").unwrap(), today);
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    library.check_action(id, true, today).unwrap();
    library.check_action(step, true, today).unwrap();
    settings.rule.count = Some(3);
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert_eq!(
        library.note_on(step, today.add_days(1)).unwrap().action.unwrap().state,
        ActionState::Open
    );
}

#[test]
fn a_delayed_checkbox_keeps_its_rendered_cycle_target() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Water", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), today).unwrap();
    let context = library.note_on(id, today).unwrap().action.unwrap().occurrence;
    library.check_action_in(id, true, today, context).unwrap();
    let next = library.current_occurrence(id, today).unwrap();
    library.check_action_in(id, true, today.add_days(2), context).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().target, next.target);
    assert_eq!(library.note(id).unwrap().action.unwrap().recur.unwrap().handled.len(), 1);
}

#[test]
fn calendar_replacement_rejects_an_old_rows_target_and_preserves_history() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    let old = library.current_occurrence(id, today).unwrap();
    library.handle_occurrence_target(id, old.target, Some(OccurrenceState::Done), today).unwrap();
    settings.rule = Rule::parse("FREQ=WEEKLY").unwrap();
    library.set_recurrence(id, Some(settings), today).unwrap();
    let current = library.current_occurrence(id, today).unwrap();
    assert_ne!(current.target.key, old.target.key);
    assert!(matches!(
        library.handle_occurrence_target(id, old.target, None, today),
        Err(commonplace_core::Error::InvalidOccurrence)
    ));
    assert!(
        library
            .occurrences(id, today, 10, 5)
            .iter()
            .any(|occurrence| occurrence.target == old.target && occurrence.handled.is_some())
    );
}

#[test]
fn bounds_and_calendar_policy_edits_keep_identity_but_reactivation_does_not() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    let first = library.current_occurrence(id, today).unwrap().target;
    settings.rule.count = Some(4);
    settings.missed = Missed::All;
    settings.defer = Some(-3);
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().target, first);
    library.set_recurrence(id, None, today).unwrap();
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert_ne!(library.current_occurrence(id, today).unwrap().target.key, first.key);
}

#[test]
fn completion_snapshots_survive_moves_offsets_and_calendar_replacement() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Bill", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.missed = Missed::All;
    settings.defer = Some(-2);
    settings.due = Some(3);
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    let original = library.current_occurrence(id, today).unwrap();
    library
        .handle_occurrence_target(id, original.target, Some(OccurrenceState::Done), today)
        .unwrap();
    library.move_occurrence_target(id, original.target, Some(today.add_days(20))).unwrap();
    settings.due = None;
    settings.defer = Some(30);
    settings.start = today.add_days(1);
    library.set_recurrence(id, Some(settings), today).unwrap();
    let history = library.occurrences(id, today, 10, 5);
    let completed = history.iter().find(|entry| entry.target == original.target).unwrap();
    assert_eq!(
        (completed.on, completed.defer, completed.due),
        (original.on, original.defer, original.due)
    );
}

#[test]
fn position_selected_slots_keep_step_and_memo_records_when_holidays_change() {
    let mut library = Library::in_memory();
    let today = day("2026-12-01");
    let id = library.create_task("Review", None).unwrap();
    let step = library.create_task("Step", Some(id)).unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", day("2026-12-31"));
    library.set_days_off(&holiday).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=1").unwrap(),
        today,
    );
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), today).unwrap();
    let before = library.current_occurrence(id, today).unwrap();
    let memo = library.write_occurrence_note_target(id, before.target).unwrap();
    library.check_action(step, true, today).unwrap();
    library.archive_days_off(DaysOffKind::Holidays, &holiday.id, true, today).unwrap();
    let after = library.current_occurrence(id, today).unwrap();
    assert_eq!(before.target, after.target);
    assert_eq!(after.on, day("2026-12-31"));
    assert_eq!(after.note, Some(memo));
    assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
}

#[test]
fn moving_a_series_to_another_library_keeps_all_its_occurrence_records() {
    let mut spaces = commonplace_core::Spaces::in_memory(&["a", "b"]);
    let today = day("2026-11-01");
    let id = spaces.library_mut("a").unwrap().create_task("Routine", None).unwrap();
    let step = spaces.library_mut("a").unwrap().create_task("Step", Some(id)).unwrap();
    spaces
        .set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    let memo = spaces.write_occurrence_note(id, today).unwrap();
    spaces.check_action(step, true, today).unwrap();
    let target = spaces.current_occurrence(id, today).unwrap().target;
    spaces.move_occurrence_target(id, target, Some(today.add_days(1))).unwrap();
    spaces.move_to_space(id, "b").unwrap();
    assert_eq!(spaces.current_occurrence(id, today).unwrap().note, Some(memo));
    assert_eq!(spaces.current_occurrence(id, today).unwrap().on, today.add_days(1));
    assert_eq!(spaces.note_on(step, today).unwrap().action.unwrap().state, ActionState::Done);
    spaces.handle_occurrence_target(id, target, Some(OccurrenceState::Done), today).unwrap();
    assert!(
        spaces
            .occurrences(id, today, 10, 5)
            .iter()
            .any(|entry| entry.target == target && entry.status == OccurrenceStatus::Done)
    );
}

#[test]
fn reopening_an_after_cycle_keeps_the_origin_that_selected_its_slot() {
    let mut library = Library::in_memory();
    let start = day("2026-10-01");
    let id = library.create_task("Review", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1").unwrap(),
        start,
    );
    settings.missed = Missed::After;
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), start).unwrap();
    let first = library.current_occurrence(id, start).unwrap();
    library
        .handle_occurrence_target(id, first.target, Some(OccurrenceState::Done), first.on)
        .unwrap();
    let second = library.current_occurrence(id, first.on).unwrap();
    library
        .handle_occurrence_target(id, second.target, Some(OccurrenceState::Done), second.on)
        .unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Day off", first.on);
    library.set_days_off(&holiday).unwrap();
    library.handle_occurrence_target(id, first.target, None, second.on).unwrap();
    let reopened = library.current_occurrence(id, second.on).unwrap();
    assert_eq!(reopened.target, first.target);
    assert_eq!(reopened.on, day("2026-10-29"));
}

#[test]
fn removing_a_blocked_repeat_keeps_the_plain_action_open() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Day off", today);
    library.set_days_off(&holiday).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=1").unwrap(), today);
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert_eq!(
        library.occurrence_summary(id, today).unwrap().state,
        commonplace_core::SeriesState::Blocked
    );
    library.set_recurrence(id, None, today).unwrap();
    assert_eq!(library.note(id).unwrap().action.unwrap().state, ActionState::Open);
}

#[test]
fn policy_transitions_keep_records_in_their_calendar_or_cycle_scope() {
    for before in [Missed::After, Missed::All, Missed::Latest] {
        for after in [Missed::After, Missed::All, Missed::Latest] {
            let mut library = Library::in_memory();
            let start = day("2026-11-01");
            let today = start.add_days(3);
            let id = library.create_task("Routine", None).unwrap();
            let step = library.create_task("Step", Some(id)).unwrap();
            let mut settings = RecurrenceSettings::new(
                Rule::parse("FREQ=DAILY;INTERVAL=3;COUNT=3").unwrap(),
                start,
            );
            settings.missed = before;
            library.set_recurrence(id, Some(settings.clone()), start).unwrap();
            library.check_action(id, true, start).unwrap();
            library.check_action(step, true, today).unwrap();
            let old = library.current_occurrence(id, today).unwrap();
            settings.missed = after;
            library.set_recurrence(id, Some(settings), today).unwrap();
            let new = library.current_occurrence(id, today).unwrap();
            let replaced = (before == Missed::After) != (after == Missed::After);
            assert_eq!(old.target.key != new.target.key, replaced);
            assert_eq!(
                library.note_on(step, today).unwrap().action.unwrap().state,
                if replaced { ActionState::Open } else { ActionState::Done }
            );
            assert!(library.occurrences(id, today, 10, 3).iter().any(|o| o.handled.is_some()));
        }
    }
}

#[test]
fn concurrent_calendar_edits_keep_each_rule_and_origin_together() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    seed.set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), start)), start).unwrap();
    let mut laptop = copy_library(&seed);
    let mut phone = copy_library(&seed);
    let left = RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY").unwrap(), start.add_days(1));
    let right = RecurrenceSettings::new(Rule::parse("FREQ=MONTHLY").unwrap(), start.add_days(2));
    laptop.set_recurrence(id, Some(left.clone()), start).unwrap();
    phone.set_recurrence(id, Some(right.clone()), start).unwrap();
    merge_both(&mut laptop, &mut phone);
    for library in [&laptop, &phone] {
        let settings = library.note(id).unwrap().action.unwrap().recur.unwrap().settings;
        assert!(settings == left || settings == right);
    }
}

#[test]
fn until_bounds_nominal_slots_before_days_off_adjust_their_effective_dates() {
    let mut library = Library::in_memory();
    let start = day("2026-12-01");
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", day("2026-12-31"));
    library.set_days_off(&holiday).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;UNTIL=20261230").unwrap(),
        start,
    );
    settings.days_off.holidays = DayOffEffect::Skip;
    assert!(library.preview_dates(&settings, start, 3).is_empty());
    settings.rule.until = Some(day("2026-12-31"));
    assert_eq!(library.preview_dates(&settings, start, 3), [day("2026-12-30")]);
    settings.days_off.holidays = DayOffEffect::Move;
    assert_eq!(library.preview_dates(&settings, start, 3), [day("2027-01-01")]);
}

#[test]
fn a_reopened_after_cycle_blocked_by_days_off_is_not_exhausted() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let last = start.add_days(7);
    let id = library.create_task("Water", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=DAILY;INTERVAL=7;UNTIL=20261108").unwrap(),
        start,
    );
    settings.missed = Missed::After;
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    let first = library.current_occurrence(id, start).unwrap();
    library.handle_occurrence_target(id, first.target, Some(OccurrenceState::Done), start).unwrap();
    library.check_action(id, true, last).unwrap();
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Time off", start);
    holiday.last = last;
    library.set_days_off(&holiday).unwrap();
    library.handle_occurrence_target(id, first.target, None, last).unwrap();
    assert_eq!(
        library.occurrence_summary(id, last).unwrap().state,
        commonplace_core::SeriesState::Blocked
    );
    assert_eq!(library.note_on(id, last).unwrap().action.unwrap().state, ActionState::Open);
    settings.days_off.holidays = DayOffEffect::Ignore;
    library.set_recurrence(id, Some(settings), last).unwrap();
    assert_eq!(library.current_occurrence(id, last).unwrap().target, first.target);
}

#[test]
fn starting_a_blocked_repeat_transfers_plain_completions_to_its_first_logical_slot() {
    for policy in [Missed::After, Missed::All, Missed::Latest] {
        for effect in [DayOffEffect::Move, DayOffEffect::Skip] {
            for done in [false, true] {
                let mut library = Library::in_memory();
                let start = day("2026-11-01");
                let id = library.create_task("Routine", None).unwrap();
                let step = library.create_task("Step", Some(id)).unwrap();
                library
                    .change_action(step, commonplace_core::ActionChange::State(ActionState::Done))
                    .unwrap();
                if done {
                    library
                        .change_action(id, commonplace_core::ActionChange::State(ActionState::Done))
                        .unwrap();
                }
                let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Time off", start);
                holiday.last = day("9998-12-31");
                library.set_days_off(&holiday).unwrap();
                let mut settings =
                    RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=2").unwrap(), start);
                settings.days_off.holidays = effect;
                settings.missed = policy;
                library.set_recurrence(id, Some(settings.clone()), start).unwrap();
                assert_eq!(library.note(step).unwrap().action.unwrap().state, ActionState::Open);
                assert_eq!(library.note(id).unwrap().action.unwrap().state, ActionState::Open);
                assert_eq!(
                    library.occurrence_summary(id, start).unwrap().state,
                    commonplace_core::SeriesState::Blocked
                );
                settings.days_off.holidays = DayOffEffect::Ignore;
                library.set_recurrence(id, Some(settings), start).unwrap();
                assert_eq!(
                    library.note_on(step, start).unwrap().action.unwrap().state,
                    if done { ActionState::Open } else { ActionState::Done }
                );
                assert_eq!(
                    library.note(id).unwrap().action.unwrap().recur.unwrap().handled.len(),
                    usize::from(done)
                );
            }
        }
    }
}

#[test]
fn an_impossible_calendar_preserves_the_plain_completion_when_it_has_no_slot() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let id = library.create_task("Routine", None).unwrap();
    library.change_action(id, commonplace_core::ActionChange::State(ActionState::Done)).unwrap();
    let before = library.note(id).unwrap().action.unwrap();
    let settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYMONTH=2;BYMONTHDAY=30;COUNT=1").unwrap(),
        start,
    );
    library.set_recurrence(id, Some(settings), start).unwrap();
    let after = library.note(id).unwrap().action.unwrap();
    assert_eq!((after.state, after.closed), (before.state, before.closed));
}

#[test]
fn an_outer_repeat_preserves_a_nested_series_stop_record() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let outer = library.create_task("Outer", None).unwrap();
    let inner = library.create_task("Inner", Some(outer)).unwrap();
    let settings = RecurrenceSettings::new(Rule::default(), start);
    library.set_recurrence(inner, Some(settings.clone()), start).unwrap();
    library.change_action(inner, commonplace_core::ActionChange::State(ActionState::Done)).unwrap();
    assert_eq!(
        library.occurrence_summary(inner, start).unwrap().state,
        commonplace_core::SeriesState::Stopped
    );
    library.set_recurrence(outer, Some(settings), start).unwrap();
    library.check_action(inner, true, start).unwrap();
    library.check_action(inner, false, start).unwrap();
    library.set_recurrence(outer, None, start).unwrap();
    assert_eq!(
        library.occurrence_summary(inner, start).unwrap().state,
        commonplace_core::SeriesState::Stopped
    );
    assert_eq!(library.note_on(inner, start).unwrap().action.unwrap().state, ActionState::Done);
}

#[test]
fn independent_activation_keeps_a_series_stop_and_an_occurrence_completion() {
    let start = day("2026-11-01");
    let mut seed = Library::in_memory();
    let id = seed.create_task("Routine", None).unwrap();
    let mut laptop = copy_library(&seed);
    let mut phone = copy_library(&seed);
    let settings = RecurrenceSettings::new(Rule::default(), start);
    for library in [&mut laptop, &mut phone] {
        library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    }
    laptop.change_action(id, commonplace_core::ActionChange::State(ActionState::Done)).unwrap();
    phone.check_action(id, true, start).unwrap();
    merge_both(&mut laptop, &mut phone);
    for library in [&laptop, &phone] {
        let action = library.note(id).unwrap().action.unwrap();
        assert_eq!(action.state, ActionState::Open);
        let recur = action.recur.unwrap();
        assert!(recur.stopped.is_some());
        assert_eq!(recur.handled.len(), 1);
        assert_eq!(
            library.occurrence_summary(id, start).unwrap().state,
            commonplace_core::SeriesState::Stopped
        );
    }
    laptop.check_action(id, false, start).unwrap();
    assert_eq!(laptop.current_occurrence(id, start).unwrap().date, start.add_days(1));
    laptop.change_action(id, commonplace_core::ActionChange::State(ActionState::Done)).unwrap();
    let stopped = laptop.note(id).unwrap().action.unwrap().recur.unwrap().stopped;
    laptop.set_recurrence(id, None, start).unwrap();
    let plain = laptop.note(id).unwrap().action.unwrap();
    assert_eq!(plain.state, ActionState::Done);
    assert_eq!(plain.closed, stopped);
}
