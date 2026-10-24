// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::NoteId;

const SCHEMES: [&str; 2] = ["commonplace:", "web+commonplace:"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinkSyntax {
    /// A web address written as plain text, which GitHub Flavored Markdown turns into a link.
    Bare,
    Markdown,
    Wiki,
}

/// A link found in a note's body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Link {
    pub label: Option<String>,
    /// The note the target names, if it's a valid identifier. The note may not exist.
    pub note: Option<NoteId>,
    /// Byte range of the whole link in the body.
    pub range: Range<usize>,
    pub syntax: LinkSyntax,
    pub target: String,
}

impl Link {
    /// The URI to open outside Commonplace, for a link that names one and doesn't lead to a note.
    pub fn external_uri(&self) -> Option<&str> {
        if self.syntax == LinkSyntax::Wiki || self.note.is_some() {
            return None;
        }
        let scheme = uri_scheme(&self.target)?;
        let ours = SCHEMES.iter().any(|ours| ours[..ours.len() - 1].eq_ignore_ascii_case(scheme));
        (!ours).then_some(self.target.as_str())
    }
}

/// Returns the scheme of `uri`, as RFC 3986 defines it, if it starts with one.
fn uri_scheme(uri: &str) -> Option<&str> {
    let (scheme, _) = uri.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next()?.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    valid.then_some(scheme)
}

pub fn note_uri(id: NoteId) -> String {
    format!("commonplace:note/{id}")
}

/// Parses a note URI in either scheme, ignoring any query or fragment.
pub fn parse_note_uri(uri: &str) -> Option<NoteId> {
    let rest = SCHEMES.iter().find_map(|scheme| {
        let prefix = uri.get(..scheme.len())?;
        prefix.eq_ignore_ascii_case(scheme).then(|| &uri[scheme.len()..])
    })?;
    let id = rest.strip_prefix("note/")?;
    let end = id.find(['?', '#']).unwrap_or(id.len());
    id[..end].parse().ok()
}

/// Formats the wikilink an editor inserts for a note.
pub fn wikilink(id: NoteId, label: Option<&str>) -> String {
    format!("[[{id}{}]]", label_part(label))
}

/// The part of a wikilink after its target and before its closing brackets: a `|` and the label,
/// escaped, or nothing.
fn label_part(label: Option<&str>) -> String {
    let Some(label) = label else {
        return String::new();
    };
    let mut part = String::from("|");
    for c in link_label(label).chars() {
        if ESCAPABLE.contains(&c) {
            part.push('\\');
        }
        part.push(c);
    }
    part
}

/// Turns `text` into a wikilink's label, which can't hold a line break.
pub fn link_label(text: &str) -> String {
    text.replace(['\n', '\r'], " ").trim().to_owned()
}

/// The characters a backslash escapes inside a wikilink.
const ESCAPABLE: [char; 3] = ['\\', ']', '|'];

/// The byte offsets of the backslashes in `text` that escape the character after them.
fn escapes(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\\' && chars.next_if(|(_, next)| ESCAPABLE.contains(next)).is_some() {
            found.push(i);
        }
    }
    found
}

/// Finds the first `pattern` in `text` that doesn't start with an escaped character.
fn find_unescaped(text: &str, pattern: &str) -> Option<usize> {
    let mut i = 0;
    while let Some(rest) = text.get(i..).filter(|rest| !rest.is_empty()) {
        if rest.starts_with('\\') && rest[1..].starts_with(ESCAPABLE) {
            i += 2;
        } else if rest.starts_with(pattern) {
            return Some(i);
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

fn unescape(text: &str) -> String {
    let escapes = escapes(text);
    text.char_indices().filter(|(i, _)| !escapes.contains(i)).map(|(_, c)| c).collect()
}

/// The byte offset in `body` of the `|` that starts the label of `link`, a wikilink, if it has
/// one.
pub fn label_separator(body: &str, link: &Link) -> Option<usize> {
    let inner = link.range.start + 2..link.range.end - 2;
    find_unescaped(&body[inner.clone()], "|").map(|i| inner.start + i)
}

/// The byte offsets in `body` of the backslashes that escape characters inside `link`, a
/// wikilink.
pub(crate) fn wikilink_escapes(body: &str, link: &Link) -> Vec<usize> {
    let inner = link.range.start + 2..link.range.end - 2;
    escapes(&body[inner.clone()]).into_iter().map(|i| inner.start + i).collect()
}

/// Gives each wikilink to a note that has no label the title that `title` returns for the note,
/// so the text shows the title.
pub fn expand_links(text: &str, mut title: impl FnMut(NoteId) -> Option<String>) -> String {
    replace_wikilinks(text, |link| {
        let id = link.note.filter(|_| link.label.is_none())?;
        let label = link_label(&title(id)?);
        (!label.is_empty()).then(|| wikilink(id, Some(&label)))
    })
}

/// Removes the label of each wikilink to a note whose label `is_title` accepts as a title the
/// text showed for the note.
pub fn collapse_links(text: &str, mut is_title: impl FnMut(NoteId, &str) -> bool) -> String {
    replace_wikilinks(text, |link| {
        let id = link.note?;
        is_title(id, link.label.as_deref()?).then(|| wikilink(id, None))
    })
}

/// The titles an editor fills into the links of the note it shows, as the links spec describes, so
/// it can store the note without them. `title` arguments return a note's title, or `None` if it
/// has none.
#[derive(Debug, Default)]
pub struct ShownTitles(HashMap<NoteId, Vec<String>>);

impl ShownTitles {
    /// The text to show for `body`, with each wikilink that has no label showing its note's title.
    pub fn expand(
        &mut self,
        body: &str,
        mut title: impl FnMut(NoteId) -> Option<String>,
    ) -> String {
        self.0.clear();
        expand_links(body, |id| {
            let title = shown_title(title(id))?;
            self.remember(id, &title);
            Some(title)
        })
    }

    /// The text to store for the editor's `text`, without the titles the editor showed. A label
    /// counts as a shown title if it's the note's current title or one the editor showed for it
    /// since `expand`, so undoing an update to a title still stores the link without it.
    pub fn collapse(&self, text: &str, mut title: impl FnMut(NoteId) -> Option<String>) -> String {
        collapse_links(text, |id, label| {
            shown_title(title(id)).as_deref() == Some(label)
                || self.0.get(&id).is_some_and(|shown| shown.iter().any(|t| t == label))
        })
    }

    /// The edits that bring the titles in `text` up to date: each byte range to replace, from the
    /// end of a wikilink's target to its closing brackets, with its replacement. A wikilink
    /// without a label gets its note's title, and one that shows an old title gets the current
    /// one, or none if the note has lost its title. Custom labels stay.
    pub fn updates(
        &mut self,
        text: &str,
        mut title: impl FnMut(NoteId) -> Option<String>,
    ) -> Vec<(Range<usize>, String)> {
        let mut titles: HashMap<NoteId, Option<String>> = HashMap::new();
        let mut updates = Vec::new();
        if !text.contains("[[") {
            return updates;
        }
        for link in parse_links(text).into_iter().filter(|link| link.syntax == LinkSyntax::Wiki) {
            let Some(id) = link.note else { continue };
            let current = titles.entry(id).or_insert_with(|| shown_title(title(id))).clone();
            let stale = match link.label.as_deref() {
                None => current.is_some(),
                Some(label) => {
                    current.as_deref() != Some(label)
                        && self.0.get(&id).is_some_and(|shown| shown.iter().any(|t| t == label))
                }
            };
            if !stale {
                continue;
            }
            if let Some(current) = &current {
                self.remember(id, current);
            }
            let inner_end = link.range.end - 2;
            let label_start = label_separator(text, &link).unwrap_or(inner_end);
            updates.push((label_start..inner_end, label_part(current.as_deref())));
        }
        updates
    }

    fn remember(&mut self, id: NoteId, title: &str) {
        let shown = self.0.entry(id).or_default();
        if !shown.iter().any(|t| t == title) {
            shown.push(title.to_owned());
        }
    }
}

/// A title as a wikilink shows it, or `None` if nothing of it remains.
fn shown_title(title: Option<String>) -> Option<String> {
    title.map(|title| link_label(&title)).filter(|title| !title.is_empty())
}

/// Replaces each wikilink in `text` with its label, or else the title that `title` returns for its
/// note, or else its target.
pub fn plain_links(text: &str, mut title: impl FnMut(NoteId) -> Option<String>) -> String {
    replace_wikilinks(text, |link| {
        Some(match &link.label {
            Some(label) => label.clone(),
            None => link.note.and_then(&mut title).unwrap_or_else(|| link.target.clone()),
        })
    })
}

fn replace_wikilinks(text: &str, mut replace: impl FnMut(&Link) -> Option<String>) -> String {
    if !text.contains("[[") {
        return text.to_owned();
    }
    let mut result = String::with_capacity(text.len());
    let mut rest_start = 0;
    for link in parse_links(text).into_iter().filter(|link| link.syntax == LinkSyntax::Wiki) {
        if let Some(replacement) = replace(&link) {
            result.push_str(&text[rest_start..link.range.start]);
            result.push_str(&replacement);
            rest_start = link.range.end;
        }
    }
    result.push_str(&text[rest_start..]);
    result
}

/// Finds every wikilink, Markdown link, and bare web address in `body`, in order.
pub fn parse_links(body: &str) -> Vec<Link> {
    let (mut excluded, mut links) = scan_markdown(body);
    let wikilinks = scan_wikilinks(body, &excluded);
    excluded.extend(wikilinks.iter().map(|link| link.range.clone()));
    links.extend(wikilinks);
    links.extend(scan_bare_links(body, &excluded));
    links.sort_by_key(|link| link.range.start);
    links
}

/// Returns the Markdown links, and the byte ranges where no other kind of link is recognized.
fn scan_markdown(body: &str) -> (Vec<Range<usize>>, Vec<Link>) {
    let mut excluded = Vec::new();
    let mut links = Vec::new();
    let mut open_link: Option<(Range<usize>, String, String)> = None;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Code(_) => excluded.push(range),
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock) => excluded.push(range),
            Event::Start(Tag::Link { dest_url, .. }) => {
                open_link = Some((range, dest_url.into_string(), String::new()));
            }
            Event::Text(text) => {
                if let Some((_, _, label)) = &mut open_link {
                    label.push_str(&text);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((range, target, label)) = open_link.take() {
                    let label = label.trim();
                    links.push(Link {
                        label: (!label.is_empty()).then(|| label.to_owned()),
                        note: parse_note_uri(&target),
                        range,
                        syntax: LinkSyntax::Markdown,
                        target,
                    });
                }
            }
            _ => {}
        }
    }
    excluded.extend(links.iter().map(|link| link.range.clone()));
    (excluded, links)
}

fn scan_wikilinks(body: &str, excluded: &[Range<usize>]) -> Vec<Link> {
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(offset) = body[from..].find("[[") {
        let start = from + offset;
        from = start + 1;
        if excluded.iter().any(|range| range.contains(&start)) || is_escaped(body, start) {
            continue;
        }
        let inner_start = start + 2;
        let Some(length) = find_unescaped(&body[inner_start..], "]]") else {
            break;
        };
        let inner = &body[inner_start..inner_start + length];
        if inner.contains(['\n', '\r']) {
            continue;
        }
        let (target, label) = match find_unescaped(inner, "|") {
            Some(i) => (unescape(inner[..i].trim()), Some(unescape(inner[i + 1..].trim()))),
            None => (unescape(inner.trim()), None),
        };
        if target.is_empty() {
            continue;
        }
        let end = inner_start + length + 2;
        links.push(Link {
            label: label.filter(|label| !label.is_empty()),
            note: target.parse().ok(),
            range: start..end,
            syntax: LinkSyntax::Wiki,
            target,
        });
        from = end;
    }
    links
}

/// Finds web addresses written as plain text, following GitHub Flavored Markdown's rules for where
/// one ends.
fn scan_bare_links(body: &str, excluded: &[Range<usize>]) -> Vec<Link> {
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(start) = ["http://", "https://"]
        .iter()
        .filter_map(|prefix| body[from..].find(prefix).map(|offset| from + offset))
        .min()
    {
        let length = body[start..].find(|c: char| c.is_whitespace() || c == '<');
        let mut end = start + length.unwrap_or(body.len() - start);
        from = end.max(start + 1);
        let follows_word = body[..start].chars().next_back().is_some_and(char::is_alphanumeric);
        if follows_word || excluded.iter().any(|range| range.contains(&start)) {
            continue;
        }
        loop {
            let url = &body[start..end];
            let trim = match url.chars().next_back() {
                Some('?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"') => true,
                Some(')') => url.matches(')').count() > url.matches('(').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            end -= 1;
        }
        let url = &body[start..end];
        if url.split_once("://").is_some_and(|(_, rest)| !rest.is_empty()) {
            links.push(Link {
                label: None,
                note: None,
                range: start..end,
                syntax: LinkSyntax::Bare,
                target: url.to_owned(),
            });
        }
    }
    links
}

fn is_escaped(body: &str, index: usize) -> bool {
    let backslashes = body[..index].bytes().rev().take_while(|&b| b == b'\\').count();
    backslashes % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(body: &str) -> Vec<String> {
        parse_links(body).into_iter().map(|link| link.target).collect()
    }

    #[test]
    fn parses_wikilinks() {
        let id = NoteId::generate();
        let body = format!("See [[{id}|the plan]] and [[ other ]].");
        let links = parse_links(&body);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].note, Some(id));
        assert_eq!(links[0].label.as_deref(), Some("the plan"));
        assert_eq!(&body[links[0].range.clone()], format!("[[{id}|the plan]]"));
        assert_eq!(links[1].target, "other");
        assert_eq!(links[1].note, None);
        assert_eq!(links[1].label, None);
    }

    #[test]
    fn parses_escapes() {
        let labels = |body: &str| -> Vec<Option<String>> {
            parse_links(body).into_iter().map(|link| link.label).collect()
        };
        assert_eq!(
            labels(r"[[a|x\]]] [[b|C:\temp]] [[c|end\\]]"),
            [Some("x]".to_owned()), Some(r"C:\temp".to_owned()), Some(r"end\".to_owned()),]
        );
        let body = r"[[a\|b|c\|d]]";
        let link = &parse_links(body)[0];
        assert_eq!((link.target.as_str(), link.label.as_deref()), ("a|b", Some("c|d")));
        assert_eq!(label_separator(body, link), Some(6));
        assert_eq!(wikilink_escapes(body, link), [3, 8]);
    }

    #[test]
    fn handles_multibyte_text() {
        let body = "Café ☕ [[naïve]] ✓";
        let links = parse_links(body);
        assert_eq!(&body[links[0].range.clone()], "[[naïve]]");
    }

    #[test]
    fn skips_code_and_escapes() {
        let body = "`[[a]]`\n\n```\n[[b]]\n```\n\n    [[c]]\n\n\\[[d]] \\\\[[e]]";
        assert_eq!(targets(body), ["e"]);
    }

    #[test]
    fn skips_broken_syntax() {
        assert!(targets("[[]] [[ | x]] [[a\nb]] [[open").is_empty());
    }

    #[test]
    fn ends_at_first_closing_brackets() {
        assert_eq!(targets("[[a|b]]c]]"), ["a"]);
    }

    #[test]
    fn parses_markdown_links() {
        let id = NoteId::generate();
        let body =
            format!("A [the *plan*](commonplace:note/{id}?x) and [web](https://example.com).");
        let links = parse_links(&body);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].syntax, LinkSyntax::Markdown);
        assert_eq!(links[0].note, Some(id));
        assert_eq!(links[0].label.as_deref(), Some("the plan"));
        assert_eq!(links[0].external_uri(), None);
        assert_eq!(links[1].note, None);
        assert_eq!(links[1].external_uri(), Some("https://example.com"));
    }

    #[test]
    fn parses_bare_links() {
        let body = "See https://example.com/a_(b)?q=1. Or (https://example.com/x), \
            <https://example.com/auto>, [x](https://example.com/md), `https://code.example`, \
            nothttps://example.com, and https:// alone.";
        let links = parse_links(body);
        let found: Vec<_> = links.iter().map(|link| (link.syntax, link.target.as_str())).collect();
        assert_eq!(
            found,
            [
                (LinkSyntax::Bare, "https://example.com/a_(b)?q=1"),
                (LinkSyntax::Bare, "https://example.com/x"),
                (LinkSyntax::Markdown, "https://example.com/auto"),
                (LinkSyntax::Markdown, "https://example.com/md"),
            ]
        );
        assert_eq!(&body[links[0].range.clone()], "https://example.com/a_(b)?q=1");
    }

    #[test]
    fn finds_external_uris() {
        let external = |body: &str| parse_links(body)[0].external_uri().map(str::to_owned);
        assert_eq!(external("[m](mailto:a@example.com)").as_deref(), Some("mailto:a@example.com"));
        assert_eq!(external("[r](notes/plan.md)"), None);
        assert_eq!(external("[bad](commonplace:note/nope)"), None);
        assert_eq!(external("[[https://example.com]]"), None);
    }

    #[test]
    fn parses_uris() {
        let id = NoteId::generate();
        assert_eq!(parse_note_uri(&note_uri(id)), Some(id));
        assert_eq!(parse_note_uri(&format!("WEB+Commonplace:note/{id}#top")), Some(id));
        assert_eq!(parse_note_uri(&format!("commonplace:memo/{id}")), None);
        assert_eq!(parse_note_uri("commonplace:note/nope"), None);
        assert_eq!(parse_note_uri("ć"), None);
    }

    #[test]
    fn formats_wikilinks() {
        let id = NoteId::generate();
        assert_eq!(wikilink(id, Some(" a|b]]c\\ ")), format!(r"[[{id}|a\|b\]\]c\\]]"));
        assert_eq!(
            parse_links(&wikilink(id, Some(" a|b]]c\\ ")))[0].label.as_deref(),
            Some(r"a|b]]c\")
        );
        assert_eq!(wikilink(id, None), format!("[[{id}]]"));
        assert_eq!(parse_links(&wikilink(id, Some("x]]y")))[0].note, Some(id));
        let bracketed = wikilink(id, Some("Read [this]"));
        assert_eq!(parse_links(&bracketed)[0].label.as_deref(), Some("Read [this]"));
        assert_eq!(parse_links(&bracketed)[0].range.end, bracketed.len());
    }

    /// Applies `updates` to `text`, as an editor would.
    fn apply(text: &str, updates: Vec<(Range<usize>, String)>) -> String {
        let mut text = text.to_owned();
        for (range, replacement) in updates.into_iter().rev() {
            text.replace_range(range, &replacement);
        }
        text
    }

    #[test]
    fn tracks_shown_titles() {
        let (a, b, empty) = (NoteId::generate(), NoteId::generate(), NoteId::generate());
        let mut titles = HashMap::from([(a, "Blah".to_owned()), (b, "Read [this]".to_owned())]);
        let body = format!("[[{a}]] [[{a}|custom]] [[{b}]] [[{empty}]]");
        let mut shown = ShownTitles::default();
        let text = shown.expand(&body, |id| titles.get(&id).cloned());
        assert_eq!(text, format!("[[{a}|Blah]] [[{a}|custom]] [[{b}|Read [this\\]]] [[{empty}]]"));
        assert_eq!(shown.collapse(&text, |id| titles.get(&id).cloned()), body);

        titles.insert(a, "Yadda".to_owned());
        titles.insert(empty, "Now titled".to_owned());
        let lookup = |id| titles.get(&id).cloned();
        let updated = apply(&text, shown.updates(&text, lookup));
        assert_eq!(
            updated,
            format!("[[{a}|Yadda]] [[{a}|custom]] [[{b}|Read [this\\]]] [[{empty}|Now titled]]")
        );
        assert!(shown.updates(&updated, lookup).is_empty());
        assert_eq!(shown.collapse(&updated, lookup), body);
        // Undoing the update brings back the old title, which still isn't a custom label.
        assert_eq!(shown.collapse(&text, lookup), body);

        titles.remove(&a);
        let lookup = |id| titles.get(&id).cloned();
        let untitled = apply(&updated, shown.updates(&updated, lookup));
        assert!(untitled.starts_with(&format!("[[{a}]] [[{a}|custom]]")));
    }

    #[test]
    fn expands_and_collapses_links() {
        let (a, b, gone) = (NoteId::generate(), NoteId::generate(), NoteId::generate());
        let title = |id| (id == a).then(|| "Blah | blah".to_owned());
        let body = format!("[[{a}]] [[{a}|custom]] [[{gone}]] [[other]] `[[{a}]]`");
        let expanded = expand_links(&body, title);
        assert_eq!(
            expanded,
            format!(r"[[{a}|Blah \| blah]] [[{a}|custom]] [[{gone}]] [[other]] `[[{a}]]`")
        );
        let shown = link_label("Blah | blah");
        assert_eq!(collapse_links(&expanded, |id, label| id == a && label == shown), body);
        let typed = format!(r"[[{a}|Blah \| blah]] and [[{b}|Blah \| blah]]");
        assert_eq!(
            collapse_links(&typed, |id, label| id == a && label == shown),
            format!(r"[[{a}]] and [[{b}|Blah \| blah]]")
        );
    }

    #[test]
    fn writes_links_as_plain_text() {
        let (a, gone) = (NoteId::generate(), NoteId::generate());
        let title = |id| (id == a).then(|| "Title".to_owned());
        let text = format!("See [[{a}]], [[{a}|this]], [[{gone}]], and [[other]]");
        assert_eq!(plain_links(&text, title), format!("See Title, this, {gone}, and other"));
    }
}
