// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Exact UTF-8 drafts. Geometry and visual navigation belong to the renderer.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use crate::{inspector::MAX_INPUT_BYTES, tools::ToolError};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextAffinity {
    Before,
    #[default]
    After,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextCursor {
    pub byte: usize,
    pub affinity: TextAffinity,
}

impl TextCursor {
    pub fn new(byte: usize) -> Self {
        Self {
            byte,
            affinity: TextAffinity::After,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextSelection {
    pub anchor: TextCursor,
    pub focus: TextCursor,
}

impl TextSelection {
    pub fn caret(cursor: TextCursor) -> Self {
        Self {
            anchor: cursor,
            focus: cursor,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.anchor.byte.min(self.focus.byte)..self.anchor.byte.max(self.focus.byte)
    }
}

/// Both identities must match the frame used for a pointer or visual movement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextRevision {
    pub draft: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionSnapshot {
    pub text: String,
    /// Original committed selection; never updated by subsequent preedits.
    pub replacement: Range<usize>,
    /// SDL scalar offsets converted to UTF-8 bytes; None means unset.
    pub selection: Option<Range<usize>>,
    /// Preedit's exact byte range in the virtual display string.
    pub display_range: Range<usize>,
}

/// One stored line break or control character (a CR LF pair counts as one)
/// shown as a visible marker. Display only: the committed text keeps it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayMarker {
    /// Its bytes in the text before marking: the committed text with any
    /// preedit spliced in.
    pub source: Range<usize>,
    /// The whole marker unit in `display_text`, including any zero-width
    /// guard that keeps a neighbour from joining the marker's grapheme.
    pub display: Range<usize>,
    /// The visible marker glyph inside `display`.
    pub glyph: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSnapshot {
    pub committed: String,
    /// Complete UTF-8 for shared shaping, never escaped or elided, except
    /// that each stored line break or control character is shown as one
    /// marker grapheme (see [`mark_stored_breaks`]) so the field stays on one
    /// line.
    pub display_text: String,
    pub selection: TextSelection,
    /// Grapheme-safe cursors in `display_text`, including the preedit splice.
    pub display_selection: TextSelection,
    pub composition: Option<CompositionSnapshot>,
    /// Markers in `display_text`, in order. Empty when nothing is marked.
    pub markers: Vec<DisplayMarker>,
    pub revision: TextRevision,
}

impl TextInputSnapshot {
    /// Map a displayed cursor back to committed text. Interior preedit positions
    /// have no committed counterpart; callers must cancel composition first.
    /// A position inside a marker unit (beside its zero-width guard) maps to
    /// the edge of the stored character on the same side of the glyph.
    pub fn committed_cursor(&self, cursor: TextCursor) -> Result<TextCursor, ToolError> {
        validate_cursor(&self.display_text, cursor)?;
        let cursor = TextCursor {
            byte: unmarked_byte(&self.markers, cursor.byte),
            ..cursor
        };
        let byte = if let Some(composition) = &self.composition {
            let display = &composition.display_range;
            if cursor.byte < display.start {
                cursor.byte
            } else if cursor.byte == display.start {
                composition.replacement.start
            } else if cursor.byte < display.end {
                return Err(ToolError("Cursor is inside transient composition"));
            } else {
                composition
                    .replacement
                    .end
                    .checked_add(cursor.byte - display.end)
                    .ok_or(ToolError("Invalid composition mapping"))?
            }
        } else {
            cursor.byte
        };
        let result = TextCursor { byte, ..cursor };
        validate_cursor(&self.committed, result)?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Composition {
    text: String,
    replacement: Range<usize>,
    selection: Option<Range<usize>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInput {
    text: String,
    selection: TextSelection,
    composition: Option<Composition>,
    revision: TextRevision,
    /// Byte budget for the committed text; new input past it is refused.
    limit: usize,
}

impl TextInput {
    pub fn new(text: String) -> Result<Self, ToolError> {
        Self::with_identity(text, 0)
    }

    pub(crate) fn with_identity(text: String, draft: u64) -> Result<Self, ToolError> {
        Self::with_limit(text, draft, MAX_INPUT_BYTES)
    }

    /// A draft whose committed text never exceeds `limit` bytes (at most
    /// [`MAX_INPUT_BYTES`]).
    pub(crate) fn with_limit(text: String, draft: u64, limit: usize) -> Result<Self, ToolError> {
        let limit = limit.min(MAX_INPUT_BYTES);
        // Existing source can contain controls or line separators. Opening it
        // must remain lossless; only newly inserted user input follows the
        // single-line policy in `validate_insert`.
        if text.len() > limit {
            return Err(ToolError("Input exceeds byte budget"));
        }
        let selection = TextSelection::caret(TextCursor::new(text.len()));
        Ok(Self {
            text,
            selection,
            composition: None,
            revision: TextRevision { draft, revision: 0 },
            limit,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn selection(&self) -> TextSelection {
        self.selection
    }
    pub fn revision(&self) -> TextRevision {
        self.revision
    }
    pub fn is_composing(&self) -> bool {
        self.composition.is_some()
    }

    pub fn snapshot(&self) -> TextInputSnapshot {
        let mut display_text = self.text.clone();
        let mut display_selection = self.selection;
        let composition = self.composition.as_ref().map(|composition| {
            display_text.replace_range(composition.replacement.clone(), &composition.text);
            let display_range = composition.replacement.start
                ..composition.replacement.start + composition.text.len();
            let selected = composition
                .selection
                .clone()
                .unwrap_or(composition.text.len()..composition.text.len());
            let focus = TextCursor::new(snap_after(
                &display_text,
                display_range.start + selected.end,
            ));
            display_selection = if selected.is_empty() {
                TextSelection::caret(focus)
            } else {
                TextSelection {
                    anchor: TextCursor::new(snap_before(
                        &display_text,
                        display_range.start + selected.start,
                    )),
                    focus,
                }
            };
            CompositionSnapshot {
                text: composition.text.clone(),
                replacement: composition.replacement.clone(),
                selection: composition.selection.clone(),
                display_range,
            }
        });
        // Preedit never holds a separator (`validate_insert`), so every marker
        // stands for a committed character.
        let (display_text, markers) = mark_stored_breaks(&display_text);
        let to_display = |cursor: TextCursor| TextCursor {
            byte: marked_byte(&markers, cursor.byte),
            ..cursor
        };
        let display_selection = TextSelection {
            anchor: to_display(display_selection.anchor),
            focus: to_display(display_selection.focus),
        };
        let composition = composition.map(|composition| CompositionSnapshot {
            display_range: marked_byte(&markers, composition.display_range.start)
                ..marked_byte(&markers, composition.display_range.end),
            ..composition
        });
        TextInputSnapshot {
            committed: self.text.clone(),
            display_text,
            selection: self.selection,
            display_selection,
            composition,
            markers,
            revision: self.revision,
        }
    }

    pub fn set_selection(
        &mut self,
        selection: TextSelection,
        presented: TextRevision,
    ) -> Result<(), ToolError> {
        self.require_revision(presented)?;
        self.require_no_composition()?;
        validate_cursor(&self.text, selection.anchor)?;
        validate_cursor(&self.text, selection.focus)?;
        if self.selection != selection {
            let revision = self.next_revision()?;
            self.selection = selection;
            self.revision = revision;
        }
        Ok(())
    }

    pub fn select_all(&mut self) -> Result<(), ToolError> {
        self.set_selection(
            TextSelection {
                anchor: TextCursor::new(0),
                focus: TextCursor::new(self.text.len()),
            },
            self.revision,
        )
    }

    /// Commit exactly once over the original composition selection, or the
    /// current ordinary selection. Every rejection leaves all state untouched.
    pub fn input(&mut self, text: &str) -> Result<(), ToolError> {
        let range = self.composition.as_ref().map_or_else(
            || self.selection.range(),
            |composition| composition.replacement.clone(),
        );
        self.replace(range, text)
    }

    pub fn backspace(&mut self) -> Result<(), ToolError> {
        self.require_no_composition()?;
        let mut range = self.selection.range();
        if range.is_empty() && range.start != 0 {
            range.start = self.text[..range.start]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(byte, _)| byte);
        }
        self.replace(range, "")
    }

    pub fn delete(&mut self) -> Result<(), ToolError> {
        self.require_no_composition()?;
        let mut range = self.selection.range();
        if range.is_empty() {
            range.end += self.text[range.end..]
                .graphemes(true)
                .next()
                .map_or(0, str::len);
        }
        self.replace(range, "")
    }

    pub fn update_preedit(&mut self, text: &str, start: i32, length: i32) -> Result<(), ToolError> {
        let replacement = self.composition.as_ref().map_or_else(
            || self.selection.range(),
            |composition| composition.replacement.clone(),
        );
        validate_insert(&self.text, &replacement, text, self.limit)?;
        // Even empty cancellation events validate their offsets first.
        let selection = scalar_selection_bytes(text, start, length)?;
        let next = if text.is_empty() {
            None
        } else {
            Some(Composition {
                text: text.into(),
                replacement,
                selection,
            })
        };
        if next != self.composition {
            let revision = self.next_revision()?;
            self.composition = next;
            self.revision = revision;
        }
        Ok(())
    }

    pub fn cancel_preedit(&mut self) -> Result<(), ToolError> {
        if self.composition.is_some() {
            let revision = self.next_revision()?;
            self.composition = None;
            self.revision = revision;
        }
        Ok(())
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), ToolError> {
        validate_insert(&self.text, &range, text, self.limit)?;
        if range.is_empty() && text.is_empty() && self.composition.is_none() {
            return Ok(());
        }
        let revision = self.next_revision()?;
        let mut next = self.text.clone();
        next.replace_range(range.clone(), text);
        // Insertion may join adjacent graphemes (marks, ZWJ, regional indicators).
        let cursor = TextCursor::new(snap_after(&next, range.start + text.len()));
        self.text = next;
        self.selection = TextSelection::caret(cursor);
        self.composition = None;
        self.revision = revision;
        Ok(())
    }

    fn next_revision(&self) -> Result<TextRevision, ToolError> {
        Ok(TextRevision {
            revision: self
                .revision
                .revision
                .checked_add(1)
                .ok_or(ToolError("Input revision exhausted"))?,
            ..self.revision
        })
    }

    fn require_revision(&self, presented: TextRevision) -> Result<(), ToolError> {
        if self.revision == presented {
            Ok(())
        } else {
            Err(ToolError("Input presentation is stale"))
        }
    }

    fn require_no_composition(&self) -> Result<(), ToolError> {
        if self.is_composing() {
            Err(ToolError("Finish or cancel composition first"))
        } else {
            Ok(())
        }
    }
}

/// SDL reports Unicode scalar counts, never UTF-8 bytes or UTF-16 units.
pub fn scalar_selection_bytes(
    text: &str,
    start: i32,
    length: i32,
) -> Result<Option<Range<usize>>, ToolError> {
    if start < -1 || length < -1 {
        return Err(ToolError("Invalid composition selection"));
    }
    if start == -1 || length == -1 {
        let count = text.chars().count();
        for known in [start, length].into_iter().filter(|value| *value >= 0) {
            if usize::try_from(known).map_err(|_| ToolError("Invalid composition selection"))?
                > count
            {
                return Err(ToolError("Composition selection is out of bounds"));
            }
        }
        return Ok(None);
    }
    let start = usize::try_from(start).map_err(|_| ToolError("Invalid composition selection"))?;
    let length = usize::try_from(length).map_err(|_| ToolError("Invalid composition selection"))?;
    let end = start
        .checked_add(length)
        .ok_or(ToolError("Invalid composition selection"))?;
    let byte_at = |index: usize| {
        text.char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .nth(index)
            .ok_or(ToolError("Composition selection is out of bounds"))
    };
    Ok(Some(byte_at(start)?..byte_at(end)?))
}

fn validate_insert(
    current: &str,
    range: &Range<usize>,
    text: &str,
    limit: usize,
) -> Result<(), ToolError> {
    if range.start > range.end {
        return Err(ToolError("Invalid replacement range"));
    }
    validate_cursor(current, TextCursor::new(range.start))?;
    validate_cursor(current, TextCursor::new(range.end))?;
    if text.len() > limit.saturating_sub(current.len() - range.len())
        || text.chars().any(breaks_single_line)
    {
        return Err(ToolError(
            "Input exceeds byte budget or contains control characters or line breaks",
        ));
    }
    Ok(())
}

/// Characters a single-line field never accepts as new input. Text layout
/// breaks lines at every control line break (LF, CR, VT, FF, NEL) and also at
/// U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR, which are not
/// controls.
fn breaks_single_line(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

/// Marker for a line break: LF, CR, a CR LF pair, VT, FF, NEL and U+2028.
pub const LINE_BREAK_MARKER: char = '\u{23ce}';
/// Marker for U+2029 PARAGRAPH SEPARATOR.
pub const PARAGRAPH_MARKER: char = '\u{b6}';
/// Marker for a tab.
pub const TAB_MARKER: char = '\u{2192}';
/// Marker for every other control character.
pub const CONTROL_MARKER: char = '\u{2b1a}';
/// Zero-width, grapheme-breaking guard placed beside a marker only when a
/// neighbour (a prepended or combining character) would otherwise join the
/// marker's grapheme, which the stored character never does.
const MARKER_GUARD: char = '\u{200b}';

/// The marker shown for one stored character (or CR LF pair) that a
/// single-line field never accepts as new input.
fn marker_for(stored: &str) -> char {
    match stored {
        "\r\n" | "\n" | "\r" | "\u{b}" | "\u{c}" | "\u{85}" | "\u{2028}" => LINE_BREAK_MARKER,
        "\u{2029}" => PARAGRAPH_MARKER,
        "\t" => TAB_MARKER,
        _ => CONTROL_MARKER,
    }
}

/// Stored characters a single-line field shows as markers, as byte ranges
/// in `text`. A CR LF pair is one range, as it is one grapheme to editing.
fn stored_breaks(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start, character) = chars.next()?;
            if !breaks_single_line(character) {
                continue;
            }
            let mut end = start + character.len_utf8();
            if character == '\r' && chars.next_if(|(_, next)| *next == '\n').is_some() {
                end += 1;
            }
            return Some(start..end);
        }
    })
}

/// Display text for a single-line value: each stored line break or control
/// character (a CR LF pair counts as one) becomes one visible marker
/// grapheme, so nothing breaks the line. Returns the markers in order. The
/// stored value is never changed; this is presentation only.
pub fn mark_stored_breaks(text: &str) -> (String, Vec<DisplayMarker>) {
    let mut display = String::with_capacity(text.len());
    let mut markers = Vec::new();
    let mut copied = 0;
    for source in stored_breaks(text) {
        display.push_str(&text[copied..source.start]);
        let glyph = marker_for(&text[source.clone()]);
        let mut glyph_text = [0; 4];
        let glyph_text: &str = glyph.encode_utf8(&mut glyph_text);
        let start = display.len();
        // The stored character is its own grapheme (UAX #29 GB4/GB5); the
        // marker must stay one too, or a caret could not sit beside it.
        let joins_before = display
            .graphemes(true)
            .next_back()
            .is_some_and(|before| format!("{before}{glyph}").graphemes(true).count() == 1);
        let joins_after = text[source.end..]
            .graphemes(true)
            .next()
            .is_some_and(|after| {
                format!("{glyph}{after}").graphemes(true).next() != Some(glyph_text)
            });
        if joins_before {
            display.push(MARKER_GUARD);
        }
        let glyph_range = display.len()..display.len() + glyph.len_utf8();
        display.push(glyph);
        if joins_after {
            display.push(MARKER_GUARD);
        }
        markers.push(DisplayMarker {
            source: source.clone(),
            display: start..display.len(),
            glyph: glyph_range,
        });
        copied = source.end;
    }
    display.push_str(&text[copied..]);
    (display, markers)
}

/// A short note on the stored line breaks (paragraph separators included)
/// and other control characters a single-line value holds, or `None`
/// without any. A CR LF pair counts once, as it has one marker.
pub fn stored_break_hint(text: &str) -> Option<String> {
    let (breaks, controls) = stored_breaks(text).fold((0, 0), |(breaks, controls), range| {
        match marker_for(&text[range]) {
            LINE_BREAK_MARKER | PARAGRAPH_MARKER => (breaks + 1, controls),
            _ => (breaks, controls + 1),
        }
    });
    let count =
        |count: usize, noun: &str| format!("{count} {noun}{}", if count == 1 { "" } else { "s" });
    match (breaks, controls) {
        (0, 0) => None,
        (breaks, 0) => Some(format!("Contains {}", count(breaks, "line break"))),
        (0, controls) => Some(format!("Contains {}", count(controls, "control character"))),
        (breaks, controls) => Some(format!(
            "Contains {} and {}",
            count(breaks, "line break"),
            count(controls, "control character")
        )),
    }
}

/// A byte position before marking to its position in the marked text.
fn marked_byte(markers: &[DisplayMarker], byte: usize) -> usize {
    let index = markers.partition_point(|marker| marker.source.start < byte);
    match index.checked_sub(1).map(|index| &markers[index]) {
        // Inside a CR LF pair: never a grapheme boundary; keep it at the start.
        Some(marker) if byte < marker.source.end => marker.display.start,
        Some(marker) => marker.display.end + (byte - marker.source.end),
        None => byte,
    }
}

/// A position in the marked text to its position before marking. Inside a
/// marker unit (next to a guard) it maps to the stored character's edge on
/// the same side of the glyph; the guard has no width, so that is also where
/// the caret is drawn.
fn unmarked_byte(markers: &[DisplayMarker], byte: usize) -> usize {
    let index = markers.partition_point(|marker| marker.display.start < byte);
    match index.checked_sub(1).map(|index| &markers[index]) {
        Some(marker) if byte < marker.display.end => {
            if byte <= marker.glyph.start {
                marker.source.start
            } else {
                marker.source.end
            }
        }
        Some(marker) => marker.source.end + (byte - marker.display.end),
        None => byte,
    }
}

fn validate_cursor(text: &str, cursor: TextCursor) -> Result<(), ToolError> {
    if cursor.byte == text.len()
        || text
            .grapheme_indices(true)
            .any(|(byte, _)| byte == cursor.byte)
    {
        Ok(())
    } else {
        Err(ToolError("Cursor is not an extended grapheme boundary"))
    }
}

fn snap_after(text: &str, byte: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index >= byte)
        .unwrap_or(text.len())
}

fn snap_before(text: &str, byte: usize) -> usize {
    if byte == text.len() {
        return byte;
    }
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .take_while(|index| *index <= byte)
        .last()
        .unwrap_or(0)
}
