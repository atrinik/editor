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
    let (carried, mut uids) = inventory(object);
    if carried > 0 {
        lines.push(format!(
            "Warning: {name} holds {carried} inventory object{}; deleting it deletes them too.",
            plural(carried)
        ));
    }
    if let Some((plane, layer, at)) = tile(world, uid) {
        let dependents = dependents(world, uid, &plane, layer, at);
        if !dependents.is_empty() {
            let listed = list(&dependents);
            let one = dependents.len() == 1;
            lines.push(format!(
                "Warning: {listed} target{} this object's tile ({}, {}); {} will still target that tile after the deletion.",
                if one { "s" } else { "" },
                at[0],
                at[1],
                if one { "it" } else { "they" },
            ));
        }
    }
    uids.insert(0, name);
    let one = uids.len() == 1;
    lines.push(format!(
        "Uid{} {} {} removed with the object; do not reuse {} for other objects.",
        if one { "" } else { "s" },
        list(&uids),
        if one { "is" } else { "are" },
        if one { "it" } else { "them" },
    ));
}

/// The first names, then a count of the rest.
fn list(names: &[String]) -> String {
    let mut listed = names
        .iter()
        .take(MAXIMUM_NAMES)
        .cloned()
        .collect::<Vec<_>>();
    if names.len() > MAXIMUM_NAMES {
        listed.push(format!("{} more", names.len() - MAXIMUM_NAMES));
    }
    listed.join(", ")
}

/// The number of objects nested in the inventory at any depth, and the
/// shortened uids among them in source order.
fn inventory(object: &Object) -> (usize, Vec<String>) {
    let mut count = 0usize;
    let mut uids = Vec::new();
    let mut pending: Vec<&Object> = object.inventory.iter().rev().collect();
    while let Some(entry) = pending.pop() {
        count = count.saturating_add(1);
        if let Some(uid) = &entry.uid {
            uids.push(short(uid));
        }
        pending.extend(entry.inventory.iter().rev());
    }
    (count, uids)
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

#[cfg(test)]
mod tests {
    use super::{MAXIMUM_DESCRIBED, MAXIMUM_NAME_CHARS, warnings};
    use serde_json::{Value, json};
    use std::{fs, path::Path};

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn removals(uids: &[&str]) -> String {
        let operations: Vec<Value> = uids
            .iter()
            .map(|uid| json!({"op": "object.remove", "uid": uid, "expect": {"proto": "marker"}}))
            .collect();
        json!({"format": "tilewright.transaction/1", "title": "t", "actor": "test",
            "operations": operations})
        .to_string()
    }

    #[test]
    fn descriptions_names_and_identifiers_are_bounded() {
        let long = format!("long_{}", "x".repeat(70));
        let dir = tempfile::tempdir().unwrap();
        copy_tree(
            &tilewright_editor_testkit::fixture_world("prototype-neutral"),
            dir.path(),
        );
        let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 0..9 {
            objects.push(json!({"uid": format!("t_{index}"), "at": [index, 10],
                "proto": "marker", "layer": "item"}));
        }
        for index in 0..4 {
            objects.push(json!({"uid": format!("p_{index}"), "at": [index, 11],
                "proto": "marker", "layer": "item", "portal": {"layer": 0, "at": [0, 10]}}));
        }
        objects.push(json!({"uid": long, "at": [0, 12], "proto": "marker", "layer": "item"}));
        let held: Vec<Value> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| json!({"uid": format!("held_{id}"), "proto": "marker"}))
            .collect();
        objects.push(
            json!({"uid": "holder", "at": [0, 13], "proto": "marker", "layer": "item",
            "inventory": held}),
        );
        fs::write(&path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
        let world = tilewright_source::load(dir.path()).unwrap();

        let uids: Vec<String> = (0..9).map(|index| format!("t_{index}")).collect();
        let lines = warnings(
            &world,
            &removals(&uids.iter().map(String::as_str).collect::<Vec<_>>()),
        );
        assert_eq!(
            lines.iter().filter(|line| line.starts_with("Uid")).count(),
            MAXIMUM_DESCRIBED
        );
        assert!(!lines.iter().any(|line| line.contains("t_8")));
        assert_eq!(
            lines.last().unwrap(),
            "Further deletions are not described here."
        );
        assert!(
            lines[0].starts_with("Warning: portal p_0, portal p_1, portal p_2, 1 more target "),
            "{lines:?}"
        );

        let lines = warnings(&world, &removals(&[&long]));
        let shown: String = long.chars().take(MAXIMUM_NAME_CHARS - 1).collect();
        assert_eq!(
            lines,
            [format!(
                "Uid {shown}… is removed with the object; do not reuse it for other objects."
            )]
        );

        let lines = warnings(&world, &removals(&["holder"]));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("holds 4 inventory objects"));
        assert!(
            lines[1].starts_with("Uids holder, held_a, held_b, 2 more are removed"),
            "{lines:?}"
        );
    }
}
