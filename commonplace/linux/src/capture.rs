// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::editor;
use crate::state::State;

/// The widget name that marks a capture window.
const NAME: &str = "capture";

/// Whether `window` is a capture window.
pub fn is_capture(window: &gtk::Window) -> bool {
    window.widget_name() == NAME
}

/// Opens a small window that saves its text as a new memo, or a new task in the inbox if `task` is
/// set. Saving closes the window, and closing it any other way discards the text.
pub fn present(app: &adw::Application, state: Rc<State>, task: bool) -> adw::ApplicationWindow {
    let view = editor::new_view();
    let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
    let discard = gtk::Button::builder().label("_Discard").use_underline(true).build();
    let save = gtk::Button::builder()
        .label("_Save")
        .use_underline(true)
        .css_classes(["suggested-action"])
        .build();
    let kind = if task { "Action" } else { "Memo" };
    let header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new(&format!("New {kind}"), ""))
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&discard);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .name(NAME)
        .title(crate::window_title("Capture"))
        .default_width(480)
        .default_height(260)
        .content(&toolbar)
        .build();

    let saving = Rc::new(Cell::new(false));
    window.connect_close_request(glib::clone!(
        #[strong]
        saving,
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |window| {
            let text = editor::text(&view.buffer());
            if !saving.replace(false) || text.trim().is_empty() {
                return glib::Propagation::Proceed;
            }
            let created = state.write(|library| {
                if task { library.create_inbox_task(&text) } else { library.create_memo(&text) }
            });
            match created {
                Ok(_) => glib::Propagation::Proceed,
                Err(error) => {
                    let dialog = adw::AlertDialog::new(
                        Some(&format!("Couldn’t Save the {kind}")),
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
    let save_action = gio::SimpleAction::new("save", None);
    save_action.connect_activate(glib::clone!(
        #[strong]
        save_and_close,
        move |_, _| save_and_close()
    ));
    window.add_action(&save_action);

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
    window
}
