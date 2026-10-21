// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::NoteId;
use gtk::{gdk, glib, graphene};

use crate::rows::display_label;
use crate::state::State;

const MAX_RESULTS: usize = 20;

#[derive(Clone, Debug)]
pub enum Choice {
    /// A new task in the inbox with this text.
    CreateAction(String),
    /// A new memo with this text.
    CreateMemo(String),
    Note(NoteId, Option<String>),
    /// No parent: the top level of the action hierarchy.
    TopLevel,
}

/// What the user picks a note for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    /// A note to link to, or a new memo or action to create and link to.
    Link,
    /// An action to move an action under, or the top level.
    Parent,
}

type PickHandler = Box<dyn Fn(Choice)>;

/// A popover that searches the library for a note to link to.
pub struct LinkPicker {
    choices: RefCell<Vec<Choice>>,
    entry: gtk::SearchEntry,
    exclude: RefCell<Vec<NoteId>>,
    list: gtk::ListBox,
    on_pick: RefCell<Option<PickHandler>>,
    popover: gtk::Popover,
    purpose: Cell<Purpose>,
    state: Rc<State>,
}

impl LinkPicker {
    /// Creates a picker inside `parent`. A `GtkTextView` can't be the parent: when the view is
    /// disposed, it loops forever trying to remove children that it didn't add itself.
    pub fn new(state: Rc<State>, parent: &gtk::Box) -> Rc<Self> {
        let entry = gtk::SearchEntry::builder().placeholder_text("Link to…").build();
        let list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(320)
            .min_content_width(320)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&entry);
        content.append(&scrolled);
        let popover = gtk::Popover::builder().child(&content).build();
        popover.set_parent(parent);

        let picker = Rc::new(Self {
            choices: RefCell::default(),
            entry,
            exclude: RefCell::default(),
            list,
            on_pick: RefCell::default(),
            popover,
            purpose: Cell::new(Purpose::Link),
            state,
        });

        picker.entry.connect_search_changed(glib::clone!(
            #[weak]
            picker,
            move |_| picker.refresh()
        ));
        picker.entry.connect_activate(glib::clone!(
            #[weak]
            picker,
            move |_| {
                let row = picker.list.selected_row().or_else(|| picker.list.row_at_index(0));
                if let Some(row) = row {
                    picker.pick(row.index());
                }
            }
        ));
        picker.entry.connect_stop_search(glib::clone!(
            #[weak]
            picker,
            move |_| picker.popover.popdown()
        ));
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            picker,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let step = match key {
                    gdk::Key::Down => 1,
                    gdk::Key::Up => -1,
                    _ => return glib::Propagation::Proceed,
                };
                let current = picker.list.selected_row().map_or(-1, |row| row.index());
                if let Some(row) = picker.list.row_at_index((current + step).max(0)) {
                    picker.list.select_row(Some(&row));
                }
                glib::Propagation::Stop
            }
        ));
        picker.entry.add_controller(keys);
        picker.list.connect_row_activated(glib::clone!(
            #[weak]
            picker,
            move |_, row| picker.pick(row.index())
        ));
        picker
    }

    /// Shows the picker pointing at `at`, a rectangle in `widget`'s coordinates, leaving out the
    /// notes in `exclude`, and calls `on_pick` with the user's choice.
    pub fn show(
        &self,
        widget: &impl IsA<gtk::Widget>,
        at: &gdk::Rectangle,
        purpose: Purpose,
        exclude: Vec<NoteId>,
        on_pick: impl Fn(Choice) + 'static,
    ) {
        self.purpose.set(purpose);
        self.entry.set_placeholder_text(Some(match purpose {
            Purpose::Link => "Link to…",
            Purpose::Parent => "Move to…",
        }));
        *self.exclude.borrow_mut() = exclude;
        *self.on_pick.borrow_mut() = Some(Box::new(on_pick));
        self.entry.set_text("");
        self.refresh();
        let parent = self.popover.parent().expect("the picker has a parent");
        let origin = graphene::Point::new(at.x() as f32, at.y() as f32);
        let origin = widget.compute_point(&parent, &origin).unwrap_or(origin);
        let at = gdk::Rectangle::new(origin.x() as i32, origin.y() as i32, at.width(), at.height());
        self.popover.set_pointing_to(Some(&at));
        self.popover.popup();
        self.entry.grab_focus();
    }

    pub fn connect_closed(&self, f: impl Fn() + 'static) {
        self.popover.connect_closed(move |_| f());
    }

    fn refresh(&self) {
        let query = self.entry.text().trim().to_owned();
        let purpose = self.purpose.get();
        let exclude = self.exclude.borrow().clone();
        let notes: Vec<(Choice, String)> = self.state.read(|library| {
            library
                .search(&query)
                .into_iter()
                .filter(|note| !exclude.contains(&note.id))
                .filter(|note| purpose == Purpose::Link || note.action.is_some())
                .take(MAX_RESULTS)
                .map(|note| (Choice::Note(note.id, note.label()), display_label(&note)))
                .collect()
        });
        let mut choices = Vec::new();
        if purpose == Purpose::Parent {
            choices.push((Choice::TopLevel, "Top level".to_owned()));
        }
        choices.extend(notes);
        if purpose == Purpose::Link && !query.is_empty() {
            choices.push((Choice::CreateMemo(query.clone()), format!("New memo: {query}")));
            choices.push((Choice::CreateAction(query.clone()), format!("New action: {query}")));
        }
        self.list.remove_all();
        for (choice, text) in &choices {
            let label = gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            if matches!(choice, Choice::CreateAction(_) | Choice::CreateMemo(_) | Choice::TopLevel)
            {
                label.add_css_class("accent");
            }
            self.list.append(&label);
        }
        // Once the user types, Enter picks the first match rather than the top level.
        let first_match = usize::from(
            !query.is_empty() && matches!(choices.first(), Some((Choice::TopLevel, _))),
        );
        let first = self.list.row_at_index(first_match as i32).or(self.list.row_at_index(0));
        if let Some(row) = first {
            self.list.select_row(Some(&row));
        }
        *self.choices.borrow_mut() = choices.into_iter().map(|(choice, _)| choice).collect();
    }

    fn pick(&self, index: i32) {
        let choice =
            usize::try_from(index).ok().and_then(|index| self.choices.borrow().get(index).cloned());
        let Some(choice) = choice else {
            return;
        };
        let on_pick = self.on_pick.borrow_mut().take();
        self.popover.popdown();
        if let Some(on_pick) = on_pick {
            on_pick(choice);
        }
    }
}
