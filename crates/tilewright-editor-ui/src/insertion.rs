// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only review notes for pending object insertions: what is added
//! where, and a warning when an object of the same prototype already stands
//! on that tile. They inform the author and never refuse or block the
//! reviewed transaction.
use tilewright_source::World;

/// At most this many insertions in one transaction are described.
const MAXIMUM_DESCRIBED: usize = 8;
/// Displayed identifiers are cut to this many characters.
const MAXIMUM_NAME_CHARS: usize = 64;

/// Notes for each `object.insert` in `sealed`, judged against the
/// `committed` World that does not hold the object yet.
pub(crate) fn notes(committed: &World, sealed: &str) -> Vec<String> {
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
    let mut lines = Vec::new();
    let insertions = operations.iter().filter(|operation| {
        operation
            .get("op")
            .and_then(tilewright_jsonc::Value::as_str)
            == Some("object.insert")
    });
    for (index, operation) in insertions.enumerate() {
        if index == MAXIMUM_DESCRIBED {
            lines.push("Further insertions are not described here.".into());
            break;
        }
        describe(committed, operation, &mut lines);
    }
    lines
}

fn describe(world: &World, operation: &tilewright_jsonc::Value, lines: &mut Vec<String>) {
    let text = |value: Option<&tilewright_jsonc::Value>| {
        value.and_then(tilewright_jsonc::Value::as_str).map(short)
    };
    let object = operation.get("object");
    let Some(proto) = object.and_then(|object| object.get("proto")) else {
        return;
    };
    let Some(proto) = proto.as_str() else {
        return;
    };
    let plane = operation
        .get("plane")
        .and_then(tilewright_jsonc::Value::as_str);
    let layer = operation
        .get("layer")
        .and_then(tilewright_jsonc::Value::as_i64)
        .and_then(|layer| i32::try_from(layer).ok());
    let at = operation
        .get("at")
        .and_then(tilewright_jsonc::Value::as_array)
        .and_then(|axes| Some([axes.first()?.as_i64()?, axes.get(1)?.as_i64()?]));
    let (Some(plane), Some(layer), Some([x, y])) = (plane, layer, at) else {
        return;
    };
    let uid = text(object.and_then(|object| object.get("uid")))
        .map_or_else(|| "no uid".into(), |uid| format!("uid {uid}"));
    lines.push(format!(
        "Adds {} at tile ({x}, {y}) of {} layer {layer}, with {uid}.",
        short(proto),
        short(plane),
    ));
    let stacked = stacked(world, plane, layer, [x, y], proto);
    if stacked > 0 {
        lines.push(format!(
            "Warning: {stacked} {} object{} already on this tile; the new one is added on top.",
            short(proto),
            if stacked == 1 { " is" } else { "s are" },
        ));
    }
}

/// How many top-level objects of `proto` already stand on plane tile `at`.
fn stacked(world: &World, plane: &str, layer: i32, at: [i64; 2], proto: &str) -> usize {
    let Some(plane) = world.planes().get(plane) else {
        return 0;
    };
    let size = i64::from(plane.chunk_size);
    if size <= 0 {
        return 0;
    }
    let local = [
        u16::try_from(at[0].rem_euclid(size)).ok(),
        u16::try_from(at[1].rem_euclid(size)).ok(),
    ];
    let [Some(lx), Some(ly)] = local else {
        return 0;
    };
    plane
        .chunks
        .get(&(layer, at[0].div_euclid(size), at[1].div_euclid(size)))
        .map_or(0, |chunk| {
            chunk
                .objects
                .iter()
                .filter(|object| object.at == Some([lx, ly]) && object.proto == proto)
                .count()
        })
}

fn short(text: &str) -> String {
    if text.chars().count() <= MAXIMUM_NAME_CHARS {
        return text.into();
    }
    let mut cut: String = text.chars().take(MAXIMUM_NAME_CHARS - 1).collect();
    cut.push('…');
    cut
}
