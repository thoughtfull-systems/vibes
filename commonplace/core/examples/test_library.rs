// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Builds two libraries of sample memos, actions, and journals, Personal and Work, for trying the
//! apps by hand:
//!
//! ```sh
//! cargo run --example test_library -- DIRECTORY
//! ```
//!
//! It covers every state an action can be in and every kind of formatting the editor renders. Dates
//! count from the day it runs, so overdue, due-soon, and upcoming actions stay that way. When a
//! feature is ready to ship, add samples of it here.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use commonplace_core::{
    ActionChange, ActionList, ActionState, Answer, ChoiceOption, Date, Days, Error, Library,
    NoteId, Question, QuestionKind, Schedule, Spaces, Times, wikilink,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: test_library DIRECTORY");
        std::process::exit(2);
    };
    if dir.exists() {
        eprintln!("test_library: {} already exists", dir.display());
        std::process::exit(1);
    }
    let mut spaces = Spaces::open(&dir)?;
    let personal = spaces.keys().remove(0);
    let work = spaces.create()?;
    let today = today();
    let trip = memos(spaces.library_mut(&personal)?)?;
    actions(spaces.library_mut(&personal)?, today)?;
    work_notes(spaces.library_mut(&work)?, trip, today)?;
    health_journal(spaces.library_mut(&personal)?, today)?;
    work_log(spaces.library_mut(&work)?, today)?;
    // The Linux app keeps the names it gives libraries in its device settings.
    fs::write(dir.join("device.ini"), format!("[names]\n{personal}=Personal\n{work}=Work\n"))?;
    Ok(())
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
    library.create_task("Pick the tiles", Some(kitchen))?;
    library.create_task("Find a plumber", Some(kitchen))?;

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
    for question in [&sleep, &mood, &exercise, &caffeine, &notes] {
        library.add_question(journal, question)?;
    }
    let mornings = Schedule::new(Days::EVERY_DAY, Times::Fixed(vec![8 * 60]));
    library.set_schedule(journal, &mornings)?;
    let hours = [7.5, 6.0, 8.0, 5.5, 7.0, 8.5, 6.5];
    for (day, &hour) in hours.iter().enumerate() {
        let mut answers = BTreeMap::from([
            (sleep.id.clone(), Answer::Number(hour)),
            (mood.id.clone(), Answer::Number(((day % 5) + 1) as f64)),
            (caffeine.id.clone(), Answer::Bool(day % 3 == 0)),
        ]);
        if day % 2 == 0 {
            let picked = vec![options[day % 3].id.clone()];
            answers.insert(exercise.id.clone(), Answer::Choices(picked));
        }
        let body = format!("## Anything else?\n\nDay {} of the week.\n", day + 1);
        library.create_entry(
            journal,
            &body,
            hours_ago(24 * (6 - day as i64) + 2),
            None,
            &answers,
            today,
        )?;
    }
    library.create_entry(
        journal,
        "",
        hours_ago(1),
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
        (2, "Reviewed the quarterly numbers."),
        (5, "Paired on the release checklist."),
        (26, "Fixed the flaky export test."),
    ] {
        let body = format!("## What have you been working on?\n\n{text}\n\n## Blockers\n\nNone.\n");
        library.create_entry(journal, &body, hours_ago(hours), None, &BTreeMap::new(), today)?;
    }
    let memo = library.create_memo("Shipped the hotfix\n\nTurned into a work log entry.\n")?;
    library.convert_memo(memo, journal)?;
    Ok(())
}

/// The timestamp `hours` hours ago.
fn hours_ago(hours: i64) -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis());
    now as i64 - hours * 60 * 60 * 1000
}

/// Today's date in UTC, which is close enough for samples.
fn today() -> Date {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs());
    Date::new(1970, 1, 1).expect("the epoch is a date").add_days((seconds / 86_400) as i32)
}
