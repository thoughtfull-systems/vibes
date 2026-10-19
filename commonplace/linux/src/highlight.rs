// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

use std::collections::HashMap;
use std::ops::Range;

use adw::prelude::*;
use commonplace_core::code_blocks;
use sourceview5::prelude::*;

/// Properties of GtkSourceView's highlighting tags that the highlighter copies, each with the
/// property that says whether a tag sets it.
const PROPERTIES: [(&str, &str); 6] = [
    ("background-rgba", "background-set"),
    ("foreground-rgba", "foreground-set"),
    ("strikethrough", "strikethrough-set"),
    ("style", "style-set"),
    ("underline", "underline-set"),
    ("weight", "weight-set"),
];

/// Character ranges within a block's code, each with the key of the tag that colors it.
type Highlights = Vec<(Range<i32>, String)>;

/// A block's style scheme, language, and code.
type CacheKey = (String, String, String);

/// Colors the code in fenced code blocks that name a language, using GtkSourceView's definitions
/// for that language and the buffer's style scheme.
#[derive(Default)]
pub struct Highlighter {
    /// Highlights of the blocks colored last time.
    cache: HashMap<CacheKey, Highlights>,
    /// A buffer outside any view that GtkSourceView highlights each block in.
    scratch: Option<sourceview5::Buffer>,
    /// Tags in the note's buffer, by a key made of the properties they set.
    tags: HashMap<String, gtk::TextTag>,
}

impl Highlighter {
    /// Removes the colors from last time, and colors each code block in `body`, the text of
    /// `buffer`. `chars` converts a byte offset in `body` to a character offset in `buffer`.
    pub fn apply(
        &mut self,
        buffer: &sourceview5::Buffer,
        body: &str,
        chars: impl Fn(usize) -> usize,
    ) {
        let (start, end) = buffer.bounds();
        for tag in self.tags.values() {
            buffer.remove_tag(tag, &start, &end);
        }
        let scheme = buffer.style_scheme();
        let scheme_id = scheme.as_ref().map(|scheme| scheme.id().to_string()).unwrap_or_default();
        let mut used = HashMap::new();
        for block in code_blocks(body) {
            let Some(language) = language(&block.language) else {
                continue;
            };
            let code = body[block.content.clone()].to_owned();
            let key = (scheme_id.clone(), language.id().to_string(), code);
            let highlights = match self.cache.remove(&key) {
                Some(highlights) => highlights,
                None => self.highlight(&language, scheme.as_ref(), &key.2, buffer),
            };
            let offset = chars(block.content.start) as i32;
            for (range, tag_key) in &highlights {
                if let Some(tag) = self.tags.get(tag_key) {
                    let start = buffer.iter_at_offset(offset + range.start);
                    let end = buffer.iter_at_offset(offset + range.end);
                    buffer.apply_tag(tag, &start, &end);
                }
            }
            used.insert(key, highlights);
        }
        self.cache = used;
    }

    /// Highlights `code` in the scratch buffer, and returns its highlights, adding a tag to
    /// `buffer` for each new combination of properties.
    fn highlight(
        &mut self,
        language: &sourceview5::Language,
        scheme: Option<&sourceview5::StyleScheme>,
        code: &str,
        buffer: &sourceview5::Buffer,
    ) -> Highlights {
        let scratch = self.scratch.get_or_insert_with(|| sourceview5::Buffer::new(None));
        scratch.set_style_scheme(scheme);
        scratch.set_language(Some(language));
        scratch.set_text(code);
        let (start, end) = scratch.bounds();
        scratch.ensure_highlight(&start, &end);
        let mut highlights = Vec::new();
        let mut position = start;
        while !position.is_end() {
            let mut next = position;
            next.forward_to_tag_toggle(None::<&gtk::TextTag>);
            for tag in position.tags() {
                let key = tag_key(&tag);
                if key.is_empty() {
                    continue;
                }
                if !self.tags.contains_key(&key) {
                    let copy = gtk::TextTag::new(None);
                    for (property, set) in PROPERTIES {
                        if tag.property::<bool>(set) {
                            copy.set_property_from_value(property, &tag.property_value(property));
                        }
                    }
                    buffer.tag_table().add(&copy);
                    self.tags.insert(key.clone(), copy);
                }
                highlights.push((position.offset()..next.offset(), key));
            }
            position = next;
        }
        highlights
    }
}

/// Describes the properties that `tag` sets, or returns an empty string if it sets none of them.
fn tag_key(tag: &gtk::TextTag) -> String {
    PROPERTIES
        .iter()
        .filter(|(_, set)| tag.property::<bool>(set))
        .map(|(property, _)| format!("{property}={:?};", tag.property_value(property)))
        .collect()
}

/// Common names for languages in code fences that GtkSourceView doesn't know by its identifiers,
/// names, or file extensions, with the identifier it uses.
const ALIASES: [(&str, &str); 6] = [
    ("bash", "sh"),
    ("golang", "go"),
    ("make", "makefile"),
    ("patch", "diff"),
    ("shell", "sh"),
    ("zsh", "sh"),
];

/// Finds GtkSourceView's definition for a fenced code block's language, by its identifier, its
/// name, or a file extension that it uses.
fn language(name: &str) -> Option<sourceview5::Language> {
    let manager = sourceview5::LanguageManager::default();
    let name = ALIASES.iter().find(|(alias, _)| *alias == name).map_or(name, |(_, id)| id);
    manager
        .language(name)
        .or_else(|| {
            manager.language_ids().iter().find_map(|id| {
                manager.language(id).filter(|language| language.name().to_lowercase() == name)
            })
        })
        .or_else(|| manager.guess_language(Some(&format!("code.{name}")), None))
}
