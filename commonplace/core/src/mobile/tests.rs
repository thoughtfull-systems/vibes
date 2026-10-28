// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use super::*;
use crate::{ActionList, ActionState, ChoiceOption, Library, QuestionKind, Times};

#[test]
fn mobile_and_linux_share_the_same_library() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let memo = mobile.create_memo(space.clone(), "Phone 🦀 note".into()).unwrap();
    let task = mobile.create_action(space.clone(), "Buy milk".into()).unwrap();
    mobile.change_action(task, ActionChange::List(ActionList::Active)).unwrap();
    mobile.change_action(task, ActionChange::Project(true)).unwrap();
    let step = mobile.create_step(task, "At the shop".into()).unwrap();
    mobile.change_action(step, ActionChange::State(ActionState::Done)).unwrap();
    mobile.set_archived(memo, true).unwrap();
    drop(mobile);
    let spaces = Spaces::open(dir.path()).unwrap();
    assert_eq!(spaces.note(memo).unwrap().body, "Phone 🦀 note");
    assert!(spaces.note(memo).unwrap().archived.is_some());
    assert_eq!(spaces.library(&space).unwrap().action_parent(step), Some(task));
    assert_eq!(spaces.note(step).unwrap().action.unwrap().state, ActionState::Done);
}

#[test]
fn an_open_editor_keeps_another_devices_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let memo = mobile.create_memo(space.clone(), "Groceries".into()).unwrap();
    let shown = mobile.editor_body(memo).unwrap();
    {
        let mut state = mobile.state().unwrap();
        let library = state.spaces.library_mut(&space).unwrap();
        let mut laptop = Library::in_memory();
        laptop.merge_updates(&library.export_snapshot().unwrap(), false).unwrap();
        laptop.set_body(memo, "Saturday\nGroceries").unwrap();
        let updates = laptop.updates_since(&library.version()).unwrap().unwrap();
        library.merge_updates(&updates, false).unwrap();
    }
    assert!(mobile.set_body(memo, format!("{shown} for the week")).unwrap());
    // The editor loads the merged text, then saves in place again.
    let shown = mobile.editor_body(memo).unwrap();
    assert_eq!(shown, "Saturday\nGroceries for the week");
    assert!(!mobile.set_body(memo, shown + "\n- milk").unwrap());
    assert_eq!(
        mobile.note(memo).unwrap().unwrap().body,
        "Saturday\nGroceries for the week\n- milk"
    );
}

#[test]
fn unicode_editor_offsets_use_kotlin_utf16_units() {
    let body = "🦀\n- café";
    let edit = list_edit(body.into(), body.encode_utf16().count() as u32, ListKey::Enter).unwrap();
    assert_eq!(edit.start, 9);
    assert_eq!(edit.text, "\n- ");
    assert_eq!(edit.cursor, 12);
    let heading = outline("🦀\n\n# Héllo".into()).remove(0);
    assert_eq!(heading.start, 4);
    assert!(list_edit("🦀".into(), 1, ListKey::Enter).is_none());
    let mixed = "a🦀 *é* [[x]] **b**";
    let units = Utf16::new(mixed);
    for (byte, _) in mixed.char_indices().chain([(mixed.len(), ' ')]) {
        assert_eq!(units.at(byte), utf16(mixed, byte));
    }
    let span = spans(mixed.into()).remove(0);
    assert_eq!((span.start, span.end), (4, 7));
    let markup = editor_markup("🦀 [café](https://example.com)".into());
    let link = &markup.links[0];
    assert_eq!((link.start, link.end), (3, 30));
    assert_eq!(link.uri.as_deref(), Some("https://example.com"));
    assert!(markup.spans.iter().any(|span| (span.start, span.end) == (3, 30)));
}

#[test]
fn journal_capture_keeps_structured_answers_and_prompt_state() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let journal = mobile.create_journal(space, "Work log".into()).unwrap();
    let question = Question::new(
        "Doing",
        QuestionKind::Choice { multiple: false, options: vec![ChoiceOption::new("Writing")] },
    );
    mobile.set_question(journal, question.clone(), true).unwrap();
    let schedule = Schedule::new(Days::EVERY_DAY, Times::Fixed(vec![600]));
    mobile.set_schedule(journal, schedule).unwrap();
    let today = "2026-10-25".parse().unwrap();
    let prompt = mobile
        .due_prompts(vec![journal], today, 600, 36_000_000, (0..1440).map(|m| m * 60_000).collect())
        .unwrap()
        .remove(0);
    let answers = HashMap::from([(question.id, Answer::Choices(vec!["choice".into()]))]);
    let entry = mobile
        .create_entry(
            journal,
            "Writing".into(),
            36_000_000,
            Some(CapturedPrompt { key: prompt.key, prompted: 36_000_000 }),
            answers.clone(),
            today,
        )
        .unwrap();
    assert_eq!(mobile.note(entry).unwrap().unwrap().entry.unwrap().answers, answers);
    assert!(
        mobile
            .due_prompts(
                vec![journal],
                today,
                600,
                36_000_000,
                (0..1440).map(|m| m * 60_000).collect()
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn inherited_trash_and_links_match_the_desktop() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let parent = mobile.create_action(space.clone(), "Project".into()).unwrap();
    let step = mobile.create_step(parent, "Step".into()).unwrap();
    let memo = mobile.create_memo(space, crate::wikilink(step, None)).unwrap();
    let editor = mobile.editor_body(memo).unwrap();
    assert!(editor.contains("Step"));
    mobile.set_body(memo, editor).unwrap();
    assert_eq!(mobile.note(memo).unwrap().unwrap().body, crate::wikilink(step, None));
    mobile.trash(parent).unwrap();
    assert!(mobile.note(step).unwrap().unwrap().in_trash);
    mobile.restore(parent).unwrap();
    assert!(!mobile.note(step).unwrap().unwrap().in_trash);
    assert_eq!(mobile.backlinks(step).unwrap().len(), 1);
}

#[test]
fn bottom_panel_keeps_step_order_depth_and_closed_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let parent = mobile.create_action(space, "Project".into()).unwrap();
    let first = mobile.create_step(parent, "First".into()).unwrap();
    let nested = mobile.create_step(first, "Nested".into()).unwrap();
    let last = mobile.create_step(parent, "Last".into()).unwrap();
    mobile.change_action(first, ActionChange::State(ActionState::Done)).unwrap();
    let today = "2026-10-26".parse().unwrap();
    let steps = mobile.action_steps(parent, today).unwrap();
    assert_eq!(
        steps.iter().map(|entry| (entry.note.id, entry.depth)).collect::<Vec<_>>(),
        [(first, 0), (nested, 1), (last, 0)]
    );
    mobile.reorder_action(last, true).unwrap();
    mobile.trash(first).unwrap();
    let steps = mobile.action_steps(parent, today).unwrap();
    assert_eq!(steps.into_iter().map(|entry| entry.note.id).collect::<Vec<_>>(), [last]);
}

#[test]
fn sample_libraries_match_the_desktop_and_preserve_existing_notes() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let key = mobile.spaces().unwrap().remove(0).key;
    let memo = mobile.create_memo(key.clone(), "My own note".into()).unwrap();
    let names = mobile.load_test_library("2026-10-26".parse().unwrap()).unwrap();
    assert_eq!(mobile.spaces().unwrap().len(), 3);
    assert_eq!(mobile.note(memo).unwrap().unwrap().body, "My own note");
    assert!(!names.contains_key(&key));
    assert!(names.values().any(|name| name == "Personal"));
    assert!(names.values().any(|name| name == "Work"));
    let notes = mobile.notes("".into()).unwrap();
    assert!(notes.iter().any(|note| note.title == "Formatting sampler"));
    assert!(notes.iter().any(|note| note.in_trash));
    assert!(notes.iter().any(|note| note.archived.is_some()));
    assert!(notes.iter().any(|note| note.journal.is_some()));
    assert!(notes.iter().any(|note| note.entry.is_some()));
    let kinds: std::collections::BTreeSet<_> = notes
        .iter()
        .flat_map(|note| note.journal.iter())
        .flat_map(|journal| &journal.questions)
        .map(|question| question.kind.name())
        .collect();
    assert_eq!(kinds.len(), 8);
    assert!(
        mobile
            .actions(ActionView::Waiting, "2026-10-26".parse().unwrap())
            .unwrap()
            .iter()
            .any(|action| action.note.action.is_some())
    );
    drop(mobile);
    let reopened = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    assert_eq!(reopened.notes("".into()).unwrap().len(), notes.len());
}

#[test]
fn sample_libraries_reuse_a_fresh_empty_library() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    mobile.load_test_library("2026-10-26".parse().unwrap()).unwrap();
    assert_eq!(mobile.spaces().unwrap().len(), 2);
}

#[test]
fn newly_inserted_links_keep_automatic_titles_through_a_rename() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let source = mobile.create_memo(space.clone(), "".into()).unwrap();
    let target = mobile.create_memo(space, "A title | with ] syntax".into()).unwrap();
    mobile.editor_body(source).unwrap();
    let link = mobile.link_text(source, target).unwrap();
    assert!(link.contains("A title"));
    mobile.set_body(source, link.clone()).unwrap();
    assert_eq!(mobile.note(source).unwrap().unwrap().body, crate::wikilink(target, None));
    mobile.set_body(target, "Renamed".into()).unwrap();
    mobile.set_body(source, format!("{link}\nMore writing")).unwrap();
    assert_eq!(
        mobile.note(source).unwrap().unwrap().body,
        format!("{}\nMore writing", crate::wikilink(target, None))
    );
}

#[test]
fn archived_actions_keep_their_inherited_dates() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let parent = mobile.create_action(space, "Parent deadline".into()).unwrap();
    let due: Date = "2026-11-01".parse().unwrap();
    mobile.change_action(parent, ActionChange::Due(Some(due))).unwrap();
    let step = mobile.create_step(parent, "Archived step".into()).unwrap();
    let today: Date = "2026-10-27".parse().unwrap();
    let before = mobile.action_dates(step, today).unwrap();
    mobile.set_archived(step, true).unwrap();
    let after = mobile.action_dates(step, today).unwrap();
    assert_eq!(before.due, after.due);
    assert_eq!(before.start, after.start);
    assert_eq!(after.due.unwrap().from, parent);
    assert_eq!(after.due.unwrap().date, due);
    assert_eq!(mobile.all_action_dates(today).unwrap()[&step].due, after.due);
    assert_eq!(mobile.action_paths().unwrap()[&step], [parent]);
}

#[test]
fn phone_dragging_reorders_and_nests_whole_subtrees() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().to_string_lossy().into()).unwrap();
    let space = mobile.spaces().unwrap().remove(0).key;
    let parent = mobile.create_action(space, "Project".into()).unwrap();
    let a = mobile.create_step(parent, "First".into()).unwrap();
    let b = mobile.create_step(parent, "Second".into()).unwrap();
    let child = mobile.create_step(a, "Child".into()).unwrap();
    let today = "2026-10-27".parse().unwrap();
    mobile.move_action_beside(a, b, true).unwrap();
    let ids = || {
        mobile.action_steps(parent, today).unwrap().iter().map(|s| s.note.id).collect::<Vec<_>>()
    };
    assert_eq!(ids(), vec![b, a, child]);
    mobile.move_action(a, Some(b), None).unwrap();
    assert_eq!(mobile.note(a).unwrap().unwrap().parent, Some(b));
    assert_eq!(mobile.note(child).unwrap().unwrap().parent, Some(a));
    assert!(mobile.move_action(b, Some(child), None).is_err());
}

struct Quiet;

impl SyncListener for Quiet {
    fn sync_changed(&self) {}

    fn libraries_changed(&self) {}
}

#[test]
fn sync_forgets_the_shares_of_removed_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let mobile = MobileNotebook::open(dir.path().join("library").to_string_lossy().into()).unwrap();
    assert!(mobile.sync_config().is_err());
    let sync = dir.path().join("sync").to_string_lossy().into_owned();
    mobile.clone().start_sync(sync.clone(), Box::new(Quiet)).unwrap();
    let address = mobile.sync_address().unwrap();
    let other = iroh::SecretKey::generate().public().to_string();
    let device = mobile.add_device(other, "Laptop".into()).unwrap();
    mobile.set_sync_automatic(false).unwrap();
    let space = mobile.create_space().unwrap();
    mobile.share_library(space.clone(), device.clone()).unwrap();
    assert_eq!(mobile.sync_config().unwrap().shares[0].library, space);
    mobile.remove_space(space).unwrap();
    assert!(mobile.sync_config().unwrap().shares.is_empty());
    mobile.stop_sync();
    assert!(!mobile.sync_running());
    mobile.clone().start_sync(sync, Box::new(Quiet)).unwrap();
    assert_eq!(mobile.sync_address().unwrap(), address);
    assert_eq!(mobile.sync_config().unwrap().devices[0].name, "Laptop");
    mobile.stop_sync();
}
