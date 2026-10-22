// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::lists::Item as ListItem;
use crate::{LinkSyntax, parse_links};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Style {
    BlockQuote,
    /// A checklist item's box, `[ ]` or `[x]`, and whether it's checked.
    Checkbox(bool),
    Code,
    CodeBlock,
    /// The text of a checklist item that's checked.
    Completed,
    Emphasis,
    /// A heading of level 1 to 6.
    Heading(u8),
    /// Text marked with `==`, as Obsidian, Pandoc, and others mark highlights.
    Highlight,
    Link,
    /// The indentation, marker, and checkbox that start a list item's line, or the indentation of a
    /// line that continues one. Editors draw these in a monospace font, so that continuation lines
    /// and nested items line up with the text above them.
    ListPrefix,
    Strikethrough,
    Strong,
}

/// A styled element of a note's body, for editors that render Markdown as it's written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    /// Byte range of the whole element, including its syntax.
    pub range: Range<usize>,
    pub style: Style,
    /// Byte ranges of the syntax to hide while the cursor is away from the element. None of them
    /// contain a line break.
    pub syntax: Vec<Range<usize>>,
}

impl Span {
    /// Whether the cursor or selection `range` touches the element, so its syntax should show.
    pub fn touches(&self, range: &Range<usize>) -> bool {
        self.range.start <= range.end && range.start <= self.range.end
    }
}

/// Finds the styled elements of `body`, ordered by where they start.
///
/// The elements follow CommonMark, with three exceptions. A setext heading underlined with a single
/// `-` isn't styled, as in Obsidian's editor, so a line doesn't flash into a heading while the
/// next line starts a list. A heading without text, such as `##` while it's being typed, stays
/// plain text until text follows its space. And `==` marks a highlight, which CommonMark lacks.
pub fn spans(body: &str) -> Vec<Span> {
    struct Frame {
        /// Bytes covered by the element's children: its content, without its syntax.
        inner: Option<Range<usize>>,
        range: Range<usize>,
        style: Option<Style>,
    }

    let mut spans = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    // Highlight markers in the block being read, to pair once the block ends.
    let mut marks: Vec<Range<usize>> = Vec::new();
    let mut in_code_block = false;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        if !matches!(event, Event::End(_))
            && let Some(parent) = stack.last_mut()
        {
            parent.inner = Some(match parent.inner.take() {
                Some(inner) => inner.start.min(range.start)..inner.end.max(range.end),
                None => range.clone(),
            });
        }
        let inline = match &event {
            Event::Start(tag) => is_inline(tag),
            Event::End(tag) => is_inline_end(*tag),
            _ => true,
        };
        if !inline {
            pair_highlights(body, &mut marks, &mut spans);
        }
        match &event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            _ => {}
        }
        match event {
            Event::Text(_) if !in_code_block => marks.extend(highlight_marks(body, &range)),
            Event::Start(tag) => {
                let style = match tag {
                    Tag::BlockQuote(_) => Some(Style::BlockQuote),
                    Tag::CodeBlock(_) => Some(Style::CodeBlock),
                    Tag::Emphasis => Some(Style::Emphasis),
                    Tag::Heading { level, .. } => Some(Style::Heading(heading_level(level))),
                    Tag::Link { .. } => Some(Style::Link),
                    Tag::Strikethrough => Some(Style::Strikethrough),
                    Tag::Strong => Some(Style::Strong),
                    _ => None,
                };
                stack.push(Frame { inner: None, range, style });
            }
            Event::End(_) => {
                if let Some(Frame { inner, range, style: Some(style) }) = stack.pop() {
                    // pulldown-cmark can report a child outside its parent: in a checklist item
                    // that turns into a setext heading, the box comes before the heading's start.
                    let inner = inner
                        .map(|inner| inner.start.max(range.start)..inner.end.min(range.end))
                        .filter(|inner| inner.start < inner.end);
                    if style == Style::Heading(2) && is_single_dash_underline(body, &range) {
                        continue;
                    }
                    if matches!(style, Style::Heading(_)) && inner.is_none() {
                        continue;
                    }
                    let syntax = match (style, inner) {
                        (Style::BlockQuote, _) => quote_markers(body, &range),
                        (Style::CodeBlock, _) | (_, None) => Vec::new(),
                        (_, Some(inner)) => vec![range.start..inner.start, inner.end..range.end],
                    };
                    spans.push(span(body, range, style, syntax));
                }
            }
            Event::Code(_) => {
                let ticks = body[range.clone()].bytes().take_while(|&b| b == b'`').count();
                let syntax = vec![range.start..range.start + ticks, range.end - ticks..range.end];
                spans.push(span(body, range, Style::Code, syntax));
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    let line_end =
                        body[range.end..].find('\n').map_or(body.len(), |i| range.end + i);
                    let rest = &body[range.end..line_end];
                    let text_start = line_end - rest.trim_start().len();
                    spans.push(span(body, text_start..line_end, Style::Completed, Vec::new()));
                }
                spans.push(span(body, range, Style::Checkbox(checked), Vec::new()));
            }
            _ => {}
        }
    }
    pair_highlights(body, &mut marks, &mut spans);
    spans.extend(list_prefixes(body));
    for link in parse_links(body).into_iter().filter(|link| link.syntax == LinkSyntax::Wiki) {
        let Range { start, end } = link.range;
        let label_start = body[start + 2..end - 2].find('|').map_or(start + 2, |i| start + 3 + i);
        let syntax = vec![start..label_start, end - 2..end];
        spans.push(span(body, start..end, Style::Link, syntax));
    }
    spans.sort_by_key(|span| span.range.start);
    spans
}

/// A run of text that shows in a line of Markdown, with the inline styles that apply to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Piece {
    pub styles: Vec<Style>,
    pub text: String,
}

/// Splits a single line of Markdown, such as a note's label, into the text that shows, leaving
/// out the syntax, with the inline styles of each run: emphasis, strong, strikethrough, code,
/// highlights, and links.
pub fn inline_pieces(line: &str) -> Vec<Piece> {
    let inline = |style: &Style| {
        matches!(
            style,
            Style::Code
                | Style::Emphasis
                | Style::Highlight
                | Style::Link
                | Style::Strikethrough
                | Style::Strong
        )
    };
    let spans: Vec<Span> = spans(line).into_iter().filter(|span| inline(&span.style)).collect();
    let mut cuts = vec![0, line.len()];
    for span in &spans {
        cuts.extend([span.range.start, span.range.end]);
        cuts.extend(span.syntax.iter().flat_map(|syntax| [syntax.start, syntax.end]));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces: Vec<Piece> = Vec::new();
    for cut in cuts.windows(2) {
        let piece = cut[0]..cut[1];
        let hidden = spans
            .iter()
            .flat_map(|span| &span.syntax)
            .any(|syntax| syntax.start <= piece.start && piece.end <= syntax.end);
        if hidden || piece.is_empty() {
            continue;
        }
        let styles: Vec<Style> = spans
            .iter()
            .filter(|span| span.range.start <= piece.start && piece.end <= span.range.end)
            .map(|span| span.style)
            .collect();
        match pieces.last_mut() {
            Some(last) if last.styles == styles => last.text.push_str(&line[piece]),
            _ => pieces.push(Piece { styles, text: line[piece].to_owned() }),
        }
    }
    pieces
}

/// Whether a tag sits inside a line of text, rather than holding lines of its own.
fn is_inline(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Emphasis
            | Tag::Image { .. }
            | Tag::Link { .. }
            | Tag::Strikethrough
            | Tag::Strong
            | Tag::Subscript
            | Tag::Superscript
    )
}

fn is_inline_end(tag: TagEnd) -> bool {
    matches!(
        tag,
        TagEnd::Emphasis
            | TagEnd::Image
            | TagEnd::Link
            | TagEnd::Strikethrough
            | TagEnd::Strong
            | TagEnd::Subscript
            | TagEnd::Superscript
    )
}

/// The runs of exactly two `=` in the text at `range`, which can open or close a highlight.
fn highlight_marks(body: &str, range: &Range<usize>) -> Vec<Range<usize>> {
    let bytes = body.as_bytes();
    let mut marks = Vec::new();
    let mut i = range.start;
    while i < range.end {
        if bytes[i] != b'=' {
            i += 1;
            continue;
        }
        let start = i;
        while i < range.end && bytes[i] == b'=' {
            i += 1;
        }
        if i - start == 2 {
            marks.push(start..i);
        }
    }
    marks
}

/// Pairs the highlight markers of one block, in order, like `~~` pairs for strikethrough: a marker
/// opens if text follows it without a space, and closes if text precedes it without a space.
fn pair_highlights(body: &str, marks: &mut Vec<Range<usize>>, spans: &mut Vec<Span>) {
    let mut open: Option<Range<usize>> = None;
    for mark in marks.drain(..) {
        let can_open = body[mark.end..].chars().next().is_some_and(|c| !c.is_whitespace());
        let can_close = body[..mark.start].chars().next_back().is_some_and(|c| !c.is_whitespace());
        open = match open.take() {
            Some(opener) if can_close => {
                let range = opener.start..mark.end;
                spans.push(span(body, range, Style::Highlight, vec![opener, mark]));
                None
            }
            Some(opener) => Some(if can_open { mark } else { opener }),
            None => can_open.then_some(mark),
        };
    }
}

/// Finds the innermost list item that byte `offset` falls in, and returns the edit that toggles
/// its checkbox: the byte range to replace, and the text to replace it with. An item without a
/// checkbox gets an unchecked one. An item covers its lines from their start, including
/// indentation, up to the end of its text, but not its nested items.
pub fn toggle_checkbox(body: &str, offset: usize) -> Option<(Range<usize>, &'static str)> {
    struct OpenItem {
        /// The checkbox's range and whether it's checked, once the parser reports it.
        checkbox: Option<(Range<usize>, bool)>,
        range: Range<usize>,
    }

    let mut open: Vec<OpenItem> = Vec::new();
    let mut items = Vec::new();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Item) => open.push(OpenItem { checkbox: None, range }),
            Event::TaskListMarker(checked) => {
                if let Some(item) = open.last_mut() {
                    item.checkbox = Some((range, checked));
                }
            }
            Event::End(TagEnd::Item) => {
                let Some(OpenItem { checkbox, range: item }) = open.pop() else { continue };
                let line_start = body[..item.start].rfind('\n').map_or(0, |i| i + 1);
                let start = if body[line_start..item.start].trim().is_empty() {
                    line_start
                } else {
                    item.start
                };
                let text_end = item.start + body[item.clone()].trim_end().len();
                let trailing = body[text_end..].bytes().take_while(|b| matches!(b, b' ' | b'\t'));
                let end = text_end + trailing.count();
                let edit = match checkbox {
                    Some((mark, checked)) => {
                        (mark.start + 1..mark.end - 1, if checked { " " } else { "x" })
                    }
                    None => {
                        let line_end =
                            body[line_start..].find('\n').map_or(body.len(), |i| line_start + i);
                        let line = &body[line_start..line_end];
                        let Some(parsed) = ListItem::parse(line) else {
                            continue;
                        };
                        let at = line_start + parsed.content_start;
                        // GitHub Flavored Markdown only counts a box followed by text as a
                        // checkbox, but an item whose only text is a box should toggle it, not get
                        // a second box.
                        let lone_box =
                            parsed.checkbox.then(|| line[..parsed.content_start].rfind('['));
                        match lone_box.flatten() {
                            Some(open) => {
                                let mark = line_start + open + 1..line_start + open + 2;
                                let checked = body[mark.clone()] != *" ";
                                (mark, if checked { " " } else { "x" })
                            }
                            None => {
                                let spaced = body[..at].ends_with(' ');
                                (at..at, if spaced { "[ ] " } else { " [ ] " })
                            }
                        }
                    }
                };
                items.push((start..end, edit));
            }
            _ => {}
        }
    }
    items
        .into_iter()
        .filter(|(item, _)| item.start <= offset && offset <= item.end)
        .min_by_key(|(item, _)| item.len())
        .map(|(_, edit)| edit)
}

/// A heading, for an outline of a note.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Heading {
    /// From 1 to 6.
    pub level: u8,
    /// Byte range of the whole heading, including its syntax.
    pub range: Range<usize>,
    /// The heading's text as a reader sees it, without Markdown syntax.
    pub text: String,
}

/// Finds the headings in `body` that an editor styles as headings, in order.
pub fn outline(body: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut open: Option<(u8, Range<usize>, String)> = None;
    for (event, range) in Parser::new_ext(body, Options::ENABLE_TASKLISTS).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                open = Some((heading_level(level), range, String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, heading)) = &mut open {
                    heading.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, heading)) = &mut open {
                    heading.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let Some((level, range, raw)) = open.take() else { continue };
                if level == 2 && is_single_dash_underline(body, &range) {
                    continue;
                }
                let text = without_wikilinks(&raw).trim().to_owned();
                if !text.is_empty() {
                    let range = span(body, range, Style::Heading(level), Vec::new()).range;
                    headings.push(Heading { level, range, text });
                }
            }
            _ => {}
        }
    }
    headings
}

/// Replaces each wikilink in `text` with its label, or its target if it has none.
fn without_wikilinks(text: &str) -> String {
    let mut plain = String::new();
    let mut rest_start = 0;
    for link in parse_links(text).into_iter().filter(|link| link.syntax == LinkSyntax::Wiki) {
        plain.push_str(&text[rest_start..link.range.start]);
        plain.push_str(link.label.as_deref().unwrap_or(&link.target));
        rest_start = link.range.end;
    }
    plain.push_str(&text[rest_start..]);
    plain
}

/// A fenced code block that names its language, such as ```` ```rust ````.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeBlock {
    /// Byte range of the code between the fences. Inside a list item, it includes the indentation
    /// of every line but the first.
    pub content: Range<usize>,
    /// The first word of the opening fence's info string, in lowercase.
    pub language: String,
}

/// Finds the fenced code blocks in `body` that name a language and contain code.
pub fn code_blocks(body: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut open: Option<(String, Option<Range<usize>>)> = None;
    for (event, range) in Parser::new_ext(body, Options::ENABLE_TASKLISTS).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let language = info.split_whitespace().next().map(str::to_lowercase);
                open = language.map(|language| (language, None));
            }
            Event::Text(_) => {
                if let Some((_, content)) = &mut open {
                    let start = content.as_ref().map_or(range.start, |content| content.start);
                    *content = Some(start..range.end);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, Some(content))) = open.take() {
                    blocks.push(CodeBlock { content, language });
                }
            }
            _ => {}
        }
    }
    blocks
}

/// The fence that closes a fenced code block left open at the end of `body`, which would otherwise
/// swallow whatever text follows `body`.
pub(crate) fn open_fence(body: &str) -> Option<String> {
    // Checking whether a block runs into appended text leaves pulldown-cmark to decide where block
    // quotes and list items end the blocks inside them.
    let probe = format!("{body}\n\nx");
    Parser::new_ext(&probe, Options::ENABLE_TASKLISTS).into_offset_iter().find_map(
        |(event, range)| {
            let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) = event else {
                return None;
            };
            if range.end < probe.len() {
                return None;
            }
            let opening = probe[range].trim_start();
            let marker = opening.chars().next()?;
            Some(opening.chars().take_while(|&c| c == marker).collect())
        },
    )
}

fn list_prefixes(body: &str) -> Vec<Span> {
    let mut prefixes = Vec::new();
    let mut depth = 0;
    let mut list_start = 0;
    for (event, range) in Parser::new_ext(body, Options::ENABLE_TASKLISTS).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_)) => {
                if depth == 0 {
                    list_start = range.start;
                }
                depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                depth -= 1;
                if depth > 0 {
                    continue;
                }
                let mut line_start = body[..list_start].rfind('\n').map_or(0, |i| i + 1);
                while line_start < range.end {
                    let line_end =
                        body[line_start..].find('\n').map_or(body.len(), |i| line_start + i);
                    let line = &body[line_start..line_end];
                    let prefix = ListItem::parse(line).map_or_else(
                        || line.len() - line.trim_start_matches(' ').len(),
                        |item| item.content_start,
                    );
                    if prefix > 0 {
                        let range = line_start..line_start + prefix;
                        prefixes.push(span(body, range, Style::ListPrefix, Vec::new()));
                    }
                    line_start = line_end + 1;
                }
            }
            _ => {}
        }
    }
    prefixes
}

fn is_single_dash_underline(body: &str, heading: &Range<usize>) -> bool {
    let text = body[heading.clone()].trim_end();
    let underline = text.rsplit('\n').next().unwrap_or(text).trim();
    !text.trim_start().starts_with('#') && underline == "-"
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Finds the `>` that starts each line of a block quote, with the space after it.
fn quote_markers(body: &str, range: &Range<usize>) -> Vec<Range<usize>> {
    let mut markers = Vec::new();
    let mut line_start = range.start;
    for line in body[range.clone()].split_inclusive('\n') {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if line[indent..].starts_with('>') {
            let space = usize::from(line[indent + 1..].starts_with(' '));
            markers.push(line_start..line_start + indent + 1 + space);
        }
        line_start += line.len();
    }
    markers
}

/// Builds a span whose range ends before any trailing line break, so a cursor on the next line
/// doesn't touch it, and whose syntax never hides a line break.
fn span(body: &str, mut range: Range<usize>, style: Style, syntax: Vec<Range<usize>>) -> Span {
    while range.end > range.start && matches!(body.as_bytes()[range.end - 1], b'\n' | b'\r') {
        range.end -= 1;
    }
    let syntax = syntax
        .into_iter()
        .filter(|range| range.start < range.end)
        .flat_map(|range| {
            let mut pieces = Vec::new();
            let mut piece_start = range.start;
            for (i, byte) in body[range.clone()].bytes().enumerate() {
                if matches!(byte, b'\n' | b'\r') {
                    pieces.push(piece_start..range.start + i);
                    piece_start = range.start + i + 1;
                }
            }
            pieces.push(piece_start..range.end);
            pieces
        })
        .filter(|piece| !piece.is_empty())
        .collect();
    Span { range, style, syntax }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoteId;

    fn hidden(body: &str) -> Vec<(Style, Vec<&str>)> {
        spans(body)
            .into_iter()
            .map(|span| (span.style, span.syntax.iter().map(|r| &body[r.clone()]).collect()))
            .collect()
    }

    #[test]
    fn finds_inline_syntax() {
        assert_eq!(
            hidden("Some *em*, **strong**, ~~gone~~, `code`, and [label](https://x.example)."),
            [
                (Style::Emphasis, vec!["*", "*"]),
                (Style::Strong, vec!["**", "**"]),
                (Style::Strikethrough, vec!["~~", "~~"]),
                (Style::Code, vec!["`", "`"]),
                (Style::Link, vec!["[", "](https://x.example)"]),
            ]
        );
    }

    #[test]
    fn splits_lines_into_pieces() {
        let id = NoteId::generate();
        let line = format!("Plan **the ~~trip~~** with [[{id}|Sam]] `now`");
        let pieces: Vec<(String, Vec<Style>)> =
            inline_pieces(&line).into_iter().map(|piece| (piece.text, piece.styles)).collect();
        assert_eq!(
            pieces,
            [
                ("Plan ".into(), vec![]),
                ("the ".into(), vec![Style::Strong]),
                ("trip".into(), vec![Style::Strong, Style::Strikethrough]),
                (" with ".into(), vec![]),
                ("Sam".into(), vec![Style::Link]),
                (" ".into(), vec![]),
                ("now".into(), vec![Style::Code]),
            ]
        );
    }

    #[test]
    fn finds_highlights() {
        for (body, expected) in [
            ("==marked==", vec![(Style::Highlight, vec!["==", "=="])]),
            (
                "==a *b* c==",
                vec![(Style::Highlight, vec!["==", "=="]), (Style::Emphasis, vec!["*", "*"])],
            ),
            ("a == b == c", vec![]),
            ("===no===", vec![]),
            ("`==code==`", vec![(Style::Code, vec!["`", "`"])]),
            ("==one\n\ntwo==", vec![]),
            ("```\n==code==\n```", vec![(Style::CodeBlock, vec![])]),
        ] {
            assert_eq!(hidden(body), expected, "{body:?}");
        }
    }

    #[test]
    fn finds_nested_syntax() {
        assert_eq!(
            hidden("**bold *both***"),
            [(Style::Strong, vec!["**", "**"]), (Style::Emphasis, vec!["*", "*"])]
        );
    }

    #[test]
    fn needs_two_dashes_for_setext_headings() {
        for (body, expected) in [
            ("this\n-", vec![]),
            ("this\n-\n", vec![]),
            ("this\n - \n\nmore", vec![]),
            ("this\n--", vec![(Style::Heading(2), vec!["--"])]),
            ("this\n=", vec![(Style::Heading(1), vec!["="])]),
            ("## -", vec![(Style::Heading(2), vec!["## "])]),
            ("##", vec![]),
            ("## ", vec![]),
            ("## a", vec![(Style::Heading(2), vec!["## "])]),
        ] {
            assert_eq!(hidden(body), expected, "{body:?}");
        }
    }

    #[test]
    fn finds_headings() {
        let body = "# Title\n\nText\n\n## Sub ##\n";
        assert_eq!(
            hidden(body),
            [(Style::Heading(1), vec!["# "]), (Style::Heading(2), vec!["## ", " ##"])]
        );
        let span = &spans(body)[0];
        assert_eq!(&body[span.range.clone()], "# Title");
    }

    #[test]
    fn finds_wikilinks() {
        let id = NoteId::generate();
        let with_label = format!("[[{id}|the plan]]");
        let without = format!("[[{id}]]");
        assert_eq!(hidden(&with_label), [(Style::Link, vec![&*format!("[[{id}|"), "]]"])]);
        assert_eq!(hidden(&without), [(Style::Link, vec!["[[", "]]"])]);
    }

    #[test]
    fn parses_every_prefix_without_panicking() {
        let id = NoteId::generate();
        for full in [
            "- [ ] parent\n  continued\n  - [ ] child\n".to_owned(),
            "- [x] done\n  ===\n> - [ ] quoted *item*\n>   ---\n".to_owned(),
            format!(
                "# Head **bold [[{id}|link]]** `x`\n\n1. [ ] [a](https://x.example)\n   ~~y~~\n"
            ),
            "```\n- [ ] in code\n```\n* [X] **a\n  b**\n  -\n".to_owned(),
        ] {
            for (end, _) in full.char_indices().chain([(full.len(), ' ')]) {
                let body = &full[..end];
                for span in spans(body) {
                    assert!(span.range.start <= span.range.end, "{body:?}");
                    assert!(span.syntax.iter().all(|range| range.start < range.end), "{body:?}");
                }
                for offset in 0..=body.len() {
                    toggle_checkbox(body, offset);
                }
            }
        }
    }

    #[test]
    fn outlines_headings() {
        let id = NoteId::generate();
        let body = format!(
            "# Plan *for* `now`\n\ntext\n\n## See [[{id}|the deal]]\n\n\
            Setext\n--\n\nnot\n-\n\n#\n\n### Last"
        );
        let found: Vec<_> = outline(&body)
            .into_iter()
            .map(|heading| (heading.level, heading.text, &body[heading.range]))
            .collect();
        assert_eq!(
            found,
            [
                (1, "Plan for now".to_owned(), "# Plan *for* `now`"),
                (2, "See the deal".to_owned(), &*format!("## See [[{id}|the deal]]")),
                (2, "Setext".to_owned(), "Setext\n--"),
                (3, "Last".to_owned(), "### Last"),
            ]
        );
    }

    #[test]
    fn finds_code_blocks_with_languages() {
        let body = "```Rust ignore\nfn main() {}\nlet x = 1;\n```\n\n```\nplain\n```\n\n\
            ```py\n```\n\n- item\n  ```sh\n  echo hi\n  ls\n  ```\n";
        let found: Vec<_> = code_blocks(body)
            .into_iter()
            .map(|block| (block.language, &body[block.content]))
            .collect();
        assert_eq!(
            found,
            [
                ("rust".to_owned(), "fn main() {}\nlet x = 1;\n"),
                ("sh".to_owned(), "echo hi\n  ls\n"),
            ]
        );
    }

    #[test]
    fn finds_list_prefixes() {
        let body = "Intro\n\n- [ ] one\n      more\n  - two\n10. ten\n\nAfter\n";
        let prefixes: Vec<_> = spans(body)
            .into_iter()
            .filter(|span| span.style == Style::ListPrefix)
            .map(|span| &body[span.range])
            .collect();
        assert_eq!(prefixes, ["- [ ] ", "      ", "  - ", "10. "]);
    }

    #[test]
    fn finds_checklists() {
        let body = "- [ ] open\n- [x]  done *well*\n  - [X] nested\n";
        let found: Vec<_> = spans(body)
            .into_iter()
            .filter(|span| matches!(span.style, Style::Checkbox(_) | Style::Completed))
            .map(|span| (span.style, &body[span.range]))
            .collect();
        assert_eq!(
            found,
            [
                (Style::Checkbox(false), "[ ]"),
                (Style::Checkbox(true), "[x]"),
                (Style::Completed, "done *well*"),
                (Style::Checkbox(true), "[X]"),
                (Style::Completed, "nested"),
            ]
        );
    }

    #[test]
    fn toggles_checkboxes() {
        let toggle = |body: &str, offset| {
            toggle_checkbox(body, offset).map(|(range, mark)| {
                let mut body = body.to_owned();
                body.replace_range(range, mark);
                body
            })
        };
        assert_eq!(toggle("- [ ] a", 2).as_deref(), Some("- [x] a"));
        assert_eq!(toggle("- [x] abc", 8).as_deref(), Some("- [ ] abc"));
        assert_eq!(toggle("- [ ] a", 0).as_deref(), Some("- [x] a"));
        assert_eq!(toggle("- [ ] a\n\nafter", 9), None);
        assert_eq!(toggle("- plain item", 3).as_deref(), Some("- [ ] plain item"));
        assert_eq!(toggle("  * nested", 9).as_deref(), Some("  * [ ] nested"));
        assert_eq!(toggle("3.  wide", 0).as_deref(), Some("3.  [ ] wide"));
        assert_eq!(toggle("-", 1).as_deref(), Some("- [ ] "));
        assert_eq!(toggle("- [ ] ", 6).as_deref(), Some("- [x] "));
        assert_eq!(toggle("- [x]  \nnext", 7).as_deref(), Some("- [ ]  \nnext"));
        assert_eq!(toggle("- [ ]", 5).as_deref(), Some("- [x]"));
        assert_eq!(toggle("- [X]", 5).as_deref(), Some("- [ ]"));
        assert_eq!(toggle("- [ ] t  ", 9).as_deref(), Some("- [x] t  "));
        assert_eq!(toggle("plain text", 3), None);
        assert_eq!(toggle("[ ] not a list", 1), None);
        let nested = "- [ ] parent\n  continued\n  - [ ] child\n";
        assert_eq!(
            toggle(nested, 16).as_deref(),
            Some("- [x] parent\n  continued\n  - [ ] child\n")
        );
        assert_eq!(
            toggle(nested, 25).as_deref(),
            Some("- [ ] parent\n  continued\n  - [x] child\n")
        );
        assert_eq!(
            toggle(nested, 34).as_deref(),
            Some("- [ ] parent\n  continued\n  - [x] child\n")
        );
    }

    #[test]
    fn finds_quote_markers_without_line_breaks() {
        assert_eq!(hidden("> one\n> two\n"), [(Style::BlockQuote, vec!["> ", "> "])]);
    }

    #[test]
    fn leaves_code_blocks_and_empty_links_visible() {
        assert_eq!(
            hidden("```\n*x*\n```\n\n[](https://x.example)"),
            [(Style::CodeBlock, vec![]), (Style::Link, vec![])]
        );
    }

    #[test]
    fn touches_at_either_end() {
        let span = &spans("a *b* c")[0];
        assert!(!span.touches(&(1..1)));
        assert!(span.touches(&(2..2)));
        assert!(span.touches(&(5..5)));
        assert!(!span.touches(&(6..6)));
        assert!(span.touches(&(0..3)));
    }

    #[test]
    fn finds_open_fences() {
        assert_eq!(open_fence("Text\n\n```sh\nls"), Some("```".to_owned()));
        assert_eq!(open_fence("  ~~~~\ncode\n~~~"), Some("~~~~".to_owned()));
        assert_eq!(open_fence("```\ncode\n```"), None);
        assert_eq!(open_fence("> ```\n> code"), None);
        assert_eq!(open_fence("- ```\n  code"), None);
        assert_eq!(open_fence("Text"), None);
    }
}
