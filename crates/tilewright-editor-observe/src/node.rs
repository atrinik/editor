// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The one document shape every report is built as, and its two printed
//! forms. Member names are always this crate's own text; world text only
//! ever appears as a string value, clipped when it is built and escaped
//! when it is printed.
use crate::limits::MAXIMUM_TEXT_CHARACTERS;
use std::fmt::Write as _;
use tilewright_editor_document::{terminal_field, terminal_safe, unprintable};

/// A value of a report.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Integer(i64),
    /// A JSON number written exactly as it is given (a World field value).
    Number(f64),
    Text(String),
    List(Vec<Node>),
    Map(Map),
}

impl Node {
    /// A count, saturated at `i64::MAX`.
    pub fn count(count: usize) -> Self {
        Self::Integer(i64::try_from(count).unwrap_or(i64::MAX))
    }

    fn scalar(&self) -> bool {
        !matches!(self, Self::List(_) | Self::Map(_))
    }
}

impl From<bool> for Node {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for Node {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<i32> for Node {
    fn from(value: i32) -> Self {
        Self::Integer(value.into())
    }
}
impl From<u32> for Node {
    fn from(value: u32) -> Self {
        Self::Integer(value.into())
    }
}
impl From<u16> for Node {
    fn from(value: u16) -> Self {
        Self::Integer(value.into())
    }
}
impl From<i16> for Node {
    fn from(value: i16) -> Self {
        Self::Integer(value.into())
    }
}
impl From<[i64; 2]> for Node {
    fn from(value: [i64; 2]) -> Self {
        Self::List(value.into_iter().map(Self::Integer).collect())
    }
}
impl From<Map> for Node {
    fn from(value: Map) -> Self {
        Self::Map(value)
    }
}

/// `text` cut to [`MAXIMUM_TEXT_CHARACTERS`] characters, and whether it was
/// cut.
pub fn clip(text: &str) -> (String, bool) {
    match text.char_indices().nth(MAXIMUM_TEXT_CHARACTERS) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// An object with members in a fixed order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Node)>);

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds member `key`.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl Into<Node>) -> Self {
        self.0.push((key.to_owned(), value.into()));
        self
    }

    /// Adds a count.
    #[must_use]
    pub fn count(self, key: &'static str, count: usize) -> Self {
        self.with(key, Node::count(count))
    }

    /// Adds text, clipped to [`MAXIMUM_TEXT_CHARACTERS`]; a clipped value is
    /// followed by the member `<key>_clipped: true`.
    #[must_use]
    pub fn text(mut self, key: &'static str, text: &str) -> Self {
        let (value, clipped) = clip(text);
        self.0.push((key.to_owned(), Node::Text(value)));
        if clipped {
            self.0.push((format!("{key}_clipped"), Node::Bool(true)));
        }
        self
    }

    /// Adds text, or null.
    #[must_use]
    pub fn optional_text(self, key: &'static str, text: Option<&str>) -> Self {
        match text {
            Some(text) => self.text(key, text),
            None => self.with(key, Node::Null),
        }
    }

    /// Adds a list of at most `maximum` items and the member `<key>_more`
    /// with how many were left out. `item` runs only for the items listed;
    /// the rest are only counted, so callers build nodes in `item`.
    #[must_use]
    pub fn bounded<T>(
        mut self,
        key: &'static str,
        items: impl IntoIterator<Item = T>,
        maximum: usize,
        mut item: impl FnMut(T) -> Node,
    ) -> Self {
        let mut items = items.into_iter();
        let listed: Vec<Node> = items.by_ref().take(maximum).map(&mut item).collect();
        let more = items.count();
        self.0.push((key.to_owned(), Node::List(listed)));
        self.0.push((format!("{key}_more"), Node::count(more)));
        self
    }

    /// Adds a list of identifiers, which World bounds itself.
    #[must_use]
    pub fn ids<'a>(self, key: &'static str, ids: impl IntoIterator<Item = &'a str>) -> Self {
        self.with(
            key,
            Node::List(ids.into_iter().map(|id| Node::Text(clip(id).0)).collect()),
        )
    }
}

/// Whether the printers write `character` as an escape rather than as
/// itself. This is the one definition of the escaped set:
///
/// - everything [`unprintable`] matches: every control character (C0, DEL
///   and C1), U+2028 and U+2029, and the bidirectional formatting
///   characters U+061C, U+200E, U+200F, U+202A to U+202E and U+2066 to
///   U+2069;
/// - and invisible or format characters a reader would not see: U+00AD
///   (soft hyphen), U+180E, U+200B to U+200F, U+2060 to U+2064, U+FEFF,
///   U+FFF9 to U+FFFB and the tag characters U+E0000 to U+E007F.
///
/// Variation selectors are not escaped: they change how a character is
/// drawn, not what text says.
pub fn escaped(character: char) -> bool {
    unprintable(character)
        || matches!(
            character,
            '\u{ad}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2060}'..='\u{2064}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
                | '\u{e0000}'..='\u{e007f}'
        )
}

/// The JSON form: one document, two-space indented, ending with a newline.
/// Every [`escaped`] character is written as a `\u` escape (a UTF-16
/// surrogate pair above U+FFFF), so the document is safe to print as it is.
pub fn to_json(root: &Map) -> String {
    let mut output = String::new();
    map_json(root, 0, &mut output);
    output.push('\n');
    output
}

/// Writes `text` as a JSON string.
pub fn json_string(text: &str, output: &mut String) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            character if escaped(character) => {
                let mut units = [0; 2];
                for unit in character.encode_utf16(&mut units) {
                    let _ = write!(output, "\\u{unit:04x}");
                }
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

/// `text` as one field of the text form: every [`escaped`] character
/// written as its `\u{...}` escape, then [`terminal_field`] over the rest.
pub fn text_field(text: &str) -> String {
    let marked: String = text.chars().fold(
        String::with_capacity(text.len()),
        |mut marked, character| {
            if escaped(character) && !unprintable(character) {
                marked.extend(character.escape_unicode());
            } else {
                marked.push(character);
            }
            marked
        },
    );
    terminal_field(&marked)
}

fn indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn map_json(map: &Map, depth: usize, output: &mut String) {
    if map.0.is_empty() {
        output.push_str("{}");
        return;
    }
    output.push_str("{\n");
    for (index, (key, value)) in map.0.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        indent(depth + 1, output);
        json_string(key, output);
        output.push_str(": ");
        node_json(value, depth + 1, output);
    }
    output.push('\n');
    indent(depth, output);
    output.push('}');
}

fn node_json(node: &Node, depth: usize, output: &mut String) {
    match node {
        Node::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Node::Integer(value) => {
            let _ = write!(output, "{value}");
        }
        // Rust's shortest round-trip form of a finite number is JSON.
        Node::Number(value) if value.is_finite() => {
            let _ = write!(output, "{value:?}");
        }
        Node::Null | Node::Number(_) => output.push_str("null"),
        Node::Text(text) => json_string(text, output),
        Node::Map(map) => map_json(map, depth, output),
        Node::List(items) if items.iter().all(Node::scalar) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                node_json(item, depth, output);
            }
            output.push(']');
        }
        Node::List(items) => {
            output.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(",\n");
                }
                indent(depth + 1, output);
                node_json(item, depth + 1, output);
            }
            output.push('\n');
            indent(depth, output);
            output.push(']');
        }
    }
}

/// The text form: one `name: value` line per member, nested members
/// indented, list items marked `- `. Every string value is one escaped
/// field ([`text_field`]), so world text cannot add, forge or reorder
/// lines, and the whole is passed through [`terminal_safe`] as a backstop.
pub fn to_text(root: &Map) -> String {
    let mut output = String::new();
    map_text(root, 0, &mut output);
    terminal_safe(&output)
}

fn scalar_text(node: &Node) -> String {
    match node {
        Node::Null => "none".into(),
        Node::Bool(value) => value.to_string(),
        Node::Integer(value) => value.to_string(),
        Node::Number(value) => format!("{value:?}"),
        Node::Text(text) if text.is_empty() => "\"\"".into(),
        Node::Text(text) => text_field(text),
        Node::List(items) => format!(
            "[{}]",
            items.iter().map(scalar_text).collect::<Vec<_>>().join(", ")
        ),
        Node::Map(_) => "{...}".into(),
    }
}

fn map_text(map: &Map, depth: usize, output: &mut String) {
    for (key, value) in &map.0 {
        indent(depth, output);
        output.push_str(key);
        match value {
            Node::Map(inner) if inner.0.is_empty() => output.push_str(": (none)\n"),
            Node::Map(inner) => {
                output.push_str(":\n");
                map_text(inner, depth + 1, output);
            }
            Node::List(items) if items.is_empty() => output.push_str(": (none)\n"),
            Node::List(items) if items.iter().all(Node::scalar) => {
                let _ = writeln!(output, ": {}", scalar_text(value));
            }
            Node::List(items) => {
                output.push_str(":\n");
                for item in items {
                    let mut lines = String::new();
                    match item {
                        Node::Map(inner) if !inner.0.is_empty() => {
                            map_text(inner, depth + 2, &mut lines);
                        }
                        other => {
                            indent(depth + 2, &mut lines);
                            lines.push_str(&scalar_text(other));
                            lines.push('\n');
                        }
                    }
                    // The first line's indent becomes the item marker.
                    let start = 2 * (depth + 1);
                    lines.replace_range(start..start + 2, "- ");
                    output.push_str(&lines);
                }
            }
            scalar => {
                let _ = writeln!(output, ": {}", scalar_text(scalar));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Map, Node, clip, to_json, to_text};
    use crate::limits::MAXIMUM_TEXT_CHARACTERS;

    #[test]
    fn both_forms_escape_world_text_and_keep_its_shape() {
        let hostile = "a\"b\\c\n\u{1b}[2J\u{7f}\u{85}\u{2028}\u{202e}d";
        let map = Map::new()
            .text("name", hostile)
            .with("tile", [-2_i64, 3])
            .with(
                "items",
                Node::List(vec![
                    Map::new().text("id", "x").count("n", 2).into(),
                    Map::new().text("id", "y").into(),
                ]),
            )
            .with("nothing", Node::List(Vec::new()));
        let json = to_json(&map);
        assert!(
            json.contains(r#""name": "a\"b\\c\u000a\u001b[2J\u007f\u0085\u2028\u202ed""#),
            "{json}"
        );
        assert!(json.contains(r#""tile": [-2, 3]"#), "{json}");
        let text = to_text(&map);
        assert_eq!(
            text,
            "name: a\"b\\c\\u{a}\\u{1b}[2J\\u{7f}\\u{85}\\u{2028}\\u{202e}d\ntile: [-2, 3]\nitems:\n  - id: x\n    n: 2\n  - id: y\nnothing: (none)\n"
        );
    }

    #[test]
    fn invisible_characters_are_escaped_and_astral_ones_as_surrogate_pairs() {
        // A tag character, BOM, zero-width space, soft hyphen, word joiner
        // and an annotation anchor are escaped; a variation selector is not.
        let text = "a\u{e0041}\u{feff}\u{200b}\u{ad}\u{2060}\u{fff9}\u{180e}\u{fe0f}b";
        let json = to_json(&Map::new().text("v", text));
        assert!(
            json.contains(r#""v": "a\udb40\udc41\ufeff\u200b\u00ad\u2060\ufff9\u180e"#),
            "{json}"
        );
        assert!(json.contains("\u{fe0f}b\""), "{json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["v"], text);
        let shown = to_text(&Map::new().text("v", text));
        assert_eq!(
            shown,
            "v: a\\u{e0041}\\u{feff}\\u{200b}\\u{ad}\\u{2060}\\u{fff9}\\u{180e}\u{fe0f}b\n"
        );
        assert!(!super::escaped('\u{fe0f}'));
        assert!(super::escaped('\u{e007f}') && !super::escaped('\u{e0080}'));
    }

    #[test]
    fn long_text_is_clipped_and_flagged() {
        let long = "é".repeat(MAXIMUM_TEXT_CHARACTERS + 5);
        let (value, clipped) = clip(&long);
        assert!(clipped);
        assert_eq!(value.chars().count(), MAXIMUM_TEXT_CHARACTERS);
        let json = to_json(&Map::new().text("label", &long));
        assert!(json.contains("\"label_clipped\": true"), "{json}");
        assert!(!to_json(&Map::new().text("label", "short")).contains("clipped"));
    }

    #[test]
    fn bounded_lists_count_what_they_leave_out() {
        let map = Map::new().bounded("values", 0..10, 3, |value: i64| value.into());
        assert_eq!(
            to_json(&map),
            "{\n  \"values\": [0, 1, 2],\n  \"values_more\": 7\n}\n"
        );
    }
}
