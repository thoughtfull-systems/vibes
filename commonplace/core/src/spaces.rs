// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Every library on a device, shown together as spaces, as decision 0007 describes.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use loro::ExportMode;

use crate::actions::order_key;
use crate::library::{MODIFIED, loro_error};
use crate::{
    ActionChange, ActionDates, ActionEntry, ActionView, Answer, Date, Error, Handled, Library,
    Note, NoteId, Question, Schedule, plain_links,
};

const LIBRARIES: &str = "libraries";
const LOCK: &str = "lock";
/// The files of a library kept directly in the directory, before libraries had folders of their
/// own.
const SINGLE_LIBRARY: [&str; 2] = ["library.loro", "library.log"];

struct Space {
    key: String,
    library: Library,
    shown: bool,
}

/// The libraries on a device, each in a folder of its own named by a key that never changes. The
/// lists cover the libraries that show, and everything else covers them all.
pub struct Spaces {
    /// Counts changes to which libraries exist or show, which no library's commits record.
    changes: u64,
    dir: Option<PathBuf>,
    // Holds the lock for as long as the spaces are open.
    _lock: Option<File>,
    migrated: Option<String>,
    spaces: Vec<Space>,
}

impl Spaces {
    /// Opens every library in `dir`, creating one if there are none. A library kept directly in
    /// `dir`, as before spaces, moves into a folder of its own.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        fs::create_dir_all(dir.join(LIBRARIES))?;
        let lock = File::create(dir.join(LOCK))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::Locked(dir.to_owned())),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        let migrated = migrate(dir)?;
        let mut keys = Vec::new();
        for entry in fs::read_dir(dir.join(LIBRARIES))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                keys.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        keys.sort();
        let mut spaces = Self {
            changes: 0,
            dir: Some(dir.to_owned()),
            _lock: Some(lock),
            migrated,
            spaces: Vec::new(),
        };
        for key in keys {
            let library = Library::open(&dir.join(LIBRARIES).join(&key))?;
            spaces.spaces.push(Space { key, library, shown: true });
        }
        if spaces.spaces.is_empty() {
            spaces.create()?;
        }
        Ok(spaces)
    }

    /// Gathers libraries that are never saved, keyed as given.
    pub fn in_memory(keys: &[&str]) -> Self {
        let mut spaces: Vec<Space> = keys
            .iter()
            .map(|&key| Space { key: key.to_owned(), library: Library::in_memory(), shown: true })
            .collect();
        spaces.sort_by(|a, b| a.key.cmp(&b.key));
        Self { changes: 0, dir: None, _lock: None, migrated: None, spaces }
    }

    /// The key of the library that `open` moved into a folder of its own, if it did.
    pub fn migrated(&self) -> Option<&str> {
        self.migrated.as_deref()
    }

    /// Every library's key, in the order the keys sort.
    pub fn keys(&self) -> Vec<String> {
        self.spaces.iter().map(|space| space.key.clone()).collect()
    }

    pub fn library(&self, key: &str) -> Option<&Library> {
        self.spaces.iter().find(|space| space.key == key).map(|space| &space.library)
    }

    /// Creates an empty library, and returns its key.
    pub fn create(&mut self) -> Result<String, Error> {
        let key = NoteId::generate().to_string();
        let library = match &self.dir {
            Some(dir) => Library::open(&dir.join(LIBRARIES).join(&key))?,
            None => Library::in_memory(),
        };
        self.spaces.push(Space { key: key.clone(), library, shown: true });
        self.spaces.sort_by(|a, b| a.key.cmp(&b.key));
        self.changes += 1;
        Ok(key)
    }

    /// Deletes a library and its folder for good. The last library stays.
    pub fn remove(&mut self, key: &str) -> Result<(), Error> {
        let index = self.index(key)?;
        if self.spaces.len() == 1 {
            return Err(Error::LastLibrary);
        }
        // Dropping the library releases its lock before its folder goes.
        self.spaces.remove(index);
        self.changes += 1;
        if let Some(dir) = &self.dir {
            match fs::remove_dir_all(dir.join(LIBRARIES).join(key)) {
                Err(error) if error.kind() != ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    /// Merges `source` into `target`, as syncing them would, then deletes `source`.
    pub fn merge(&mut self, source: &str, target: &str) -> Result<(), Error> {
        let (from, to) = (self.index(source)?, self.index(target)?);
        if from == to {
            return Err(Error::SameLibrary);
        }
        let (source_library, target_library) = pair(&mut self.spaces, from, to);
        source_library.check_writable()?;
        let before = target_library.doc.fork();
        let bytes = source_library.doc.export(ExportMode::Snapshot).map_err(loro_error)?;
        target_library.import(&bytes)?;
        // A note that a move left in both libraries, or deleted from one, can lose to the other
        // side's deletion, so the merge writes back each note that either library held.
        let originals = [source_library.doc.get_map("notes"), before.get_map("notes")];
        if target_library.restore_lost(&originals)? {
            target_library.commit()?;
        }
        self.remove(source)
    }

    /// Whether the library shows in the lists. A device's only library always shows.
    pub fn is_shown(&self, key: &str) -> bool {
        self.spaces.len() == 1 || self.spaces.iter().any(|space| space.key == key && space.shown)
    }

    pub fn set_shown(&mut self, key: &str, shown: bool) {
        if let Some(space) = self.spaces.iter_mut().find(|space| space.key == key)
            && space.shown != shown
        {
            space.shown = shown;
            self.changes += 1;
        }
    }

    /// The key of the library that holds the note.
    pub fn space_of(&self, id: NoteId) -> Option<&str> {
        self.spaces
            .iter()
            .find(|space| space.library.note_map(id).is_ok())
            .map(|space| space.key.as_str())
    }

    /// Whether any library is read-only, because a newer version of Commonplace wrote it.
    pub fn is_read_only(&self) -> bool {
        self.spaces.iter().any(|space| space.library.is_read_only())
    }

    /// How many changes this instance has made, to any library or to which libraries show.
    pub fn commits(&self) -> u64 {
        self.changes + self.spaces.iter().map(|space| space.library.commits()).sum::<u64>()
    }

    pub fn note(&self, id: NoteId) -> Option<Note> {
        self.spaces.iter().find_map(|space| space.library.note(id))
    }

    /// The title a link to `id` shows, as plain text: the note's label, or the label it had when it
    /// was deleted for good.
    pub fn link_title(&self, id: NoteId) -> Option<String> {
        // A link inside the title shows its target's label without expanding that label's own
        // links, so titles that link to each other can't recurse.
        let nested = |id| self.raw_label(id).map(|label| plain_links(&label, |_| None));
        self.raw_label(id).map(|label| plain_links(&label, nested))
    }

    /// The note's label as its body writes it, or the label it had when it was deleted for good,
    /// from any library.
    fn raw_label(&self, id: NoteId) -> Option<String> {
        match self.note(id) {
            Some(note) => note.label(),
            None => self.all().find_map(|library| library.deleted_label(id)),
        }
    }

    /// The notes in the libraries that show, including those in the trash, most recently modified
    /// first.
    pub fn notes(&self) -> Vec<Note> {
        newest_first(self.shown().flat_map(Library::notes).collect())
    }

    /// Notes outside the trash, in the libraries that show, whose bodies contain every word of
    /// `query`, ignoring case.
    pub fn search(&self, query: &str) -> Vec<Note> {
        newest_first(self.shown().flat_map(|library| library.search(query)).collect())
    }

    /// Notes outside the trash, in the libraries that show, that link to `id`.
    pub fn backlinks(&self, id: NoteId) -> Vec<Note> {
        newest_first(self.shown().flat_map(|library| library.backlinks(id)).collect())
    }

    pub fn notes_in_trash(&self) -> HashSet<NoteId> {
        self.all().flat_map(Library::notes_in_trash).collect()
    }

    pub fn is_in_trash(&self, id: NoteId) -> bool {
        self.owner(id).is_some_and(|library| library.is_in_trash(id))
    }

    pub fn action_dates(&self, today: Date) -> HashMap<NoteId, ActionDates> {
        self.all().flat_map(|library| library.action_dates(today)).collect()
    }

    pub fn action_paths(&self) -> HashMap<NoteId, Vec<NoteId>> {
        self.all().flat_map(Library::action_paths).collect()
    }

    pub fn action_descendants(&self, id: NoteId) -> Vec<NoteId> {
        self.owner(id).map(|library| library.action_descendants(id)).unwrap_or_default()
    }

    pub fn action_subtree(&self, id: NoteId, today: Date) -> Vec<ActionEntry> {
        self.owner(id).map(|library| library.action_subtree(id, today)).unwrap_or_default()
    }

    pub fn action_brief(&self, id: NoteId) -> Result<String, Error> {
        self.owner(id).ok_or(Error::NotFound(id))?.action_brief(id)
    }

    /// The actions a view shows from the libraries that show, in the view's order. Ties go to the
    /// library whose key sorts first.
    pub fn action_view(&self, view: ActionView, today: Date) -> Vec<ActionEntry> {
        let mut lists: Vec<Vec<ActionEntry>> =
            self.shown().map(|library| library.action_view(view, today)).collect();
        if lists.len() == 1 {
            return lists.remove(0);
        }
        let mut entries: Vec<ActionEntry> = lists.into_iter().flatten().collect();
        let action = |entry: &ActionEntry| entry.note.action.clone().unwrap_or_default();
        match view {
            ActionView::Closed => entries.sort_by_key(|entry| Reverse(action(entry).closed)),
            ActionView::Planning => entries.sort_by_key(|entry| action(entry).planning),
            ActionView::All => {
                // Each top-level action ranks by the most pressing action in its outline, as the
                // view ranks siblings.
                let mut outlines: Vec<Vec<ActionEntry>> = Vec::new();
                for entry in entries {
                    match outlines.last_mut() {
                        Some(outline) if entry.depth > 0 => outline.push(entry),
                        _ => outlines.push(vec![entry]),
                    }
                }
                outlines.sort_by_key(|outline| {
                    outline.iter().map(|entry| order_key(&entry.dates, today)).min()
                });
                return outlines.into_iter().flatten().collect();
            }
            _ => entries.sort_by_key(|entry| order_key(&entry.dates, today)),
        }
        entries
    }

    pub fn note_setting(&self, id: NoteId, key: &str) -> Option<i64> {
        self.owner(id)?.note_setting(id, key)
    }

    pub fn create_memo(&mut self, space: &str, body: &str) -> Result<NoteId, Error> {
        self.library_mut(space)?.create_memo(body)
    }

    pub fn create_inbox_task(&mut self, space: &str, body: &str) -> Result<NoteId, Error> {
        self.library_mut(space)?.create_inbox_task(body)
    }

    /// Creates a task as the last step of `parent`, in the parent's library.
    pub fn create_task(&mut self, body: &str, parent: NoteId) -> Result<NoteId, Error> {
        self.owner_mut(parent)?.create_task(body, Some(parent))
    }

    pub fn set_body(&mut self, id: NoteId, body: &str) -> Result<(), Error> {
        self.owner_mut(id)?.set_body(id, body)
    }

    pub fn trash(&mut self, id: NoteId) -> Result<(), Error> {
        self.owner_mut(id)?.trash(id)
    }

    pub fn restore(&mut self, id: NoteId) -> Result<(), Error> {
        self.owner_mut(id)?.restore(id)
    }

    pub fn delete(&mut self, id: NoteId) -> Result<(), Error> {
        self.owner_mut(id)?.delete(id)
    }

    pub fn set_archived(&mut self, id: NoteId, archived: bool) -> Result<(), Error> {
        self.owner_mut(id)?.set_archived(id, archived)
    }

    pub fn create_journal(&mut self, space: &str, name: &str) -> Result<NoteId, Error> {
        self.library_mut(space)?.create_journal(name)
    }

    /// The journals outside the trash and the archive in every library, shown or not, by label.
    pub fn all_journals(&self) -> Vec<Note> {
        journals_by_label(self.all())
    }

    /// The journals outside the trash and the archive in the libraries that show, by label.
    pub fn journals(&self) -> Vec<Note> {
        journals_by_label(self.shown())
    }

    pub fn journal_entries(&self, journal: NoteId) -> Vec<NoteId> {
        self.owner(journal).map(|library| library.journal_entries(journal)).unwrap_or_default()
    }

    pub fn set_journal_color(&mut self, id: NoteId, color: &str) -> Result<(), Error> {
        self.owner_mut(id)?.set_journal_color(id, color)
    }

    pub fn add_question(&mut self, id: NoteId, question: &Question) -> Result<(), Error> {
        self.owner_mut(id)?.add_question(id, question)
    }

    pub fn set_question(&mut self, id: NoteId, question: &Question) -> Result<(), Error> {
        self.owner_mut(id)?.set_question(id, question)
    }

    pub fn move_question(&mut self, id: NoteId, question: &str, up: bool) -> Result<(), Error> {
        self.owner_mut(id)?.move_question(id, question, up)
    }

    pub fn set_schedule(&mut self, id: NoteId, schedule: &Schedule) -> Result<(), Error> {
        self.owner_mut(id)?.set_schedule(id, schedule)
    }

    pub fn remove_schedule(&mut self, id: NoteId, schedule: &str) -> Result<(), Error> {
        self.owner_mut(id)?.remove_schedule(id, schedule)
    }

    pub fn handle_prompt(
        &mut self,
        journal: NoteId,
        key: &str,
        handled: Handled,
        today: Date,
    ) -> Result<(), Error> {
        self.owner_mut(journal)?.handle_prompt(journal, key, handled, today)
    }

    /// Creates an entry in `journal`, in the journal's library.
    pub fn create_entry(
        &mut self,
        journal: NoteId,
        body: &str,
        at: i64,
        prompt: Option<(&str, i64)>,
        answers: &BTreeMap<String, Answer>,
        today: Date,
    ) -> Result<NoteId, Error> {
        self.owner_mut(journal)?.create_entry(journal, body, at, prompt, answers, today)
    }

    pub fn set_entry_at(&mut self, id: NoteId, at: i64) -> Result<(), Error> {
        self.owner_mut(id)?.set_entry_at(id, at)
    }

    pub fn set_answer(
        &mut self,
        id: NoteId,
        question: &str,
        answer: Option<&Answer>,
    ) -> Result<(), Error> {
        self.owner_mut(id)?.set_answer(id, question, answer)
    }

    /// Turns a memo into an entry in `journal`, which must be in the memo's library.
    pub fn convert_memo(&mut self, memo: NoteId, journal: NoteId) -> Result<NoteId, Error> {
        if self.owner_index(memo)? != self.owner_index(journal)? {
            return Err(Error::DifferentLibraries);
        }
        self.owner_mut(memo)?.convert_memo(memo, journal)
    }

    pub fn change_action(&mut self, id: NoteId, change: ActionChange) -> Result<(), Error> {
        self.owner_mut(id)?.change_action(id, change)
    }

    pub fn set_note_setting(&mut self, id: NoteId, key: &str, value: i64) -> Result<(), Error> {
        self.owner_mut(id)?.set_note_setting(id, key, value)
    }

    pub fn remove_note_setting(&mut self, id: NoteId, key: &str) -> Result<(), Error> {
        self.owner_mut(id)?.remove_note_setting(id, key)
    }

    /// Moves an action under `parent`, or to the top level of its library, at `index` among its
    /// new siblings, or last. A parent in another library takes the action and its steps there.
    pub fn move_action(
        &mut self,
        id: NoteId,
        parent: Option<NoteId>,
        index: Option<usize>,
    ) -> Result<(), Error> {
        if let Some(parent) = parent {
            let to = self.owner_index(parent)?;
            if to != self.owner_index(id)? {
                self.transfer(id, to, Some(parent))?;
            }
        }
        self.owner_mut(id)?.move_action(id, parent, index)
    }

    /// Moves an action next to `sibling`, under the same parent, which can be in another library.
    pub fn move_action_beside(
        &mut self,
        id: NoteId,
        sibling: NoteId,
        after: bool,
    ) -> Result<(), Error> {
        let to = self.owner_index(sibling)?;
        if to != self.owner_index(id)? {
            let parent = self.spaces[to].library.action_parent(sibling);
            self.transfer(id, to, parent)?;
        }
        self.owner_mut(id)?.move_action_beside(id, sibling, after)
    }

    pub fn move_action_among_siblings(&mut self, id: NoteId, up: bool) -> Result<(), Error> {
        self.owner_mut(id)?.move_action_among_siblings(id, up)
    }

    /// Moves a note, with its steps if it's an action, to the top level of another library.
    pub fn move_to_space(&mut self, id: NoteId, space: &str) -> Result<(), Error> {
        if self.note(id).is_some_and(|note| note.entry.is_some()) {
            return Err(Error::EntryMovesWithJournal(id));
        }
        let to = self.index(space)?;
        if to != self.owner_index(id)? {
            self.transfer(id, to, None)?;
        }
        Ok(())
    }

    /// Writes a note, with its subtree if it's an action or its entries if it's a journal, into
    /// another library under `parent`, then deletes it from its own, skipping the trash so it
    /// doesn't linger there.
    fn transfer(&mut self, id: NoteId, to: usize, parent: Option<NoteId>) -> Result<(), Error> {
        let from = self.owner_index(id)?;
        let (source, target) = pair(&mut self.spaces, from, to);
        source.check_writable()?;
        target.check_writable()?;
        let note = source.note(id).ok_or(Error::NotFound(id))?;
        let is_action = note.action.is_some();
        let mut moving = vec![(id, parent)];
        let entries = if note.journal.is_some() { source.journal_entries(id) } else { Vec::new() };
        if is_action {
            let hierarchy = source.hierarchy();
            moving.extend(
                hierarchy.descendants(id).into_iter().map(|step| (step, hierarchy.parent(step))),
            );
        }
        for &(note, parent) in &moving {
            target.insert_copy(note, &source.note_map(note)?)?;
            if is_action {
                target.place_node(note, parent)?;
            }
        }
        for &entry in &entries {
            target.insert_copy(entry, &source.note_map(entry)?)?;
        }
        target.note_map(id)?.insert(MODIFIED, target.now()).map_err(loro_error)?;
        target.commit()?;
        if is_action {
            source.delete_action_subtree(id)?;
        } else {
            source.notes_map().delete(&id.to_string()).map_err(loro_error)?;
        }
        for entry in entries {
            source.notes_map().delete(&entry.to_string()).map_err(loro_error)?;
        }
        source.commit()
    }

    fn shown(&self) -> impl Iterator<Item = &Library> {
        self.spaces.iter().filter(|space| self.is_shown(&space.key)).map(|space| &space.library)
    }

    fn all(&self) -> impl Iterator<Item = &Library> {
        self.spaces.iter().map(|space| &space.library)
    }

    fn index(&self, key: &str) -> Result<usize, Error> {
        self.spaces
            .iter()
            .position(|space| space.key == key)
            .ok_or_else(|| Error::NoSuchLibrary(key.to_owned()))
    }

    pub fn library_mut(&mut self, key: &str) -> Result<&mut Library, Error> {
        let index = self.index(key)?;
        Ok(&mut self.spaces[index].library)
    }

    fn owner(&self, id: NoteId) -> Option<&Library> {
        self.all().find(|library| library.note_map(id).is_ok())
    }

    fn owner_index(&self, id: NoteId) -> Result<usize, Error> {
        self.spaces
            .iter()
            .position(|space| space.library.note_map(id).is_ok())
            .ok_or(Error::NotFound(id))
    }

    fn owner_mut(&mut self, id: NoteId) -> Result<&mut Library, Error> {
        let index = self.owner_index(id)?;
        Ok(&mut self.spaces[index].library)
    }
}

/// The journals outside the trash and the archive in `libraries`, by label, ignoring case.
fn journals_by_label<'a>(libraries: impl Iterator<Item = &'a Library>) -> Vec<Note> {
    let mut journals: Vec<Note> = libraries
        .flat_map(|library| {
            library
                .journals()
                .into_iter()
                // A journal is never in the action tree, so only its own flag puts it in the trash.
                .filter(|note| note.archived.is_none() && !note.is_trashed())
        })
        .collect();
    journals.sort_by_key(|note| (note.label().unwrap_or_default().to_lowercase(), note.id));
    journals
}

/// Moves the files of a library kept directly in `dir` into a folder of its own, and returns the
/// folder's key.
fn migrate(dir: &Path) -> Result<Option<String>, Error> {
    let files: Vec<&str> =
        SINGLE_LIBRARY.into_iter().filter(|file| dir.join(file).exists()).collect();
    if files.is_empty() {
        return Ok(None);
    }
    let key = NoteId::generate().to_string();
    let folder = dir.join(LIBRARIES).join(&key);
    fs::create_dir_all(&folder)?;
    for file in files {
        fs::rename(dir.join(file), folder.join(file))?;
    }
    File::open(dir)?.sync_all()?;
    Ok(Some(key))
}

/// Mutable references to two different libraries.
fn pair(spaces: &mut [Space], first: usize, second: usize) -> (&mut Library, &mut Library) {
    if first < second {
        let (head, tail) = spaces.split_at_mut(second);
        (&mut head[first].library, &mut tail[0].library)
    } else {
        let (head, tail) = spaces.split_at_mut(first);
        (&mut tail[0].library, &mut head[second].library)
    }
}

fn newest_first(mut notes: Vec<Note>) -> Vec<Note> {
    notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 10, 22).unwrap()
    }

    fn ids(notes: Vec<Note>) -> Vec<NoteId> {
        notes.into_iter().map(|note| note.id).collect()
    }

    #[test]
    fn lists_only_the_libraries_that_show() {
        let mut spaces = Spaces::in_memory(&["a", "b"]);
        let a = spaces.create_memo("a", "apple").unwrap();
        let b = spaces.create_memo("b", "banana").unwrap();
        assert_eq!(ids(spaces.notes()), [b, a]);
        spaces.set_shown("a", false);
        assert_eq!(ids(spaces.search("")), [b]);
        assert!(spaces.note(a).is_some());
        spaces.set_shown("b", false);
        assert!(spaces.notes().is_empty());
        let mut single = Spaces::in_memory(&["only"]);
        single.create_memo("only", "x").unwrap();
        single.set_shown("only", false);
        assert_eq!(single.notes().len(), 1);
    }

    #[test]
    fn orders_views_across_libraries() {
        let mut spaces = Spaces::in_memory(&["a", "b"]);
        let later = spaces.library_mut("a").unwrap().create_task("later", None).unwrap();
        let sooner = spaces.library_mut("b").unwrap().create_task("sooner", None).unwrap();
        let undated = spaces.library_mut("a").unwrap().create_task("undated", None).unwrap();
        spaces.change_action(later, ActionChange::Due(Some(today().add_days(5)))).unwrap();
        spaces.change_action(sooner, ActionChange::Due(Some(today().add_days(1)))).unwrap();
        let next: Vec<NoteId> = spaces
            .action_view(ActionView::Next, today())
            .into_iter()
            .map(|entry| entry.note.id)
            .collect();
        assert_eq!(next, [sooner, later, undated]);
    }

    #[test]
    fn moves_a_journal_with_its_entries() {
        let mut spaces = Spaces::in_memory(&["home", "work"]);
        let journal = spaces.create_journal("home", "Work log").unwrap();
        let today: Date = "2026-10-26".parse().unwrap();
        let entry = spaces.create_entry(journal, "x", 1, None, &BTreeMap::new(), today).unwrap();
        assert!(matches!(
            spaces.move_to_space(entry, "work"),
            Err(Error::EntryMovesWithJournal(_))
        ));
        spaces.move_to_space(journal, "work").unwrap();
        assert_eq!(spaces.space_of(journal), Some("work"));
        assert_eq!(spaces.space_of(entry), Some("work"));
        assert_eq!(spaces.journal_entries(journal), [entry]);
        let names: Vec<String> =
            spaces.journals().iter().filter_map(|journal| journal.label()).collect();
        assert_eq!(names, ["Work log"]);
        spaces.set_archived(journal, true).unwrap();
        assert!(spaces.journals().is_empty());
        assert!(spaces.all_journals().is_empty());
    }

    #[test]
    fn moves_an_action_and_its_steps_to_another_library() {
        let mut spaces = Spaces::in_memory(&["home", "work"]);
        let project = spaces.library_mut("home").unwrap().create_task("project", None).unwrap();
        let step = spaces.create_task("step", project).unwrap();
        let substep = spaces.create_task("substep", step).unwrap();
        let target = spaces.library_mut("work").unwrap().create_task("target", None).unwrap();
        spaces.move_action(step, Some(target), None).unwrap();
        assert_eq!(spaces.space_of(step), Some("work"));
        assert_eq!(spaces.space_of(substep), Some("work"));
        let work = spaces.library("work").unwrap();
        assert_eq!(work.action_parent(step), Some(target));
        assert_eq!(work.action_parent(substep), Some(step));
        assert!(work.note(target).unwrap().action.unwrap().project);
        let home = spaces.library("home").unwrap();
        assert!(home.note(step).is_none() && home.note(substep).is_none());
        assert!(home.notes_in_trash().is_empty());
        let memo = spaces.create_memo("home", "memo").unwrap();
        spaces.move_to_space(memo, "work").unwrap();
        assert_eq!(spaces.note(memo).unwrap().body, "memo");
        assert_eq!(spaces.space_of(memo), Some("work"));
    }

    #[test]
    fn moves_a_nested_step_without_its_projects() {
        let mut spaces = Spaces::in_memory(&["home", "work"]);
        let outer = spaces.library_mut("home").unwrap().create_task("outer", None).unwrap();
        let inner = spaces.create_task("inner", outer).unwrap();
        let step = spaces.create_task("step", inner).unwrap();
        let sibling = spaces.create_task("sibling", inner).unwrap();
        spaces.move_to_space(step, "work").unwrap();
        assert_eq!(spaces.space_of(step), Some("work"));
        for id in [outer, inner, sibling] {
            assert_eq!(spaces.space_of(id), Some("home"));
        }
        assert_eq!(spaces.library("work").unwrap().notes().len(), 1);
        assert_eq!(spaces.library("work").unwrap().action_parent(step), None);
        let home = spaces.library("home").unwrap();
        assert_eq!(home.action_parent(sibling), Some(inner));
        assert_eq!(home.action_parent(inner), Some(outer));
    }

    #[test]
    fn merges_libraries_and_keeps_notes_a_move_deleted() {
        let mut spaces = Spaces::in_memory(&["a", "b"]);
        let kept = spaces.create_memo("a", "kept").unwrap();
        let moved = spaces.create_memo("a", "moved").unwrap();
        spaces.move_to_space(moved, "b").unwrap();
        let theirs = spaces.library_mut("b").unwrap().create_task("theirs", None).unwrap();
        spaces.merge("a", "b").unwrap();
        assert_eq!(spaces.keys(), ["b"]);
        for id in [kept, moved, theirs] {
            assert_eq!(spaces.space_of(id), Some("b"));
        }
        assert_eq!(spaces.note(moved).unwrap().body, "moved");
        assert!(matches!(spaces.remove("b"), Err(Error::LastLibrary)));
    }

    #[test]
    fn finds_link_titles_across_libraries() {
        let mut spaces = Spaces::in_memory(&["a", "b"]);
        let deleted = spaces.create_memo("a", "# Old plan").unwrap();
        let moved = spaces.create_memo("a", "Moved").unwrap();
        let linking =
            spaces.create_memo("b", &format!("See [[{deleted}]] or [[{moved}|it]]")).unwrap();
        let cycle = spaces.create_memo("b", &format!("Back to [[{linking}]]")).unwrap();
        spaces.trash(deleted).unwrap();
        spaces.delete(deleted).unwrap();
        spaces.move_to_space(moved, "b").unwrap();
        assert_eq!(spaces.link_title(deleted).as_deref(), Some("Old plan"));
        assert_eq!(spaces.link_title(moved).as_deref(), Some("Moved"));
        assert_eq!(spaces.library("a").unwrap().deleted_label(moved), None);
        assert_eq!(spaces.link_title(linking).as_deref(), Some("See Old plan or it"));
        assert_eq!(
            spaces.link_title(cycle).as_deref(),
            Some(&*format!("Back to See {deleted} or it"))
        );
        let linked_title =
            spaces.create_memo("b", &format!("Follow up on [[{moved}|it]]")).unwrap();
        spaces.trash(linked_title).unwrap();
        spaces.delete(linked_title).unwrap();
        assert_eq!(spaces.link_title(linked_title).as_deref(), Some("Follow up on it"));
        let both_deleted = spaces.create_memo("b", &format!("Instead of [[{deleted}]]")).unwrap();
        spaces.trash(both_deleted).unwrap();
        spaces.delete(both_deleted).unwrap();
        assert_eq!(spaces.link_title(both_deleted).as_deref(), Some("Instead of Old plan"));
    }

    #[test]
    fn moves_a_single_library_into_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let id = Library::open(dir.path()).unwrap().create_memo("old").unwrap();
        let spaces = Spaces::open(dir.path()).unwrap();
        let key = spaces.migrated().unwrap().to_owned();
        assert_eq!(spaces.keys(), [key.as_str()]);
        assert_eq!(spaces.note(id).unwrap().body, "old");
        assert!(!dir.path().join("library.loro").exists());
        drop(spaces);
        let spaces = Spaces::open(dir.path()).unwrap();
        assert_eq!(spaces.migrated(), None);
        assert_eq!(spaces.keys(), [key]);
        assert!(matches!(Spaces::open(dir.path()), Err(Error::Locked(_))));
    }

    #[test]
    fn creates_and_removes_library_folders() {
        let dir = tempfile::tempdir().unwrap();
        let mut spaces = Spaces::open(dir.path()).unwrap();
        let first = spaces.keys().remove(0);
        let second = spaces.create().unwrap();
        spaces.create_memo(&second, "x").unwrap();
        spaces.remove(&first).unwrap();
        assert!(!dir.path().join(LIBRARIES).join(&first).exists());
        drop(spaces);
        assert_eq!(Spaces::open(dir.path()).unwrap().keys(), [second]);
    }
}
