// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Bounded, read-only views of the authoritative World schema and selection.
use tilewright_editor_preview::{PreviewCatalog, SelectionKey};
use tilewright_source::{FieldSpec, FieldType, FieldValue, Object, World};

pub const MAX_INSPECTOR_ROWS: usize = 128;
pub const MAX_DISPLAY_BYTES: usize = 512;
pub const MAX_INPUT_BYTES: usize = 4096;
pub const MAX_CHOICES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotStamp {
    pub key: SelectionKey,
    pub generation: u64,
    pub revision: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionSummary<'a> {
    Object {
        uid: &'a str,
        prototype: &'a str,
        plane: &'a str,
        layer: i32,
        at: Option<[i64; 2]>,
        movable: bool,
    },
    Cell {
        plane: &'a str,
        layer: i32,
        grid: &'a str,
        x: i64,
        y: i64,
        prototype: Option<&'a str>,
    },
}

#[derive(Clone, Debug)]
pub struct FieldRow<'a> {
    pub name: &'a str,
    pub spec: &'a FieldSpec,
    pub current: Option<&'a FieldValue>,
    pub editable: bool,
    /// Missing optional scalars can be entered; required missing values are
    /// explicitly reported as inconsistent, never silently defaulted.
    pub reason: Option<&'static str>,
}

#[derive(Debug)]
pub struct Inspector<'a> {
    pub stamp: Option<SnapshotStamp>,
    pub selection: Option<SelectionSummary<'a>>,
    pub rows: Vec<FieldRow<'a>>,
    pub truncated: bool,
    pub reason: Option<&'static str>,
    pub(crate) world: &'a World,
    pub(crate) catalog: &'a PreviewCatalog,
}

/// `observed_revision` is the document revision observed with this frame, not
/// a revision looked up later at submission time (including for pending views).
pub fn inspect<'a>(
    world: &'a World,
    catalog: &'a PreviewCatalog,
    selection: Option<&'a SelectionKey>,
    generation: u64,
    observed_revision: [u8; 32],
) -> Inspector<'a> {
    let mut result = Inspector {
        stamp: None,
        selection: None,
        rows: Vec::new(),
        truncated: false,
        reason: None,
        world,
        catalog,
    };
    let Some(key) = selection else { return result };
    let (view_plane, view_layer, bounds) = catalog.view();
    match key {
        SelectionKey::Object { uid } => {
            let Some(object) = world.object(uid) else {
                result.reason = Some("Selected UID no longer exists");
                return result;
            };
            let location = world.object_location(uid).expect("object has location");
            if location.path.len() != 1
                || location.plane != view_plane
                || location.chunk.0 != view_layer
            {
                result.reason = Some("Selection is outside the active top-level view");
                return result;
            }
            let size = i64::from(world.planes()[&location.plane].chunk_size);
            let at = object.at.and_then(|[x, y]| {
                Some([
                    location
                        .chunk
                        .1
                        .checked_mul(size)?
                        .checked_add(i64::from(x))?,
                    location
                        .chunk
                        .2
                        .checked_mul(size)?
                        .checked_add(i64::from(y))?,
                ])
            });
            result.selection = Some(SelectionSummary::Object {
                uid,
                prototype: &object.proto,
                plane: &location.plane,
                layer: location.chunk.0,
                at,
                movable: at.is_some() && catalog.editing().movable_uids.contains(uid),
            });
            let fields = world.profile().object_fields.fields();
            result.truncated = fields.len() > MAX_INSPECTOR_ROWS;
            result.rows = inspect_fields(world, catalog, uid, object);
        }
        SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } => {
            let in_bounds = x
                .checked_sub(bounds[0])
                .is_some_and(|v| v >= 0 && v < bounds[2])
                && y.checked_sub(bounds[1])
                    .is_some_and(|v| v >= 0 && v < bounds[3]);
            let Some(grid_index) = world.profile().ground_layers.iter().position(|g| g == grid)
            else {
                result.reason = Some("Selected grid no longer exists");
                return result;
            };
            if plane != view_plane || *layer != view_layer || !in_bounds {
                result.reason = Some("Selected cell is outside the active view");
                return result;
            }
            let Some(p) = world.planes().get(plane) else {
                result.reason = Some("Selected plane no longer exists");
                return result;
            };
            let size = i64::from(p.chunk_size);
            let index = usize::try_from(y.rem_euclid(size) * size + x.rem_euclid(size)).ok();
            let prototype = p
                .chunks
                .get(&(*layer, x.div_euclid(size), y.div_euclid(size)))
                .and_then(|chunk| chunk.ground.get(grid_index))
                .and_then(|g| index.and_then(|i| g.value(i)));
            result.selection = Some(SelectionSummary::Cell {
                plane,
                layer: *layer,
                grid,
                x: *x,
                y: *y,
                prototype,
            });
        }
    }
    result.stamp = Some(SnapshotStamp {
        key: key.clone(),
        generation,
        revision: observed_revision,
    });
    result
}

fn inspect_fields<'a>(
    world: &'a World,
    catalog: &PreviewCatalog,
    uid: &str,
    object: &'a Object,
) -> Vec<FieldRow<'a>> {
    let mut rows = Vec::new();
    for (name, spec) in world
        .profile()
        .object_fields
        .fields()
        .iter()
        .take(MAX_INSPECTOR_ROWS)
    {
        let current = object
            .fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v);
        let reason = if matches!(
            spec.field_type,
            FieldType::Json | FieldType::List { .. } | FieldType::Object { .. }
        ) {
            Some("Structured field is read-only")
        } else if current.is_none() && spec.required {
            Some("Required field is absent; source must be repaired")
        } else if !catalog
            .editing()
            .fields
            .contains(&(uid.to_owned(), name.clone()))
        {
            Some("Field has no editing capability")
        } else if !choices_bounded(world, &spec.field_type) {
            Some("Field choices exceed input budget")
        } else if current
            .is_some_and(|v| matches!(v, FieldValue::String(text) if text.len() > MAX_INPUT_BYTES))
        {
            Some("Current value exceeds input budget")
        } else if current.is_none() {
            Some("Optional field is absent; enter a value to add it")
        } else {
            None
        };
        let editable =
            reason.is_none() || reason == Some("Optional field is absent; enter a value to add it");
        rows.push(FieldRow {
            name,
            spec,
            current,
            editable,
            reason,
        });
    }
    rows
}

pub(crate) fn choices_bounded(world: &World, kind: &FieldType) -> bool {
    match kind {
        FieldType::String { values, .. } => {
            values.len() <= MAX_CHOICES && values.iter().all(|s| s.len() <= MAX_INPUT_BYTES)
        }
        FieldType::Reference { target } => {
            world
                .definitions()
                .keys()
                .filter(|(t, _)| t == target)
                .take(MAX_CHOICES + 1)
                .count()
                <= MAX_CHOICES
        }
        _ => true,
    }
}

/// Bounded UTF-8 presentation. Authoritative strings in World remain intact.
pub fn display_text(value: &str) -> String {
    if value.len() <= MAX_DISPLAY_BYTES {
        return value.into();
    }
    let mut end = MAX_DISPLAY_BYTES - '…'.len_utf8();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

pub fn scalar_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Null => "null".into(),
        FieldValue::Bool(v) => v.to_string(),
        FieldValue::Integer(v) => v.to_string(),
        FieldValue::Number(v) => v.to_string(),
        FieldValue::String(v) => v.clone(),
        FieldValue::Json(_) => "[structured value]".into(),
    }
}
