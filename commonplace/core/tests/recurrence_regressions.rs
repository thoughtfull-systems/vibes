// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, OccurrenceState,
    RecurrenceSettings, Rule,
};

fn date(s: &str) -> Date {
    s.parse().unwrap()
}

#[test]
fn malformed_rule_fields_return_none_instead_of_panicking() {
    for text in [
        "FREQ=MONTHLY;BYMONTHDAY=-128",
        "FREQ=MONTHLY;BYDAY=-128MO",
        "FREQ=MONTHLY;BYSETPOS=-32768",
    ] {
        let result = std::panic::catch_unwind(|| Rule::parse(text));
        assert!(matches!(result, Ok(None)), "{text}: {result:?}");
    }
}

#[test]
fn selecting_no_repeat_on_a_plain_task_keeps_it_open() {
    let mut library = Library::in_memory();
    let id = library.create_task("Plain", None).unwrap();
    let commits = library.commits();
    library.set_recurrence(id, None, date("2026-10-31")).unwrap();
    assert_eq!(library.note(id).unwrap().action.unwrap().state, ActionState::Open);
    assert_eq!(library.commits(), commits);
}

#[test]
fn an_explicit_move_survives_a_new_holiday_on_its_original_date() {
    let mut library = Library::in_memory();
    let id = library.create_task("Bill", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap(),
        date("2026-11-01"),
    );
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), date("2026-10-31")).unwrap();
    library.move_occurrence(id, date("2026-11-01"), Some(date("2026-11-02"))).unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Holiday", date("2026-11-01"));
    library.set_days_off(&holiday).unwrap();
    let current = library.current_occurrence(id, date("2026-10-31")).unwrap();
    assert_eq!(current.date, date("2026-11-01"));
    assert_eq!(current.on, date("2026-11-02"));
}

#[test]
fn a_long_vacation_keeps_every_occurrence_that_has_not_started() {
    let mut library = Library::in_memory();
    let id = library.create_task("Daily", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY").unwrap(), date("2026-01-01"));
    settings.missed = Missed::All;
    settings.days_off.vacations = DayOffEffect::Move;
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-01-01"));
    trip.last = date("2026-03-31");
    library.set_days_off(&trip).unwrap();
    library.set_recurrence(id, Some(settings), date("2026-01-01")).unwrap();
    library.move_occurrence(id, date("2026-02-20"), Some(date("2026-02-19"))).unwrap();
    library
        .handle_occurrence(id, date("2026-02-20"), Some(OccurrenceState::Done), date("2026-02-20"))
        .unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2026-02-20")).unwrap().date,
        date("2026-01-01"),
    );
}

#[test]
fn reducing_the_start_offset_does_not_revive_missed_occurrences() {
    let mut library = Library::in_memory();
    let id = library.create_task("Review", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap(), date("2026-10-05"));
    settings.missed = Missed::All;
    settings.defer = Some(-14);
    library.set_recurrence(id, Some(settings.clone()), date("2026-10-05")).unwrap();
    library
        .handle_occurrence(id, date("2026-10-19"), Some(OccurrenceState::Done), date("2026-10-05"))
        .unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2026-10-06")).unwrap().date,
        date("2026-10-26"),
    );
    settings.defer = Some(0);
    library.set_recurrence(id, Some(settings), date("2026-10-06")).unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2026-10-06")).unwrap().date,
        date("2026-10-26"),
    );
}

#[test]
fn a_manual_move_to_the_original_date_overrides_days_off() {
    let mut library = Library::in_memory();
    let id = library.create_task("Bill", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap(),
        date("2026-11-01"),
    );
    settings.days_off.holidays = DayOffEffect::Move;
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Holiday", date("2026-11-01"));
    library.set_days_off(&holiday).unwrap();
    library.set_recurrence(id, Some(settings), date("2026-10-31")).unwrap();
    library.move_occurrence(id, date("2026-11-01"), Some(date("2026-11-01"))).unwrap();
    assert_eq!(library.current_occurrence(id, date("2026-10-31")).unwrap().on, date("2026-11-01"),);
}

#[test]
fn missed_occurrences_stay_missed_after_moves_days_off_and_sync() {
    let mut laptop = Library::in_memory();
    let id = laptop.create_task("Review", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap(), date("2026-10-05"));
    settings.missed = Missed::All;
    settings.days_off.vacations = DayOffEffect::Move;
    laptop.set_recurrence(id, Some(settings), date("2026-10-05")).unwrap();
    laptop
        .handle_occurrence(id, date("2026-10-19"), Some(OccurrenceState::Done), date("2026-10-20"))
        .unwrap();
    let mut phone = Library::in_memory();
    phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
    phone.move_occurrence(id, date("2026-10-05"), Some(date("2026-12-01"))).unwrap();
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-10-12"));
    trip.last = date("2026-10-25");
    phone.set_days_off(&trip).unwrap();
    let updates = phone.updates_since(&laptop.version()).unwrap().unwrap();
    laptop.merge_updates(&updates, false).unwrap();
    for library in [&laptop, &phone] {
        assert_eq!(
            library.current_occurrence(id, date("2026-10-21")).unwrap().date,
            date("2026-10-26"),
        );
    }
    phone.handle_occurrence(id, date("2026-10-19"), None, date("2026-10-21")).unwrap();
    assert_eq!(phone.current_occurrence(id, date("2026-10-21")).unwrap().date, date("2026-10-12"),);
}

#[test]
fn moved_in_occurrences_keep_their_missed_effect_after_offset_edits() {
    let mut library = Library::in_memory();
    let id = library.create_task("Review", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap(), date("2026-10-05"));
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings.clone()), date("2026-10-05")).unwrap();
    library.move_occurrence(id, date("2026-11-02"), Some(date("2026-10-20"))).unwrap();
    library.move_occurrence(id, date("2026-11-09"), Some(date("2026-10-20"))).unwrap();
    library
        .handle_occurrence(id, date("2026-11-09"), Some(OccurrenceState::Done), date("2026-10-21"))
        .unwrap();
    settings.defer = Some(30);
    library.set_recurrence(id, Some(settings), date("2026-10-21")).unwrap();
    library.check_action(id, true, date("2026-12-10")).unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2026-12-10")).unwrap().date,
        date("2026-11-16"),
    );
}
