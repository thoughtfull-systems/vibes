// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! The platform-independent logic of Commonplace, shared by every implementation.

mod actions;
mod id;
mod journals;
mod label;
mod library;
mod links;
mod lists;
mod markup;
mod spaces;
mod store;
mod words;

use std::path::PathBuf;

pub use actions::{
    Action, ActionChange, ActionDates, ActionEntry, ActionList, ActionState, ActionStatus,
    ActionView, Date, Dated, ParseDateError,
};
pub use id::{NoteId, ParseNoteIdError};
pub use journals::{
    Answer, COLORS, ChoiceOption, Days, ENTRY_KIND, Entry, Handled, JOURNAL_KIND, Journal, Prompt,
    Question, QuestionKind, Schedule, Times, format_time, parse_loose_time, parse_time, prompt_key,
};
pub use label::label;
pub use library::{Library, MEMO, Note};
pub use links::{
    Link, LinkSyntax, ShownTitles, collapse_links, expand_links, label_separator, link_label,
    note_uri, parse_links, parse_note_uri, plain_links, wikilink,
};
pub use lists::{Edit, ListKey, list_edit};
pub use markup::{
    CodeBlock, Heading, Piece, Span, Style, code_blocks, inline_pieces, outline, spans,
    toggle_checkbox,
};
pub use spaces::Spaces;
pub use words::word_count;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("an action can't move inside itself")]
    CyclicMove(NoteId),
    #[error("the memo and the journal are in different libraries")]
    DifferentLibraries,
    #[error("an entry moves with its journal")]
    EntryMovesWithJournal(NoteId),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a device keeps at least one library")]
    LastLibrary,
    #[error("another program has the library in {0} open")]
    Locked(PathBuf),
    #[error("the library is corrupt: {0}")]
    Loro(String),
    #[error("note {0} is malformed")]
    Malformed(NoteId),
    #[error("note {0} isn't a journal")]
    NotAJournal(NoteId),
    #[error("note {0} isn't a memo")]
    NotAMemo(NoteId),
    #[error("not a Commonplace library: its format is {0:?}")]
    NotALibrary(String),
    #[error("there's no library {0}")]
    NoSuchLibrary(String),
    #[error("note {0} isn't an action")]
    NotAnAction(NoteId),
    #[error("note {0} doesn't exist")]
    NotFound(NoteId),
    #[error("note {0} isn't in the trash")]
    NotTrashed(NoteId),
    #[error("a question keeps its type; add a new question instead")]
    QuestionTypeChanged,
    #[error("a newer version of Commonplace wrote this library, so it's read-only")]
    ReadOnly,
    #[error("a library can't merge into itself")]
    SameLibrary,
}
