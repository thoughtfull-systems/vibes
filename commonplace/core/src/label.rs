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
}
