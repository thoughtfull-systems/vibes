// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Actions and their hierarchy, as version 3 of the library format spec defines them.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use loro::{LoroMap, LoroTree, LoroValue, TreeID, TreeParentId, ValueOrContainer};

use crate::library::{ACTION_KIND, MODIFIED, integer, loro_error, string, value};
use crate::markup::open_fence;
use crate::recurring::{OCCURRENCES, RECUR, Recurrence, StepOccurrence, step_occurrences};
use crate::{Error, Library, Note, NoteId};

pub(crate) const ACTION: &str = "action";
const ACTIONS: &str = "actions";
const CLOSED: &str = "closed";
const DEFER: &str = "defer";
const DUE: &str = "due";
const LIST: &str = "list";
const NOTE: &str = "note";
const PLANNING: &str = "planning";
const PROJECT: &str = "project";
const STATE: &str = "state";
const WAITING_FOR: &str = "waiting_for";

/// A calendar day, with no time of day and no time zone.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    pub const EPOCH: Self = Self { year: 1970, month: 1, day: 1 };

    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days).contains(&day).then_some(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// How many days `month` of `year` has, or 0 if `month` isn't a month.
    pub fn days_in_month(year: u16, month: u8) -> u8 {
        (28..=31).rev().find(|&day| Self::new(year, month, day).is_some()).unwrap_or(0)
    }

    /// The day of the week, from 0 for Monday to 6 for Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday.
        (self.days() + 3).rem_euclid(7) as u8
    }
}

impl Date {
    /// How many days after `earlier` this date is, or before it if negative.
    pub fn days_since(self, earlier: Self) -> i32 {
        (self.days() - earlier.days()) as i32
    }

    /// The date `days` days later, or earlier if `days` is negative.
    pub fn add_days(self, days: i32) -> Self {
        Self::from_days(self.days() + i64::from(days))
    }

    /// Days since 1970-01-01, by Howard Hinnant's algorithm for the proleptic Gregorian calendar.
    pub(crate) fn days(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month = i64::from(self.month);
        let day_of_year =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    pub(crate) fn from_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u8;
        let month = (if month_index < 10 { month_index + 3 } else { month_index - 9 }) as u8;
        let year = (year_of_era + era * 400 + i64::from(month <= 2)) as u16;
        Self { year, month, day }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("not a date in the form YYYY-MM-DD: {0:?}")]
pub struct ParseDateError(pub String);

impl FromStr for Date {
    type Err = ParseDateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseDateError(s.to_owned());
        let bytes = s.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(error());
        }
        let number = |range: std::ops::Range<usize>| {
            let digits = &s[range];
            digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse::<u16>().ok()).flatten()
        };
        let (year, month, day) = (number(0..4), number(5..7), number(8..10));
        let (Some(year), Some(month), Some(day)) = (year, month, day) else {
            return Err(error());
        };
        Date::new(
            year,
            u8::try_from(month).map_err(|_| error())?,
            u8::try_from(day).map_err(|_| error())?,
        )
        .ok_or_else(error)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum ActionState {
    Done,
    #[default]
    Open,
}

impl ActionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Open => "open",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "done" => Self::Done,
            _ => Self::Open,
        }
    }
}

/// The Getting Things Done list an action sits on.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum ActionList {
    /// Being worked on: a task to do now, or a project in progress.
    #[default]
    Active,
    /// Captured, and not yet sorted onto another list.
    Inbox,
    /// Not sure it should be done at all.
    Maybe,
    /// To be done, but not yet decided when.
    Someday,
    Waiting,
}

impl ActionList {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inbox => "inbox",
            Self::Maybe => "maybe",
            Self::Someday => "someday",
            Self::Waiting => "waiting",
        }
    }

    /// Whether the list holds an action back from being done now: inbox, someday, or maybe.
    pub fn holds_back(self) -> bool {
        matches!(self, Self::Inbox | Self::Maybe | Self::Someday)
    }

    fn parse(text: &str) -> Self {
        match text {
            "inbox" => Self::Inbox,
            "maybe" => Self::Maybe,
            "someday" => Self::Someday,
            "waiting" => Self::Waiting,
            _ => Self::Active,
        }
    }

    /// Of an action's own list and the list its ancestors put it on, the one that applies.
    fn inherit(self, own: Self) -> Self {
        let rank = |list| match list {
            Self::Inbox => 4,
            Self::Maybe => 3,
            Self::Someday => 2,
            Self::Waiting => 1,
            Self::Active => 0,
        };
        if rank(own) > rank(self) { own } else { self }
    }
}

/// An action's fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Action {
    pub closed: Option<i64>,
    pub defer: Option<Date>,
    pub due: Option<Date>,
    pub list: ActionList,
    /// When the action joined the planning queue, if it's in it.
    pub planning: Option<i64>,
    pub project: bool,
    pub state: ActionState,
    pub waiting_for: Option<String>,
    /// The rule and record of a recurring action.
    pub recur: Option<Recurrence>,
    /// For a step of a recurring action, the occurrences in which it's done.
    pub occurrences: Vec<StepOccurrence>,
}

impl Action {
    pub(crate) fn from_value(value: Option<&LoroValue>) -> Self {
        let Some(LoroValue::Map(map)) = value else {
            return Self::default();
        };
        let text = |key| map.get(key).and_then(|v| string(v.clone()));
        Self {
            closed: map.get(CLOSED).and_then(|v| integer(v.clone())),
            defer: text(DEFER).and_then(|t| t.parse().ok()),
            due: text(DUE).and_then(|t| t.parse().ok()),
            list: text(LIST).map_or_else(ActionList::default, |t| ActionList::parse(&t)),
            planning: map.get(PLANNING).and_then(|v| integer(v.clone())),
            project: matches!(map.get(PROJECT), Some(LoroValue::Bool(true))),
            state: text(STATE).map_or_else(ActionState::default, |t| ActionState::parse(&t)),
            waiting_for: text(WAITING_FOR).filter(|t| !t.trim().is_empty()),
            recur: Recurrence::from_value(map.get(RECUR)),
            occurrences: step_occurrences(map.get(OCCURRENCES)),
        }
    }

    pub fn is_open(&self) -> bool {
        self.state == ActionState::Open
    }
}

/// A change to one of an action's fields.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum ActionChange {
    Defer(Option<Date>),
    Due(Option<Date>),
    List(ActionList),
    /// Adds the action to the planning queue, or takes it out.
    Planning(bool),
    Project(bool),
    State(ActionState),
    WaitingFor(Option<String>),
}

/// A view of actions for a Getting Things Done review.
///
/// A project is put off when it's on the inbox, someday, or maybe list, or starts after today. It
/// then stands for the actions inside it, which the views leave out.
///
/// Every view but `Closed` filters its actions first, then orders them the same way: overdue, then
/// due within a week, both soonest due first; then started or undated, with those that have a due
/// date first; then starting within a week, and last starting later, both soonest start first. Ties
/// go to the action that comes first in the outline. The views that show a hierarchy order each set
/// of siblings this way, ranking an action by the most pressing of itself and the actions under it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum ActionView {
    /// Every action on any list, as a hierarchy, except those that are done, archived, or in the
    /// trash, themselves or through an action above them.
    All,
    /// Done actions outside the archive, most recently closed first.
    Closed,
    /// Open actions on the inbox list that have started, outside any put-off project.
    Inbox,
    /// Open actions on the maybe list, outside any put-off project.
    Maybe,
    /// Open tasks on the active list that start within a week and aren't in the planning queue,
    /// with only the first in order of each top-level project's steps, and without the starred
    /// tasks, which `Starred` shows.
    Next,
    /// Open actions in the planning queue, the longest queued first.
    Planning,
    /// Open projects on any list, outside any put-off project.
    Projects,
    /// Open actions with a start date of their own after today, outside any put-off project.
    Scheduled,
    /// Open actions on the someday list, outside any put-off project.
    Someday,
    /// Open starred actions, on any list, whatever their start date.
    Starred,
    /// Open projects that have started, aren't put off to someday or maybe, sit outside any
    /// put-off project, and have no open task that isn't put off either.
    Stalled,
    /// Open actions on the waiting list, outside any put-off project.
    Waiting,
}

/// How many days ahead an action counts as coming up or due soon.
const SOON: i32 = 7;

/// How pressing an open action is, from the start and due dates that apply to it, its own or a
/// project's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum ActionStatus {
    /// Due within a week, or today.
    DueSoon,
    #[default]
    Normal,
    /// Past its due date.
    Overdue,
    /// Not started yet: its start date is after today.
    Upcoming,
}

/// A date that applies to an action, and the action it belongs to: the action itself, or a project
/// that contains it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Dated {
    pub date: Date,
    pub from: NoteId,
}

/// The start and due dates that apply to an action, its own or a project's, and how pressing they
/// make it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct ActionDates {
    /// The earliest due date of the action and the projects that contain it.
    pub due: Option<Dated>,
    /// The latest start date of the action and the projects that contain it.
    pub start: Option<Dated>,
    pub status: ActionStatus,
}

/// An action in a view.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionEntry {
    pub dates: ActionDates,
    /// How many of the action's ancestors the view also shows.
    pub depth: usize,
    pub note: Note,
    /// The actions the action belongs to, outermost first.
    pub parents: Vec<NoteId>,
    /// For a project, whether it has no open tasks.
    pub stalled: bool,
    /// For a recurring action that keeps every occurrence, how many more occurrences have fallen
    /// due behind the current one.
    pub waiting: u32,
}

/// The hierarchy of actions, after resolving the cases the spec describes.
#[derive(Default)]
pub(crate) struct Hierarchy {
    children: HashMap<Option<NoteId>, Vec<NoteId>>,
    nodes: HashMap<NoteId, TreeID>,
    parents: HashMap<NoteId, Option<NoteId>>,
}

impl Hierarchy {
    fn children(&self, parent: Option<NoteId>) -> &[NoteId] {
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn parent(&self, id: NoteId) -> Option<NoteId> {
        self.parents.get(&id).copied().flatten()
    }

    pub(crate) fn ancestors(&self, id: NoteId) -> impl Iterator<Item = NoteId> + '_ {
        let mut seen = HashSet::from([id]);
        std::iter::successors(self.parent(id), move |&ancestor| self.parent(ancestor))
            .take_while(move |&ancestor| seen.insert(ancestor))
    }

    /// The action's ancestors, outermost first.
    fn path(&self, id: NoteId) -> Vec<NoteId> {
        let mut path: Vec<NoteId> = self.ancestors(id).collect();
        path.reverse();
        path
    }

    pub(crate) fn descendants(&self, id: NoteId) -> Vec<NoteId> {
        let mut found = Vec::new();
        let mut stack: Vec<NoteId> = self.children(Some(id)).iter().rev().copied().collect();
        let mut seen = HashSet::from([id]);
        while let Some(next) = stack.pop() {
            if seen.insert(next) {
                found.push(next);
                stack.extend(self.children(Some(next)).iter().rev());
            }
        }
        found
    }
}

impl Library {
    /// Creates an open task, an action that isn't a project, on the active list, as the last child
    /// of `parent`, or at the top level.
    /// The first line of `body` is its title.
    pub fn create_task(&mut self, body: &str, parent: Option<NoteId>) -> Result<NoteId, Error> {
        self.insert_action(body, parent, ActionList::Active)
    }

    /// Creates an open, top-level task on the inbox list, waiting to be sorted onto another list.
    pub fn create_inbox_task(&mut self, body: &str) -> Result<NoteId, Error> {
        self.insert_action(body, None, ActionList::Inbox)
    }

    fn insert_action(
        &mut self,
        body: &str,
        parent: Option<NoteId>,
        list: ActionList,
    ) -> Result<NoteId, Error> {
        self.check_writable()?;
        let id = self.insert_note(body, ACTION_KIND)?;
        insert_action_map(&self.note_map(id)?, list)?;
        self.place_node(id, parent)?;
        self.commit()?;
        Ok(id)
    }

    pub fn change_action(&mut self, id: NoteId, change: ActionChange) -> Result<(), Error> {
        self.check_writable()?;
        let map = self.note_map(id)?;
        let action = match map.get(ACTION) {
            Some(ValueOrContainer::Container(container)) => container.into_map().ok(),
            _ => None,
        }
        .ok_or(Error::NotAnAction(id))?;
        let optional = |key: &str, text: Option<String>| match text {
            Some(text) => action.insert(key, text).map(|_| ()),
            None => action.delete(key),
        };
        match change {
            ActionChange::Defer(date) => optional(DEFER, date.map(|d| d.to_string())),
            ActionChange::Due(date) => optional(DUE, date.map(|d| d.to_string())),
            ActionChange::List(list) => action.insert(LIST, list.as_str()),
            ActionChange::Planning(true) if action.get(PLANNING).is_some() => Ok(()),
            ActionChange::Planning(true) => action.insert(PLANNING, self.now()),
            ActionChange::Planning(false) => action.delete(PLANNING),
            ActionChange::Project(project) => action.insert(PROJECT, project),
            ActionChange::State(state) => {
                action.insert(STATE, state.as_str()).map_err(loro_error)?;
                match state {
                    ActionState::Open => action.delete(CLOSED),
                    ActionState::Done => action.insert(CLOSED, self.now()),
                }
            }
            ActionChange::WaitingFor(text) => {
                optional(WAITING_FOR, text.filter(|text| !text.trim().is_empty()))
            }
        }
        .map_err(loro_error)?;
        map.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// Moves an action under `parent`, or to the top level, at `index` among its new siblings, or
    /// last. Fails if `parent` is the action or one of its descendants.
    pub fn move_action(
        &mut self,
        id: NoteId,
        parent: Option<NoteId>,
        index: Option<usize>,
    ) -> Result<(), Error> {
        self.check_writable()?;
        let hierarchy = self.hierarchy();
        if let Some(parent) = parent
            && (parent == id || hierarchy.ancestors(parent).any(|ancestor| ancestor == id))
        {
            return Err(Error::CyclicMove(id));
        }
        let siblings: Vec<NoteId> =
            hierarchy.children(parent).iter().copied().filter(|&sibling| sibling != id).collect();
        let index = index.unwrap_or(siblings.len()).min(siblings.len());
        let tree = self.actions_tree();
        let node = match hierarchy.nodes.get(&id) {
            Some(&node) => node,
            None => self.place_node(id, None)?,
        };
        let parent_node = match parent {
            Some(parent) => Some(match hierarchy.nodes.get(&parent) {
                Some(&node) => node,
                None => self.place_node(parent, None)?,
            }),
            None => None,
        };
        match siblings.get(index).and_then(|sibling| hierarchy.nodes.get(sibling)) {
            Some(&before) => tree.mov_before(node, before),
            None => tree.mov(node, TreeParentId::from(parent_node)),
        }
        .map_err(loro_error)?;
        if let Some(parent) = parent {
            self.promote(parent)?;
        }
        self.note_map(id)?.insert(MODIFIED, self.now()).map_err(loro_error)?;
        self.commit()
    }

    /// Moves an action next to `sibling`, under the same parent: just before it, or just after it
    /// if `after` is set.
    pub fn move_action_beside(
        &mut self,
        id: NoteId,
        sibling: NoteId,
        after: bool,
    ) -> Result<(), Error> {
        let hierarchy = self.hierarchy();
        let parent = hierarchy.parent(sibling);
        let position = hierarchy
            .children(parent)
            .iter()
            .filter(|&&other| other != id)
            .position(|&other| other == sibling)
            .ok_or(Error::NotFound(sibling))?;
        self.move_action(id, parent, Some(position + usize::from(after)))
    }

    /// Moves an action up among its siblings if `up` is set, or down otherwise. Does nothing for
    /// the first or last sibling.
    pub fn move_action_among_siblings(&mut self, id: NoteId, up: bool) -> Result<(), Error> {
        let hierarchy = self.hierarchy();
        let parent = hierarchy.parent(id);
        let siblings = hierarchy.children(parent);
        let Some(position) = siblings.iter().position(|&other| other == id) else {
            return Err(Error::NotFound(id));
        };
        let target = if up { position.checked_sub(1) } else { Some(position + 1) };
        match target.filter(|&target| target < siblings.len()) {
            Some(target) => self.move_action(id, parent, Some(target)),
            None => Ok(()),
        }
    }

    /// The action's parent, if it has one.
    pub fn action_parent(&self, id: NoteId) -> Option<NoteId> {
        self.hierarchy().parent(id)
    }

    /// The action's steps, in order, including those that are closed or in the trash.
    pub fn action_children(&self, id: NoteId) -> Vec<Note> {
        let hierarchy = self.hierarchy();
        hierarchy.children(Some(id)).iter().filter_map(|&child| self.note(child)).collect()
    }

    /// The steps under an action, each followed by its own steps, in outline order, so the order
    /// the owner arranges them in is the order they show. Steps in the trash are left out.
    pub fn action_subtree(&self, id: NoteId, today: Date) -> Vec<ActionEntry> {
        let mut notes = self.action_notes();
        let standings = self.resolve_recurrence(&mut notes, today);
        let hierarchy = self.hierarchy();
        let all_dates: HashMap<NoteId, ActionDates> = walk(&hierarchy, &notes, today)
            .into_iter()
            .map(|entry| (entry.note.id, entry.dates))
            .collect();
        let mut entries = Vec::new();
        let mut seen = HashSet::from([id]);
        let mut stack: Vec<(NoteId, usize)> =
            hierarchy.children(Some(id)).iter().rev().map(|&child| (child, 0)).collect();
        while let Some((child, depth)) = stack.pop() {
            let Some(note) = notes.get(&child).filter(|note| !note.is_trashed()).cloned() else {
                continue;
            };
            if !seen.insert(child) {
                continue;
            }
            let dates = all_dates.get(&child).copied().unwrap_or_default();
            let parents = hierarchy.path(child);
            let waiting = standings.get(&child).map_or(0, |standing| standing.waiting);
            entries.push(ActionEntry { dates, depth, note, parents, stalled: false, waiting });
            stack.extend(
                hierarchy.children(Some(child)).iter().rev().map(|&step| (step, depth + 1)),
            );
        }
        entries
    }

    /// Every note in the trash, itself or through an action above it or the journal it belongs
    /// to.
    pub fn notes_in_trash(&self) -> HashSet<NoteId> {
        let notes = self.notes();
        let trashed: HashSet<NoteId> =
            notes.iter().filter(|note| note.is_trashed()).map(|note| note.id).collect();
        let hierarchy = self.hierarchy();
        notes
            .iter()
            .filter(|note| {
                let journal = note.entry.as_ref().map(|entry| entry.journal);
                std::iter::once(note.id)
                    .chain(hierarchy.ancestors(note.id))
                    .chain(journal)
                    .any(|id| trashed.contains(&id))
            })
            .map(|note| note.id)
            .collect()
    }

    /// The dates that apply to each action, by its identifier.
    pub fn action_dates(&self, today: Date) -> HashMap<NoteId, ActionDates> {
        let mut notes = self.action_notes();
        self.resolve_recurrence(&mut notes, today);
        walk(&self.hierarchy(), &notes, today)
            .into_iter()
            .map(|entry| (entry.note.id, entry.dates))
            .collect()
    }

    /// The actions that each action belongs to, outermost first, by the action's identifier.
    pub fn action_paths(&self) -> HashMap<NoteId, Vec<NoteId>> {
        let hierarchy = self.hierarchy();
        hierarchy.parents.keys().map(|&id| (id, hierarchy.path(id))).collect()
    }

    /// The identifiers of the action's steps, their steps, and so on.
    pub fn action_descendants(&self, id: NoteId) -> Vec<NoteId> {
        self.hierarchy().descendants(id)
    }

    /// The bodies of the action's projects, outermost first, each in a `<project>` element, then
    /// the action's own body in an `<action>` element, to hand the action to someone along with
    /// what it's part of. Projects with empty bodies are left out.
    pub fn action_brief(&self, id: NoteId) -> Result<String, Error> {
        let note = self.note(id).ok_or(Error::NotFound(id))?;
        if note.action.is_none() {
            return Err(Error::NotAnAction(id));
        }
        let projects =
            self.hierarchy().path(id).into_iter().filter_map(|ancestor| self.note(ancestor));
        let mut elements: Vec<String> =
            projects.filter_map(|project| brief_element("project", &project.body)).collect();
        let action = brief_element("action", &note.body);
        elements.push(action.unwrap_or_else(|| "<action>\n</action>".to_owned()));
        Ok(format!("{}\n", elements.join("\n\n")))
    }

    /// Whether the note is in the trash, itself or through an action or a journal it belongs to.
    pub fn is_in_trash(&self, id: NoteId) -> bool {
        let hierarchy = self.hierarchy();
        std::iter::once(id)
            .chain(hierarchy.ancestors(id))
            .chain(self.entry_journal(id))
            .any(|note| self.note(note).is_some_and(|note| note.is_trashed()))
    }

    /// The actions a view shows, in its order. `today` decides which actions have started and which
    /// are due soon.
    pub fn action_view(&self, view: ActionView, today: Date) -> Vec<ActionEntry> {
        let mut notes = self.action_notes();
        let standings = self.resolve_recurrence(&mut notes, today);
        let hierarchy = self.hierarchy();
        let mut entries = walk(&hierarchy, &notes, today);
        // Tasks that keep a project moving: open, and not put off to someday or maybe.
        let actions: HashSet<NoteId> = entries
            .iter()
            .filter(|entry| {
                !entry.action.project && entry.context.active() && !entry.context.list.holds_back()
            })
            .map(|entry| entry.note.id)
            .collect();
        let stalled: HashSet<NoteId> = entries
            .iter()
            .filter(|entry| {
                entry.action.project
                    && entry.context.active()
                    && !matches!(entry.context.list, ActionList::Maybe | ActionList::Someday)
                    && !entry.context.inside_put_off(today)
                    && !entry.context.delayed(today)
            })
            .filter(|project| {
                !hierarchy.descendants(project.note.id).iter().any(|id| actions.contains(id))
            })
            .map(|entry| entry.note.id)
            .collect();
        let result = |entries: Vec<Walked>, depths: bool| {
            entries
                .into_iter()
                .map(|entry| ActionEntry {
                    dates: entry.dates,
                    depth: if depths { entry.depth } else { 0 },
                    parents: hierarchy.path(entry.note.id),
                    stalled: stalled.contains(&entry.note.id),
                    waiting: standings.get(&entry.note.id).map_or(0, |standing| standing.waiting),
                    note: entry.note,
                })
                .collect()
        };
        // The walk visits actions in outline order.
        let outline: HashMap<NoteId, usize> =
            entries.iter().enumerate().map(|(i, entry)| (entry.note.id, i)).collect();
        let key = |entry: &Walked| (order_key(&entry.dates, today), outline[&entry.note.id]);
        let flat = |mut entries: Vec<Walked>| {
            entries.sort_by_key(key);
            result(entries, false)
        };
        let nested = |entries: Vec<Walked>| {
            result(order_as_outline(entries, &hierarchy, &outline, today), true)
        };
        match view {
            ActionView::Closed => {
                entries.retain(|entry| {
                    !entry.context.trashed && !entry.context.archived && !entry.action.is_open()
                });
                entries.sort_by_key(|entry| std::cmp::Reverse(entry.action.closed));
                result(entries, false)
            }
            ActionView::Inbox => {
                entries.retain(|entry| {
                    entry.context.active()
                        && entry.action.list == ActionList::Inbox
                        && !entry.context.inside_put_off(today)
                        && !entry.context.delayed(today)
                });
                flat(entries)
            }
            ActionView::Maybe | ActionView::Someday => {
                let list =
                    if view == ActionView::Maybe { ActionList::Maybe } else { ActionList::Someday };
                entries.retain(|entry| {
                    entry.context.active()
                        && entry.action.list == list
                        && !entry.context.inside_put_off(today)
                });
                flat(entries)
            }
            ActionView::Next => {
                entries.retain(|entry| {
                    entry.context.active()
                        && !entry.action.project
                        && entry.action.planning.is_none()
                        && entry.context.list == ActionList::Active
                        && entry.context.start.is_none_or(|s| s.date <= today.add_days(SOON))
                });
                // Each top-level project shows one step, the first in the views' order.
                let is_project = |id: &NoteId| {
                    notes.get(id).and_then(|note| note.action.as_ref()).is_some_and(|t| t.project)
                };
                let mut chosen = HashMap::new();
                for entry in &entries {
                    let Some(project) =
                        hierarchy.ancestors(entry.note.id).filter(is_project).last()
                    else {
                        continue;
                    };
                    let candidate = (key(entry), entry.note.id);
                    let best = chosen.entry(project).or_insert(candidate);
                    if candidate < *best {
                        *best = candidate;
                    }
                }
                let keep: HashSet<NoteId> = chosen.into_values().map(|(_, id)| id).collect();
                entries.retain(|entry| {
                    entry.note.starred.is_none()
                        && (keep.contains(&entry.note.id)
                            || !hierarchy.ancestors(entry.note.id).any(|id| is_project(&id)))
                });
                flat(entries)
            }
            ActionView::Projects => {
                entries.retain(|entry| {
                    entry.action.project
                        && entry.context.active()
                        && !entry.context.inside_put_off(today)
                });
                flat(entries)
            }
            ActionView::Planning => {
                entries.retain(|entry| entry.context.active() && entry.action.planning.is_some());
                entries.sort_by_key(|entry| (entry.action.planning, outline[&entry.note.id]));
                result(entries, false)
            }
            ActionView::Starred => {
                entries.retain(|entry| entry.context.active() && entry.note.starred.is_some());
                flat(entries)
            }
            ActionView::Stalled => {
                entries.retain(|entry| stalled.contains(&entry.note.id));
                flat(entries)
            }
            ActionView::Scheduled => {
                entries.retain(|entry| {
                    entry.context.active()
                        && entry.action.defer.is_some_and(|defer| defer > today)
                        && !entry.context.inside_put_off(today)
                });
                flat(entries)
            }
            ActionView::All => {
                entries.retain(|entry| entry.context.active());
                nested(entries)
            }
            ActionView::Waiting => {
                entries.retain(|entry| {
                    entry.context.active()
                        && entry.action.list == ActionList::Waiting
                        && !entry.context.inside_put_off(today)
                });
                flat(entries)
            }
        }
    }

    /// Every action, by its identifier.
    fn action_notes(&self) -> HashMap<NoteId, Note> {
        self.notes()
            .into_iter()
            .filter(|note| note.action.is_some())
            .map(|note| (note.id, note))
            .collect()
    }

    /// Deletes the nodes and notes of actions and their subtrees.
    pub(crate) fn delete_action_subtree(&mut self, id: NoteId) -> Result<(), Error> {
        let mut doomed = vec![id];
        doomed.extend(self.hierarchy().descendants(id));
        let tree = self.actions_tree();
        for node in self.nodes_of(&doomed) {
            if tree.contains(node) && !tree.is_node_deleted(&node).unwrap_or(true) {
                tree.delete(node).map_err(loro_error)?;
            }
        }
        for note in doomed {
            self.notes_map().delete(&note.to_string()).map_err(loro_error)?;
        }
        Ok(())
    }

    pub(crate) fn actions_tree(&self) -> LoroTree {
        let tree = self.doc.get_tree(ACTIONS);
        tree.enable_fractional_index(0);
        tree
    }

    /// Creates a node for an action as the last child of `parent`'s node, or at the top level.
    pub(crate) fn place_node(
        &mut self,
        id: NoteId,
        parent: Option<NoteId>,
    ) -> Result<TreeID, Error> {
        let parent_node = match parent {
            Some(parent) => match self.hierarchy().nodes.get(&parent) {
                Some(&node) => Some(node),
                None => Some(self.place_node(parent, None)?),
            },
            None => None,
        };
        let node =
            self.actions_tree().create(TreeParentId::from(parent_node)).map_err(loro_error)?;
        self.actions_tree()
            .get_meta(node)
            .map_err(loro_error)?
            .insert(NOTE, id.to_string())
            .map_err(loro_error)?;
        if let Some(parent) = parent {
            self.promote(parent)?;
        }
        Ok(node)
    }

    fn promote(&mut self, id: NoteId) -> Result<(), Error> {
        let action = self.note(id).and_then(|note| note.action);
        if action.is_some_and(|action| !action.project) {
            let map = self.note_map(id)?;
            if let Some(ValueOrContainer::Container(container)) = map.get(ACTION)
                && let Ok(action) = container.into_map()
            {
                action.insert(PROJECT, true).map_err(loro_error)?;
            }
        }
        Ok(())
    }

    /// Every live node whose note is one of `ids`, including duplicates.
    fn nodes_of(&self, ids: &[NoteId]) -> Vec<TreeID> {
        let tree = self.actions_tree();
        live_nodes(&tree)
            .into_iter()
            .filter(|&node| node_note(&tree, node).is_some_and(|note| ids.contains(&note)))
            .collect()
    }

    pub(crate) fn hierarchy(&self) -> Hierarchy {
        let tree = self.actions_tree();
        let actions: HashMap<NoteId, i64> = self
            .notes()
            .into_iter()
            .filter(|note| note.action.is_some())
            .map(|note| (note.id, note.created))
            .collect();
        let mut owners: HashMap<NoteId, TreeID> = HashMap::new();
        for node in live_nodes(&tree) {
            if let Some(note) = node_note(&tree, node).filter(|note| actions.contains_key(note)) {
                let owner = owners.entry(note).or_insert(node);
                if (node.peer, node.counter) < (owner.peer, owner.counter) {
                    *owner = node;
                }
            }
        }
        let mut hierarchy = Hierarchy::default();
        let mut stack: Vec<(TreeID, Option<NoteId>)> =
            tree.roots().into_iter().rev().map(|root| (root, None)).collect();
        while let Some((node, parent)) = stack.pop() {
            let note = node_note(&tree, node).filter(|note| actions.contains_key(note));
            let children_parent = match note {
                Some(note) if owners.get(&note) == Some(&node) => {
                    let parent = parent.filter(|&parent| parent != note);
                    hierarchy.parents.insert(note, parent);
                    hierarchy.children.entry(parent).or_default().push(note);
                    hierarchy.nodes.insert(note, node);
                    Some(note)
                }
                Some(note) => Some(note),
                None => parent,
            };
            if let Some(children) = tree.children(node) {
                stack.extend(children.into_iter().rev().map(|child| (child, children_parent)));
            }
        }
        let mut unplaced: Vec<(i64, NoteId)> = actions
            .iter()
            .filter(|(note, _)| !hierarchy.parents.contains_key(note))
            .map(|(&note, &created)| (created, note))
            .collect();
        unplaced.sort();
        for (_, note) in unplaced {
            hierarchy.parents.insert(note, None);
            hierarchy.children.entry(None).or_default().push(note);
        }
        hierarchy
    }
}

/// How an action ranks in a list, before outline order breaks ties: overdue first, then due within
/// a week, both soonest due first; then started or undated, with a due date before none; then
/// starting within a week, and last starting later, both soonest start first.
pub(crate) fn order_key(dates: &ActionDates, today: Date) -> Rank {
    let due = dates.due.map(|due| due.date);
    let start = dates.start.map(|start| start.date);
    let (rank, first) = match dates.status {
        ActionStatus::Overdue => (0, due),
        ActionStatus::DueSoon => (1, due),
        ActionStatus::Normal => (2, None),
        ActionStatus::Upcoming if start.is_some_and(|s| s <= today.add_days(SOON)) => (3, start),
        ActionStatus::Upcoming => (4, start),
    };
    (rank, first, due.is_none(), due)
}

/// Orders actions as an outline: each action under the nearest of its ancestors that's also in
/// `entries`. Siblings rank by the most pressing of themselves and the actions under them, by
/// `order_key`, with ties in outline order. Sets each action's depth in that outline.
fn order_as_outline(
    entries: Vec<Walked>,
    hierarchy: &Hierarchy,
    outline: &HashMap<NoteId, usize>,
    today: Date,
) -> Vec<Walked> {
    let shown: HashSet<NoteId> = entries.iter().map(|entry| entry.note.id).collect();
    let mut groups: HashMap<Option<NoteId>, Vec<Walked>> = HashMap::new();
    for entry in entries {
        let parent = hierarchy.ancestors(entry.note.id).find(|ancestor| shown.contains(ancestor));
        groups.entry(parent).or_default().push(entry);
    }
    let mut ranks = HashMap::new();
    rank_subtree(&groups, None, today, &mut ranks);
    for siblings in groups.values_mut() {
        siblings.sort_by_key(|entry| (ranks[&entry.note.id], outline[&entry.note.id]));
    }
    order_subtree(&mut groups, None, 0)
}

pub(crate) type Rank = (u8, Option<Date>, bool, Option<Date>);

/// Records, for each action under `parent`, the most pressing rank among it and the actions under
/// it, and returns the most pressing among them all.
fn rank_subtree(
    groups: &HashMap<Option<NoteId>, Vec<Walked>>,
    parent: Option<NoteId>,
    today: Date,
    ranks: &mut HashMap<NoteId, Rank>,
) -> Option<Rank> {
    let mut best = None;
    for entry in groups.get(&parent).into_iter().flatten() {
        let below = rank_subtree(groups, Some(entry.note.id), today, ranks);
        let own = order_key(&entry.dates, today);
        let rank = below.map_or(own, |below| below.min(own));
        ranks.insert(entry.note.id, rank);
        best = Some(best.map_or(rank, |best: Rank| best.min(rank)));
    }
    best
}

fn order_subtree(
    groups: &mut HashMap<Option<NoteId>, Vec<Walked>>,
    parent: Option<NoteId>,
    depth: usize,
) -> Vec<Walked> {
    let mut ordered = Vec::new();
    for mut entry in groups.remove(&parent).unwrap_or_default() {
        entry.depth = depth;
        let id = entry.note.id;
        ordered.push(entry);
        ordered.extend(order_subtree(groups, Some(id), depth + 1));
    }
    ordered
}

/// Visits every action in the hierarchy, in order, with what it inherits from its ancestors.
fn walk(hierarchy: &Hierarchy, notes: &HashMap<NoteId, Note>, today: Date) -> Vec<Walked> {
    let mut walk = Walk { entries: Vec::new(), hierarchy, notes, today, visited: HashSet::new() };
    walk.visit(None, &Context::default());
    walk.entries
}

/// What an action inherits from its ancestors.
#[derive(Clone, Default)]
struct Context {
    /// Whether the action or an ancestor is archived.
    archived: bool,
    /// Whether the action or an ancestor is done.
    closed_ancestor: bool,
    /// The earliest due date of the action and its ancestors.
    due: Option<Dated>,
    /// The list that applies to the action, from its own list and its ancestors' lists.
    list: ActionList,
    /// The list that applies to the action's parent.
    parent_list: ActionList,
    /// The latest start date of the action's ancestors.
    parent_start: Option<Dated>,
    /// The latest start date of the action and its ancestors.
    start: Option<Dated>,
    trashed: bool,
}

impl Context {
    /// Whether the action and all its ancestors are open and outside the archive and the trash.
    fn active(&self) -> bool {
        !self.archived && !self.closed_ancestor && !self.trashed
    }

    /// Whether a project above the action is put off, by being on the inbox, someday, or maybe
    /// list, or by starting after `today`. The project then stands for the actions inside it.
    fn inside_put_off(&self, today: Date) -> bool {
        self.parent_list.holds_back() || self.parent_start.is_some_and(|start| start.date > today)
    }

    /// Whether the action, or a project above it, starts after `today`.
    fn delayed(&self, today: Date) -> bool {
        self.start.is_some_and(|start| start.date > today)
    }
}

struct Walked {
    context: Context,
    depth: usize,
    note: Note,
    dates: ActionDates,
    action: Action,
}

struct Walk<'a> {
    entries: Vec<Walked>,
    hierarchy: &'a Hierarchy,
    notes: &'a HashMap<NoteId, Note>,
    today: Date,
    visited: HashSet<NoteId>,
}

impl Walk<'_> {
    fn visit(&mut self, parent: Option<NoteId>, inherited: &Context) {
        for &id in self.hierarchy.children(parent) {
            let Some(note) = self.notes.get(&id) else { continue };
            if !self.visited.insert(id) {
                continue;
            }
            let action = note.action.clone().unwrap_or_default();
            let context = Context {
                archived: inherited.archived || note.archived.is_some(),
                closed_ancestor: inherited.closed_ancestor || !action.is_open(),
                due: match (inherited.due, action.due) {
                    (Some(project), Some(own)) if project.date < own => Some(project),
                    (inherited, own) => own.map(|date| Dated { date, from: id }).or(inherited),
                },
                list: inherited.list.inherit(action.list),
                parent_list: inherited.list,
                parent_start: inherited.start,
                start: match (inherited.start, action.defer) {
                    (Some(project), Some(own)) if project.date > own => Some(project),
                    (inherited, own) => own.map(|date| Dated { date, from: id }).or(inherited),
                },
                trashed: inherited.trashed || note.is_trashed(),
            };
            let today = self.today;
            let status = match (context.due.map(|d| d.date), context.start.map(|s| s.date)) {
                _ if !context.active() => ActionStatus::Normal,
                (Some(due), _) if due < today => ActionStatus::Overdue,
                (Some(due), _) if due <= today.add_days(SOON) => ActionStatus::DueSoon,
                (_, Some(start)) if start > today => ActionStatus::Upcoming,
                _ => ActionStatus::Normal,
            };
            let dates = ActionDates { due: context.due, start: context.start, status };
            let depth = self.hierarchy.ancestors(id).count();
            let walked =
                Walked { context: context.clone(), dates, depth, note: note.clone(), action };
            self.entries.push(walked);
            self.visit(Some(id), &context);
        }
    }
}

fn insert_action_map(note: &LoroMap, list: ActionList) -> Result<(), Error> {
    let action = note.insert_container(ACTION, LoroMap::new()).map_err(loro_error)?;
    action.insert(STATE, ActionState::Open.as_str()).map_err(loro_error)?;
    action.insert(LIST, list.as_str()).map_err(loro_error)?;
    action.insert(PROJECT, false).map_err(loro_error)?;
    // The action may become a step of a recurring action. Creating the map later, on two devices
    // at once, would make two containers under one key, and sync keeps only one.
    action.insert_container(OCCURRENCES, LoroMap::new()).map_err(loro_error)?;
    Ok(())
}

fn live_nodes(tree: &LoroTree) -> Vec<TreeID> {
    let mut nodes = Vec::new();
    let mut stack = tree.roots();
    while let Some(node) = stack.pop() {
        nodes.push(node);
        stack.extend(tree.children(node).unwrap_or_default());
    }
    nodes
}

fn node_note(tree: &LoroTree, node: TreeID) -> Option<NoteId> {
    let meta = tree.get_meta(node).ok()?;
    meta.get(NOTE).and_then(value).and_then(string)?.parse().ok()
}

/// `body` in an element named `name`, with blank lines around it so Markdown reads the tags as
/// HTML blocks of their own, or `None` if the body is blank. The first line keeps its indentation,
/// which can make it code.
fn brief_element(name: &str, body: &str) -> Option<String> {
    let first = body.find(|c: char| !c.is_whitespace())?;
    let start = body[..first].rfind('\n').map_or(0, |newline| newline + 1);
    let body = body[start..].trim_end();
    let close = open_fence(body).map(|fence| format!("\n{fence}")).unwrap_or_default();
    Some(format!("<{name}>\n\n{body}{close}\n\n</{name}>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::new(2026, 10, 19).unwrap()
    }

    fn titles(entries: Vec<ActionEntry>) -> Vec<(usize, String)> {
        entries
            .into_iter()
            .map(|entry| (entry.depth, entry.note.label().unwrap_or_default()))
            .collect()
    }

    #[test]
    fn parses_dates() {
        assert_eq!("2024-02-29".parse::<Date>().unwrap().to_string(), "2024-02-29");
        for bad in ["2023-02-29", "2026-13-01", "2026-1-01", "20261019", "2026-10-1x", ""] {
            assert!(bad.parse::<Date>().is_err(), "{bad:?}");
        }
        assert!("2026-10-18".parse::<Date>().unwrap() < today());
    }

    #[test]
    fn creates_actions_and_promotes_parents() {
        let mut library = Library::in_memory();
        let project = library.create_task("Paint the shed\nNotes", None).unwrap();
        assert!(!library.note(project).unwrap().action.unwrap().project);
        let step = library.create_task("Buy paint", Some(project)).unwrap();
        let note = library.note(step).unwrap();
        assert_eq!(note.label().as_deref(), Some("Buy paint"));
        assert_eq!(note.action.as_ref().unwrap().list, ActionList::Active);
        assert!(library.note(project).unwrap().action.unwrap().project);
        assert_eq!(library.action_parent(step), Some(project));
        assert_eq!(library.action_children(project)[0].id, step);
    }

    #[test]
    fn briefs_an_action_with_its_projects() {
        let mut library = Library::in_memory();
        let project = library.create_task("\nPaint the shed\n\n---\n\nUse green.\n", None).unwrap();
        let step = library.create_task(" \n", Some(project)).unwrap();
        let task = library.create_task("Buy sandpaper\n\n```sh\norder 120", Some(step)).unwrap();
        let project_element = "<project>\n\nPaint the shed\n\n---\n\nUse green.\n\n</project>\n\n";
        let action_element = "<action>\n\nBuy sandpaper\n\n```sh\norder 120\n```\n\n</action>\n";
        let brief = format!("{project_element}{action_element}");
        assert_eq!(library.action_brief(task).unwrap(), brief);
        let empty = format!("{project_element}<action>\n</action>\n");
        assert_eq!(library.action_brief(step).unwrap(), empty);
        library.trash(project).unwrap();
        library.trash(task).unwrap();
        assert_eq!(library.action_brief(task).unwrap(), brief);
        let memo = library.create_memo("x").unwrap();
        assert!(matches!(library.action_brief(memo), Err(Error::NotAnAction(_))));
        library.trash(memo).unwrap();
        library.delete(memo).unwrap();
        assert!(matches!(library.action_brief(memo), Err(Error::NotFound(_))));
    }

    #[test]
    fn brief_elements_keep_indentation_and_skip_blank_bodies() {
        assert_eq!(
            brief_element("action", "\n \n    code\n").unwrap(),
            "<action>\n\n    code\n\n</action>"
        );
        assert_eq!(brief_element("action", " \n\t"), None);
    }

    #[test]
    fn changes_fields_and_closes() {
        let mut library = Library::in_memory();
        let id = library.create_task("Call the plumber", None).unwrap();
        library.change_action(id, ActionChange::Due(Some(today()))).unwrap();
        library.change_action(id, ActionChange::WaitingFor(Some("Bob".into()))).unwrap();
        library.change_action(id, ActionChange::State(ActionState::Done)).unwrap();
        let action = library.note(id).unwrap().action.unwrap();
        assert_eq!(action.due, Some(today()));
        assert_eq!(action.waiting_for.as_deref(), Some("Bob"));
        assert!(action.closed.is_some());
        library.change_action(id, ActionChange::State(ActionState::Open)).unwrap();
        library.change_action(id, ActionChange::Due(None)).unwrap();
        let action = library.note(id).unwrap().action.unwrap();
        assert_eq!((action.closed, action.due), (None, None));
        let memo = library.create_memo("x").unwrap();
        assert!(matches!(
            library.change_action(memo, ActionChange::Project(true)),
            Err(Error::NotAnAction(_))
        ));
    }

    #[test]
    fn moves_actions_without_cycles() {
        let mut library = Library::in_memory();
        let a = library.create_task("a", None).unwrap();
        let b = library.create_task("b", None).unwrap();
        let c = library.create_task("c", Some(a)).unwrap();
        library.move_action(b, Some(a), Some(0)).unwrap();
        let children: Vec<_> = library.action_children(a).into_iter().map(|n| n.id).collect();
        assert_eq!(children, [b, c]);
        assert!(matches!(library.move_action(a, Some(c), None), Err(Error::CyclicMove(_))));
        library.move_action(c, None, Some(0)).unwrap();
        assert_eq!(library.action_parent(c), None);
        assert_eq!(
            titles(library.action_view(ActionView::All, today())),
            [(0, "c".into()), (0, "a".into()), (1, "b".into()),]
        );
    }

    #[test]
    fn adds_days() {
        let date = |y, m, d| Date::new(y, m, d).unwrap();
        assert_eq!(date(2026, 10, 19).add_days(7), date(2026, 10, 26));
        assert_eq!(date(2026, 12, 28).add_days(7), date(2027, 1, 4));
        assert_eq!(date(2024, 2, 28).add_days(1), date(2024, 2, 29));
        assert_eq!(date(2024, 3, 1).add_days(-1), date(2024, 2, 29));
        assert_eq!(date(1970, 1, 1).days(), 0);
        assert_eq!(date(2000, 3, 1).add_days(-366), date(1999, 3, 1));
        assert_eq!(date(2000, 3, 1).add_days(-367), date(1999, 2, 28));
    }

    #[test]
    fn rates_how_pressing_actions_are() {
        let mut library = Library::in_memory();
        let today = today();
        let mut add = |name: &str, defer: Option<Date>, due: Option<Date>, parent| {
            let id = library.create_task(name, parent).unwrap();
            library.change_action(id, ActionChange::Defer(defer)).unwrap();
            library.change_action(id, ActionChange::Due(due)).unwrap();
            id
        };
        let overdue = add("overdue", None, Some(today.add_days(-1)), None);
        let soon = add("due today", None, Some(today), None);
        let week = add("due in a week", None, Some(today.add_days(7)), None);
        let later = add("due later", None, Some(today.add_days(8)), None);
        let starts = add("starts in a week", Some(today.add_days(7)), None, None);
        let far = add("starts later", Some(today.add_days(8)), None, None);
        let project = add("project", None, Some(today.add_days(2)), None);
        let step = add("step", Some(today.add_days(3)), None, Some(project));
        let dates = library.action_dates(today);
        let status = |id| dates[&id].status;
        assert_eq!(status(overdue), ActionStatus::Overdue);
        assert_eq!(status(soon), ActionStatus::DueSoon);
        assert_eq!(status(week), ActionStatus::DueSoon);
        assert_eq!(status(later), ActionStatus::Normal);
        assert_eq!(status(starts), ActionStatus::Upcoming);
        assert_eq!(status(far), ActionStatus::Upcoming);
        assert_eq!(status(step), ActionStatus::DueSoon);
        assert_eq!(dates[&step].due, Some(Dated { date: today.add_days(2), from: project }));
        assert_eq!(dates[&step].start, Some(Dated { date: today.add_days(3), from: step }));
        assert_eq!(dates[&soon].due.map(|due| due.from), Some(soon));
        let next: Vec<_> = library
            .action_view(ActionView::Next, today)
            .into_iter()
            .map(|entry| entry.note.label().unwrap())
            .collect();
        assert_eq!(
            next,
            ["overdue", "due today", "step", "due in a week", "due later", "starts in a week"]
        );
    }

    #[test]
    fn shows_one_next_action_per_project() {
        let mut library = Library::in_memory();
        let house = library.create_task("House", None).unwrap();
        let roof = library.create_task("Roof", Some(house)).unwrap();
        let call = library.create_task("Call roofer", Some(roof)).unwrap();
        library.change_action(call, ActionChange::List(ActionList::Waiting)).unwrap();
        library.create_task("Buy shingles", Some(roof)).unwrap();
        library.create_task("Paint", Some(house)).unwrap();
        library.create_task("Loose task", None).unwrap();
        library.create_task("Another loose task", None).unwrap();
        let next = |library: &Library| -> Vec<String> {
            let entries = library.action_view(ActionView::Next, today());
            entries.into_iter().map(|entry| entry.note.label().unwrap()).collect()
        };
        assert_eq!(next(&library), ["Buy shingles", "Loose task", "Another loose task"]);
        assert_eq!(library.action_view(ActionView::Next, today())[0].parents, [house, roof]);
        let shingles = library.action_children(roof)[1].id;
        library.change_action(shingles, ActionChange::State(ActionState::Done)).unwrap();
        assert_eq!(next(&library), ["Paint", "Loose task", "Another loose task"]);
        let today = today();
        let late = library.create_task("Late step", Some(roof)).unwrap();
        library.change_action(late, ActionChange::Due(Some(today.add_days(5)))).unwrap();
        assert_eq!(next(&library)[0], "Late step");
        let overdue = library.create_task("Overdue step", Some(house)).unwrap();
        library.change_action(overdue, ActionChange::Due(Some(today.add_days(-1)))).unwrap();
        assert_eq!(next(&library), ["Overdue step", "Loose task", "Another loose task"]);
        let solo = library.create_task("Solo project", None).unwrap();
        let soon = library.create_task("Starts soon", Some(solo)).unwrap();
        library.change_action(soon, ActionChange::Defer(Some(today.add_days(2)))).unwrap();
        let sooner = library.create_task("Starts sooner", Some(solo)).unwrap();
        library.change_action(sooner, ActionChange::Defer(Some(today.add_days(1)))).unwrap();
        assert_eq!(next(&library).last().map(String::as_str), Some("Starts sooner"));
    }

    #[test]
    fn sorts_next_actions() {
        let mut library = Library::in_memory();
        let today = today();
        let mut add = |name: &str, defer: Option<i32>, due: Option<i32>| {
            let id = library.create_task(name, None).unwrap();
            library
                .change_action(id, ActionChange::Defer(defer.map(|d| today.add_days(d))))
                .unwrap();
            library.change_action(id, ActionChange::Due(due.map(|d| today.add_days(d)))).unwrap();
        };
        add("starts in two days", Some(2), None);
        add("plain", None, None);
        add("due in a month", None, Some(30));
        add("started yesterday", Some(-1), None);
        add("started last week", Some(-7), Some(20));
        add("due soon, not started", Some(1), Some(3));
        add("overdue", None, Some(-2));
        add("starts tomorrow", Some(1), None);
        let found: Vec<_> = library
            .action_view(ActionView::Next, today)
            .into_iter()
            .map(|entry| entry.note.label().unwrap())
            .collect();
        assert_eq!(
            found,
            [
                "overdue",
                "due soon, not started",
                "started last week",
                "due in a month",
                "plain",
                "started yesterday",
                "starts tomorrow",
                "starts in two days",
            ]
        );
    }

    #[test]
    fn creates_actions_in_the_inbox() {
        let mut library = Library::in_memory();
        let first = library.create_inbox_task("Call the dentist").unwrap();
        let second = library.create_inbox_task("Plan the party").unwrap();
        library.create_task("Ready to go", None).unwrap();
        library.create_task("Guest list", Some(second)).unwrap();
        let view = |library: &Library, view| -> Vec<String> {
            let entries = library.action_view(view, today());
            entries.into_iter().map(|entry| entry.note.label().unwrap()).collect()
        };
        assert_eq!(view(&library, ActionView::Inbox), ["Call the dentist", "Plan the party"]);
        assert_eq!(view(&library, ActionView::Next), ["Ready to go"]);
        library.change_action(first, ActionChange::List(ActionList::Active)).unwrap();
        assert_eq!(view(&library, ActionView::Inbox), ["Plan the party"]);
        assert_eq!(view(&library, ActionView::Next), ["Call the dentist", "Ready to go"]);
    }

    #[test]
    fn sorts_scheduled_actions() {
        let mut library = Library::in_memory();
        let day = |d| Date::new(2026, 11, d);
        let mut add = |name: &str, defer, due| {
            let id = library.create_task(name, None).unwrap();
            library.change_action(id, ActionChange::Defer(defer)).unwrap();
            library.change_action(id, ActionChange::Due(due)).unwrap();
        };
        add("later start", day(2), None);
        add("no due, older", day(1), None);
        add("no due, newer", day(1), None);
        add("due late", day(1), day(9));
        add("due soon", day(1), day(5));
        let found: Vec<_> = library
            .action_view(ActionView::Scheduled, today())
            .into_iter()
            .map(|entry| entry.note.label().unwrap())
            .collect();
        assert_eq!(
            found,
            ["due soon", "due late", "no due, older", "no due, newer", "later start"]
        );
    }

    #[test]
    fn orders_views_by_dates_and_steps_by_outline() {
        let mut library = Library::in_memory();
        let today = today();
        let house = library.create_task("House", None).unwrap();
        library.create_task("Paint", Some(house)).unwrap();
        let roof = library.create_task("Roof", Some(house)).unwrap();
        library.create_task("Shingles", Some(roof)).unwrap();
        let gutters = library.create_task("Gutters", Some(roof)).unwrap();
        library.change_action(gutters, ActionChange::Due(Some(today.add_days(1)))).unwrap();
        let garden = library.create_task("Garden", None).unwrap();
        library.change_action(garden, ActionChange::Due(Some(today.add_days(-1)))).unwrap();
        let outline = |entries: Vec<ActionEntry>| -> Vec<(usize, String)> {
            entries.into_iter().map(|e| (e.depth, e.note.label().unwrap())).collect()
        };
        assert_eq!(
            outline(library.action_view(ActionView::All, today)),
            [
                (0, "Garden".into()),
                (0, "House".into()),
                (1, "Roof".into()),
                (2, "Gutters".into()),
                (2, "Shingles".into()),
                (1, "Paint".into()),
            ]
        );
        assert_eq!(
            outline(library.action_subtree(house, today)),
            [
                (0, "Paint".into()),
                (0, "Roof".into()),
                (1, "Shingles".into()),
                (1, "Gutters".into())
            ]
        );
    }

    #[test]
    fn lists_subtrees() {
        let mut library = Library::in_memory();
        let house = library.create_task("House", None).unwrap();
        let roof = library.create_task("Roof", Some(house)).unwrap();
        library.create_task("Shingles", Some(roof)).unwrap();
        let junk = library.create_task("Junk", Some(roof)).unwrap();
        library.create_task("Junk step", Some(junk)).unwrap();
        library.create_task("Paint", Some(house)).unwrap();
        library.trash(junk).unwrap();
        let found: Vec<_> = library
            .action_subtree(house, today())
            .into_iter()
            .map(|entry| (entry.depth, entry.note.label().unwrap()))
            .collect();
        assert_eq!(found, [(0, "Roof".into()), (1, "Shingles".into()), (0, "Paint".into())]);
    }

    #[test]
    fn reorders_steps() {
        let mut library = Library::in_memory();
        let project = library.create_task("Project", None).unwrap();
        let a = library.create_task("a", Some(project)).unwrap();
        let b = library.create_task("b", Some(project)).unwrap();
        let c = library.create_task("c", Some(project)).unwrap();
        let order = |library: &Library| -> Vec<String> {
            let children = library.action_children(project);
            children.into_iter().map(|note| note.label().unwrap()).collect()
        };
        library.move_action_beside(c, a, false).unwrap();
        assert_eq!(order(&library), ["c", "a", "b"]);
        library.move_action_beside(c, b, true).unwrap();
        assert_eq!(order(&library), ["a", "b", "c"]);
        library.move_action_among_siblings(a, false).unwrap();
        assert_eq!(order(&library), ["b", "a", "c"]);
        library.move_action_among_siblings(b, true).unwrap();
        library.move_action_among_siblings(c, false).unwrap();
        assert_eq!(order(&library), ["b", "a", "c"]);
        library.move_action_among_siblings(c, true).unwrap();
        assert_eq!(order(&library), ["b", "c", "a"]);
        let next = library.action_view(ActionView::Next, today());
        assert_eq!(next[0].note.id, b);
    }

    #[test]
    fn trashes_and_deletes_subtrees() {
        let mut library = Library::in_memory();
        let project = library.create_task("Project", None).unwrap();
        let step = library.create_task("Step", Some(project)).unwrap();
        library.trash(project).unwrap();
        assert!(library.is_in_trash(step));
        assert!(library.action_view(ActionView::Next, today()).is_empty());
        library.delete(project).unwrap();
        assert!(library.note(step).is_none());
        assert!(library.hierarchy().nodes.is_empty());
        assert_eq!(library.deleted_label(step).as_deref(), Some("Step"));
    }

    #[test]
    fn computes_gtd_views() {
        let mut library = Library::in_memory();
        let today = today();
        let tomorrow = Date::new(2026, 10, 20).unwrap();
        let house = library.create_task("House", None).unwrap();
        let paint = library.create_task("Paint", Some(house)).unwrap();
        let call = library.create_task("Call Bob", Some(house)).unwrap();
        library.change_action(call, ActionChange::List(ActionList::Waiting)).unwrap();
        let later = library.create_task("Later", Some(house)).unwrap();
        library.change_action(later, ActionChange::Defer(Some(tomorrow))).unwrap();
        let due = library.create_task("Taxes", None).unwrap();
        library.change_action(due, ActionChange::Due(Some(tomorrow))).unwrap();
        let dream = library.create_task("Sail", None).unwrap();
        library.change_action(dream, ActionChange::List(ActionList::Someday)).unwrap();
        let boat = library.create_task("Buy a boat", Some(dream)).unwrap();
        library.change_action(boat, ActionChange::List(ActionList::Maybe)).unwrap();
        let goat = library.create_task("Get a goat", None).unwrap();
        library.change_action(goat, ActionChange::List(ActionList::Maybe)).unwrap();
        let barn = library.create_task("Build a barn", Some(goat)).unwrap();
        library.change_action(barn, ActionChange::List(ActionList::Someday)).unwrap();
        let empty = library.create_task("Empty", None).unwrap();
        library.change_action(empty, ActionChange::Project(true)).unwrap();
        let done = library.create_task("Done", None).unwrap();
        library.change_action(done, ActionChange::State(ActionState::Done)).unwrap();
        library.create_task("Hidden", Some(done)).unwrap();

        let view = |view| titles(library.action_view(view, today));
        assert_eq!(view(ActionView::Next), [(0, "Taxes".into()), (0, "Paint".into())]);
        assert_eq!(view(ActionView::Waiting), [(0, "Call Bob".into())]);
        assert_eq!(view(ActionView::Scheduled), [(0, "Later".into())]);
        assert_eq!(view(ActionView::Someday), [(0, "Sail".into())]);
        assert_eq!(view(ActionView::Maybe), [(0, "Get a goat".into())]);
        assert_eq!(view(ActionView::Closed), [(0, "Done".into())]);
        assert_eq!(view(ActionView::Stalled), [(0, "Empty".into())]);
        let projects = library.action_view(ActionView::Projects, today);
        let stalled: Vec<_> =
            projects.iter().map(|e| (e.note.label().unwrap(), e.stalled)).collect();
        assert_eq!(
            stalled,
            [
                ("House".into(), false),
                ("Sail".into(), false),
                ("Get a goat".into(), false),
                ("Empty".into(), true)
            ]
        );
        library.change_action(paint, ActionChange::State(ActionState::Done)).unwrap();
        library.set_archived(call, true).unwrap();
        library.change_action(later, ActionChange::State(ActionState::Done)).unwrap();
        assert!(library.action_view(ActionView::Projects, today)[0].stalled);
    }

    #[test]
    fn keeps_steps_of_trashed_projects_out_of_search_and_backlinks() {
        let mut library = Library::in_memory();
        let memo = library.create_memo("Memo").unwrap();
        let project = library.create_task("Project", None).unwrap();
        let step =
            library.create_task(&format!("Step {}", crate::wikilink(memo, None)), Some(project));
        let step = step.unwrap();
        library.trash(project).unwrap();
        assert!(library.notes_in_trash().contains(&step));
        assert!(library.search("Step").is_empty());
        assert!(library.backlinks(memo).is_empty());
    }

    #[test]
    fn restores_trashed_projects_above_a_restored_step() {
        let mut library = Library::in_memory();
        let project = library.create_task("Project", None).unwrap();
        let step = library.create_task("Step", Some(project)).unwrap();
        library.trash(step).unwrap();
        library.trash(project).unwrap();
        library.restore(step).unwrap();
        assert!(!library.is_in_trash(step));
        assert!(!library.is_in_trash(project));
    }

    #[test]
    fn lets_put_off_projects_stand_for_what_they_hold() {
        let mut library = Library::in_memory();
        let today = today();
        let later = today.add_days(30);
        let cabin = library.create_task("Cabin", None).unwrap();
        library.change_action(cabin, ActionChange::List(ActionList::Someday)).unwrap();
        let foundation = library.create_task("Foundation", Some(cabin)).unwrap();
        library.change_action(foundation, ActionChange::Project(true)).unwrap();
        let survey = library.create_task("Survey", Some(cabin)).unwrap();
        library.change_action(survey, ActionChange::Defer(Some(today.add_days(10)))).unwrap();
        let deferred = library.create_task("Deferred", None).unwrap();
        library.change_action(deferred, ActionChange::Project(true)).unwrap();
        library.change_action(deferred, ActionChange::Defer(Some(later))).unwrap();
        let inside = library.create_task("Inside", Some(deferred)).unwrap();
        library.change_action(inside, ActionChange::List(ActionList::Inbox)).unwrap();
        let names = |view| -> Vec<String> {
            library.action_view(view, today).into_iter().map(|e| e.note.label().unwrap()).collect()
        };
        assert_eq!(names(ActionView::Projects), ["Cabin", "Deferred"]);
        assert_eq!(names(ActionView::Scheduled), ["Deferred"]);
        assert!(names(ActionView::Stalled).is_empty());
        assert!(names(ActionView::Inbox).is_empty());
    }

    #[test]
    fn counts_projects_in_the_inbox_as_stalled() {
        let mut library = Library::in_memory();
        let captured = library.create_inbox_task("Captured").unwrap();
        library.change_action(captured, ActionChange::Project(true)).unwrap();
        let view = titles(library.action_view(ActionView::Stalled, today()));
        assert_eq!(view, [(0, "Captured".into())]);
    }

    #[test]
    fn archives_without_changing_anything_else() {
        let mut library = Library::in_memory();
        let done = library.create_task("Done", None).unwrap();
        library.change_action(done, ActionChange::State(ActionState::Done)).unwrap();
        let task = library.create_task("Task", None).unwrap();
        library.create_task("Step", Some(task)).unwrap();
        let before = library.note(task).unwrap();
        for id in [done, task] {
            library.set_archived(id, true).unwrap();
        }
        let after = library.note(task).unwrap();
        assert!(after.archived.is_some());
        assert_eq!(after.modified, before.modified);
        assert_eq!(after.action.unwrap().state, ActionState::Open);
        for view in [ActionView::All, ActionView::Closed, ActionView::Next, ActionView::Projects] {
            assert!(library.action_view(view, today()).is_empty(), "{view:?}");
        }
        library.set_archived(task, false).unwrap();
        assert_eq!(titles(library.action_view(ActionView::Next, today())), [(0, "Step".into())]);
    }

    #[test]
    fn queues_actions_for_planning() {
        let mut library = Library::in_memory();
        let first = library.create_task("First", None).unwrap();
        let second = library.create_task("Second", None).unwrap();
        library.change_action(second, ActionChange::Planning(true)).unwrap();
        // Timestamps have millisecond precision.
        std::thread::sleep(std::time::Duration::from_millis(2));
        library.change_action(first, ActionChange::Planning(true)).unwrap();
        let queued = library.note(second).unwrap().action.unwrap().planning;
        library.change_action(second, ActionChange::Planning(true)).unwrap();
        assert_eq!(library.note(second).unwrap().action.unwrap().planning, queued);
        let view = |library: &Library| titles(library.action_view(ActionView::Planning, today()));
        assert_eq!(view(&library), [(0, "Second".into()), (0, "First".into())]);
        assert!(library.action_view(ActionView::Next, today()).is_empty());
        library.change_action(second, ActionChange::Planning(false)).unwrap();
        library.change_action(first, ActionChange::State(ActionState::Done)).unwrap();
        assert!(view(&library).is_empty());
    }

    #[test]
    fn stars_actions() {
        let mut library = Library::in_memory();
        let house = library.create_task("House", None).unwrap();
        let paint = library.create_task("Paint", Some(house)).unwrap();
        library.create_task("Roof", Some(house)).unwrap();
        let sail = library.create_task("Sail", None).unwrap();
        library.change_action(sail, ActionChange::List(ActionList::Someday)).unwrap();
        library.set_starred(house, true).unwrap();
        library.set_starred(paint, true).unwrap();
        library.set_starred(sail, true).unwrap();
        let before = library.note(sail).unwrap();
        library.set_starred(sail, true).unwrap();
        let after = library.note(sail).unwrap();
        assert_eq!((after.starred, after.modified), (before.starred, before.modified));
        let view = |library: &Library, view| titles(library.action_view(view, today()));
        assert_eq!(
            view(&library, ActionView::Starred),
            [(0, "House".into()), (0, "Paint".into()), (0, "Sail".into())]
        );
        // The project's next step is starred, so it shows only under Starred.
        assert!(view(&library, ActionView::Next).is_empty());
        library.change_action(house, ActionChange::State(ActionState::Done)).unwrap();
        assert_eq!(view(&library, ActionView::Starred), [(0, "Sail".into())]);
        library.change_action(house, ActionChange::State(ActionState::Open)).unwrap();
        library.set_starred(paint, false).unwrap();
        assert_eq!(view(&library, ActionView::Next), [(0, "Paint".into())]);
        library.set_archived(sail, true).unwrap();
        assert_eq!(view(&library, ActionView::Starred), [(0, "House".into())]);
    }

    #[test]
    fn resolves_duplicate_and_orphaned_nodes() {
        let mut library = Library::in_memory();
        let a = library.create_task("a", None).unwrap();
        let b = library.create_task("b", None).unwrap();
        let tree = library.actions_tree();
        let duplicate = tree.create(TreeParentId::from(None::<TreeID>)).unwrap();
        tree.get_meta(duplicate).unwrap().insert(NOTE, a.to_string()).unwrap();
        let orphan = tree.create(TreeParentId::from(None::<TreeID>)).unwrap();
        tree.get_meta(orphan).unwrap().insert(NOTE, NoteId::generate().to_string()).unwrap();
        let node_b = library.hierarchy().nodes[&b];
        tree.mov(node_b, TreeParentId::from(Some(orphan))).unwrap();
        library.doc.commit();
        let hierarchy = library.hierarchy();
        assert_eq!(hierarchy.children(None), [a, b]);
        assert_eq!(hierarchy.parent(b), None);
    }
}
