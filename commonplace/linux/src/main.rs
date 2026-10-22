// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

mod action_panel;
mod bottom_pane;
mod browse;
mod capture;
mod drag_scroll;
mod editor;
mod highlight;
mod inbox;
mod layout;
mod link_picker;
mod note_page;
mod rows;
mod state;
mod window;

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use commonplace_core::{Library, note_uri, parse_note_uri};
use gtk::{gio, glib};

use crate::state::State;
use crate::window::Window;

const APP_ID: &str = "systems.thoughtfull.Commonplace";

/// The application ID, which `COMMONPLACE_APP_ID` overrides so a test copy runs beside the
/// everyday one instead of handing its work to it.
fn app_id() -> String {
    std::env::var("COMMONPLACE_APP_ID").unwrap_or_else(|_| APP_ID.to_owned())
}
const CSS: &str = "
textview.note-editor { font-size: 1.25em; }
.project-title { font-style: italic; }
.step-list:drop(active) { box-shadow: none; outline: none; }
.drop-line { background-color: @accent_color; border-radius: 1px; }
.done { text-decoration-line: line-through; }
.dragged { opacity: 0.55; }
.drop-inside { box-shadow: inset 0 0 0 2px @accent_color; border-radius: 6px; }
";

const USAGE: &str = "\
Usage:
  commonplace                         Open the main window
  commonplace capture memo            Open the quick capture window for a memo
  commonplace capture action          Open the quick capture window for an action
  commonplace capture memo TEXT…      Save TEXT as a memo, and print its URI
  commonplace capture action TEXT…    Save TEXT as an action in the inbox, and print its URI
  commonplace URI                     Open the note at a commonplace: URI

The library lives in $COMMONPLACE_LIBRARY, or in $XDG_DATA_HOME/commonplace if that's unset.
Set $COMMONPLACE_APP_ID to run a separate copy beside the one already running.
";

/// What the primary instance holds for the rest of its life.
#[derive(Default)]
struct App {
    state: Option<Result<Rc<State>, String>>,
    window: Option<Rc<Window>>,
}

fn main() -> glib::ExitCode {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    if matches!(
        arguments.iter().map(String::as_str).collect::<Vec<_>>()[..],
        ["capture", "memo" | "action"]
    ) {
        start_in_background();
    }
    let app = adw::Application::builder()
        .application_id(app_id())
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let shared = Rc::new(RefCell::new(App::default()));
    app.connect_startup(glib::clone!(
        #[strong]
        shared,
        move |app| {
            let css = gtk::CssProvider::new();
            css.load_from_string(CSS);
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            // A copy under another app ID is a test copy, which mustn't touch the everyday library.
            let state = if std::env::var_os("COMMONPLACE_APP_ID").is_some()
                && std::env::var_os("COMMONPLACE_LIBRARY").is_none()
            {
                Err("COMMONPLACE_APP_ID needs COMMONPLACE_LIBRARY too".to_owned())
            } else {
                Library::open(&library_dir())
                    .map(|library| Rc::new(State::new(library)))
                    .map_err(|error| error.to_string())
            };
            shared.borrow_mut().state = Some(state);
            // Closing each window, unlike Application::quit, saves any pending edits.
            let quit = gio::ActionEntry::builder("quit")
                .activate(|app: &adw::Application, _, _| {
                    if let Some(window) = app.active_window().filter(capture::is_capture) {
                        window.close();
                        return;
                    }
                    // Quitting from the main window keeps whatever an open capture holds.
                    for window in app.windows() {
                        if capture::is_capture(&window) {
                            let _ = window.activate_action("win.save", None);
                        } else {
                            window.close();
                        }
                    }
                })
                .build();
            app.add_action_entries([quit]);
            // Keep Emacs keys free. Emacs users remap them system-wide into standard keys, such as
            // C-s into Control+F and C-x C-c into Control+Q, so shortcuts use either those standard
            // keys or keys that Emacs leaves unbound.
            let accels: [(&str, &[&str]); 5] = [
                ("app.quit", &["<Control>q"]),
                ("win.insert-link", &["<Alt>bracketleft"]),
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
        ["capture", kind @ ("memo" | "action")] => {
            let window = capture_with_window(app, state, shared, kind == "action");
            // The command that asked for the capture exits once it's saved or discarded.
            let command_line = command_line.clone();
            window.connect_destroy(move |_| command_line.done());
        }
        ["capture", kind @ ("memo" | "action"), ref words @ ..] => {
            let task = kind == "action";
            let text = words.join(" ");
            let created = state.write(|library| {
                if task { library.create_inbox_task(&text) } else { library.create_memo(&text) }
            });
            match created {
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

/// Opens the capture window, after opening the main window if Commonplace has just started, so one
/// command both starts the app and captures.
fn capture_with_window(
    app: &adw::Application,
    state: Rc<State>,
    shared: &Rc<RefCell<App>>,
    task: bool,
) -> adw::ApplicationWindow {
    if shared.borrow().window.is_none() {
        main_window(app, &state, shared).present();
    }
    capture::present(app, state, task)
}

/// Starts Commonplace in a process of its own if it isn't running, and waits for it to take its
/// name on the session bus, so this process hands it the command and exits instead of becoming the
/// instance that stays running.
fn start_in_background() {
    if is_running() {
        return;
    }
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let started = Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    if started.is_err() {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !is_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether a Commonplace instance owns the app's name on the session bus.
fn is_running() -> bool {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return false;
    };
    bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&(app_id(),).to_variant()),
        Some(glib::VariantTy::new("(b)").expect("the type string is valid")),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|reply| reply.get::<(bool,)>())
    .is_some_and(|(owned,)| owned)
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
