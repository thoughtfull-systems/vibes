// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

/// Derives a note's label from its body, as the library format spec defines it.
pub fn label(body: &str) -> Option<String> {
    let line = body.lines().find(|line| !line.trim().is_empty())?.trim_start();
    let line = strip_heading(line)
        .or_else(|| strip_list_item(line))
        .or_else(|| line.strip_prefix('>'))
        .unwrap_or(line)
        .trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// Replaces the text of `body`'s label with `label`, as one line, keeping the marker in front of
/// it, such as a heading's hashes, and every other line.
pub fn relabel(body: &str, label: &str) -> String {
    let label = label.replace(['\n', '\r'], " ");
    let label = label.trim();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if !content.trim().is_empty() {
            let trimmed = content.trim_start();
            let rest = strip_heading(trimmed)
                .or_else(|| strip_list_item(trimmed))
                .or_else(|| trimmed.strip_prefix('>'))
                .unwrap_or(trimmed)
                .trim_start();
            let start = offset + content.len() - rest.len();
            return format!("{}{label}{}", &body[..start], &body[offset + content.len()..]);
        }
        offset += line.len();
    }
    format!("{label}{body}")
}

/// Derives an entry's label, which skips the headings that its journal's template wrote, so
/// entries in one journal don't all share the label of its first question. An entry with nothing
/// but headings takes its first heading.
pub(crate) fn entry_label(body: &str) -> Option<String> {
    body.lines()
        .find(|line| !line.trim().is_empty() && strip_heading(line.trim_start()).is_none())
        .and_then(label)
        .or_else(|| label(body))
}

fn strip_heading(line: &str) -> Option<&str> {
    line.trim_start_matches('#').strip_prefix(' ').filter(|_| line.starts_with('#'))
}

fn strip_list_item(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(['-', '*', '+'])?.strip_prefix(' ')?;
    let trimmed = rest.trim_start();
    let checkbox = ["[ ]", "[x]", "[X]"].iter().find_map(|box_| trimmed.strip_prefix(box_));
    Some(checkbox.unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_labels() {
        for (body, expected) in [
            ("", None),
            ("  \n\t\n", None),
            ("\n\n  Lawn mowing: $20  \nmore", Some("Lawn mowing: $20")),
            ("## Heading", Some("Heading")),
            ("#hashtag", Some("#hashtag")),
            ("- item", Some("item")),
            ("* [ ] item", Some("item")),
            ("+ [x] done", Some("done")),
            ("> quoted", Some("quoted")),
            ("-not a list", Some("-not a list")),
            ("# ", None),
        ] {
            assert_eq!(label(body).as_deref(), expected, "{body:?}");
        }
    }

    #[test]
    fn relabels() {
        for (body, new, expected) in [
            ("", "New", "New"),
            ("\n\n", "New", "New\n\n"),
            ("Old\nmore", "New", "New\nmore"),
            ("\n  Old  \r\nmore", "New", "\n  New\r\nmore"),
            ("## Old\n", " New ", "## New\n"),
            ("- [ ] Old", "New", "- [ ] New"),
            ("> Old", "New\nline", "> New line"),
            ("-not a list", "New", "New"),
        ] {
            assert_eq!(relabel(body, new), expected, "{body:?}");
        }
    }

    #[test]
    fn skips_template_headings_in_entry_labels() {
        let body = "## What have you been working on?\n\n- Shipped it\n\n## Blockers\n";
        assert_eq!(entry_label(body).as_deref(), Some("Shipped it"));
        assert_eq!(entry_label("## Blockers\n\n").as_deref(), Some("Blockers"));
        assert_eq!(entry_label("").as_deref(), None);
    }
}
