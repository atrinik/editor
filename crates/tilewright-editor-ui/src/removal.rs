// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only review warnings for pending object deletions. They inform the
//! author and never refuse or block the reviewed transaction.
use tilewright_source::{Object, Portal, World};

/// At most this many deletions in one transaction are described.
const MAXIMUM_DESCRIBED: usize = 8;
/// At most this many dependent names are listed per warning.
const MAXIMUM_NAMES: usize = 3;
/// Displayed identifiers are cut to this many characters.
const MAXIMUM_NAME_CHARS: usize = 64;

/// Warnings for each `object.remove` addressed by uid in `sealed`, judged
/// against the `committed` World that still holds the object.
pub(crate) fn warnings(committed: &World, sealed: &str) -> Vec<String> {
    let limits = tilewright_jsonc::Limits::default();
    let Ok(document) = tilewright_jsonc::Document::parse(sealed.to_owned(), &limits) else {
        return Vec::new();
    };
    let Some(operations) = document
        .root()
        .get("operations")
        .and_then(tilewright_jsonc::Value::as_array)
    else {
        return Vec::new();
    };
    let uids = operations
        .iter()
        .filter(|operation| {
            operation
                .get("op")
                .and_then(tilewright_jsonc::Value::as_str)
                == Some("object.remove")
        })
        .filter_map(|operation| {
            operation
                .get("uid")
                .and_then(tilewright_jsonc::Value::as_str)
        });
    let mut lines = Vec::new();
    for (index, uid) in uids.enumerate() {
        if index == MAXIMUM_DESCRIBED {
            lines.push("Further deletions are not described here.".into());
            break;
        }
        describe(committed, uid, &mut lines);
    }
    lines
}

fn describe(world: &World, uid: &str, lines: &mut Vec<String>) {
    let name = short(uid);
    let Some(object) = world.object(uid) else {
        return;
    };
    let carried = inventory_count(object);
    if carried > 0 {
        lines.push(format!(
            "Warning: {name} holds {carried} inventory object{}; deleting it deletes them too.",
            plural(carried)
        ));
    }
    if let Some((plane, layer, at)) = tile(world, uid) {
        let dependents = dependents(world, uid, &plane, layer, at);
        if !dependents.is_empty() {
            let mut listed = dependents
                .iter()
                .take(MAXIMUM_NAMES)
                .cloned()
                .collect::<Vec<_>>();
            if dependents.len() > MAXIMUM_NAMES {
                listed.push(format!("{} more", dependents.len() - MAXIMUM_NAMES));
            }
            let listed = listed.join(", ");
            lines.push(format!(
                "Warning: {listed} target{} this tile ({}, {}); {} will point at an empty tile.",
                if dependents.len() == 1 { "s" } else { "" },
                at[0],
                at[1],
                if dependents.len() == 1 { "it" } else { "they" },
            ));
        }
    }
    lines.push(format!(
        "Uid {name} is removed with the object; do not reuse it for another object."
    ));
}

/// Every object nested in the inventory, at any depth.
fn inventory_count(object: &Object) -> usize {
    let mut count = 0usize;
    let mut pending: Vec<&Object> = object.inventory.iter().collect();
    while let Some(entry) = pending.pop() {
        count = count.saturating_add(1);
        pending.extend(entry.inventory.iter());
    }
    count
}

/// The plane, spatial layer and plane coordinates of a placed object.
fn tile(world: &World, uid: &str) -> Option<(String, i32, [i64; 2])> {
    let location = world.object_location(uid)?;
    let at = world.object(uid)?.at?;
    let size = i64::from(world.planes().get(&location.plane)?.chunk_size);
    let global = |chunk: i64, local: u16| chunk.checked_mul(size)?.checked_add(i64::from(local));
    Some((
        location.plane.clone(),
        location.chunk.0,
        [
            global(location.chunk.1, at[0])?,
            global(location.chunk.2, at[1])?,
        ],
    ))
}

/// Portals anywhere and anchors of the object's plane whose target is the
/// object's tile, the object itself excepted.
fn dependents(world: &World, uid: &str, plane: &str, layer: i32, at: [i64; 2]) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(own) = world.planes().get(plane) {
        for (id, anchor) in &own.anchors {
            if anchor.layer == layer && anchor.at == at {
                names.push(format!("anchor {}", short(id)));
            }
        }
    }
    for (source_plane, source) in world.planes() {
        for chunk in source.chunks.values() {
            for object in chunk.depth_first() {
                let Some(portal) = &object.portal else {
                    continue;
                };
                if object.uid.as_deref() == Some(uid) {
                    continue;
                }
                let target = match portal {
                    Portal::Location {
                        plane: target,
                        layer,
                        at,
                    } => Some((target.as_deref().unwrap_or(source_plane), *layer, *at)),
                    Portal::Anchor {
                        plane: target,
                        anchor,
                    } => {
                        let target = target.as_deref().unwrap_or(source_plane);
                        world
                            .planes()
                            .get(target)
                            .and_then(|p| p.anchors.get(anchor))
                            .map(|a| (target, a.layer, a.at))
                    }
                };
                if target == Some((plane, layer, at)) {
                    names.push(object.uid.as_deref().map_or_else(
                        || "an unnamed portal".to_owned(),
                        |id| format!("portal {}", short(id)),
                    ));
                }
            }
        }
    }
    names
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn short(text: &str) -> String {
    if text.chars().count() <= MAXIMUM_NAME_CHARS {
        return text.into();
    }
    let mut cut: String = text.chars().take(MAXIMUM_NAME_CHARS - 1).collect();
    cut.push('…');
    cut
}
