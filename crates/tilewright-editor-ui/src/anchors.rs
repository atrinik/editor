// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only anchor and portal resolution for display, refusals and review
//! notes. An anchor is `{id, layer, at}` in its plane's `anchors.jsonc`; a
//! portal is the `portal` member of a chunk object, at any inventory depth,
//! and targets either a location or an anchor. A portal without `plane`
//! targets its own object's plane. Nothing in this module edits anything,
//! and nothing here validates World data: World refuses a portal that names
//! a missing anchor or plane, and this module only reads what it loaded.
use std::collections::{BTreeMap, BTreeSet};
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::{Object, Plane, Portal, World};

/// Where a portal leads, with its plane resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target<'w> {
    Location {
        plane: &'w str,
        layer: i32,
        at: [i64; 2],
    },
    Anchor {
        plane: &'w str,
        anchor: &'w str,
    },
}

/// The target of `portal` on an object in plane `own`: a portal without a
/// `plane` member targets its own object's plane.
pub fn resolve<'w>(own: &'w str, portal: &'w Portal) -> Target<'w> {
    match portal {
        Portal::Location { plane, layer, at } => Target::Location {
            plane: plane.as_deref().unwrap_or(own),
            layer: *layer,
            at: *at,
        },
        Portal::Anchor { plane, anchor } => Target::Anchor {
            plane: plane.as_deref().unwrap_or(own),
            anchor,
        },
    }
}

/// One object that holds a portal, and where it is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalHolder<'w> {
    pub plane: &'w str,
    pub layer: i32,
    /// The plane tile of the placed object, or of the placed object whose
    /// inventory holds this one.
    pub tile: Option<[i64; 2]>,
    pub uid: Option<&'w str>,
    /// 0 for a placed object, 1 for an entry in its inventory, and so on.
    pub depth: usize,
    pub target: Target<'w>,
}

/// The plane tile of chunk-local `at` in chunk `(cx, cy)`.
fn plane_tile(size: i64, cx: i64, cy: i64, at: [u16; 2]) -> Option<[i64; 2]> {
    Some([
        cx.checked_mul(size)?.checked_add(i64::from(at[0]))?,
        cy.checked_mul(size)?.checked_add(i64::from(at[1]))?,
    ])
}

/// Every object with a portal anywhere in the world: every plane, layer and
/// chunk, placed objects and inventory entries at any depth, with or
/// without a uid. In plane, chunk and source order, each placed object
/// before its inventory.
pub fn portals(world: &World) -> Vec<PortalHolder<'_>> {
    let mut holders = Vec::new();
    for (plane_id, plane) in world.planes() {
        let size = i64::from(plane.chunk_size);
        for (&(layer, cx, cy), chunk) in &plane.chunks {
            for placed in &chunk.objects {
                let tile = placed.at.and_then(|at| plane_tile(size, cx, cy, at));
                let mut pending: Vec<(&Object, usize)> = vec![(placed, 0)];
                while let Some((object, depth)) = pending.pop() {
                    if let Some(portal) = &object.portal {
                        holders.push(PortalHolder {
                            plane: plane_id,
                            layer,
                            tile,
                            uid: object.uid.as_deref(),
                            depth,
                            target: resolve(plane_id, portal),
                        });
                    }
                    pending.extend(
                        object
                            .inventory
                            .iter()
                            .rev()
                            .map(|entry| (entry, depth + 1)),
                    );
                }
            }
        }
    }
    holders
}

/// The objects whose portal names anchor `anchor` of plane `plane`.
pub fn references<'w>(world: &'w World, plane: &str, anchor: &str) -> Vec<PortalHolder<'w>> {
    portals(world)
        .into_iter()
        .filter(|holder| holder.target == Target::Anchor { plane, anchor })
        .collect()
}

/// How many portals name each anchor of plane `plane`, for every anchor
/// that at least one portal names.
pub fn reference_counts<'w>(world: &'w World, plane: &str) -> BTreeMap<&'w str, usize> {
    let mut counts = BTreeMap::new();
    for holder in portals(world) {
        if let Target::Anchor {
            plane: target,
            anchor,
        } = holder.target
            && target == plane
        {
            *counts.entry(anchor).or_default() += 1;
        }
    }
    counts
}

/// Most anchors of one tile a tile description lists; the rest are counted.
pub const MAXIMUM_TILE_ANCHORS: usize = 8;

/// The anchors of `plane` on spatial `layer` at plane tile `at`, sorted by
/// id.
pub fn tile_anchors(plane: &Plane, layer: i32, at: [i64; 2]) -> Vec<&str> {
    plane
        .anchors
        .iter()
        .filter(|(_, anchor)| anchor.layer == layer && anchor.at == at)
        .map(|(id, _)| id.as_str())
        .collect()
}

/// The plane, layer and tile `target` leads to, when it can be resolved: a
/// location as it is written, an anchor where its plane defines it.
pub fn target_tile<'w>(world: &'w World, target: &Target<'w>) -> Option<(&'w str, i32, [i64; 2])> {
    match *target {
        Target::Location { plane, layer, at } => Some((plane, layer, at)),
        Target::Anchor { plane, anchor } => world
            .planes()
            .get(plane)
            .and_then(|source| source.anchors.get(anchor))
            .map(|found| (plane, found.layer, found.at)),
    }
}

/// The anchors on tiles of the active view (its plane, spatial layer and
/// bounds), sorted by id, with their plane tile.
pub fn view_anchors<'w>(world: &'w World, catalog: &PreviewCatalog) -> Vec<(&'w str, [i64; 2])> {
    let (plane, layer, _) = catalog.view();
    world.planes().get(plane).map_or_else(Vec::new, |source| {
        source
            .anchors
            .iter()
            .filter(|(_, anchor)| {
                catalog.tile_in_active_view(plane, layer, anchor.at[0], anchor.at[1])
                    && anchor.layer == layer
            })
            .map(|(id, anchor)| (id.as_str(), anchor.at))
            .collect()
    })
}

/// The tiles of the active view that hold an object with a portal: a placed
/// object with one, or a placed object with one in its inventory.
pub fn view_portal_tiles(world: &World, catalog: &PreviewCatalog) -> BTreeSet<[i64; 2]> {
    let (plane, layer, _) = catalog.view();
    let Some(source) = world.planes().get(plane) else {
        return BTreeSet::new();
    };
    let size = i64::from(source.chunk_size);
    let mut tiles = BTreeSet::new();
    for (&(chunk_layer, cx, cy), chunk) in &source.chunks {
        if chunk_layer != layer {
            continue;
        }
        for placed in &chunk.objects {
            let Some([x, y]) = placed.at.and_then(|at| plane_tile(size, cx, cy, at)) else {
                continue;
            };
            if catalog.tile_in_active_view(plane, layer, x, y) && holds_portal(placed) {
                tiles.insert([x, y]);
            }
        }
    }
    tiles
}

/// Whether `object` or anything in its inventory has a portal.
fn holds_portal(object: &Object) -> bool {
    let mut pending = vec![object];
    while let Some(object) = pending.pop() {
        if object.portal.is_some() {
            return true;
        }
        pending.extend(&object.inventory);
    }
    false
}

/// At most this many referencing objects are named in one line.
pub const MAXIMUM_NAMED: usize = 3;
/// Displayed identifiers are cut to this many characters.
const MAXIMUM_NAME_CHARS: usize = 64;

/// Identifiers cut to 64 characters, the last one an ellipsis.
pub fn short(text: &str) -> String {
    if text.chars().count() <= MAXIMUM_NAME_CHARS {
        return text.into();
    }
    let mut cut: String = text.chars().take(MAXIMUM_NAME_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// `1 portal` or `N portals`.
pub fn portal_count(count: usize) -> String {
    if count == 1 {
        "1 portal".into()
    } else {
        format!("{count} portals")
    }
}

/// One referencing object, for a refusal or a review note: its uid (or that
/// it has none), its plane, layer and tile, and whether it sits in an
/// inventory.
pub fn describe_holder(holder: &PortalHolder<'_>) -> String {
    let name = holder.uid.map_or_else(
        || "an object without a uid".to_owned(),
        |uid| format!("object {}", short(uid)),
    );
    let place = holder.tile.map_or_else(
        || format!("{} layer {}", short(holder.plane), holder.layer),
        |[x, y]| format!("{} layer {} ({x}, {y})", short(holder.plane), holder.layer),
    );
    let nested = if holder.depth > 0 {
        ", in an inventory"
    } else {
        ""
    };
    format!("{name} on {place}{nested}")
}

/// One referencing object in a short form for a sidebar row: its uid (or
/// that it has none), plane, layer and tile, and whether it is carried in
/// an inventory.
pub fn describe_holder_short(holder: &PortalHolder<'_>) -> String {
    let name = holder.uid.map_or_else(|| "(no uid)".to_owned(), short);
    let tile = holder
        .tile
        .map_or_else(String::new, |[x, y]| format!(", ({x}, {y})"));
    let nested = if holder.depth > 0 { ", carried" } else { "" };
    format!(
        "{name}: {}, layer {}{tile}{nested}",
        short(holder.plane),
        holder.layer
    )
}

/// `N portals: a; b; c; and M more`, naming at most [`MAXIMUM_NAMED`].
pub fn describe_holders(holders: &[PortalHolder<'_>]) -> String {
    let mut named: Vec<String> = holders
        .iter()
        .take(MAXIMUM_NAMED)
        .map(describe_holder)
        .collect();
    if holders.len() > MAXIMUM_NAMED {
        named.push(format!("and {} more", holders.len() - MAXIMUM_NAMED));
    }
    format!("{}: {}", portal_count(holders.len()), named.join("; "))
}

/// A portal target in words, for the inspector.
pub fn describe_target(world: &World, target: &Target<'_>) -> String {
    match *target {
        Target::Location { plane, layer, at } => {
            format!("{} layer {layer} ({}, {})", short(plane), at[0], at[1])
        }
        Target::Anchor { plane, anchor } => {
            let place = world
                .planes()
                .get(plane)
                .and_then(|source| source.anchors.get(anchor))
                .map_or_else(
                    || " (not defined)".to_owned(),
                    |found| {
                        format!(
                            " = {} layer {} ({}, {})",
                            short(plane),
                            found.layer,
                            found.at[0],
                            found.at[1]
                        )
                    },
                );
            format!("anchor {}{place}", short(anchor))
        }
    }
}

/// At most this many anchor changes of one transaction are described.
const MAXIMUM_DESCRIBED: usize = 8;

/// Neutral review notes for every anchor the pending World creates, moves
/// or deletes against the committed World, in plane and id order, with the
/// portals that name a moved anchor and a warning that scripts or saves
/// outside the world may use its id. They compare anchors, not operations,
/// so they describe undo and redo too, and they never block Accept.
pub(crate) fn notes(committed: &World, pending: &World) -> Vec<String> {
    let mut lines = Vec::new();
    let planes: BTreeSet<&String> = committed
        .planes()
        .keys()
        .chain(pending.planes().keys())
        .collect();
    let empty = BTreeMap::new();
    let mut described = 0;
    for plane in planes {
        let before = committed.planes().get(plane).map_or(&empty, |p| &p.anchors);
        let after = pending.planes().get(plane).map_or(&empty, |p| &p.anchors);
        let ids: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for id in ids {
            let shown = short(id);
            let change = match (before.get(id), after.get(id)) {
                (None, Some(new)) => vec![format!(
                    "Creates anchor {shown} on layer {} at ({}, {}).",
                    new.layer, new.at[0], new.at[1]
                )],
                (Some(old), None) => vec![
                    format!(
                        "Deletes anchor {shown} (layer {}, ({}, {})); no portal in the world names it.",
                        old.layer, old.at[0], old.at[1]
                    ),
                    format!(
                        "Warning: scripts or saves outside the world may still use the anchor id {shown}; they will no longer find it."
                    ),
                ],
                (Some(old), Some(new)) if old != new => {
                    let mut change = vec![format!(
                        "Moves anchor {shown} from layer {} ({}, {}) to layer {} ({}, {}).",
                        old.layer, old.at[0], old.at[1], new.layer, new.at[0], new.at[1]
                    )];
                    let holders = references(committed, plane, id);
                    change.push(if holders.is_empty() {
                        "No portal in the world names it.".into()
                    } else {
                        format!(
                            "Portals naming it now lead to the new tile ({}).",
                            describe_holders(&holders)
                        )
                    });
                    change.push(format!(
                        "Warning: scripts or saves outside the world may refer to the anchor id {shown}; they will lead to the new tile too."
                    ));
                    change
                }
                _ => Vec::new(),
            };
            if change.is_empty() {
                continue;
            }
            if described == MAXIMUM_DESCRIBED {
                lines.push("Further anchor changes are not described here.".into());
                return lines;
            }
            described += 1;
            lines.extend(change);
        }
    }
    lines
}
