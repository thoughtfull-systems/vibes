// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib, graphene};

/// How far past a thin handle GtkPaned starts a drag, for a pointer and for a touch, matching
/// `HANDLE_EXTRA_SIZE` and `TOUCH_EXTRA_AREA_WIDTH` in its source.
const POINTER_REACH: f32 = 6.0;
const TOUCH_REACH: f32 = POINTER_REACH + 50.0;

type ResizedHandler = Rc<dyn Fn(i32)>;

/// Divides a note's text from the panel below it. The panel fits what it shows, unless the user
/// has moved the handle to give the open note a height of its own.
pub struct BottomPane {
    /// Set while a pointer or touch that went down on the handle stays down.
    dragging: Cell<bool>,
    /// Where the handle was, and the height the user had given the panel, when the handle took
    /// keyboard focus, while it has it.
    focus_start: Cell<Option<(i32, Option<i32>)>>,
    handle: gtk::Widget,
    /// The height the user gave the open note's panel, if they have.
    height: Cell<Option<i32>>,
    on_resized: RefCell<Option<ResizedHandler>>,
    panel: gtk::Box,
    pub paned: gtk::Paned,
    /// Set while the pane moves the handle itself, so the move isn't taken for the user's.
    placing: Cell<bool>,
    /// Set once the user moves the handle, until they let go of it.
    resized: Cell<bool>,
}

impl BottomPane {
    pub fn new(text: &impl IsA<gtk::Widget>, panel: &gtk::Box) -> Rc<Self> {
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(text)
            .end_child(panel)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .vexpand(true)
            .build();
        // GtkPaned keeps its handle as an internal child between the two it shows.
        let handle = std::iter::successors(paned.first_child(), |child| child.next_sibling())
            .find(|child| {
                child != text.upcast_ref::<gtk::Widget>()
                    && child != panel.upcast_ref::<gtk::Widget>()
            })
            .expect("GtkPaned has a handle");
        let this = Rc::new(Self {
            dragging: Cell::new(false),
            focus_start: Cell::new(None),
            handle,
            height: Cell::new(None),
            on_resized: RefCell::default(),
            panel: panel.clone(),
            paned,
            placing: Cell::new(false),
            resized: Cell::new(false),
        });
        this.paned.connect_max_position_notify(glib::clone!(
            #[weak]
            this,
            move |_| this.update()
        ));
        this.paned.connect_position_notify(glib::clone!(
            #[weak]
            this,
            move |_| this.moved()
        ));
        this.paned.connect_has_focus_notify(glib::clone!(
            #[weak]
            this,
            move |paned| this.focus_changed(paned.has_focus())
        ));
        // GtkPaned doesn't say when the user drags its handle, and it also moves the handle
        // itself as the window resizes.
        let pointer = gtk::EventControllerLegacy::new();
        pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
        pointer.connect_event(glib::clone!(
            #[weak]
            this,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                match event.event_type() {
                    gdk::EventType::ButtonPress => this.dragging.set(this.on_handle(event, false)),
                    gdk::EventType::TouchBegin => this.dragging.set(this.on_handle(event, true)),
                    gdk::EventType::ButtonRelease
                    | gdk::EventType::TouchEnd
                    | gdk::EventType::TouchCancel => this.released(),
                    _ => {}
                }
                glib::Propagation::Proceed
            }
        ));
        this.paned.add_controller(pointer);
        this
    }

    /// Calls `f` with the height the user moves the panel to, once they let go of the handle.
    pub fn connect_resized(&self, f: impl Fn(i32) + 'static) {
        *self.on_resized.borrow_mut() = Some(Rc::new(f));
    }

    /// Gives the panel the height the user chose for the note that now shows, or fits the panel
    /// to what it shows if they haven't chosen one.
    pub fn show(&self, height: Option<i32>) {
        self.height.set(height);
        self.update();
    }

    /// Places the handle again, after what the panel shows changes.
    pub fn update(&self) {
        if !self.ready() || self.dragging.get() || self.focus_start.get().is_some() {
            return;
        }
        let height = self.height.get().unwrap_or_else(|| {
            self.panel.measure(gtk::Orientation::Vertical, self.paned.width()).1
        });
        // GtkPaned measures from the start, and its maximum leaves the panel its least height.
        let position = self.paned.max_position() + self.min() - height;
        if self.paned.is_position_set() && self.paned.position() == position {
            return;
        }
        self.placing.set(true);
        self.paned.set_position(position);
        self.placing.set(false);
    }

    fn moved(&self) {
        let by_user = self.dragging.get() || self.focus_start.get().is_some();
        if by_user && !self.placing.get() && self.ready() {
            self.height.set(Some(self.paned.max_position() + self.min() - self.paned.position()));
            self.resized.set(true);
        }
    }

    fn released(&self) {
        self.dragging.set(false);
        self.finish_resize();
    }

    /// GtkPaned moves its handle with the arrow keys while the pane itself has focus, which F8
    /// gives it, and Escape puts the handle back before focus returns to the text.
    fn focus_changed(&self, focused: bool) {
        if focused {
            self.focus_start.set(Some((self.paned.position(), self.height.get())));
            return;
        }
        let Some((position, height)) = self.focus_start.take() else { return };
        if self.paned.position() == position {
            self.height.set(height);
            self.resized.set(false);
        } else {
            self.finish_resize();
        }
    }

    fn finish_resize(&self) {
        let handler = self.on_resized.borrow().clone();
        if self.resized.take()
            && let Some(height) = self.height.get()
            && let Some(handler) = handler
        {
            handler(height);
        }
    }

    /// Whether `event` went down where GtkPaned starts a drag of the handle.
    fn on_handle(&self, event: &gdk::Event, touch: bool) -> bool {
        let (Some((x, y)), Some(native)) = (event.position(), self.paned.native()) else {
            return false;
        };
        let (dx, dy) = native.surface_transform();
        let point = graphene::Point::new((x - dx) as f32, (y - dy) as f32);
        let (Some(point), Some(bounds)) =
            (native.compute_point(&self.paned, &point), self.handle.compute_bounds(&self.paned))
        else {
            return false;
        };
        let reach = if touch { TOUCH_REACH } else { POINTER_REACH };
        (bounds.y() - reach..=bounds.y() + bounds.height() + reach).contains(&point.y())
    }

    /// Whether the panel shows and the pane knows its own height. Until the pane's first
    /// allocation, its maximum is a placeholder.
    fn ready(&self) -> bool {
        self.panel.get_visible() && self.paned.max_position() < i32::MAX
    }

    fn min(&self) -> i32 {
        self.panel.measure(gtk::Orientation::Vertical, self.paned.width()).0
    }
}
