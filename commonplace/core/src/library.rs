// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{
    Container, ExportMode, LoroDoc, LoroList, LoroMap, LoroText, LoroValue, UpdateOptions,
    ValueOrContainer, VersionVector,
};

use crate::actions::{ACTION, Action};
use crate::journals::{ENTRY, ENTRY_KIND, Entry, JOURNAL, JOURNAL_KIND, Journal};
use crate::store::Store;
use crate::{Error, NoteId, label, parse_links};

const SETTINGS: &str = "settings";

const FORMAT: &str = "commonplace-library";
const VERSION: i64 = 3;

pub(crate) const ARCHIVED: &str = "archived";
const BODY: &str = "body";
const CREATED: &str = "created";
const DELETED: &str = "deleted";
const KIND: &str = "kind";
const LABEL: &str = "label";
pub(crate) const MODIFIED: &str = "modified";
const TRASHED: &str = "trashed";

pub const MEMO: &str = "memo";
pub(crate) const ACTION_KIND: &str = "action";

/// The version of a library that an editor's text reflects, so that saving the text keeps changes
/// that arrived since. See [`Library::set_body_since`].
#[derive(Clone, Debug, PartialEq)]
pub struct EditBase(loro::Frontiers);

/// A copy of a note's contents at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub body: String,
    pub created: i64,
    pub id: NoteId,
    pub kind: String,
    pub modified: i64,
    /// The action's fields, for a note whose kind is action.
    pub action: Option<Action>,
    /// When the owner set the note aside, out of every list.
    pub archived: Option<i64>,
    /// The journal's fields, for a note whose kind is journal.
    pub journal: Option<Journal>,
    /// The entry's fields, for a note whose kind is entry.
    pub entry: Option<Entry>,
    pub trashed: Option<i64>,
}

impl Note {
    /// The note's title, derived from its body.
    pub fn label(&self) -> Option<String> {
        if self.entry.is_some() { crate::label::entry_label(&self.body) } else { label(&self.body) }
    }

    /// Whether the note shows as a memo: a memo, or a note of a kind this version doesn't know.
    pub fn is_memo(&self) -> bool {
        self.action.is_none() && self.journal.is_none() && self.entry.is_none()
    }

    pub fn is_trashed(&self) -> bool {
        self.trashed.is_some()
    }

    /// Whether the body contains every word of `query`, ignoring case.
    pub fn matches(&self, query: &str) -> bool {
        let body = self.body.to_lowercase();
        query.split_whitespace().all(|word| body.contains(&word.to_lowercase()))
    }
}

/// One person's collection of notes, stored as the library format spec defines.
pub struct Library {
    /// A fixed time for every change, in milliseconds since the epoch, for backdated samples.
    clock: Option<i64>,
    /// How many changes this instance has committed, so callers can tell whether a write changed
    /// anything.
    commits: u64,
    pub(crate) doc: LoroDoc,
    read_only: bool,
    store: Option<Store>,
}

impl Library {
    /// Opens the library in `dir`, creating it if the directory holds none.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let doc = LoroDoc::new();
        let store = Store::open(dir, &doc)?;
        let mut library =
            Self { clock: None, commits: 0, doc, read_only: false, store: Some(store) };
        library.prepare()?;
        Ok(library)
    }

    /// Creates an empty library that's never saved.
    pub fn in_memory() -> Self {
        let mut library =
            Self { clock: None, commits: 0, doc: LoroDoc::new(), read_only: false, store: None };
        // Without a store, preparing a new library can't fail.
        library.prepare().unwrap();
        library
    }

    /// Creates an empty library that's never saved, and that dates every change `clock`, in
    /// milliseconds since the epoch.
    pub(crate) fn in_memory_at(clock: i64) -> Self {
        let mut library = Self::in_memory();
        library.clock = Some(clock);
        library
    }

    /// The time to record for a change, in milliseconds since the epoch.
    pub(crate) fn now(&self) -> i64 {
        self.clock.unwrap_or_else(system_now)
    }

    /// Writes the format and version, and brings older data up to date, or opens read-only if a
    /// newer version wrote the library.
    fn prepare(&mut self) -> Result<(), Error> {
        let meta = self.doc.get_map("meta");
        let format = meta.get("format").and_then(value).and_then(string);
        let version = meta.get("version").and_then(value).and_then(integer);
        let upgraded = match (format, version) {
            (None, _) => {
                meta.insert("format", FORMAT).map_err(loro_error)?;
                meta.insert("version", VERSION).map_err(loro_error)?;
                true
            }
            (Some(format), _) if format != FORMAT => return Err(Error::NotALibrary(format)),
            (Some(_), Some(version)) if version > VERSION => {
                self.read_only = true;
                return Ok(());
            }
            (Some(_), Some(VERSION)) => false,
            (Some(_), _) => {
                meta.insert("version", VERSION).map_err(loro_error)?;
                true
            }
        };
        if self.move_archived_to_notes()? || upgraded { self.commit() } else { Ok(()) }
    }

    /// Moves `archived` from the action fields, where version 2 kept it, to the note, and returns
    /// whether it found any. A device still on version 2 can archive an action offline and sync it
    /// after another device upgrades the library, so this runs on every open and import.
    fn move_archived_to_notes(&self) -> Result<bool, Error> {
        let notes = self.notes_map();
        let mut keys = Vec::new();
        notes.for_each(|key, _| keys.push(key.to_owned()));
        let mut moved = false;
        for key in keys {
            let Some(ValueOrContainer::Container(Container::Map(note))) = notes.get(&key) else {
                continue;
            };
            let Some(ValueOrContainer::Container(Container::Map(action))) = note.get(ACTION) else {
                continue;
            };
            let Some(archived) = action.get(ARCHIVED).and_then(value) else {
                continue;
            };
            if note.get(ARCHIVED).is_none() {
                note.insert(ARCHIVED, archived).map_err(loro_error)?;
            }
            action.delete(ARCHIVED).map_err(loro_error)?;
            moved = true;
        }
        Ok(moved)
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn create_memo(&mut self, body: &str) -> Result<NoteId, Error> {
        self.check_writable()?;
        let id = self.insert_note(body, MEMO)?;
        self.commit()?;
        Ok(id)
    }

    /// Inserts a note with the fields every kind has, without committing.
    pub(crate) fn insert_note(&mut self, body: &str, kind: &str) -> Result<NoteId, Error> {
        let id = NoteId::generate();
        let now = self.now();
        let note = self
            .notes_map()
            .insert_container(&id.to_string(), LoroMap::new())
            .map_err(loro_error)?;
        let text = note.insert_container(BODY, LoroText::new()).map_err(loro_error)?;
        text.insert(0, body).map_err(loro_error)?;
        note.insert(CREATED, now).map_err(loro_error)?;
        note.insert(KIND, kind).map_err(loro_error)?;
        note.insert(MODIFIED, now).map_err(loro_error)?;
        Ok(id)
    }

    pub fn note(&self, id: NoteId) -> Option<Note> {
        let value = self.notes_map().get(&id.to_string())?.get_deep_value();
        note_from_value(id, &value)
    }

    /// Every note, including those in the trash, most recently modified first.
    pub fn notes(&self) -> Vec<Note> {
        let LoroValue::Map(notes) = self.notes_map().get_deep_value() else {
            return Vec::new();
        };
        let mut notes: Vec<Note> = notes
            .iter()
            .filter_map(|(key, value)| note_from_value(key.parse().ok()?, value))
            .collect();
        notes.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
        notes
    }

    pub fn set_body(&mut self, id: NoteId, body: &str) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        let text = self.body_text(id)?;
        if text.to_string() == body {
            return Ok(());
        }
        text.update(body, UpdateOptions::default()).map_err(loro_error)?;
        note.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// The version that an editor's text reflects once it loads a body from the library.
    pub fn edit_base(&self) -> EditBase {
        EditBase(self.doc.oplog_frontiers())
    }

    /// Saves an editor's text, which reflects the library at `base`, and returns the version that
    /// the text reflects afterward. Changes that reached the library since `base`, such as another
    /// device's, survive, because the edit applies to the library as it was at `base` and then
    /// merges. Returns whether such changes arrived, so the editor knows to load the merged text.
    pub fn set_body_since(
        &mut self,
        id: NoteId,
        body: &str,
        base: &EditBase,
    ) -> Result<(EditBase, bool), Error> {
        if base.0 == self.doc.oplog_frontiers() {
            self.set_body(id, body)?;
            return Ok((self.edit_base(), false));
        }
        self.check_writable()?;
        let mut branch = Self {
            clock: self.clock,
            commits: 0,
            doc: self.doc.fork_at(&base.0).map_err(loro_error)?,
            read_only: false,
            store: None,
        };
        let before = branch.doc.oplog_vv();
        branch.set_body(id, body)?;
        let after = branch.edit_base();
        let updates = branch.doc.export(ExportMode::updates(&before)).map_err(loro_error)?;
        self.doc.import(&updates).map_err(loro_error)?;
        self.commit()?;
        Ok((after, true))
    }

    pub(crate) fn body_text(&self, id: NoteId) -> Result<LoroText, Error> {
        match self.note_map(id)?.get(BODY) {
            Some(ValueOrContainer::Container(container)) => container.into_text().ok(),
            _ => None,
        }
        .ok_or(Error::Malformed(id))
    }

    /// Archives the note, or takes it out of the archive, without changing it.
    pub fn set_archived(&mut self, id: NoteId, archived: bool) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        match archived {
            true if note.get(ARCHIVED).is_some() => return Ok(()),
            true => note.insert(ARCHIVED, self.now()),
            false => note.delete(ARCHIVED),
        }
        .map_err(loro_error)?;
        self.commit()
    }

    pub fn trash(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        let now = self.now();
        note.insert(TRASHED, now).map_err(loro_error)?;
        note.insert(MODIFIED, now).map_err(loro_error)?;
        self.commit()
    }

    /// Takes a note out of the trash, with any actions above it that are in the trash, so it shows
    /// again rather than staying in the trash through one of them.
    pub fn restore(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let now = self.now();
        let mut ancestors: Vec<NoteId> = self.hierarchy().ancestors(id).collect();
        ancestors.extend(self.entry_journal(id));
        for note in std::iter::once(id).chain(ancestors) {
            if self.note(note).is_some_and(|note| note.is_trashed()) {
                let map = self.note_map(note)?;
                map.delete(TRASHED).map_err(loro_error)?;
                map.insert(MODIFIED, now).map_err(loro_error)?;
            }
        }
        self.commit()
    }

    /// Deletes a note for good, with its steps if it's an action. The note must be in the trash.
    pub fn delete(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note(id).ok_or(Error::NotFound(id))?;
        if !self.is_in_trash(id) {
            return Err(Error::NotTrashed(id));
        }
        let mut doomed = vec![id];
        if note.action.is_some() {
            doomed.extend(self.hierarchy().descendants(id));
        }
        let entries = if note.journal.is_some() { self.journal_entries(id) } else { Vec::new() };
        doomed.extend(&entries);
        let now = self.now();
        for &note in &doomed {
            let label = self.note(note).and_then(|note| note.label());
            let tombstone = self
                .doc
                .get_map(DELETED)
                .insert_container(&note.to_string(), LoroMap::new())
                .map_err(loro_error)?;
            tombstone.insert(DELETED, now).map_err(loro_error)?;
            if let Some(label) = label {
                tombstone.insert(LABEL, label).map_err(loro_error)?;
            }
        }
        if note.action.is_some() {
            self.delete_action_subtree(id)?;
        } else {
            self.notes_map().delete(&id.to_string()).map_err(loro_error)?;
        }
        for entry in entries {
            self.notes_map().delete(&entry.to_string()).map_err(loro_error)?;
        }
        self.commit()
    }

    /// The label a note had when it was deleted for good, so links to it can still name it.
    pub fn deleted_label(&self, id: NoteId) -> Option<String> {
        let LoroValue::Map(tombstone) =
            self.doc.get_map(DELETED).get(&id.to_string())?.get_deep_value()
        else {
            return None;
        };
        tombstone.get(LABEL).and_then(|label| string(label.clone()))
    }

    /// Notes outside the trash whose bodies contain every word of `query`, ignoring case.
    pub fn search(&self, query: &str) -> Vec<Note> {
        let in_trash = self.notes_in_trash();
        self.notes()
            .into_iter()
            .filter(|note| !in_trash.contains(&note.id) && note.matches(query))
            .collect()
    }

    /// Notes outside the trash that link to `id`, other than the note itself.
    pub fn backlinks(&self, id: NoteId) -> Vec<Note> {
        let in_trash = self.notes_in_trash();
        self.notes()
            .into_iter()
            .filter(|note| note.id != id && !in_trash.contains(&note.id))
            .filter(|note| parse_links(&note.body).iter().any(|link| link.note == Some(id)))
            .collect()
    }

    /// A whole-number setting, such as a sidebar's width, that follows the owner across devices.
    pub fn setting(&self, key: &str) -> Option<i64> {
        int_value(&self.doc.get_map(SETTINGS), key)
    }

    /// Changes a setting, writing nothing if it already has the value.
    pub fn set_setting(&mut self, key: &str, value: i64) -> Result<(), Error> {
        self.check_writable()?;
        if self.setting(key) == Some(value) {
            return Ok(());
        }
        self.doc.get_map(SETTINGS).insert(key, value).map_err(loro_error)?;
        self.commit()
    }

    /// A text setting, such as the screen the user last viewed, that follows the owner across
    /// devices.
    pub fn text_setting(&self, key: &str) -> Option<String> {
        match self.doc.get_map(SETTINGS).get(key) {
            Some(ValueOrContainer::Value(LoroValue::String(value))) => Some(value.to_string()),
            _ => None,
        }
    }

    /// Changes a text setting, writing nothing if it already has the value.
    pub fn set_text_setting(&mut self, key: &str, value: &str) -> Result<(), Error> {
        self.check_writable()?;
        if self.text_setting(key).as_deref() == Some(value) {
            return Ok(());
        }
        self.doc.get_map(SETTINGS).insert(key, value).map_err(loro_error)?;
        self.commit()
    }

    /// The keys of every setting in the library, so an implementation can carry its settings off
    /// to wherever it keeps them now.
    pub fn setting_keys(&self) -> Vec<String> {
        self.doc.get_map(SETTINGS).keys().map(|key| key.to_string()).collect()
    }

    /// A whole-number setting that belongs to one note, such as the height of a panel below its
    /// text, and goes with the note when it's deleted.
    pub fn note_setting(&self, id: NoteId, key: &str) -> Option<i64> {
        let settings =
            self.note_map(id).ok()?.get(SETTINGS)?.into_container().ok()?.into_map().ok()?;
        int_value(&settings, key)
    }

    /// Changes a note's setting, writing nothing if it already has the value. The note's
    /// modification time stays, since its content doesn't change.
    pub fn set_note_setting(&mut self, id: NoteId, key: &str, value: i64) -> Result<(), Error> {
        self.check_writable()?;
        if self.note_setting(id, key) == Some(value) {
            return Ok(());
        }
        let note = self.note_map(id)?;
        let settings = match note.get(SETTINGS) {
            Some(ValueOrContainer::Container(container)) => {
                container.into_map().map_err(|_| Error::Malformed(id))?
            }
            _ => note.insert_container(SETTINGS, LoroMap::new()).map_err(loro_error)?,
        };
        settings.insert(key, value).map_err(loro_error)?;
        self.commit()
    }

    /// Removes a note's setting, writing nothing if it has none. The note's modification time
    /// stays, since its content doesn't change.
    pub fn remove_note_setting(&mut self, id: NoteId, key: &str) -> Result<(), Error> {
        self.check_writable()?;
        if self.note_setting(id, key).is_none() {
            return Ok(());
        }
        let Some(ValueOrContainer::Container(container)) = self.note_map(id)?.get(SETTINGS) else {
            return Err(Error::Malformed(id));
        };
        let settings = container.into_map().map_err(|_| Error::Malformed(id))?;
        settings.delete(key).map_err(loro_error)?;
        self.commit()
    }

    pub(crate) fn notes_map(&self) -> LoroMap {
        self.doc.get_map("notes")
    }

    pub(crate) fn note_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.notes_map().get(&id.to_string()) {
            Some(ValueOrContainer::Container(container)) => {
                container.into_map().map_err(|_| Error::Malformed(id))
            }
            Some(_) => Err(Error::Malformed(id)),
            None => Err(Error::NotFound(id)),
        }
    }

    /// Writes a copy of `note`, a note's map from any document, as note `id`, without committing.
    pub(crate) fn insert_copy(&mut self, id: NoteId, note: &LoroMap) -> Result<(), Error> {
        let copy = self
            .notes_map()
            .insert_container(&id.to_string(), LoroMap::new())
            .map_err(loro_error)?;
        copy_map(note, &copy)
    }

    /// Exports the document in Loro's snapshot encoding, including its editing history.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, Error> {
        self.doc.export(loro::ExportMode::Snapshot).map_err(loro_error)
    }

    pub(crate) fn validate_snapshot(bytes: &[u8]) -> Result<(), Error> {
        let doc = LoroDoc::new();
        doc.import(bytes).map_err(loro_error)?;
        Self::validate(&doc)
    }

    /// The library's version vector, in Loro's encoding, which names every change it holds.
    pub fn version(&self) -> Vec<u8> {
        self.doc.oplog_vv().encode()
    }

    /// The changes that a library at `version` lacks, in Loro's update encoding, or `None` if it
    /// lacks none.
    pub fn updates_since(&self, version: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let version = VersionVector::decode(version).map_err(loro_error)?;
        if version.includes_vv(&self.doc.oplog_vv()) {
            return Ok(None);
        }
        self.doc.export(ExportMode::updates(&version)).map(Some).map_err(loro_error)
    }

    /// Merges changes from another device, after checking in a copy that they leave a library this
    /// version can write. With `repair`, it then writes back each note that either side held and
    /// the merge lost, as merging libraries does.
    pub fn merge_updates(&mut self, bytes: &[u8], repair: bool) -> Result<(), Error> {
        self.check_writable()?;
        let trial = self.doc.fork();
        trial.import(bytes).map_err(loro_error)?;
        Self::validate(&trial)?;
        let before = repair.then(|| self.doc.fork());
        self.doc.import(bytes).map_err(loro_error)?;
        self.move_archived_to_notes()?;
        if let Some(before) = before {
            // Changes that build on history this library lacks don't import on their own, but
            // then the two sides share history, and nothing needs repair on their account.
            let incoming = LoroDoc::new();
            let _ = incoming.import(bytes);
            self.restore_lost(&[before.get_map("notes"), incoming.get_map("notes")])?;
        }
        self.commit()
    }

    /// Writes back, without committing, each note in `originals` that the library no longer holds,
    /// and returns whether it wrote any.
    pub(crate) fn restore_lost(&mut self, originals: &[LoroMap]) -> Result<bool, Error> {
        let held: HashSet<NoteId> = originals
            .iter()
            .flat_map(|notes| notes.keys())
            .filter_map(|key| key.parse().ok())
            .collect();
        let mut restored = false;
        for id in held {
            if self.note_map(id).is_ok() {
                continue;
            }
            let original = originals.iter().find_map(|notes| note_map_in(notes, id));
            if let Some(original) = original {
                self.insert_copy(id, &original)?;
                restored = true;
            }
        }
        Ok(restored)
    }

    /// Checks that `doc` is a library that this version can write.
    fn validate(doc: &LoroDoc) -> Result<(), Error> {
        let meta = doc.get_map("meta");
        let format = meta.get("format").and_then(value).and_then(string);
        if format.as_deref() != Some(FORMAT) {
            return Err(Error::NotALibrary(format.unwrap_or_default()));
        }
        if meta.get("version").and_then(value).and_then(integer).unwrap_or(0) > VERSION {
            return Err(Error::ReadOnly);
        }
        Ok(())
    }

    /// Validates an incoming snapshot before merging any changes into this library.
    pub fn import_snapshot(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Self::validate_snapshot(bytes)?;
        self.import(bytes)?;
        self.prepare()
    }

    /// Merges another document's changes into the library, as a sync does.
    pub(crate) fn import(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.check_writable()?;
        self.doc.import(bytes).map_err(loro_error)?;
        self.move_archived_to_notes()?;
        self.commit()
    }

    pub(crate) fn check_writable(&self) -> Result<(), Error> {
        if self.read_only { Err(Error::ReadOnly) } else { Ok(()) }
    }

    /// How many changes this instance has committed.
    pub fn commits(&self) -> u64 {
        self.commits
    }

    pub(crate) fn commit(&mut self) -> Result<(), Error> {
        self.commits += 1;
        self.doc.commit();
        match &mut self.store {
            Some(store) => store.save(&self.doc),
            None => Ok(()),
        }
    }
}

fn note_map_in(notes: &LoroMap, id: NoteId) -> Option<LoroMap> {
    match notes.get(&id.to_string()) {
        Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
        _ => None,
    }
}

fn copy_map(from: &LoroMap, to: &LoroMap) -> Result<(), Error> {
    let mut entries = Vec::new();
    from.for_each(|key, value| entries.push((key.to_owned(), value)));
    for (key, value) in entries {
        match value {
            ValueOrContainer::Value(value) => to.insert(&key, value).map_err(loro_error)?,
            ValueOrContainer::Container(Container::Map(map)) => {
                copy_map(&map, &to.insert_container(&key, LoroMap::new()).map_err(loro_error)?)?
            }
            ValueOrContainer::Container(Container::Text(text)) => to
                .insert_container(&key, LoroText::new())
                .map_err(loro_error)?
                .insert(0, &text.to_string())
                .map_err(loro_error)?,
            ValueOrContainer::Container(Container::List(list)) => {
                copy_list(&list, &to.insert_container(&key, LoroList::new()).map_err(loro_error)?)?
            }
            // The spec puts no other kind of container in a note, so a plain value keeps what an
            // unknown one holds.
            ValueOrContainer::Container(other) => to
                .insert(&key, ValueOrContainer::Container(other).get_deep_value())
                .map_err(loro_error)?,
        }
    }
    Ok(())
}

fn copy_list(from: &LoroList, to: &LoroList) -> Result<(), Error> {
    let mut items = Vec::new();
    from.for_each(|value| items.push(value));
    for value in items {
        match value {
            ValueOrContainer::Value(value) => to.push(value).map_err(loro_error)?,
            ValueOrContainer::Container(Container::Map(map)) => {
                copy_map(&map, &to.push_container(LoroMap::new()).map_err(loro_error)?)?
            }
            ValueOrContainer::Container(Container::Text(text)) => to
                .push_container(LoroText::new())
                .map_err(loro_error)?
                .insert(0, &text.to_string())
                .map_err(loro_error)?,
            ValueOrContainer::Container(Container::List(list)) => {
                copy_list(&list, &to.push_container(LoroList::new()).map_err(loro_error)?)?
            }
            ValueOrContainer::Container(other) => {
                to.push(ValueOrContainer::Container(other).get_deep_value()).map_err(loro_error)?
            }
        }
    }
    Ok(())
}

fn int_value(map: &LoroMap, key: &str) -> Option<i64> {
    match map.get(key) {
        Some(ValueOrContainer::Value(LoroValue::I64(value))) => Some(value),
        _ => None,
    }
}

fn note_from_value(id: NoteId, value: &LoroValue) -> Option<Note> {
    let LoroValue::Map(map) = value else {
        return None;
    };
    let created = map.get(CREATED).and_then(|v| integer(v.clone()))?;
    let kind = map.get(KIND).and_then(|v| string(v.clone())).unwrap_or_else(|| MEMO.to_owned());
    let action = (kind == ACTION_KIND).then(|| Action::from_value(map.get(ACTION)));
    Some(Note {
        body: map.get(BODY).and_then(|v| string(v.clone())).unwrap_or_default(),
        created,
        id,
        modified: map.get(MODIFIED).and_then(|v| integer(v.clone())).unwrap_or(created),
        action,
        archived: map.get(ARCHIVED).and_then(|v| integer(v.clone())),
        journal: (kind == JOURNAL_KIND)
            .then(|| crate::journals::journal_from_value(map.get(JOURNAL))),
        entry: if kind == ENTRY_KIND {
            crate::journals::entry_from_value(map.get(ENTRY), created)
        } else {
            None
        },
        trashed: map.get(TRASHED).and_then(|v| integer(v.clone())),
        kind,
    })
}

pub(crate) fn value(value: ValueOrContainer) -> Option<LoroValue> {
    match value {
        ValueOrContainer::Value(value) => Some(value),
        ValueOrContainer::Container(_) => None,
    }
}

pub(crate) fn string(value: LoroValue) -> Option<String> {
    match value {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

pub(crate) fn integer(value: LoroValue) -> Option<i64> {
    match value {
        LoroValue::I64(i) => Some(i),
        _ => None,
    }
}

fn system_now() -> i64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
}

pub(crate) fn loro_error(error: impl ToString) -> Error {
    Error::Loro(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_editor_keeps_changes_that_arrive_while_it_shows_a_note() {
        let mut laptop = Library::in_memory();
        let id = laptop.create_memo("one\ntwo").unwrap();
        let mut phone = Library::in_memory();
        phone.merge_updates(&laptop.export_snapshot().unwrap(), false).unwrap();
        let base = laptop.edit_base();
        phone.set_body(id, "one\ntwo\nfrom the phone").unwrap();
        let updates = phone.updates_since(&laptop.version()).unwrap().unwrap();
        laptop.merge_updates(&updates, false).unwrap();

        let (base, merged) = laptop.set_body_since(id, "one, edited\ntwo", &base).unwrap();
        assert!(merged);
        assert_eq!(laptop.note(id).unwrap().body, "one, edited\ntwo\nfrom the phone");
        // The editor still shows the text without the phone's line, and saves it again.
        let (_, merged) = laptop.set_body_since(id, "one, edited again\ntwo", &base).unwrap();
        assert!(merged);
        assert_eq!(laptop.note(id).unwrap().body, "one, edited again\ntwo\nfrom the phone");
    }

    #[test]
    fn an_editor_saves_in_place_while_nothing_else_changes() {
        let mut library = Library::in_memory();
        let id = library.create_memo("one").unwrap();
        let base = library.edit_base();
        let (base, merged) = library.set_body_since(id, "one two", &base).unwrap();
        assert!(!merged);
        assert_eq!(base, library.edit_base());
        assert_eq!(library.note(id).unwrap().body, "one two");
    }

    #[test]
    fn a_fixed_clock_dates_every_change() {
        let mut library = Library::in_memory_at(123);
        let id = library.create_memo("Dated").unwrap();
        library.set_archived(id, true).unwrap();
        let note = library.note(id).unwrap();
        assert_eq!((note.created, note.modified, note.archived), (123, 123, Some(123)));
        assert!(Library::in_memory().now() > 123);
    }

    #[test]
    fn keeps_settings() {
        let mut library = Library::in_memory();
        assert_eq!(library.setting("linux.sidebar.outline"), None);
        library.set_setting("linux.sidebar.outline", 240).unwrap();
        assert_eq!(library.setting("linux.sidebar.outline"), Some(240));
        library.set_text_setting("linux.screen", "memos").unwrap();
        assert_eq!(library.text_setting("linux.screen").as_deref(), Some("memos"));
        assert_eq!(library.setting("linux.screen"), None);
    }

    #[test]
    fn creates_and_edits_memos() {
        let mut library = Library::in_memory();
        let id = library.create_memo("Pay kids $20 per mow").unwrap();
        let note = library.note(id).unwrap();
        assert_eq!(note.kind, MEMO);
        assert_eq!(note.label().as_deref(), Some("Pay kids $20 per mow"));
        library.set_body(id, "Pay kids $25 per mow").unwrap();
        assert_eq!(library.note(id).unwrap().body, "Pay kids $25 per mow");
    }

    #[test]
    fn trashes_restores_and_deletes() {
        let mut library = Library::in_memory();
        let id = library.create_memo("x").unwrap();
        assert!(matches!(library.delete(id), Err(Error::NotTrashed(_))));
        library.trash(id).unwrap();
        assert!(library.note(id).unwrap().is_trashed());
        library.restore(id).unwrap();
        assert!(!library.note(id).unwrap().is_trashed());
        assert_eq!(library.deleted_label(id), None);
        library.trash(id).unwrap();
        library.delete(id).unwrap();
        assert_eq!(library.note(id), None);
        assert!(library.notes().is_empty());
        assert_eq!(library.deleted_label(id).as_deref(), Some("x"));
    }

    #[test]
    fn archives_memos_without_modifying_them() {
        let mut library = Library::in_memory();
        let id = library.create_memo("x").unwrap();
        let modified = library.note(id).unwrap().modified;
        library.set_archived(id, true).unwrap();
        let archived = library.note(id).unwrap().archived;
        assert!(archived.is_some());
        library.set_archived(id, true).unwrap();
        assert_eq!(library.note(id).unwrap().archived, archived);
        assert_eq!(library.note(id).unwrap().modified, modified);
        library.set_archived(id, false).unwrap();
        assert_eq!(library.note(id).unwrap().archived, None);
        assert_eq!(library.note(id).unwrap().modified, modified);
    }

    #[test]
    fn moves_archived_from_version_2_actions_to_notes() {
        let mut library = Library::in_memory();
        let id = library.create_task("x", None).unwrap();
        let action = match library.note_map(id).unwrap().get(ACTION) {
            Some(ValueOrContainer::Container(Container::Map(action))) => action,
            _ => panic!("no action map"),
        };
        action.insert(ARCHIVED, 1_000).unwrap();
        library.doc.get_map("meta").insert("version", 2).unwrap();
        library.commit().unwrap();
        assert_eq!(library.note(id).unwrap().archived, None);
        library.prepare().unwrap();
        assert_eq!(library.note(id).unwrap().archived, Some(1_000));
        assert!(action.get(ARCHIVED).is_none());
        let meta = library.doc.get_map("meta");
        assert_eq!(meta.get("version").and_then(value).and_then(integer), Some(VERSION));
    }

    #[test]
    fn keeps_note_settings_without_modifying_notes() {
        let mut library = Library::in_memory();
        let id = library.create_memo("x").unwrap();
        let modified = library.note(id).unwrap().modified;
        assert_eq!(library.note_setting(id, "linux.height"), None);
        library.set_note_setting(id, "linux.height", 300).unwrap();
        assert_eq!(library.note_setting(id, "linux.height"), Some(300));
        assert_eq!(library.note(id).unwrap().modified, modified);
        assert_eq!(library.setting("linux.height"), None);
        library.trash(id).unwrap();
        library.delete(id).unwrap();
        assert_eq!(library.note_setting(id, "linux.height"), None);
        assert!(library.set_note_setting(id, "linux.height", 1).is_err());
    }

    #[test]
    fn removes_note_settings_without_modifying_notes() {
        let mut library = Library::in_memory();
        let id = library.create_memo("x").unwrap();
        let modified = library.note(id).unwrap().modified;
        library.remove_note_setting(id, "linux.height").unwrap();
        library.set_note_setting(id, "linux.height", 300).unwrap();
        library.set_note_setting(id, "linux.width", 200).unwrap();
        library.remove_note_setting(id, "linux.height").unwrap();
        assert_eq!(library.note_setting(id, "linux.height"), None);
        assert_eq!(library.note_setting(id, "linux.width"), Some(200));
        assert_eq!(library.note(id).unwrap().modified, modified);
    }

    #[test]
    fn searches_and_finds_backlinks() {
        let mut library = Library::in_memory();
        let target = library.create_memo("Lawn mowing rates").unwrap();
        let source = library.create_memo(&format!("See [[{target}]]")).unwrap();
        let other = library.create_memo("Groceries").unwrap();
        assert_eq!(ids(library.search("MOWING lawn")), [target]);
        assert_eq!(ids(library.backlinks(target)), [source]);
        library.trash(source).unwrap();
        assert!(library.backlinks(target).is_empty());
        assert_eq!(ids(library.search("")), [other, target]);
    }

    #[test]
    fn persists_across_opens() {
        let dir = tempfile::tempdir().unwrap();
        let id = {
            let mut library = Library::open(dir.path()).unwrap();
            let id = library.create_memo("first").unwrap();
            library.set_body(id, "first, edited").unwrap();
            id
        };
        let mut library = Library::open(dir.path()).unwrap();
        assert_eq!(library.note(id).unwrap().body, "first, edited");
        library.create_memo("second").unwrap();
        drop(library);
        assert_eq!(Library::open(dir.path()).unwrap().notes().len(), 2);
    }

    #[test]
    fn merges_concurrent_edits() {
        let a = Library::in_memory();
        let mut a = a;
        let id = a.create_memo("one two").unwrap();
        let b_doc = LoroDoc::new();
        b_doc.import(&a.doc.export(loro::ExportMode::Snapshot).unwrap()).unwrap();
        let mut b = Library { clock: None, commits: 0, doc: b_doc, read_only: false, store: None };
        a.set_body(id, "zero one two").unwrap();
        b.set_body(id, "one two three").unwrap();
        a.doc.import(&b.doc.export(loro::ExportMode::all_updates()).unwrap()).unwrap();
        assert_eq!(a.note(id).unwrap().body, "zero one two three");
    }

    #[test]
    fn opens_newer_libraries_read_only() {
        let doc = LoroDoc::new();
        let meta = doc.get_map("meta");
        meta.insert("format", FORMAT).unwrap();
        meta.insert("version", VERSION + 1).unwrap();
        doc.commit();
        let mut library = Library { clock: None, commits: 0, doc, read_only: false, store: None };
        library.prepare().unwrap();
        assert!(library.is_read_only());
        assert!(matches!(library.create_memo("x"), Err(Error::ReadOnly)));
    }

    fn ids(notes: Vec<Note>) -> Vec<NoteId> {
        notes.into_iter().map(|note| note.id).collect()
    }
}
