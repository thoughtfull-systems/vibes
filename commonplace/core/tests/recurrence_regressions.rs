// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Library, Missed, OccurrenceState,
    RecurrenceSettings, Rule, Schedule, Times,
};
use loro::{Container, ExportMode, LoroDoc, ValueOrContainer};

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

#[test]
fn a_legacy_step_moved_under_a_repeat_keeps_both_devices_completions() {
    let mut seed = Library::in_memory();
    let parent = seed.create_task("Daily project", None).unwrap();
    let step = seed.create_task("Existing task", None).unwrap();
    let legacy = LoroDoc::new();
    legacy.import(&seed.export_snapshot().unwrap()).unwrap();
    let Some(ValueOrContainer::Container(Container::Map(note))) =
        legacy.get_map("notes").get(&step.to_string())
    else {
        panic!()
    };
    let Some(ValueOrContainer::Container(Container::Map(action))) = note.get("action") else {
        panic!()
    };
    action.delete("occurrences").unwrap();
    legacy.commit();
    let mut laptop = Library::in_memory();
    laptop.merge_updates(&legacy.export(ExportMode::Snapshot).unwrap(), false).unwrap();
    let settings = RecurrenceSettings::new(Rule::default(), date("2026-11-01"));
    laptop.set_recurrence(parent, Some(settings), date("2026-11-01")).unwrap();
    laptop.move_action(step, Some(parent), None).unwrap();
    let mut phone = Library::in_memory();
    phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
    laptop.check_action(step, true, date("2026-11-01")).unwrap();
    phone.check_action(step, true, date("2026-11-02")).unwrap();
    let updates = phone.updates_since(&laptop.version()).unwrap().unwrap();
    laptop.merge_updates(&updates, false).unwrap();
    let dates: Vec<_> = laptop
        .note(step)
        .unwrap()
        .action
        .unwrap()
        .occurrences
        .into_iter()
        .map(|entry| entry.date)
        .collect();
    assert_eq!(dates, [date("2026-11-01"), date("2026-11-02")]);
}

#[test]
fn after_schedules_keep_explicit_moves_when_holidays_change() {
    let mut library = Library::in_memory();
    let id = library.create_task("Water", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap(),
        date("2026-11-01"),
    );
    settings.missed = Missed::After;
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings), date("2026-10-31")).unwrap();
    library.move_occurrence(id, date("2026-11-01"), Some(date("2026-11-02"))).unwrap();
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Holiday", date("2026-11-01"));
    library.set_days_off(&holiday).unwrap();
    let current = library.current_occurrence(id, date("2026-10-31")).unwrap();
    assert_eq!((current.date, current.on), (date("2026-11-01"), date("2026-11-02")));
}

#[test]
fn after_schedules_count_from_the_most_recent_handling_day() {
    let mut library = Library::in_memory();
    let id = library.create_task("Water", None).unwrap();
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY;INTERVAL=7").unwrap(), date("2026-10-01"));
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), date("2026-10-01")).unwrap();
    library
        .handle_occurrence(id, date("2026-10-15"), Some(OccurrenceState::Done), date("2026-10-01"))
        .unwrap();
    library.check_action(id, true, date("2026-10-10")).unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2026-10-10")).unwrap().date,
        date("2026-10-17")
    );
}

#[test]
fn exclusions_cannot_add_occurrences_to_a_counted_partial_first_period() {
    let mut library = Library::in_memory();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1;COUNT=1").unwrap(),
        date("2026-11-15"),
    );
    settings.days_off.vacations = DayOffEffect::Skip;
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Trip", date("2026-11-01"));
    trip.last = date("2026-11-15");
    library.set_days_off(&trip).unwrap();
    assert_eq!(library.preview_dates(&settings, settings.start, 3), [date("2026-11-16")]);
}

#[test]
fn history_keeps_nearer_dates_when_an_occurrence_moves_years_earlier() {
    let mut library = Library::in_memory();
    let id = library.create_task("Review", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::parse("FREQ=YEARLY").unwrap(), date("2026-11-01"));
    library.set_recurrence(id, Some(settings), date("2026-10-31")).unwrap();
    library.move_occurrence(id, date("2028-11-01"), Some(date("2026-10-31"))).unwrap();
    let history = library.occurrences(id, date("2026-10-31"), 10, 5);
    assert_eq!(history[0].date, date("2028-11-01"));
    assert_eq!(history[1].date, date("2026-11-01"));
}

#[test]
fn occurrences_move_past_vacations_longer_than_a_year() {
    let mut library = Library::in_memory();
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Sabbatical", date("2026-01-01"));
    trip.last = date("2028-01-03");
    library.set_days_off(&trip).unwrap();
    let id = library.create_task("Daily", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), date("2026-01-01"));
    settings.missed = Missed::All;
    settings.days_off.vacations = DayOffEffect::Move;
    library.set_recurrence(id, Some(settings), date("2026-01-01")).unwrap();
    assert_eq!(library.current_occurrence(id, date("2026-01-01")).unwrap().on, date("2028-01-04"));
    library.move_occurrence(id, date("2027-12-01"), Some(date("2027-12-01"))).unwrap();
    library.check_action(id, true, date("2027-12-01")).unwrap();
    assert_eq!(
        library.current_occurrence(id, date("2027-12-01")).unwrap().date,
        date("2026-01-01")
    );
}

#[test]
fn journal_prompts_move_past_vacations_longer_than_a_year() {
    let mut library = Library::in_memory();
    let journal = library.create_journal("Log").unwrap();
    let mut schedule = Schedule::new(
        Rule::parse("FREQ=YEARLY;COUNT=1").unwrap(),
        date("2026-01-01"),
        Times::Fixed(vec![600]),
    );
    schedule.days_off.vacations = DayOffEffect::Move;
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Sabbatical", date("2026-01-01"));
    trip.last = date("2027-01-03");
    library.set_days_off(&trip).unwrap();
    library.set_schedule(journal, &schedule).unwrap();
    assert!(library.schedule_minutes(journal, &schedule, date("2026-01-01")).is_empty());
    assert!(library.schedule_minutes(journal, &schedule, date("2027-01-03")).is_empty());
    assert_eq!(library.schedule_minutes(journal, &schedule, date("2027-01-04")), [600]);
}

#[test]
fn repeating_vacations_keep_their_full_span_and_join_adjacent_days_off() {
    let mut library = Library::in_memory();
    let id = library.create_task("Review", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), date("2026-01-01"));
    settings.missed = Missed::All;
    settings.days_off.vacations = DayOffEffect::Move;
    settings.days_off.holidays = DayOffEffect::Skip;
    let mut trip = DaysOff::new(DaysOffKind::Vacations, "Sabbatical", date("2026-01-01"));
    trip.last = date("2027-01-03");
    trip.rule = Rule::parse("FREQ=YEARLY;INTERVAL=3");
    library.set_days_off(&trip).unwrap();
    assert!(trip.covers(date("2027-01-03")));
    assert!(trip.covers(date("2030-01-03")));
    assert!(!trip.covers(date("2027-01-04")));
    let holiday = DaysOff::new(DaysOffKind::Holidays, "Holiday", date("2027-01-04"));
    library.set_days_off(&holiday).unwrap();
    library.set_recurrence(id, Some(settings), date("2026-01-01")).unwrap();
    assert_eq!(library.current_occurrence(id, date("2026-01-01")).unwrap().on, date("2027-01-05"));
}
