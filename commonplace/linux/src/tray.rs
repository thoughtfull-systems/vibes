// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! An icon in the system tray, through the [StatusNotifierItem] and [DBusMenu] interfaces that the
//! trays of KDE, waybar, and GNOME's AppIndicator extension host.
//!
//! [StatusNotifierItem]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/
//! [DBusMenu]: https://git.launchpad.net/libdbusmenu/tree/libdbusmenu-glib/dbus-menu.xml

use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

/// What the user asked for through the icon.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    /// A click on the icon.
    Activate,
    CaptureAction,
    CaptureMemo,
    Quit,
}

/// The menu's items, in the order it shows them, by their ID in the menu's layout. ID 0 is the
/// menu itself.
const ITEMS: [(i32, &str, Event); 3] = [
    (1, "Quick Capture _Action", Event::CaptureAction),
    (2, "Quick Capture _Memo", Event::CaptureMemo),
    (3, "_Quit", Event::Quit),
];

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";

const INTERFACES: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="Category" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="IconThemePath" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="WindowId" type="i" access="read"/>
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
    <method name="SecondaryActivate">
      <arg type="i" direction="in"/><arg type="i" direction="in"/>
    </method>
    <signal name="NewAttentionIcon"/>
    <signal name="NewIcon"/>
    <signal name="NewOverlayIcon"/>
    <signal name="NewStatus"><arg type="s"/></signal>
    <signal name="NewTitle"/>
    <signal name="NewToolTip"/>
  </interface>
  <interface name="com.canonical.dbusmenu">
    <property name="IconThemePath" type="as" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Version" type="u" access="read"/>
    <method name="AboutToShow">
      <arg type="i" direction="in"/><arg type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg type="ai" direction="in"/><arg type="ai" direction="out"/>
      <arg type="ai" direction="out"/>
    </method>
    <method name="Event">
      <arg type="i" direction="in"/><arg type="s" direction="in"/>
      <arg type="v" direction="in"/><arg type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg type="a(isvu)" direction="in"/><arg type="ai" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg type="ai" direction="in"/><arg type="as" direction="in"/>
      <arg type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetLayout">
      <arg type="i" direction="in"/><arg type="i" direction="in"/>
      <arg type="as" direction="in"/><arg type="u" direction="out"/>
      <arg type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg type="i" direction="in"/><arg type="s" direction="in"/>
      <arg type="v" direction="out"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg type="a(ia{sv})"/><arg type="a(ias)"/>
    </signal>
    <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
  </interface>
</node>
"#;

/// Puts the icon in the tray whenever a tray is running, and calls `shown` each time a tray takes
/// it. `event` receives what the user asks for through it.
pub fn start(
    connection: &gio::DBusConnection,
    shown: impl Fn() + 'static,
    event: impl Fn(Event) + 'static,
) -> Result<(), glib::Error> {
    let node = gio::DBusNodeInfo::for_xml(INTERFACES)?;
    let item = node.lookup_interface("org.kde.StatusNotifierItem").expect("the XML declares it");
    let menu = node.lookup_interface("com.canonical.dbusmenu").expect("the XML declares it");
    let event = Rc::new(event);
    let icon_theme_path = icon_theme_path();
    let id = crate::app_id();
    connection
        .register_object(ITEM_PATH, &item)
        .method_call(glib::clone!(
            #[strong]
            event,
            move |_, _, _, _, method, _, invocation| {
                if method == "Activate" {
                    event(Event::Activate);
                }
                invocation.return_value(None);
            }
        ))
        .property(move |_, _, _, _, property| item_property(property, &icon_theme_path, &id))
        .build()?;
    connection
        .register_object(MENU_PATH, &menu)
        .method_call(move |_, _, _, _, method, parameters, invocation| {
            invocation.return_value(menu_call(method, &parameters, &*event).as_ref());
        })
        .property(|_, _, _, _, property| menu_property(property))
        .build()?;
    // The specification names the item by a well-known name, which some trays insist on.
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    gio::bus_own_name_on_connection(
        connection,
        &name,
        gio::BusNameOwnerFlags::NONE,
        |_, _| {},
        |_, _| {},
    );
    let shown = Rc::new(shown);
    // A tray that starts after Commonplace, or restarts, needs the item registered again.
    gio::bus_watch_name_on_connection(
        connection,
        WATCHER,
        gio::BusNameWatcherFlags::NONE,
        move |connection, _, _| {
            let shown = shown.clone();
            connection.call(
                Some(WATCHER),
                "/StatusNotifierWatcher",
                WATCHER,
                "RegisterStatusNotifierItem",
                Some(&(name.as_str(),).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |result| {
                    if result.is_ok() {
                        shown();
                    }
                },
            );
        },
        |_, _| {},
    );
    Ok(())
}

fn item_property(property: &str, icon_theme_path: &str, id: &str) -> glib::Variant {
    let no_pixmaps: Vec<(i32, i32, Vec<u8>)> = Vec::new();
    match property {
        "Category" => "ApplicationStatus".to_variant(),
        "IconName" => crate::APP_ID.to_variant(),
        "IconThemePath" => icon_theme_path.to_variant(),
        "Id" => id.to_variant(),
        "ItemIsMenu" => false.to_variant(),
        "Menu" => {
            glib::variant::ObjectPath::try_from(MENU_PATH).expect("the path is valid").to_variant()
        }
        "Status" => "Active".to_variant(),
        "Title" => "Commonplace".to_variant(),
        "ToolTip" => ("", no_pixmaps, "Commonplace", "").to_variant(),
        "WindowId" => 0i32.to_variant(),
        name if name.ends_with("Pixmap") => no_pixmaps.to_variant(),
        _ => "".to_variant(),
    }
}

fn menu_property(property: &str) -> glib::Variant {
    match property {
        "IconThemePath" => Vec::<String>::new().to_variant(),
        "Status" => "normal".to_variant(),
        "TextDirection" => "ltr".to_variant(),
        _ => 3u32.to_variant(),
    }
}

/// Answers a call to the menu's interface, or returns `None` for a call with no reply.
fn menu_call(
    method: &str,
    parameters: &glib::Variant,
    event: &dyn Fn(Event),
) -> Option<glib::Variant> {
    let click = |id: i32, kind: &str| {
        if kind == "clicked"
            && let Some((_, _, item)) = ITEMS.iter().find(|(item, _, _)| *item == id)
        {
            event(*item);
        }
    };
    let no_ids: Vec<i32> = Vec::new();
    match method {
        "AboutToShow" => Some((false,).to_variant()),
        "AboutToShowGroup" => Some((no_ids.clone(), no_ids).to_variant()),
        "Event" => {
            if let Some((id, kind, _, _)) = parameters.get::<(i32, String, glib::Variant, u32)>() {
                click(id, &kind);
            }
            None
        }
        "EventGroup" => {
            let events = parameters
                .get::<(Vec<(i32, String, glib::Variant, u32)>,)>()
                .map(|(events,)| events)
                .unwrap_or_default();
            for (id, kind, _, _) in events {
                click(id, &kind);
            }
            Some((no_ids,).to_variant())
        }
        "GetGroupProperties" => {
            let ids = parameters.get::<(Vec<i32>, Vec<String>)>().map(|(ids, _)| ids);
            let properties: Vec<(i32, HashMap<String, glib::Variant>)> = ITEMS
                .iter()
                .filter(|(id, _, _)| {
                    ids.as_ref().is_none_or(|ids| ids.is_empty() || ids.contains(id))
                })
                .map(|&(id, label, _)| (id, item_properties(label)))
                .collect();
            Some((properties,).to_variant())
        }
        "GetLayout" => {
            let children: Vec<glib::Variant> = ITEMS
                .iter()
                .map(|&(id, label, _)| {
                    (id, item_properties(label), Vec::<glib::Variant>::new()).to_variant()
                })
                .collect();
            let root = HashMap::from([("children-display".to_owned(), "submenu".to_variant())]);
            Some((1u32, (0i32, root, children)).to_variant())
        }
        "GetProperty" => {
            let label = parameters.get::<(i32, String)>().and_then(|(id, property)| {
                let (_, label, _) = ITEMS.iter().find(|(item, _, _)| *item == id)?;
                (property == "label").then_some(*label)
            });
            Some((label.unwrap_or_default().to_variant(),).to_variant())
        }
        _ => None,
    }
}

fn item_properties(label: &str) -> HashMap<String, glib::Variant> {
    HashMap::from([("label".to_owned(), label.to_variant())])
}

/// The folder that holds the app's icon, which the tray needs when the icon isn't installed where
/// the tray's own icon theme looks, such as in a debug build or a Nix package.
///
/// Waybar replaces its search path with this folder and looks for the hicolor theme there, which
/// it can't find without hicolor's index.theme. Neither the source tree nor the package holds one,
/// so the path names the icon's own folder, where GTK finds icons that belong to no theme. That
/// folder holds the small icon, drawn for the size a tray shows.
fn icon_theme_path() -> String {
    const APPS: &str = "hicolor/16x16/apps";
    if cfg!(debug_assertions) {
        return format!("{}/{APPS}", crate::SOURCE_ICONS);
    }
    let icon = format!("{}.svg", crate::APP_ID);
    std::iter::once(glib::user_data_dir())
        .chain(glib::system_data_dirs())
        .map(|dir| dir.join("icons").join(APPS))
        .find(|dir| dir.join(&icon).is_file())
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default()
}
