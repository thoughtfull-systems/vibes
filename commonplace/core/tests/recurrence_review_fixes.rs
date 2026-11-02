// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionChange, ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed,
    OccurrenceState, RecurrenceSettings, Rule, SeriesState,
};

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn mixed_positions_cannot_create_ambiguous_nominal_slots() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let id = library.create_task("Mondays", None).unwrap();
    for positions in [vec![1, -5], vec![-5, 1], vec![2, -4], vec![1, -1]] {
        let mut rule = Rule::parse("FREQ=MONTHLY;BYDAY=MO").unwrap();
        rule.positions = positions;
        assert!(!rule.is_valid());
        assert!(Rule::parse(&rule.to_string()).is_none());
        let settings = RecurrenceSettings::new(rule, start);
        assert!(library.preview_dates(&settings, start, 3).is_empty());
        assert!(library.set_recurrence(id, Some(settings), start).is_err());
    }
}

#[test]
fn positions_from_one_end_keep_each_slots_identity() {
    for positions in ["1,2", "-2,-1"] {
        let mut library = Library::in_memory();
        let start = day("2026-11-01");
        let id = library.create_task("Mondays", None).unwrap();
        let rule =
            Rule::parse(&format!("FREQ=MONTHLY;BYDAY=MO;BYSETPOS={positions};COUNT=2")).unwrap();
        let mut settings = RecurrenceSettings::new(rule, start);
        settings.missed = Missed::All;
        settings.days_off.holidays = DayOffEffect::Skip;
        library.set_recurrence(id, Some(settings), start).unwrap();
        let before = library.occurrences(id, start, 0, 2);
        let date = if positions.starts_with('-') { "2026-11-30" } else { "2026-11-02" };
        let holiday = DaysOff::new(DaysOffKind::Holidays, "Monday off", day(date));
        library.set_days_off(&holiday).unwrap();
        let after = library.occurrences(id, start, 0, 2);
        assert_eq!(before.len(), 2);
        assert_eq!(after.len(), 2);
        for (old, new) in before.iter().zip(&after) {
            assert_eq!(old.target, new.target);
            assert_ne!(old.on, new.on);
        }
        assert_ne!(after[0].on, after[1].on);
    }
}

#[test]
fn reopening_preserves_the_settlement_of_other_slots_after_sync() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let today = day("2026-11-10");
    let tomorrow = today.add_days(1);
    let id = library.create_task("Daily", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=10").unwrap(), start);
    library.set_recurrence(id, Some(settings), start).unwrap();
    let closed = library.current_occurrence(id, today).unwrap().target;
    library.handle_occurrence_target(id, closed, Some(OccurrenceState::Done), today).unwrap();
    let mut peer = Library::in_memory();
    peer.merge_updates(&library.export_snapshot().unwrap(), false).unwrap();
    library.move_occurrence(id, start, Some(tomorrow)).unwrap();
    peer.handle_occurrence_target(id, closed, None, tomorrow).unwrap();
    library
        .merge_updates(&peer.updates_since(&library.version()).unwrap().unwrap(), false)
        .unwrap();
    assert_eq!(library.current_occurrence(id, tomorrow).unwrap().target, closed);
    library.handle_occurrence_target(id, closed, Some(OccurrenceState::Done), today).unwrap();
    assert!(library.current_occurrence(id, tomorrow).is_none());
    library.handle_occurrence_target(id, closed, None, tomorrow).unwrap();
    assert_eq!(library.current_occurrence(id, tomorrow).unwrap().target, closed);
}

#[test]
fn removing_recurrence_copies_open_steps_over_permanent_completions() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let parent = library.create_task("Routine", None).unwrap();
    let step = library.create_task("Done before joining routine", None).unwrap();
    library.check_action(step, true, today).unwrap();
    library
        .set_recurrence(parent, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    library.move_action(step, Some(parent), None).unwrap();
    assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Open);
    library.set_recurrence(parent, None, today).unwrap();
    let step = library.note_on(step, today).unwrap().action.unwrap();
    assert_eq!(step.state, ActionState::Open);
    assert_eq!(step.closed, None);
}

#[test]
fn blocked_recurrence_suppresses_plain_dates_and_their_inheritance() {
    let mut library = Library::in_memory();
    let today = day("2026-11-01");
    let id = library.create_task("Water", None).unwrap();
    let step = library.create_task("Step", Some(id)).unwrap();
    library.change_action(id, ActionChange::Due(Some(today.add_days(-10)))).unwrap();
    library.change_action(id, ActionChange::Defer(Some(today.add_days(10)))).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.due = None;
    settings.days_off.holidays = DayOffEffect::Move;
    library.set_recurrence(id, Some(settings), today).unwrap();
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Time off", today);
    holiday.last = day("9998-12-31");
    library.set_days_off(&holiday).unwrap();
    assert_eq!(library.occurrence_summary(id, today).unwrap().state, SeriesState::Blocked);
    let action = library.note_on(id, today).unwrap().action.unwrap();
    assert_eq!(action.due, None);
    assert_eq!(action.defer, None);
    let dates = library.action_dates(today);
    assert_eq!(dates[&step].due, None);
    assert_eq!(dates[&step].start, None);
}

#[test]
fn completion_relative_commands_respect_shortened_end_dates() {
    let mut library = Library::in_memory();
    let start = day("2026-11-01");
    let id = library.create_task("Water", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY;UNTIL=20261130").unwrap(), start);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings.clone()), start).unwrap();
    let stale = library.occurrences(id, start, 0, 1)[1].target;
    settings.rule.until = Some(start);
    library.set_recurrence(id, Some(settings), start).unwrap();
    let before = library.commits();
    assert!(
        library.handle_occurrence_target(id, stale, Some(OccurrenceState::Done), start).is_err()
    );
    assert!(library.move_occurrence_target(id, stale, Some(start)).is_err());
    assert!(library.write_occurrence_note_target(id, stale).is_err());
    assert_eq!(library.commits(), before);
    assert_eq!(library.current_occurrence(id, start).unwrap().date, start);
}

#[test]
fn settlement_moves_with_its_action_between_libraries() {
    let mut spaces = commonplace_core::Spaces::in_memory(&["home", "work"]);
    let start = day("2026-11-01");
    let today = day("2026-11-10");
    let id = spaces.library_mut("home").unwrap().create_task("Routine", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=10").unwrap(), start);
    spaces.set_recurrence(id, Some(settings), start).unwrap();
    let closed = spaces.current_occurrence(id, today).unwrap().target;
    spaces.handle_occurrence_target(id, closed, Some(OccurrenceState::Done), today).unwrap();
    spaces.move_occurrence(id, start, Some(today.add_days(1))).unwrap();
    spaces.move_to_space(id, "work").unwrap();
    spaces.handle_occurrence_target(id, closed, None, today).unwrap();
    assert_eq!(spaces.current_occurrence(id, today.add_days(1)).unwrap().target, closed);
}

#[test]
fn concurrent_handling_of_one_slot_keeps_both_settlement_events() {
    let mut laptop = Library::in_memory();
    let start = day("2026-11-01");
    let today = day("2026-11-10");
    let id = laptop.create_task("Routine", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=DAILY;COUNT=10").unwrap(), start);
    laptop.set_recurrence(id, Some(settings), start).unwrap();
    let target = laptop.current_occurrence(id, today).unwrap().target;
    let mut phone = Library::in_memory();
    phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
    laptop.handle_occurrence_target(id, target, Some(OccurrenceState::Done), today).unwrap();
    phone.move_occurrence(id, start, Some(today.add_days(10))).unwrap();
    for sequence in 0..100 {
        phone.set_setting("test.sequence", sequence).unwrap();
    }
    phone.handle_occurrence_target(id, target, Some(OccurrenceState::Done), today).unwrap();
    let winning = phone.note(id).unwrap().action.unwrap().recur.unwrap().handled[0];
    laptop.merge_updates(&phone.updates_since(&laptop.version()).unwrap().unwrap(), false).unwrap();
    let recur = laptop.note(id).unwrap().action.unwrap().recur.unwrap();
    assert_eq!(recur.handled, [winning]);
    assert_eq!(recur.reaches.len(), 2);
    assert!(laptop.current_occurrence(id, today.add_days(10)).is_none());
}
