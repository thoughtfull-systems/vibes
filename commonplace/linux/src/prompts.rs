// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Date, Handled, Journal, Note, NoteId, Prompt};
use gtk::{gio, glib};

use crate::capture::{self, Kind};
use crate::journal_capture::PromptContext;
use crate::rows::today;
use crate::state::State;

/// How often the app looks for prompts that have come due.
const INTERVAL_SECONDS: u32 = 20;

/// The notifications showing, by journal: the prompt each shows and when it came due.
type Shown = Rc<RefCell<HashMap<NoteId, (String, i64)>>>;

/// The journals as of a count of the libraries' commits, so a check reads them again only after
/// they change.
type Journals = Rc<RefCell<Option<(u64, Vec<Note>)>>>;

/// Sends a notification for each journal that prompts on this device when its latest prompt comes
/// due, and handles the notifications' actions.
pub fn start(app: &adw::Application, state: Rc<State>) {
    let shown: Shown = Rc::default();
    let prompt_type = glib::VariantTy::new("(ssx)").expect("the type string is valid");
    let answer = gio::SimpleAction::new("answer-prompt", Some(prompt_type));
    answer.connect_activate(glib::clone!(
        #[weak]
        app,
        #[strong]
        state,
        #[strong]
        shown,
        move |_, parameter| {
            let Some((journal, key, prompted)) = parameter.and_then(prompt_parameter) else {
                return;
            };
            app.withdraw_notification(&notification_id(journal));
            let context = PromptContext { journal, key, prompted };
            if let Some(window) =
                capture::present(&app, state.clone(), Kind::Entry, None, Some(context))
            {
                // Closing the window without saving leaves the prompt unhandled, so it comes back.
                window.connect_destroy(glib::clone!(
                    #[strong]
                    shown,
                    move |_| {
                        shown.borrow_mut().remove(&journal);
                    }
                ));
            }
        }
    ));
    app.add_action(&answer);
    let snooze_type = glib::VariantTy::new("(ssx)").expect("the type string is valid");
    let snooze = gio::SimpleAction::new("snooze-prompt", Some(snooze_type));
    snooze.connect_activate(glib::clone!(
        #[weak]
        app,
        #[strong]
        state,
        move |_, parameter| {
            let Some((journal, key, minutes)) = parameter.and_then(prompt_parameter) else {
                return;
            };
            let until = now() + minutes * 60_000;
            handle(&app, &state, journal, &key, Handled::Snoozed { until });
        }
    ));
    app.add_action(&snooze);
    let skip_type = glib::VariantTy::new("(ssx)").expect("the type string is valid");
    let skip = gio::SimpleAction::new("skip-prompt", Some(skip_type));
    skip.connect_activate(glib::clone!(
        #[weak]
        app,
        #[strong]
        state,
        move |_, parameter| {
            if let Some((journal, key, _)) = parameter.and_then(prompt_parameter) {
                handle(&app, &state, journal, &key, Handled::Skipped);
            }
        }
    ));
    app.add_action(&skip);

    let journals: Journals = Rc::default();
    let check = glib::clone!(
        #[weak]
        app,
        #[strong]
        state,
        #[strong]
        shown,
        move || check(&app, &state, &shown, &journals)
    );
    check();
    glib::timeout_add_seconds_local(INTERVAL_SECONDS, move || {
        check();
        glib::ControlFlow::Continue
    });
}

/// Records what happened to a prompt, and takes its notification away.
fn handle(app: &adw::Application, state: &State, journal: NoteId, key: &str, handled: Handled) {
    app.withdraw_notification(&notification_id(journal));
    if let Err(error) = state.write(|spaces| spaces.handle_prompt(journal, key, handled, today())) {
        eprintln!("commonplace: couldn't record the prompt: {error}");
    }
}

/// Shows each journal's due prompt, unless it already shows, and takes away notifications for
/// prompts that something else has handled, such as another device.
fn check(app: &adw::Application, state: &State, shown: &Shown, cache: &Journals) {
    let Ok(local) = glib::DateTime::now_local() else {
        return;
    };
    let today = today();
    let minute = (local.hour() * 60 + local.minute()) as u16;
    let now = now();
    let commits = state.read(|spaces| spaces.commits());
    if cache.borrow().as_ref().is_none_or(|(seen, _)| *seen != commits) {
        cache.replace(Some((commits, state.read(|spaces| spaces.all_journals()))));
    }
    let journals: Vec<Note> = cache
        .borrow()
        .iter()
        .flat_map(|(_, journals)| journals)
        .filter(|journal| state.prompts_here(journal.id))
        .cloned()
        .collect();
    let mut due: HashMap<NoteId, (Prompt, i64)> = HashMap::new();
    for journal in &journals {
        let Some(fields) = &journal.journal else {
            continue;
        };
        let prompt =
            state.read(|spaces| spaces.due_prompt(journal.id, today, minute, now, local_millis));
        if let Some(prompt) = prompt {
            let at = due_at(fields, &prompt);
            due.insert(journal.id, (prompt, at));
        }
    }
    let mut shown = shown.borrow_mut();
    shown.retain(|journal, (key, at)| {
        let still = due.get(journal).is_some_and(|(prompt, due)| prompt.key == *key && due == at);
        if !still {
            app.withdraw_notification(&notification_id(*journal));
        }
        still
    });
    for journal in &journals {
        let Some((prompt, at)) = due.remove(&journal.id) else {
            continue;
        };
        if shown.contains_key(&journal.id) {
            continue;
        }
        app.send_notification(Some(&notification_id(journal.id)), &notification(journal, &prompt));
        shown.insert(journal.id, (prompt.key, at));
    }
}

/// When a prompt came due: when its snooze ran out, or else at its scheduled time.
fn due_at(journal: &Journal, prompt: &Prompt) -> i64 {
    match journal.handled.get(&prompt.key) {
        Some(Handled::Snoozed { until }) => *until,
        _ => local_millis(prompt.date, prompt.minute),
    }
}

/// A notification that names the journal, shows its first question, and offers to answer,
/// snooze, or skip the prompt.
fn notification(journal: &Note, prompt: &Prompt) -> gio::Notification {
    let name = journal.label().unwrap_or_else(|| "Untitled journal".to_owned());
    let notification = gio::Notification::new(&name);
    let question = journal
        .journal
        .as_ref()
        .and_then(|fields| fields.current_questions().next().map(|q| q.prompt.trim().to_owned()));
    notification.set_body(Some(question.as_deref().unwrap_or("Time for an entry")));
    notification.set_priority(gio::NotificationPriority::High);
    let target = |number: i64| (journal.id.to_string(), prompt.key.clone(), number).to_variant();
    let prompted = local_millis(prompt.date, prompt.minute);
    notification.set_default_action_and_target_value("app.answer-prompt", Some(&target(prompted)));
    notification.add_button_with_target_value(
        "Answer",
        "app.answer-prompt",
        Some(&target(prompted)),
    );
    notification.add_button_with_target_value("15 Minutes", "app.snooze-prompt", Some(&target(15)));
    notification.add_button_with_target_value("1 Hour", "app.snooze-prompt", Some(&target(60)));
    notification.add_button_with_target_value("Skip", "app.skip-prompt", Some(&target(0)));
    notification
}

fn notification_id(journal: NoteId) -> String {
    format!("prompt-{journal}")
}

/// The journal, prompt key, and number in an action's parameter.
fn prompt_parameter(parameter: &glib::Variant) -> Option<(NoteId, String, i64)> {
    let (journal, key, number) = parameter.get::<(String, String, i64)>()?;
    Some((journal.parse().ok()?, key, number))
}

/// The timestamp of a local date and time of day, in minutes since midnight.
pub fn local_millis(date: Date, minute: u16) -> i64 {
    glib::DateTime::from_local(
        i32::from(date.year()),
        i32::from(date.month()),
        i32::from(date.day()),
        i32::from(minute / 60),
        i32::from(minute % 60),
        0.0,
    )
    .map(|time| time.to_unix() * 1000)
    .unwrap_or_default()
}

fn now() -> i64 {
    glib::real_time() / 1000
}
