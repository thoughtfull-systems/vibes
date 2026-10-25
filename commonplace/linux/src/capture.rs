// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::Error;
use gtk::{gdk, gio, glib};

use crate::editor;
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
/// runs `save` and closes the window, or keeps it open with a dialog titled `failure` if `save`
/// fails, and closing it any other way discards what it holds. Alt with a digit turns on that
/// one of `choices`.
pub fn window(
    app: &adw::Application,
    header: &adw::HeaderBar,
    content: &impl IsA<gtk::Widget>,
    size: (i32, i32),
    choices: Vec<gtk::ToggleButton>,
    failure: &'static str,
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
        .title("Quick Capture")
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
                    let dialog = adw::AlertDialog::new(Some(failure), Some(&error.to_string()));
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

/// Opens a small window that saves its text as a new memo, or a new task in the inbox if `task` is
/// set, in the library its space buttons pick.
pub fn present(app: &adw::Application, state: Rc<State>, task: bool) -> adw::ApplicationWindow {
    let view = editor::new_view();
    let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
    let kind = if task { "Action" } else { "Memo" };
    let header = header();
    let space = Rc::new(RefCell::new(state.capture_space()));
    let spaces = state.spaces();
    let bar = gtk::Box::builder().spacing(6).build();
    let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
    for (index, choice) in spaces.iter().enumerate() {
        let button = gtk::ToggleButton::builder()
            .label(&choice.name)
            .tooltip_text(format!("Save in {} (Alt+{})", choice.name, index + 1))
            .active(choice.key == *space.borrow())
            .build();
        button.set_group(buttons.first());
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
        bar.append(&button);
        buttons.push(button);
    }
    let title = adw::WindowTitle::new(&format!("New {kind}"), "");
    if spaces.len() > 1 {
        header.pack_start(&bar);
        header.pack_end(&title);
        header.set_show_title(false);
    } else {
        header.set_title_widget(Some(&title));
    }
    let failure = if task { "Couldn’t Save the Action" } else { "Couldn’t Save the Memo" };
    let save = glib::clone!(
        #[weak]
        view,
        #[upgrade_or]
        Ok(()),
        move || {
            let text = editor::text(&view.buffer());
            if text.trim().is_empty() {
                return Ok(());
            }
            let space = space.borrow().clone();
            state.set_capture_space(&space);
            let text = state.read(|spaces| editor::collapse_titles(spaces, &text));
            state
                .write(|spaces| {
                    if task {
                        spaces.create_inbox_task(&space, &text)
                    } else {
                        spaces.create_memo(&space, &text)
                    }
                })
                .map(|_| ())
        }
    );
    let window = window(app, &header, &scrolled, (480, 260), buttons, failure, save);
    window.present();
    view.grab_focus();
    window
}
