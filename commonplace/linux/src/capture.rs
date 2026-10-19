// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::editor;
use crate::state::State;

/// Opens a small window that saves its text as a new memo when it closes.
pub fn present(app: &adw::Application, state: Rc<State>) {
    let view = editor::new_view();
    let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
    let discard = gtk::Button::builder().label("_Discard").use_underline(true).build();
    let save = gtk::Button::builder()
        .label("_Save")
        .use_underline(true)
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::new();
    header.pack_start(&discard);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Capture")
        .default_width(480)
        .default_height(260)
        .content(&toolbar)
        .build();

    let discarded = Rc::new(Cell::new(false));
    window.connect_close_request(glib::clone!(
        #[strong]
        discarded,
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |window| {
            let text = editor::text(&view.buffer());
            if discarded.get() || text.trim().is_empty() {
                return glib::Propagation::Proceed;
            }
            match state.write(|library| library.create_memo(&text)) {
                Ok(_) => glib::Propagation::Proceed,
                Err(error) => {
                    let dialog = adw::AlertDialog::new(
                        Some("Couldn’t Save the Memo"),
                        Some(&error.to_string()),
                    );
                    dialog.add_response("ok", "_OK");
                    dialog.present(Some(window));
                    glib::Propagation::Stop
                }
            }
        }
    ));
    discard.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| {
            discarded.set(true);
            window.close();
        }
    ));
    save.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));

    let save_action = gio::SimpleAction::new("save", None);
    save_action.connect_activate(glib::clone!(
        #[weak]
        window,
        move |_, _| window.close()
    ));
    window.add_action(&save_action);

    // The capture phase sees the keys before the editor turns Control+Return into a line break.
    let keys = gtk::EventControllerKey::builder()
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            let control_return = modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter);
            if control_return || key == gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(keys);

    window.present();
    view.grab_focus();
}
