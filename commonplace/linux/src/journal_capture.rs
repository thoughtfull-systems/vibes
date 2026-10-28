// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use commonplace_core::{Note, NoteId, parse_note_uri};

use crate::state::State;

/// The prompt a capture answers.
#[derive(Clone, Debug)]
pub struct PromptContext {
    pub journal: NoteId,
    pub key: String,
    /// When the prompt came due, as a timestamp.
    pub prompted: i64,
}

/// Finds the journal that `query` names: by its identifier, its URI, or its label, ignoring case,
/// if exactly one journal has that label.
pub fn find_journal(state: &State, query: &str) -> Result<NoteId, String> {
    let journals = state.read(|spaces| spaces.journals());
    let query = query.trim();
    let id = parse_note_uri(query).or_else(|| query.parse().ok());
    if let Some(id) = id.filter(|id| journals.iter().any(|journal| journal.id == *id)) {
        return Ok(id);
    }
    let named: Vec<&Note> = journals
        .iter()
        .filter(|journal| {
            journal.label().is_some_and(|label| label.to_lowercase() == query.to_lowercase())
        })
        .collect();
    match named[..] {
        [journal] => Ok(journal.id),
        [] => Err(format!("there's no journal named {query:?}")),
        _ => Err(format!("more than one journal is named {query:?}; give its URI instead")),
    }
}

/// The journal's label, or a placeholder for a journal with none.
pub fn journal_name(journal: &Note) -> String {
    journal.label().unwrap_or_else(|| "Untitled journal".to_owned())
}
