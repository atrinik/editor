// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Object grants exactly as `Session` applies them, for callers that say
//! what an edit would be allowed without dispatching it. Deletion, portal
//! editing and field edits need nothing beyond the catalog's own
//! predicates (`PreviewCatalog::can_delete`, `can_edit_portal`,
//! `can_edit_field` and `editable_fields`), which `Session` calls as they
//! are. A move also needs the object placed on the view's plane and spatial
//! layer, which a per-uid grant alone does not ensure.
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::{ObjectLocation, World};

/// Where an object is, for a move: only a placed object on the active
/// view's plane and spatial layer can move.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MoveScope {
    /// Placed on the view's plane and spatial layer.
    InView,
    /// Placed, but on another plane or spatial layer.
    OtherPlaneOrLayer,
    /// Carried in an inventory.
    Carried,
}

/// The [`MoveScope`] of the object at `location`.
pub fn move_scope(catalog: &PreviewCatalog, location: &ObjectLocation) -> MoveScope {
    let (plane, layer, _) = catalog.view();
    if location.path.len() != 1 {
        MoveScope::Carried
    } else if location.plane != plane || location.chunk.0 != layer {
        MoveScope::OtherPlaneOrLayer
    } else {
        MoveScope::InView
    }
}

/// Whether `Session` would move `uid` to some tile of the active view: the
/// catalog grants it (`PreviewCatalog::can_move`) and it is placed on the
/// view's plane and spatial layer.
pub fn can_move(world: &World, catalog: &PreviewCatalog, uid: &str) -> bool {
    catalog.can_move(world, uid)
        && world
            .object_location(uid)
            .is_some_and(|location| move_scope(catalog, location) == MoveScope::InView)
}
