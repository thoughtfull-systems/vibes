// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

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
    match label {
        Some(label) => {
            let label = label.replace("]]", " ").replace(['|', '\n', '\r'], " ");
            format!("[[{id}|{}]]", label.trim())
        }
        None => format!("[[{id}]]"),
    }
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
        let Some(length) = body[inner_start..].find("]]") else {
            break;
        };
        let inner = &body[inner_start..inner_start + length];
        if inner.contains(['\n', '\r']) {
            continue;
        }
        let (target, label) = match inner.split_once('|') {
            Some((target, label)) => (target.trim(), Some(label.trim())),
            None => (inner.trim(), None),
        };
        if target.is_empty() {
            continue;
        }
        let end = inner_start + length + 2;
        links.push(Link {
            label: label.filter(|label| !label.is_empty()).map(str::to_owned),
            note: target.parse().ok(),
            range: start..end,
            syntax: LinkSyntax::Wiki,
            target: target.to_owned(),
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
        assert_eq!(wikilink(id, Some(" a|b]]c ")), format!("[[{id}|a b c]]"));
        assert_eq!(wikilink(id, None), format!("[[{id}]]"));
        assert_eq!(parse_links(&wikilink(id, Some("x]]y")))[0].note, Some(id));
    }
}
