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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSnapshot {
    pub committed: String,
    /// Raw, complete UTF-8 for shared shaping; do not terminal-escape or elide.
    pub display_text: String,
    pub selection: TextSelection,
    /// Grapheme-safe cursors in `display_text`, including the preedit splice.
    pub display_selection: TextSelection,
    pub composition: Option<CompositionSnapshot>,
    pub revision: TextRevision,
}

impl TextInputSnapshot {
    /// Map a displayed cursor back to committed text. Interior preedit positions
    /// have no committed counterpart; callers must cancel composition first.
    pub fn committed_cursor(&self, cursor: TextCursor) -> Result<TextCursor, ToolError> {
        validate_cursor(&self.display_text, cursor)?;
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
}

impl TextInput {
    pub fn new(text: String) -> Result<Self, ToolError> {
        Self::with_identity(text, 0)
    }

    pub(crate) fn with_identity(text: String, draft: u64) -> Result<Self, ToolError> {
        // Existing source can contain controls. Opening it must remain lossless;
        // only newly inserted user input retains the single-line control policy.
        if text.len() > MAX_INPUT_BYTES {
            return Err(ToolError("Input exceeds byte budget"));
        }
        let selection = TextSelection::caret(TextCursor::new(text.len()));
        Ok(Self {
            text,
            selection,
            composition: None,
            revision: TextRevision { draft, revision: 0 },
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
        TextInputSnapshot {
            committed: self.text.clone(),
            display_text,
            selection: self.selection,
            display_selection,
            composition,
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
        validate_insert(&self.text, &replacement, text)?;
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
        validate_insert(&self.text, &range, text)?;
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

fn validate_insert(current: &str, range: &Range<usize>, text: &str) -> Result<(), ToolError> {
    if range.start > range.end {
        return Err(ToolError("Invalid replacement range"));
    }
    validate_cursor(current, TextCursor::new(range.start))?;
    validate_cursor(current, TextCursor::new(range.end))?;
    if text.len() > MAX_INPUT_BYTES - (current.len() - range.len())
        || text.chars().any(char::is_control)
    {
        return Err(ToolError(
            "Input exceeds byte budget or contains control characters",
        ));
    }
    Ok(())
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
