// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Error, NoteId};
use gtk::{gio, glib};

use crate::state::State;

/// Asks for a library's name, and calls `on_name` with it unless the user cancels.
pub fn ask_name(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    initial: &str,
    on_name: impl Fn(String) + 'static,
) {
    let entry = gtk::Entry::builder().text(initial).activates_default(true).build();
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.set_extra_child(Some(&entry));
    dialog.add_responses(&[("cancel", "_Cancel"), ("name", "_Save")]);
    dialog.set_response_appearance("name", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("name"));
    dialog.set_close_response("cancel");
    let update = glib::clone!(
        #[weak]
        dialog,
        move |entry: &gtk::Entry| {
            dialog.set_response_enabled("name", !entry.text().trim().is_empty());
        }
    );
    update(&entry);
    entry.connect_changed(update);
    dialog.connect_response(
        Some("name"),
        glib::clone!(
            #[weak]
            entry,
            move |_, _| on_name(entry.text().trim().to_owned())
        ),
    );
    dialog.present(Some(parent));
    entry.grab_focus();
}

/// Warns before a note moves to another library, and calls `on_move` if the user goes ahead.
pub fn confirm_move(
    parent: &impl IsA<gtk::Widget>,
    space: &str,
    on_move: impl Fn() + 'static,
    on_cancel: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::new(
        Some(&format!("Move to {space}?")),
        Some("Edits to the note on another device that haven’t synced yet are lost"),
    );
    dialog.add_responses(&[("cancel", "_Cancel"), ("move", "_Move")]);
    dialog.set_default_response(Some("move"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        None,
        move |_, response| {
            if response == "move" { on_move() } else { on_cancel() }
        },
    );
    dialog.present(Some(parent));
}

/// Asks for the name of a library this device hasn't named yet, once there's more than one
/// library. Naming it changes the space buttons, which ask about the next.
pub fn name_unnamed(state: &Rc<State>, parent: &impl IsA<gtk::Widget>) {
    if state.spaces().len() < 2 {
        return;
    }
    let Some(key) = state.unnamed().into_iter().next() else {
        return;
    };
    let count =
        state.read(|spaces| spaces.library(&key).map(|library| library.notes().len())).unwrap_or(0);
    let notes = if count == 1 { "1 note".to_owned() } else { format!("{count} notes") };
    ask_name(
        parent,
        "Name this library",
        &format!("This device has a library with {notes} and no name yet"),
        "",
        glib::clone!(
            #[weak]
            state,
            move |name| state.set_name(&key, &name)
        ),
    );
}

/// The dialog that adds, names, merges, and deletes this device's libraries.
pub fn present_dialog(state: &Rc<State>, parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::PreferencesDialog::builder().title("Libraries").build();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .description(
            "Each library is a space that syncs on its own. Names belong to this device alone.",
        )
        .build();
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Add library")
        .css_classes(["flat"])
        .build();
    group.set_header_suffix(Some(&add));
    page.add(&group);
    dialog.add(&page);

    let actions = gio::SimpleActionGroup::new();
    let merge = gio::SimpleAction::new("merge", Some(glib::VariantTy::STRING_ARRAY));
    let delete = gio::SimpleAction::new("delete", Some(glib::VariantTy::STRING));
    actions.add_action(&merge);
    actions.add_action(&delete);
    dialog.insert_action_group("libraries", Some(&actions));

    let rows: Rc<std::cell::RefCell<Vec<adw::EntryRow>>> = Rc::default();
    let fill = Rc::new(glib::clone!(
        #[weak]
        state,
        #[weak]
        group,
        #[strong]
        rows,
        move || {
            for row in rows.borrow_mut().drain(..) {
                group.remove(&row);
            }
            let spaces = state.spaces();
            for space in &spaces {
                let read_only = state.read(|spaces| {
                    spaces.library(&space.key).is_some_and(|library| library.is_read_only())
                });
                let row = adw::EntryRow::builder()
                    .title(if read_only {
                        "Name on this device · read-only"
                    } else {
                        "Name on this device"
                    })
                    .text(state.device().name(&space.key).unwrap_or_default())
                    .show_apply_button(true)
                    .build();
                let key = space.key.clone();
                row.connect_apply(glib::clone!(
                    #[weak]
                    state,
                    move |row| {
                        let name = row.text();
                        if !name.trim().is_empty() {
                            state.set_name(&key, &name);
                        }
                    }
                ));
                let menu = gio::Menu::new();
                let others = gio::Menu::new();
                for other in spaces.iter().filter(|other| other.key != space.key) {
                    let item = gio::MenuItem::new(Some(&other.name), None);
                    item.set_action_and_target_value(
                        Some("libraries.merge"),
                        Some(&[space.key.as_str(), other.key.as_str()].to_variant()),
                    );
                    others.append_item(&item);
                }
                if spaces.len() > 1 {
                    menu.append_submenu(Some("_Merge into"), &others);
                    let item = gio::MenuItem::new(Some("_Delete library"), None);
                    item.set_action_and_target_value(
                        Some("libraries.delete"),
                        Some(&space.key.to_variant()),
                    );
                    menu.append_item(&item);
                }
                let button = gtk::MenuButton::builder()
                    .icon_name("view-more-symbolic")
                    .tooltip_text("Library menu")
                    .menu_model(&menu)
                    .valign(gtk::Align::Center)
                    .sensitive(spaces.len() > 1)
                    .css_classes(["flat"])
                    .build();
                row.add_suffix(&button);
                group.add(&row);
                rows.borrow_mut().push(row);
            }
        }
    ));
    fill();

    add.connect_clicked(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        #[strong]
        fill,
        move |_| {
            let fill = fill.clone();
            ask_name(
                &dialog,
                "New library",
                "Name it for this device",
                "",
                move |name| match state.create_space(&name) {
                    Ok(_) => fill(),
                    Err(error) => eprintln!("commonplace: couldn't add a library: {error}"),
                },
            );
        }
    ));
    merge.connect_activate(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        #[strong]
        fill,
        move |_, parameter| {
            let Some([source, target]) = parameter
                .and_then(|value| value.get::<Vec<String>>())
                .and_then(|keys| <[String; 2]>::try_from(keys).ok())
            else {
                return;
            };
            let name = |key: &str| state.device().name(key).unwrap_or_else(|| "Unnamed".into());
            let alert = adw::AlertDialog::new(
                Some(&format!("Merge {} Into {}?", name(&source), name(&target))),
                Some(&format!(
                    "Commonplace adds every note in {} to {}, then deletes {} from this device. \
                    Other devices keep their copies.",
                    name(&source),
                    name(&target),
                    name(&source),
                )),
            );
            alert.add_responses(&[("cancel", "_Cancel"), ("merge", "_Merge")]);
            alert.set_response_appearance("merge", adw::ResponseAppearance::Destructive);
            alert.set_default_response(Some("cancel"));
            alert.set_close_response("cancel");
            let fill = fill.clone();
            alert.connect_response(
                Some("merge"),
                glib::clone!(
                    #[weak]
                    state,
                    #[weak]
                    dialog,
                    move |_, _| {
                        match state.merge_space(&source, &target) {
                            Ok(()) => fill(),
                            Err(error) => {
                                dialog.add_toast(adw::Toast::new(&error.to_string()));
                            }
                        }
                    }
                ),
            );
            alert.present(Some(&dialog));
        }
    ));
    delete.connect_activate(glib::clone!(
        #[weak]
        state,
        #[weak]
        dialog,
        #[strong]
        fill,
        move |_, parameter| {
            let Some(key) = parameter.and_then(|value| value.get::<String>()) else {
                return;
            };
            let name = state.device().name(&key).unwrap_or_else(|| "Unnamed".into());
            let alert = adw::AlertDialog::new(
                Some(&format!("Delete {name}?")),
                Some(
                    "Commonplace deletes the library and every note in it permanently from this \
                    device. Other devices keep their copies.",
                ),
            );
            alert.add_responses(&[("cancel", "_Cancel"), ("delete", "_Delete")]);
            alert.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            alert.set_default_response(Some("cancel"));
            alert.set_close_response("cancel");
            let fill = fill.clone();
            alert.connect_response(
                Some("delete"),
                glib::clone!(
                    #[weak]
                    state,
                    #[weak]
                    dialog,
                    move |_, _| match state.remove_space(&key) {
                        Ok(()) => fill(),
                        Err(error) => dialog.add_toast(adw::Toast::new(&error.to_string())),
                    }
                ),
            );
            alert.present(Some(&dialog));
        }
    ));
    dialog.present(Some(parent));
}

/// Moves an action under `parent`, or to the top level, after a warning if the parent is in another
/// library, and hands the outcome to `report`.
pub fn move_action(
    state: &Rc<State>,
    parent_widget: &impl IsA<gtk::Widget>,
    id: NoteId,
    parent: Option<NoteId>,
    report: impl Fn(Result<(), Error>) + 'static,
) {
    let space_of = |id| state.read(|spaces| spaces.space_of(id).map(str::to_owned));
    let target = parent.and_then(space_of).filter(|target| Some(target) != space_of(id).as_ref());
    let run = glib::clone!(
        #[strong]
        state,
        move || report(state.write(|spaces| spaces.move_action(id, parent, None)))
    );
    match target {
        Some(target) => {
            let name = state.device().name(&target).unwrap_or_else(|| "Unnamed".into());
            confirm_move(parent_widget, &name, run, || {});
        }
        None => run(),
    }
}
