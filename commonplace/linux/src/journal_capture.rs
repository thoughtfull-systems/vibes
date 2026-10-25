// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Note, NoteId, parse_note_uri};
use gtk::{gdk, gio, glib};

use crate::answers::AnswerForm;
use crate::capture;
use crate::editor;
use crate::rows::today;
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
fn journal_name(journal: &Note) -> String {
    journal.label().unwrap_or_else(|| "Untitled journal".to_owned())
}

/// Opens a small window that saves a new entry in the journal its buttons pick, starting on
/// `journal` or the journal capture last saved into. With `prompt`, it answers that prompt, in
/// the prompt's journal. Returns `None` when there's no journal to capture into.
pub fn present(
    app: &adw::Application,
    state: Rc<State>,
    journal: Option<NoteId>,
    prompt: Option<PromptContext>,
) -> Option<adw::ApplicationWindow> {
    let mut journals = state.read(|spaces| spaces.journals());
    if let Some(prompt) = &prompt
        && !journals.iter().any(|journal| journal.id == prompt.journal)
    {
        journals.extend(state.read(|spaces| spaces.note(prompt.journal)));
    }
    let start = prompt
        .as_ref()
        .map(|prompt| prompt.journal)
        .or(journal)
        .or_else(|| state.capture_journal())
        .filter(|id| journals.iter().any(|journal| journal.id == *id))
        .or_else(|| journals.first().map(|journal| journal.id))?;
    let opened = glib::DateTime::now_utc().map(|now| now.to_unix() * 1000).unwrap_or_default();

    let view = editor::new_view();
    let scrolled = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
    let form_holder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(6)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&form_holder);
    content.append(&scrolled);
    let discard = gtk::Button::builder()
        .label("_Discard")
        .use_underline(true)
        .tooltip_text("Discard (Escape)")
        .build();
    let save = gtk::Button::builder()
        .label("_Save")
        .use_underline(true)
        .tooltip_text("Save (Ctrl+Return)")
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let actions = gtk::ActionBar::new();
    actions.pack_start(&discard);
    actions.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_bottom_bar(&actions);
    toolbar.set_content(Some(&content));

    let current = Rc::new(Cell::new(start));
    let form: Rc<RefCell<Option<Rc<AnswerForm>>>> = Rc::default();
    let template = Rc::new(RefCell::new(String::new()));
    let edited = Rc::new(Cell::new(false));
    let loading = Rc::new(Cell::new(false));
    view.buffer().connect_changed(glib::clone!(
        #[strong]
        edited,
        #[strong]
        loading,
        move |_| {
            if !loading.get() {
                edited.set(true);
            }
        }
    ));
    let title = adw::WindowTitle::new("New Entry", "");
    let switch = glib::clone!(
        #[strong]
        state,
        #[strong]
        current,
        #[strong]
        form,
        #[strong]
        template,
        #[strong]
        edited,
        #[strong]
        loading,
        #[weak]
        form_holder,
        #[weak]
        view,
        #[weak]
        title,
        move |id: NoteId| {
            current.set(id);
            let Some(note) = state.read(|spaces| spaces.note(id)) else {
                return;
            };
            let Some(fields) = note.journal.as_ref() else {
                return;
            };
            title.set_subtitle(&journal_name(&note));
            let new_form = AnswerForm::new(fields);
            if let Some(old) = form.replace(Some(new_form.clone())) {
                form_holder.remove(&old.widget);
            }
            form_holder.append(&new_form.widget);
            form_holder.set_visible(new_form.widget.is_visible());
            let new_template = fields.template();
            if !edited.get() {
                loading.set(true);
                view.buffer().set_text(&new_template);
                loading.set(false);
            }
            template.replace(new_template);
        }
    );
    let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
    if prompt.is_none() && journals.len() > 1 {
        let bar = gtk::Box::builder().spacing(6).build();
        for (index, journal) in journals.iter().enumerate() {
            let name = journal_name(journal);
            let button = gtk::ToggleButton::builder()
                .label(&name)
                .tooltip_text(match index {
                    0..9 => format!("Write in {name} (Alt+{})", index + 1),
                    _ => format!("Write in {name}"),
                })
                .active(journal.id == start)
                .build();
            button.set_group(buttons.first());
            let id = journal.id;
            let switch = switch.clone();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    switch(id);
                }
            });
            bar.append(&button);
            buttons.push(button);
        }
        header.pack_start(&bar);
        header.pack_end(&title);
        header.set_show_title(false);
    } else {
        if prompt.is_some() {
            title.set_title("Prompt");
        }
        header.set_title_widget(Some(&title));
    }
    switch(start);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .name(capture::NAME)
        // The README's Sway rule floats capture windows by this exact title.
        .title("Quick Capture")
        .default_width(520)
        .default_height(420)
        .content(&toolbar)
        .build();

    let saving = Rc::new(Cell::new(false));
    window.connect_close_request(glib::clone!(
        #[strong]
        saving,
        #[strong]
        state,
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |window| {
            if !saving.replace(false) {
                return glib::Propagation::Proceed;
            }
            let text = editor::text(&view.buffer());
            let answers = form.borrow().as_ref().map(|form| form.answers()).unwrap_or_default();
            let unwritten = text.trim().is_empty() || text.trim() == template.borrow().trim();
            if unwritten && answers.is_empty() {
                return glib::Propagation::Proceed;
            }
            let journal = current.get();
            state.set_capture_journal(journal);
            let text = state.read(|spaces| editor::collapse_titles(spaces, &text));
            let prompt = prompt.as_ref().map(|prompt| (prompt.key.as_str(), prompt.prompted));
            let created = state.write(|spaces| {
                spaces.create_entry(journal, &text, opened, prompt, &answers, today())
            });
            match created {
                Ok(_) => glib::Propagation::Proceed,
                Err(error) => {
                    let dialog = adw::AlertDialog::new(
                        Some("Couldn’t Save the Entry"),
                        Some(&error.to_string()),
                    );
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
    save.connect_clicked(glib::clone!(
        #[strong]
        save_and_close,
        move |_| save_and_close()
    ));
    let save_action = gio::SimpleAction::new(capture::SAVE_ACTION, None);
    save_action.connect_activate(move |_, _| save_and_close());
    window.add_action(&save_action);
    let toggle_space = gio::SimpleAction::new("toggle-space", Some(glib::VariantTy::INT32));
    toggle_space.connect_activate(move |_, parameter| {
        let index = parameter.and_then(|value| value.get::<i32>()).unwrap_or(0) - 1;
        if let Some(button) = usize::try_from(index).ok().and_then(|index| buttons.get(index)) {
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

    window.present();
    view.grab_focus();
    Some(window)
}
