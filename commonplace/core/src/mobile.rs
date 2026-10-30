// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::sync::{Config, Host, OutgoingState, Status, Syncer};
use crate::{
    Action, ActionChange, ActionDates, ActionView, Answer, Date, DaysOff, DaysOffKind, EditBase,
    Handled, Journal, ListKey, Note, NoteId, Occurrence, OccurrenceState, Question, QuestionKind,
    RecurrenceSettings, RepeatForm, Rule, Schedule, ShownTitles, Spaces, Style,
};

uniffi::custom_type!(Date, String, {
    lower: |date| date.to_string(),
    try_lift: |text| Ok(text.parse()?),
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

type CoreResult<T> = std::result::Result<T, crate::Error>;
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
    pub starred: Option<i64>,
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
    pub waiting: u32,
}

#[derive(uniffi::Record)]
pub struct MobileDaysOff {
    pub days: DaysOff,
    pub space: String,
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

#[derive(uniffi::Record)]
pub struct MobileDevice {
    pub address: String,
    pub name: String,
}

#[derive(uniffi::Record)]
pub struct MobileShare {
    pub devices: Vec<String>,
    pub id: String,
    pub library: String,
}

#[derive(uniffi::Record)]
pub struct MobileOffer {
    pub device: String,
    pub share: String,
}

#[derive(uniffi::Record)]
pub struct MobileSyncConfig {
    pub automatic: bool,
    pub devices: Vec<MobileDevice>,
    /// The offers that wait for the owner to accept or decline them.
    pub offers: Vec<MobileOffer>,
    pub relay: Option<String>,
    pub shares: Vec<MobileShare>,
}

impl From<Config> for MobileSyncConfig {
    fn from(config: Config) -> Self {
        Self {
            automatic: config.automatic,
            offers: config
                .open_offers()
                .map(|offer| MobileOffer {
                    device: offer.device.clone(),
                    share: offer.share.clone(),
                })
                .collect(),
            devices: config
                .devices
                .into_iter()
                .map(|device| MobileDevice { address: device.address, name: device.name })
                .collect(),
            relay: config.relay,
            shares: config
                .shares
                .into_iter()
                .map(|share| MobileShare {
                    devices: share.devices,
                    id: share.id,
                    library: share.library,
                })
                .collect(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct MobilePairingRequest {
    pub address: String,
    pub code: String,
    pub name: String,
}

#[derive(uniffi::Record)]
pub struct MobileOutgoingPairing {
    pub address: String,
    /// The code that the other device shows too. Pairing by invite needs none.
    pub code: Option<String>,
    pub state: MobilePairingState,
}

#[derive(uniffi::Enum)]
pub enum MobilePairingState {
    Waiting,
    Paired { name: String },
    Failed { reason: String },
}

#[derive(uniffi::Record)]
pub struct MobileSyncStatus {
    pub error: Option<String>,
    pub synced: Option<i64>,
    pub syncing: bool,
}

impl From<Status> for MobileSyncStatus {
    fn from(status: Status) -> Self {
        Self { error: status.error, synced: status.synced, syncing: status.syncing }
    }
}

/// Hears from sync on one of its own threads, so it has to return without waiting for the
/// notebook.
#[uniffi::export(callback_interface)]
pub trait SyncListener: Send + Sync {
    /// The configuration or a device's status changed.
    fn sync_changed(&self);

    /// Another device's changes merged into a library.
    fn libraries_changed(&self);
}

/// What the core keeps about a note that an editor shows.
#[derive(Default)]
struct Editor {
    /// The version of the library that the editor's text reflects.
    base: Option<EditBase>,
    titles: ShownTitles,
}

struct State {
    editors: HashMap<NoteId, Editor>,
    spaces: Spaces,
}

#[derive(uniffi::Object)]
pub struct MobileNotebook {
    state: Mutex<State>,
    syncer: Mutex<Option<Syncer>>,
}

impl MobileNotebook {
    fn state(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| MobileError::Failed {
            reason: "the library stopped after an internal error; restart Commonplace".into(),
        })
    }

    /// Changes the libraries, then tells sync which ones `change` committed to.
    fn write<T>(&self, change: impl FnOnce(&mut State) -> CoreResult<T>) -> Result<T> {
        let (result, changed) = {
            let mut state = self.state()?;
            let before = library_commits(&state.spaces);
            let result = change(&mut state);
            let changed: Vec<String> = library_commits(&state.spaces)
                .into_iter()
                .filter(|library| !before.contains(library))
                .map(|(key, _)| key)
                .collect();
            (result, changed)
        };
        if !changed.is_empty()
            && let Some(syncer) = self.syncer.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
        {
            for library in &changed {
                syncer.changed(library);
            }
        }
        Ok(result?)
    }

    fn with_syncer<T>(&self, call: impl FnOnce(&Syncer) -> CoreResult<T>) -> Result<T> {
        let syncer = self.syncer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let syncer = syncer
            .as_ref()
            .ok_or_else(|| MobileError::Failed { reason: "sync isn't running".into() })?;
        Ok(call(syncer)?)
    }

    fn forget_library(&self, library: &str) -> Result<()> {
        let syncer = self.syncer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match syncer.as_ref() {
            Some(syncer) => Ok(syncer.forget_library(library)?),
            None => Ok(()),
        }
    }
}

fn library_commits(spaces: &Spaces) -> Vec<(String, u64)> {
    spaces
        .keys()
        .into_iter()
        .filter_map(|key| spaces.library(&key).map(|library| (key.clone(), library.commits())))
        .collect()
}

/// Lets sync read and write the notebook's libraries.
struct NotebookHost {
    listener: Box<dyn SyncListener>,
    // A strong reference would keep the notebook alive through its own syncer.
    notebook: Weak<MobileNotebook>,
}

impl NotebookHost {
    fn notebook(&self) -> CoreResult<Arc<MobileNotebook>> {
        self.notebook.upgrade().ok_or_else(|| crate::Error::Sync("Commonplace is closing".into()))
    }
}

fn host_error(error: MobileError) -> crate::Error {
    let MobileError::Failed { reason } = error;
    crate::Error::Sync(reason)
}

impl Host for NotebookHost {
    fn version(&self, library: &str) -> CoreResult<Vec<u8>> {
        let notebook = self.notebook()?;
        let state = notebook.state().map_err(host_error)?;
        let library =
            state.spaces.library(library).ok_or(crate::Error::NoSuchLibrary(library.into()))?;
        Ok(library.version())
    }

    fn updates_since(&self, library: &str, version: &[u8]) -> CoreResult<Option<Vec<u8>>> {
        let notebook = self.notebook()?;
        let state = notebook.state().map_err(host_error)?;
        state
            .spaces
            .library(library)
            .ok_or(crate::Error::NoSuchLibrary(library.into()))?
            .updates_since(version)
    }

    fn merge_updates(&self, library: &str, updates: &[u8], repair: bool) -> CoreResult<()> {
        let notebook = self.notebook()?;
        notebook
            .write(|state| state.spaces.library_mut(library)?.merge_updates(updates, repair))
            .map_err(host_error)?;
        self.listener.libraries_changed();
        Ok(())
    }

    fn changed(&self) {
        self.listener.sync_changed();
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
            syncer: Mutex::default(),
        }))
    }

    /// Starts syncing, with the device's key and configuration kept in `directory`, unless sync
    /// already runs.
    pub fn start_sync(
        self: Arc<Self>,
        directory: String,
        listener: Box<dyn SyncListener>,
    ) -> Result<()> {
        let mut syncer = self.syncer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if syncer.is_none() {
            let host = NotebookHost { listener, notebook: Arc::downgrade(&self) };
            *syncer = Some(Syncer::start(Path::new(&directory), Arc::new(host))?);
        }
        Ok(())
    }

    /// Stops syncing, and closes connections to other devices.
    pub fn stop_sync(&self) {
        let syncer = self.syncer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        // Dropping the syncer waits for its connections to close, and a sync that merges into a
        // library in the meantime takes the lock.
        drop(syncer);
    }

    pub fn sync_running(&self) -> bool {
        self.syncer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some()
    }

    pub fn sync_address(&self) -> Result<String> {
        self.with_syncer(|syncer| Ok(syncer.address()))
    }

    pub fn sync_config(&self) -> Result<MobileSyncConfig> {
        self.with_syncer(|syncer| Ok(syncer.config().into()))
    }

    pub fn sync_status(&self, device: String) -> Result<MobileSyncStatus> {
        self.with_syncer(|syncer| Ok(syncer.status(&device).into()))
    }

    /// Whether a sync with any device is still running.
    pub fn syncing(&self) -> Result<bool> {
        self.with_syncer(|syncer| {
            let config = syncer.config();
            Ok(config.devices.iter().any(|device| syncer.status(&device.address).syncing))
        })
    }

    /// Trusts the device at `address`, and returns its key in its usual form.
    pub fn add_device(&self, address: String, name: String) -> Result<String> {
        self.with_syncer(|syncer| syncer.add_device(&address, &name))
    }

    pub fn rename_device(&self, address: String, name: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.rename_device(&address, &name))
    }

    pub fn remove_device(&self, address: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.remove_device(&address))
    }

    pub fn share_library(&self, library: String, device: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.share(&library, &device))
    }

    pub fn unshare_library(&self, library: String, device: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.unshare(&library, &device))
    }

    pub fn accept_offer(&self, share: String, device: String, library: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.accept(&share, &device, &library))
    }

    pub fn decline_offer(&self, share: String, device: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.decline(&share, &device))
    }

    pub fn set_sync_automatic(&self, automatic: bool) -> Result<()> {
        self.with_syncer(|syncer| syncer.set_automatic(automatic))
    }

    /// Sets the relay server to use instead of the public ones, from the next start.
    pub fn set_sync_relay(&self, relay: Option<String>) -> Result<()> {
        self.with_syncer(|syncer| syncer.set_relay(relay.as_deref()))
    }

    pub fn sync_now(&self) -> Result<()> {
        self.with_syncer(|syncer| {
            syncer.sync_now();
            Ok(())
        })
    }

    /// Reports that the network changed, so devices that were out of reach may answer now.
    pub fn sync_reconnect(&self) -> Result<()> {
        self.with_syncer(|syncer| {
            syncer.reconnect();
            Ok(())
        })
    }

    /// This device's name, which devices that pair with it learn.
    pub fn sync_name(&self) -> Result<String> {
        self.with_syncer(|syncer| Ok(syncer.name()))
    }

    pub fn set_sync_name(&self, name: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.set_name(&name))
    }

    /// Makes an invite link that pairs the device that opens it within 15 minutes, once, and
    /// shares `libraries` with it.
    pub fn sync_invite(&self, libraries: Vec<String>) -> Result<String> {
        self.with_syncer(|syncer| syncer.invite(&libraries))
    }

    /// Pairs with the device whose invite `link` is, in the background.
    pub fn open_sync_invite(&self, link: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.open_invite(&link))
    }

    /// Announces this device on the local network, and listens for others, while the Sync screen
    /// is open.
    pub fn start_nearby(&self) -> Result<()> {
        self.with_syncer(|syncer| syncer.start_nearby())
    }

    pub fn stop_nearby(&self) -> Result<()> {
        self.with_syncer(|syncer| {
            syncer.stop_nearby();
            Ok(())
        })
    }

    pub fn nearby_devices(&self) -> Result<Vec<MobileDevice>> {
        self.with_syncer(|syncer| {
            Ok(syncer
                .nearby()
                .into_iter()
                .map(|device| MobileDevice { address: device.address, name: device.name })
                .collect())
        })
    }

    pub fn pair_nearby(&self, address: String) -> Result<()> {
        self.with_syncer(|syncer| syncer.pair_nearby(&address))
    }

    pub fn pairing_requests(&self) -> Result<Vec<MobilePairingRequest>> {
        self.with_syncer(|syncer| {
            Ok(syncer
                .requests()
                .into_iter()
                .map(|request| MobilePairingRequest {
                    address: request.address,
                    code: request.code,
                    name: request.name,
                })
                .collect())
        })
    }

    pub fn answer_pairing(&self, address: String, accept: bool) -> Result<()> {
        self.with_syncer(|syncer| {
            syncer.answer(&address, accept);
            Ok(())
        })
    }

    /// The pairing that this device asked for most recently.
    pub fn outgoing_pairing(&self) -> Result<Option<MobileOutgoingPairing>> {
        self.with_syncer(|syncer| {
            Ok(syncer.outgoing().map(|outgoing| MobileOutgoingPairing {
                address: outgoing.address,
                code: outgoing.code,
                state: match outgoing.state {
                    OutgoingState::Waiting => MobilePairingState::Waiting,
                    OutgoingState::Paired(name) => MobilePairingState::Paired { name },
                    OutgoingState::Failed(reason) => MobilePairingState::Failed { reason },
                },
            }))
        })
    }

    /// Forgets the outgoing pairing once the owner has seen how it went.
    pub fn clear_outgoing_pairing(&self) -> Result<()> {
        self.with_syncer(|syncer| {
            syncer.clear_outgoing();
            Ok(())
        })
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
        let names = self.write(|state| crate::samples::populate(&mut state.spaces, today))?;
        Ok(names.into_iter().collect())
    }

    pub fn create_space(&self) -> Result<String> {
        self.write(|state| state.spaces.create())
    }

    pub fn remove_space(&self, key: String) -> Result<()> {
        self.write(|state| state.spaces.remove(&key))?;
        self.forget_library(&key)
    }

    pub fn merge_spaces(&self, source: String, target: String) -> Result<()> {
        self.write(|state| state.spaces.merge(&source, &target))?;
        self.forget_library(&source)
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
                waiting: entry.waiting,
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
                waiting: entry.waiting,
            })
            .collect())
    }

    pub fn action_dates(&self, id: NoteId, today: Date) -> Result<ActionDates> {
        Ok(self.state()?.spaces.action_dates(today).remove(&id).unwrap_or_default())
    }

    pub fn all_action_dates(&self, today: Date) -> Result<HashMap<NoteId, ActionDates>> {
        Ok(self.state()?.spaces.action_dates(today))
    }

    pub fn action_paths(&self) -> Result<HashMap<NoteId, Vec<NoteId>>> {
        Ok(self.state()?.spaces.action_paths())
    }

    pub fn create_memo(&self, space: String, body: String) -> Result<NoteId> {
        self.write(|state| state.spaces.create_memo(&space, &body))
    }

    pub fn create_action(&self, space: String, body: String) -> Result<NoteId> {
        self.write(|state| state.spaces.create_inbox_task(&space, &body))
    }

    pub fn create_step(&self, parent: NoteId, body: String) -> Result<NoteId> {
        self.write(|state| state.spaces.create_task(&body, parent))
    }

    pub fn editor_body(&self, id: NoteId) -> Result<String> {
        let mut state = self.state()?;
        let note = state.spaces.note(id).ok_or(crate::Error::NotFound(id))?;
        let mut titles = ShownTitles::default();
        let body = titles.expand(&note.body, |id| state.spaces.link_title(id));
        let base = state.spaces.edit_base(id);
        state.editors.insert(id, Editor { base, titles });
        Ok(body)
    }

    pub fn editable_label(&self, id: NoteId) -> Result<String> {
        Ok(self.state()?.spaces.editable_label(id).ok_or(crate::Error::NotFound(id))?)
    }

    pub fn set_label(&self, id: NoteId, text: String) -> Result<()> {
        self.write(|state| state.spaces.set_label(id, &text))
    }

    pub fn link_text(&self, source: NoteId, target: NoteId) -> Result<String> {
        let mut state = self.state()?;
        let mut editor = state.editors.remove(&source).unwrap_or_default();
        let mut text = crate::wikilink(target, None);
        for (range, replacement) in editor.titles.updates(&text, |id| state.spaces.link_title(id)) {
            text.replace_range(range, &replacement);
        }
        state.editors.insert(source, editor);
        Ok(text)
    }

    /// Saves an editor's text, and returns whether changes from elsewhere, such as another device,
    /// merged into it, so the editor should load the body again.
    pub fn set_body(&self, id: NoteId, text: String) -> Result<bool> {
        self.write(|state| {
            let editor = state.editors.entry(id).or_default();
            let body = editor.titles.collapse(&text, |id| state.spaces.link_title(id));
            let Some(base) = editor.base.clone() else {
                state.spaces.set_body(id, &body)?;
                return Ok(false);
            };
            let (base, merged) = state.spaces.set_body_since(id, &body, &base)?;
            if let Some(editor) = state.editors.get_mut(&id) {
                editor.base = Some(base);
            }
            Ok(merged)
        })
    }

    /// A note as of `today`, with a recurring action's dates and its steps' states for the current
    /// occurrence.
    pub fn note_on(&self, id: NoteId, today: Date) -> Result<Option<MobileNote>> {
        let state = self.state()?;
        Ok(state.spaces.note_on(id, today).map(|note| mobile_note(&state.spaces, note)))
    }

    pub fn check_action(&self, id: NoteId, done: bool, today: Date) -> Result<()> {
        self.write(|state| state.spaces.check_action(id, done, today))
    }

    pub fn set_recurrence(
        &self,
        id: NoteId,
        settings: Option<RecurrenceSettings>,
        today: Date,
    ) -> Result<()> {
        self.write(|state| state.spaces.set_recurrence(id, settings, today))
    }

    pub fn handle_occurrence(
        &self,
        id: NoteId,
        date: Date,
        state: Option<OccurrenceState>,
        today: Date,
    ) -> Result<()> {
        self.write(|spaces| spaces.spaces.handle_occurrence(id, date, state, today))
    }

    pub fn move_occurrence(&self, id: NoteId, date: Date, to: Option<Date>) -> Result<()> {
        self.write(|state| state.spaces.move_occurrence(id, date, to))
    }

    pub fn write_occurrence_note(&self, id: NoteId, date: Date) -> Result<NoteId> {
        self.write(|state| state.spaces.write_occurrence_note(id, date))
    }

    pub fn current_occurrence(&self, id: NoteId, today: Date) -> Result<Option<Occurrence>> {
        Ok(self.state()?.spaces.current_occurrence(id, today))
    }

    pub fn occurrences(
        &self,
        id: NoteId,
        today: Date,
        past: u32,
        upcoming: u32,
    ) -> Result<Vec<Occurrence>> {
        Ok(self.state()?.spaces.occurrences(id, today, past, upcoming))
    }

    /// The dates the first `count` occurrences on or after `from` would fall on, with the days off
    /// of `id`'s library, or of `space`.
    pub fn preview_dates(
        &self,
        id: Option<NoteId>,
        space: Option<String>,
        settings: RecurrenceSettings,
        from: Date,
        count: u32,
    ) -> Result<Vec<Date>> {
        let state = self.state()?;
        Ok(state.spaces.preview_dates(id, space.as_deref(), &settings, from, count))
    }

    /// The days off of every library that shows.
    pub fn days_off(&self) -> Result<Vec<MobileDaysOff>> {
        let days = self.state()?.spaces.days_off();
        Ok(days.into_iter().map(|(space, days)| MobileDaysOff { days, space }).collect())
    }

    /// The days off that apply to a note.
    pub fn days_off_of(&self, id: NoteId) -> Result<Vec<DaysOff>> {
        Ok(self.state()?.spaces.days_off_of(id))
    }

    pub fn set_days_off(&self, space: String, days: DaysOff) -> Result<()> {
        self.write(|state| state.spaces.set_days_off(&space, &days))
    }

    pub fn archive_days_off(
        &self,
        space: String,
        kind: DaysOffKind,
        id: String,
        archived: bool,
        today: Date,
    ) -> Result<()> {
        self.write(|state| state.spaces.archive_days_off(&space, kind, &id, archived, today))
    }

    pub fn change_action(&self, id: NoteId, change: ActionChange) -> Result<()> {
        self.write(|state| state.spaces.change_action(id, change))
    }

    pub fn move_action(
        &self,
        id: NoteId,
        parent: Option<NoteId>,
        index: Option<u32>,
    ) -> Result<()> {
        self.write(|state| state.spaces.move_action(id, parent, index.map(|n| n as usize)))
    }

    pub fn move_action_beside(&self, id: NoteId, sibling: NoteId, after: bool) -> Result<()> {
        self.write(|state| state.spaces.move_action_beside(id, sibling, after))
    }

    pub fn reorder_action(&self, id: NoteId, up: bool) -> Result<()> {
        self.write(|state| state.spaces.move_action_among_siblings(id, up))
    }

    pub fn move_to_space(&self, id: NoteId, space: String) -> Result<()> {
        self.write(|state| state.spaces.move_to_space(id, &space))
    }

    pub fn set_archived(&self, id: NoteId, archived: bool) -> Result<()> {
        self.write(|state| state.spaces.set_archived(id, archived))
    }

    pub fn set_starred(&self, id: NoteId, starred: bool) -> Result<()> {
        self.write(|state| state.spaces.set_starred(id, starred))
    }

    pub fn trash(&self, id: NoteId) -> Result<()> {
        self.write(|state| state.spaces.trash(id))
    }

    pub fn restore(&self, id: NoteId) -> Result<()> {
        self.write(|state| state.spaces.restore(id))
    }

    pub fn delete(&self, id: NoteId) -> Result<()> {
        self.write(|state| state.spaces.delete(id))
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
        self.write(|state| state.spaces.create_journal(&space, &name))
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
        self.write(|state| state.spaces.set_journal_color(id, &color))
    }

    pub fn set_question(&self, id: NoteId, question: Question, new: bool) -> Result<()> {
        self.write(|state| {
            if new {
                state.spaces.add_question(id, &question)
            } else {
                state.spaces.set_question(id, &question)
            }
        })
    }

    pub fn move_question(&self, id: NoteId, question: String, up: bool) -> Result<()> {
        self.write(|state| state.spaces.move_question(id, &question, up))
    }

    pub fn set_schedule(&self, id: NoteId, schedule: Schedule) -> Result<()> {
        self.write(|state| state.spaces.set_schedule(id, &schedule))
    }

    pub fn remove_schedule(&self, id: NoteId, schedule: String) -> Result<()> {
        self.write(|state| state.spaces.remove_schedule(id, &schedule))
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
        let answers = answers.into_iter().collect();
        self.write(|state| state.spaces.create_entry(journal, &body, at, prompt, &answers, today))
    }

    pub fn set_entry_at(&self, id: NoteId, at: i64) -> Result<()> {
        self.write(|state| state.spaces.set_entry_at(id, at))
    }

    pub fn set_answer(&self, id: NoteId, question: String, answer: Option<Answer>) -> Result<()> {
        self.write(|state| state.spaces.set_answer(id, &question, answer.as_ref()))
    }

    pub fn convert_memo(&self, memo: NoteId, journal: NoteId) -> Result<NoteId> {
        self.write(|state| state.spaces.convert_memo(memo, journal))
    }

    pub fn handle_prompt(
        &self,
        journal: NoteId,
        key: String,
        handled: Handled,
        today: Date,
    ) -> Result<()> {
        self.write(|state| state.spaces.handle_prompt(journal, &key, handled, today))
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
                for minute in state.spaces.schedule_minutes(note.id, schedule, date) {
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
                let prompt = state.spaces.due_prompt(note.id, today, minute, now, |_, at| {
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
        self.write(|state| state.spaces.set_note_setting(id, &key, value))
    }

    pub fn note_setting(&self, id: NoteId, key: String) -> Result<Option<i64>> {
        Ok(self.state()?.spaces.note_setting(id, &key))
    }

    pub fn remove_note_setting(&self, id: NoteId, key: String) -> Result<()> {
        self.write(|state| state.spaces.remove_note_setting(id, &key))
    }

    pub fn shows_done_steps(&self, id: NoteId) -> Result<bool> {
        Ok(self.state()?.spaces.shows_done_steps(id))
    }

    pub fn set_shows_done_steps(&self, id: NoteId, shown: bool) -> Result<()> {
        self.write(|state| state.spaces.set_shows_done_steps(id, shown))
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
        starred: note.starred,
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

#[uniffi::export]
pub fn answer_placeholder(kind: QuestionKind) -> String {
    kind.placeholder()
}

#[uniffi::export]
pub fn parse_answer(kind: QuestionKind, text: String) -> Option<Answer> {
    kind.parse_answer(&text)
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

/// Whether `text` is a link that invites this device to pair with another.
#[uniffi::export]
pub fn is_sync_invite(text: String) -> bool {
    Syncer::is_invite(&text)
}

#[cfg(test)]
mod tests;

/// A short description of a rule, the same as the Linux app shows.
#[uniffi::export]
pub fn describe_rule(rule: Rule, start: Date) -> String {
    crate::describe_rule(&rule, start)
}

/// The rules the repeat menu offers before Custom, for something that starts on `start`.
#[uniffi::export]
pub fn repeat_presets(start: Date) -> Vec<Rule> {
    crate::repeat_presets(start)
}

/// The repeat editor's form for `rule`, or nothing if the editor can't show it.
#[uniffi::export]
pub fn repeat_form(rule: Rule, start: Date) -> Option<RepeatForm> {
    RepeatForm::from_rule(&rule, start)
}

#[uniffi::export]
pub fn new_repeat_form(start: Date) -> RepeatForm {
    RepeatForm::new(start)
}

#[uniffi::export]
pub fn repeat_form_rule(form: RepeatForm) -> Rule {
    form.rule()
}

/// "first" to "fifth", or "last."
#[uniffi::export]
pub fn ordinal_word(ordinal: i8) -> String {
    crate::ordinal_word(ordinal)
}

#[uniffi::export]
pub fn new_days_off(kind: DaysOffKind, name: String, first: Date) -> DaysOff {
    DaysOff::new(kind, &name, first)
}

/// When days off fall, as the Linux app shows it.
#[uniffi::export]
pub fn describe_days_off(days: DaysOff) -> String {
    days.describe()
}

#[uniffi::export]
pub fn days_off_hidden(days: DaysOff, today: Date) -> bool {
    days.is_hidden(today)
}

#[uniffi::export]
pub fn days_off_kind_name(kind: DaysOffKind) -> String {
    kind.name().to_owned()
}
