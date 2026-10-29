// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::BTreeMap;

use crate::{
    ActionChange, ActionList, ActionState, Answer, ChoiceOption, Date, Days, Error, Library,
    NoteId, Question, QuestionKind, Schedule, Spaces, Times, wikilink,
};

/// Adds the same Personal and Work samples on every platform, preserving existing notes.
pub fn populate(spaces: &mut Spaces, today: Date) -> Result<BTreeMap<String, String>, Error> {
    generate(spaces, today, Library::in_memory)
}

/// Builds samples as of noon UTC on `date`, leaving existing notes and the app's clock alone.
pub fn populate_on(spaces: &mut Spaces, date: Date) -> Result<BTreeMap<String, String>, Error> {
    let timestamp = date.days() * 86_400_000 + 12 * 60 * 60 * 1000;
    generate(spaces, date, || Library::in_memory_at(timestamp))
}

/// Builds the samples in libraries from `library`, then merges them into `spaces`.
fn generate(
    spaces: &mut Spaces,
    today: Date,
    library: impl Fn() -> Library,
) -> Result<BTreeMap<String, String>, Error> {
    let mut personal = library();
    let mut work = library();
    let trip = memos(&mut personal)?;
    actions(&mut personal, today)?;
    work_notes(&mut work, trip, today)?;
    health_journal(&mut personal, today)?;
    work_log(&mut work, today)?;
    import_samples(spaces, &personal.export_snapshot()?, &work.export_snapshot()?)
}

/// Imports the snapshots into `spaces`, removing any library it created if one fails.
fn import_samples(
    spaces: &mut Spaces,
    personal: &[u8],
    work: &[u8],
) -> Result<BTreeMap<String, String>, Error> {
    let keys = spaces.keys();
    let reused = (keys.len() == 1
        && spaces
            .library(&keys[0])
            .is_some_and(|library| !library.is_read_only() && library.notes().is_empty()))
    .then(|| keys[0].clone());
    let mut created = Vec::new();
    let result = add_samples(spaces, reused, &mut created, personal, work);
    if result.is_err() {
        for key in &created {
            let _ = spaces.remove(key);
        }
    }
    result
}

/// Imports the samples into `reused`, or a new library, and a new library for work, adding each
/// library it creates to `created`. The reused library takes its samples last, so a failure
/// leaves it untouched.
fn add_samples(
    spaces: &mut Spaces,
    reused: Option<String>,
    created: &mut Vec<String>,
    personal: &[u8],
    work: &[u8],
) -> Result<BTreeMap<String, String>, Error> {
    let personal_key = match reused {
        Some(key) => key,
        None => {
            let key = spaces.create()?;
            created.push(key.clone());
            key
        }
    };
    let work_key = spaces.create()?;
    created.push(work_key.clone());
    spaces.library_mut(&work_key)?.import_snapshot(work)?;
    spaces.library_mut(&personal_key)?.import_snapshot(personal)?;
    Ok(BTreeMap::from([(personal_key, "Personal".into()), (work_key, "Work".into())]))
}

fn work_notes(library: &mut Library, trip: NoteId, today: Date) -> Result<(), Error> {
    library.create_memo(&format!(
        "Time off\n\nAway for {}, so hand the release to the team before then.\n",
        wikilink(trip, Some("the trip, in the Personal library"))
    ))?;
    library.create_inbox_task("Answer the survey from HR")?;
    let report = library.create_task("Write the quarterly report", None)?;
    library.change_action(report, ActionChange::Due(Some(today.add_days(4))))?;
    library.create_task("Gather the numbers", Some(report))?;
    library.create_task("Draft the summary", Some(report))?;
    library.create_task("Review pull requests", None)?;
    Ok(())
}

fn memos(library: &mut Library) -> Result<NoteId, Error> {
    let trip = library.create_memo(
        "Trip to Portugal\n\n# Places\n\nLisbon, then Porto by train.\n\n## Lisbon\n\n\
          - Alfama\n- Belém\n  - The tower\n  - The pastry shop\n\n## Porto\n\n\
          Ribeira and the bridge.\n\n# Food\n\nPastéis de nata, grilled sardines, vinho verde.\n",
    )?;
    library.set_starred(trip, true)?;
    library.create_memo(&format!(
        "Packing list for {}\n\n- [x] Passport\n- [x] Adapter\n- [ ] Walking shoes\n- [ ] Rain \
          jacket\n",
        wikilink(trip, Some("the trip"))
    ))?;
    library.create_memo(
        "Formatting sampler\n\n\
          # Heading one\n\n## Heading two\n\n### Heading three\n\n#### Heading four\n\n\
          Text with *emphasis*, _more emphasis_, **strong**, ***both***, ~~struck~~, `inline \
          code`, and ==a highlight==, plus ==highlighted *and emphasized*== text.\n\n\
          Not highlights: a == b, and ===three===.\n\n\
          > A block quote at the left margin,\n> running onto a second line.\n\n\
          ```\nfn main() {\n    println!(\"a code block\");\n}\n```\n\n\
          1. A numbered item\n2. Another\n   1. Nested\n\n\
          - A bullet\n  - Nested under it\n    - And deeper\n\n\
          - [ ] An open checklist item\n- [x] A checked one\n\n\
          A web link to [the CommonMark site](https://commonmark.org), and a bare one: \
          <https://example.com>.\n",
    )?;
    library.create_memo(
        "A **bold** and *italic* title with ~~strikethrough~~, `code`, and ==highlight==\n\n\
          Lists render the title's formatting, and the window title strips it.\n",
    )?;
    let meeting = library.create_memo(
        "Meeting with the landlord\n\nAgreed to fix the gutters before winter. Follow up in two \
          weeks if nothing happens.\n",
    )?;
    library.create_memo(&format!(
        "Notes that link back\n\nSee {} and {} for the details.\n",
        wikilink(trip, None),
        wikilink(meeting, Some("the landlord meeting"))
    ))?;
    library.create_memo(
        "A long memo to scroll\n\n# Outline\n\n## One\n\nThe outline sidebar lists each \
          heading.\n\n## Two\n\nClick one to jump to it.\n\n## Three\n\nThe end.\n",
    )?;
    let deleted = library.create_memo("Old packing ideas\n\nDeleted for good.\n")?;
    library.trash(deleted)?;
    library.delete(deleted)?;
    let bracketed = library.create_memo("Budget [draft] | v2\n\nA title with link syntax.\n")?;
    library.create_memo(&format!(
        "Links to {}\n\nThe title shows the linked note's title. {} still names the note it led \
          to, deleted for good. {} shows its brackets and bar.\n",
        wikilink(meeting, None),
        wikilink(deleted, None),
        wikilink(bracketed, None)
    ))?;
    library.create_memo("")?;
    let trashed = library.create_memo("A trashed memo\n\nRestore it or delete it for good.\n")?;
    library.trash(trashed)?;
    library.create_memo(&format!(
        "A link into the trash\n\nOpen {}, then delete it for good to return here.\n",
        wikilink(trashed, None)
    ))?;
    let archived = library
        .create_memo("An archived memo\n\nAll Notes lists it, and the Memos tab doesn't.\n")?;
    library.set_archived(archived, true)?;
    Ok(trip)
}

fn actions(library: &mut Library, today: Date) -> Result<(), Error> {
    let list = |library: &mut Library, id: NoteId, list: ActionList| {
        library.change_action(id, ActionChange::List(list))
    };
    let due = |library: &mut Library, id: NoteId, days: i32| {
        library.change_action(id, ActionChange::Due(Some(today.add_days(days))))
    };
    let starts = |library: &mut Library, id: NoteId, days: i32| {
        library.change_action(id, ActionChange::Defer(Some(today.add_days(days))))
    };

    // The inbox.
    library.create_inbox_task("Call the dentist")?;
    library.create_inbox_task("Look into a new bike\n\nThe old one's chain keeps slipping.\n")?;
    let captured = library.create_inbox_task("Plan the garden")?;
    library.change_action(captured, ActionChange::Project(true))?;

    // Active tasks in each part of Execute.
    let overdue = library.create_task("Renew the passport", None)?;
    due(library, overdue, -2)?;
    let soon = library.create_task("Pay the electric bill", None)?;
    due(library, soon, 3)?;
    library.create_task("Water the plants", None)?;
    let later = library.create_task("File the taxes", None)?;
    due(library, later, 40)?;
    let starting = library.create_task("Book the train tickets", None)?;
    starts(library, starting, 3)?;
    let scheduled = library.create_task("Change the smoke alarm batteries", None)?;
    starts(library, scheduled, 21)?;
    library.create_task(
        "Read **Getting Things Done** again\n\nWith *notes*, and a [[link]] that leads nowhere.\n",
        None,
    )?;

    // A project with steps, a nested project, a done step, and a date the steps inherit.
    let house = library.create_task("Fix up the house", None)?;
    due(library, house, 30)?;
    let gutters = library.create_task("Clean the gutters", Some(house))?;
    library.change_action(gutters, ActionChange::State(ActionState::Done))?;
    library.create_task("Paint the fence", Some(house))?;
    let kitchen = library.create_task("Redo the kitchen", Some(house))?;
    let tiles = library.create_task("Pick the tiles", Some(kitchen))?;
    library.create_task("Find a plumber", Some(kitchen))?;

    // Starred actions: a project and a step deep inside it. A task put off to someday has one too.
    library.set_starred(house, true)?;
    library.set_starred(tiles, true)?;

    // Stalled projects: one with no steps, and one whose only step waits in the inbox.
    let empty = library.create_task("Learn Portuguese", None)?;
    library.change_action(empty, ActionChange::Project(true))?;
    let blocked = library.create_task("Organize the photos", None)?;
    let sort = library.create_task("Sort the old albums", Some(blocked))?;
    list(library, sort, ActionList::Inbox)?;

    // Waiting, someday, and maybe.
    let waiting = library.create_task("Get the deposit back", None)?;
    list(library, waiting, ActionList::Waiting)?;
    library.change_action(waiting, ActionChange::WaitingFor(Some("the landlord".to_owned())))?;
    let someday = library.create_task("Learn to sail", None)?;
    list(library, someday, ActionList::Someday)?;
    library.set_starred(someday, true)?;
    let maybe = library.create_task("Get a dog", None)?;
    list(library, maybe, ActionList::Maybe)?;
    let shelved = library.create_task("Build a cabin", None)?;
    list(library, shelved, ActionList::Someday)?;
    library.create_task("Buy land", Some(shelved))?;

    // The planning queue.
    let plan_task = library.create_task("Choose a new phone plan", None)?;
    library.change_action(plan_task, ActionChange::Planning(true))?;
    let plan_project = library.create_task("Throw a birthday party", None)?;
    library.create_task("Pick a date", Some(plan_project))?;
    library.change_action(plan_project, ActionChange::Planning(true))?;

    // Done, archived, and trashed actions.
    let done = library.create_task("Return the library books", None)?;
    library.change_action(done, ActionChange::State(ActionState::Done))?;
    let archived = library.create_task("Start a podcast", None)?;
    library.create_task("Buy a microphone", Some(archived))?;
    library.set_archived(archived, true)?;
    let trashed = library.create_task("A trashed action", None)?;
    library.create_task("Its step goes with it", Some(trashed))?;
    library.trash(trashed)?;
    Ok(())
}

/// A journal of structured answers, with a week of entries to chart.
fn health_journal(library: &mut Library, today: Date) -> Result<(), Error> {
    let journal = library.create_journal("Health")?;
    library.set_journal_color(journal, "green")?;
    let unit = Some("h".to_owned());
    let sleep =
        Question::new("Sleep", QuestionKind::Number { unit, min: Some(0.0), max: Some(24.0) });
    let (low, high) = (Some("Awful".to_owned()), Some("Great".to_owned()));
    let mood = Question::new("Mood", QuestionKind::Scale { min: 1, max: 5, low, high });
    let options = ["Run", "Swim", "Lift"].map(ChoiceOption::new).to_vec();
    let exercise = Question::new(
        "Exercise",
        QuestionKind::Choice { multiple: true, options: options.clone() },
    );
    let caffeine = Question::new("Coffee after noon?", QuestionKind::YesNo);
    let notes = Question::new("Anything else?", QuestionKind::Text);
    let bedtime = Question::new("Bedtime", QuestionKind::TimeOfDay);
    let highlight = Question::new("Daily highlight", QuestionKind::ShortText);
    let training = Question::new("Training time", QuestionKind::Duration);
    // In the order capture asks them, so the free-form question comes last.
    for question in [&sleep, &bedtime, &mood, &exercise, &training, &caffeine, &highlight, &notes] {
        library.add_question(journal, question)?;
    }
    let mornings = Schedule::new(Days::EVERY_DAY, Times::Fixed(vec![8 * 60]));
    library.set_schedule(journal, &mornings)?;
    let hours = [7.5, 6.0, 8.0, 5.5, 7.0, 8.5, 6.5];
    for (day, &hour) in hours.iter().enumerate() {
        let mut answers = BTreeMap::from([
            (bedtime.id.clone(), Answer::Text(format!("22:{:02}", day * 5))),
            (caffeine.id.clone(), Answer::Bool(day % 3 == 0)),
            (highlight.id.clone(), Answer::Text("Spent time outside".into())),
            (mood.id.clone(), Answer::Number(((day % 5) + 1) as f64)),
            (sleep.id.clone(), Answer::Number(hour)),
            (training.id.clone(), Answer::Number((day + 1) as f64 * 10.0)),
        ]);
        if day % 2 == 0 {
            let picked = vec![options[day % 3].id.clone()];
            answers.insert(exercise.id.clone(), Answer::Choices(picked));
        }
        let body = format!("## Anything else?\n\nDay {} of the week.\n", day + 1);
        library.create_entry(
            journal,
            &body,
            hours_ago(library, 24 * (6 - day as i64) + 2),
            None,
            &answers,
            today,
        )?;
    }
    library.create_entry(
        journal,
        &format!("{today}\n\n"),
        hours_ago(library, 1),
        None,
        &BTreeMap::from([(sleep.id.clone(), Answer::Number(7.0))]),
        today,
    )?;
    Ok(())
}

/// A work log that prompts on random workday times, for standup reports.
fn work_log(library: &mut Library, today: Date) -> Result<(), Error> {
    let journal = library.create_journal("Work log\n\nWhat I did, for standups.")?;
    library.set_journal_color(journal, "orange")?;
    for prompt in ["What have you been working on?", "Blockers"] {
        library.add_question(journal, &Question::new(prompt, QuestionKind::Text))?;
    }
    let random = Times::Random { start: 9 * 60, end: 17 * 60, mean: 45, min_gap: 30 };
    library.set_schedule(journal, &Schedule::new(Days::WORKDAYS, random))?;
    for (hours, text) in [
        (2, "Reviewed the **quarterly numbers** and ==next steps==."),
        (5, "Paired on the *release checklist* and ran `cargo test`."),
        (26, "Fixed the flaky export test."),
    ] {
        let body = format!("## What have you been working on?\n\n{text}\n\n## Blockers\n\nNone.\n");
        library.create_entry(
            journal,
            &body,
            hours_ago(library, hours),
            None,
            &BTreeMap::new(),
            today,
        )?;
    }
    let memo = library.create_memo("Shipped the hotfix\n\nTurned into a work log entry.\n")?;
    library.convert_memo(memo, journal)?;
    Ok(())
}

fn hours_ago(library: &Library, hours: i64) -> i64 {
    library.now() - hours * 60 * 60 * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_import_removes_the_libraries_it_created() {
        let dir = tempfile::tempdir().unwrap();
        let mut spaces = Spaces::open(dir.path()).unwrap();
        let work = Library::in_memory().export_snapshot().unwrap();
        for _ in 0..2 {
            assert!(import_samples(&mut spaces, b"not a snapshot", &work).is_err());
            assert_eq!(spaces.keys().len(), 1);
        }
        let existing = spaces.keys().remove(0);
        spaces.library_mut(&existing).unwrap().create_memo("Keep me").unwrap();
        assert!(import_samples(&mut spaces, b"not a snapshot", &work).is_err());
        assert_eq!(spaces.keys(), [existing]);
    }

    #[test]
    fn a_chosen_day_dates_every_sample_without_retiming_existing_notes_or_later_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut spaces = Spaces::open(dir.path()).unwrap();
        let existing_space = spaces.keys()[0].clone();
        let existing = spaces.library_mut(&existing_space).unwrap().create_memo("Keep me").unwrap();
        let original = spaces.note(existing).unwrap();
        let date = Date::new(2024, 2, 29).unwrap();
        let timestamp = 1_709_208_000_000;
        let names = populate_on(&mut spaces, date).unwrap();
        assert_eq!(spaces.note(existing).unwrap(), original);
        assert_eq!(names.len(), 2);

        for key in names.keys() {
            let library = spaces.library(key).unwrap();
            let notes = library.notes();
            assert!(!notes.is_empty());
            for note in &notes {
                assert_eq!(note.created, timestamp, "creation time of {:?}", note.label());
                assert_eq!(note.modified, timestamp, "modification time of {:?}", note.label());
                for time in [
                    note.action.as_ref().and_then(|action| action.closed),
                    note.action.as_ref().and_then(|action| action.planning),
                    note.starred,
                    note.archived,
                    note.trashed,
                ]
                .into_iter()
                .flatten()
                {
                    assert_eq!(time, timestamp, "state timestamp of {:?}", note.label());
                }
            }
        }
        let personal = names.iter().find(|(_, name)| *name == "Personal").unwrap().0;
        let notes = spaces.library(personal).unwrap().notes();
        let taxes =
            notes.iter().find(|note| note.label().as_deref() == Some("File the taxes")).unwrap();
        assert_eq!(taxes.action.as_ref().unwrap().due, Some(date.add_days(40)));
        let health = notes.iter().find(|note| note.label().as_deref() == Some("Health")).unwrap();
        let entries: Vec<_> = notes
            .iter()
            .filter_map(|note| note.entry.as_ref())
            .filter(|entry| entry.journal == health.id)
            .map(|entry| entry.at)
            .collect();
        assert_eq!(entries.len(), 8);
        assert_eq!(entries.iter().max(), Some(&(timestamp - 60 * 60 * 1000)));
        assert_eq!(entries.iter().min(), Some(&(timestamp - 146 * 60 * 60 * 1000)));

        let edited = spaces.library_mut(personal).unwrap();
        edited.set_body(health.id, "Edited with the real clock").unwrap();
        assert!(edited.note(health.id).unwrap().modified >= original.created);
        drop(spaces);
        let reopened = Spaces::open(dir.path()).unwrap();
        assert_eq!(reopened.note(existing).unwrap(), original);
        assert_eq!(reopened.note(taxes.id).unwrap().created, timestamp);
    }
}
