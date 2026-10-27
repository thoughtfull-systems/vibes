// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{Link, note_uri, parse_links};
use gtk::{gio, glib};

use crate::editor;

mod imp {
    use gtk::subclass::prelude::*;
    use sourceview5::subclass::prelude::*;

    use super::*;

    #[derive(Default)]
    pub struct View;

    #[glib::object_subclass]
    impl ObjectSubclass for View {
        const NAME: &'static str = "CommonplaceEditorView";
        type Type = super::View;
        type ParentType = sourceview5::View;

        fn class_init(class: &mut Self::Class) {
            for name in ["redo", "undo"] {
                class.install_action(&format!("text.{name}"), None, move |view, _, _| {
                    let _ = view.activate_action(&format!("history.{name}"), None);
                });
            }
        }
    }

    impl ObjectImpl for View {}
    impl WidgetImpl for View {}
    impl TextViewImpl for View {}
    impl ViewImpl for View {}
}

glib::wrapper! {
    pub struct View(ObjectSubclass<imp::View>)
        @extends sourceview5::View, gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

pub fn new_view(buffer: &sourceview5::Buffer) -> sourceview5::View {
    glib::Object::builder::<View>().property("buffer", buffer).build().upcast()
}

/// The offsets of a buffer's link icons. GTK counts child anchors in its offsets but omits them
/// from `TextBuffer::text`. Finding the icons walks the buffer, so code that converts many offsets
/// takes one snapshot for all of them.
pub struct Icons(Vec<i32>);

impl Icons {
    pub fn of(buffer: &gtk::TextBuffer) -> Self {
        let Some(tag) = buffer.tag_table().lookup("link-icon") else { return Self(Vec::new()) };
        let mut offsets = Vec::new();
        let mut at = buffer.start_iter();
        loop {
            while at.has_tag(&tag) {
                if at.child_anchor().is_some() {
                    offsets.push(at.offset());
                }
                if !at.forward_char() {
                    return Self(offsets);
                }
            }
            if !at.forward_to_tag_toggle(Some(&tag)) {
                return Self(offsets);
            }
        }
    }

    pub fn offsets(&self) -> &[i32] {
        &self.0
    }

    pub fn source_offset(&self, offset: i32) -> i32 {
        offset - self.0.partition_point(|at| *at < offset) as i32
    }

    /// The buffer offset of the text offset `offset`, after any icon there.
    pub fn buffer_offset(&self, offset: i32) -> i32 {
        let mut raw = offset;
        for &at in &self.0 {
            if at > raw {
                break;
            }
            raw += 1;
        }
        raw
    }

    pub fn source_iter(&self, buffer: &gtk::TextBuffer, offset: i32) -> gtk::TextIter {
        buffer.iter_at_offset(self.buffer_offset(offset))
    }
}

pub fn source_offset(buffer: &gtk::TextBuffer, offset: i32) -> i32 {
    Icons::of(buffer).source_offset(offset)
}

pub fn source_iter(buffer: &gtk::TextBuffer, offset: i32) -> gtk::TextIter {
    Icons::of(buffer).source_iter(buffer, offset)
}

pub fn load(view: &sourceview5::View, body: &str) {
    let _ = view.activate_action("history.load", Some(&body.to_variant()));
}

/// Keeps display-only anchors out of the text history: GTK doesn't record their insertion, but
/// records their deletion and counts them in text edits. A text-only buffer owns GTK's history.
pub fn attach(view: &sourceview5::View) {
    let buffer = view.buffer();
    buffer.set_enable_undo(false);
    buffer.tag_table().add(&gtk::TextTag::builder().name("link-icon").build());
    buffer
        .tag_table()
        .add(&gtk::TextTag::builder().name("link-icon-prefix").allow_breaks(false).build());
    let history = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
    history.set_enable_undo(true);
    let decorating = Rc::new(Cell::new(false));
    let restoring = Rc::new(Cell::new(false));
    let pending = Rc::new(Cell::new(false));
    buffer.connect_changed(glib::clone!(
        #[strong]
        decorating,
        #[strong]
        pending,
        #[weak]
        view,
        move |buffer| {
            if decorating.get() {
                buffer.stop_signal_emission_by_name("changed");
                return;
            }
            if !pending.replace(true) {
                glib::idle_add_local_once(glib::clone!(
                    #[strong]
                    decorating,
                    #[strong]
                    pending,
                    #[weak]
                    view,
                    move || {
                        pending.set(false);
                        decorating.set(true);
                        rebuild(&view);
                        decorating.set(false);
                    }
                ));
            }
        }
    ));
    buffer.connect_mark_set(glib::clone!(
        #[strong]
        decorating,
        #[strong]
        history,
        #[strong]
        restoring,
        move |buffer, _, _| {
            if decorating.get() {
                buffer.stop_signal_emission_by_name("mark-set");
            } else if !restoring.get() {
                let icons = Icons::of(buffer);
                let insert = icons.source_offset(buffer.cursor_position());
                let bound =
                    icons.source_offset(buffer.iter_at_mark(&buffer.selection_bound()).offset());
                history
                    .select_range(&history.iter_at_offset(insert), &history.iter_at_offset(bound));
            }
        }
    ));
    buffer.connect_insert_text(glib::clone!(
        #[strong]
        decorating,
        #[strong]
        restoring,
        #[strong]
        history,
        move |buffer, at, text| {
            if !decorating.get() && !restoring.get() {
                let mut at = history.iter_at_offset(source_offset(buffer, at.offset()));
                history.insert(&mut at, text);
            }
        }
    ));
    buffer.connect_delete_range(glib::clone!(
        #[strong]
        decorating,
        #[strong]
        restoring,
        #[strong]
        history,
        move |buffer, start, end| {
            if !decorating.get() && !restoring.get() {
                let icons = Icons::of(buffer);
                let mut start = history.iter_at_offset(icons.source_offset(start.offset()));
                let mut end = history.iter_at_offset(icons.source_offset(end.offset()));
                if start != end {
                    history.delete(&mut start, &mut end);
                }
            }
        }
    ));
    buffer.connect_begin_user_action(glib::clone!(
        #[strong]
        history,
        move |_| history.begin_user_action()
    ));
    buffer.connect_end_user_action(glib::clone!(
        #[strong]
        history,
        move |_| history.end_user_action()
    ));
    let actions = gio::SimpleActionGroup::new();
    let load = gio::SimpleAction::new("load", Some(glib::VariantTy::STRING));
    load.connect_activate(glib::clone!(
        #[strong]
        history,
        #[strong]
        restoring,
        #[weak]
        buffer,
        move |_, value| {
            let Some(body) = value.and_then(|value| value.str()) else { return };
            restoring.set(true);
            history.set_text(body);
            buffer.set_text(body);
            restoring.set(false);
        }
    ));
    actions.add_action(&load);

    for (name, redo) in [("redo", true), ("undo", false)] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[strong]
            history,
            #[strong]
            restoring,
            #[weak]
            buffer,
            move |_, _| {
                restoring.set(true);
                if redo {
                    history.redo()
                } else {
                    history.undo()
                }
                buffer.set_text(&editor::text(&history));
                buffer.select_range(
                    &buffer.iter_at_offset(history.cursor_position()),
                    &buffer
                        .iter_at_offset(history.iter_at_mark(&history.selection_bound()).offset()),
                );
                restoring.set(false);
            }
        ));
        if redo {
            history.bind_property("can-redo", &action, "enabled").sync_create().build();
        } else {
            history.bind_property("can-undo", &action, "enabled").sync_create().build();
        }
        actions.add_action(&action);
    }
    view.insert_action_group("history", Some(&actions));
    history.connect_notify_local(
        None,
        glib::clone!(
            #[weak]
            view,
            move |history, _| {
                view.action_set_enabled("text.undo", view.is_editable() && history.can_undo());
                view.action_set_enabled("text.redo", view.is_editable() && history.can_redo());
            }
        ),
    );
    view.connect_editable_notify(glib::clone!(
        #[strong]
        history,
        move |view| {
            view.action_set_enabled("text.undo", view.is_editable() && history.can_undo());
            view.action_set_enabled("text.redo", view.is_editable() && history.can_redo());
        }
    ));
    view.connect_backspace(|view| {
        let buffer = view.buffer();
        if !buffer.has_selection() {
            let mut before = buffer.iter_at_mark(&buffer.get_insert());
            if before.backward_char() && before.child_anchor().is_some() {
                buffer.place_cursor(&before);
            }
        }
    });
    view.connect_delete_from_cursor(|view, kind, count| {
        let buffer = view.buffer();
        if kind == gtk::DeleteType::Chars && count > 0 && !buffer.has_selection() {
            let mut at = buffer.iter_at_mark(&buffer.get_insert());
            if at.child_anchor().is_some() {
                at.forward_char();
                buffer.place_cursor(&at);
            }
        }
    });
}

fn rebuild(view: &sourceview5::View) {
    let buffer = view.buffer();
    let icons = Icons::of(&buffer);
    let body = editor::text(&buffer);
    let starts: Vec<i32> = parse_links(&body)
        .iter()
        .filter(|link| link.note.is_some() || link.external_uri().is_some())
        .map(|link| body[..link.range.start].chars().count() as i32)
        .collect();
    let placed = icons.offsets().iter().enumerate().map(|(index, at)| at - index as i32);
    // Each icon finds its link when clicked, so icons already before each followable link stay.
    if placed.eq(starts.iter().copied()) {
        return;
    }
    let insert = icons.source_offset(buffer.cursor_position());
    let bound = icons.source_offset(buffer.iter_at_mark(&buffer.selection_bound()).offset());
    for &offset in icons.offsets().iter().rev() {
        let mut start = buffer.iter_at_offset(offset);
        let mut end = buffer.iter_at_offset(offset + 1);
        buffer.delete(&mut start, &mut end);
    }
    // Inserting from the end leaves the offsets of the links before each insertion unchanged.
    for &offset in starts.iter().rev() {
        let mut at = buffer.iter_at_offset(offset);
        let anchor = buffer.create_child_anchor(&mut at);
        let mut before = at;
        before.backward_char();
        buffer.remove_all_tags(&before, &at);
        buffer.apply_tag_by_name("link-icon", &before, &at);
        let image = gtk::DrawingArea::builder().content_width(12).content_height(12).build();
        image.set_draw_func(|area, cr, _, _| {
            cr.scale(0.75, 0.75);
            let ink = area.color();
            cr.set_source_rgba(ink.red().into(), ink.green().into(), ink.blue().into(), 1.0);
            cr.set_line_width(1.8);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            cr.move_to(8.5, 1.5);
            cr.line_to(14.5, 1.5);
            cr.line_to(14.5, 7.5);
            cr.move_to(14.5, 1.5);
            cr.line_to(6.5, 9.5);
            cr.move_to(6.0, 3.5);
            cr.line_to(1.5, 3.5);
            cr.line_to(1.5, 14.5);
            cr.line_to(12.5, 14.5);
            cr.line_to(12.5, 10.0);
            let _ = cr.stroke();
        });
        let button = gtk::Button::builder()
            .child(&image)
            .css_classes(["flat", "inline-link"])
            .focus_on_click(false)
            .tooltip_text("Open link")
            .build();
        button.set_cursor_from_name(Some("pointer"));
        button.connect_clicked(glib::clone!(
            #[weak]
            view,
            #[weak]
            anchor,
            move |_| {
                let buffer = view.buffer();
                let at = buffer.iter_at_child_anchor(&anchor).offset();
                let at = Icons::of(&buffer).source_offset(at);
                let body = editor::text(&buffer);
                let link = parse_links(&body)
                    .into_iter()
                    .find(|link| body[..link.range.start].chars().count() as i32 == at);
                if let (Some(link), Some(window)) =
                    (link, view.root().and_downcast::<gtk::Window>())
                {
                    open(&window, &link);
                }
            }
        ));
        view.add_child_at_anchor(&button, &anchor);
    }
    let icons = Icons::of(&buffer);
    buffer.select_range(&icons.source_iter(&buffer, insert), &icons.source_iter(&buffer, bound));
}

fn open(window: &gtk::Window, link: &Link) {
    let uri = link.note.map(note_uri).or_else(|| link.external_uri().map(str::to_owned));
    let Some(uri) = uri else { return };
    if link.note.is_some()
        && let Some(app) = window.application()
    {
        app.activate_action("open-note", Some(&uri.to_variant()));
    } else {
        gtk::UriLauncher::new(&uri).launch(Some(window), gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                eprintln!("Couldn't open link: {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle() {
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
    }

    #[test]
    #[ignore = "needs an isolated GTK Broadway display"]
    fn anchors_preserve_unicode_edits_and_text_history() {
        gtk::init().unwrap();
        let css = gtk::CssProvider::new();
        css.load_from_string(crate::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let app = adw::Application::builder()
            .application_id(format!(
                "systems.thoughtfull.CommonplaceAgent.Editor{}",
                std::process::id()
            ))
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let opened = Rc::new(Cell::new(false));
        let open = gio::SimpleAction::new("open-note", Some(glib::VariantTy::STRING));
        open.connect_activate(glib::clone!(
            #[strong]
            opened,
            move |_, value| opened.set(
                value
                    .and_then(|value| value.str())
                    .is_some_and(|uri| uri.starts_with("commonplace:note/"))
            )
        ));
        app.add_action(&open);
        let view = editor::new_view();
        let window =
            gtk::Window::builder().application(&app).default_width(400).child(&view).build();
        window.present();
        let body = "🦀 [café](https://example.com) and https://example.org\n- [ ] item";
        load(&view, body);
        settle();
        let buffer = view.buffer();
        assert_eq!(editor::text(&buffer), body);
        assert_eq!(buffer.char_count() as usize, body.chars().count() + 2);
        let link = body.chars().position(|c| c == '[').unwrap() as i32;
        let at = source_iter(&buffer, link);
        assert_eq!(at.char(), '[');
        assert_eq!(source_offset(&buffer, at.offset()), link);
        buffer.place_cursor(&source_iter(&buffer, link + 2));
        buffer.begin_user_action();
        buffer.insert_at_cursor("new ");
        buffer.end_user_action();
        settle();
        assert!(editor::text(&buffer).contains("[cnew afé]"));
        view.activate_action("text.undo", None).unwrap();
        settle();
        assert_eq!(editor::text(&buffer), body);
        view.activate_action("text.redo", None).unwrap();
        settle();
        assert!(editor::text(&buffer).contains("[cnew afé]"));
        view.activate_action("text.undo", None).unwrap();
        settle();
        let start = source_iter(&buffer, link);
        let mut icon = start;
        icon.backward_char();
        assert!(icon.child_anchor().is_some());
        assert!(icon.tags().iter().all(|tag| !tag.is_invisible()));
        assert!(start.tags().iter().all(|tag| !tag.is_invisible()));
        buffer.place_cursor(&buffer.end_iter());
        assert!(editor::toggle_checkbox_at(&buffer, buffer.cursor_position()));
        settle();
        assert!(editor::text(&buffer).ends_with("- [x] item"));
        view.activate_action("text.undo", None).unwrap();
        settle();
        assert_eq!(editor::text(&buffer), body);
        buffer.place_cursor(&source_iter(&buffer, link));
        view.emit_by_name::<()>("backspace", &[]);
        settle();
        assert_eq!(editor::text(&buffer), body.replacen("🦀 ", "🦀", 1));
        view.activate_action("text.undo", None).unwrap();
        settle();
        assert_eq!(editor::text(&buffer), body);
        let target = commonplace_core::NoteId::generate();
        load(&view, &commonplace_core::wikilink(target, Some("Target")));
        settle();
        buffer.place_cursor(&source_iter(&buffer, 3));
        let icon = buffer.start_iter().child_anchor().unwrap();
        icon.widgets().first().unwrap().downcast_ref::<gtk::Button>().unwrap().emit_clicked();
        assert!(opened.get());
        load(&view, "[[nowhere]] leads nowhere");
        settle();
        assert!(Icons::of(&buffer).offsets().is_empty());
        load(&view, "A different note");
        settle();
        view.activate_action("text.undo", None).unwrap();
        assert_eq!(editor::text(&buffer), "A different note");
        window.close();
    }
}
