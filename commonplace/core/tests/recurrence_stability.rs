// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{
    ActionState, Date, DayOffEffect, DaysOff, DaysOffKind, Error, Library, Missed, OccurrenceState,
    OccurrenceStatus, RecurrenceSettings, Rule, Spaces,
};
use loro::{Container, ExportMode, LoroDoc, ValueOrContainer};

fn date(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn continuous_days_off_leave_no_occurrence_to_move_to() {
    let mut library = Library::in_memory();
    let mut vacation = DaysOff::new(DaysOffKind::Vacations, "Week off", date("2026-10-31"));
    vacation.last = date("2026-11-06");
    vacation.rule = Rule::parse("FREQ=WEEKLY");
    library.set_days_off(&vacation).unwrap();
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), vacation.first);
    settings.days_off.vacations = DayOffEffect::Move;
    library.set_recurrence(id, Some(settings), vacation.first).unwrap();
    assert!(library.current_occurrence(id, vacation.first).is_none());
    library.archive_days_off(DaysOffKind::Vacations, &vacation.id, true, vacation.first).unwrap();
    assert_eq!(library.current_occurrence(id, vacation.first).unwrap().on, date("2026-11-01"));
}

#[test]
fn stopping_an_ignored_repeat_keeps_the_outer_steps_temporary() {
    let mut library = Library::in_memory();
    let outer = library.create_task("Outer", None).unwrap();
    let inner = library.create_task("Inner", Some(outer)).unwrap();
    let step = library.create_task("Step", Some(inner)).unwrap();
    let today = date("2026-10-31");
    for id in [outer, inner] {
        library
            .set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today)
            .unwrap();
    }
    library.check_action(step, true, today).unwrap();
    library.set_recurrence(inner, None, today).unwrap();
    library.check_action(outer, true, today).unwrap();
    assert_eq!(
        library.note_on(step, today.add_days(1)).unwrap().action.unwrap().state,
        ActionState::Open
    );
}

#[test]
fn reopening_after_uses_handling_order_instead_of_occurrence_order() {
    let mut library = Library::in_memory();
    let id = library.create_task("Water", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=DAILY;INTERVAL=7;COUNT=3").unwrap(),
        date("2026-10-01"),
    );
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), date("2026-10-01")).unwrap();
    library
        .handle_occurrence(id, date("2026-10-15"), Some(OccurrenceState::Done), date("2026-10-01"))
        .unwrap();
    library.check_action(id, true, date("2026-10-02")).unwrap();
    library.check_action(id, true, date("2026-10-03")).unwrap();
    assert!(library.current_occurrence(id, date("2026-10-03")).is_none());
    library.check_action(id, false, date("2026-10-03")).unwrap();
    let handled = library.note(id).unwrap().action.unwrap().recur.unwrap().handled;
    assert!(handled.iter().any(|entry| entry.date == date("2026-10-15")));
    assert!(!handled.iter().any(|entry| entry.date == date("2026-10-09")));
    assert_eq!(
        library.current_occurrence(id, date("2026-10-03")).unwrap().date,
        date("2026-10-09")
    );
}

#[test]
fn history_includes_nearer_moves_from_beyond_its_preview() {
    let mut library = Library::in_memory();
    let id = library.create_task("Daily", None).unwrap();
    let today = date("2026-10-31");
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.missed = Missed::All;
    library.set_recurrence(id, Some(settings), today).unwrap();
    library.move_occurrence(id, date("2026-11-30"), Some(date("2026-11-01"))).unwrap();
    let history = library.occurrences(id, today, 10, 5);
    assert!(history.iter().any(|occurrence| occurrence.date == date("2026-11-30")));
}

#[test]
fn sparse_rules_keep_dates_beyond_four_thousand_empty_periods() {
    let library = Library::in_memory();
    let settings = RecurrenceSettings::new(
        Rule::parse("FREQ=DAILY;BYMONTH=2;BYMONTHDAY=29;BYDAY=MO").unwrap(),
        date("2044-02-29"),
    );
    assert_eq!(
        library.preview_dates(&settings, settings.start, 2),
        [date("2044-02-29"), date("2072-02-29")]
    );
}

#[test]
fn waiting_counts_use_due_offsets_in_both_directions() {
    let mut library = Library::in_memory();
    let id = library.create_task("Daily", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), date("2026-10-28"));
    settings.missed = Missed::All;
    settings.due = Some(10);
    let today = date("2026-10-31");
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    assert_eq!(library.occurrence_summary(id, today).unwrap().waiting, 0);
    settings.due = Some(-2);
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert_eq!(library.occurrence_summary(id, today).unwrap().waiting, 5);
}

#[test]
fn public_rule_records_validate_their_numeric_fields() {
    let mut library = Library::in_memory();
    let id = library.create_task("Daily", None).unwrap();
    let today = date("2026-10-31");
    for rule in [
        Rule { count: Some(0), ..Rule::default() },
        Rule { interval: 0, ..Rule::default() },
        Rule { month_days: vec![-128], ..Rule::default() },
        Rule { month_days: vec![0], ..Rule::default() },
        Rule { months: vec![0], ..Rule::default() },
        Rule { months: vec![13], ..Rule::default() },
        Rule { positions: vec![-32768], ..Rule::default() },
        Rule { positions: vec![0], ..Rule::default() },
    ] {
        assert!(!rule.is_valid(), "{rule:?}");
        let settings = RecurrenceSettings::new(rule, today);
        assert!(library.preview_dates(&settings, today, 3).is_empty());
        assert!(matches!(
            library.set_recurrence(id, Some(settings), today),
            Err(Error::InvalidRule)
        ));
        assert!(library.note(id).unwrap().action.unwrap().recur.is_none());
    }
}

#[test]
fn old_overrides_do_not_change_a_new_rules_candidates() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let today = date("2026-12-01");
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    library.move_occurrence(id, date("2026-12-30"), Some(date("2027-01-05"))).unwrap();
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", date("2026-12-30"));
    holiday.last = date("2026-12-31");
    library.set_days_off(&holiday).unwrap();
    settings.rule = Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1").unwrap();
    library.set_recurrence(id, Some(settings), today).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().on, date("2026-12-29"));
}

#[test]
fn missed_moves_from_future_dates_remain_in_history() {
    let mut library = Library::in_memory();
    let id = library.create_task("Monthly", None).unwrap();
    let settings =
        RecurrenceSettings::new(Rule::parse("FREQ=MONTHLY").unwrap(), date("2026-10-01"));
    let today = date("2026-10-02");
    library.set_recurrence(id, Some(settings), today).unwrap();
    library.move_occurrence(id, date("2026-11-01"), Some(date("2026-10-01"))).unwrap();
    library.move_occurrence(id, date("2026-12-01"), Some(today)).unwrap();
    let history = library.occurrences(id, today, 100, 5);
    let missed = history.iter().filter(|occurrence| occurrence.status == OccurrenceStatus::Missed);
    assert_eq!(missed.count() as u32, library.occurrence_summary(id, today).unwrap().missed);
}

#[test]
fn unticking_a_returned_step_clears_both_completion_sources() {
    let mut library = Library::in_memory();
    let parent = library.create_task("Routine", None).unwrap();
    let step = library.create_task("Step", Some(parent)).unwrap();
    let today = date("2026-10-31");
    library
        .set_recurrence(parent, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    library.check_action(step, true, today).unwrap();
    library.move_action(step, None, None).unwrap();
    library.check_action(step, true, today).unwrap();
    library.move_action(step, Some(parent), None).unwrap();
    library.check_action(step, false, today).unwrap();
    assert_eq!(library.note_on(step, today).unwrap().action.unwrap().state, ActionState::Open);
}

#[test]
fn deleted_occurrence_notes_allow_replacements() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let today = date("2026-10-31");
    library
        .set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    let memo = library.write_occurrence_note(id, today).unwrap();
    library.trash(memo).unwrap();
    library.delete(memo).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().note, None);
    let replacement = library.write_occurrence_note(id, today).unwrap();
    assert_eq!(library.current_occurrence(id, today).unwrap().note, Some(replacement));
}

#[test]
fn after_previews_keep_distinct_cycles_that_reuse_a_date() {
    let mut library = Library::in_memory();
    let id = library.create_task("Routine", None).unwrap();
    let today = date("2026-10-01");
    let mut settings =
        RecurrenceSettings::new(Rule::parse("FREQ=DAILY;INTERVAL=3;COUNT=4").unwrap(), today);
    settings.missed = Missed::After;
    library.set_recurrence(id, Some(settings), today).unwrap();
    library.handle_occurrence(id, today, Some(OccurrenceState::Done), today).unwrap();
    library.move_occurrence(id, date("2026-10-04"), Some(today)).unwrap();
    let history = library.occurrences(id, today, 10, 5);
    let dates: Vec<Date> = history.iter().map(|occurrence| occurrence.date).collect();
    assert_eq!(dates, [date("2026-10-04"), date("2026-10-04"), date("2026-10-07"), today]);
    let keys: std::collections::HashSet<_> = history.iter().map(|o| o.target.key).collect();
    assert_eq!(keys.len(), history.len());
}

#[test]
fn continuous_skips_keep_explicit_moves_and_archive_boundaries() {
    for policy in [Missed::After, Missed::All, Missed::Latest] {
        let mut library = Library::in_memory();
        let today = date("2026-10-31");
        let mut vacation = DaysOff::new(DaysOffKind::Vacations, "Week off", today);
        vacation.last = today.add_days(6);
        vacation.rule = Rule::parse("FREQ=WEEKLY");
        library.set_days_off(&vacation).unwrap();
        let id = library.create_task("Routine", None).unwrap();
        let mut settings = RecurrenceSettings::new(Rule::default(), today);
        settings.days_off.vacations = DayOffEffect::Skip;
        settings.missed = policy;
        library.set_recurrence(id, Some(settings), today).unwrap();
        assert!(library.current_occurrence(id, today).is_none());
        let moved = today.add_days(400);
        library.move_occurrence(id, moved, Some(today)).unwrap();
        assert_eq!(library.current_occurrence(id, today).unwrap().date, moved);
        library.move_occurrence(id, moved, None).unwrap();
        library.archive_days_off(DaysOffKind::Vacations, &vacation.id, true, today).unwrap();
        assert_eq!(library.current_occurrence(id, today).unwrap().date, today.add_days(1));
    }
}

#[test]
fn holiday_adjusted_moves_survive_equivalent_rules_and_sync() {
    let mut library = Library::in_memory();
    let today = date("2026-12-01");
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", date("2026-12-31"));
    library.set_days_off(&holiday).unwrap();
    let id = library.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(
        Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1").unwrap(),
        today,
    );
    settings.days_off.holidays = DayOffEffect::Skip;
    library.set_recurrence(id, Some(settings.clone()), today).unwrap();
    library.move_occurrence(id, date("2026-12-30"), Some(date("2027-01-05"))).unwrap();
    holiday.first = date("2026-12-30");
    library.set_days_off(&holiday).unwrap();
    settings.rule = Rule::parse("FREQ=MONTHLY;BYDAY=FR,TH,WE,TU,MO;BYSETPOS=-1;COUNT=3").unwrap();
    library.set_recurrence(id, Some(settings), today).unwrap();
    let mut peer = Library::in_memory();
    peer.merge_updates(&library.export_snapshot().unwrap(), false).unwrap();
    assert_eq!(peer.current_occurrence(id, today).unwrap().on, date("2027-01-05"));
}

#[test]
fn rule_edits_ignore_concurrent_moves_from_the_old_rule() {
    let mut laptop = Library::in_memory();
    let today = date("2026-12-01");
    let id = laptop.create_task("Routine", None).unwrap();
    let mut settings = RecurrenceSettings::new(Rule::default(), today);
    settings.days_off.holidays = DayOffEffect::Skip;
    laptop.set_recurrence(id, Some(settings.clone()), today).unwrap();
    let mut holiday = DaysOff::new(DaysOffKind::Holidays, "Year end", date("2026-12-30"));
    holiday.last = date("2026-12-31");
    let mut phone = Library::in_memory();
    phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
    phone.move_occurrence(id, date("2026-12-30"), Some(date("2027-01-05"))).unwrap();
    laptop.set_days_off(&holiday).unwrap();
    settings.rule = Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1").unwrap();
    laptop.set_recurrence(id, Some(settings), today).unwrap();
    let updates = phone.updates_since(&laptop.version()).unwrap().unwrap();
    laptop.merge_updates(&updates, false).unwrap();
    let updates = laptop.updates_since(&phone.version()).unwrap().unwrap();
    phone.merge_updates(&updates, false).unwrap();
    for library in [&laptop, &phone] {
        assert_eq!(library.current_occurrence(id, today).unwrap().on, date("2026-12-29"));
    }
}

#[test]
fn legacy_moves_acquire_their_original_rule_before_an_edit() {
    let mut seed = Library::in_memory();
    let today = date("2026-12-01");
    let id = seed.create_task("Routine", None).unwrap();
    let settings = RecurrenceSettings::new(Rule::default(), today);
    seed.set_recurrence(id, Some(settings.clone()), today).unwrap();
    seed.move_occurrence(id, date("2026-12-30"), Some(date("2027-01-05"))).unwrap();
    let doc = LoroDoc::new();
    doc.import(&seed.export_snapshot().unwrap()).unwrap();
    let Some(ValueOrContainer::Container(Container::Map(note))) =
        doc.get_map("notes").get(&id.to_string())
    else {
        panic!()
    };
    let Some(ValueOrContainer::Container(Container::Map(action))) = note.get("action") else {
        panic!()
    };
    action.delete("recur").unwrap();
    let recur = action.insert_container("recur", loro::LoroMap::new()).unwrap();
    recur.insert("rule", "FREQ=DAILY").unwrap();
    recur.insert("start", "2026-12-01").unwrap();
    let moved = recur.insert_container("moved", loro::LoroMap::new()).unwrap();
    moved.insert("2026-12-30", "2027-01-05").unwrap();
    doc.get_map("recurrence_moved").clear().unwrap();
    doc.commit();
    let mut library = Library::in_memory();
    library.merge_updates(&doc.export(ExportMode::Snapshot).unwrap(), false).unwrap();
    let moved = library.note(id).unwrap().action.unwrap().recur.unwrap().moved;
    assert_eq!(moved[0].rule.as_deref(), Some("FREQ=DAILY"));
    assert_eq!(moved[0].start, Some(today));
    let mut changed = settings;
    changed.rule = Rule::parse("FREQ=MONTHLY;BYMONTHDAY=1").unwrap();
    library.set_recurrence(id, Some(changed), today).unwrap();
    assert!(library.note(id).unwrap().action.unwrap().recur.unwrap().moved.is_empty());
}

#[test]
fn occurrence_notes_follow_moves_between_libraries_and_known_deletions() {
    let mut spaces = Spaces::in_memory(&["a", "b"]);
    let today = date("2026-10-31");
    let id = spaces.library_mut("a").unwrap().create_task("Routine", None).unwrap();
    spaces
        .set_recurrence(id, Some(RecurrenceSettings::new(Rule::default(), today)), today)
        .unwrap();
    let memo = spaces.write_occurrence_note(id, today).unwrap();
    spaces.move_to_space(memo, "b").unwrap();
    assert_eq!(spaces.current_occurrence(id, today).unwrap().note, Some(memo));
    assert_eq!(spaces.occurrence_summary(id, today).unwrap().current.unwrap().note, Some(memo));
    assert_eq!(spaces.occurrences(id, today, 10, 5)[0].note, Some(memo));
    spaces.trash(memo).unwrap();
    spaces.delete(memo).unwrap();
    assert_eq!(spaces.current_occurrence(id, today).unwrap().note, None);
    assert_eq!(spaces.occurrence_summary(id, today).unwrap().current.unwrap().note, None);
    assert_eq!(spaces.occurrences(id, today, 10, 5)[0].note, None);
}

#[test]
fn weekly_rules_keep_partial_weeks_at_the_calendar_limits() {
    let library = Library::in_memory();
    let first = date("0001-01-01");
    let settings =
        RecurrenceSettings::new(Rule::parse("FREQ=WEEKLY;BYDAY=MO;WKST=SU").unwrap(), first);
    assert_eq!(library.preview_dates(&settings, first, 1), [first]);
    let last = date("9998-12-31");
    let start = last.add_days(-6);
    let settings = RecurrenceSettings::new(
        Rule::parse("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR,SA,SU;WKST=SU").unwrap(),
        start,
    );
    assert_eq!(
        library.preview_dates(&settings, start, 20),
        (0..=6).map(|days| start.add_days(days)).collect::<Vec<_>>()
    );
}
