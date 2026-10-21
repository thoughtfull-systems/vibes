// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! The platform-independent logic of Commonplace, shared by every implementation.

mod actions;
mod id;
mod label;
mod library;
mod links;
mod lists;
mod markup;
mod store;
mod words;

use std::path::PathBuf;

pub use actions::{
    Action, ActionChange, ActionDates, ActionEntry, ActionList, ActionState, ActionStatus,
    ActionView, Date, Dated, ParseDateError,
};
pub use id::{NoteId, ParseNoteIdError};
pub use label::label;
pub use library::{Library, MEMO, Note};
pub use links::{Link, LinkSyntax, note_uri, parse_links, parse_note_uri, wikilink};
pub use lists::{Edit, ListKey, list_edit};
pub use markup::{
    CodeBlock, Heading, Piece, Span, Style, code_blocks, inline_pieces, outline, spans,
    toggle_checkbox,
};
pub use words::word_count;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("an action can't move inside itself")]
    CyclicMove(NoteId),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("another program has the library in {0} open")]
    Locked(PathBuf),
    #[error("the library is corrupt: {0}")]
    Loro(String),
    #[error("note {0} is malformed")]
    Malformed(NoteId),
    #[error("not a Commonplace library: its format is {0:?}")]
    NotALibrary(String),
    #[error("note {0} isn't an action")]
    NotAnAction(NoteId),
    #[error("note {0} doesn't exist")]
    NotFound(NoteId),
    #[error("note {0} isn't in the trash")]
    NotTrashed(NoteId),
    #[error("a newer version of Commonplace wrote this library, so it's read-only")]
    ReadOnly,
}
