// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Anchor editing: create, move and delete the anchors of the active view's
//! plane in `planes/<p>/anchors.jsonc`, each one reviewed World
//! transaction. Anchor ids are fixed; nothing here changes one. The editor
//! checks only what it must to refuse clearly (the grant, the view, the
//! identifier's shape, duplicates and the portals that name an anchor);
//! World validates the result when it is prepared and its refusal leaves
//! nothing pending.
use super::{Action, RefusalCode, Session, SessionError, invalid_source, refuse};
use serde_json::Value;

/// JSON text of `text` as a string.
fn string(text: &str) -> String {
    Value::String(text.into()).to_string()
}

/// An anchor entry or its `at` written as text, so it keeps the member
/// order and the spacing of the format's own examples.
fn entry(anchor: &str, layer: i32, [x, y]: [i64; 2]) -> String {
    format!(
        "{{\"id\": {}, \"layer\": {layer}, \"at\": [{x}, {y}]}}",
        string(anchor)
    )
}

impl Session {
    pub(super) fn edit_anchors(&mut self, action: &Action) -> Result<(), SessionError> {
        if !self.catalog.editing().anchors.edit {
            return Err(refuse(
                RefusalCode::GrantMissing,
                "anchor editing refused: the catalog does not grant editing.anchors.edit",
            ));
        }
        let (Action::CreateAnchor {
            plane, expected, ..
        }
        | Action::MoveAnchor {
            plane, expected, ..
        }
        | Action::DeleteAnchor {
            plane, expected, ..
        }) = action
        else {
            return Err(refuse(RefusalCode::Internal, "not an anchor action"));
        };
        let expected = *expected;
        let (view_plane, layer, _) = self.catalog.view();
        if plane != view_plane {
            return Err(refuse(
                RefusalCode::ScopeOutsideView,
                "anchor editing is limited to the active view's plane",
            ));
        }
        self.verify_revision(expected)?;
        let path = format!("planes/{plane}/anchors.jsonc");
        let world = self.history.document().world();
        let source = world
            .planes()
            .get(plane)
            .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown plane"))?;
        let operations = match action {
            Action::CreateAnchor { anchor, x, y, .. } => {
                if !tilewright_source::is_identifier(anchor) {
                    return Err(refuse(
                        RefusalCode::InputIdentifier,
                        "invalid anchor id: use 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
                    ));
                }
                if source.anchors.contains_key(anchor) {
                    return Err(refuse(
                        RefusalCode::TargetExists,
                        "anchor id already used in this plane",
                    ));
                }
                if !self.catalog.tile_in_active_view(plane, layer, *x, *y) {
                    return Err(refuse(
                        RefusalCode::ScopeOutsideView,
                        "anchors can only be created on tiles of the active view",
                    ));
                }
                self.create_anchor_operations(&path, anchor, layer, [*x, *y])?
            }
            Action::MoveAnchor { anchor, x, y, .. } => {
                self.move_anchor_operations(&path, anchor, [*x, *y])?
            }
            Action::DeleteAnchor { anchor, .. } => self.delete_anchor_operations(&path, anchor)?,
            _ => return Err(refuse(RefusalCode::Internal, "not an anchor action")),
        };
        self.prepare_text(expected, &operations, "anchor change refused")
    }

    /// One `set` of the anchor's `at`, expecting its committed source
    /// position. Its `layer` stays: it is on the view's layer already.
    fn move_anchor_operations(
        &self,
        path: &str,
        anchor: &str,
        [x, y]: [i64; 2],
    ) -> Result<Vec<String>, SessionError> {
        let (plane, layer, _) = self.catalog.view();
        let current = self
            .history
            .document()
            .world()
            .planes()
            .get(plane)
            .and_then(|source| source.anchors.get(anchor))
            .ok_or_else(|| {
                refuse(
                    RefusalCode::TargetUnknown,
                    "unknown anchor: it is not in this plane's anchors.jsonc",
                )
            })?;
        if !self
            .catalog
            .tile_in_active_view(plane, current.layer, current.at[0], current.at[1])
        {
            return Err(refuse(
                RefusalCode::ScopeOutsideView,
                "only an anchor on a tile of the active view can be moved",
            ));
        }
        if !self.catalog.tile_in_active_view(plane, layer, x, y) {
            return Err(refuse(
                RefusalCode::ScopeOutsideView,
                "anchors can only be moved to tiles of the active view",
            ));
        }
        if current.at == [x, y] {
            return Err(refuse(
                RefusalCode::ChangeNone,
                "the anchor is already on this tile",
            ));
        }
        let syntax = self.anchors_source(path)?;
        let at = anchor_entry(&syntax, anchor)?
            .get("at")
            .map(|at| syntax.canonical(at))
            .ok_or_else(|| {
                refuse(
                    RefusalCode::SourceInvalid,
                    "the anchor's entry has no position",
                )
            })?;
        Ok(vec![format!(
            "{{\"op\": \"set\", \"path\": {}, \"pointer\": [\"anchors\", {{\"id\": {}}}, \"at\"], \"expect\": {at}, \"value\": [{x}, {y}]}}",
            string(path),
            string(anchor)
        )])
    }

    /// One `remove` of the anchor's entry, expecting its committed source
    /// JSON, once no portal anywhere in the world names it.
    fn delete_anchor_operations(
        &self,
        path: &str,
        anchor: &str,
    ) -> Result<Vec<String>, SessionError> {
        let plane = self.catalog.view().0;
        let world = self.history.document().world();
        if !world
            .planes()
            .get(plane)
            .is_some_and(|source| source.anchors.contains_key(anchor))
        {
            return Err(refuse(
                RefusalCode::TargetUnknown,
                "unknown anchor: it is not in this plane's anchors.jsonc",
            ));
        }
        let holders = crate::anchors::references(world, plane, anchor);
        if !holders.is_empty() {
            return Err(SessionError::new(
                RefusalCode::TargetInUse,
                format!(
                    "anchor is named by portals and cannot be deleted: {}",
                    crate::anchors::describe_holders(&holders)
                ),
            ));
        }
        let syntax = self.anchors_source(path)?;
        let canonical = syntax.canonical(anchor_entry(&syntax, anchor)?);
        Ok(vec![format!(
            "{{\"op\": \"remove\", \"path\": {}, \"pointer\": [\"anchors\", {{\"id\": {}}}], \"expect\": {canonical}}}",
            string(path),
            string(anchor)
        )])
    }

    /// The parsed `anchors.jsonc` at `path`, or `None` when the plane has
    /// none.
    fn read_anchors(&self, path: &str) -> Result<Option<tilewright_jsonc::Document>, SessionError> {
        let document = self.history.document();
        let limits = document.load_options().limits;
        let limit = u64::try_from(limits.max_input_bytes)
            .map_err(|_| refuse(RefusalCode::Internal, "source byte budget overflow"))?;
        let Some(bytes) = document
            .source()
            .read(path, limit)
            .map_err(invalid_source)?
        else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| refuse(RefusalCode::SourceInvalid, "anchors.jsonc is not UTF-8"))?;
        tilewright_jsonc::Document::parse(text, &limits)
            .map(Some)
            .map_err(invalid_source)
    }

    fn anchors_source(&self, path: &str) -> Result<tilewright_jsonc::Document, SessionError> {
        self.read_anchors(path)?
            .ok_or_else(|| refuse(RefusalCode::SourceInvalid, "the plane has no anchors.jsonc"))
    }

    /// The operations that add the new anchor: an `insert` at the end of
    /// the `anchors` array guarded by an absent test, or `document.create`
    /// of `anchors.jsonc` when the plane has none.
    fn create_anchor_operations(
        &self,
        path: &str,
        anchor: &str,
        layer: i32,
        at: [i64; 2],
    ) -> Result<Vec<String>, SessionError> {
        let entry = entry(anchor, layer, at);
        let Some(syntax) = self.read_anchors(path)? else {
            // World writes the new document in its canonical layout.
            let text = format!("{{\"format\": \"tilewright.anchors/1\", \"anchors\": [{entry}]}}");
            return Ok(vec![format!(
                "{{\"op\": \"document.create\", \"path\": {}, \"document\": {text}}}",
                string(path)
            )]);
        };
        let count = syntax
            .root()
            .get("anchors")
            .and_then(tilewright_jsonc::Value::as_array)
            .ok_or_else(|| {
                refuse(
                    RefusalCode::SourceInvalid,
                    "anchors.jsonc has no anchors array to add to",
                )
            })?
            .len();
        Ok(vec![
            format!(
                "{{\"op\": \"test\", \"path\": {}, \"pointer\": [\"anchors\", {{\"id\": {}}}], \"absent\": true}}",
                string(path),
                string(anchor)
            ),
            format!(
                "{{\"op\": \"insert\", \"path\": {}, \"pointer\": [\"anchors\", {count}], \"value\": {entry}}}",
                string(path)
            ),
        ])
    }
}

/// The source entry of anchor `anchor` in a parsed `anchors.jsonc`.
fn anchor_entry<'d>(
    syntax: &'d tilewright_jsonc::Document,
    anchor: &str,
) -> Result<&'d tilewright_jsonc::Value, SessionError> {
    syntax
        .root()
        .get("anchors")
        .and_then(tilewright_jsonc::Value::as_array)
        .and_then(|entries| {
            entries.iter().find(|entry| {
                entry.get("id").and_then(tilewright_jsonc::Value::as_str) == Some(anchor)
            })
        })
        .ok_or_else(|| {
            refuse(
                RefusalCode::SourceInvalid,
                "the anchor's source entry was not found",
            )
        })
}
