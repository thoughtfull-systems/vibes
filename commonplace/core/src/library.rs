// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use loro::{LoroDoc, LoroMap, LoroText, LoroValue, UpdateOptions, ValueOrContainer};

use crate::store::Store;
use crate::{Error, NoteId, label, parse_links};

const FORMAT: &str = "commonplace-library";
const VERSION: i64 = 1;

const BODY: &str = "body";
const CREATED: &str = "created";
const KIND: &str = "kind";
const MODIFIED: &str = "modified";
const TRASHED: &str = "trashed";

pub const MEMO: &str = "memo";

/// A copy of a note's contents at one moment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Note {
    pub body: String,
    pub created: i64,
    pub id: NoteId,
    pub kind: String,
    pub modified: i64,
    pub trashed: Option<i64>,
}

impl Note {
    pub fn label(&self) -> Option<String> {
        label(&self.body)
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
    doc: LoroDoc,
    read_only: bool,
    store: Option<Store>,
}

impl Library {
    /// Opens the library in `dir`, creating it if the directory holds none.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let doc = LoroDoc::new();
        let store = Store::open(dir, &doc)?;
        let mut library = Self { doc, read_only: false, store: Some(store) };
        library.prepare()?;
        Ok(library)
    }

    /// Creates an empty library that's never saved.
    pub fn in_memory() -> Self {
        let mut library = Self { doc: LoroDoc::new(), read_only: false, store: None };
        // Without a store, preparing a new library can't fail.
        library.prepare().unwrap();
        library
    }

    /// Writes the format and version, or opens read-only if a newer version wrote the library.
    fn prepare(&mut self) -> Result<(), Error> {
        let meta = self.doc.get_map("meta");
        let format = meta.get("format").and_then(value).and_then(string);
        let version = meta.get("version").and_then(value).and_then(integer);
        match (format, version) {
            (None, _) => {
                meta.insert("format", FORMAT).map_err(loro_error)?;
                meta.insert("version", VERSION).map_err(loro_error)?;
            }
            (Some(format), _) if format != FORMAT => return Err(Error::NotALibrary(format)),
            (Some(_), Some(version)) if version > VERSION => {
                self.read_only = true;
                return Ok(());
            }
            (Some(_), Some(VERSION)) => return Ok(()),
            (Some(_), _) => meta.insert("version", VERSION).map_err(loro_error)?,
        }
        self.commit()
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn create_memo(&mut self, body: &str) -> Result<NoteId, Error> {
        self.check_writable()?;
        let id = NoteId::generate();
        let now = now();
        let note = self
            .notes_map()
            .insert_container(&id.to_string(), LoroMap::new())
            .map_err(loro_error)?;
        let text = note.insert_container(BODY, LoroText::new()).map_err(loro_error)?;
        text.insert(0, body).map_err(loro_error)?;
        note.insert(CREATED, now).map_err(loro_error)?;
        note.insert(KIND, MEMO).map_err(loro_error)?;
        note.insert(MODIFIED, now).map_err(loro_error)?;
        self.commit()?;
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
        let text = match note.get(BODY) {
            Some(ValueOrContainer::Container(container)) => container.into_text().ok(),
            _ => None,
        }
        .ok_or(Error::Malformed(id))?;
        if text.to_string() == body {
            return Ok(());
        }
        text.update(body, UpdateOptions::default()).map_err(loro_error)?;
        note.insert(MODIFIED, now()).map_err(loro_error)?;
        self.commit()
    }

    pub fn trash(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        let now = now();
        note.insert(TRASHED, now).map_err(loro_error)?;
        note.insert(MODIFIED, now).map_err(loro_error)?;
        self.commit()
    }

    pub fn restore(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note_map(id)?;
        note.delete(TRASHED).map_err(loro_error)?;
        note.insert(MODIFIED, now()).map_err(loro_error)?;
        self.commit()
    }

    /// Deletes a note for good. The note must be in the trash.
    pub fn delete(&mut self, id: NoteId) -> Result<(), Error> {
        self.check_writable()?;
        let note = self.note(id).ok_or(Error::NotFound(id))?;
        if !note.is_trashed() {
            return Err(Error::NotTrashed(id));
        }
        self.notes_map().delete(&id.to_string()).map_err(loro_error)?;
        self.commit()
    }

    /// Notes outside the trash whose bodies contain every word of `query`, ignoring case.
    pub fn search(&self, query: &str) -> Vec<Note> {
        self.notes().into_iter().filter(|note| !note.is_trashed() && note.matches(query)).collect()
    }

    /// Notes outside the trash that link to `id`, other than the note itself.
    pub fn backlinks(&self, id: NoteId) -> Vec<Note> {
        self.notes()
            .into_iter()
            .filter(|note| note.id != id && !note.is_trashed())
            .filter(|note| parse_links(&note.body).iter().any(|link| link.note == Some(id)))
            .collect()
    }

    fn notes_map(&self) -> LoroMap {
        self.doc.get_map("notes")
    }

    fn note_map(&self, id: NoteId) -> Result<LoroMap, Error> {
        match self.notes_map().get(&id.to_string()) {
            Some(ValueOrContainer::Container(container)) => {
                container.into_map().map_err(|_| Error::Malformed(id))
            }
            Some(_) => Err(Error::Malformed(id)),
            None => Err(Error::NotFound(id)),
        }
    }

    fn check_writable(&self) -> Result<(), Error> {
        if self.read_only { Err(Error::ReadOnly) } else { Ok(()) }
    }

    fn commit(&mut self) -> Result<(), Error> {
        self.doc.commit();
        match &mut self.store {
            Some(store) => store.save(&self.doc),
            None => Ok(()),
        }
    }
}

fn note_from_value(id: NoteId, value: &LoroValue) -> Option<Note> {
    let LoroValue::Map(map) = value else {
        return None;
    };
    let created = map.get(CREATED).and_then(|v| integer(v.clone()))?;
    Some(Note {
        body: map.get(BODY).and_then(|v| string(v.clone())).unwrap_or_default(),
        created,
        id,
        kind: map.get(KIND).and_then(|v| string(v.clone())).unwrap_or_else(|| MEMO.to_owned()),
        modified: map.get(MODIFIED).and_then(|v| integer(v.clone())).unwrap_or(created),
        trashed: map.get(TRASHED).and_then(|v| integer(v.clone())),
    })
}

fn value(value: ValueOrContainer) -> Option<LoroValue> {
    match value {
        ValueOrContainer::Value(value) => Some(value),
        ValueOrContainer::Container(_) => None,
    }
}

fn string(value: LoroValue) -> Option<String> {
    match value {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

fn integer(value: LoroValue) -> Option<i64> {
    match value {
        LoroValue::I64(i) => Some(i),
        _ => None,
    }
}

fn now() -> i64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
}

fn loro_error(error: impl ToString) -> Error {
    Error::Loro(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        library.trash(id).unwrap();
        library.delete(id).unwrap();
        assert_eq!(library.note(id), None);
        assert!(library.notes().is_empty());
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
        let mut b = Library { doc: b_doc, read_only: false, store: None };
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
        let mut library = Library { doc, read_only: false, store: None };
        library.prepare().unwrap();
        assert!(library.is_read_only());
        assert!(matches!(library.create_memo("x"), Err(Error::ReadOnly)));
    }

    fn ids(notes: Vec<Note>) -> Vec<NoteId> {
        notes.into_iter().map(|note| note.id).collect()
    }
}
