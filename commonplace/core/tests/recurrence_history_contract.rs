// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionChange, ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, NoteId,
    OccurrenceInactiveReason, OccurrenceState, OccurrenceStatus, RecurrenceSettings, Rule,
    SeriesState,
};

fn start() -> Date {
    "2026-11-01".parse().unwrap()
}

fn series(policy: Missed, count: u32) -> (Library, NoteId, RecurrenceSettings) {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), start());
    settings.missed = policy;
    settings.rule.count = Some(count);
    library.set_recurrence(id, Some(settings.clone()), start()).unwrap();
    (library, id, settings)
}

fn copy(source: &Library) -> Library {
    let mut library = Library::in_memory();
    library.import_snapshot(&source.export_snapshot().unwrap()).unwrap();
    library
}

#[test]
fn settled_history_survives_live_offsets_exclusions_and_bounds() {
    for edit in 0..3 {
        let (mut library, id, mut settings) = series(Missed::Latest, 2);
        let today = start().add_days(1);
        let first = library.current_occurrence(id, start()).unwrap().target;
        library.check_action(id, true, today).unwrap();
        match edit {
            0 => settings.defer = Some(20),
            1 => {
                settings.days_off.holidays = DayOffEffect::Skip;
                library
                    .set_days_off(&DaysOff::new(DaysOffKind::Holidays, "Holiday", start()))
                    .unwrap();
            }
            _ => settings.rule.until = Some(start().add_days(-1)),
        }
        if settings.rule.until.is_some() {
            settings.rule.count = None;
        }
        library.set_recurrence(id, Some(settings), today).unwrap();
        for replica in [&library, &copy(&library)] {
            let summary = replica.occurrence_summary(id, today).unwrap();
            assert_eq!(summary.state, SeriesState::Exhausted);
            assert_eq!(summary.missed, 1, "edit {edit}");
            let history = replica.occurrences(id, today, 10, 10);
            let missed = history.iter().find(|entry| entry.target == first).unwrap();
            assert_eq!(missed.status, OccurrenceStatus::Missed);
            assert_eq!(missed.settled, Some(today));
            assert_eq!(missed.on, first.date);
            assert_eq!((missed.defer, missed.due), (None, None));
            assert!(!missed.upcoming);
        }
    }
}

#[test]
fn reopened_records_remain_visible_outside_bounds_until_they_expand() {
    for policy in [Missed::After, Missed::All, Missed::Latest] {
        let (mut library, id, mut settings) = series(policy, 3);
        library.check_action(id, true, start()).unwrap();
        let today = start().add_days(1);
        let second = library.current_occurrence(id, today).unwrap().target;
        library.check_action(id, true, today).unwrap();
        settings.rule.count = Some(1);
        library.set_recurrence(id, Some(settings.clone()), today).unwrap();
        library.handle_occurrence_target(id, second, None, today).unwrap();
        for replica in [&library, &copy(&library)] {
            assert!(replica.current_occurrence(id, today).is_none());
            let history = replica.occurrences(id, today, 10, 10);
            let reopened = history.iter().find(|entry| entry.target == second).unwrap();
            assert_eq!(reopened.status, OccurrenceStatus::Reopened);
            assert_eq!(reopened.handled.unwrap().state, OccurrenceState::Open);
            assert_eq!(
                reopened.inactive_reason,
                Some(if policy == Missed::After {
                    OccurrenceInactiveReason::CycleLimit
                } else {
                    OccurrenceInactiveReason::Bounds
                })
            );
            assert!(!reopened.upcoming);
        }
        settings.rule.count = Some(3);
        library.set_recurrence(id, Some(settings), today).unwrap();
        assert_eq!(library.current_occurrence(id, today).unwrap().target, second);
    }
}

#[test]
fn a_reopened_record_explains_each_lifecycle_restriction() {
    for reason in [
        OccurrenceInactiveReason::DaysOff,
        OccurrenceInactiveReason::Ignored,
        OccurrenceInactiveReason::PreviousCalendar,
        OccurrenceInactiveReason::Stopped,
    ] {
        let (mut library, id, mut settings) = series(Missed::All, 1);
        let first = library.current_occurrence(id, start()).unwrap().target;
        library.check_action(id, true, start()).unwrap();
        library.handle_occurrence_target(id, first, None, start()).unwrap();
        match reason {
            OccurrenceInactiveReason::DaysOff => {
                settings.days_off.holidays = DayOffEffect::Skip;
                library.set_recurrence(id, Some(settings), start()).unwrap();
                library
                    .set_days_off(&DaysOff::new(DaysOffKind::Holidays, "Holiday", start()))
                    .unwrap();
            }
            OccurrenceInactiveReason::Ignored => {
                let parent = library.create_task("Outer repeat", None).unwrap();
                library.set_recurrence(parent, Some(settings), start()).unwrap();
                library.move_action(id, Some(parent), None).unwrap();
            }
            OccurrenceInactiveReason::PreviousCalendar => {
                settings.rule.interval = 2;
                library.set_recurrence(id, Some(settings), start()).unwrap();
            }
            OccurrenceInactiveReason::Stopped => {
                library.change_action(id, ActionChange::State(ActionState::Done)).unwrap();
            }
            _ => unreachable!(),
        }
        for replica in [&library, &copy(&library)] {
            let history = replica.occurrences(id, start(), 10, 5);
            let entry = history.iter().find(|entry| entry.target == first).unwrap();
            assert_eq!(entry.status, OccurrenceStatus::Reopened);
            assert_eq!(entry.inactive_reason, Some(reason));
            assert_eq!(entry.editable, reason != OccurrenceInactiveReason::PreviousCalendar);
            assert!(!entry.upcoming);
        }
    }
}

#[test]
fn concurrent_reopen_and_bounds_edit_preserve_the_dormant_record() {
    for policy in [Missed::After, Missed::All, Missed::Latest] {
        let (mut left, id, mut settings) = series(policy, 3);
        left.check_action(id, true, start()).unwrap();
        let today = start().add_days(1);
        let second = left.current_occurrence(id, today).unwrap().target;
        left.check_action(id, true, today).unwrap();
        let mut right = copy(&left);
        left.handle_occurrence_target(id, second, None, today).unwrap();
        settings.rule.count = Some(1);
        right.set_recurrence(id, Some(settings), today).unwrap();
        let left_updates = left.export_snapshot().unwrap();
        let right_updates = right.export_snapshot().unwrap();
        for _ in 0..2 {
            left.merge_updates(&right_updates, false).unwrap();
            right.merge_updates(&left_updates, false).unwrap();
        }
        assert_eq!(left.occurrences(id, today, 10, 5), right.occurrences(id, today, 10, 5));
        assert!(left.current_occurrence(id, today).is_none());
        assert!(
            left.occurrences(id, today, 10, 5).iter().any(|entry| {
                entry.target == second && entry.status == OccurrenceStatus::Reopened
            })
        );
    }
}

#[test]
fn after_selects_an_eligible_reopened_cycle_past_an_ineligible_earlier_cycle() {
    let (mut library, id, mut settings) = series(Missed::After, 3);
    let mut first = library.current_occurrence(id, start()).unwrap().target;
    first.date = start().add_days(4);
    library.handle_occurrence_target(id, first, Some(OccurrenceState::Done), start()).unwrap();
    let second = library.current_occurrence(id, start()).unwrap().target;
    library.check_action(id, true, start()).unwrap();
    library.handle_occurrence_target(id, first, None, start()).unwrap();
    library.handle_occurrence_target(id, second, None, start()).unwrap();
    settings.rule.count = None;
    settings.rule.until = Some(start().add_days(2));
    library.set_recurrence(id, Some(settings), start()).unwrap();
    assert_eq!(library.current_occurrence(id, start()).unwrap().target, second);
    let history = library.occurrences(id, start(), 10, 2);
    let dormant = history.iter().find(|entry| entry.target == first).unwrap();
    assert_eq!(dormant.inactive_reason, Some(OccurrenceInactiveReason::Bounds));
}

#[test]
fn policy_changes_reinterpret_settlement_but_date_edits_only_restore_superseded_work() {
    let (mut library, id, mut settings) = series(Missed::Latest, 3);
    let today = start().add_days(1);
    let first = library.current_occurrence(id, start()).unwrap().target;
    assert_eq!(library.occurrence_summary(id, today).unwrap().missed, 1);
    assert!(library.occurrences(id, today, 10, 3).iter().any(|entry| {
        entry.target == first && entry.status == OccurrenceStatus::Missed && entry.settled.is_none()
    }));
    settings.defer = Some(20);
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().target, first);
    settings.defer = Some(0);
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    library
        .handle_occurrence(id, start().add_days(2), Some(OccurrenceState::Done), today.add_days(1))
        .unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().target, first);
    settings.missed = Missed::Latest;
    library.set_recurrence(id, Some(settings.clone()), today.add_days(1)).unwrap();
    assert!(library.current_occurrence(id, today.add_days(1)).is_none());
    assert_eq!(library.occurrence_summary(id, today.add_days(1)).unwrap().missed, 2);
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings), today.add_days(1)).unwrap();
    assert_eq!(library.current_occurrence(id, today.add_days(1)).unwrap().target, first);
}

#[test]
fn an_early_completion_stays_upcoming_when_the_series_is_exhausted() {
    let (mut library, id, _) = series(Missed::Latest, 1);
    let earlier = start().add_days(-5);
    library.check_action(id, true, earlier).unwrap();
    assert!(library.current_occurrence(id, earlier).is_none());
    let history = library.occurrences(id, earlier, 10, 5);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, OccurrenceStatus::Done);
    assert!(history[0].upcoming);
}

#[test]
fn concurrent_completion_and_offset_edit_keep_the_same_historical_classification() {
    let (mut left, id, mut settings) = series(Missed::Latest, 2);
    let mut right = copy(&left);
    let today = start().add_days(1);
    let first = left.current_occurrence(id, start()).unwrap().target;
    left.check_action(id, true, today).unwrap();
    settings.defer = Some(20);
    right.set_recurrence(id, Some(settings), today).unwrap();
    left.merge_updates(&right.export_snapshot().unwrap(), false).unwrap();
    right.merge_updates(&left.export_snapshot().unwrap(), false).unwrap();
    for replica in [&left, &right] {
        assert_eq!(replica.occurrence_summary(id, today).unwrap().missed, 1);
        assert!(
            replica
                .occurrences(id, today, 10, 10)
                .iter()
                .any(|entry| { entry.target == first && entry.status == OccurrenceStatus::Missed })
        );
    }
}
