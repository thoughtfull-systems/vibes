// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use commonplace_core::{ActionChange, ActionList, ActionState, Date, Note, NoteId};
use gtk::{gdk, glib};

use crate::drag_scroll::DragScroll;
use crate::libraries;
use crate::link_picker::{Choice, LinkPicker, Purpose};
use crate::rows::{connect_day_clicked, format_date, note_row, parent_labels, today};
use crate::state::State;

const LISTS: [(&str, ActionList); 5] = [
    ("Inbox", ActionList::Inbox),
    ("Active", ActionList::Active),
    ("Waiting", ActionList::Waiting),
    ("Someday", ActionList::Someday),
    ("Maybe", ActionList::Maybe),
];

const SHOW_DONE: &str = "linux.steps.show_done";

type OpenHandler = Rc<dyn Fn(NoteId)>;

/// The fields of the open action, shown above its text, and its steps, shown below.
pub struct ActionPanel {
    current: Cell<Option<NoteId>>,
    defer: DateButton,
    done: gtk::Switch,
    /// A line over the steps list that shows where a dragged step would go between two steps.
    drop_line: gtk::Box,
    due: DateButton,
    editable: Cell<bool>,
    /// Names the projects whose start or due dates apply, if they aren't the action's own.
    inherited: gtk::Label,
    /// Set while the panel changes its widgets itself, so their signals don't echo back.
    loading: Cell<bool>,
    list: gtk::DropDown,
    /// The lists the list picker offers, in its order. The inbox is among them only while the
    /// open action started out there, because only capture puts actions in the inbox.
    lists: RefCell<Vec<ActionList>>,
    move_button: gtk::Button,
    on_open: RefCell<Option<OpenHandler>>,
    /// The actions the open action belongs to, outermost first, each a button that opens it.
    path: gtk::Label,
    picker: Rc<LinkPicker>,
    planning: gtk::Switch,
    planning_label: gtk::Label,
    project: gtk::Switch,
    /// The scroll position to put the steps list back to once it's laid out again.
    scroll_back: Cell<Option<f64>>,
    show_done: gtk::Switch,
    show_done_label: gtk::Label,
    /// Show done choices that a read-only library can't save, by action.
    show_done_unsaved: RefCell<HashMap<NoteId, bool>>,
    /// Where the steps list shows a dragged step would go, if anywhere.
    shown_drop: Cell<Option<(i32, Place)>>,
    state: Rc<State>,
    step_entry: gtk::Entry,
    step_ids: RefCell<Vec<NoteId>>,
    /// The parent of each step in the list, in the list's order.
    step_parents: RefCell<Vec<Option<NoteId>>>,
    /// A step to focus after the steps list is rebuilt, because it just moved.
    step_to_focus: Cell<Option<NoteId>>,
    /// A step to scroll into view once the steps list is laid out again, because it's new.
    step_to_reveal: Cell<Option<NoteId>>,
    steps: gtk::ListBox,
    /// The steps list, its header, and the entry for new steps, which an action that can't
    /// change and has no steps leaves out.
    steps_box: gtk::Box,
    steps_drag_scroll: Rc<DragScroll>,
    /// The action whose steps the list shows.
    steps_of: Cell<Option<NoteId>>,
    steps_scroll: gtk::ScrolledWindow,
    /// The projects the action belongs to, and its steps.
    pub steps_section: gtk::Box,
    waiting_for: gtk::Entry,
    /// A pending save of the waiting-for text, which waits for a pause in typing.
    waiting_save: RefCell<Option<glib::SourceId>>,
    waiting_label: gtk::Label,
    /// The fields, for a sidebar beside the action's text.
    pub widget: gtk::Box,
}

/// A button that shows a date, with a popover to pick or clear it.
struct DateButton {
    button: gtk::MenuButton,
    calendar: gtk::Calendar,
    clear: gtk::Button,
}

impl DateButton {
    fn new() -> Self {
        let calendar = gtk::Calendar::new();
        let clear = gtk::Button::builder().label("_Clear").use_underline(true).build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&calendar);
        content.append(&clear);
        // An autohide popover consumes the click that closes it, so a click on the other date
        // button would only close this one. The panel closes these popovers itself instead.
        let popover = gtk::Popover::builder().child(&content).autohide(false).build();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            popover,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key != gdk::Key::Escape {
                    return glib::Propagation::Proceed;
                }
                popover.popdown();
                glib::Propagation::Stop
            }
        ));
        popover.add_controller(keys);
        // A popover without autohide doesn't take the keyboard focus, so Escape would go to the
        // note behind it instead of closing it.
        popover.connect_show(glib::clone!(
            #[weak]
            calendar,
            move |_| {
                calendar.grab_focus();
            }
        ));
        let button =
            gtk::MenuButton::builder().label("None").hexpand(true).popover(&popover).build();
        Self { button, calendar, clear }
    }

    fn show(&self, date: Option<Date>) {
        self.button.set_label(&date.map_or_else(|| "None".to_owned(), format_date));
        let shown = date.and_then(|date| {
            glib::DateTime::from_local(
                i32::from(date.year()),
                i32::from(date.month()),
                i32::from(date.day()),
                0,
                0,
                0.0,
            )
            .ok()
        });
        if let Some(shown) = shown.or_else(|| glib::DateTime::now_local().ok()) {
            self.calendar.set_date(&shown);
        }
    }
}

/// Where a dragged step goes, relative to the step it's dropped on.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Place {
    After,
    Before,
    Inside,
}

/// Finds where a step dropped at (`x`, `y`) in `list` would go: the index of a row and the place
/// beside or inside it. Over a row's project target, the step goes inside it. Elsewhere over a
/// row, its halves decide the place. In a gap between rows, the step goes after the row above.
fn locate_drop(list: &gtk::ListBox, x: f64, y: f64) -> Option<(i32, Place)> {
    let mut previous = None;
    let mut index = 0;
    while let Some(row) = list.row_at_index(index) {
        if let Some(bounds) = row.compute_bounds(list) {
            let (top, height) = (f64::from(bounds.y()), f64::from(bounds.height()));
            if y < top {
                return Some(
                    previous.map_or((index, Place::Before), |above| (above, Place::After)),
                );
            }
            if y <= top + height {
                let over_target = project_target_end(list, &row).is_some_and(|end| x <= end);
                let place = if over_target {
                    Place::Inside
                } else if y - top < height / 2.0 {
                    Place::Before
                } else {
                    Place::After
                };
                return Some((index, place));
            }
        }
        previous = Some(index);
        index += 1;
    }
    previous.map(|last| (last, Place::After))
}

/// Text as wide as the stretch of a step's title that the target for dropping a step inside it
/// takes in, so the target grows with the text instead of the window.
const PROJECT_TARGET_REACH: &str = "word word";

/// Where, in `list`'s coordinates, the target for dropping a step inside `row` ends: a word or two
/// past the row's leading widget, but no further than halfway across the row. The target reaches
/// from there to the list's left edge, so it takes in the row's indentation. The rest of the row is
/// left for drops beside the step, so they don't make it a project by accident.
fn project_target_end(list: &gtk::ListBox, row: &gtk::ListBoxRow) -> Option<f64> {
    let content = row.child()?;
    let leading = content.first_child()?.compute_bounds(list)?;
    let reach = content.create_pango_layout(Some(PROJECT_TARGET_REACH)).pixel_size().0 as f32;
    let bounds = content.compute_bounds(list)?;
    let half = bounds.x() + bounds.width() / 2.0;
    Some(f64::from((leading.x() + leading.width() + reach).min(half)))
}

/// Where the line that shows a drop between rows starts, as (x, y) in the coordinates of `space`,
/// the widget that holds the line. The list's padding makes them differ from the list's own
/// coordinates. The line starts at the indentation of the step the drop goes beside, so a drop
/// after a project and a drop before its first step look different.
fn drop_line_start(
    list: &gtk::ListBox,
    space: &gtk::Widget,
    index: i32,
    place: Place,
) -> Option<(f32, f32)> {
    let bounds = |index: i32| list.row_at_index(index).and_then(|row| row.compute_bounds(space));
    let bottom = |rect: gtk::graphene::Rect| rect.y() + rect.height();
    let row = bounds(index)?;
    let y = match place {
        Place::After => {
            bounds(index + 1).map_or(bottom(row) + 1.0, |next| (bottom(row) + next.y()) / 2.0)
        }
        Place::Before => {
            let previous = if index > 0 { bounds(index - 1) } else { None };
            previous.map_or((row.y() - 1.0).max(1.0), |above| (bottom(above) + row.y()) / 2.0)
        }
        Place::Inside => return None,
    };
    let content = list.row_at_index(index)?.child()?.compute_bounds(space)?;
    Some((content.x(), y))
}

/// A step being dragged. A type of its own, unlike a string, keeps text widgets such as the editor
/// from taking the drop.
#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "CommonplaceDraggedStep")]
struct DraggedStep(NoteId);

/// Lets the user drag a step's row. The steps list decides where the step goes.
fn make_draggable(row: &gtk::Box, id: NoteId) {
    let source = gtk::DragSource::builder().actions(gdk::DragAction::MOVE).build();
    source.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&DraggedStep(id).to_value()))
    });
    source.connect_drag_begin(glib::clone!(
        #[weak]
        row,
        move |_, _| row.add_css_class("dragged")
    ));
    source.connect_drag_end(glib::clone!(
        #[weak]
        row,
        move |_, _, _| row.remove_css_class("dragged")
    ));
    row.add_controller(source);
}

impl ActionPanel {
    pub fn new(state: Rc<State>, picker: Rc<LinkPicker>) -> Rc<Self> {
        let done =
            gtk::Switch::builder().halign(gtk::Align::Start).valign(gtk::Align::Center).build();
        let list = gtk::DropDown::builder()
            .model(&gtk::StringList::new(&LISTS.map(|(name, _)| name)))
            .hexpand(true)
            .build();
        let project =
            gtk::Switch::builder().halign(gtk::Align::Start).valign(gtk::Align::Center).build();
        let due = DateButton::new();
        let defer = DateButton::new();
        let inherited =
            gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption", "dimmed"]).build();
        let waiting_for =
            gtk::Entry::builder().placeholder_text("Who or what").hexpand(true).build();
        let field_label = |text: &str| {
            gtk::Label::builder()
                .label(text)
                .use_underline(true)
                .xalign(0.0)
                .css_classes(["dimmed"])
                .build()
        };
        let labelled = |text: &str, widget: &gtk::Widget| {
            let label = field_label(text);
            label.set_mnemonic_widget(Some(widget));
            label
        };
        let waiting_label = labelled("Waiting for", waiting_for.upcast_ref());
        let fields = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        fields.attach(&labelled("Done?", done.upcast_ref()), 0, -1, 1, 1);
        fields.attach(&done, 1, -1, 1, 1);
        fields.attach(&labelled("List", list.upcast_ref()), 0, 0, 1, 1);
        fields.attach(&list, 1, 0, 1, 1);
        fields.attach(&waiting_label, 0, 1, 1, 1);
        fields.attach(&waiting_for, 1, 1, 1, 1);
        fields.attach(&labelled("Project?", project.upcast_ref()), 0, 2, 1, 1);
        fields.attach(&project, 1, 2, 1, 1);
        fields.attach(&labelled("Starts", defer.button.upcast_ref()), 0, 3, 1, 1);
        fields.attach(&defer.button, 1, 3, 1, 1);
        fields.attach(&labelled("Due", due.button.upcast_ref()), 0, 4, 1, 1);
        fields.attach(&due.button, 1, 4, 1, 1);
        fields.attach(&inherited, 0, 5, 2, 1);
        let planning = gtk::Switch::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .tooltip_text("Leave the next tasks until planned")
            .build();
        let planning_label = labelled("_Unclear?", planning.upcast_ref());
        fields.attach(&planning_label, 0, 6, 1, 1);
        fields.attach(&planning, 1, 6, 1, 1);
        let path = gtk::Label::builder().xalign(0.0).build();
        let move_button = gtk::Button::builder().label("Move…").halign(gtk::Align::Start).build();
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(12)
            .visible(false)
            .build();
        widget.append(&fields);

        let steps =
            gtk::ListBox::builder().css_classes(["navigation-sidebar", "step-list"]).build();
        let step_entry = gtk::Entry::builder()
            .placeholder_text("Add a step…")
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        let steps_section =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).visible(false).build();
        let crumbs = gtk::Box::builder()
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        crumbs.append(&field_label("In"));
        // A wrapping label would make the panel's width depend on its height, which GtkPaned
        // measures at heights below the panel's least, so GTK warns. Scrolling keeps every
        // ancestor's link reachable.
        crumbs.append(
            &gtk::ScrolledWindow::builder()
                .child(&path)
                .hexpand(true)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_width(true)
                .build(),
        );
        crumbs.append(&move_button);
        steps_section.append(&crumbs);
        let steps_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        steps_section.append(&steps_box);
        steps_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let show_done = gtk::Switch::builder()
            .tooltip_text("Show the steps that are done")
            .valign(gtk::Align::Center)
            .build();
        let show_done_label = labelled("Show done?", show_done.upcast_ref());
        let steps_header =
            gtk::Box::builder().spacing(6).margin_start(12).margin_end(12).margin_top(6).build();
        steps_header.append(
            &gtk::Label::builder()
                .label("Steps")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["heading"])
                .build(),
        );
        steps_header.append(&show_done_label);
        steps_header.append(&show_done);
        steps_box.append(&steps_header);
        let drop_line = gtk::Box::builder()
            .height_request(2)
            .valign(gtk::Align::Start)
            .margin_end(6)
            .css_classes(["drop-line"])
            .can_target(false)
            .visible(false)
            .build();
        let steps_overlay = gtk::Overlay::builder().child(&steps).build();
        steps_overlay.add_overlay(&drop_line);
        // Scrolling lets the steps give up height to the text, so a long list, or a bar above the
        // text, can't push the entry for new steps out of the window.
        let steps_scroll = gtk::ScrolledWindow::builder()
            .child(&steps_overlay)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(240)
            .vexpand(true)
            .build();
        steps_box.append(&steps_scroll);
        steps_box.append(&step_entry);

        let panel = Rc::new_cyclic(|panel: &Weak<Self>| Self {
            current: Cell::new(None),
            defer,
            done,
            drop_line,
            due,
            editable: Cell::new(false),
            inherited,
            loading: Cell::new(false),
            list,
            lists: RefCell::default(),
            move_button,
            on_open: RefCell::default(),
            path,
            picker,
            planning,
            planning_label,
            project,
            scroll_back: Cell::new(None),
            show_done,
            show_done_label,
            show_done_unsaved: RefCell::default(),
            shown_drop: Cell::new(None),
            state,
            step_entry,
            step_ids: RefCell::default(),
            step_parents: RefCell::default(),
            step_to_focus: Cell::new(None),
            step_to_reveal: Cell::new(None),
            steps,
            steps_box,
            steps_drag_scroll: DragScroll::attach(
                &steps_scroll,
                DraggedStep::static_type(),
                glib::clone!(
                    #[strong]
                    panel,
                    move |pointer| {
                        if let Some(panel) = panel.upgrade() {
                            panel.show_drop_after_scroll(pointer);
                        }
                    }
                ),
            ),
            steps_of: Cell::new(None),
            steps_scroll,
            steps_section,
            waiting_for,
            waiting_save: RefCell::default(),
            waiting_label,
            widget,
        });
        panel.connect_signals();
        panel
    }

    /// Saves the waiting-for text now if a save is waiting for a pause in typing.
    fn save_waiting_for(&self) {
        if let Some(source) = self.waiting_save.take() {
            source.remove();
            self.save_waiting_for_now();
        }
    }

    fn save_waiting_for_now(&self) {
        let text = self.waiting_for.text().trim().to_owned();
        let saved = self.current.get().and_then(|id| {
            self.state.read(|library| library.note(id)).and_then(|note| note.action)
        });
        if saved.is_some_and(|action| action.waiting_for.unwrap_or_default() != text) {
            self.change(ActionChange::WaitingFor(Some(text)));
        }
    }

    /// Calls `f` when the user asks to open another note from the panel.
    pub fn connect_open(&self, f: impl Fn(NoteId) + 'static) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    /// Shows the fields and steps of `note` if it's an action, and hides the panel otherwise. The
    /// panel follows the note, so it appears if the note becomes an action.
    pub fn show(self: &Rc<Self>, note: Option<&Note>, editable: bool) {
        self.save_waiting_for();
        self.current.set(note.map(|note| note.id));
        let in_inbox = note
            .and_then(|note| note.action.as_ref())
            .is_some_and(|action| action.list == ActionList::Inbox);
        let lists: Vec<(&str, ActionList)> =
            LISTS.into_iter().filter(|(_, list)| in_inbox || *list != ActionList::Inbox).collect();
        self.loading.set(true);
        self.list.set_model(Some(&gtk::StringList::new(
            &lists.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        )));
        self.loading.set(false);
        *self.lists.borrow_mut() = lists.into_iter().map(|(_, list)| list).collect();
        self.refresh(editable);
    }

    /// Shows the open action's fields and steps again, after the library changes, letting the
    /// user change the action or only look at it.
    pub fn refresh(self: &Rc<Self>, editable: bool) {
        self.editable.set(editable);
        self.show_current();
    }

    fn set_sensitive(&self, editable: bool) {
        self.widget.set_sensitive(editable);
        // The show done switch stays usable, since it only changes the view.
        for widget in [
            self.move_button.upcast_ref::<gtk::Widget>(),
            self.path.upcast_ref(),
            self.steps.upcast_ref(),
        ] {
            widget.set_sensitive(editable);
        }
        self.step_entry.set_visible(editable);
    }

    fn show_current(self: &Rc<Self>) {
        let editable = self.editable.get();
        self.set_sensitive(editable);
        let note = self.current.get().and_then(|id| self.state.read(|library| library.note(id)));
        let Some((note, action)) =
            note.and_then(|note| note.action.clone().map(|action| (note, action)))
        else {
            self.widget.set_visible(false);
            self.steps_section.set_visible(false);
            return;
        };
        self.loading.set(true);
        self.done.set_active(action.state == ActionState::Done);
        let list = self.lists.borrow().iter().position(|list| *list == action.list).unwrap_or(0);
        self.list.set_selected(list as u32);
        self.project.set_active(action.project);
        self.planning.set_active(action.planning.is_some());
        self.planning.set_visible(action.is_open());
        self.planning_label.set_visible(action.is_open());
        self.due.show(action.due);
        self.defer.show(action.defer);
        let dates = self.state.read(|library| library.action_dates(today()));
        let dates = dates.get(&note.id).copied().unwrap_or_default();
        let start = dates.start.filter(|start| start.date > today());
        let inherited: Vec<String> = [("Starts", start), ("Due", dates.due)]
            .into_iter()
            .filter_map(|(word, dated)| {
                let dated = dated.filter(|dated| dated.from != note.id)?;
                let project = parent_labels(&self.state, &[dated.from]);
                let project = project.first().map_or("", |(_, label)| label.as_str());
                Some(format!("{word} {} from {project}", format_date(dated.date)))
            })
            .collect();
        let inherited = inherited.join(" · ");
        self.inherited.set_label(&inherited);
        self.inherited.set_visible(!inherited.is_empty());
        self.inherited.set_tooltip_text((!inherited.is_empty()).then_some(inherited.as_str()));
        let waiting_for = action.waiting_for.clone().unwrap_or_default();
        // A leading space trims away when saved, so it mustn't count as a difference.
        let typing = self.waiting_save.borrow().is_some();
        if !typing && self.waiting_for.text().trim() != waiting_for {
            self.waiting_for.set_text(&waiting_for);
        }
        let waiting = action.list == ActionList::Waiting;
        self.waiting_label.set_visible(waiting);
        self.waiting_for.set_visible(waiting);
        self.show_path(note.id);
        let unsaved = self.show_done_unsaved.borrow().get(&note.id).copied();
        let show_done = unsaved.unwrap_or_else(|| {
            self.state.read(|library| library.note_setting(note.id, SHOW_DONE)) == Some(1)
        });
        self.show_done.set_active(show_done);
        let mut steps = self.state.read(|library| library.action_subtree(note.id, today()));
        self.steps_box.set_visible(editable || !steps.is_empty());
        self.show_done.set_visible(!steps.is_empty());
        self.show_done_label.set_visible(!steps.is_empty());
        if !show_done {
            // Steps under a hidden step go with it. Shown alone, they would look like steps of
            // the step above.
            let mut hidden_below = None;
            steps.retain(|step| {
                if hidden_below.is_some_and(|depth| step.depth > depth) {
                    return false;
                }
                let done = step.note.action.as_ref().is_some_and(|a| a.state == ActionState::Done);
                hidden_below = done.then_some(step.depth);
                !done
            });
        }
        let scroll = self.steps_scroll.vadjustment();
        if self.steps_of.replace(Some(note.id)) == Some(note.id) {
            // Rebuilding the list loses its scroll position. A refresh that comes before the last
            // one's rows are laid out mustn't replace the position with the one in between.
            if self.scroll_back.get().is_none() {
                self.scroll_back.set(Some(scroll.value()));
            }
            self.settle_scroll();
        } else {
            self.scroll_back.set(None);
            scroll.set_value(0.0);
        }
        self.steps.remove_all();
        self.shown_drop.set(None);
        self.drop_line.set_visible(false);
        for step in &steps {
            let row = note_row(&self.state, step, false);
            make_draggable(&row, step.note.id);
            self.steps.append(&row);
        }
        *self.step_ids.borrow_mut() = steps.iter().map(|step| step.note.id).collect();
        *self.step_parents.borrow_mut() =
            steps.iter().map(|step| step.parents.last().copied()).collect();
        self.steps_scroll.set_visible(!steps.is_empty());
        if let Some(moved) = self.step_to_focus.take()
            && let Some(index) = steps.iter().position(|step| step.note.id == moved)
            && let Some(row) = self.steps.row_at_index(index as i32)
        {
            // Focusing the row scrolls it into view, which restoring the old position would undo.
            self.scroll_back.set(None);
            row.grab_focus();
        }
        self.loading.set(false);
        self.widget.set_visible(true);
        self.steps_section.set_visible(true);
    }

    /// Shows the actions that `id` belongs to as a breadcrumb of buttons that open them.
    fn show_path(&self, id: NoteId) {
        let parents = self.state.read(|library| library.action_paths().remove(&id));
        let parents = parent_labels(&self.state, &parents.unwrap_or_default());
        let markup = if parents.is_empty() {
            "Top level".to_owned()
        } else {
            let links: Vec<String> = parents
                .iter()
                .map(|(parent, label)| {
                    format!("<a href=\"{parent}\">{}</a>", glib::markup_escape_text(label))
                })
                .collect();
            links.join(" › ")
        };
        self.path.set_markup(&markup);
    }

    fn change(&self, change: ActionChange) {
        if self.loading.get() {
            return;
        }
        let Some(id) = self.current.get() else {
            return;
        };
        if let Err(error) = self.state.write(|library| library.change_action(id, change)) {
            eprintln!("commonplace: {error}");
        }
    }

    fn open(&self, id: NoteId) {
        if let Some(on_open) = self.on_open.borrow().as_ref() {
            on_open(id);
        }
    }

    /// Closes one date popover when the other opens, and both when the user clicks elsewhere in
    /// the window.
    fn connect_date_popovers(self: &Rc<Self>) {
        let menus = [self.due.button.clone(), self.defer.button.clone()];
        for (index, menu) in menus.iter().enumerate() {
            let other = menus[1 - index].clone();
            menu.connect_active_notify(move |menu| {
                if menu.is_active() {
                    other.popdown();
                }
            });
        }
        let watching = Cell::new(false);
        self.widget.connect_map(move |widget| {
            let Some(window) = widget.root().and_downcast::<gtk::Window>() else { return };
            if watching.replace(true) {
                return;
            }
            let click = gtk::GestureClick::builder()
                .propagation_phase(gtk::PropagationPhase::Capture)
                .build();
            let menus = menus.clone();
            click.connect_pressed(glib::clone!(
                #[weak]
                window,
                move |_, _, x, y| {
                    let target = window.pick(x, y, gtk::PickFlags::DEFAULT);
                    for menu in &menus {
                        let on_menu = target.as_ref().is_some_and(|target| {
                            target == menu.upcast_ref::<gtk::Widget>() || target.is_ancestor(menu)
                        });
                        if menu.is_active() && !on_menu {
                            menu.popdown();
                        }
                    }
                }
            ));
            window.add_controller(click);
        });
    }

    fn connect_signals(self: &Rc<Self>) {
        self.path.connect_activate_link(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, uri| {
                let handler = panel.on_open.borrow().clone();
                if let (Ok(id), Some(handler)) = (uri.parse::<NoteId>(), handler) {
                    // Opening the parent rewrites this label, which GTK can't survive while it's
                    // still handling the click on one of its links.
                    glib::idle_add_local_once(move || handler(id));
                }
                glib::Propagation::Stop
            }
        ));
        self.connect_date_popovers();
        self.done.connect_active_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |done| {
                let state = if done.is_active() { ActionState::Done } else { ActionState::Open };
                panel.change(ActionChange::State(state));
            }
        ));
        self.list.connect_selected_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |list| {
                let chosen = panel.lists.borrow().get(list.selected() as usize).copied();
                if let Some(list) = chosen {
                    panel.change(ActionChange::List(list));
                }
            }
        ));
        self.project.connect_active_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |project| panel.change(ActionChange::Project(project.is_active()))
        ));
        self.planning.connect_active_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |planning| panel.change(ActionChange::Planning(planning.is_active()))
        ));
        for (button, change) in [
            (&self.due, ActionChange::Due as fn(Option<Date>) -> ActionChange),
            (&self.defer, ActionChange::Defer),
        ] {
            let menu = button.button.clone();
            connect_day_clicked(
                &menu,
                &button.calendar,
                glib::clone!(
                    #[weak(rename_to = panel)]
                    self,
                    move |day| panel.change(change(Some(day)))
                ),
            );
            button.clear.connect_clicked(glib::clone!(
                #[weak(rename_to = panel)]
                self,
                #[weak]
                menu,
                move |_| {
                    panel.change(change(None));
                    menu.popdown();
                }
            ));
        }
        self.waiting_for.connect_changed(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_| {
                if panel.loading.get() {
                    return;
                }
                if let Some(source) = panel.waiting_save.take() {
                    source.remove();
                }
                let source = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(600),
                    glib::clone!(
                        #[weak]
                        panel,
                        move || {
                            panel.waiting_save.take();
                            panel.save_waiting_for_now();
                        }
                    ),
                );
                *panel.waiting_save.borrow_mut() = Some(source);
            }
        ));
        self.waiting_for.connect_activate(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_| panel.save_waiting_for()
        ));
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_| panel.save_waiting_for()
        ));
        self.waiting_for.add_controller(focus);
        self.move_button.connect_clicked(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |button| panel.pick_parent(button)
        ));
        self.show_done.connect_active_notify(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |show_done| {
                if panel.loading.get() {
                    return;
                }
                let Some(id) = panel.current.get() else { return };
                if panel.state.read(|library| library.is_read_only()) {
                    panel.show_done_unsaved.borrow_mut().insert(id, show_done.is_active());
                    panel.show_current();
                    return;
                }
                let value = i64::from(show_done.is_active());
                let result =
                    panel.state.write(|library| library.set_note_setting(id, SHOW_DONE, value));
                if let Err(error) = result {
                    eprintln!("commonplace: {error}");
                }
            }
        ));
        self.connect_step_drops();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
                let up = match key {
                    gdk::Key::Up => true,
                    gdk::Key::Down => false,
                    _ => return glib::Propagation::Proceed,
                };
                if modifiers != gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK {
                    return glib::Propagation::Proceed;
                }
                let focused = panel.steps.focus_child().and_downcast::<gtk::ListBoxRow>();
                let index = focused.map(|row| row.index() as usize);
                let id = index.and_then(|index| panel.step_ids.borrow().get(index).copied());
                let sibling = index.and_then(|index| panel.shown_sibling(index, up));
                if let (Some(id), Some(sibling)) = (id, sibling) {
                    panel.step_to_focus.set(Some(id));
                    let result =
                        panel.state.write(|library| library.move_action_beside(id, sibling, !up));
                    if let Err(error) = result {
                        eprintln!("commonplace: {error}");
                    }
                }
                glib::Propagation::Stop
            }
        ));
        self.steps.add_controller(keys);
        self.steps.connect_row_activated(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_, row| {
                let id = panel.step_ids.borrow().get(row.index() as usize).copied();
                if let Some(id) = id {
                    panel.open(id);
                }
            }
        ));
        self.step_entry.connect_activate(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |entry| {
                let text = entry.text().trim().to_owned();
                let Some(parent) = panel.current.get() else { return };
                if text.is_empty() {
                    return;
                }
                match panel.state.write(|library| library.create_task(&text, parent)) {
                    Ok(id) => {
                        entry.set_text("");
                        panel.reveal_step(id);
                    }
                    Err(error) => eprintln!("commonplace: {error}"),
                }
            }
        ));
    }

    /// The step that a step moved up or down among the shown steps from `index` would go past: the
    /// nearest shown step above or below with the same parent. Hidden done steps don't count, so a
    /// move never looks like it did nothing.
    fn shown_sibling(&self, index: usize, up: bool) -> Option<NoteId> {
        let parents = self.step_parents.borrow();
        let parent = *parents.get(index)?;
        let found = if up {
            (0..index).rev().find(|&other| parents[other] == parent)
        } else {
            (index + 1..parents.len()).find(|&other| parents[other] == parent)
        };
        found.and_then(|other| self.step_ids.borrow().get(other).copied())
    }

    /// Scrolls the steps list until the step `id` is in view, once its row is laid out.
    fn reveal_step(self: &Rc<Self>, id: NoteId) {
        self.step_to_reveal.set(Some(id));
        self.settle_scroll();
    }

    /// Scrolls the steps list once its rows are laid out, to the step waiting to be revealed, or
    /// else back to where it was before it was rebuilt.
    fn settle_scroll(self: &Rc<Self>) {
        // GTK lays out the new rows before it runs an idle callback of default priority.
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move || {
                let scroll = panel.steps_scroll.vadjustment();
                let back = panel.scroll_back.take();
                let Some(id) = panel.step_to_reveal.take() else {
                    if let Some(back) = back {
                        scroll.set_value(back);
                    }
                    return;
                };
                let index = panel.step_ids.borrow().iter().position(|&step| step == id);
                let bounds = index
                    .and_then(|index| panel.steps.row_at_index(index as i32))
                    .and_then(|row| row.compute_bounds(&panel.steps));
                let Some(bounds) = bounds else { return };
                let (top, bottom) =
                    (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
                let value = back.unwrap_or(scroll.value());
                if bottom > value + scroll.page_size() {
                    scroll.set_value(bottom - scroll.page_size());
                } else if top < value {
                    scroll.set_value(top);
                } else {
                    scroll.set_value(value);
                }
            }
        ));
    }

    /// Shows where a dragged step would go: a line between two steps, or a box around the step it
    /// would go inside. `None` hides both.
    fn show_drop(&self, target: Option<(i32, Place)>) {
        if self.shown_drop.replace(target) == target {
            return;
        }
        let space = self.drop_line.parent();
        let line = target
            .zip(space)
            .and_then(|((index, place), space)| drop_line_start(&self.steps, &space, index, place));
        if let Some((x, y)) = line {
            self.drop_line.set_margin_start(x.round().max(0.0) as i32);
            self.drop_line.set_margin_top((y - 1.0).round().max(0.0) as i32);
        }
        self.drop_line.set_visible(line.is_some());
        let mut index = 0;
        while let Some(row) = self.steps.row_at_index(index) {
            if let Some(child) = row.child() {
                let inside = target == Some((index, Place::Inside));
                if inside {
                    child.add_css_class("drop-inside")
                } else {
                    child.remove_css_class("drop-inside")
                }
            }
            index += 1;
        }
    }

    /// Lets the user drop a dragged step onto the steps list, before, after, or inside a step.
    fn connect_step_drops(self: &Rc<Self>) {
        let target = gtk::DropTarget::new(DraggedStep::static_type(), gdk::DragAction::MOVE);
        target.connect_motion(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            gdk::DragAction::empty(),
            move |_, x, y| {
                panel.show_drop(locate_drop(&panel.steps, x, y));
                gdk::DragAction::MOVE
            }
        ));
        target.connect_leave(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            move |_| panel.show_drop(None)
        ));
        target.connect_drop(glib::clone!(
            #[weak(rename_to = panel)]
            self,
            #[upgrade_or]
            false,
            move |_, value, x, y| {
                panel.steps_drag_scroll.stop();
                panel.show_drop(None);
                let dragged = value.get::<DraggedStep>().ok().map(|step| step.0);
                let target = locate_drop(&panel.steps, x, y).and_then(|(index, place)| {
                    let id = panel.step_ids.borrow().get(index as usize).copied()?;
                    Some((id, place))
                });
                let (Some(dragged), Some((id, place))) = (dragged, target) else {
                    return false;
                };
                if dragged == id {
                    return false;
                }
                // Moving rebuilds the steps list, which would destroy the dragged row while the
                // drag is still finishing.
                glib::idle_add_local_once(glib::clone!(
                    #[strong(rename_to = state)]
                    panel.state,
                    move || {
                        let result = state.write(|library| match place {
                            Place::After => library.move_action_beside(dragged, id, true),
                            Place::Before => library.move_action_beside(dragged, id, false),
                            Place::Inside => library.move_action(dragged, Some(id), None),
                        });
                        if let Err(error) = result {
                            eprintln!("commonplace: {error}");
                        }
                    }
                ));
                true
            }
        ));
        self.steps.add_controller(target);
    }

    /// Moves the drop indicator with the steps as they scroll under a pointer at `pointer` in the
    /// coordinates of their view.
    fn show_drop_after_scroll(&self, pointer: Option<gtk::graphene::Point>) {
        let point = pointer.and_then(|point| self.steps_scroll.compute_point(&self.steps, &point));
        if let Some(point) = point {
            let (x, y) = (f64::from(point.x()), f64::from(point.y()));
            self.show_drop(locate_drop(&self.steps, x, y));
        }
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
                #[strong(rename_to = state)]
                self.state,
                #[weak(rename_to = widget)]
                self.widget,
                move |choice| {
                    let parent = match choice {
                        Choice::Note(parent) => Some(parent),
                        Choice::TopLevel => None,
                        Choice::CreateAction(_) | Choice::CreateMemo(_) => return,
                    };
                    libraries::move_action(&state, &widget, id, parent, |result| {
                        if let Err(error) = result {
                            eprintln!("commonplace: {error}");
                        }
                    });
                }
            ),
        );
    }
}
