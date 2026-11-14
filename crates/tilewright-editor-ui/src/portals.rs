// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only portal review notes and the anchor list portal editing offers.
//! A portal is one-ended: it sits on one object, and a way back is another
//! object's own portal, so a target tile without a way back is normal and
//! never warned about. World checks that a target plane and a target anchor
//! exist; it does not check that a location target has a map chunk or
//! ground, so the review warns about those. Nothing here edits anything or
//! validates World data beyond what a warning needs.
use crate::anchors::{PortalHolder, Target, portals, short, target_tile};
use std::collections::BTreeMap;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::World;

/// At most this many portal changes of one transaction are described.
const MAXIMUM_DESCRIBED: usize = 8;

/// The grid tiles are picked on when the view opens: the first grid the
/// catalog grants terrain editing on, else the profile's first ground grid
/// (the same rule as `PreviewCatalog::default_view`).
pub fn pick_grid<'a>(world: &'a World, catalog: &'a PreviewCatalog) -> Option<&'a str> {
    catalog
        .editing()
        .terrain
        .keys()
        .next()
        .or_else(|| world.profile().ground_layers.first())
        .map(String::as_str)
}

/// A portal target in words: `anchor arrival (main, layer 0, (7, 3))`, or
/// `main, layer 0, (7, 3)` for a location.
pub fn describe(world: &World, target: &Target<'_>) -> String {
    match *target {
        Target::Location { plane, layer, at } => {
            format!("{}, layer {layer}, ({}, {})", short(plane), at[0], at[1])
        }
        Target::Anchor { plane, anchor } => {
            let place = world
                .planes()
                .get(plane)
                .and_then(|source| source.anchors.get(anchor))
                .map_or_else(
                    || format!("{}, not defined", short(plane)),
                    |found| {
                        format!(
                            "{}, layer {}, ({}, {})",
                            short(plane),
                            found.layer,
                            found.at[0],
                            found.at[1]
                        )
                    },
                );
            format!("anchor {} ({place})", short(anchor))
        }
    }
}

/// What is wrong, or worth saying, about where a portal leads in the
/// loaded World. World checks that a target plane and a target anchor
/// exist; it does not check that a location target has a map chunk or
/// ground, so these are the editor's own checks. A portal is one-ended, so a
/// target without a way back is never reported.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetCheck<'w> {
    /// The portal leads to the tile its own object stands on.
    pub own_tile: bool,
    /// For a location target on a plane the World has: what it lacks.
    pub location: Option<LocationCheck<'w>>,
}

/// The checks of a location target on a plane the World has.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocationCheck<'w> {
    /// The map chunk holding the target tile does not exist.
    pub missing_chunk: bool,
    /// The chunk exists but has no ground on this grid at the target tile.
    pub missing_ground: Option<&'w str>,
    /// The target is not on `view`'s plane and spatial layer.
    pub outside_view: bool,
}

/// Checks where `holder`'s portal leads in `world`: whether it is the
/// object's own tile and, for a location target, whether its chunk exists
/// and has ground on `grid` (the pick grid, see [`pick_grid`]). `view` is
/// the plane and spatial layer the caller shows, if any; a target elsewhere
/// is checked the same way.
pub fn check_target<'w>(
    world: &'w World,
    grid: Option<&'w str>,
    view: Option<(&str, i32)>,
    holder: &PortalHolder<'_>,
) -> TargetCheck<'w> {
    let mut check = TargetCheck::default();
    let Some((plane, layer, at)) = target_tile(world, &holder.target) else {
        return check;
    };
    check.own_tile = plane == holder.plane && layer == holder.layer && holder.tile == Some(at);
    let Target::Location { .. } = holder.target else {
        return check;
    };
    let Some(source) = world.planes().get(plane) else {
        return check;
    };
    let mut location = LocationCheck {
        outside_view: view
            .is_some_and(|(view_plane, view_layer)| plane != view_plane || layer != view_layer),
        ..LocationCheck::default()
    };
    let size = i64::from(source.chunk_size);
    let chunk = (size > 0)
        .then(|| {
            source
                .chunks
                .get(&(layer, at[0].div_euclid(size), at[1].div_euclid(size)))
        })
        .flatten();
    match chunk {
        None => location.missing_chunk = true,
        Some(chunk) => {
            if let Some(grid) = grid {
                let index = world.profile().ground_layers.iter().position(|g| g == grid);
                let local =
                    usize::try_from(at[1].rem_euclid(size) * size + at[0].rem_euclid(size)).ok();
                let ground = index
                    .zip(local)
                    .and_then(|(index, local)| chunk.ground.get(index)?.value(local));
                if ground.is_none() {
                    location.missing_ground = Some(grid);
                }
            }
        }
    }
    check.location = Some(location);
    check
}

/// Warnings and notes about where `holder`'s portal now leads in `world`:
/// the object's own tile, and for a location target a missing chunk or
/// missing ground on the pick grid. A target on another plane or layer is
/// checked the same way in the loaded world, which the view cannot show.
fn target_lines(
    world: &World,
    catalog: &PreviewCatalog,
    holder: &PortalHolder<'_>,
    name: &str,
) -> Vec<String> {
    let (view_plane, view_layer, _) = catalog.view();
    let check = check_target(
        world,
        pick_grid(world, catalog),
        Some((view_plane, view_layer)),
        holder,
    );
    let mut lines = Vec::new();
    if check.own_tile {
        lines.push(format!(
            "Warning: {name}'s portal leads to the tile the object stands on."
        ));
    }
    let Some(location) = check.location else {
        return lines;
    };
    if location.missing_chunk {
        lines.push(format!(
            "Warning: {name}'s portal target is in a map chunk that does not exist; World does not check portal targets."
        ));
    }
    if let Some(grid) = location.missing_ground {
        lines.push(format!(
            "Warning: {name}'s portal target has no ground on the {} grid; World does not check portal targets.",
            short(grid)
        ));
    }
    if location.outside_view {
        lines.push(not_shown());
    }
    lines
}

fn not_shown() -> String {
    "The target is not on the view's plane and layer: it was checked in the loaded world, but the view cannot show it.".into()
}

/// The objects with a uid whose portal is placed or carried in `world`, by
/// uid. Objects without a uid cannot be edited here.
fn by_uid(world: &World) -> BTreeMap<&str, PortalHolder<'_>> {
    portals(world)
        .into_iter()
        .filter_map(|holder| holder.uid.map(|uid| (uid, holder)))
        .collect()
}

/// Review notes for every object whose portal the pending World adds,
/// changes or clears against the committed World, in uid order, with the
/// warnings about each new target. They compare portals, not operations,
/// so they describe undo and redo too, and they never block Accept. An
/// object the pending World adds or removes is left to the placement and
/// deletion notes.
pub(crate) fn notes(committed: &World, pending: &World, catalog: &PreviewCatalog) -> Vec<String> {
    let (before, after) = (by_uid(committed), by_uid(pending));
    let mut uids: Vec<&str> = before.keys().chain(after.keys()).copied().collect();
    uids.sort_unstable();
    uids.dedup();
    let mut lines = Vec::new();
    let mut described = 0;
    for uid in uids {
        let name = short(uid);
        let change = match (before.get(uid), after.get(uid)) {
            (Some(old), Some(new)) if old.target != new.target => {
                let mut change = vec![format!(
                    "{name} now leads to {}; it led to {}.",
                    describe(pending, &new.target),
                    describe(committed, &old.target)
                )];
                change.extend(target_lines(pending, catalog, new, &name));
                change
            }
            (None, Some(new)) if committed.object(uid).is_some() => {
                let mut change = vec![format!(
                    "{name} now has a portal to {}.",
                    describe(pending, &new.target)
                )];
                change.extend(target_lines(pending, catalog, new, &name));
                change
            }
            (Some(old), None) if pending.object(uid).is_some() => vec![format!(
                "{name} no longer has a portal; it led to {}.",
                describe(committed, &old.target)
            )],
            _ => Vec::new(),
        };
        if change.is_empty() {
            continue;
        }
        if described == MAXIMUM_DESCRIBED {
            lines.push("Further portal changes are not described here.".into());
            return lines;
        }
        described += 1;
        lines.extend(change);
    }
    lines
}

/// At most this many anchors are offered as portal targets.
pub const MAXIMUM_TARGET_ANCHORS: usize = 256;

/// One anchor a portal may target: its plane, id, layer and tile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetAnchor<'w> {
    pub plane: &'w str,
    pub anchor: &'w str,
    pub layer: i32,
    pub at: [i64; 2],
}

/// The anchors a portal may target, grouped by plane: the view's plane
/// first, then the other planes in id order, each plane's anchors in id
/// order, at most [`MAXIMUM_TARGET_ANCHORS`] in all. Returns the listed
/// anchors and the real total in the world.
pub fn target_anchors<'w>(
    world: &'w World,
    catalog: &PreviewCatalog,
) -> (Vec<TargetAnchor<'w>>, usize) {
    let view = catalog.view().0;
    let planes = world
        .planes()
        .iter()
        .filter(|(id, _)| id.as_str() == view)
        .chain(world.planes().iter().filter(|(id, _)| id.as_str() != view));
    let mut listed = Vec::new();
    let mut total = 0;
    for (plane, source) in planes {
        total += source.anchors.len();
        for (anchor, found) in &source.anchors {
            if listed.len() < MAXIMUM_TARGET_ANCHORS {
                listed.push(TargetAnchor {
                    plane,
                    anchor,
                    layer: found.layer,
                    at: found.at,
                });
            }
        }
    }
    (listed, total)
}
