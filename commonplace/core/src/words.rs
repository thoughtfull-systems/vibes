// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use crate::parse_links;

/// Counts the words a reader sees in a note's body. A link counts as the words of its label, or as
/// one word if it has none, and Markdown syntax with no letters or digits doesn't count.
pub fn word_count(body: &str) -> usize {
    let mut count = 0;
    let mut rest_start = 0;
    for link in parse_links(body) {
        count += words(&body[rest_start..link.range.start]);
        count += link.label.as_deref().map_or(0, words).max(1);
        rest_start = link.range.end;
    }
    count + words(&body[rest_start..])
}

fn words(text: &str) -> usize {
    text.split_whitespace().filter(|word| word.chars().any(char::is_alphanumeric)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoteId;

    #[test]
    fn counts_words() {
        let id = NoteId::generate();
        for (body, expected) in [
            (String::new(), 0),
            ("  \n ".to_owned(), 0),
            ("# Lawn mowing\n\n- $20 per mow — edging extra".to_owned(), 7),
            (format!("See [[{id}|the lawn deal]] and [[{id}]]."), 6),
            ("Read [the plan](https://example.com) at https://example.com/x".to_owned(), 5),
            ("Café naïve 2026".to_owned(), 3),
        ] {
            assert_eq!(word_count(&body), expected, "{body:?}");
        }
    }
}
