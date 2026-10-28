// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Error, NoteId};
use gtk::{gdk, gio, glib};

use crate::answers::AnswerForm;
use crate::editor;
use crate::journal_capture::{PromptContext, journal_name};
use crate::rows::today;
use crate::state::State;

/// The widget name that marks a capture window.
const NAME: &str = "capture";

/// The window action that saves a capture and closes its window.
const SAVE_ACTION: &str = "capture";

/// Whether `window` is a capture window.
pub fn is_capture(window: &gtk::Window) -> bool {
    window.widget_name() == NAME
}

/// Gives the capture window's save its keys. They're the same as `win.toggle-checkbox`, which
/// only the main window has.
pub fn add_accels(app: &adw::Application) {
    app.set_accels_for_action(
        &format!("win.{SAVE_ACTION}"),
        &["<Control>Return", "<Control>KP_Enter"],
    );
}

/// Saves the text in the capture window `window` and closes it.
pub fn save(window: &gtk::Window) -> Result<(), glib::BoolError> {
    window.activate_action(&format!("win.{SAVE_ACTION}"), None)
}

/// A header bar for a capture window, which has no buttons to close or resize it.
pub fn header() -> adw::HeaderBar {
    adw::HeaderBar::builder().show_start_title_buttons(false).show_end_title_buttons(false).build()
}

/// Builds a capture window around `header` and `content`, with Discard and Save buttons. Saving
/// runs `save` and closes the window, or keeps it open with a dialog that `failure` titles if
/// `save` fails, and closing it any other way discards what it holds. Alt with a digit turns on
/// that one of the buttons that `choices` gives.
pub fn window(
    app: &adw::Application,
    header: &adw::HeaderBar,
    content: &impl IsA<gtk::Widget>,
    size: (i32, i32),
    choices: impl Fn() -> Vec<gtk::ToggleButton> + 'static,
    failure: impl Fn() -> &'static str + 'static,
    save: impl Fn() -> Result<(), Error> + 'static,
) -> adw::ApplicationWindow {
    let discard = gtk::Button::builder()
        .label("_Discard")
        .use_underline(true)
        .tooltip_text("Discard (Escape)")
        .build();
    let save_button = gtk::Button::builder()
        .label("_Save")
        .use_underline(true)
        .tooltip_text("Save (Ctrl+Return)")
        .css_classes(["suggested-action"])
        .build();
    let actions = gtk::ActionBar::new();
    actions.pack_start(&discard);
    actions.pack_end(&save_button);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(header);
    toolbar.add_bottom_bar(&actions);
    toolbar.set_content(Some(content));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .name(NAME)
        // The README's Sway rule floats this window by its exact title. Keep it unlike any
        // `window_title`, which the main window shows for notes of any label.
        .title("Quick capture")
        .default_width(size.0)
        .default_height(size.1)
        .content(&toolbar)
        .build();

    let saving = Rc::new(Cell::new(false));
    window.connect_close_request(glib::clone!(
        #[strong]
        saving,
        move |window| {
            if !saving.replace(false) {
                return glib::Propagation::Proceed;
            }
            match save() {
                Ok(()) => glib::Propagation::Proceed,
                Err(error) => {
                    let dialog = adw::AlertDialog::new(Some(failure()), Some(&error.to_string()));
                    dialog.add_response("ok", "_OK");
                    dialog.present(Some(window));
                    glib::Propagation::Stop
                }
            }
        }
    ));
    let save_and_close = glib::clone!(
        #[weak]
        window,
        move || {
            saving.set(true);
            window.close();
        }
    );
    discard.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    save_button.connect_clicked(glib::clone!(
        #[strong]
        save_and_close,
        move |_| save_and_close()
    ));
    // It isn't named `save`, because Control+S activates `win.save` and mustn't save a capture.
    let save_action = gio::SimpleAction::new(SAVE_ACTION, None);
    save_action.connect_activate(move |_, _| save_and_close());
    window.add_action(&save_action);
    let toggle_space = gio::SimpleAction::new("toggle-space", Some(glib::VariantTy::INT32));
    toggle_space.connect_activate(move |_, parameter| {
        let index = parameter.and_then(|value| value.get::<i32>()).unwrap_or(0) - 1;
        let choices = choices();
        if let Some(button) = usize::try_from(index).ok().and_then(|index| choices.get(index)) {
            button.set_active(true);
        }
    });
    window.add_action(&toggle_space);

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            window.close();
            glib::Propagation::Stop
        }
    ));
    window.add_controller(keys);
    window
}

/// The kinds of note that capture creates, in the order its buttons show them.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Memo,
    Action,
    Entry,
}

/// Opens a small window that saves its text as a new memo, a new task in the inbox, or a new
/// journal entry, starting on `kind`. A memo or task goes in the library its library buttons
/// pick, and an entry in the journal its journal buttons pick, starting on `journal` or the
/// journal capture last saved into. With `prompt`, it answers that prompt, in the prompt's journal.
/// Returns `None` when an entry has no journal to go in.
pub fn present(
    app: &adw::Application,
    state: Rc<State>,
    kind: Kind,
    journal: Option<NoteId>,
    prompt: Option<PromptContext>,
) -> Option<adw::ApplicationWindow> {
    let mut journals = state.read(|spaces| spaces.journals());
    if let Some(prompt) = &prompt
        && !journals.iter().any(|journal| journal.id == prompt.journal)
    {
        journals.extend(state.read(|spaces| spaces.note(prompt.journal)));
    }
    // A prompt answers only in its own journal, which may be gone by the time the owner answers.
    if let Some(prompt) = &prompt
        && !journals.iter().any(|journal| journal.id == prompt.journal && journal.journal.is_some())
    {
        return None;
    }
    let start = prompt
        .as_ref()
        .map(|prompt| prompt.journal)
        .or(journal)
        .or_else(|| state.capture_journal())
        .filter(|id| journals.iter().any(|journal| journal.id == *id))
        .or_else(|| journals.first().map(|journal| journal.id));
    if kind == Kind::Entry && start.is_none() {
        return None;
    }
    let opened = glib::DateTime::now_utc().map(|now| now.to_unix() * 1000).unwrap_or_default();
    let day = glib::DateTime::from_unix_local(opened.div_euclid(1000))
        .and_then(|time| time.format("%Y-%m-%d"))
        .map(String::from)
        .unwrap_or_default();

    let view = editor::new_view();
    let scrolled = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
    let form_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(6)
        .build();
    // Scrolling keeps a long form from pushing the text and the buttons out of the window, which
    // doesn't grow when a switch to an entry adds the form.
    let form_holder = gtk::ScrolledWindow::builder()
        .child(&form_box)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(220)
        .visible(false)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = header();
    let choices = gtk::Box::builder()
        .spacing(6)
        .margin_start(6)
        .margin_end(6)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    content.append(&choices);
    content.append(&form_holder);
    content.append(&scrolled);

    let space = Rc::new(RefCell::new(state.capture_space()));
    let spaces = state.spaces();
    let space_bar = gtk::Box::builder().spacing(6).build();
    let mut space_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for (index, choice) in spaces.iter().enumerate() {
        let button = gtk::ToggleButton::builder()
            .label(&choice.name)
            .tooltip_text(format!("Save in {} (Alt+{})", choice.name, index + 1))
            .active(choice.key == *space.borrow())
            .build();
        button.set_group(space_buttons.first());
        let key = choice.key.clone();
        button.connect_toggled(glib::clone!(
            #[strong]
            space,
            move |button| {
                if button.is_active() {
                    *space.borrow_mut() = key.clone();
                }
            }
        ));
        space_bar.append(&button);
        space_buttons.push(button);
    }
    choices.append(&space_bar);

    let current = Rc::new(Cell::new(start));
    let form: Rc<RefCell<Option<Rc<AnswerForm>>>> = Rc::default();
    let template = Rc::new(RefCell::new(String::new()));
    let kind = Rc::new(Cell::new(kind));
    let title = adw::WindowTitle::new("Prompt", "");
    let switch_journal = glib::clone!(
        #[strong]
        state,
        #[strong]
        current,
        #[strong]
        form,
        #[strong]
        template,
        #[strong]
        kind,
        #[weak]
        form_box,
        #[weak]
        form_holder,
        #[weak]
        view,
        #[weak]
        title,
        move |id: NoteId| {
            current.set(Some(id));
            let Some(note) = state.read(|spaces| spaces.note(id)) else {
                return;
            };
            let Some(fields) = note.journal.as_ref() else {
                return;
            };
            title.set_subtitle(&journal_name(&note));
            let new_form = AnswerForm::new(fields);
            if let Some(old) = form.replace(Some(new_form.clone())) {
                form_box.remove(&old.widget);
            }
            form_box.append(&new_form.widget);
            form_holder.set_visible(kind.get() == Kind::Entry && new_form.widget.get_visible());
            let new_template = format!("{day}\n\n{}", fields.template());
            let text = editor::text(&view.buffer());
            if kind.get() == Kind::Entry && text.trim() == template.borrow().trim() {
                crate::link_icons::load(&view, &new_template);
            }
            template.replace(new_template);
        }
    );
    let journal_bar = gtk::Box::builder().spacing(6).build();
    let mut journal_buttons: Vec<gtk::ToggleButton> = Vec::new();
    for (index, journal) in journals.iter().enumerate() {
        let name = journal_name(journal);
        let button = gtk::ToggleButton::builder()
            .label(&name)
            .tooltip_text(match index {
                0..9 => format!("Write in {name} (Alt+{})", index + 1),
                _ => format!("Write in {name}"),
            })
            .active(Some(journal.id) == start)
            .build();
        button.set_group(journal_buttons.first());
        let id = journal.id;
        let switch_journal = switch_journal.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                switch_journal(id);
            }
        });
        journal_bar.append(&button);
        journal_buttons.push(button);
    }
    choices.append(&journal_bar);

    let journal_count = journals.len();
    let prompted = prompt.is_some();
    let show_kind = Rc::new(glib::clone!(
        #[strong]
        kind,
        #[strong]
        form,
        #[strong]
        template,
        #[weak]
        view,
        #[weak]
        space_bar,
        #[weak]
        journal_bar,
        #[weak]
        choices,
        #[weak]
        form_holder,
        move |new: Kind| {
            let old = kind.replace(new);
            let entry = new == Kind::Entry;
            space_bar.set_visible(!entry && spaces.len() > 1);
            journal_bar.set_visible(entry && journal_count > 1 && !prompted);
            choices.set_visible(space_bar.get_visible() || journal_bar.get_visible());
            let questions = form.borrow().as_ref().is_some_and(|form| form.widget.get_visible());
            form_holder.set_visible(entry && questions);
            let text = editor::text(&view.buffer());
            if old == Kind::Entry && !entry && text.trim() == template.borrow().trim() {
                crate::link_icons::load(&view, "");
            } else if old != Kind::Entry && entry && text.trim().is_empty() {
                crate::link_icons::load(&view, &template.borrow());
            }
        }
    ));
    if let Some(id) = start {
        switch_journal(id);
    }
    if prompt.is_some() {
        header.set_title_widget(Some(&title));
    } else {
        let kinds = gtk::Box::builder().css_classes(["linked"]).build();
        let mut kind_buttons: Vec<gtk::ToggleButton> = Vec::new();
        let offered = [("Memo", Kind::Memo), ("Action", Kind::Action), ("Entry", Kind::Entry)];
        for (label, choice) in offered {
            if choice == Kind::Entry && journal_count == 0 {
                continue;
            }
            let button =
                gtk::ToggleButton::builder().label(label).active(choice == kind.get()).build();
            button.set_group(kind_buttons.first());
            button.connect_toggled(glib::clone!(
                #[strong]
                show_kind,
                move |button| {
                    if button.is_active() {
                        show_kind(choice);
                    }
                }
            ));
            kinds.append(&button);
            kind_buttons.push(button);
        }
        header.set_title_widget(Some(&kinds));
    }
    // Starting from the other kind runs the switch, which loads an entry's template.
    let first = kind.replace(if kind.get() == Kind::Entry { Kind::Memo } else { Kind::Entry });
    crate::link_icons::load(&view, "");
    show_kind(first);

    let failure = glib::clone!(
        #[strong]
        kind,
        move || match kind.get() {
            Kind::Memo => "Couldn’t save the memo",
            Kind::Action => "Couldn’t save the action",
            Kind::Entry => "Couldn’t save the entry",
        }
    );
    let save = glib::clone!(
        #[weak]
        view,
        #[strong]
        kind,
        #[upgrade_or]
        Ok(()),
        move || {
            let text = editor::text(&view.buffer());
            if kind.get() != Kind::Entry {
                if text.trim().is_empty() {
                    return Ok(());
                }
                let space = space.borrow().clone();
                state.set_capture_space(&space);
                let text = state.read(|spaces| editor::collapse_titles(spaces, &text));
                let task = kind.get() == Kind::Action;
                return state
                    .write(|spaces| {
                        if task {
                            spaces.create_inbox_task(&space, &text)
                        } else {
                            spaces.create_memo(&space, &text)
                        }
                    })
                    .map(|_| ());
            }
            let Some(journal) = current.get() else {
                return Ok(());
            };
            let answers = form.borrow().as_ref().map(|form| form.answers()).unwrap_or_default();
            let unwritten = text.trim().is_empty() || text.trim() == template.borrow().trim();
            if unwritten && answers.is_empty() {
                return Ok(());
            }
            state.set_capture_journal(journal);
            let text = state.read(|spaces| editor::collapse_titles(spaces, &text));
            let prompt = prompt.as_ref().map(|prompt| (prompt.key.as_str(), prompt.prompted));
            state
                .write(|spaces| {
                    spaces.create_entry(journal, &text, opened, prompt, &answers, today())
                })
                .map(|_| ())
        }
    );
    let pick = glib::clone!(
        #[strong]
        kind,
        move || {
            match kind.get() {
                // A prompt answers only in its own journal.
                Kind::Entry if prompted => Vec::new(),
                Kind::Entry => journal_buttons.clone(),
                _ => space_buttons.clone(),
            }
        }
    );
    let window = window(app, &header, &content, (520, 480), pick, failure, save);
    window.present();
    view.grab_focus();
    Some(window)
}
