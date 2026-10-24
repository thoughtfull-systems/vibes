// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::cell::RefCell;
use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{
    Link, ListKey, Span, Style, list_edit, parse_links, spans, toggle_checkbox,
};
use gtk::{gdk, glib, pango};

use crate::highlight::Highlighter;
use sourceview5::prelude::*;

const BROKEN: &str = "broken-link";
const CODE_BLOCK_SHADE: &str = "markup-code-block-shade";
/// The colors of headings' text, kept apart from the heading tags so a heading's markers keep the
/// editor's text color.
const HEADING_COLORS: [&str; 4] = [
    "markup-heading-color-1",
    "markup-heading-color-2",
    "markup-heading-color-3",
    "markup-heading-color-4",
];
const HIDDEN: &str = "markup-hidden";
const LINK: &str = "link";
const QUOTE_MARGIN: &str = "markup-quote-margin";
/// How far a block quote sits inside the editor's left margin.
const QUOTE_INDENT: i32 = 16;
const SYNTAX: &str = "markup-syntax";
const TARGET: &str = "link-target";

/// Creates a Markdown editor that renders styles as you write, and shows the Markdown syntax of
/// whatever the cursor touches. Its style follows the system's light or dark preference.
pub fn new_view() -> sourceview5::View {
    let buffer = sourceview5::Buffer::new(None);
    let style_manager = adw::StyleManager::default();
    let apply_scheme = {
        let buffer = buffer.clone();
        move |style_manager: &adw::StyleManager| {
            let dark = style_manager.is_dark();
            let name = if dark { "Adwaita-dark" } else { "Adwaita" };
            buffer
                .set_style_scheme(sourceview5::StyleSchemeManager::default().scheme(name).as_ref());
            color_headings(&buffer.tag_table(), dark);
        }
    };

    let table = buffer.tag_table();
    table.add(&gtk::TextTag::builder().name(LINK).underline(pango::Underline::Single).build());
    table.add(
        &gtk::TextTag::builder()
            .name(TARGET)
            .scale(0.75)
            .foreground_rgba(&gdk::RGBA::new(0.5, 0.5, 0.5, 0.8))
            .build(),
    );
    table.add(&gtk::TextTag::builder().name(BROKEN).underline(pango::Underline::Error).build());
    add_preview_tags(&table);
    apply_scheme(&style_manager);
    style_manager.connect_dark_notify(apply_scheme);
    attach_preview(&buffer);

    let view = sourceview5::View::builder()
        .buffer(&buffer)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(12)
        .bottom_margin(24)
        .left_margin(16)
        .right_margin(16)
        .vexpand(true)
        .build();
    view.add_css_class("note-editor");
    // A tag's margin replaces the view's margin rather than adding to it, so the quote margin
    // follows the view's.
    let indent_quotes = |view: &sourceview5::View| {
        if let Some(tag) = view.buffer().tag_table().lookup(QUOTE_MARGIN) {
            tag.set_left_margin(view.left_margin() + QUOTE_INDENT);
        }
    };
    indent_quotes(&view);
    view.connect_left_margin_notify(indent_quotes);
    attach_list_keys(&view);
    attach_wrap_keys(&view);
    view
}

/// Wraps the selection in Markdown delimiters when the user types one, rather than replacing it,
/// and keeps the text inside selected, so typing the delimiter again doubles it.
fn attach_wrap_keys(view: &sourceview5::View) {
    // The capture phase sees the key before the view replaces the selection with it.
    let keys = gtk::EventControllerKey::builder()
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
            if !(modifiers - gdk::ModifierType::SHIFT_MASK).is_empty() || !view.is_editable() {
                return glib::Propagation::Proceed;
            }
            let (open, close) = match key.to_unicode() {
                Some(delimiter @ ('*' | '_' | '`' | '~' | '=')) => (delimiter, delimiter),
                Some('[') => ('[', ']'),
                _ => return glib::Propagation::Proceed,
            };
            let buffer = view.buffer();
            let Some((mut start, mut end)) = buffer.selection_bounds() else {
                return glib::Propagation::Proceed;
            };
            // Line breaks at either end of the selection stay outside the delimiters, which can't
            // pair across lines.
            while start < end && start.char() == '\n' {
                start.forward_char();
            }
            while start < end {
                let mut last = end;
                last.backward_char();
                if last.char() != '\n' {
                    break;
                }
                end = last;
            }
            if start == end {
                return glib::Propagation::Proceed;
            }
            let (start_offset, end_offset) = (start.offset(), end.offset());
            let selected = buffer.text(&start, &end, true);
            // One insertion, rather than one per delimiter, so handlers that react to a typed
            // character, such as the link picker's `[[`, don't fire.
            buffer.begin_user_action();
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, &format!("{open}{selected}{close}"));
            buffer.select_range(
                &buffer.iter_at_offset(start_offset + 1),
                &buffer.iter_at_offset(end_offset + 1),
            );
            buffer.end_user_action();
            glib::Propagation::Stop
        }
    ));
    view.add_controller(keys);
}

/// Gives Enter, Shift+Enter, Tab, and Shift+Tab their outliner meanings inside list items.
fn attach_list_keys(view: &sourceview5::View) {
    // The capture phase sees the keys before the view inserts a line break or a tab.
    let keys = gtk::EventControllerKey::builder()
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, modifiers| {
            let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
            let shift = match modifiers {
                gdk::ModifierType::SHIFT_MASK => true,
                modifiers if modifiers.is_empty() => false,
                _ => return glib::Propagation::Proceed,
            };
            let list_key = match key {
                gdk::Key::Return | gdk::Key::KP_Enter if shift => ListKey::ShiftEnter,
                gdk::Key::Return | gdk::Key::KP_Enter => ListKey::Enter,
                gdk::Key::ISO_Left_Tab | gdk::Key::Tab if shift => ListKey::ShiftTab,
                gdk::Key::Tab => ListKey::Tab,
                _ => return glib::Propagation::Proceed,
            };
            let buffer = view.buffer();
            if view.is_editable() && !buffer.has_selection() && apply_list_edit(&buffer, list_key) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    view.add_controller(keys);
}

/// Makes the edit that `key` makes at the cursor, as one undoable step. Returns whether the
/// cursor was in a list item.
fn apply_list_edit(buffer: &gtk::TextBuffer, key: ListKey) -> bool {
    let body = text(buffer);
    let cursor = usize::try_from(buffer.cursor_position()).unwrap_or(0);
    let cursor = body.char_indices().nth(cursor).map_or(body.len(), |(byte, _)| byte);
    let Some(edit) = list_edit(&body, cursor, key) else {
        return false;
    };
    if edit.range.is_empty() && edit.text.is_empty() {
        return true;
    }
    let range = char_range(&body, edit.range.clone());
    let mut changed = body;
    changed.replace_range(edit.range, &edit.text);
    let new_cursor = changed[..edit.cursor].chars().count();
    let mut start = buffer.iter_at_offset(range.start as i32);
    let mut end = buffer.iter_at_offset(range.end as i32);
    buffer.begin_user_action();
    buffer.delete(&mut start, &mut end);
    buffer.insert(&mut start, &edit.text);
    buffer.place_cursor(&buffer.iter_at_offset(new_cursor as i32));
    buffer.end_user_action();
    true
}

/// Returns the buffer's whole text, including the Markdown syntax that the preview hides. Leaving
/// out hidden text would delete that syntax from the note when it's saved.
pub fn text(buffer: &gtk::TextBuffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).into()
}

/// What the preview last applied to a buffer.
#[derive(Default)]
struct Preview {
    /// Character offsets of each byte boundary of the text, for converting spans to offsets.
    byte_chars: Vec<usize>,
    /// Byte offsets of each character boundary, for converting the cursor to bytes.
    char_bytes: Vec<usize>,
    highlighter: Highlighter,
    /// Whether each span's syntax is showing.
    revealed: Vec<bool>,
    spans: Vec<Span>,
}

impl Preview {
    fn chars(&self, bytes: &Range<usize>) -> Range<usize> {
        self.byte_chars[bytes.start]..self.byte_chars[bytes.end]
    }

    /// The cursor or selection as a byte range.
    fn selection(&self, buffer: &gtk::TextBuffer) -> Range<usize> {
        let (start, end) = buffer.selection_bounds().unwrap_or_else(|| {
            let cursor = buffer.iter_at_mark(&buffer.get_insert());
            (cursor, cursor)
        });
        let byte = |offset: i32| {
            let offset = usize::try_from(offset).unwrap_or(0);
            self.char_bytes[offset.min(self.char_bytes.len() - 1)]
        };
        byte(start.offset())..byte(end.offset())
    }
}

/// The color of a heading of `level`: a hue from the GNOME palette for each of the first four
/// levels, purple, blue, green, and orange, in a darker shade on a light background and a lighter
/// one on a dark background. Deeper levels share the fourth's.
pub fn heading_color(level: u8, dark: bool) -> &'static str {
    let (light, dark_shade) = match level {
        1 => ("#813d9c", "#dc8add"),
        2 => ("#1a5fb4", "#99c1f1"),
        3 => ("#26a269", "#8ff0a4"),
        _ => ("#c64600", "#ffbe6f"),
    };
    if dark { dark_shade } else { light }
}

fn color_headings(table: &gtk::TextTagTable, dark: bool) {
    for (level, name) in (1..).zip(HEADING_COLORS) {
        if let Some(tag) = table.lookup(name) {
            tag.set_foreground(Some(heading_color(level, dark)));
        }
    }
}

fn add_preview_tags(table: &gtk::TextTagTable) {
    let dim = gdk::RGBA::new(0.5, 0.5, 0.5, 0.9);
    let shade = gdk::RGBA::new(0.5, 0.5, 0.5, 0.15);
    let heading = |name: &str, scale: f64| {
        gtk::TextTag::builder().name(name).weight(700).scale(scale).pixels_above_lines(6).build()
    };
    for tag in [
        gtk::TextTag::builder().name(SYNTAX).foreground_rgba(&dim).build(),
        heading(style_tag(Style::Heading(1)).unwrap(), 1.6),
        heading(style_tag(Style::Heading(2)).unwrap(), 1.4),
        heading(style_tag(Style::Heading(3)).unwrap(), 1.2),
        heading(style_tag(Style::Heading(4)).unwrap(), 1.0),
        gtk::TextTag::builder()
            .name(style_tag(Style::BlockQuote).unwrap())
            .style(pango::Style::Italic)
            .foreground_rgba(&dim)
            .build(),
        gtk::TextTag::builder().name(QUOTE_MARGIN).build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Code).unwrap())
            .family("monospace")
            .background_rgba(&shade)
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::CodeBlock).unwrap())
            .family("monospace")
            .build(),
        gtk::TextTag::builder().name(CODE_BLOCK_SHADE).paragraph_background_rgba(&shade).build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Emphasis).unwrap())
            .style(pango::Style::Italic)
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Strikethrough).unwrap())
            .strikethrough(true)
            .build(),
        gtk::TextTag::builder().name(style_tag(Style::Strong).unwrap()).weight(700).build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Highlight).unwrap())
            .background_rgba(&gdk::RGBA::new(0.96, 0.83, 0.18, 0.45))
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Completed).unwrap())
            .foreground_rgba(&dim)
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Checkbox(false)).unwrap())
            .family("monospace")
            .weight(700)
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::Checkbox(true)).unwrap())
            .family("monospace")
            .weight(700)
            .foreground_rgba(&dim)
            .build(),
        gtk::TextTag::builder()
            .name(style_tag(Style::ListPrefix).unwrap())
            .family("monospace")
            // GTK's GL renderer leaves a faint ghost of a marker that moves or goes, such as when
            // Tab indents an item, because it redraws too little around the old marker. A
            // background makes it redraw the marker's whole cell. A fully transparent one renders
            // black. Cairo and GSK_DEBUG=full-redraw never show the ghost, so check a change here
            // against them.
            .background_rgba(&gdk::RGBA::new(0.5, 0.5, 0.5, 0.01))
            .build(),
        gtk::TextTag::builder().name(HIDDEN).invisible(true).build(),
    ] {
        table.add(&tag);
    }
    // A tag added later wins, so syntax would take the color of a heading it sits in.
    for name in HEADING_COLORS {
        table.add(&gtk::TextTag::builder().name(name).build());
    }
    if let Some(syntax) = table.lookup(SYNTAX) {
        syntax.set_priority(table.size() - 1);
    }
}

/// The tag that styles a span, if the preview styles it. Links get their style from
/// `highlight_links`, which knows whether their targets exist.
fn style_tag(style: Style) -> Option<&'static str> {
    match style {
        Style::BlockQuote => Some("markup-quote"),
        Style::Code => Some("markup-code"),
        Style::CodeBlock => Some("markup-code-block"),
        Style::Completed => Some("markup-completed"),
        Style::Emphasis => Some("markup-emphasis"),
        Style::Heading(1) => Some("markup-heading-1"),
        Style::Heading(2) => Some("markup-heading-2"),
        Style::Heading(3) => Some("markup-heading-3"),
        Style::Heading(_) => Some("markup-heading-4"),
        Style::Highlight => Some("markup-highlight"),
        Style::Link => None,
        Style::ListPrefix => Some("markup-list-prefix"),
        Style::Strikethrough => Some("markup-strikethrough"),
        Style::Strong => Some("markup-strong"),
        Style::Checkbox(false) => Some("markup-checkbox"),
        Style::Checkbox(true) => Some("markup-checkbox-checked"),
    }
}

fn attach_preview(buffer: &sourceview5::Buffer) {
    let preview = Rc::new(RefCell::new(Preview::default()));
    buffer.connect_changed({
        let preview = preview.clone();
        move |buffer| {
            contain_panic(|| restyle(buffer.upcast_ref(), &mut preview.borrow_mut()));
        }
    });
    buffer.connect_style_scheme_notify({
        let preview = preview.clone();
        move |buffer| {
            contain_panic(|| restyle(buffer.upcast_ref(), &mut preview.borrow_mut()));
        }
    });
    buffer.connect_mark_set(move |buffer, _, mark| {
        if *mark == buffer.get_insert() || *mark == buffer.selection_bound() {
            contain_panic(|| reveal(buffer.upcast_ref(), &mut preview.borrow_mut()));
        }
    });
}

/// Runs `f`, and stops a panic in it from going further. A panic can't unwind through a GTK signal
/// handler, so it would abort the app and lose edits not yet saved, all over a missing style. The
/// panic hook still reports it, and the next change restyles the buffer from scratch.
fn contain_panic(f: impl FnOnce()) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(f));
}

/// Styles every span from scratch, after the text changes.
fn restyle(buffer: &gtk::TextBuffer, preview: &mut Preview) {
    let (start, end) = buffer.bounds();
    let mut names = vec![CODE_BLOCK_SHADE, HIDDEN, QUOTE_MARGIN, SYNTAX];
    names.extend(HEADING_COLORS);
    names.extend((1..=4).filter_map(|level| style_tag(Style::Heading(level))));
    names.extend(
        [Style::BlockQuote, Style::Code, Style::CodeBlock, Style::Emphasis]
            .into_iter()
            .chain([Style::Completed, Style::Highlight, Style::ListPrefix, Style::Strikethrough])
            .chain([Style::Strong])
            .chain([Style::Checkbox(false), Style::Checkbox(true)])
            .filter_map(style_tag),
    );
    for name in names {
        buffer.remove_tag_by_name(name, &start, &end);
    }
    let body = text(buffer);
    let mut byte_chars = vec![0; body.len() + 1];
    let mut char_bytes = Vec::with_capacity(body.len() + 1);
    for (char_index, (byte, c)) in body.char_indices().enumerate() {
        char_bytes.push(byte);
        for offset in 0..c.len_utf8() {
            byte_chars[byte + offset] = char_index;
        }
    }
    char_bytes.push(body.len());
    byte_chars[body.len()] = char_bytes.len() - 1;
    preview.byte_chars = byte_chars;
    preview.char_bytes = char_bytes;
    preview.spans = spans(&body);
    let selection = preview.selection(buffer);
    preview.revealed = preview.spans.iter().map(|span| span.touches(&selection)).collect();
    for (span, &revealed) in preview.spans.iter().zip(&preview.revealed) {
        if let Some(name) = style_tag(span.style) {
            apply(buffer, name, preview.chars(&span.range));
        }
        // GTK takes a line's paragraph properties, such as its margin and background, from the
        // tags at the line's first character. A block nested in a list item starts partway into its
        // first line, after the item's marker or indentation.
        let line_start = body[..span.range.start].rfind('\n').map_or(0, |i| i + 1);
        match span.style {
            Style::CodeBlock => {
                apply(buffer, CODE_BLOCK_SHADE, preview.chars(&(line_start..span.range.end)))
            }
            Style::BlockQuote if line_start == span.range.start => {
                apply(buffer, QUOTE_MARGIN, preview.chars(&span.range));
            }
            _ => {}
        }
        if let Style::Heading(level) = span.style {
            let color = HEADING_COLORS[usize::from(level.clamp(1, 4)) - 1];
            let mut start = span.range.start;
            for syntax in span.syntax.iter().chain([&(span.range.end..span.range.end)]) {
                if start < syntax.start {
                    apply(buffer, color, preview.chars(&(start..syntax.start)));
                }
                start = start.max(syntax.end);
            }
        }
        for syntax in &span.syntax {
            if !matches!(span.style, Style::Heading(_)) {
                apply(buffer, SYNTAX, preview.chars(syntax));
            }
            if !revealed {
                apply(buffer, HIDDEN, preview.chars(syntax));
            }
        }
    }
    if let Some(buffer) = buffer.downcast_ref::<sourceview5::Buffer>() {
        let byte_chars = &preview.byte_chars;
        preview.highlighter.apply(buffer, &body, |byte| byte_chars[byte]);
    }
}

/// Shows the syntax of spans the cursor now touches, and hides it for spans it left.
fn reveal(buffer: &gtk::TextBuffer, preview: &mut Preview) {
    // Marks can move before the buffer reports a change, while the spans are stale.
    if preview.char_bytes.len() != buffer.char_count() as usize + 1 {
        return;
    }
    let selection = preview.selection(buffer);
    for i in 0..preview.spans.len() {
        let revealed = preview.spans[i].touches(&selection);
        if revealed == preview.revealed[i] {
            continue;
        }
        preview.revealed[i] = revealed;
        for syntax in &preview.spans[i].syntax {
            let range = preview.chars(syntax);
            let start = buffer.iter_at_offset(range.start as i32);
            let end = buffer.iter_at_offset(range.end as i32);
            if revealed {
                buffer.remove_tag_by_name(HIDDEN, &start, &end);
            } else {
                buffer.apply_tag_by_name(HIDDEN, &start, &end);
            }
        }
    }
}

/// Styles every link in the buffer. `exists` tells whether a link's target note exists.
pub fn highlight_links(buffer: &gtk::TextBuffer, exists: impl Fn(&Link) -> bool) {
    let (start, end) = buffer.bounds();
    for name in [LINK, TARGET, BROKEN] {
        buffer.remove_tag_by_name(name, &start, &end);
    }
    let body = text(buffer);
    for link in parse_links(&body) {
        let range = char_range(&body, link.range.clone());
        let name = if exists(&link) { LINK } else { BROKEN };
        apply(buffer, name, range.clone());
        if link.label.is_some() {
            let target_end = body[link.range.clone()].find('|').map(|i| link.range.start + i + 1);
            if let Some(target_end) = target_end {
                apply(buffer, TARGET, char_range(&body, link.range.start..target_end));
            }
        }
    }
}

/// Checks or unchecks the list item at the character `offset`, or gives it a checkbox if it has
/// none, as one undoable edit. Returns whether there was an item to change.
pub fn toggle_checkbox_at(buffer: &gtk::TextBuffer, offset: i32) -> bool {
    let body = text(buffer);
    let Ok(offset) = usize::try_from(offset) else {
        return false;
    };
    let byte = body.char_indices().nth(offset).map_or(body.len(), |(byte, _)| byte);
    let Some((mark, replacement)) = toggle_checkbox(&body, byte) else {
        return false;
    };
    let range = char_range(&body, mark);
    let mut start = buffer.iter_at_offset(range.start as i32);
    let mut end = buffer.iter_at_offset(range.end as i32);
    buffer.begin_user_action();
    buffer.delete(&mut start, &mut end);
    buffer.insert(&mut start, replacement);
    buffer.end_user_action();
    true
}

/// Finds the link that contains the character at `offset`.
pub fn link_at(buffer: &gtk::TextBuffer, offset: i32) -> Option<Link> {
    let body = text(buffer);
    let offset = usize::try_from(offset).ok()?;
    parse_links(&body)
        .into_iter()
        .find(|link| char_range(&body, link.range.clone()).contains(&offset))
}

fn apply(buffer: &gtk::TextBuffer, name: &str, range: Range<usize>) {
    let start = buffer.iter_at_offset(range.start as i32);
    let end = buffer.iter_at_offset(range.end as i32);
    buffer.apply_tag_by_name(name, &start, &end);
}

/// Converts a byte range in `text` to the character range that `GtkTextBuffer` offsets use.
fn char_range(text: &str, bytes: Range<usize>) -> Range<usize> {
    let start = text[..bytes.start].chars().count();
    start..start + text[bytes].chars().count()
}
