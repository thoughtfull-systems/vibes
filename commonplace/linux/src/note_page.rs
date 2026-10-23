// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use commonplace_core::{Error, Link, Note, NoteId, label, note_uri, outline, wikilink, word_count};
use gtk::{gdk, gio, glib};

use crate::action_panel::ActionPanel;
use crate::bottom_pane::BottomPane;
use crate::editor;
use crate::libraries;
use crate::link_picker::{Choice, LinkPicker, Purpose};
use crate::rows::{display_label, entry, note_row, today};
use crate::state::{Space, State};

const BOTTOM_HEIGHT: &str = "linux.bottom.height";
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// The narrowest the details sidebar gets.
const DETAILS_MIN: i32 = 200;

type Handler = Rc<dyn Fn()>;
type OpenHandler = Rc<dyn Fn(NoteId)>;

/// The view that edits one note, with its outline beside it and its backlinks below.
pub struct NotePage {
    backlink_ids: RefCell<Vec<NoteId>>,
    backlinks: gtk::ListBox,
    backlinks_section: gtk::Box,
    /// Divides the outline from the rest.
    body: gtk::Paned,
    bottom: Rc<BottomPane>,
    copy_link: gio::SimpleAction,
    current: Cell<Option<NoteId>>,
    delete_button: gtk::Button,
    details: gtk::Box,
    /// A width to give the details once they show, restored from an earlier session.
    details_restore: Rc<Cell<Option<i32>>>,
    /// The details' width when they last showed.
    details_width: Rc<Cell<i32>>,
    /// Set while the view changes widgets itself, so their change signals don't echo back.
    loading: Cell<bool>,
    on_leave: RefCell<Option<Handler>>,
    note_menu_button: gtk::MenuButton,
    on_trashed: RefCell<Option<Handler>>,
    on_open: RefCell<Option<OpenHandler>>,
    outline_box: gtk::Box,
    outline_list: gtk::ListBox,
    /// The outline's width when it last showed.
    outline_width: Rc<Cell<i32>>,
    /// The character offset where each heading in the outline starts.
    outline_offsets: RefCell<Vec<i32>>,
    pub widget: gtk::Box,
    pending_save: RefCell<Option<glib::SourceId>>,
    picker: Rc<LinkPicker>,
    action_panel: Rc<ActionPanel>,
    restore_button: gtk::Button,
    /// A button for each library, the one that holds the note on, to move the note to another.
    space_row: adw::WrapBox,
    /// The note, the libraries, and the note's library that the space buttons last showed. The
    /// buttons move the note they were built for, so a different note needs new ones.
    space_row_shown: RefCell<Option<(NoteId, Vec<Space>, String)>>,
    state: Rc<State>,
    toasts: adw::ToastOverlay,
    /// Offers to restore or delete a note in the trash.
    trash_bar: gtk::Box,
    trash_reason: gtk::Label,
    trash_note: gio::SimpleAction,
    view: sourceview5::View,
    window: adw::ApplicationWindow,
    word_count: gtk::Label,
}

impl NotePage {
    pub fn new(
        state: Rc<State>,
        window: adw::ApplicationWindow,
        toasts: adw::ToastOverlay,
    ) -> Rc<Self> {
        let copy_link = gio::SimpleAction::new("copy-link", None);
        let trash_note = gio::SimpleAction::new("trash-note", None);
        let restore_button = gtk::Button::builder().label("_Restore").use_underline(true).build();
        let delete_button = gtk::Button::builder()
            .label("_Delete")
            .use_underline(true)
            .css_classes(["destructive-action"])
            .build();
        let trash_bar = gtk::Box::builder().spacing(6).css_classes(["toolbar"]).build();
        let trash_reason = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .css_classes(["dimmed"])
            .margin_start(6)
            .build();
        trash_bar.append(&trash_reason);
        trash_bar.append(&restore_button);
        trash_bar.append(&delete_button);
        let flat_button = |icon: &str, tooltip: &str| {
            gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tooltip)
                .css_classes(["flat"])
                .build()
        };
        let hide_outline = flat_button("sidebar-show-symbolic", "Hide Outline");
        let show_outline = flat_button("sidebar-show-symbolic", "Show Outline");
        let sidebar_bar = gtk::Box::builder().margin_top(6).margin_start(6).margin_end(6).build();
        hide_outline.set_hexpand(true);
        hide_outline.set_halign(gtk::Align::End);
        sidebar_bar.append(&hide_outline);
        // Stands in for the sidebar's button while the sidebar is hidden.
        let hidden_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_start(6)
            .build();
        hidden_bar.append(&show_outline);

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
        let backlinks_separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        backlinks_section.append(&backlinks_separator);
        backlinks_section.append(&backlinks_heading);
        backlinks_section.append(
            &gtk::ScrolledWindow::builder()
                .child(&backlinks)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .max_content_height(200)
                .vexpand(true)
                .build(),
        );
        let word_count = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["dimmed", "numeric", "word-count"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(3)
            .margin_bottom(3)
            .build();
        let editor_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let note_menu = gio::Menu::new();
        note_menu.append(Some("_Copy Link"), Some("win.copy-link"));
        note_menu.append(Some("_Move to Trash"), Some("win.trash-note"));
        let note_menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Note Menu")
            .menu_model(&note_menu)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(6)
            .margin_end(6)
            .css_classes(["flat"])
            .build();
        // Room for the menu button, so text never runs under it.
        view.set_right_margin(48);
        let editor_overlay = gtk::Overlay::builder().child(&editor_scrolled).build();
        editor_overlay.add_overlay(&note_menu_button);
        let text_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["view"])
            .build();
        text_box.append(&editor_overlay);
        text_box.append(&word_count);
        let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bottom_pane = BottomPane::new(&text_box, &bottom);
        editor_box.append(&bottom_pane.widget);

        let outline_list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        outline_list.set_placeholder(Some(
            &gtk::Label::builder()
                .label("No headings")
                .css_classes(["dimmed"])
                .margin_top(12)
                .build(),
        ));
        let outline_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(150)
            .css_classes(["sidebar-pane"])
            .build();
        outline_box.append(&sidebar_bar);
        outline_box.append(
            &gtk::ScrolledWindow::builder()
                .child(&outline_list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );

        let hide_details = flat_button("sidebar-show-right-symbolic", "Hide Details");
        let details_bar = gtk::Box::builder().margin_top(6).margin_start(6).margin_end(6).build();
        details_bar.append(&hide_details);
        let details_scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let details = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(DETAILS_MIN)
            .css_classes(["sidebar-pane"])
            .build();
        details.append(&details_bar);
        details.append(&details_scrolled);
        let show_details = flat_button("sidebar-show-right-symbolic", "Show Details");
        let details_hidden_bar =
            gtk::Box::builder().valign(gtk::Align::Start).margin_top(6).margin_end(6).build();
        details_hidden_bar.append(&show_details);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&hidden_bar);
        content.append(&editor_box);
        content.append(&details_hidden_bar);
        editor_box.set_hexpand(true);
        let with_details = gtk::Paned::builder()
            .wide_handle(true)
            .start_child(&content)
            .end_child(&details)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();
        let body = gtk::Paned::builder()
            .wide_handle(true)
            .start_child(&outline_box)
            .end_child(&with_details)
            .position(220)
            .resize_start_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();

        let narrow = adw::BreakpointBin::builder().width_request(360).height_request(294).build();
        narrow.set_child(Some(&body));
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&narrow);

        let picker = LinkPicker::new(state.clone(), &editor_box);
        let action_panel = ActionPanel::new(state.clone(), picker.clone());
        let space_row = adw::WrapBox::builder()
            .child_spacing(6)
            .line_spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .visible(false)
            .build();
        let details_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        details_content.append(&space_row);
        details_content.append(&action_panel.widget);
        details_scrolled.set_child(Some(&details_content));

        // Whether the user, or the window's width, leaves each sidebar shown. Only actions, and
        // notes with libraries to move between, have details to show.
        let outline_shown = Rc::new(Cell::new(true));
        let details_shown = Rc::new(Cell::new(true));
        let outline_width = Rc::new(Cell::new(0));
        let details_width = Rc::new(Cell::new(0));
        let update_sidebars: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[strong]
            outline_shown,
            #[strong]
            details_shown,
            #[strong]
            outline_width,
            #[strong]
            details_width,
            #[weak]
            outline_box,
            #[weak]
            hidden_bar,
            #[weak]
            details,
            #[weak]
            details_hidden_bar,
            #[weak(rename_to = panel)]
            action_panel.widget,
            #[weak]
            space_row,
            move || {
                if outline_box.get_visible() && outline_box.width() > 0 {
                    outline_width.set(outline_box.width());
                }
                if details.get_visible() && details.width() > 0 {
                    details_width.set(details.width());
                }
                outline_box.set_visible(outline_shown.get());
                hidden_bar.set_visible(!outline_shown.get());
                let any = panel.get_visible() || space_row.get_visible();
                details.set_visible(any && details_shown.get());
                details_hidden_bar.set_visible(any && !details_shown.get());
            }
        ));
        for (button, flag, shown) in [
            (&hide_outline, &outline_shown, false),
            (&show_outline, &outline_shown, true),
            (&hide_details, &details_shown, false),
            (&show_details, &details_shown, true),
        ] {
            let flag = flag.clone();
            let update = update_sidebars.clone();
            button.connect_clicked(move |_| {
                flag.set(shown);
                update();
            });
        }
        for details in [action_panel.widget.upcast_ref::<gtk::Widget>(), space_row.upcast_ref()] {
            details.connect_visible_notify(glib::clone!(
                #[strong]
                update_sidebars,
                move |_| update_sidebars()
            ));
        }
        // libadwaita applies only the last matching breakpoint, so the narrow one hides both
        // sidebars itself. The outline gives way first, since the details matter more.
        for (width, hides_details) in [(1000.0, false), (700.0, true)] {
            let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
                adw::BreakpointConditionLengthType::MaxWidth,
                width,
                adw::LengthUnit::Sp,
            ));
            let show = glib::clone!(
                #[strong]
                outline_shown,
                #[strong]
                details_shown,
                #[strong]
                update_sidebars,
                move |shown: bool| {
                    outline_shown.set(shown);
                    if hides_details {
                        details_shown.set(shown);
                    }
                    update_sidebars();
                }
            );
            let hide = show.clone();
            breakpoint.connect_apply(move |_| hide(false));
            breakpoint.connect_unapply(move |_| show(true));
            narrow.add_breakpoint(breakpoint);
        }
        update_sidebars();
        let details_restore = Rc::new(Cell::new(None::<i32>));
        // GtkPaned measures from the start, so the details' width becomes a position only once
        // the details show and the pane knows its own width.
        with_details.connect_max_position_notify(glib::clone!(
            #[strong]
            details_restore,
            #[weak]
            details,
            move |paned| {
                // Until the pane's first allocation, its maximum is a placeholder.
                if details.get_visible()
                    && paned.max_position() < i32::MAX
                    && let Some(width) = details_restore.take()
                {
                    paned.set_position(paned.max_position() + DETAILS_MIN - width);
                }
            }
        ));
        editor_box.prepend(&trash_bar);
        for action in [&copy_link, &trash_note] {
            window.add_action(action);
        }
        bottom.append(&action_panel.steps_section);
        bottom.append(&backlinks_section);
        // The pane shows its handle only while the panel shows, and the handle stands in for the
        // separator above whichever section comes first.
        let update_bottom = glib::clone!(
            #[weak]
            bottom,
            #[weak(rename_to = steps)]
            action_panel.steps_section,
            #[weak]
            backlinks_section,
            #[weak]
            backlinks_separator,
            move || {
                bottom.set_visible(steps.get_visible() || backlinks_section.get_visible());
                backlinks_separator.set_visible(steps.get_visible());
            }
        );
        for section in [&action_panel.steps_section, &backlinks_section] {
            let update = update_bottom.clone();
            section.connect_visible_notify(move |_| update());
        }
        update_bottom();
        let this = Rc::new(Self {
            backlink_ids: RefCell::default(),
            backlinks,
            backlinks_section,
            body,
            bottom: bottom_pane,
            copy_link,
            current: Cell::new(None),
            delete_button,
            details,
            details_restore,
            details_width,
            loading: Cell::new(false),
            on_leave: RefCell::default(),
            note_menu_button,
            on_trashed: RefCell::default(),
            on_open: RefCell::default(),
            outline_box,
            outline_list,
            outline_width,
            outline_offsets: RefCell::default(),
            widget,
            pending_save: RefCell::default(),
            picker,
            action_panel,
            restore_button,
            space_row,
            space_row_shown: RefCell::default(),
            state,
            toasts,
            trash_bar,
            trash_reason,
            trash_note,
            view,
            window,
            word_count,
        });
        this.connect_signals();
        adw::StyleManager::default().connect_dark_notify(glib::clone!(
            #[weak]
            this,
            move |_| this.refresh_outline()
        ));
        this
    }

    /// Calls `f` when the user presses Escape, or deletes the open note, so the view has nothing
    /// to show.
    pub fn connect_leave(&self, f: impl Fn() + 'static) {
        *self.on_leave.borrow_mut() = Some(Rc::new(f));
    }

    /// Calls `f` when the user moves the open note to the trash.
    pub fn connect_trashed(&self, f: impl Fn() + 'static) {
        *self.on_trashed.borrow_mut() = Some(Rc::new(f));
    }

    /// The widths of the outline and the details, as they show now or last showed.
    pub fn sidebar_widths(&self) -> (i32, i32) {
        let width = |sidebar: &gtk::Box, last: &Cell<i32>| {
            if sidebar.get_visible() && sidebar.width() > 0 { sidebar.width() } else { last.get() }
        };
        (width(&self.outline_box, &self.outline_width), width(&self.details, &self.details_width))
    }

    /// Gives the outline and the details these widths.
    pub fn set_sidebar_widths(&self, outline: i32, details: i32) {
        self.body.set_position(outline);
        self.outline_width.set(outline);
        self.details_width.set(details);
        self.details_restore.set(Some(details));
    }

    /// Calls `f` when the user asks to open another note from this one, by a link, a backlink,
    /// or a breadcrumb.
    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    pub fn show(self: &Rc<Self>, id: NoteId) {
        self.show_note(Some(id));
    }

    /// Saves the open note and forgets it, once the view is gone.
    pub fn close(self: &Rc<Self>) {
        self.show_note(None);
    }

    pub fn focus_editor(&self) {
        self.view.grab_focus();
    }

    fn request_open(&self, id: NoteId) {
        let handler = self.on_open.borrow().clone();
        if let Some(handler) = handler {
            handler(id);
        }
    }

    /// Shows the open note again, after the library changes.
    pub fn refresh(self: &Rc<Self>) {
        if self.current.get().is_none() {
            return;
        }
        self.refresh_space_row();
        self.refresh_backlinks();
        self.highlight_links();
        self.action_panel.refresh();
        self.update_buttons();
        self.bottom.update();
    }

    pub fn insert_link(self: &Rc<Self>) {
        if self.view.is_editable() && self.current.get().is_some() {
            self.pick_link(false);
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        self.bottom.connect_resized(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |height| {
                let Some(id) = this.current.get() else { return };
                if !this.state.read(|library| library.is_read_only()) {
                    this.report(this.state.write(|library| match height {
                        Some(height) => {
                            library.set_note_setting(id, BOTTOM_HEIGHT, i64::from(height))
                        }
                        None => library.remove_note_setting(id, BOTTOM_HEIGHT),
                    }));
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
                    this.request_open(id);
                    this.view.grab_focus();
                }
            }
        ));
        self.copy_link.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| {
                if let Some(id) = this.current.get() {
                    this.window.clipboard().set_text(&note_uri(id));
                    this.toast("Link copied");
                }
            }
        ));
        self.trash_note.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.trash_current()
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
        self.action_panel.connect_open(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |id| this.request_open(id)
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
        // In the bubble phase, an open popover, such as the link picker, uses Escape first.
        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let plain = (modifiers & gtk::accelerator_get_default_mod_mask()).is_empty();
                if key != gdk::Key::Escape || !plain {
                    return glib::Propagation::Proceed;
                }
                this.save_now();
                this.leave();
                glib::Propagation::Stop
            }
        ));
        self.widget.add_controller(escape);
    }

    fn show_note(self: &Rc<Self>, id: Option<NoteId>) {
        self.save_now();
        let note = id.and_then(|id| self.state.read(|library| library.note(id)));
        self.current.set(note.as_ref().map(|note| note.id));
        self.loading.set(true);
        match &note {
            Some(note) => {
                self.view.buffer().set_text(&note.body);
                self.view.buffer().place_cursor(&self.view.buffer().start_iter());
                let writable = !self.state.read(|l| l.is_in_trash(note.id) || l.is_read_only());
                self.view.set_editable(writable);
            }
            None => self.view.buffer().set_text(""),
        }
        self.loading.set(false);
        self.action_panel.show(note.as_ref(), self.view.is_editable());
        self.refresh_space_row();
        self.update_buttons();
        self.highlight_links();
        self.refresh_backlinks();
        let height = note.as_ref().and_then(|note| {
            self.state.read(|library| library.note_setting(note.id, BOTTOM_HEIGHT))
        });
        self.bottom.show(height.and_then(|height| i32::try_from(height).ok()));
    }

    /// Names the window after the open note while the view shows, and shows the buttons that apply
    /// to the note.
    fn update_buttons(&self) {
        let note = self.current.get().and_then(|id| self.state.read(|library| library.note(id)));
        if let Some(note) = note.as_ref().filter(|_| self.widget.is_child_visible()) {
            self.window.set_title(Some(&crate::window_title(&display_label(note))));
        }
        let in_trash =
            note.as_ref().is_some_and(|note| self.state.read(|l| l.is_in_trash(note.id)));
        let trashed = note.as_ref().is_some_and(Note::is_trashed);
        let read_only = self.state.read(|library| library.is_read_only());
        let writable = note.is_some() && !in_trash && !read_only;
        self.copy_link.set_enabled(note.is_some());
        self.trash_note.set_enabled(writable);
        self.trash_bar.set_visible(in_trash && !read_only);
        self.trash_reason.set_label(if trashed {
            "This note is in the trash"
        } else {
            "In the trash with a project above it, which restoring brings back too"
        });
        self.note_menu_button.set_visible(note.is_some() && !in_trash);
    }

    /// Shows a button for each library while there's more than one and the note can move, with
    /// the note's own library on. Turning another on moves the note there.
    fn refresh_space_row(self: &Rc<Self>) {
        let id = self.current.get();
        let spaces = self.state.spaces();
        let space = id.and_then(|id| self.state.read(|s| s.space_of(id).map(str::to_owned)));
        let movable =
            id.is_some_and(|id| !self.state.read(|s| s.is_in_trash(id) || s.is_read_only()));
        let shown = id
            .zip(space)
            .filter(|_| movable && spaces.len() > 1)
            .map(|(id, space)| (id, spaces, space));
        if *self.space_row_shown.borrow() == shown {
            return;
        }
        self.space_row_shown.replace(shown.clone());
        self.space_row.remove_all();
        let Some((id, spaces, space)) = shown else {
            self.space_row.set_visible(false);
            return;
        };
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for choice in spaces {
            let button = gtk::ToggleButton::builder()
                .label(&choice.name)
                .active(choice.key == space)
                .tooltip_text(format!("Move to {}", choice.name))
                .build();
            button.set_group(buttons.first());
            button.connect_toggled(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |button| {
                    let here = this.state.read(|s| s.space_of(id).map(str::to_owned));
                    if !button.is_active() || here.as_deref() == Some(choice.key.as_str()) {
                        return;
                    }
                    let key = choice.key.clone();
                    libraries::confirm_move(
                        &this.window,
                        &choice.name,
                        glib::clone!(
                            #[weak]
                            this,
                            move || {
                                this.save_now();
                                this.report(this.state.write(|s| s.move_to_space(id, &key)));
                            }
                        ),
                        glib::clone!(
                            #[weak]
                            this,
                            move || {
                                this.space_row_shown.take();
                                this.refresh_space_row();
                            }
                        ),
                    );
                }
            ));
            self.space_row.append(&button);
            buttons.push(button);
        }
        self.space_row.set_visible(true);
    }

    fn refresh_outline(&self) {
        let body = editor::text(&self.view.buffer());
        let headings = outline(&body);
        self.outline_list.remove_all();
        let dark = adw::StyleManager::default().is_dark();
        for heading in &headings {
            let color = editor::heading_color(heading.level, dark);
            let text = glib::markup_escape_text(&heading.text);
            let label = gtk::Label::builder()
                .label(format!("<span foreground=\"{color}\">{text}</span>"))
                .use_markup(true)
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

    fn refresh_backlinks(&self) {
        let (notes, dates, paths) = match self.current.get() {
            Some(id) => {
                self.state.read(|l| (l.backlinks(id), l.action_dates(today()), l.action_paths()))
            }
            None => Default::default(),
        };
        self.backlinks.remove_all();
        for note in &notes {
            let entry = entry(note.clone(), &dates, &paths);
            self.backlinks.append(&note_row(&self.state, &entry, true));
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

    pub fn save_now(&self) {
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

    fn leave(&self) {
        let handler = self.on_leave.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn trash_current(&self) {
        let Some(id) = self.current.get() else {
            return;
        };
        self.save_now();
        if let Err(error) = self.state.write(|library| library.trash(id)) {
            self.report_error(&error);
            return;
        }
        let handler = self.on_trashed.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn confirm_delete(self: &Rc<Self>) {
        let Some(id) = self.current.get() else {
            return;
        };
        let dialog = adw::AlertDialog::new(
            Some("Delete Note?"),
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
                    this.leave();
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
            Purpose::Link,
            self.current.get().into_iter().collect(),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |choice| this.put_link(choice, after_brackets, selection.clone())
            ),
        );
    }

    fn put_link(&self, choice: Choice, after_brackets: bool, selection: Option<String>) {
        let (id, note_label) = match choice {
            Choice::Note(id, label) => (id, label),
            Choice::CreateAction(ref text) | Choice::CreateMemo(ref text) => {
                let action = matches!(choice, Choice::CreateAction(_));
                let Some(space) = self
                    .current
                    .get()
                    .and_then(|id| self.state.read(|s| s.space_of(id).map(str::to_owned)))
                else {
                    return;
                };
                let created = self.state.write(|spaces| {
                    if action {
                        spaces.create_inbox_task(&space, text)
                    } else {
                        spaces.create_memo(&space, text)
                    }
                });
                match created {
                    Ok(id) => (id, label(text)),
                    Err(error) => {
                        self.report_error(&error);
                        return;
                    }
                }
            }
            Choice::TopLevel => return,
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
            Some(id) if self.state.read(|library| library.note(id)).is_some() => {
                self.request_open(id)
            }
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
