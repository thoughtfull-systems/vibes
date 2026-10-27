// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    Action, ActionChange, ActionDates, ActionView, Answer, Date, Days, Handled, Journal, ListKey,
    Note, NoteId, Question, Schedule, ShownTitles, Spaces, Style,
};

uniffi::custom_type!(Date, String, {
    lower: |date| date.to_string(),
    try_lift: |text| Ok(text.parse()?),
});
uniffi::custom_type!(Days, String, {
    lower: |days| days.rule(),
    try_lift: |text| {
        Days::parse(&text).ok_or_else(|| uniffi::deps::anyhow::anyhow!("invalid days"))
    },
});
uniffi::custom_type!(NoteId, String, {
    lower: |id| id.to_string(),
    try_lift: |text| Ok(text.parse()?),
});

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<crate::Error> for MobileError {
    fn from(error: crate::Error) -> Self {
        Self::Failed { reason: error.to_string() }
    }
}

type Result<T> = std::result::Result<T, MobileError>;

#[derive(uniffi::Record)]
pub struct CapturedPrompt {
    pub key: String,
    pub prompted: i64,
}

#[derive(Clone, uniffi::Record)]
pub struct MobileEntry {
    pub answers: HashMap<String, Answer>,
    pub at: i64,
    pub journal: NoteId,
    pub prompt: Option<String>,
    pub prompted: Option<i64>,
}

#[derive(Clone, uniffi::Record)]
pub struct MobileNote {
    pub action: Option<Action>,
    pub archived: Option<i64>,
    pub body: String,
    pub created: i64,
    pub entry: Option<MobileEntry>,
    pub id: NoteId,
    pub in_trash: bool,
    pub journal: Option<Journal>,
    pub kind: String,
    pub modified: i64,
    pub parent: Option<NoteId>,
    pub read_only: bool,
    pub space: String,
    pub title: String,
    pub trashed: Option<i64>,
}

#[derive(uniffi::Record)]
pub struct MobileAction {
    pub dates: ActionDates,
    pub depth: u32,
    pub note: MobileNote,
    pub parents: Vec<NoteId>,
    pub stalled: bool,
}

#[derive(uniffi::Record)]
pub struct MobileSpace {
    pub key: String,
    pub read_only: bool,
    pub shown: bool,
}

#[derive(uniffi::Record)]
pub struct MobileEdit {
    pub cursor: u32,
    pub end: u32,
    pub start: u32,
    pub text: String,
}

#[derive(uniffi::Record)]
pub struct MobileSpan {
    pub end: u32,
    pub start: u32,
    pub style: Style,
    pub syntax: Vec<MobileRange>,
}

#[derive(uniffi::Record)]
pub struct MobileMarkup {
    pub links: Vec<MobileLink>,
    pub spans: Vec<MobileSpan>,
}

#[derive(uniffi::Record)]
pub struct MobileRange {
    pub end: u32,
    pub start: u32,
}

#[derive(uniffi::Record)]
pub struct MobileHeading {
    pub level: u8,
    pub start: u32,
    pub text: String,
}

#[derive(uniffi::Record)]
pub struct MobileLink {
    pub end: u32,
    pub note: Option<NoteId>,
    pub start: u32,
    pub uri: Option<String>,
}

#[derive(uniffi::Record)]
pub struct ScheduledPrompt {
    pub handled: Option<Handled>,
    pub journal: NoteId,
    pub key: String,
    pub minute: u16,
}

#[derive(uniffi::Record)]
pub struct MobilePrompt {
    pub journal: NoteId,
    pub key: String,
    pub minute: u16,
    pub title: String,
}

struct State {
    editors: HashMap<NoteId, ShownTitles>,
    spaces: Spaces,
}

#[derive(uniffi::Object)]
pub struct MobileNotebook {
    state: Mutex<State>,
}

impl MobileNotebook {
    fn state(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| MobileError::Failed {
            reason: "the library stopped after an internal error; restart Commonplace".into(),
        })
    }
}

#[uniffi::export]
impl MobileNotebook {
    #[uniffi::constructor]
    pub fn open(directory: String) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            state: Mutex::new(State {
                editors: HashMap::new(),
                spaces: Spaces::open(Path::new(&directory))?,
            }),
        }))
    }

    pub fn spaces(&self) -> Result<Vec<MobileSpace>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .keys()
            .into_iter()
            .map(|key| MobileSpace {
                read_only: state.spaces.library(&key).is_some_and(|lib| lib.is_read_only()),
                shown: state.spaces.is_shown(&key),
                key,
            })
            .collect())
    }

    pub fn load_test_library(&self, today: Date) -> Result<HashMap<String, String>> {
        Ok(crate::samples::populate(&mut self.state()?.spaces, today)?.into_iter().collect())
    }

    pub fn create_space(&self) -> Result<String> {
        Ok(self.state()?.spaces.create()?)
    }

    pub fn remove_space(&self, key: String) -> Result<()> {
        Ok(self.state()?.spaces.remove(&key)?)
    }

    pub fn merge_spaces(&self, source: String, target: String) -> Result<()> {
        Ok(self.state()?.spaces.merge(&source, &target)?)
    }

    pub fn show_space(&self, key: String, shown: bool) -> Result<()> {
        self.state()?.spaces.set_shown(&key, shown);
        Ok(())
    }

    pub fn notes(&self, query: String) -> Result<Vec<MobileNote>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .notes()
            .into_iter()
            .filter(|note| note.matches(&query))
            .map(|note| mobile_note(&state.spaces, note))
            .collect())
    }

    pub fn note(&self, id: NoteId) -> Result<Option<MobileNote>> {
        let state = self.state()?;
        Ok(state.spaces.note(id).map(|note| mobile_note(&state.spaces, note)))
    }

    pub fn actions(&self, view: ActionView, today: Date) -> Result<Vec<MobileAction>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .action_view(view, today)
            .into_iter()
            .map(|entry| MobileAction {
                dates: entry.dates,
                depth: entry.depth as u32,
                note: mobile_note(&state.spaces, entry.note),
                parents: entry.parents,
                stalled: entry.stalled,
            })
            .collect())
    }

    pub fn action_steps(&self, id: NoteId, today: Date) -> Result<Vec<MobileAction>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .action_subtree(id, today)
            .into_iter()
            .map(|entry| MobileAction {
                dates: entry.dates,
                depth: entry.depth as u32,
                note: mobile_note(&state.spaces, entry.note),
                parents: entry.parents,
                stalled: entry.stalled,
            })
            .collect())
    }

    pub fn action_dates(&self, id: NoteId, today: Date) -> Result<ActionDates> {
        Ok(self.state()?.spaces.action_dates(today).remove(&id).unwrap_or_default())
    }

    pub fn create_memo(&self, space: String, body: String) -> Result<NoteId> {
        Ok(self.state()?.spaces.create_memo(&space, &body)?)
    }

    pub fn create_action(&self, space: String, body: String) -> Result<NoteId> {
        Ok(self.state()?.spaces.create_inbox_task(&space, &body)?)
    }

    pub fn create_step(&self, parent: NoteId, body: String) -> Result<NoteId> {
        Ok(self.state()?.spaces.create_task(&body, parent)?)
    }

    pub fn editor_body(&self, id: NoteId) -> Result<String> {
        let mut state = self.state()?;
        let note = state.spaces.note(id).ok_or(crate::Error::NotFound(id))?;
        let mut titles = ShownTitles::default();
        let body = titles.expand(&note.body, |id| state.spaces.link_title(id));
        state.editors.insert(id, titles);
        Ok(body)
    }

    pub fn link_text(&self, source: NoteId, target: NoteId) -> Result<String> {
        let mut state = self.state()?;
        let mut titles = state.editors.remove(&source).unwrap_or_default();
        let mut text = crate::wikilink(target, None);
        for (range, replacement) in titles.updates(&text, |id| state.spaces.link_title(id)) {
            text.replace_range(range, &replacement);
        }
        state.editors.insert(source, titles);
        Ok(text)
    }

    pub fn set_body(&self, id: NoteId, text: String) -> Result<()> {
        let mut state = self.state()?;
        state.editors.entry(id).or_default();
        let body = state.editors[&id].collapse(&text, |id| state.spaces.link_title(id));
        Ok(state.spaces.set_body(id, &body)?)
    }

    pub fn change_action(&self, id: NoteId, change: ActionChange) -> Result<()> {
        Ok(self.state()?.spaces.change_action(id, change)?)
    }

    pub fn move_action(
        &self,
        id: NoteId,
        parent: Option<NoteId>,
        index: Option<u32>,
    ) -> Result<()> {
        Ok(self.state()?.spaces.move_action(id, parent, index.map(|n| n as usize))?)
    }

    pub fn move_action_beside(&self, id: NoteId, sibling: NoteId, after: bool) -> Result<()> {
        Ok(self.state()?.spaces.move_action_beside(id, sibling, after)?)
    }

    pub fn reorder_action(&self, id: NoteId, up: bool) -> Result<()> {
        Ok(self.state()?.spaces.move_action_among_siblings(id, up)?)
    }

    pub fn move_to_space(&self, id: NoteId, space: String) -> Result<()> {
        Ok(self.state()?.spaces.move_to_space(id, &space)?)
    }

    pub fn set_archived(&self, id: NoteId, archived: bool) -> Result<()> {
        Ok(self.state()?.spaces.set_archived(id, archived)?)
    }

    pub fn trash(&self, id: NoteId) -> Result<()> {
        Ok(self.state()?.spaces.trash(id)?)
    }

    pub fn restore(&self, id: NoteId) -> Result<()> {
        Ok(self.state()?.spaces.restore(id)?)
    }

    pub fn delete(&self, id: NoteId) -> Result<()> {
        Ok(self.state()?.spaces.delete(id)?)
    }

    pub fn backlinks(&self, id: NoteId) -> Result<Vec<MobileNote>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .backlinks(id)
            .into_iter()
            .map(|note| mobile_note(&state.spaces, note))
            .collect())
    }

    pub fn create_journal(&self, space: String, name: String) -> Result<NoteId> {
        Ok(self.state()?.spaces.create_journal(&space, &name)?)
    }

    pub fn journals(&self) -> Result<Vec<MobileNote>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .journals()
            .into_iter()
            .map(|note| mobile_note(&state.spaces, note))
            .collect())
    }

    pub fn set_journal_color(&self, id: NoteId, color: String) -> Result<()> {
        Ok(self.state()?.spaces.set_journal_color(id, &color)?)
    }

    pub fn set_question(&self, id: NoteId, question: Question, new: bool) -> Result<()> {
        let mut state = self.state()?;
        if new {
            state.spaces.add_question(id, &question)?;
        } else {
            state.spaces.set_question(id, &question)?;
        }
        Ok(())
    }

    pub fn move_question(&self, id: NoteId, question: String, up: bool) -> Result<()> {
        Ok(self.state()?.spaces.move_question(id, &question, up)?)
    }

    pub fn set_schedule(&self, id: NoteId, schedule: Schedule) -> Result<()> {
        Ok(self.state()?.spaces.set_schedule(id, &schedule)?)
    }

    pub fn remove_schedule(&self, id: NoteId, schedule: String) -> Result<()> {
        Ok(self.state()?.spaces.remove_schedule(id, &schedule)?)
    }

    pub fn create_entry(
        &self,
        journal: NoteId,
        body: String,
        at: i64,
        prompt: Option<CapturedPrompt>,
        answers: HashMap<String, Answer>,
        today: Date,
    ) -> Result<NoteId> {
        let prompt = prompt.as_ref().map(|prompt| (prompt.key.as_str(), prompt.prompted));
        Ok(self.state()?.spaces.create_entry(
            journal,
            &body,
            at,
            prompt,
            &answers.into_iter().collect(),
            today,
        )?)
    }

    pub fn set_entry_at(&self, id: NoteId, at: i64) -> Result<()> {
        Ok(self.state()?.spaces.set_entry_at(id, at)?)
    }

    pub fn set_answer(&self, id: NoteId, question: String, answer: Option<Answer>) -> Result<()> {
        Ok(self.state()?.spaces.set_answer(id, &question, answer.as_ref())?)
    }

    pub fn convert_memo(&self, memo: NoteId, journal: NoteId) -> Result<NoteId> {
        Ok(self.state()?.spaces.convert_memo(memo, journal)?)
    }

    pub fn handle_prompt(
        &self,
        journal: NoteId,
        key: String,
        handled: Handled,
        today: Date,
    ) -> Result<()> {
        Ok(self.state()?.spaces.handle_prompt(journal, &key, handled, today)?)
    }

    pub fn scheduled_prompts(
        &self,
        journals: Vec<NoteId>,
        date: Date,
    ) -> Result<Vec<ScheduledPrompt>> {
        let state = self.state()?;
        let mut result = Vec::new();
        for note in state.spaces.all_journals() {
            if !journals.contains(&note.id) {
                continue;
            }
            let Some(journal) = note.journal else {
                continue;
            };
            for schedule in &journal.schedules {
                for minute in schedule.minutes_on(note.id, date) {
                    let key = crate::prompt_key(&schedule.id, date, minute);
                    result.push(ScheduledPrompt {
                        handled: journal.handled.get(&key).copied(),
                        journal: note.id,
                        key,
                        minute,
                    });
                }
            }
        }
        Ok(result)
    }

    pub fn due_prompts(
        &self,
        journals: Vec<NoteId>,
        today: Date,
        minute: u16,
        now: i64,
        local_times: Vec<i64>,
    ) -> Result<Vec<MobilePrompt>> {
        let state = self.state()?;
        Ok(state
            .spaces
            .all_journals()
            .into_iter()
            .filter(|note| journals.contains(&note.id))
            .filter_map(|note| {
                let prompt =
                    note.journal.as_ref()?.due_prompt(note.id, today, minute, now, |_, at| {
                        local_times.get(usize::from(at)).copied().unwrap_or(now)
                    })?;
                Some(MobilePrompt {
                    journal: note.id,
                    key: prompt.key,
                    minute: prompt.minute,
                    title: state.spaces.link_title(note.id).unwrap_or_default(),
                })
            })
            .collect())
    }

    pub fn action_brief(&self, id: NoteId) -> Result<String> {
        Ok(self.state()?.spaces.action_brief(id)?)
    }

    pub fn set_note_setting(&self, id: NoteId, key: String, value: i64) -> Result<()> {
        Ok(self.state()?.spaces.set_note_setting(id, &key, value)?)
    }

    pub fn note_setting(&self, id: NoteId, key: String) -> Result<Option<i64>> {
        Ok(self.state()?.spaces.note_setting(id, &key))
    }
}

fn mobile_note(spaces: &Spaces, note: Note) -> MobileNote {
    let space = spaces.space_of(note.id).unwrap_or_default().to_owned();
    MobileNote {
        action: note.action,
        archived: note.archived,
        body: note.body,
        created: note.created,
        entry: note.entry.map(|entry| MobileEntry {
            answers: entry.answers.into_iter().collect(),
            at: entry.at,
            journal: entry.journal,
            prompt: entry.prompt,
            prompted: entry.prompted,
        }),
        id: note.id,
        in_trash: spaces.is_in_trash(note.id),
        journal: note.journal,
        kind: note.kind,
        modified: note.modified,
        parent: spaces.library(&space).and_then(|lib| lib.action_parent(note.id)),
        read_only: spaces.library(&space).is_some_and(|lib| lib.is_read_only()),
        space,
        title: spaces.link_title(note.id).unwrap_or_default(),
        trashed: note.trashed,
    }
}

#[uniffi::export]
pub fn new_id() -> String {
    NoteId::generate().to_string()
}

#[uniffi::export]
pub fn note_uri(id: NoteId) -> String {
    crate::note_uri(id)
}

#[uniffi::export]
pub fn parse_note_uri(uri: String) -> Option<NoteId> {
    crate::parse_note_uri(&uri)
}

/// Reads `HH:MM` as minutes since midnight.
#[uniffi::export]
pub fn parse_time_of_day(text: String) -> Option<u16> {
    crate::parse_time(text)
}

#[uniffi::export]
pub fn schedule_is_valid(schedule: Schedule) -> bool {
    schedule.is_valid()
}

#[uniffi::export]
pub fn wikilink(id: NoteId) -> String {
    crate::wikilink(id, None)
}

fn utf16(body: &str, byte: usize) -> u32 {
    body[..byte].encode_utf16().count() as u32
}

/// The UTF-16 offset of each byte in a body, for converting many byte offsets in one pass.
struct Utf16(Vec<u32>);

impl Utf16 {
    fn new(body: &str) -> Self {
        let mut units = Vec::with_capacity(body.len() + 1);
        let mut count = 0;
        for ch in body.chars() {
            units.extend(std::iter::repeat_n(count, ch.len_utf8()));
            count += ch.len_utf16() as u32;
        }
        units.push(count);
        Self(units)
    }

    fn at(&self, byte: usize) -> u32 {
        self.0[byte]
    }
}

fn byte_offset(body: &str, offset: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in body.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16() as u32;
    }
    (units == offset).then_some(body.len())
}

#[uniffi::export]
pub fn list_edit(body: String, cursor: u32, key: ListKey) -> Option<MobileEdit> {
    let edit = crate::list_edit(&body, byte_offset(&body, cursor)?, key)?;
    let mut after = body.clone();
    after.replace_range(edit.range.clone(), &edit.text);
    Some(MobileEdit {
        cursor: utf16(&after, edit.cursor),
        end: utf16(&body, edit.range.end),
        start: utf16(&body, edit.range.start),
        text: edit.text,
    })
}

#[uniffi::export]
pub fn toggle_checkbox(body: String, offset: u32) -> Option<MobileEdit> {
    let (range, text) = crate::toggle_checkbox(&body, byte_offset(&body, offset)?)?;
    Some(MobileEdit {
        cursor: offset,
        end: utf16(&body, range.end),
        start: utf16(&body, range.start),
        text: text.to_owned(),
    })
}

#[uniffi::export]
pub fn spans(body: String) -> Vec<MobileSpan> {
    let units = Utf16::new(&body);
    crate::spans(&body).into_iter().map(|span| mobile_span(&units, span)).collect()
}

/// An editor's styled spans and its links, from one scan of `body`.
#[uniffi::export]
pub fn editor_markup(body: String) -> MobileMarkup {
    let units = Utf16::new(&body);
    let (spans, links) = crate::spans_and_links(&body);
    MobileMarkup {
        links: links
            .into_iter()
            .map(|link| MobileLink {
                end: units.at(link.range.end),
                note: link.note,
                start: units.at(link.range.start),
                uri: link.external_uri().map(str::to_owned),
            })
            .collect(),
        spans: spans.into_iter().map(|span| mobile_span(&units, span)).collect(),
    }
}

fn mobile_span(units: &Utf16, span: crate::Span) -> MobileSpan {
    MobileSpan {
        end: units.at(span.range.end),
        start: units.at(span.range.start),
        style: span.style,
        syntax: span
            .syntax
            .into_iter()
            .map(|range| MobileRange { end: units.at(range.end), start: units.at(range.start) })
            .collect(),
    }
}

#[uniffi::export]
pub fn inline_pieces(body: String) -> Vec<crate::Piece> {
    crate::inline_pieces(&body)
}

#[uniffi::export]
pub fn outline(body: String) -> Vec<MobileHeading> {
    let units = Utf16::new(&body);
    crate::outline(&body)
        .into_iter()
        .map(|heading| MobileHeading {
            level: heading.level,
            start: units.at(heading.range.start),
            text: heading.text,
        })
        .collect()
}

#[uniffi::export]
pub fn word_count(body: String) -> u64 {
    crate::word_count(&body) as u64
}

#[cfg(test)]
mod tests;
