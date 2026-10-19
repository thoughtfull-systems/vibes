// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use commonplace_core::{Error, Link, Note, NoteId, label, note_uri, outline, wikilink, word_count};
use gtk::{gdk, gio, glib};

use crate::editor;
use crate::link_picker::{Choice, LinkPicker};
use crate::state::State;

const EMPTY_LABEL: &str = "Empty memo";
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// The main window: a list of notes beside an editor for the selected one.
pub struct Window {
    /// Notes to return to with Back, most recent last.
    back: RefCell<Vec<NoteId>>,
    backlink_ids: RefCell<Vec<NoteId>>,
    backlinks: gtk::ListBox,
    backlinks_section: gtk::Box,
    content_page: adw::NavigationPage,
    copy_button: gtk::Button,
    current: Cell<Option<NoteId>>,
    delete_button: gtk::Button,
    forward: RefCell<Vec<NoteId>>,
    list: gtk::ListBox,
    list_ids: RefCell<Vec<NoteId>>,
    /// Set while the window changes widgets itself, so their change signals don't echo back.
    loading: Cell<bool>,
    outline_list: gtk::ListBox,
    /// The character offset where each heading in the outline starts.
    outline_offsets: RefCell<Vec<i32>>,
    pending_save: RefCell<Option<glib::SourceId>>,
    picker: Rc<LinkPicker>,
    restore_button: gtk::Button,
    search: gtk::SearchEntry,
    show_trash: Cell<bool>,
    split: adw::NavigationSplitView,
    stack: gtk::Stack,
    state: Rc<State>,
    toasts: adw::ToastOverlay,
    trash_button: gtk::Button,
    view: sourceview5::View,
    window: adw::ApplicationWindow,
    word_count: gtk::Label,
}

impl Window {
    pub fn new(app: &adw::Application, state: Rc<State>) -> Rc<Self> {
        let new_button = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New Memo")
            .action_name("win.new-memo")
            .build();
        let menu = gio::Menu::new();
        menu.append(Some("Show _Trash"), Some("win.show-trash"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .menu_model(&menu)
            .primary(true)
            .build();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.pack_start(&new_button);
        sidebar_header.pack_end(&menu_button);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search")
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        let list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        let list_scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_box.append(&search);
        sidebar_box.append(&list_scrolled);
        let sidebar_toolbar = adw::ToolbarView::new();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        sidebar_toolbar.set_content(Some(&sidebar_box));
        let sidebar_page =
            adw::NavigationPage::builder().title("Commonplace").child(&sidebar_toolbar).build();

        let copy_button = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy Link")
            .build();
        let trash_button = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Move to Trash")
            .build();
        let restore_button = gtk::Button::builder().label("_Restore").use_underline(true).build();
        let delete_button = gtk::Button::builder()
            .label("_Delete")
            .use_underline(true)
            .css_classes(["destructive-action"])
            .build();
        let outline_button = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-right-symbolic")
            .tooltip_text("Outline")
            .active(true)
            .build();
        let content_header = adw::HeaderBar::builder().show_title(false).build();
        content_header.pack_end(&outline_button);
        content_header.pack_end(&trash_button);
        content_header.pack_end(&delete_button);
        content_header.pack_end(&restore_button);
        content_header.pack_end(&copy_button);

        let view = editor::new_view();
        let editor_scrolled = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
        let backlinks = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        let backlinks_heading = gtk::Label::builder()
            .label("Linked From")
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_start(12)
            .margin_top(6)
            .build();
        let backlinks_section = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backlinks_section.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        backlinks_section.append(&backlinks_heading);
        backlinks_section.append(&backlinks);
        let word_count = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["caption", "dimmed", "numeric"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(3)
            .margin_bottom(3)
            .build();
        let editor_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor_box.append(&editor_scrolled);
        editor_box.append(&word_count);
        editor_box.append(&backlinks_section);
        let empty = adw::StatusPage::builder()
            .icon_name("document-edit-symbolic")
            .title("No Memo Selected")
            .description("Select a memo, or press Alt+N to write a new one")
            .build();
        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&editor_box, Some("editor"));
        let banner = adw::Banner::new(
            "A newer version of Commonplace wrote this library, so it’s read-only",
        );
        banner.set_revealed(state.read(|library| library.is_read_only()));
        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(&banner);
        content_box.append(&stack);
        let outline_list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        outline_list.set_placeholder(Some(
            &gtk::Label::builder()
                .label("No headings")
                .css_classes(["dimmed"])
                .margin_top(12)
                .build(),
        ));
        let outline_heading = gtk::Label::builder()
            .label("Outline")
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_start(12)
            .margin_top(12)
            .margin_bottom(6)
            .build();
        let outline_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outline_box.append(&outline_heading);
        outline_box.append(
            &gtk::ScrolledWindow::builder()
                .child(&outline_list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        let outline_split = adw::OverlaySplitView::builder()
            .content(&content_box)
            .sidebar(&outline_box)
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(180.0)
            .max_sidebar_width(260.0)
            .build();
        outline_button
            .bind_property("active", &outline_split, "show-sidebar")
            .bidirectional()
            .sync_create()
            .build();
        let content_toolbar = adw::ToolbarView::new();
        content_toolbar.add_top_bar(&content_header);
        content_toolbar.set_content(Some(&outline_split));
        let content_page =
            adw::NavigationPage::builder().title("Memo").child(&content_toolbar).build();

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Commonplace")
            .default_width(960)
            .default_height(640)
            .width_request(360)
            .height_request(294)
            .content(&toasts)
            .build();
        // libadwaita applies only the last matching breakpoint, so the narrow one repeats the
        // medium one's setters.
        for (width, collapse_notes) in [(900.0, false), (600.0, true)] {
            let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
                adw::BreakpointConditionLengthType::MaxWidth,
                width,
                adw::LengthUnit::Sp,
            ));
            breakpoint.add_setter(&outline_split, "collapsed", Some(&true.to_value()));
            breakpoint.add_setter(&outline_split, "show-sidebar", Some(&false.to_value()));
            if collapse_notes {
                breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
            }
            window.add_breakpoint(breakpoint);
        }

        let picker = LinkPicker::new(state.clone(), &editor_box);
        let this = Rc::new(Self {
            back: RefCell::default(),
            backlink_ids: RefCell::default(),
            backlinks,
            backlinks_section,
            content_page,
            copy_button,
            current: Cell::new(None),
            delete_button,
            forward: RefCell::default(),
            list,
            list_ids: RefCell::default(),
            loading: Cell::new(false),
            outline_list,
            outline_offsets: RefCell::default(),
            pending_save: RefCell::default(),
            picker,
            restore_button,
            search,
            show_trash: Cell::new(false),
            split,
            stack,
            state,
            toasts,
            trash_button,
            view,
            window,
            word_count,
        });
        this.connect_signals();
        this.add_actions();
        this.refresh_list();
        this.show_note(None);
        this
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn connect_destroy(&self, f: impl Fn() + 'static) {
        self.window.connect_destroy(move |_| f());
    }

    /// Opens a note in the editor, or shows the empty page for `None`.
    pub fn open(&self, id: NoteId) {
        if self.state.read(|library| library.note(id)).is_none() {
            self.toast("That note doesn’t exist");
            return;
        }
        if let Some(current) = self.current.get().filter(|&current| current != id) {
            self.back.borrow_mut().push(current);
            self.forward.borrow_mut().clear();
        }
        self.show_note(Some(id));
        self.split.set_show_content(true);
    }

    /// Returns to the most recent note in `from` that still exists, and remembers the current note
    /// in `to`.
    fn step_history(&self, from: &RefCell<Vec<NoteId>>, to: &RefCell<Vec<NoteId>>) {
        while let Some(id) = from.borrow_mut().pop() {
            if self.state.read(|library| library.note(id)).is_none() {
                continue;
            }
            if let Some(current) = self.current.get() {
                to.borrow_mut().push(current);
            }
            self.show_note(Some(id));
            self.split.set_show_content(true);
            return;
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        self.state.connect_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move || {
                this.refresh_list();
                this.refresh_backlinks();
                this.highlight_links();
            }
        ));
        self.search.connect_search_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.refresh_list()
        ));
        self.list.connect_row_activated(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, row| {
                let id = this.list_ids.borrow().get(row.index() as usize).copied();
                if let Some(id) = id {
                    this.open(id);
                    this.view.grab_focus();
                }
            }
        ));
        self.outline_list.connect_row_activated(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, row| {
                let offset = this.outline_offsets.borrow().get(row.index() as usize).copied();
                if let Some(offset) = offset {
                    let buffer = this.view.buffer();
                    buffer.place_cursor(&buffer.iter_at_offset(offset));
                    this.view.scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.1);
                    this.view.grab_focus();
                }
            }
        ));
        self.backlinks.connect_row_activated(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, row| {
                let id = this.backlink_ids.borrow().get(row.index() as usize).copied();
                if let Some(id) = id {
                    this.open(id);
                    this.view.grab_focus();
                }
            }
        ));
        self.copy_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                if let Some(id) = this.current.get() {
                    this.window.clipboard().set_text(&note_uri(id));
                    this.toast("Link copied");
                }
            }
        ));
        self.trash_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.trash_current()
        ));
        self.restore_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                if let Some(id) = this.current.get() {
                    this.report(this.state.write(|library| library.restore(id)));
                    this.show_note(Some(id));
                }
            }
        ));
        self.delete_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.confirm_delete()
        ));
        self.window.connect_close_request(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                this.save_now();
                glib::Propagation::Proceed
            }
        ));

        let buffer = self.view.buffer();
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |buffer| {
                let count = word_count(&editor::text(buffer));
                let unit = if count == 1 { "word" } else { "words" };
                this.word_count.set_label(&format!("{count} {unit}"));
                this.refresh_outline();
            }
        ));
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                if !this.loading.get() {
                    this.highlight_links();
                    this.schedule_save();
                }
            }
        ));
        buffer.connect_insert_text(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, iter, text| {
                let mut before = *iter;
                if this.loading.get() || text != "[" || !before.backward_char() {
                    return;
                }
                if before.char() == '[' {
                    // The handler runs before the text is in the buffer.
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        this,
                        move || this.pick_link(true)
                    ));
                }
            }
        ));
        self.picker.connect_closed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move || {
                this.view.grab_focus();
            }
        ));

        let click = gtk::GestureClick::new();
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, x, y| {
                if !gesture.current_event_state().contains(gdk::ModifierType::CONTROL_MASK) {
                    return;
                }
                let (x, y) = this.view.window_to_buffer_coords(
                    gtk::TextWindowType::Widget,
                    x as i32,
                    y as i32,
                );
                if let Some(iter) = this.view.iter_at_location(x, y) {
                    this.activate_at(iter.offset());
                }
            }
        ));
        self.view.add_controller(click);
        let keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let control_return = modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter);
                if !control_return {
                    return glib::Propagation::Proceed;
                }
                this.activate_at(this.view.buffer().cursor_position());
                glib::Propagation::Stop
            }
        ));
        self.view.add_controller(keys);

        // The capture phase sees Alt+Left and Alt+Right before the editor uses them to move words.
        let history_keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        history_keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let alt = modifiers & gtk::accelerator_get_default_mod_mask()
                    == gdk::ModifierType::ALT_MASK;
                match key {
                    gdk::Key::Back => this.step_history(&this.back, &this.forward),
                    gdk::Key::Left if alt => this.step_history(&this.back, &this.forward),
                    gdk::Key::Forward => this.step_history(&this.forward, &this.back),
                    gdk::Key::Right if alt => this.step_history(&this.forward, &this.back),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        ));
        self.window.add_controller(history_keys);
        let history_buttons = gtk::GestureClick::builder()
            .button(0)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        history_buttons.connect_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |gesture, _, _, _| {
                match gesture.current_button() {
                    8 => this.step_history(&this.back, &this.forward),
                    9 => this.step_history(&this.forward, &this.back),
                    _ => return,
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
            }
        ));
        self.window.add_controller(history_buttons);
    }

    fn add_actions(self: &Rc<Self>) {
        let new_memo = gio::SimpleAction::new("new-memo", None);
        new_memo.set_enabled(!self.state.read(|library| library.is_read_only()));
        new_memo.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.new_memo()
        ));
        let insert_link = gio::SimpleAction::new("insert-link", None);
        insert_link.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                if this.view.is_editable() && this.current.get().is_some() {
                    this.pick_link(false);
                }
            }
        ));
        let search = gio::SimpleAction::new("search", None);
        search.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                this.split.set_show_content(false);
                this.search.grab_focus();
            }
        ));
        let save = gio::SimpleAction::new("save", None);
        save.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.save_now()
        ));
        let show_trash = gio::SimpleAction::new_stateful("show-trash", None, &false.to_variant());
        show_trash.connect_change_state(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |action, value| {
                let Some(value) = value else { return };
                action.set_state(value);
                this.show_trash.set(value.get().unwrap_or(false));
                this.refresh_list();
            }
        ));
        for action in [new_memo, insert_link, save, search, show_trash] {
            self.window.add_action(&action);
        }
    }

    fn new_memo(&self) {
        self.save_now();
        match self.state.write(|library| library.create_memo("")) {
            Ok(id) => {
                self.search.set_text("");
                self.open(id);
                self.view.grab_focus();
            }
            Err(error) => self.report_error(&error),
        }
    }

    fn show_note(&self, id: Option<NoteId>) {
        self.save_now();
        let note = id.and_then(|id| self.state.read(|library| library.note(id)));
        self.current.set(note.as_ref().map(|note| note.id));
        self.loading.set(true);
        match &note {
            Some(note) => {
                self.view.buffer().set_text(&note.body);
                self.view.buffer().place_cursor(&self.view.buffer().start_iter());
                let writable = !note.is_trashed() && !self.state.read(|l| l.is_read_only());
                self.view.set_editable(writable);
                self.set_titles(Some(&display_label(note)));
                self.stack.set_visible_child_name("editor");
            }
            None => {
                self.view.buffer().set_text("");
                self.set_titles(None);
                self.stack.set_visible_child_name("empty");
            }
        }
        self.loading.set(false);
        let trashed = note.as_ref().is_some_and(Note::is_trashed);
        let read_only = self.state.read(|library| library.is_read_only());
        self.copy_button.set_visible(note.is_some());
        self.trash_button.set_visible(note.is_some() && !trashed && !read_only);
        self.restore_button.set_visible(trashed && !read_only);
        self.delete_button.set_visible(trashed && !read_only);
        self.highlight_links();
        self.refresh_backlinks();
        self.select_current_row();
    }

    fn refresh_list(&self) {
        let query = self.search.text();
        let show_trash = self.show_trash.get();
        let notes: Vec<Note> = self.state.read(|library| {
            let notes = library.notes().into_iter();
            notes.filter(|note| note.is_trashed() == show_trash && note.matches(&query)).collect()
        });
        self.list.remove_all();
        for note in &notes {
            self.list.append(&note_row(note));
        }
        *self.list_ids.borrow_mut() = notes.iter().map(|note| note.id).collect();
        self.select_current_row();
        if let Some(note) = self.current.get().and_then(|id| self.state.read(|l| l.note(id))) {
            self.set_titles(Some(&display_label(&note)));
        }
    }

    /// Names the window and the note page after the open note, or the app if none is open.
    fn set_titles(&self, note: Option<&str>) {
        self.content_page.set_title(note.unwrap_or("Memo"));
        self.window.set_title(Some(note.unwrap_or("Commonplace")));
    }

    fn refresh_outline(&self) {
        let body = editor::text(&self.view.buffer());
        let headings = outline(&body);
        self.outline_list.remove_all();
        for heading in &headings {
            let label = gtk::Label::builder()
                .label(&heading.text)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .margin_start(12 * i32::from(heading.level - 1))
                .build();
            if heading.level == 1 {
                label.add_css_class("heading");
            }
            self.outline_list.append(&label);
        }
        *self.outline_offsets.borrow_mut() = headings
            .iter()
            .map(|heading| body[..heading.range.start].chars().count() as i32)
            .collect();
    }

    fn select_current_row(&self) {
        let index = self
            .current
            .get()
            .and_then(|id| self.list_ids.borrow().iter().position(|&listed| listed == id));
        let row = index.and_then(|index| self.list.row_at_index(index as i32));
        self.list.select_row(row.as_ref());
    }

    fn refresh_backlinks(&self) {
        let notes = match self.current.get() {
            Some(id) => self.state.read(|library| library.backlinks(id)),
            None => Vec::new(),
        };
        self.backlinks.remove_all();
        for note in &notes {
            self.backlinks.append(&note_row(note));
        }
        self.backlinks_section.set_visible(!notes.is_empty());
        *self.backlink_ids.borrow_mut() = notes.iter().map(|note| note.id).collect();
    }

    fn highlight_links(&self) {
        editor::highlight_links(&self.view.buffer(), |link| {
            link.external_uri().is_some()
                || link.note.is_some_and(|id| self.state.read(|l| l.note(id)).is_some())
        });
    }

    fn schedule_save(self: &Rc<Self>) {
        if let Some(source) = self.pending_save.take() {
            source.remove();
        }
        let source = glib::timeout_add_local_once(
            SAVE_DELAY,
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move || {
                    this.pending_save.take();
                    this.save_now();
                }
            ),
        );
        *self.pending_save.borrow_mut() = Some(source);
    }

    fn save_now(&self) {
        if let Some(source) = self.pending_save.take() {
            source.remove();
        }
        let Some(id) = self.current.get() else {
            return;
        };
        if !self.view.is_editable() {
            return;
        }
        let body = editor::text(&self.view.buffer());
        let Some(note) = self.state.read(|library| library.note(id)) else {
            return;
        };
        if note.is_trashed() || note.body == body {
            return;
        }
        self.report(self.state.write(|library| library.set_body(id, &body)));
    }

    fn trash_current(self: &Rc<Self>) {
        let Some(id) = self.current.get() else {
            return;
        };
        self.save_now();
        if let Err(error) = self.state.write(|library| library.trash(id)) {
            self.report_error(&error);
            return;
        }
        self.show_note(None);
        self.split.set_show_content(false);
        let toast = adw::Toast::builder().title("Moved to trash").button_label("_Undo").build();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| {
                this.report(this.state.write(|library| library.restore(id)));
                this.open(id);
            }
        ));
        self.toasts.add_toast(toast);
    }

    fn confirm_delete(self: &Rc<Self>) {
        let Some(id) = self.current.get() else {
            return;
        };
        let dialog = adw::AlertDialog::new(
            Some("Delete Memo?"),
            Some("Commonplace deletes it permanently, on every device it syncs with"),
        );
        dialog.add_responses(&[("cancel", "_Cancel"), ("delete", "_Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.connect_response(
            Some("delete"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.report(this.state.write(|library| library.delete(id)));
                    this.show_note(None);
                    this.split.set_show_content(false);
                }
            ),
        );
        dialog.present(Some(&self.window));
    }

    /// Shows the link picker at the cursor. `after_brackets` means the user just typed `[[`.
    fn pick_link(self: &Rc<Self>, after_brackets: bool) {
        let buffer = self.view.buffer();
        let cursor = buffer.iter_at_mark(&buffer.get_insert());
        let location = self.view.iter_location(&cursor);
        let (x, y) = self.view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            location.x(),
            location.y(),
        );
        let at = gdk::Rectangle::new(x, y, location.width().max(1), location.height());
        let selection = buffer
            .selection_bounds()
            .map(|(start, end)| buffer.text(&start, &end, true).to_string())
            .filter(|_| !after_brackets);
        self.picker.show(
            &self.view,
            &at,
            self.current.get(),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |choice| this.insert_link(choice, after_brackets, selection.clone())
            ),
        );
    }

    fn insert_link(&self, choice: Choice, after_brackets: bool, selection: Option<String>) {
        let (id, note_label) = match choice {
            Choice::Note(id, label) => (id, label),
            Choice::Create(text) => match self.state.write(|library| library.create_memo(&text)) {
                Ok(id) => (id, label(&text)),
                Err(error) => {
                    self.report_error(&error);
                    return;
                }
            },
        };
        let link = wikilink(id, selection.as_deref().or(note_label.as_deref()));
        let buffer = self.view.buffer();
        if after_brackets {
            buffer.insert_at_cursor(&link[2..]);
        } else {
            buffer.delete_selection(true, true);
            buffer.insert_at_cursor(&link);
        }
    }

    /// Follows the link at the character `offset`, or else toggles the checkbox of the list item
    /// there, adding one if it has none.
    fn activate_at(&self, offset: i32) {
        if let Some(link) = editor::link_at(&self.view.buffer(), offset) {
            self.follow(&link);
        } else if self.view.is_editable() {
            editor::toggle_checkbox_at(&self.view.buffer(), offset);
        }
    }

    fn follow(&self, link: &Link) {
        if let Some(uri) = link.external_uri() {
            let toasts = self.toasts.downgrade();
            gtk::UriLauncher::new(uri).launch(
                Some(&self.window),
                gio::Cancellable::NONE,
                move |result| {
                    let dismissed =
                        |error: &glib::Error| error.matches(gtk::DialogError::Dismissed);
                    if let (Err(error), Some(toasts)) = (result, toasts.upgrade())
                        && !dismissed(&error)
                    {
                        toasts.add_toast(adw::Toast::new(&format!(
                            "Couldn’t open the link: {error}"
                        )));
                    }
                },
            );
            return;
        }
        match link.note {
            Some(id) if self.state.read(|library| library.note(id)).is_some() => self.open(id),
            _ => self.toast("That link doesn’t lead to a note"),
        }
    }

    fn toast(&self, title: &str) {
        self.toasts.add_toast(adw::Toast::new(title));
    }

    fn report(&self, result: Result<(), Error>) {
        if let Err(error) = result {
            self.report_error(&error);
        }
    }

    fn report_error(&self, error: &Error) {
        self.toast(&error.to_string());
    }
}

fn note_row(note: &Note) -> gtk::Box {
    let title = gtk::Label::builder()
        .label(display_label(note))
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let subtitle = gtk::Label::builder()
        .label(format_time(note.modified))
        .xalign(0.0)
        .css_classes(["caption", "dimmed"])
        .build();
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    row.append(&title);
    row.append(&subtitle);
    row
}

fn display_label(note: &Note) -> String {
    note.label().unwrap_or_else(|| EMPTY_LABEL.to_owned())
}

fn format_time(milliseconds: i64) -> String {
    let (Ok(time), Ok(now)) = (
        glib::DateTime::from_unix_local(milliseconds.div_euclid(1000)),
        glib::DateTime::now_local(),
    ) else {
        return String::new();
    };
    let format = if time.ymd() == now.ymd() {
        "%H:%M"
    } else if time.year() == now.year() {
        "%b %-d"
    } else {
        "%Y-%m-%d"
    };
    time.format(format).map(String::from).unwrap_or_default()
}
