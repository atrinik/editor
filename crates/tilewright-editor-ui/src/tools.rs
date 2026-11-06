// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Local input only; every edit leaves this module as a Session action.
use crate::brush::{Brush, Stroke};
use crate::text_input::{TextInput, TextInputSnapshot, TextRevision, TextSelection};
use crate::{
    Action, SelectionKey,
    inspector::{
        Inspector, MAX_CHOICES, MAX_INPUT_BYTES, SelectionSummary, SnapshotStamp, scalar_text,
    },
};
use serde_json::Value;
use std::fmt;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::FieldType;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToolMode {
    #[default]
    Select,
    Move,
    Terrain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainChoice {
    pub grid: String,
    pub prototype: String,
}

/// Only explicit capabilities appear; palette selection never implies placement.
pub fn terrain_palette(catalog: &PreviewCatalog) -> Vec<TerrainChoice> {
    catalog
        .editing()
        .terrain
        .iter()
        .flat_map(|(grid, prototypes)| {
            prototypes.iter().map(move |prototype| TerrainChoice {
                grid: grid.clone(),
                prototype: prototype.clone(),
            })
        })
        .take(MAX_CHOICES)
        .collect()
}

#[derive(Clone, Debug)]
enum DraftKind {
    Field(String),
    Move,
}
#[derive(Clone, Debug)]
struct Draft {
    stamp: SnapshotStamp,
    kind: DraftKind,
    input: TextInput,
}

#[derive(Clone, Debug, Default)]
pub struct ToolState {
    mode: ToolMode,
    terrain: Option<TerrainChoice>,
    draft: Option<Draft>,
    next_draft: u64,
    brush: Brush,
    stroke: Option<Stroke>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolError(pub &'static str);
impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ToolError {}

impl ToolState {
    pub fn mode(&self) -> ToolMode {
        self.mode
    }
    pub fn set_mode(&mut self, mode: ToolMode) {
        self.cancel();
        self.mode = mode;
    }
    pub fn brush(&self) -> Brush {
        self.brush
    }
    /// A running stroke keeps the brush it began with.
    pub fn set_brush(&mut self, brush: Brush) {
        self.brush = brush.resized(0);
    }
    pub fn terrain_choice(&self) -> Option<&TerrainChoice> {
        self.terrain.as_ref()
    }
    pub fn choose_terrain(
        &mut self,
        catalog: &PreviewCatalog,
        grid: &str,
        prototype: &str,
    ) -> Result<(), ToolError> {
        if !terrain_palette(catalog)
            .iter()
            .any(|c| c.grid == grid && c.prototype == prototype)
        {
            return Err(ToolError("Terrain value is not in the explicit palette"));
        }
        self.terrain = Some(TerrainChoice {
            grid: grid.into(),
            prototype: prototype.into(),
        });
        Ok(())
    }
    pub fn begin_field(
        &mut self,
        inspector: &Inspector<'_>,
        field: &str,
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let row = inspector
            .rows
            .iter()
            .find(|row| row.name == field && row.editable)
            .ok_or(ToolError("Field is read-only or absent from inspector"))?;
        let stamp = inspector
            .stamp
            .clone()
            .ok_or(ToolError("No valid selection"))?;
        let text = row.current.map_or_else(String::new, scalar_text);
        if text.len() > MAX_INPUT_BYTES {
            return Err(ToolError("Input exceeds byte budget"));
        }
        let input = self.new_input(text)?;
        self.draft = Some(Draft {
            stamp,
            kind: DraftKind::Field(field.into()),
            input,
        });
        Ok(())
    }
    pub fn begin_move(
        &mut self,
        inspector: &Inspector<'_>,
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let Some(SelectionSummary::Object {
            at: Some([x, y]),
            movable: true,
            ..
        }) = inspector.selection
        else {
            return Err(ToolError("Selection has no movement capability"));
        };
        let input = self.new_input(format!("{x},{y}"))?;
        self.draft = Some(Draft {
            stamp: inspector
                .stamp
                .clone()
                .ok_or(ToolError("No valid selection"))?,
            kind: DraftKind::Move,
            input,
        });
        Ok(())
    }
    /// Replace selection with exact UTF-8 atomically; commit any active preedit.
    pub fn input(&mut self, text: &str) -> Result<(), ToolError> {
        self.input_mut()?.input(text)
    }
    pub fn backspace(&mut self) {
        if let Some(draft) = &mut self.draft {
            // Legacy callers need no error result. Composition blocks deletion;
            // native dispatch handles IME keys before calling this method.
            let _ = draft.input.backspace();
        }
    }
    /// Discard local input: the text draft and any running brush stroke.
    pub fn cancel(&mut self) {
        self.draft = None;
        self.stroke = None;
    }
    pub fn draft_text(&self) -> Option<&str> {
        self.draft.as_ref().map(|draft| draft.input.text())
    }
    pub fn draft_snapshot(&self) -> Option<TextInputSnapshot> {
        self.draft.as_ref().map(|draft| draft.input.snapshot())
    }
    pub fn input_revision(&self) -> Option<TextRevision> {
        self.draft.as_ref().map(|draft| draft.input.revision())
    }
    pub fn is_composing(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.input.is_composing())
    }
    pub fn set_selection(
        &mut self,
        selection: TextSelection,
        presented: TextRevision,
    ) -> Result<(), ToolError> {
        self.input_mut()?.set_selection(selection, presented)
    }
    pub fn select_all(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.select_all()
    }
    pub fn delete(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.delete()
    }
    pub fn update_preedit(&mut self, text: &str, start: i32, length: i32) -> Result<(), ToolError> {
        self.input_mut()?.update_preedit(text, start, length)
    }
    pub fn cancel_preedit(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.cancel_preedit()
    }

    fn input_mut(&mut self) -> Result<&mut TextInput, ToolError> {
        self.draft
            .as_mut()
            .map(|draft| &mut draft.input)
            .ok_or(ToolError("No input draft"))
    }

    fn new_input(&mut self, text: String) -> Result<TextInput, ToolError> {
        let identity = self
            .next_draft
            .checked_add(1)
            .ok_or(ToolError("Draft identity exhausted"))?;
        let input = TextInput::with_identity(text, identity)?;
        self.next_draft = identity;
        Ok(input)
    }
    pub fn build_action(
        &self,
        inspector: &Inspector<'_>,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let draft = self.draft.as_ref().ok_or(ToolError("No input draft"))?;
        if inspector.stamp.as_ref() != Some(&draft.stamp) {
            return Err(ToolError(
                "Draft selection, generation or source revision is stale",
            ));
        }
        if draft.input.is_composing() {
            return Err(ToolError("Finish or cancel composition before review"));
        }
        let SelectionKey::Object { uid } = &draft.stamp.key else {
            return Err(ToolError("Draft requires an object"));
        };
        match &draft.kind {
            DraftKind::Field(field) => {
                let row = inspector
                    .rows
                    .iter()
                    .find(|row| row.name == field && row.editable)
                    .ok_or(ToolError("Field is no longer editable"))?;
                let value = parse_scalar(&row.spec.field_type, draft.input.text(), inspector)?;
                Ok(Action::SetField {
                    uid: uid.clone(),
                    field: field.clone(),
                    value,
                    expected: draft.stamp.revision,
                })
            }
            DraftKind::Move => {
                if !matches!(
                    inspector.selection,
                    Some(SelectionSummary::Object { movable: true, .. })
                ) {
                    return Err(ToolError("Selection has no movement capability"));
                }
                let (x, y) = draft
                    .input
                    .text()
                    .split_once(',')
                    .ok_or(ToolError("Enter coordinates as x,y"))?;
                let x = x
                    .trim()
                    .parse()
                    .map_err(|_| ToolError("Invalid x coordinate"))?;
                let y = y
                    .trim()
                    .parse()
                    .map_err(|_| ToolError("Invalid y coordinate"))?;
                Ok(Action::Move {
                    uid: uid.clone(),
                    x,
                    y,
                    expected: draft.stamp.revision,
                })
            }
        }
    }
    pub fn paint_action(
        &self,
        inspector: &Inspector<'_>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        if self.mode != ToolMode::Terrain {
            return Err(ToolError("Terrain tool is not active"));
        }
        let stamp = inspector
            .stamp
            .as_ref()
            .filter(|s| &s.key == cell)
            .ok_or(ToolError("Paint requires the current inspected cell"))?;
        let choice = self
            .terrain
            .as_ref()
            .ok_or(ToolError("Choose an explicit terrain palette value"))?;
        let SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } = cell
        else {
            return Err(ToolError("Paint requires a cell"));
        };
        if grid != &choice.grid || !terrain_palette(inspector.catalog).contains(choice) {
            return Err(ToolError("Terrain choice is unavailable for this grid"));
        }
        Ok(Action::Paint {
            plane: plane.clone(),
            layer: *layer,
            grid: grid.clone(),
            x: *x,
            y: *y,
            proto: choice.prototype.clone(),
            expected: stamp.revision,
        })
    }
}

impl ToolState {
    pub fn stroke(&self) -> Option<&Stroke> {
        self.stroke.as_ref()
    }
    /// Begin a drag stroke at the inspected `cell` with the chosen terrain and
    /// brush. It uses the same capability, palette and freshness checks as a
    /// single-cell paint and stamps the press cell immediately.
    pub fn begin_stroke(
        &mut self,
        inspector: &Inspector<'_>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<(), ToolError> {
        let Action::Paint {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            expected,
        } = self.paint_action(inspector, cell, pending)?
        else {
            return Err(ToolError("Paint requires a cell"));
        };
        let generation = inspector
            .stamp
            .as_ref()
            .map(|stamp| stamp.generation)
            .ok_or(ToolError("Paint requires the current inspected cell"))?;
        let (_, _, bounds) = inspector.catalog.view();
        self.draft = None;
        let mut stroke = Stroke::begin(
            plane, layer, grid, proto, self.brush, expected, generation, bounds,
        );
        stroke.stamp([x, y]);
        self.stroke = Some(stroke);
        Ok(())
    }
    /// Add one pointer sample. `cell` must come from the exact cell mask of
    /// the stroke's own frame `generation`; a changed frame ends the stroke.
    pub fn extend_stroke(
        &mut self,
        cell: &SelectionKey,
        generation: u64,
    ) -> Result<usize, ToolError> {
        let stroke = self.stroke.as_mut().ok_or(ToolError("No brush stroke"))?;
        if stroke.generation() != generation {
            self.stroke = None;
            return Err(ToolError(
                "View changed during the brush stroke; nothing was prepared",
            ));
        }
        let SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } = cell
        else {
            return Err(ToolError("Brush sample is not a terrain cell"));
        };
        if plane != stroke.plane() || *layer != stroke.layer() || grid != stroke.grid() {
            return Err(ToolError("Brush sample is outside the stroke's grid"));
        }
        Ok(stroke.stamp([*x, *y]))
    }
    /// End the gesture. Returns the single reviewed paint action for every
    /// covered cell, or `None` when no stroke was running.
    pub fn finish_stroke(
        &mut self,
        generation: u64,
        pending: bool,
    ) -> Result<Option<Action>, ToolError> {
        let Some(stroke) = self.stroke.take() else {
            return Ok(None);
        };
        if stroke.generation() != generation {
            return Err(ToolError(
                "View changed during the brush stroke; nothing was prepared",
            ));
        }
        available(pending)?;
        stroke.into_action().map(Some)
    }
    /// Abandon a running stroke without preparing anything.
    pub fn cancel_stroke(&mut self) -> bool {
        self.stroke.take().is_some()
    }
}

fn available(pending: bool) -> Result<(), ToolError> {
    if pending {
        Err(ToolError("Accept or cancel the pending edit first"))
    } else {
        Ok(())
    }
}

/// Authoritative enum/reference options, bounded like inspector input.
pub fn field_choices<'a>(inspector: &'a Inspector<'_>, field: &str) -> Vec<&'a str> {
    let Some(row) = inspector
        .rows
        .iter()
        .find(|r| r.name == field && r.editable)
    else {
        return Vec::new();
    };
    match &row.spec.field_type {
        FieldType::String { values, .. } => values
            .iter()
            .take(MAX_CHOICES)
            .map(String::as_str)
            .collect(),
        FieldType::Reference { target } => inspector
            .world
            .definitions()
            .keys()
            .filter(|(t, _)| t == target)
            .take(MAX_CHOICES)
            .map(|(_, id)| id.as_str())
            .collect(),
        FieldType::Boolean => vec!["true", "false"],
        _ => Vec::new(),
    }
}

fn parse_scalar(
    kind: &FieldType,
    text: &str,
    inspector: &Inspector<'_>,
) -> Result<Value, ToolError> {
    match kind {
        FieldType::Boolean => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(ToolError("Enter true or false")),
        },
        FieldType::Integer { minimum, maximum } => {
            let value = text
                .parse::<i64>()
                .map_err(|_| ToolError("Enter an integer"))?;
            if value < *minimum || value > *maximum {
                return Err(ToolError("Integer is outside schema range"));
            }
            Ok(Value::from(value))
        }
        FieldType::Number { minimum, maximum } => {
            let value = text
                .parse::<f64>()
                .map_err(|_| ToolError("Enter a finite number"))?;
            if !value.is_finite() || value < *minimum || value > *maximum {
                return Err(ToolError("Number is non-finite or outside schema range"));
            }
            Ok(Value::from(value))
        }
        FieldType::String {
            maximum_length,
            values,
        } => {
            if text.chars().count() > *maximum_length
                || (!values.is_empty() && !values.contains(text))
            {
                return Err(ToolError(
                    "String exceeds schema length or is not an allowed choice",
                ));
            }
            Ok(Value::String(text.into()))
        }
        FieldType::Reference { target } => {
            if !inspector
                .world
                .definitions()
                .contains_key(&(target.clone(), text.into()))
            {
                return Err(ToolError(
                    "Reference does not name an authoritative definition",
                ));
            }
            Ok(Value::String(text.into()))
        }
        FieldType::Json | FieldType::List { .. } | FieldType::Object { .. } => {
            Err(ToolError("Structured fields are read-only"))
        }
    }
}
