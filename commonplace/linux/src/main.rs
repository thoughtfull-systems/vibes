// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

mod capture;
mod editor;
mod highlight;
mod link_picker;
mod state;
mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Library, note_uri, parse_note_uri};
use gtk::{gio, glib};

use crate::state::State;
use crate::window::Window;

const APP_ID: &str = "systems.thoughtfull.Commonplace";
const USAGE: &str = "\
Usage:
  commonplace                 Open the main window
  commonplace capture         Open the quick capture window
  commonplace capture TEXT…   Save TEXT as a new memo and print its URI
  commonplace URI             Open the note at a commonplace: URI

The library lives in $COMMONPLACE_LIBRARY, or in $XDG_DATA_HOME/commonplace if that's unset.
";

/// What the primary instance holds for the rest of its life.
#[derive(Default)]
struct App {
    state: Option<Result<Rc<State>, String>>,
    window: Option<Rc<Window>>,
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let shared = Rc::new(RefCell::new(App::default()));
    app.connect_startup(glib::clone!(
        #[strong]
        shared,
        move |app| {
            let css = gtk::CssProvider::new();
            css.load_from_string("textview.note-editor { font-size: 1.25em; }");
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            let state = Library::open(&library_dir())
                .map(|library| Rc::new(State::new(library)))
                .map_err(|error| error.to_string());
            shared.borrow_mut().state = Some(state);
            // Closing each window, unlike Application::quit, saves any pending edits.
            let quit = gio::ActionEntry::builder("quit")
                .activate(|app: &adw::Application, _, _| {
                    for window in app.windows() {
                        window.close();
                    }
                })
                .build();
            app.add_action_entries([quit]);
            // Keep Emacs keys free. Emacs users remap them system-wide into standard keys, such as
            // C-s into Control+F and C-x C-c into Control+Q, so shortcuts use either those standard
            // keys or keys that Emacs leaves unbound.
            let accels: [(&str, &[&str]); 6] = [
                ("app.quit", &["<Control>q"]),
                ("win.insert-link", &["<Alt>bracketleft"]),
                ("win.new-memo", &["<Alt>n"]),
                ("win.save", &["<Control>s"]),
                ("win.search", &["<Control>f", "<Control>o"]),
                ("window.close", &["<Control>F4"]),
            ];
            for (action, accels) in accels {
                app.set_accels_for_action(action, accels);
            }
        }
    ));
    app.connect_command_line(move |app, command_line| run(app, command_line, &shared));
    app.run()
}

fn run(
    app: &adw::Application,
    command_line: &gio::ApplicationCommandLine,
    shared: &Rc<RefCell<App>>,
) -> glib::ExitCode {
    let arguments: Vec<String> = command_line
        .arguments()
        .into_iter()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    if matches!(arguments.first().map(String::as_str), Some("-h" | "--help" | "help")) {
        command_line.print_literal(USAGE);
        return glib::ExitCode::SUCCESS;
    }
    let state = match &shared.borrow().state {
        Some(Ok(state)) => state.clone(),
        Some(Err(error)) => {
            command_line.printerr_literal(&format!("commonplace: {error}\n"));
            return glib::ExitCode::FAILURE;
        }
        None => unreachable!("startup opens the library"),
    };
    match arguments.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => main_window(app, &state, shared).present(),
        ["capture"] => capture::present(app, state),
        ["capture", ref words @ ..] => {
            let text = words.join(" ");
            match state.write(|library| library.create_memo(&text)) {
                Ok(id) => command_line.print_literal(&format!("{}\n", note_uri(id))),
                Err(error) => {
                    command_line.printerr_literal(&format!("commonplace: {error}\n"));
                    return glib::ExitCode::FAILURE;
                }
            }
        }
        [uri] if parse_note_uri(uri).is_some() => {
            let window = main_window(app, &state, shared);
            window.present();
            if let Some(id) = parse_note_uri(uri) {
                window.open(id);
            }
        }
        _ => {
            command_line.printerr_literal(USAGE);
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}

fn main_window(app: &adw::Application, state: &Rc<State>, shared: &Rc<RefCell<App>>) -> Rc<Window> {
    if let Some(window) = &shared.borrow().window {
        return window.clone();
    }
    let window = Window::new(app, state.clone());
    window.connect_destroy(glib::clone!(
        #[weak]
        shared,
        move || {
            shared.borrow_mut().window = None;
        }
    ));
    shared.borrow_mut().window = Some(window.clone());
    window
}

fn library_dir() -> PathBuf {
    std::env::var_os("COMMONPLACE_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| glib::user_data_dir().join("commonplace"))
}
