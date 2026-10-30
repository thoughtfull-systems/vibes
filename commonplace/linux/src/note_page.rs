// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use commonplace_core::{EditBase, Error, Link, Note, NoteId, note_uri, wikilink, word_count};
use gtk::{gdk, gio, glib};

use crate::action_panel::ActionPanel;
use crate::bottom_pane::BottomPane;
use crate::editor;
use crate::entry_panel::EntryPanel;
use crate::journal_panel::JournalPanel;
use crate::libraries;
use crate::link_picker::{Choice, LinkPicker, Purpose};
use crate::rows::{display_label, entry, note_row, star_toggle, today};
use crate::state::{Space, State};

const BOTTOM_HEIGHT: &str = "linux.bottom.height";
const SAVE_DELAY: Duration = Duration::from_millis(500);
/// The narrowest the details sidebar gets.
const DETAILS_MIN: i32 = 200;

type Handler = Rc<dyn Fn()>;
type OpenHandler = Rc<dyn Fn(NoteId)>;

/// What the space buttons show. The buttons move the note they were built for, so a different
/// note needs new ones.
#[derive(Clone, PartialEq)]
struct SpaceRow {
    id: NoteId,
    movable: bool,
    space: String,
    spaces: Vec<Space>,
}

/// The view that edits one note, with its details beside it and its backlinks below.
pub struct NotePage {
    /// Archives the note, or takes it out of the archive.
    archive_button: gtk::Button,
    backlink_ids: RefCell<Vec<NoteId>>,
    backlinks: gtk::ListBox,
    backlinks_section: gtk::Box,
    bottom: Rc<BottomPane>,
    /// Turns a memo into an entry in the journal its menu picks.
    convert_button: gtk::MenuButton,
    convert_memo: gio::SimpleAction,
    copy_link: gio::SimpleAction,
    entry_panel: Rc<EntryPanel>,
    journal_panel: Rc<JournalPanel>,
    current: Cell<Option<NoteId>>,
    delete_note: gio::SimpleAction,
    details: gtk::Box,
    /// A width to give the details once they show, restored from an earlier session.
    details_restore: Rc<Cell<Option<i32>>>,
    /// The details' width when they last showed.
    details_width: Rc<Cell<i32>>,
    /// The version of the library that the editor's text reflects.
    edit_base: RefCell<Option<EditBase>>,
    link_titles: editor::LinkTitles,
    /// Set while the view changes widgets itself, so their change signals don't echo back.
    loading: Cell<bool>,
    /// Stars a memo. An action's panel has a star of its own, among its other fields.
    memo_star: gtk::ToggleButton,
    memo_star_row: gtk::Grid,
    on_leave: RefCell<Option<Handler>>,
    note_menu: gio::Menu,
    note_menu_button: gtk::MenuButton,
    timestamps: gtk::Label,
    on_open: RefCell<Option<OpenHandler>>,
    pub widget: gtk::Box,
    pending_save: RefCell<Option<glib::SourceId>>,
    picker: Rc<LinkPicker>,
    action_panel: Rc<ActionPanel>,
    restore_button: gtk::Button,
    /// Shows in place of the word count while the text hasn't saved.
    retry_save: gtk::Button,
    /// A button for each library, the one that holds the note on, to move the note to another.
    space_row: adw::WrapBox,
    space_row_shown: RefCell<Option<SpaceRow>>,
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
        let convert_memo = gio::SimpleAction::new("convert-memo", Some(glib::VariantTy::STRING));
        let copy_link = gio::SimpleAction::new("copy-link", None);
        let trash_note = gio::SimpleAction::new("trash-note", None);
        let restore_button = gtk::Button::builder().label("_Restore").use_underline(true).build();
        let delete_note = gio::SimpleAction::new("delete-note", None);
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
        let flat_button = |icon: &str, tooltip: &str| {
            gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(tooltip)
                .css_classes(["flat"])
                .build()
        };
        let view = editor::new_view();
        let editor_scrolled = gtk::ScrolledWindow::builder().child(&view).vexpand(true).build();
        let backlinks = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        let backlinks_heading = gtk::Label::builder()
            .label("Linked from")
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
        note_menu.append(Some("_Copy link"), Some("win.copy-link"));
        note_menu.append(Some("_Move to trash"), Some("win.trash-note"));
        let note_menu_button = gtk::MenuButton::builder()
            .icon_name(icon!("more"))
            .tooltip_text("Note menu")
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
        let retry_save = gtk::Button::builder()
            .label("Retry save")
            .halign(gtk::Align::End)
            .hexpand(true)
            .margin_end(6)
            .visible(false)
            .css_classes(["flat"])
            .build();
        word_count.set_hexpand(true);
        let status = gtk::Box::builder().height_request(32).spacing(6).build();
        status.append(&word_count);
        status.append(&retry_save);
        text_box.append(&status);
        let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bottom_pane = BottomPane::new(&text_box, &bottom);
        editor_box.append(&bottom_pane.widget);

        let hide_details = flat_button(icon!("sidebar"), "Hide details");
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
        let show_details = flat_button(icon!("sidebar"), "Show details");
        let details_hidden_bar =
            gtk::Box::builder().valign(gtk::Align::Start).margin_top(6).margin_end(6).build();
        details_hidden_bar.append(&show_details);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
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
        let narrow = adw::BreakpointBin::builder().width_request(360).height_request(294).build();
        narrow.set_child(Some(&with_details));
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
        let memo_star = star_toggle();
        memo_star.set_halign(gtk::Align::Start);
        let memo_star_label = gtk::Label::builder()
            .label("Starred?")
            .xalign(0.0)
            .css_classes(["dimmed"])
            .mnemonic_widget(&memo_star)
            .build();
        let memo_star_row = gtk::Grid::builder()
            .column_spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .visible(false)
            .build();
        memo_star_row.attach(&memo_star_label, 0, 0, 1, 1);
        memo_star_row.attach(&memo_star, 1, 0, 1, 1);
        details_content.append(&memo_star_row);
        let journal_panel = JournalPanel::new(state.clone());
        details_content.append(&journal_panel.widget);
        let entry_panel = EntryPanel::new(state.clone());
        details_content.append(&entry_panel.widget);
        let convert_button = gtk::MenuButton::builder()
            .label("Make journal entry")
            .tooltip_text("Turn this memo into an entry, leaving a link to it here")
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .halign(gtk::Align::Start)
            .visible(false)
            .build();
        details_content.append(&convert_button);
        let timestamps = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption", "dimmed"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .valign(gtk::Align::End)
            .vexpand(true)
            .visible(false)
            .build();
        details_content.append(&timestamps);
        let archive_button = gtk::Button::builder()
            .label("Archive")
            .valign(gtk::Align::End)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(12)
            .visible(false)
            .build();
        details_content.append(&archive_button);
        details_scrolled.set_child(Some(&details_content));

        // Whether the user, or the window's width, leaves each sidebar shown.
        let details_shown = Rc::new(Cell::new(true));
        let details_width = Rc::new(Cell::new(0));
        let update_sidebars: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[strong]
            details_shown,
            #[strong]
            details_width,
            #[weak]
            details,
            #[weak]
            details_hidden_bar,
            #[weak(rename_to = panel)]
            action_panel.widget,
            #[weak]
            space_row,
            #[weak]
            timestamps,
            move || {
                if details.get_visible() && details.width() > 0 {
                    details_width.set(details.width());
                }
                let any =
                    panel.get_visible() || space_row.get_visible() || timestamps.get_visible();
                details.set_visible(any && details_shown.get());
                details_hidden_bar.set_visible(any && !details_shown.get());
            }
        ));
        for (button, flag, shown) in
            [(&hide_details, &details_shown, false), (&show_details, &details_shown, true)]
        {
            let flag = flag.clone();
            let update = update_sidebars.clone();
            button.connect_clicked(move |_| {
                flag.set(shown);
                update();
            });
        }
        for details in [
            action_panel.widget.upcast_ref::<gtk::Widget>(),
            journal_panel.widget.upcast_ref(),
            entry_panel.widget.upcast_ref(),
            convert_button.upcast_ref(),
            space_row.upcast_ref(),
            archive_button.upcast_ref(),
            timestamps.upcast_ref(),
        ] {
            details.connect_visible_notify(glib::clone!(
                #[strong]
                update_sidebars,
                move |_| update_sidebars()
            ));
        }
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            700.0,
            adw::LengthUnit::Sp,
        ));
        let show = glib::clone!(
            #[strong]
            details_shown,
            #[strong]
            update_sidebars,
            move |shown: bool| {
                details_shown.set(shown);
                update_sidebars();
            }
        );
        let hide = show.clone();
        breakpoint.connect_apply(move |_| hide(false));
        breakpoint.connect_unapply(move |_| show(true));
        narrow.add_breakpoint(breakpoint);
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
        for action in [&convert_memo, &copy_link, &delete_note, &trash_note] {
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
            archive_button,
            backlink_ids: RefCell::default(),
            backlinks,
            backlinks_section,
            bottom: bottom_pane,
            convert_button,
            convert_memo,
            copy_link,
            entry_panel,
            journal_panel,
            current: Cell::new(None),
            delete_note,
            details,
            details_restore,
            details_width,
            edit_base: RefCell::default(),
            link_titles: editor::LinkTitles::default(),
            loading: Cell::new(false),
            memo_star,
            memo_star_row,
            on_leave: RefCell::default(),
            note_menu,
            note_menu_button,
            timestamps,
            on_open: RefCell::default(),
            widget,
            pending_save: RefCell::default(),
            picker,
            action_panel,
            restore_button,
            retry_save,
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
        this
    }

    /// Calls `f` when the user presses Escape.
    pub fn connect_leave(&self, f: impl Fn() + 'static) {
        *self.on_leave.borrow_mut() = Some(Rc::new(f));
    }

    pub fn details_width(&self) -> i32 {
        if self.details.get_visible() && self.details.width() > 0 {
            self.details.width()
        } else {
            self.details_width.get()
        }
    }

    pub fn set_details_width(&self, details: i32) {
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
        let Some(id) = self.current.get() else {
            return;
        };
        let writable = self.writable(id);
        self.view.set_editable(writable);
        self.loading.set(true);
        self.state.read(|spaces| self.link_titles.update(&self.view.buffer(), spaces));
        self.loading.set(false);
        if self.pending_save.borrow().is_none() {
            self.load_stored_body(id);
        }
        self.refresh_space_row();
        self.refresh_backlinks();
        self.highlight_links();
        self.action_panel.refresh(writable);
        let note = self.state.read(|spaces| spaces.note(id));
        self.journal_panel.show(note.as_ref(), writable);
        self.entry_panel.show(note.as_ref(), writable);
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
        self.archive_button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.toggle_archived()
        ));
        self.memo_star.connect_toggled(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |star| {
                let Some(id) = this.current.get().filter(|_| !this.loading.get()) else {
                    return;
                };
                if let Err(error) =
                    this.state.write(|library| library.set_starred(id, star.is_active()))
                {
                    this.report_error(&error);
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
                }
            }
        ));
        self.delete_note.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, _| this.confirm_delete()
        ));
        self.entry_panel.connect_open(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |id| this.request_open(id)
        ));
        self.convert_memo.connect_activate(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_, parameter| {
                let journal = parameter
                    .and_then(|value| value.get::<String>())
                    .and_then(|id| id.parse().ok());
                if let (Some(memo), Some(journal)) = (this.current.get(), journal) {
                    this.save_now();
                    match this.state.write(|spaces| spaces.convert_memo(memo, journal)) {
                        Ok(entry) => this.request_open(entry),
                        Err(error) => this.report_error(&error),
                    }
                }
            }
        ));
        self.action_panel.connect_open(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |id| this.request_open(id)
        ));

        self.retry_save.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.save_now()
        ));
        let buffer = self.view.buffer();
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |buffer| {
                let count = word_count(&editor::text(buffer));
                let unit = if count == 1 { "word" } else { "words" };
                this.word_count.set_label(&format!("{count} {unit}"));
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

    /// Entries move with their journal, so they get no space buttons.
    fn is_entry(&self, id: NoteId) -> bool {
        self.state.read(|spaces| spaces.note(id)).is_some_and(|note| note.entry.is_some())
    }

    /// Offers the journals in `memo`'s library to turn it into an entry in, or hides the menu.
    fn refresh_convert_menu(&self, memo: Option<&Note>) {
        let journals: Vec<Note> = memo
            .map(|memo| {
                let space = self.state.read(|spaces| spaces.space_of(memo.id).map(str::to_owned));
                self.state
                    .read(|spaces| spaces.journals())
                    .into_iter()
                    .filter(|journal| {
                        self.state.read(|spaces| spaces.space_of(journal.id).map(str::to_owned))
                            == space
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.convert_button.set_visible(!journals.is_empty());
        let menu = gio::Menu::new();
        for journal in journals {
            let item = gio::MenuItem::new(journal.label().as_deref(), None);
            item.set_action_and_target_value(
                Some("win.convert-memo"),
                Some(&journal.id.to_string().to_variant()),
            );
            menu.append_item(&item);
        }
        self.convert_button.set_menu_model(Some(&menu));
    }

    fn writable(&self, id: NoteId) -> bool {
        !self.state.read(|library| library.is_in_trash(id) || library.is_read_only())
    }

    fn show_note(self: &Rc<Self>, id: Option<NoteId>) {
        self.save_now();
        let note = id.and_then(|id| self.state.read(|library| library.note(id)));
        self.current.set(note.as_ref().map(|note| note.id));
        self.loading.set(true);
        self.show_save_failed(false);
        match &note {
            Some(note) => {
                let text = self.state.read(|spaces| self.link_titles.expand(spaces, &note.body));
                crate::link_icons::load(&self.view, &text);
                *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(note.id));
                self.view.buffer().place_cursor(&self.view.buffer().start_iter());
                self.view.set_editable(self.writable(note.id));
            }
            None => crate::link_icons::load(&self.view, ""),
        }
        self.loading.set(false);
        self.action_panel.show(note.as_ref(), self.view.is_editable());
        self.journal_panel.show(note.as_ref(), self.view.is_editable());
        self.entry_panel.show(note.as_ref(), self.view.is_editable());
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
            self.window.set_title(Some(&crate::window_title(
                &self.state.read(|spaces| display_label(spaces, note)),
            )));
        }
        let in_trash =
            note.as_ref().is_some_and(|note| self.state.read(|l| l.is_in_trash(note.id)));
        let trashed = note.as_ref().is_some_and(Note::is_trashed);
        let read_only = self.state.read(|library| library.is_read_only());
        let writable = note.is_some() && !in_trash && !read_only;
        self.copy_link.set_enabled(note.is_some());
        let archived = note.as_ref().is_some_and(|note| note.archived.is_some());
        let (label, tooltip) = if archived {
            ("Unarchive", "Take this note out of the archive")
        } else {
            ("Archive", "Set this note aside, out of every list, without changing it")
        };
        self.archive_button.set_label(label);
        self.archive_button.set_tooltip_text(Some(tooltip));
        self.archive_button.set_visible(writable);
        let memo = note.as_ref().filter(|note| note.is_memo());
        self.memo_star_row.set_visible(writable && memo.is_some());
        let loading = self.loading.replace(true);
        self.memo_star.set_active(memo.is_some_and(|note| note.starred.is_some()));
        self.loading.set(loading);
        self.trash_note.set_enabled(writable);
        self.trash_bar.set_visible(in_trash && !read_only);
        self.trash_reason.set_label(if trashed {
            "This note is in the trash"
        } else {
            "In the trash with a project above it, which restoring brings back too"
        });
        self.note_menu_button.set_visible(note.is_some());
        self.delete_note.set_enabled(in_trash && !read_only);
        self.note_menu.remove_all();
        self.note_menu.append(Some("_Copy link"), Some("win.copy-link"));
        if in_trash {
            self.note_menu.append(Some("_Delete permanently…"), Some("win.delete-note"));
        } else {
            self.note_menu.append(Some("_Move to trash"), Some("win.trash-note"));
        }
        self.timestamps.set_visible(note.is_some());
        if let Some(note) = &note {
            let date = |at: i64| {
                glib::DateTime::from_unix_local(at.div_euclid(1000))
                    .and_then(|at| at.format("%b %-d, %Y · %H:%M"))
                    .map(String::from)
                    .unwrap_or_default()
            };
            self.timestamps.set_label(&format!(
                "Created {}\nModified {}",
                date(note.created),
                date(note.modified),
            ));
        }
        self.refresh_convert_menu(note.as_ref().filter(|note| note.is_memo() && writable));
    }

    /// Shows a button for each library while there's more than one and the libraries can change,
    /// with the note's own library on. Turning another on moves the note there. A note in the
    /// trash shows its library, but can't move.
    fn refresh_space_row(self: &Rc<Self>) {
        let id = self.current.get();
        let spaces = self.state.spaces();
        let space = id.and_then(|id| self.state.read(|s| s.space_of(id).map(str::to_owned)));
        let read_only = self.state.read(|library| library.is_read_only());
        let movable = id.is_some_and(|id| self.writable(id));
        let shown = id
            .zip(space)
            .filter(|(id, _)| !read_only && spaces.len() > 1 && !self.is_entry(*id))
            .map(|(id, space)| SpaceRow { id, movable, space, spaces });
        if *self.space_row_shown.borrow() == shown {
            return;
        }
        self.space_row_shown.replace(shown.clone());
        self.space_row.remove_all();
        let Some(SpaceRow { id, movable, space, spaces }) = shown else {
            self.space_row.set_visible(false);
            return;
        };
        self.space_row.set_sensitive(movable);
        let mut buttons: Vec<gtk::ToggleButton> = Vec::new();
        for choice in spaces {
            let button = gtk::ToggleButton::builder()
                .label(&choice.name)
                .active(choice.key == space)
                .build();
            if movable {
                button.set_tooltip_text(Some(&format!("Move to {}", choice.name)));
            }
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

    /// Shows Retry save in place of the word count, or the word count again.
    fn show_save_failed(&self, failed: bool) {
        self.retry_save.set_visible(failed);
        self.word_count.set_visible(!failed);
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
        let text = editor::text(&self.view.buffer());
        let body = self.state.read(|spaces| self.link_titles.collapse(spaces, &text));
        let Some(note) = self.state.read(|library| library.note(id)) else {
            return;
        };
        if note.is_trashed() || note.body == body {
            return;
        }
        let base = self.edit_base.borrow().clone();
        let Some(base) = base else {
            let result = self.state.write(|library| library.set_body(id, &body));
            self.show_save_failed(result.is_err());
            self.report(result);
            return;
        };
        match self.state.write(|spaces| spaces.set_body_since(id, &body, &base)) {
            Ok((base, merged)) => {
                self.show_save_failed(false);
                *self.edit_base.borrow_mut() = Some(base);
                if merged {
                    self.load_stored_body(id);
                }
            }
            Err(error) => {
                self.show_save_failed(true);
                self.report(Err(error));
            }
        }
    }

    /// Shows the stored body of note `id` in place of the editor's text, if they differ, as they do
    /// once another device's changes arrive. The cursor keeps its offset.
    fn load_stored_body(&self, id: NoteId) {
        let Some(note) = self.state.read(|spaces| spaces.note(id)) else {
            return;
        };
        let buffer = self.view.buffer();
        let shown = editor::text(&buffer);
        if self.state.read(|spaces| self.link_titles.collapse(spaces, &shown)) == note.body {
            *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(id));
            return;
        }
        let offset = buffer.cursor_position();
        let text = self.state.read(|spaces| self.link_titles.expand(spaces, &note.body));
        self.loading.set(true);
        crate::link_icons::load(&self.view, &text);
        buffer.place_cursor(&buffer.iter_at_offset(offset));
        self.loading.set(false);
        *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(id));
        self.highlight_links();
    }

    fn leave(&self) {
        let handler = self.on_leave.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn toggle_archived(&self) {
        let Some(id) = self.current.get() else {
            return;
        };
        let archived = self.state.read(|library| library.note(id)).and_then(|note| note.archived);
        self.save_now();
        if let Err(error) = self.state.write(|library| library.set_archived(id, archived.is_none()))
        {
            self.report_error(&error);
        }
    }

    fn trash_current(&self) {
        let Some(id) = self.current.get() else {
            return;
        };
        self.save_now();
        if let Err(error) = self.state.write(|library| library.trash(id)) {
            self.report_error(&error);
        }
    }

    fn confirm_delete(self: &Rc<Self>) {
        let Some(id) = self.current.get() else {
            return;
        };
        let dialog = adw::AlertDialog::new(
            Some("Delete this note permanently?"),
            Some(
                "Commonplace deletes this note permanently, including its steps or journal \
                entries, on every device it syncs with.",
            ),
        );
        dialog.add_responses(&[("cancel", "_Cancel"), ("delete", "_Delete permanently")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.connect_response(
            Some("delete"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.report(this.state.write(|library| library.delete(id)));
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
        let id = match choice {
            Choice::Note(id) => id,
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
                    Ok(id) => id,
                    Err(error) => {
                        self.report_error(&error);
                        return;
                    }
                }
            }
            Choice::TopLevel => return,
        };
        // Saving stores the link without a label unless the selection differs from the title.
        let title = self.state.read(|spaces| spaces.link_title(id));
        let link = wikilink(id, selection.as_deref().or(title.as_deref()));
        let buffer = self.view.buffer();
        if after_brackets {
            buffer.insert_at_cursor(&link[2..]);
        } else {
            buffer.delete_selection(true, true);
            buffer.insert_at_cursor(&link);
        }
    }

    /// Follows the link at the cursor while the note's text has focus.
    pub fn follow_link_at_cursor(&self) {
        if self.view.has_focus() {
            self.follow_current_link();
        }
    }

    fn follow_current_link(&self) {
        let buffer = self.view.buffer();
        if let Some(link) = editor::link_at(&buffer, buffer.cursor_position()) {
            self.follow(&link);
        }
    }

    /// Ticks the checkbox of the list item at the cursor, or adds one, while the note's text has
    /// focus.
    pub fn toggle_checkbox_at_cursor(&self) {
        let buffer = self.view.buffer();
        if self.view.has_focus() && self.view.is_editable() {
            editor::toggle_checkbox_at(&buffer, buffer.cursor_position());
        }
    }

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

#[cfg(test)]
impl NotePage {
    pub(crate) fn check_details_and_trash(self: &Rc<Self>, id: NoteId) {
        self.show(id);
        assert!(self.timestamps.text().starts_with("Created "));
        assert!(self.timestamps.text().contains("\nModified "));
        assert_eq!(self.timestamps.next_sibling(), Some(self.archive_button.clone().upcast()));
        assert_eq!(self.note_menu.n_items(), 2);
        self.state.write(|spaces| spaces.trash(id)).unwrap();
        self.show(id);
        assert!(self.trash_bar.get_visible());
        assert!(self.restore_button.next_sibling().is_none());
        assert_eq!(
            self.note_menu.item_attribute_value(1, "action", None).unwrap().get::<String>(),
            Some("win.delete-note".into())
        );
        assert!(self.delete_note.is_enabled());
        self.state.write(|spaces| spaces.restore(id)).unwrap();
    }
}
