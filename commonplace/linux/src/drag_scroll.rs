// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib, graphene};

/// How near the top or bottom of a view, in pixels, a drag starts to scroll it.
const EDGE: f64 = 32.0;
/// How fast, in pixels a second, a drag at or past an edge of a view scrolls it.
const SPEED: f64 = 600.0;

type ScrollHandler = Box<dyn Fn(Option<graphene::Point>)>;

/// Scrolls a view while something is dragged near or past its top or bottom, which GTK doesn't do
/// on its own.
pub struct DragScroll {
    content: glib::Type,
    /// The drag being followed. GDK keeps it until the drag ends, however the widget it started
    /// from fares.
    drag: glib::WeakRef<gdk::Drag>,
    on_scroll: ScrollHandler,
    /// Where the drag was last seen in the window, in the view's coordinates. The window can't see
    /// the pointer once it leaves, so this stays put, which keeps the view scrolling toward
    /// wherever the pointer left.
    pointer: Cell<Option<(f64, f64)>>,
    scroll: glib::WeakRef<gtk::ScrolledWindow>,
    ticking: Cell<bool>,
}

/// How fast, in pixels a second, to scroll a view `height` tall while something is dragged at
/// height `y` in it, which can lie outside the view. Negative speeds scroll up. The speed grows as
/// the drag nears an edge, up to its most at the edge and past it.
fn edge_speed(y: f64, height: f64) -> f64 {
    let edge = EDGE.min(height / 4.0);
    let depth = if y < edge {
        y - edge
    } else if y > height - edge {
        y - (height - edge)
    } else {
        return 0.0;
    };
    SPEED * (depth / edge).clamp(-1.0, 1.0)
}

impl DragScroll {
    /// Scrolls `scroll` while content of type `content` is dragged near or past the view's top or
    /// bottom, within the view's horizontal span. The content moves under a pointer that holds
    /// still, so `on_scroll` gets the pointer after each step, in the view's coordinates, or `None`
    /// when it's outside the view.
    pub fn attach(
        scroll: &gtk::ScrolledWindow,
        content: glib::Type,
        on_scroll: impl Fn(Option<graphene::Point>) + 'static,
    ) -> Rc<Self> {
        let this = Rc::new(Self {
            content,
            drag: glib::WeakRef::new(),
            on_scroll: Box::new(on_scroll),
            pointer: Cell::new(None),
            scroll: scroll.downgrade(),
            ticking: Cell::new(false),
        });
        let watching = Cell::new(false);
        scroll.connect_map(glib::clone!(
            #[strong]
            this,
            move |scroll| {
                let Some(window) = scroll.root().and_downcast::<gtk::Window>() else { return };
                if watching.replace(true) {
                    return;
                }
                // Drop targets in the view stop the drag's motion from bubbling up to the window.
                let motion = gtk::DropControllerMotion::builder()
                    .propagation_phase(gtk::PropagationPhase::Capture)
                    .build();
                motion.connect_motion(glib::clone!(
                    #[weak]
                    this,
                    #[weak]
                    window,
                    move |motion, x, y| this.track(&window, motion.drop(), x, y)
                ));
                motion.connect_leave(glib::clone!(
                    #[weak]
                    this,
                    move |_| {
                        // Only a followed drag says when it ends, so only its pointer can stay
                        // outside the window.
                        if this.drag.upgrade().is_none() {
                            this.pointer.set(None);
                        }
                    }
                ));
                window.add_controller(motion);
            }
        ));
        this
    }

    /// Stops scrolling for the drag in progress, such as when it drops. The drag itself says it
    /// ended only after the drop has finished.
    pub fn stop(&self) {
        self.drag.set(None);
        self.pointer.set(None);
    }

    /// Follows a drag at (`x`, `y`) in `window`, if it carries the content this view scrolls for.
    /// A drag from this app says when it ends, so the view can keep scrolling after the pointer
    /// leaves the window.
    fn track(self: &Rc<Self>, window: &gtk::Window, drop: Option<gdk::Drop>, x: f64, y: f64) {
        let Some(drop) = drop.filter(|drop| drop.formats().contains_type(self.content)) else {
            self.pointer.set(None);
            return;
        };
        if let Some(drag) = drop.drag()
            && self.drag.upgrade().as_ref() != Some(&drag)
        {
            self.drag.set(Some(&drag));
            let stop = Rc::new(glib::clone!(
                #[weak(rename_to = this)]
                self,
                move || this.stop()
            ));
            drag.connect_cancel(glib::clone!(
                #[strong]
                stop,
                move |_, _| stop()
            ));
            drag.connect_drop_performed(glib::clone!(
                #[strong]
                stop,
                move |_| stop()
            ));
            drag.connect_dnd_finished(move |_| stop());
        }
        let Some(scroll) = self.scroll.upgrade().filter(|scroll| scroll.is_mapped()) else {
            return;
        };
        let point = graphene::Point::new(x as f32, y as f32);
        let point = window.compute_point(&scroll, &point);
        self.pointer.set(point.map(|point| (f64::from(point.x()), f64::from(point.y()))));
        self.start(&scroll);
    }

    /// How fast to scroll `scroll` now, in pixels a second: 0 when the drag is away from the
    /// edges, or when the view can't scroll any further its way.
    fn speed(&self, scroll: &gtk::ScrolledWindow) -> f64 {
        let Some((x, y)) = self.pointer.get() else { return 0.0 };
        if !(0.0..=f64::from(scroll.width())).contains(&x) {
            return 0.0;
        }
        let speed = edge_speed(y, f64::from(scroll.height()));
        let adjustment = scroll.vadjustment();
        let at_end = if speed < 0.0 {
            adjustment.value() <= adjustment.lower()
        } else {
            adjustment.value() >= adjustment.upper() - adjustment.page_size()
        };
        if at_end { 0.0 } else { speed }
    }

    /// Scrolls the view each frame until it has no reason to. No motion arrives while the pointer
    /// holds still, so a tick callback does the scrolling.
    fn start(self: &Rc<Self>, scroll: &gtk::ScrolledWindow) {
        if self.ticking.get() || self.speed(scroll) == 0.0 {
            return;
        }
        self.ticking.set(true);
        let last_frame = Cell::new(None);
        scroll.add_tick_callback(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move |scroll, clock| {
                let speed = this.speed(scroll);
                if speed == 0.0 {
                    this.ticking.set(false);
                    return glib::ControlFlow::Break;
                }
                let now = clock.frame_time();
                if let Some(before) = last_frame.replace(Some(now)) {
                    let adjustment = scroll.vadjustment();
                    let seconds = (now - before) as f64 / 1_000_000.0;
                    adjustment.set_value(adjustment.value() + speed * seconds);
                    let pointer = this
                        .pointer
                        .get()
                        .filter(|&(x, y)| scroll.contains(x, y))
                        .map(|(x, y)| graphene::Point::new(x as f32, y as f32));
                    (this.on_scroll)(pointer);
                }
                glib::ControlFlow::Continue
            }
        ));
    }
}
