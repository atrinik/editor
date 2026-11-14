// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Pure action routing. World History remains the only document/history owner.
use serde_json::{Value, json};
use std::fmt;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_preview::{
    PreviewCatalog, PreviewLimits, ReadyResources, SelectionKey, loader,
};
use tilewright_source::{FieldValue, World};

use crate::Actor;
use crate::review::{PendingSummary, REVIEW_LIMIT_MESSAGE, Review};
use crate::uid::{OsUidSource, UidSource};

mod anchor_edit;
mod portal_edit;
pub(crate) mod refusal;
mod zone_edit;

use refusal::refuse;
pub use refusal::{RefusalCode, SessionError};

/// A failure outside the action's input: a bug, not a refusal of it.
fn internal(error: impl fmt::Display) -> SessionError {
    SessionError::new(RefusalCode::Internal, error.to_string())
}
/// A source document that cannot be read or parsed.
fn invalid_source(error: impl fmt::Display) -> SessionError {
    SessionError::new(RefusalCode::SourceInvalid, error.to_string())
}

/// Where a portal set by [`Action::SetPortal`] leads. The plane is always
/// named here; the written portal leaves `plane` out when it is the
/// object's own plane, because the format defaults it to that plane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalTarget {
    /// Anchor `anchor` of plane `plane`.
    Anchor { plane: String, anchor: String },
    /// Plane tile `at` of spatial layer `layer` in plane `plane`.
    Location {
        plane: String,
        layer: i32,
        at: [i64; 2],
    },
}

#[derive(Clone, Debug)]
pub enum Action {
    Select {
        generation: u64,
        key: SelectionKey,
    },
    ClearSelection,
    Move {
        uid: String,
        x: i64,
        y: i64,
        expected: [u8; 32],
    },
    SetField {
        uid: String,
        field: String,
        value: Value,
        expected: [u8; 32],
    },
    Paint {
        plane: String,
        layer: i32,
        grid: String,
        x: i64,
        y: i64,
        proto: String,
        expected: [u8; 32],
    },
    /// One brush gesture: every covered cell in one reviewed `terrain.set`.
    PaintStroke {
        plane: String,
        layer: i32,
        grid: String,
        cells: Vec<[i64; 2]>,
        proto: String,
        expected: [u8; 32],
    },
    /// One zone brush gesture: every covered cell of the active view gets
    /// `zone`, or is emptied (`None`, the plane's default zone), in one
    /// reviewed `terrain.set` on the grid `zone`. Needs `editing.zones.paint`.
    PaintZones {
        plane: String,
        layer: i32,
        cells: Vec<[i64; 2]>,
        zone: Option<String>,
        expected: [u8; 32],
    },
    /// Delete one placed object in the active view, with its inventory, as
    /// one reviewed `object.remove`.
    Remove {
        uid: String,
        expected: [u8; 32],
    },
    /// Place one new object of a granted prototype on plane tile `x, y` of
    /// the active view, as one reviewed `object.insert` on top of the tile's
    /// stack, with a new uid and the grant's render layer and defaults.
    Place {
        plane: String,
        layer: i32,
        x: i64,
        y: i64,
        proto: String,
        expected: [u8; 32],
    },
    /// Set or clear the display name of zone `zone` of the active view's
    /// plane: one `set` (or `remove` when `name` is `None` or empty) on its
    /// `name` in `zones.jsonc`. Needs `editing.zones.manage`. Zone ids are
    /// fixed: this never changes `zone` itself.
    RenameZone {
        plane: String,
        zone: String,
        name: Option<String>,
        expected: [u8; 32],
    },
    /// Make `parent` the parent of zone `zone`, or make it a top-level zone
    /// (`None`). Refused for an unknown parent and for `zone` itself or one
    /// of its descendants, which would make a cycle. Needs
    /// `editing.zones.manage`.
    SetZoneParent {
        plane: String,
        zone: String,
        parent: Option<String>,
        expected: [u8; 32],
    },
    /// Add zone `zone`, with an optional display name and parent, at the end
    /// of the plane's `zones.jsonc`, creating the document when the plane
    /// has none. `zone` must be a World identifier the plane does not use
    /// yet. Needs `editing.zones.manage`.
    CreateZone {
        plane: String,
        zone: String,
        name: Option<String>,
        parent: Option<String>,
        expected: [u8; 32],
    },
    /// Delete zone `zone` from the plane's `zones.jsonc`. Refused, with the
    /// reasons, while any tile of any layer holds it, a chunk's zone palette
    /// lists it, a zone has it as parent or it is the plane's default zone.
    /// Needs `editing.zones.manage`.
    DeleteZone {
        plane: String,
        zone: String,
        expected: [u8; 32],
    },
    /// Add anchor `anchor` on plane tile `x, y` of the active view, on its
    /// spatial layer: one `insert` at the end of the plane's
    /// `anchors.jsonc`, or its `document.create` when the plane has none.
    /// `anchor` must be a World identifier the plane does not use yet.
    /// Needs `editing.anchors.edit`. Anchor ids are fixed once created.
    CreateAnchor {
        plane: String,
        anchor: String,
        x: i64,
        y: i64,
        expected: [u8; 32],
    },
    /// Move anchor `anchor`, which is on a tile of the active view, to plane
    /// tile `x, y` of the active view: one `set` of its `at` expecting the
    /// committed position. Every portal that names the anchor now leads to
    /// the new tile. Needs `editing.anchors.edit`.
    MoveAnchor {
        plane: String,
        anchor: String,
        x: i64,
        y: i64,
        expected: [u8; 32],
    },
    /// Delete anchor `anchor` of the active view's plane: one `remove` of
    /// its entry expecting its committed JSON. Refused, with the objects
    /// listed, while a portal anywhere in the world names it. Needs
    /// `editing.anchors.edit`.
    DeleteAnchor {
        plane: String,
        anchor: String,
        expected: [u8; 32],
    },
    /// Set (`Some`) or clear (`None`) the portal of placed object `uid`,
    /// which must be a top-level object of the active view whose prototype
    /// the catalog grants `portal: true`. One `set` (after an absent test
    /// when the object has none) or `remove` of its `portal` member, each
    /// expecting the committed value. The target may be anywhere; its plane
    /// and, for an anchor target, the anchor must exist.
    SetPortal {
        uid: String,
        target: Option<PortalTarget>,
        expected: [u8; 32],
    },
    Accept,
    Cancel,
    Undo,
    Redo,
    Reopen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPhase {
    Ready,
    Pending,
    Retaining,
    Publishing,
    Blocked,
}

#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub phase: SessionPhase,
    pub message: String,
    pub applied: Option<bool>,
    pub durable: Option<bool>,
    pub cleaned: Option<bool>,
    pub recovery_descriptor: Option<String>,
}
impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.phase, self.message)?;
        if let Some(applied) = self.applied {
            write!(
                f,
                " applied={applied} durable={:?} cleaned={:?}",
                self.durable, self.cleaned
            )?;
        }
        if let Some(record) = &self.recovery_descriptor {
            write!(f, " recovery={record}")?;
        }
        Ok(())
    }
}

/// Toolkit-canonical source spellings for the fixed coordinate guard layout.
/// In particular, World's transaction comparisons distinguish `-0` from `0`.
struct CoordinateGuards {
    old_at: String,
    changed_axes: Vec<String>,
}
impl CoordinateGuards {
    fn apply(
        &self,
        text: String,
        limits: &tilewright_jsonc::Limits,
    ) -> Result<String, SessionError> {
        let mut syntax = tilewright_jsonc::Document::parse(text, limits).map_err(internal)?;
        let guards = std::iter::once((2, "value", &self.old_at)).chain(
            self.changed_axes
                .iter()
                .enumerate()
                .map(|(index, value)| (index + 3, "expect", value)),
        );
        for (index, key, value) in guards {
            let operations = syntax
                .root()
                .get("operations")
                .and_then(tilewright_jsonc::Value::as_array)
                .filter(|operations| operations.len() == 3 + self.changed_axes.len())
                .ok_or_else(|| {
                    refuse(
                        RefusalCode::Internal,
                        "invalid coordinate transaction guard layout",
                    )
                })?;
            let span = operations
                .get(index)
                .and_then(|operation| operation.get(key))
                .ok_or_else(|| {
                    refuse(
                        RefusalCode::Internal,
                        "missing coordinate transaction guard",
                    )
                })?
                .span();
            syntax = syntax.replace(span, value, limits).map_err(internal)?;
        }
        Ok(syntax.source().into())
    }
}

/// One cell a `terrain.set` changes and the value it holds before.
type TerrainChange<'w> = ([i64; 2], Option<&'w str>);

pub struct Session {
    history: History,
    catalog: PreviewCatalog,
    resources: ReadyResources,
    selection: Option<SelectionKey>,
    generation: u64,
    status: SessionStatus,
    /// Review notes and warnings for the pending transaction, refreshed
    /// after every dispatch; only dispatch changes the pending transaction.
    warnings: Vec<String>,
    /// The review of the pending transaction, refreshed with `warnings`.
    review: Review,
    /// The actor of every transaction this session prepares.
    actor: Actor,
    /// Material for the uids of placed objects.
    uids: Box<dyn UidSource>,
    /// The uid a pending placement adds; selected when it is accepted.
    placing: Option<String>,
}
impl Session {
    /// A session whose transactions all carry `actor`.
    pub fn new(
        history: History,
        catalog: PreviewCatalog,
        resources: ReadyResources,
        actor: Actor,
    ) -> Self {
        let mut session = Self {
            history,
            catalog,
            resources,
            selection: None,
            generation: 1,
            status: SessionStatus {
                phase: SessionPhase::Ready,
                message: "World workspace edits publish immediately after durable recovery".into(),
                applied: None,
                durable: None,
                cleaned: None,
                recovery_descriptor: None,
            },
            warnings: Vec::new(),
            review: Review::of(None, &[]),
            actor,
            uids: Box::new(OsUidSource),
            placing: None,
        };
        session.refresh_review();
        session
    }
    /// The actor of every transaction this session prepares.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }
    /// Replace the uid source, for example with a deterministic one in tests
    /// and the scripted demonstration.
    pub fn set_uid_source(&mut self, source: Box<dyn UidSource>) {
        self.uids = source;
    }
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn catalog(&self) -> &PreviewCatalog {
        &self.catalog
    }
    pub fn resources(&self) -> &ReadyResources {
        &self.resources
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn selection(&self) -> Option<&SelectionKey> {
        self.selection.as_ref()
    }
    pub fn status(&self) -> &SessionStatus {
        &self.status
    }
    /// Whether a new edit or history preview may be prepared. Dispatch still
    /// checks the catalog and the action author's original source revision.
    pub fn can_prepare(&self) -> bool {
        self.status.phase == SessionPhase::Ready
            && !self.history.needs_reload()
            && self.history.pending().is_none()
    }
    pub fn world(&self) -> &World {
        self.history
            .pending()
            .map_or(self.history.document().world(), |p| p.world())
    }
    /// The recovery service supplies its bounded descriptor; the UI owns no storage.
    pub fn set_recovery_descriptor(&mut self, descriptor: String) {
        self.status.recovery_descriptor = Some(if descriptor.len() <= 1024 {
            descriptor
        } else {
            descriptor.chars().take(1024).collect()
        });
    }
    /// Native camera/viewport changes invalidate earlier input frames.
    pub fn invalidate_frame(&mut self) -> Result<(), SessionError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| refuse(RefusalCode::Internal, "frame generation exhausted"))?;
        Ok(())
    }
    /// The review of the pending transaction, as a person sees it: its
    /// notes and warnings, then a bounded excerpt of every changed file.
    /// Accept is refused while it is incomplete.
    pub fn review(&self) -> &Review {
        &self.review
    }
    /// A bounded structured view of the pending transaction, or `None`
    /// when nothing is pending.
    pub fn pending_summary(&self) -> Option<PendingSummary<'_>> {
        let pending = self.history.pending()?;
        Some(PendingSummary::of(
            self.history.document().revision(),
            pending,
            &self.review,
            &self.warnings,
        ))
    }
    /// Review notes for every object the pending transaction adds and
    /// warnings for every object it removes, judged against the committed
    /// World. They never block Accept.
    pub fn pending_warnings(&self) -> &[String] {
        &self.warnings
    }
    fn verify_catalog(&self) -> Result<(), SessionError> {
        loader::revalidate(
            self.history.document().source(),
            &self.catalog,
            &self.resources,
            PreviewLimits::default(),
        )
        .map_err(|e| SessionError::new(RefusalCode::CatalogRefused, e.to_string()))
    }
    fn verify_revision(&self, expected: [u8; 32]) -> Result<(), SessionError> {
        if expected != self.history.document().revision() {
            return Err(refuse(RefusalCode::RevisionStale, "stale source revision"));
        }
        let current = self
            .history
            .document()
            .reload()
            .map_err(|e| SessionError::new(RefusalCode::SourceInvalid, e.to_string()))?;
        if current.revision() != expected {
            return Err(refuse(
                RefusalCode::SourceChanged,
                "external source change: inspect and reopen a fresh session",
            ));
        }
        Ok(())
    }
    /// Why a per-object grant refused `uid`: it does not exist, it is
    /// carried in an inventory (per-prototype grants cover placed objects
    /// only), its prototype is granted (`granted`) but it is outside the
    /// active view, or nothing grants it. Per-uid grants are not
    /// view-scoped and are checked by the caller.
    fn object_grant_refusal(
        &self,
        uid: &str,
        granted: impl FnOnce(&tilewright_editor_preview::ObjectGrant) -> bool,
    ) -> RefusalCode {
        let world = self.history.document().world();
        let (Some(object), Some(location)) = (world.object(uid), world.object_location(uid)) else {
            return RefusalCode::TargetUnknown;
        };
        if !self
            .catalog
            .editing()
            .objects
            .get(&object.proto)
            .is_some_and(granted)
        {
            RefusalCode::GrantMissing
        } else if location.path.len() != 1 {
            RefusalCode::TargetUnsupported
        } else {
            RefusalCode::ScopeOutsideView
        }
    }
    fn in_bounds(&self, x: i64, y: i64) -> bool {
        let (_, _, [min_x, min_y, w, h]) = self.catalog.view();
        x.checked_sub(min_x).is_some_and(|v| v >= 0 && v < w)
            && y.checked_sub(min_y).is_some_and(|v| v >= 0 && v < h)
    }
    fn valid_key(&self, key: &SelectionKey) -> bool {
        match key {
            SelectionKey::Object { uid } => {
                let (plane, layer, _) = self.catalog.view();
                self.world().object_location(uid).is_some_and(|location| {
                    location.path.len() == 1 && location.plane == plane && location.chunk.0 == layer
                })
            }
            SelectionKey::Cell {
                plane,
                layer,
                grid,
                x,
                y,
            } => {
                let (p, l, _) = self.catalog.view();
                plane == p
                    && *layer == l
                    && self.world().profile().ground_layers.contains(grid)
                    && self.in_bounds(*x, *y)
            }
        }
    }
    fn revalidate_selection(&mut self) {
        if self
            .selection
            .as_ref()
            .is_some_and(|key| !self.valid_key(key))
        {
            self.selection = None;
        }
    }
    /// Inspect source-only details through the toolkit's bounded AST. History
    /// still verifies the complete source revision before planning.
    fn inspect_object_source<T>(
        &self,
        uid: &str,
        inspect: impl FnOnce(
            &tilewright_jsonc::Document,
            &tilewright_jsonc::Value,
        ) -> Result<T, SessionError>,
    ) -> Result<T, SessionError> {
        let document = self.history.document();
        let world = document.world();
        let location = world
            .object_location(uid)
            .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown UID location"))?;
        let chunk = world
            .planes()
            .get(&location.plane)
            .and_then(|plane| plane.chunks.get(&location.chunk))
            .ok_or_else(|| {
                refuse(
                    RefusalCode::SourceInvalid,
                    "missing authoritative object chunk",
                )
            })?;
        let limits = document.load_options().limits;
        let limit = u64::try_from(limits.max_input_bytes)
            .map_err(|_| refuse(RefusalCode::Internal, "source byte budget overflow"))?;
        let bytes = document
            .source()
            .read(&chunk.path, limit)
            .map_err(invalid_source)?
            .ok_or_else(|| refuse(RefusalCode::SourceInvalid, "object source chunk is missing"))?;
        let text = String::from_utf8(bytes).map_err(|_| {
            refuse(
                RefusalCode::SourceInvalid,
                "object source chunk is not UTF-8",
            )
        })?;
        let syntax = tilewright_jsonc::Document::parse(text, &limits).map_err(invalid_source)?;
        let mut object = syntax.root();
        for (depth, index) in location.path.iter().enumerate() {
            object = object
                .get(if depth == 0 { "objects" } else { "inventory" })
                .and_then(tilewright_jsonc::Value::as_array)
                .and_then(|objects| objects.get(*index))
                .ok_or_else(|| {
                    refuse(RefusalCode::SourceChanged, "object source location changed")
                })?;
        }
        if object.get("uid").and_then(tilewright_jsonc::Value::as_str) != Some(uid) {
            return Err(refuse(
                RefusalCode::SourceChanged,
                "object source UID changed",
            ));
        }
        inspect(&syntax, object)
    }
    fn coordinate_guards(
        &self,
        uid: &str,
        old_at: [u16; 2],
        at: [u16; 2],
    ) -> Result<CoordinateGuards, SessionError> {
        self.inspect_object_source(uid, |syntax, object| {
            let value = object
                .get("at")
                .ok_or_else(|| refuse(RefusalCode::SourceInvalid, "missing source coordinates"))?;
            let axes = value
                .as_array()
                .filter(|axes| axes.len() == 2)
                .ok_or_else(|| refuse(RefusalCode::SourceInvalid, "invalid source coordinates"))?;
            let mut changed_axes = Vec::new();
            for axis in 0..2 {
                if axes[axis].as_i64() != Some(i64::from(old_at[axis])) {
                    return Err(refuse(
                        RefusalCode::SourceChanged,
                        "source coordinates changed",
                    ));
                }
                if old_at[axis] != at[axis] {
                    changed_axes.push(syntax.canonical(&axes[axis]));
                }
            }
            Ok(CoordinateGuards {
                old_at: syntax.canonical(value),
                changed_axes,
            })
        })
    }
    /// Capability-checked `terrain.set` cells, each expecting its committed
    /// value. With `skip_unchanged`, cells already holding `proto` are omitted.
    fn terrain_cells(
        &self,
        plane: &str,
        layer: i32,
        grid: &str,
        cells: &[[i64; 2]],
        proto: &str,
        skip_unchanged: bool,
    ) -> Result<Vec<Value>, SessionError> {
        let changes = self.terrain_changes(plane, layer, grid, cells, proto, skip_unchanged)?;
        Ok(cell_operations(changes, proto))
    }
    /// The cells of one brush stroke that its `terrain.set` changes, each
    /// with its committed value, in stroke order. Dispatch of
    /// [`Action::PaintStroke`] and [`Session::stroke_changes`] both use this,
    /// so the live preview and the prepared transaction cannot differ.
    fn stroke_plan<'w>(
        &'w self,
        plane: &str,
        layer: i32,
        grid: &str,
        cells: &[[i64; 2]],
        proto: &str,
    ) -> Result<Vec<TerrainChange<'w>>, SessionError> {
        stroke_cells(cells)?;
        let changes = self.terrain_changes(plane, layer, grid, cells, proto, true)?;
        if changes.is_empty() {
            return Err(refuse(
                RefusalCode::ChangeNone,
                "brush stroke changes no cell: every covered cell already has this terrain",
            ));
        }
        Ok(changes)
    }
    /// The cells a running brush stroke would change if it were released
    /// now: exactly the `at` of every cell in the `terrain.set` that
    /// dispatching its [`Action::PaintStroke`] would prepare, in the same
    /// order, computed by the same code. Covered cells that already hold the
    /// stroke's terrain are not included. It reads only the committed World
    /// in memory and prepares nothing; release still revalidates the
    /// catalog and reloads the source, and an external change found then
    /// refuses the whole stroke. An error means the release would be
    /// refused too, including a stroke that changes no cell.
    pub fn stroke_changes(
        &self,
        stroke: &crate::brush::Stroke,
    ) -> Result<Vec<[i64; 2]>, SessionError> {
        if !self.can_prepare() {
            // The message predates the code; a blocked session says so by code.
            let code = if self.status.phase == SessionPhase::Blocked || self.history.needs_reload()
            {
                RefusalCode::SessionBlocked
            } else {
                RefusalCode::PendingExists
            };
            return Err(refuse(
                code,
                "review pending transaction: Accept or Cancel before another edit",
            ));
        }
        if stroke.expected() != self.history.document().revision() {
            return Err(refuse(RefusalCode::RevisionStale, "stale source revision"));
        }
        let cells: Vec<[i64; 2]> = stroke.cells().collect();
        let changes = match stroke.target() {
            crate::brush::StrokeTarget::Terrain(proto) => {
                self.stroke_plan(stroke.plane(), stroke.layer(), stroke.grid(), &cells, proto)?
            }
            crate::brush::StrokeTarget::Zone(zone) => {
                self.zone_plan(stroke.plane(), stroke.layer(), &cells, zone.as_deref())?
            }
        };
        Ok(changes.into_iter().map(|(at, _)| at).collect())
    }
    /// The cells of one zone stroke whose `zone` cell `zone` changes, each
    /// with its committed cell value (`None` for an empty cell), in stroke
    /// order. Dispatch of [`Action::PaintZones`] and
    /// [`Session::stroke_changes`] both use this. Cells are compared by their
    /// own value, not the zone they display: painting the default zone into
    /// an empty cell writes it explicitly, and so is a change. Cells of
    /// chunks that do not exist are skipped; a stroke left with no change is
    /// refused.
    fn zone_plan<'w>(
        &'w self,
        plane: &str,
        layer: i32,
        cells: &[[i64; 2]],
        zone: Option<&str>,
    ) -> Result<Vec<TerrainChange<'w>>, SessionError> {
        stroke_cells(cells)?;
        if !self.catalog.editing().zones.paint {
            return Err(refuse(
                RefusalCode::GrantMissing,
                "zone painting refused: the catalog does not grant editing.zones.paint",
            ));
        }
        let (p, l, _) = self.catalog.view();
        if plane != p || layer != l || !cells.iter().all(|&[x, y]| self.in_bounds(x, y)) {
            return Err(refuse(
                RefusalCode::ScopeOutsideView,
                "zone painting is limited to the active view",
            ));
        }
        let world = self.history.document().world();
        let plane = world
            .planes()
            .get(plane)
            .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown plane"))?;
        if let Some(zone) = zone
            && !plane.zones.contains_key(zone)
        {
            return Err(refuse(
                RefusalCode::TargetUnknown,
                "unknown zone: it is not in this plane's zones.jsonc",
            ));
        }
        if !cells
            .iter()
            .any(|&[x, y]| crate::zones::chunk_exists(plane, layer, x, y))
        {
            return Err(refuse(
                RefusalCode::ChangeNone,
                "zone stroke changes no cell: the covered tiles are in map chunks that do not exist",
            ));
        }
        let changes: Vec<TerrainChange<'w>> = cells
            .iter()
            // A tile whose chunk does not exist is left out like a tile
            // outside the view: painting it would create a chunk document
            // that holds only a zone grid.
            .filter(|&&[x, y]| crate::zones::chunk_exists(plane, layer, x, y))
            .filter_map(|&[x, y]| {
                let before = crate::zones::cell_zone(plane, layer, x, y);
                (before != zone).then_some(([x, y], before))
            })
            .collect();
        if changes.is_empty() {
            return Err(refuse(
                RefusalCode::ChangeNone,
                "zone stroke changes no cell: every covered cell already has this zone",
            ));
        }
        Ok(changes)
    }
    fn terrain_changes<'w>(
        &'w self,
        plane: &str,
        layer: i32,
        grid: &str,
        cells: &[[i64; 2]],
        proto: &str,
        skip_unchanged: bool,
    ) -> Result<Vec<TerrainChange<'w>>, SessionError> {
        let (p, l, _) = self.catalog.view();
        let in_view = plane == p && layer == l && cells.iter().all(|&[x, y]| self.in_bounds(x, y));
        let granted = self
            .catalog
            .editing()
            .terrain
            .get(grid)
            .is_some_and(|values| values.contains(proto));
        if !in_view || !granted {
            let code = if granted {
                RefusalCode::ScopeOutsideView
            } else {
                RefusalCode::GrantMissing
            };
            return Err(refuse(code, "terrain capability or bounds refused"));
        }
        let world = self.history.document().world();
        let index = world
            .profile()
            .ground_layers
            .iter()
            .position(|g| g == grid)
            .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown ground grid"))?;
        let mut changes = Vec::new();
        for &[x, y] in cells {
            let local = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32))
                .map_err(|_| refuse(RefusalCode::Internal, "cell overflow"))?;
            let before = world
                .planes()
                .get(plane)
                .and_then(|p| p.chunks.get(&(layer, x.div_euclid(32), y.div_euclid(32))))
                .and_then(|c| c.ground.get(index))
                .and_then(|g| g.value(local));
            if skip_unchanged && before == Some(proto) {
                continue;
            }
            changes.push(([x, y], before));
        }
        Ok(changes)
    }
    fn prepare(&mut self, expected: [u8; 32], op: &Value) -> Result<(), SessionError> {
        self.prepare_operations(expected, std::slice::from_ref(op))
    }
    fn prepare_operations(
        &mut self,
        expected: [u8; 32],
        operations: &[Value],
    ) -> Result<(), SessionError> {
        self.prepare_guarded_operations(expected, operations, None)
    }
    fn prepare_guarded_operations(
        &mut self,
        expected: [u8; 32],
        operations: &[Value],
        coordinates: Option<&CoordinateGuards>,
    ) -> Result<(), SessionError> {
        self.verify_revision(expected)?;
        let text = json!({"format":"tilewright.transaction/1","title":"Editor semantic action","actor":self.actor.as_str(),"operations":operations}).to_string();
        let text = if let Some(coordinates) = coordinates {
            coordinates.apply(text, &self.history.document().load_options().limits)?
        } else {
            text
        };
        // History seals the complete consulted file base and enforces full World revision.
        self.history
            .prepare(expected, "editor:action", &text)
            .map_err(|e| SessionError::history(&e))?;
        self.status.phase = SessionPhase::Pending;
        self.status.message = "Pending World diff; Accept publishes, Cancel writes nothing".into();
        self.invalidate_frame()
    }
    pub fn dispatch(
        &mut self,
        action: Action,
        recovery: &mut impl RecoveryStore,
    ) -> Result<(), SessionError> {
        let result = self.dispatch_inner(action, recovery);
        self.refresh_review();
        if self.history.pending().is_none() {
            self.placing = None;
        }
        if let Err(error) = &result {
            self.status.message = error.to_string();
            if self.status.phase != SessionPhase::Blocked {
                self.status.phase = if self.history.needs_reload() {
                    SessionPhase::Blocked
                } else if self.history.pending().is_some() {
                    SessionPhase::Pending
                } else {
                    SessionPhase::Ready
                };
            }
        }
        result
    }
    /// Recompute the notes, warnings and review of the pending transaction.
    /// Only dispatch changes the pending transaction, so this runs after it.
    fn refresh_review(&mut self) {
        self.warnings = self.history.pending().map_or_else(Vec::new, |pending| {
            let committed = self.history.document().world();
            let mut lines = crate::insertion::notes(committed, pending.sealed());
            lines.extend(crate::zones::notes(committed, pending.sealed()));
            lines.extend(crate::zones::definition_notes(committed, pending.world()));
            lines.extend(crate::anchors::notes(committed, pending.world()));
            lines.extend(crate::portals::notes(
                committed,
                pending.world(),
                &self.catalog,
            ));
            lines.extend(crate::removal::warnings(committed, pending.sealed()));
            lines
        });
        self.review = Review::of(self.history.pending(), &self.warnings);
    }
    #[allow(clippy::too_many_lines)]
    fn dispatch_inner(
        &mut self,
        action: Action,
        recovery: &mut impl RecoveryStore,
    ) -> Result<(), SessionError> {
        if matches!(action, Action::Reopen) {
            return self.inspect_reopen();
        }
        if self.status.phase == SessionPhase::Blocked || self.history.needs_reload() {
            return Err(refuse(
                RefusalCode::SessionBlocked,
                "session blocked: inspect recovery and reopen explicitly",
            ));
        }
        // A pending transaction is the exact diff the author is reviewing.
        // History's general preparation API can replace it, so fence every
        // preparation action here before any call can discard that evidence.
        if self.history.pending().is_some()
            && matches!(
                action,
                Action::Move { .. }
                    | Action::SetField { .. }
                    | Action::Paint { .. }
                    | Action::PaintStroke { .. }
                    | Action::PaintZones { .. }
                    | Action::Remove { .. }
                    | Action::Place { .. }
                    | Action::RenameZone { .. }
                    | Action::SetZoneParent { .. }
                    | Action::CreateZone { .. }
                    | Action::DeleteZone { .. }
                    | Action::CreateAnchor { .. }
                    | Action::MoveAnchor { .. }
                    | Action::DeleteAnchor { .. }
                    | Action::SetPortal { .. }
                    | Action::Undo
                    | Action::Redo
            )
        {
            return Err(refuse(
                RefusalCode::PendingExists,
                "review pending transaction: Accept or Cancel before another edit",
            ));
        }
        if !matches!(action, Action::Cancel | Action::ClearSelection) {
            self.verify_catalog()?;
        }
        match action {
            Action::ClearSelection => {
                self.selection = None;
                Ok(())
            }
            Action::Select { generation, key } => {
                self.verify_revision(self.history.document().revision())?;
                if generation != self.generation || !self.valid_key(&key) {
                    return Err(refuse(
                        RefusalCode::SelectionStale,
                        "stale frame or unknown semantic selection",
                    ));
                }
                self.selection = Some(key);
                Ok(())
            }
            Action::Move {
                uid,
                x,
                y,
                expected,
            } => {
                let world = self.history.document().world();
                let code = if self.catalog.can_move(world, &uid) {
                    // Granted: only the destination can be outside the view.
                    (!self.in_bounds(x, y)).then_some(RefusalCode::ScopeOutsideView)
                } else {
                    Some(self.object_grant_refusal(&uid, |_| true))
                };
                if let Some(code) = code {
                    return Err(refuse(code, "movement capability or bounds refused"));
                }
                let object = world
                    .object(&uid)
                    .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown UID"))?;
                let location = world
                    .object_location(&uid)
                    .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown UID location"))?;
                // The catalog refuses carried `movable_uids` when it loads
                // and revalidates before every dispatch, so a carried object
                // cannot reach this check today; it stays coded like every
                // other carried target.
                let code = match crate::grants::move_scope(&self.catalog, location) {
                    crate::grants::MoveScope::InView => None,
                    crate::grants::MoveScope::OtherPlaneOrLayer => {
                        Some(RefusalCode::ScopeOutsideView)
                    }
                    crate::grants::MoveScope::Carried => Some(RefusalCode::TargetUnsupported),
                };
                if let Some(code) = code {
                    return Err(refuse(
                        code,
                        "only placed objects in the active spatial layer can move",
                    ));
                }
                if location.chunk.1 == x.div_euclid(32) && location.chunk.2 == y.div_euclid(32) {
                    let chunk = world
                        .planes()
                        .get(&location.plane)
                        .and_then(|plane| plane.chunks.get(&location.chunk))
                        .ok_or_else(|| {
                            refuse(
                                RefusalCode::SourceInvalid,
                                "missing authoritative object chunk",
                            )
                        })?;
                    let old_at = object.at.ok_or_else(|| {
                        refuse(
                            RefusalCode::TargetUnsupported,
                            "placed object has no coordinates",
                        )
                    })?;
                    let at = [
                        u16::try_from(x.rem_euclid(32))
                            .map_err(|_| refuse(RefusalCode::Internal, "local x overflow"))?,
                        u16::try_from(y.rem_euclid(32))
                            .map_err(|_| refuse(RefusalCode::Internal, "local y overflow"))?,
                    ];
                    let index = location.path[0];
                    // World 0.2 forbids object-relative set of `at`. A document
                    // pointer plus UID/prototype/old-value tests provides the
                    // supported order-preserving transaction for this chunk.
                    let mut operations = vec![
                        json!({"op":"test","object":{"uid":uid},"pointer":["proto"],"value":object.proto}),
                        json!({"op":"test","path":chunk.path,"pointer":["objects",index,"uid"],"value":uid}),
                        json!({"op":"test","path":chunk.path,"pointer":["objects",index,"at"],"value":old_at}),
                    ];
                    // Replace only changed scalar leaves, preserving array
                    // comments, whitespace and the untouched axis's spelling.
                    // A no-op remains tests only, with no recovery record.
                    for axis in 0..2 {
                        if old_at[axis] != at[axis] {
                            operations.push(json!({"op":"set","path":chunk.path,"pointer":["objects",index,"at",axis],"expect":old_at[axis],"value":at[axis]}));
                        }
                    }
                    self.verify_revision(expected)?;
                    let guards = self.coordinate_guards(&uid, old_at, at)?;
                    self.prepare_guarded_operations(expected, &operations, Some(&guards))
                } else {
                    // Crossing chunks uses the released relocation operation;
                    // exporters may refuse order changes they cannot represent.
                    self.prepare(expected, &json!({"op":"object.move","uid":uid,"expect":{"proto":object.proto},"to":{"at":[x,y]}}))
                }
            }
            Action::SetField {
                uid,
                field,
                value,
                expected,
            } => {
                bounded_value(&value)?;
                let world = self.history.document().world();
                if !self.catalog.can_edit_field(world, &uid, &field) {
                    return Err(refuse(
                        self.object_grant_refusal(&uid, |grant| grant.fields.contains(&field)),
                        "field capability or value budget refused",
                    ));
                }
                if world.profile().object_fields.field(&field).is_none() {
                    return Err(refuse(
                        RefusalCode::TargetUnknown,
                        "field absent from authoritative schema",
                    ));
                }
                let object = world
                    .object(&uid)
                    .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown UID"))?;
                if object.fields.is_empty() {
                    self.verify_revision(expected)?;
                    if self.inspect_object_source(&uid, |_, object| {
                        Ok(object.get("fields").is_none())
                    })? {
                        // Create only a proven-absent parent. Never replace an
                        // existing empty container, which can contain comments.
                        return self.prepare_operations(expected, &[
                            json!({"op":"test","object":{"uid":uid},"pointer":["fields"],"absent":true}),
                            json!({"op":"set","object":{"uid":uid},"pointer":["fields"],"value":{(field):value}}),
                        ]);
                    }
                }
                let mut op = json!({"op":"set","object":{"uid":uid},"pointer":["fields",field],"value":value});
                if let Some((_, before)) = object.fields.iter().find(|(name, _)| name == &field) {
                    op["expect"] = field_json(before)?;
                }
                self.prepare(expected, &op)
            }
            Action::Paint {
                plane,
                layer,
                grid,
                x,
                y,
                proto,
                expected,
            } => {
                let cells = self.terrain_cells(&plane, layer, &grid, &[[x, y]], &proto, false)?;
                self.prepare(expected,&json!({"op":"terrain.set","plane":plane,"layer":layer,"grid":grid,"cells":cells}))
            }
            Action::PaintStroke {
                plane,
                layer,
                grid,
                cells,
                proto,
                expected,
            } => {
                let cells = cell_operations(
                    self.stroke_plan(&plane, layer, &grid, &cells, &proto)?,
                    &proto,
                );
                // One gesture is one operation, so review, recovery, undo and
                // redo each treat the whole stroke as a single World change.
                self.prepare(expected,&json!({"op":"terrain.set","plane":plane,"layer":layer,"grid":grid,"cells":cells}))
            }
            Action::PaintZones {
                plane,
                layer,
                cells,
                zone,
                expected,
            } => {
                let cells: Vec<Value> = self
                    .zone_plan(&plane, layer, &cells, zone.as_deref())?
                    .into_iter()
                    .map(|(at, before)| json!({"at":at,"value":zone,"expect":before}))
                    .collect();
                // The pick grid is a ground layer; the edited grid is `zone`.
                self.prepare(
                    expected,
                    &json!({"op":"terrain.set","plane":plane,"layer":layer,"grid":"zone","cells":cells}),
                )
            }
            Action::Remove { uid, expected } => {
                let world = self.history.document().world();
                let object = world
                    .object(&uid)
                    .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown UID"))?;
                if !self.catalog.editing().objects.contains_key(&object.proto) {
                    return Err(refuse(
                        RefusalCode::GrantMissing,
                        "deletion capability refused: the catalog does not grant this object's prototype",
                    ));
                }
                // The same active-view scope as every per-prototype grant.
                if !self.catalog.can_delete(world, &uid) {
                    return Err(refuse(
                        self.object_grant_refusal(&uid, |_| true),
                        "only placed objects in the active view can be deleted",
                    ));
                }
                // Addressed by uid and guarded by prototype, like the
                // cross-chunk move: the inverse reinserts the exact bytes.
                self.prepare(
                    expected,
                    &json!({"op":"object.remove","uid":uid,"expect":{"proto":object.proto}}),
                )
            }
            Action::Place {
                plane,
                layer,
                x,
                y,
                proto,
                expected,
            } => self.place(&plane, layer, [x, y], &proto, expected),
            action @ (Action::RenameZone { .. }
            | Action::SetZoneParent { .. }
            | Action::CreateZone { .. }
            | Action::DeleteZone { .. }) => self.manage_zones(&action),
            action @ (Action::CreateAnchor { .. }
            | Action::MoveAnchor { .. }
            | Action::DeleteAnchor { .. }) => self.edit_anchors(&action),
            Action::SetPortal {
                uid,
                target,
                expected,
            } => self.edit_portal(&uid, target.as_ref(), expected),
            Action::Accept => {
                // The engine's own gate: a front end cannot accept a change
                // whose review was cut short, whatever it displays.
                if !self.review.complete {
                    return Err(refuse(RefusalCode::ReviewIncomplete, REVIEW_LIMIT_MESSAGE));
                }
                self.accept(recovery)
            }
            Action::Cancel => {
                self.history.cancel();
                self.revalidate_selection();
                self.status.phase = SessionPhase::Ready;
                self.status.message = "Pending action cancelled; no publication".into();
                self.invalidate_frame()
            }
            Action::Undo | Action::Redo => {
                self.verify_revision(self.history.document().revision())?;
                let result = if matches!(action, Action::Undo) {
                    self.history.prepare_undo()
                } else {
                    self.history.prepare_redo()
                };
                result.map_err(|e| SessionError::history(&e))?;
                self.status.phase = SessionPhase::Pending;
                self.status.message = "History transaction pending; review and Accept".into();
                self.invalidate_frame()
            }
            Action::Reopen => unreachable!(),
        }
    }
    /// One `object.insert` of a granted prototype on a tile of the active
    /// view, appended on top of the tile's stack, with a new unused uid,
    /// the grant's render layer and its defaults.
    fn place(
        &mut self,
        plane: &str,
        layer: i32,
        [x, y]: [i64; 2],
        proto: &str,
        expected: [u8; 32],
    ) -> Result<(), SessionError> {
        let grant = self.catalog.editing().objects.get(proto).ok_or_else(|| {
            refuse(
                RefusalCode::GrantMissing,
                "placement capability refused: the catalog does not grant this prototype",
            )
        })?;
        if !self.catalog.tile_in_active_view(plane, layer, x, y) {
            return Err(refuse(
                RefusalCode::ScopeOutsideView,
                "objects can only be placed on tiles of the active view",
            ));
        }
        self.verify_revision(expected)?;
        let world = self.history.document().world();
        let uid = crate::uid::unused_uid(self.uids.as_mut(), |uid| {
            world.object_location(uid).is_some()
        })
        .map_err(|error| SessionError::new(RefusalCode::UidUnavailable, error.to_string()))?;
        // Written as text so the placed object keeps this member order in
        // source: uid, prototype, render layer, then the defaults in catalog
        // order. Every value is JSON-encoded; defaults are canonical JSON.
        let encode = |text: &str| Value::String(text.into()).to_string();
        let mut object = format!(
            "{{\"uid\":{},\"proto\":{},\"layer\":{}",
            encode(&uid),
            encode(proto),
            encode(&grant.layer)
        );
        if !grant.defaults.is_empty() {
            object.push_str(",\"fields\":{");
            for (index, (name, value)) in grant.defaults.iter().enumerate() {
                if index > 0 {
                    object.push(',');
                }
                object.push_str(&encode(name));
                object.push(':');
                object.push_str(value);
            }
            object.push('}');
        }
        object.push('}');
        let operation = format!(
            "{{\"op\":\"object.insert\",\"plane\":{},\"layer\":{layer},\"at\":[{x},{y}],\"object\":{object}}}",
            encode(plane)
        );
        let text = format!(
            "{{\"format\":\"tilewright.transaction/1\",\"title\":\"Editor semantic action\",\"actor\":{},\"operations\":[{operation}]}}",
            encode(self.actor.as_str())
        );
        // History seals the complete consulted file base and enforces full
        // World revision; World validates the new object, defaults included.
        self.history
            .prepare(expected, "editor:action", &text)
            .map_err(|error| {
                SessionError::new(
                    RefusalCode::of_history(&error),
                    format!("placement refused: {error}"),
                )
            })?;
        self.placing = Some(uid);
        self.status.phase = SessionPhase::Pending;
        self.status.message = "Pending World diff; Accept publishes, Cancel writes nothing".into();
        self.invalidate_frame()
    }
    fn accept(&mut self, recovery: &mut impl RecoveryStore) -> Result<(), SessionError> {
        self.verify_revision(self.history.document().revision())?;
        let changed = self
            .history
            .pending()
            .is_some_and(|pending| pending.changed().next().is_some());
        self.status.applied = None;
        self.status.durable = None;
        self.status.cleaned = None;
        self.status.phase = SessionPhase::Retaining;
        // Existing History calls retain immediately before publish. The wrapper
        // advances the session phase only when retention has succeeded.
        let mut retaining = Retaining {
            store: recovery,
            phase: &mut self.status.phase,
        };
        let result = self
            .history
            .commit(&mut retaining, &tilewright_edit::PublishOptions::default());
        match result {
            Ok(published) => {
                self.status.phase = SessionPhase::Ready;
                self.status.applied = Some(changed);
                self.status.durable = Some(published.durable);
                self.status.cleaned = Some(true);
                self.status.message =
                    "Published World workspace and reloaded verified result".into();
                // An accepted placement selects the new object by its uid,
                // the same semantic key a click on it would give.
                if let Some(uid) = self.placing.take() {
                    self.selection = Some(SelectionKey::Object { uid });
                }
                self.revalidate_selection();
                self.invalidate_frame()
            }
            Err(error) => {
                self.status.message = error.to_string();
                match &error {
                    tilewright_editor_commands::Error::Publish(e) => {
                        self.status.applied = Some(e.applied);
                        self.status.durable = Some(e.durable);
                        self.status.cleaned = Some(e.cleaned);
                    }
                    tilewright_editor_commands::Error::PublishedStateChanged {
                        published, ..
                    } => {
                        self.status.applied = Some(true);
                        self.status.durable = Some(published.durable);
                        self.status.cleaned = Some(true);
                    }
                    _ => {}
                }
                self.status.phase = if self.history.needs_reload() {
                    SessionPhase::Blocked
                } else if self.history.pending().is_some() {
                    SessionPhase::Pending
                } else {
                    SessionPhase::Ready
                };
                Err(SessionError::history(&error))
            }
        }
    }
    fn inspect_reopen(&mut self) -> Result<(), SessionError> {
        let reopened = match self.history.document().reload() {
            Ok(document) => document,
            Err(error) => {
                self.status.phase = SessionPhase::Blocked;
                return Err(SessionError::new(
                    RefusalCode::SourceInvalid,
                    format!(
                        "reopen inspection failed: {error}; preserve recovery and inspect source"
                    ),
                ));
            }
        };
        if let Err(error) = self.verify_catalog() {
            self.status.phase = SessionPhase::Blocked;
            return Err(error);
        }
        self.invalidate_frame()?;
        if self.history.needs_reload() || reopened.revision() != self.history.document().revision()
        {
            self.status.phase = SessionPhase::Blocked;
            self.status.message="Reopen inspection complete; source differs or publication uncertain. Preserve recovery; open a fresh explicitly inspected session".into();
            return Err(refuse(RefusalCode::ReopenBlocked, &self.status.message));
        }
        self.selection = None;
        self.status.message = "Reopen verified committed source revision".into();
        Ok(())
    }
}
/// A stroke's cells: at least one, at most the stroke cap, distinct and in
/// order.
fn stroke_cells(cells: &[[i64; 2]]) -> Result<(), SessionError> {
    if cells.is_empty() || cells.len() > crate::brush::MAXIMUM_STROKE_CELLS {
        return Err(refuse(
            RefusalCode::InputBudget,
            "brush stroke cell count refused",
        ));
    }
    if !cells.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(refuse(
            RefusalCode::InputInvalid,
            "brush stroke cells must be distinct and ordered",
        ));
    }
    Ok(())
}

/// `terrain.set` cell entries setting `proto`, each expecting its value.
fn cell_operations(changes: Vec<TerrainChange<'_>>, proto: &str) -> Vec<Value> {
    changes
        .into_iter()
        .map(|(at, before)| json!({"at":at,"value":proto,"expect":before}))
        .collect()
}

fn field_json(value: &FieldValue) -> Result<Value, SessionError> {
    match value {
        FieldValue::Null => Ok(Value::Null),
        FieldValue::Bool(v) => Ok(json!(v)),
        FieldValue::Integer(v) => Ok(json!(v)),
        FieldValue::Number(v) => Ok(json!(v)),
        FieldValue::String(v) => Ok(json!(v)),
        FieldValue::Json(v) if v.len() <= 65536 => serde_json::from_str(v).map_err(internal),
        FieldValue::Json(_) => Err(refuse(
            RefusalCode::InputBudget,
            "existing field exceeds semantic editing budget",
        )),
    }
}

struct Retaining<'a, R> {
    store: &'a mut R,
    phase: &'a mut SessionPhase,
}
impl<R: RecoveryStore> RecoveryStore for Retaining<'_, R> {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.store.retain(inverse)?;
        *self.phase = SessionPhase::Publishing;
        Ok(())
    }
}

/// Bound arbitrary action values before serde allocates their escaped encoding.
fn bounded_value(value: &Value) -> Result<(), SessionError> {
    let mut pending = vec![(value, 0usize)];
    let mut bytes = 0usize;
    let mut count = 0usize;
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if depth > 8 || count > 4096 {
            return Err(refuse(
                RefusalCode::InputBudget,
                "field value nesting or entry budget exceeded",
            ));
        }
        let size = match value {
            Value::String(text) => text.len().checked_mul(6).and_then(|v| v.checked_add(3)),
            Value::Array(values) => {
                if values.len() > 4096usize.saturating_sub(count + pending.len()) {
                    return Err(refuse(
                        RefusalCode::InputBudget,
                        "field value entry budget exceeded",
                    ));
                }
                pending.extend(values.iter().map(|v| (v, depth + 1)));
                Some(values.len() + 2)
            }
            Value::Object(values) => {
                if values.len() > 4096usize.saturating_sub(count + pending.len()) {
                    return Err(refuse(
                        RefusalCode::InputBudget,
                        "field value entry budget exceeded",
                    ));
                }
                let mut keys = Some(2usize);
                for (key, value) in values {
                    keys = keys.and_then(|n| {
                        key.len()
                            .checked_mul(6)
                            .and_then(|k| n.checked_add(k))
                            .and_then(|n| n.checked_add(4))
                    });
                    pending.push((value, depth + 1));
                }
                keys
            }
            _ => Some(32),
        };
        bytes = size
            .and_then(|size| bytes.checked_add(size))
            .filter(|size| *size <= 65536)
            .ok_or_else(|| {
                refuse(
                    RefusalCode::InputBudget,
                    "field value encoded byte budget exceeded",
                )
            })?;
    }
    Ok(())
}
