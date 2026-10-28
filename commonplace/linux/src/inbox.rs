// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use commonplace_core::{
    ActionChange, ActionList, ActionState, ActionView, EditBase, Error, NoteId, Spaces,
};
use gtk::{gdk, glib};

use crate::editor;
use crate::libraries;
use crate::link_picker::{Choice, LinkPicker, Purpose};
use crate::rows::{connect_day_clicked, day_picker, format_date, parent_labels, today};
use crate::state::State;

const SAVE_DELAY: Duration = Duration::from_millis(500);

/// Sorts the inbox one action at a time. The user can edit the action and its fields, then sends
/// it to a list or closes it out, and the next action takes its place.
pub struct InboxView {
    current: Cell<Option<NoteId>>,
    due_button: gtk::MenuButton,
    /// The version of the library that the editor's text reflects.
    edit_base: RefCell<Option<EditBase>>,
    /// Whether the user has typed in the editor since its text was last loaded or saved. Only
    /// typed text is saved, so the editor never writes an old copy over a change made elsewhere.
    edited: Cell<bool>,
    editor: sourceview5::View,
    empty: adw::StatusPage,
    link_titles: editor::LinkTitles,
    /// Set while the view changes widgets itself, so their change signals don't echo back.
    loading: Cell<bool>,
    path: gtk::Label,
    pending_save: RefCell<Option<glib::SourceId>>,
    picker: Rc<LinkPicker>,
    progress: gtk::Label,
    project: gtk::Switch,
    /// Actions the user skipped this session, which wait until the user starts over.
    skipped: RefCell<HashSet<NoteId>>,
    start_over: gtk::Button,
    state: Rc<State>,
    toasts: adw::ToastOverlay,
    waiting_for: gtk::Entry,
    pub widget: gtk::Stack,
}

impl InboxView {
    pub fn new(state: Rc<State>, toasts: adw::ToastOverlay) -> Rc<Self> {
        let progress = gtk::Label::builder().css_classes(["dimmed"]).build();
        let heading = gtk::CenterBox::builder()
            .start_widget(
                &gtk::Label::builder().label("Sort the inbox").css_classes(["title-3"]).build(),
            )
            .end_widget(&progress)
            .build();

        let editor = editor::new_view();
        let editor_frame = gtk::ScrolledWindow::builder()
            .child(&editor)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(160)
            .max_content_height(320)
            .propagate_natural_height(true)
            .css_classes(["card"])
            .build();

        let dimmed = |text: &str| gtk::Label::builder().label(text).css_classes(["dimmed"]).build();
        let clear_due = gtk::Button::with_mnemonic("_Clear");
        let (due_button, due_calendar) = day_picker("None", Some(clear_due.upcast_ref()));
        let (schedule_button, schedule_calendar) = day_picker("Sc_hedule…", None);
        let project = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let path = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let move_button = gtk::Button::with_mnemonic("Mo_ve…");

        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let picker = LinkPicker::new(state.clone(), &card);

        let waiting_for = gtk::Entry::builder().placeholder_text("Who or what").build();
        let wait = gtk::Button::builder().label("Wait").css_classes(["suggested-action"]).build();
        let waiting_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        waiting_content.append(&waiting_for);
        waiting_content.append(&wait);
        let waiting_popover = gtk::Popover::builder().child(&waiting_content).build();
        let waiting_button = gtk::MenuButton::builder()
            .label("_Waiting…")
            .use_underline(true)
            .popover(&waiting_popover)
            .build();

        let empty = adw::StatusPage::builder().icon_name("mail-unread-symbolic").build();
        let start_over = gtk::Button::builder()
            .label("_Start over")
            .use_underline(true)
            .halign(gtk::Align::Center)
            .css_classes(["pill"])
            .build();
        empty.set_child(Some(&start_over));

        let scrolled = gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(720).child(&card).build())
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let widget = gtk::Stack::new();
        widget.add_named(&scrolled, Some("item"));
        widget.add_named(&empty, Some("empty"));

        let view = Rc::new(Self {
            current: Cell::new(None),
            due_button,
            edit_base: RefCell::default(),
            edited: Cell::new(false),
            editor,
            empty,
            link_titles: editor::LinkTitles::default(),
            loading: Cell::new(false),
            path,
            pending_save: RefCell::default(),
            picker,
            progress,
            project,
            skipped: RefCell::default(),
            start_over,
            state,
            toasts,
            waiting_for,
            widget,
        });

        connect_day_clicked(
            &view.due_button,
            &due_calendar,
            glib::clone!(
                #[weak]
                view,
                move |day| view.change(ActionChange::Due(Some(day)))
            ),
        );
        clear_due.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| {
                view.due_button.popdown();
                view.change(ActionChange::Due(None));
            }
        ));
        connect_day_clicked(
            &schedule_button,
            &schedule_calendar,
            glib::clone!(
                #[weak]
                view,
                move |day| {
                    view.sort(|library, id| {
                        library.change_action(id, ActionChange::Defer(Some(day)))?;
                        library.change_action(id, ActionChange::List(ActionList::Active))
                    })
                }
            ),
        );

        let fields = gtk::Box::builder().spacing(12).build();
        fields.append(&dimmed("Due"));
        fields.append(&view.due_button);
        fields.append(&dimmed("Project"));
        fields.append(&view.project);
        fields.append(&dimmed("In"));
        fields.append(&view.path);
        fields.append(&move_button);

        let button = |label: &str| gtk::Button::with_mnemonic(label);
        let active = button("_Active");
        active.add_css_class("suggested-action");
        let someday = button("_Someday");
        let maybe = button("_Maybe");
        let done = button("_Done");
        let archive = button("A_rchive");
        let trash = button("_Trash");
        trash.add_css_class("destructive-action");
        let skip = button("S_kip");
        skip.set_hexpand(true);
        skip.set_halign(gtk::Align::End);
        let sort_row = gtk::Box::builder().spacing(6).build();
        sort_row.append(&dimmed("Sort onto"));
        for widget in [
            active.upcast_ref::<gtk::Widget>(),
            waiting_button.upcast_ref(),
            schedule_button.upcast_ref(),
            someday.upcast_ref(),
            maybe.upcast_ref(),
        ] {
            sort_row.append(widget);
        }
        let close_row = gtk::Box::builder().spacing(6).build();
        close_row.append(&dimmed("Or"));
        for widget in [&done, &archive, &trash, &skip] {
            close_row.append(widget);
        }

        card.append(&heading);
        card.append(&editor_frame);
        card.append(&fields);
        card.append(&sort_row);
        card.append(&close_row);
        if view.state.read(|library| library.is_read_only()) {
            view.editor.set_editable(false);
            for row in [&fields, &sort_row, &close_row] {
                row.set_sensitive(false);
            }
        }

        for (button, list) in [
            (&active, ActionList::Active),
            (&someday, ActionList::Someday),
            (&maybe, ActionList::Maybe),
        ] {
            button.connect_clicked(glib::clone!(
                #[weak]
                view,
                move |_| view
                    .sort(move |library, id| library.change_action(id, ActionChange::List(list)))
            ));
        }
        let send_waiting = glib::clone!(
            #[weak]
            view,
            #[weak]
            waiting_popover,
            move || {
                let who = view.waiting_for.text().trim().to_owned();
                waiting_popover.popdown();
                view.sort(move |library, id| {
                    library.change_action(id, ActionChange::WaitingFor(Some(who)))?;
                    library.change_action(id, ActionChange::List(ActionList::Waiting))
                });
            }
        );
        wait.connect_clicked(glib::clone!(
            #[strong]
            send_waiting,
            move |_| send_waiting()
        ));
        view.waiting_for.connect_activate(move |_| send_waiting());
        waiting_popover.connect_show(glib::clone!(
            #[weak]
            view,
            move |_| {
                view.waiting_for.set_text("");
                view.waiting_for.grab_focus();
            }
        ));
        done.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.sort(|library, id| {
                library.change_action(id, ActionChange::State(ActionState::Done))
            })
        ));
        archive.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.sort(|library, id| library.set_archived(id, true))
        ));
        trash.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| view.trash_current()
        ));
        skip.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| {
                view.save_now();
                if let Some(id) = view.current.get() {
                    view.skipped.borrow_mut().insert(id);
                }
                view.refresh();
            }
        ));
        view.start_over.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |_| {
                view.skipped.borrow_mut().clear();
                view.refresh();
            }
        ));
        view.project.connect_active_notify(glib::clone!(
            #[weak]
            view,
            move |project| view.change(ActionChange::Project(project.is_active()))
        ));
        move_button.connect_clicked(glib::clone!(
            #[weak]
            view,
            move |button| view.pick_parent(button)
        ));
        view.editor.buffer().connect_changed(glib::clone!(
            #[weak]
            view,
            move |_| {
                if !view.loading.get() {
                    view.edited.set(true);
                    view.schedule_save();
                }
            }
        ));
        view.refresh();
        view
    }

    /// Shows the first action in the inbox that the user hasn't skipped, keeping the one on show
    /// if it's still waiting to be sorted.
    pub fn refresh(&self) {
        let queue: Vec<NoteId> = self.state.read(|library| {
            library
                .action_view(ActionView::Inbox, today())
                .into_iter()
                .map(|entry| entry.note.id)
                .collect()
        });
        let skipped = self.skipped.borrow().clone();
        let queue: Vec<NoteId> = queue.into_iter().filter(|id| !skipped.contains(id)).collect();
        let current = self.current.get().filter(|id| queue.contains(id)).or(queue.first().copied());
        let Some(id) = current else {
            self.save_now();
            self.current.set(None);
            let (title, description) = if skipped.is_empty() {
                ("Inbox is empty", "Tasks you capture wait here until you sort them")
            } else {
                ("Nothing left to sort", "Start over to sort the actions you skipped")
            };
            self.empty.set_title(title);
            self.empty.set_description(Some(description));
            self.start_over.set_visible(!skipped.is_empty());
            self.widget.set_visible_child_name("empty");
            return;
        };
        let Some(note) = self.state.read(|library| library.note(id)) else {
            return;
        };
        let action = note.action.clone().unwrap_or_default();
        self.loading.set(true);
        if self.current.get() != Some(id) {
            self.save_now();
            self.current.set(Some(id));
            let text = self.state.read(|spaces| self.link_titles.expand(spaces, &note.body));
            crate::link_icons::load(&self.editor, &text);
            self.editor.buffer().place_cursor(&self.editor.buffer().start_iter());
            *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(id));
        } else if !self.edited.get() && self.stored_text() != note.body {
            let text = self.state.read(|spaces| self.link_titles.expand(spaces, &note.body));
            crate::link_icons::load(&self.editor, &text);
            *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(id));
        } else {
            self.state.read(|spaces| self.link_titles.update(&self.editor.buffer(), spaces));
        }
        self.project.set_active(action.project);
        self.loading.set(false);
        self.due_button.set_label(&action.due.map_or_else(|| "None".to_owned(), format_date));
        let parents = self.state.read(|library| library.action_paths().remove(&id));
        let labels: Vec<String> = parent_labels(&self.state, &parents.unwrap_or_default())
            .into_iter()
            .map(|(_, label)| label)
            .collect();
        self.path.set_label(&if labels.is_empty() {
            "Top level".to_owned()
        } else {
            labels.join(" › ")
        });
        let more = queue.len() - 1;
        self.progress.set_label(&match more {
            0 => "Last one".to_owned(),
            more => format!("{more} more"),
        });
        self.widget.set_visible_child_name("item");
    }

    /// Changes a field of the action on show, leaving it on show.
    fn change(&self, change: ActionChange) {
        if self.loading.get() {
            return;
        }
        if let Some(id) = self.current.get() {
            self.report(self.state.write(|library| library.change_action(id, change)));
        }
    }

    /// Saves the action on show and applies `sort` to it, which takes it out of the inbox, so the
    /// next action takes its place.
    fn sort(&self, sort: impl FnOnce(&mut Spaces, NoteId) -> Result<(), Error>) {
        let Some(id) = self.current.get() else {
            return;
        };
        self.save_now();
        self.report(self.state.write(|library| sort(library, id)));
        self.refresh();
    }

    fn trash_current(&self) {
        self.sort(|library, id| library.trash(id));
    }

    fn pick_parent(self: &Rc<Self>, button: &gtk::Button) {
        let Some(id) = self.current.get() else {
            return;
        };
        let mut exclude = self.state.read(|library| library.action_descendants(id));
        exclude.push(id);
        let at = gdk::Rectangle::new(0, 0, button.width(), button.height());
        self.picker.show(
            button,
            &at,
            Purpose::Parent,
            exclude,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                move |choice| {
                    let parent = match choice {
                        Choice::Note(parent) => Some(parent),
                        Choice::TopLevel => None,
                        Choice::CreateAction(_) | Choice::CreateMemo(_) => return,
                    };
                    let report = glib::clone!(
                        #[weak]
                        view,
                        move |result| view.report(result)
                    );
                    libraries::move_action(&view.state, &view.widget, id, parent, report);
                }
            ),
        );
    }

    fn schedule_save(self: &Rc<Self>) {
        if let Some(source) = self.pending_save.take() {
            source.remove();
        }
        let source = glib::timeout_add_local_once(
            SAVE_DELAY,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                move || {
                    view.pending_save.take();
                    view.save_now();
                }
            ),
        );
        *self.pending_save.borrow_mut() = Some(source);
    }

    /// Saves the text of the action on show, if it changed.
    pub fn save_now(&self) {
        if let Some(source) = self.pending_save.take() {
            source.remove();
        }
        let Some(id) = self.current.get().filter(|_| self.edited.replace(false)) else {
            return;
        };
        let body = self.stored_text();
        let unchanged =
            self.state.read(|library| library.note(id)).is_none_or(|note| note.body == body);
        if unchanged {
            return;
        }
        let base = self.edit_base.borrow().clone();
        let Some(base) = base else {
            self.report(self.state.write(|library| library.set_body(id, &body)));
            return;
        };
        match self.state.write(|spaces| spaces.set_body_since(id, &body, &base)) {
            Ok((base, merged)) => {
                *self.edit_base.borrow_mut() = Some(base);
                if merged && !self.edited.get() {
                    self.load_stored_body(id);
                }
            }
            Err(error) => self.report(Err(error)),
        }
    }

    /// Shows the stored body of note `id` in place of the editor's text, once another device's
    /// changes merge into it.
    fn load_stored_body(&self, id: NoteId) {
        let Some(note) = self.state.read(|spaces| spaces.note(id)) else {
            return;
        };
        if self.stored_text() != note.body {
            let buffer = self.editor.buffer();
            let offset = buffer.cursor_position();
            let text = self.state.read(|spaces| self.link_titles.expand(spaces, &note.body));
            self.loading.set(true);
            crate::link_icons::load(&self.editor, &text);
            buffer.place_cursor(&buffer.iter_at_offset(offset));
            self.loading.set(false);
        }
        *self.edit_base.borrow_mut() = self.state.read(|spaces| spaces.edit_base(id));
    }

    /// The editor's text as the note stores it, without the titles it shows in links.
    fn stored_text(&self) -> String {
        let text = editor::text(&self.editor.buffer());
        self.state.read(|spaces| self.link_titles.collapse(spaces, &text))
    }

    fn report(&self, result: Result<(), Error>) {
        if let Err(error) = result {
            self.toasts.add_toast(adw::Toast::new(&error.to_string()));
        }
    }
}
