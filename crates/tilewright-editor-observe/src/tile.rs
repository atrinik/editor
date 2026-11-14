// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `tile`: everything on one plane tile, and what the catalog allows there.
use crate::limits::{MAXIMUM_ARRIVALS, MAXIMUM_FIELDS, MAXIMUM_TILE_ANCHORS, MAXIMUM_TILE_OBJECTS};
use crate::node::{Map, Node};
use crate::world::{Observed, target_node};
use tilewright_editor_preview::PreviewCatalog;
use tilewright_editor_ui::{anchors, grants, zones};
use tilewright_source::{FieldValue, Object, World};

pub const FORMAT: &str = "tilewright.editor-tile/1";

/// The tile `x, y` of spatial `layer` in `plane`, or `None` when the world
/// has no such plane.
pub fn report(observed: &Observed, plane_id: &str, layer: i32, x: i64, y: i64) -> Option<Map> {
    let world = observed.world();
    let plane = world.planes().get(plane_id)?;
    // World requires a positive chunk size; the bound keeps this total.
    let size = i64::from(plane.chunk_size.max(1));
    let key = (layer, x.div_euclid(size), y.div_euclid(size));
    let chunk = plane.chunks.get(&key);
    let local = usize::try_from(y.rem_euclid(size) * size + x.rem_euclid(size)).ok();
    let at = [
        u16::try_from(x.rem_euclid(size)).ok(),
        u16::try_from(y.rem_euclid(size)).ok(),
    ];
    let ground = world
        .profile()
        .ground_layers
        .iter()
        .enumerate()
        .map(|(index, grid)| {
            let value = chunk
                .zip(local)
                .and_then(|(chunk, local)| chunk.ground.get(index)?.value(local));
            Map::new()
                .text("grid", grid)
                .optional_text("value", value)
                .into()
        })
        .collect();
    let zone = zones::tile_zone(plane, layer, x, y).map_or(Node::Null, |zone| {
        Map::new()
            .text("id", zone.id)
            .with("from_default", zone.from_default)
            .ids("path", zone.path.iter().copied())
            .optional_text("name", zone.name)
            .into()
    });
    let catalog = observed.catalog();
    let mut status = observed.catalog_status(false);
    if let Some(catalog) = catalog {
        status = status.with(
            "in_view",
            catalog.tile_in_active_view(plane_id, layer, x, y),
        );
    }
    let objects: Vec<&Object> = match (chunk, at) {
        (Some(chunk), [Some(lx), Some(ly)]) => chunk
            .objects
            .iter()
            .filter(|object| object.at == Some([lx, ly]))
            .collect(),
        _ => Vec::new(),
    };
    let named = anchors::reference_counts(world, plane_id);
    let here = anchors::tile_anchors(plane, layer, [x, y]);
    let arrivals: Vec<anchors::PortalHolder<'_>> = anchors::portals(world)
        .into_iter()
        .filter(|holder| {
            anchors::target_tile(world, &holder.target) == Some((plane_id, layer, [x, y]))
        })
        .collect();
    Some(
        observed
            .header(FORMAT)
            .with(
                "tile",
                Map::new()
                    .text("plane", plane_id)
                    .with("layer", layer)
                    .with("x", x)
                    .with("y", y),
            )
            .with(
                "chunk",
                chunk.map_or(Node::Null, |chunk| {
                    Map::new()
                        .text("path", &chunk.path)
                        .with("at", [key.1, key.2])
                        .into()
                }),
            )
            .with("ground", Node::List(ground))
            .with(
                "height",
                chunk
                    .zip(local)
                    .and_then(|(chunk, local)| chunk.heights.get(local))
                    .map_or(Node::Null, |height| Node::from(*height)),
            )
            .with("zone", zone)
            .with("catalog", status)
            .bounded("objects", objects, MAXIMUM_TILE_OBJECTS, |object| {
                object_node(world, catalog, plane_id, object).into()
            })
            .bounded("anchors", here, MAXIMUM_TILE_ANCHORS, |id| {
                Map::new()
                    .text("id", id)
                    .count("portals", named.get(id).copied().unwrap_or(0))
                    .into()
            })
            .bounded("arrivals", &arrivals, MAXIMUM_ARRIVALS, arrival_node),
    )
}

/// One object whose portal leads to the tile.
fn arrival_node(holder: &anchors::PortalHolder<'_>) -> Node {
    let via = match holder.target {
        anchors::Target::Location { .. } => "location",
        anchors::Target::Anchor { .. } => "anchor",
    };
    Map::new()
        .optional_text("uid", holder.uid)
        .text("plane", holder.plane)
        .with("layer", holder.layer)
        .with("at", holder.tile.map_or(Node::Null, Node::from))
        .with("carried", holder.depth > 0)
        .with("via", Node::Text(via.into()))
        .into()
}

/// One placed object of the tile: what it is, its fields, its portal, its
/// inventory, and with a catalog what the catalog lets the author do to it.
fn object_node(
    world: &World,
    catalog: Option<&PreviewCatalog>,
    plane: &str,
    object: &Object,
) -> Map {
    let carried = {
        let mut count = 0usize;
        let mut pending: Vec<&Object> = object.inventory.iter().collect();
        while let Some(entry) = pending.pop() {
            count += 1;
            pending.extend(&entry.inventory);
        }
        count
    };
    let allowed = match (catalog, object.uid.as_deref()) {
        (None, _) => Node::Null,
        (Some(_), None) => Map::new()
            .with("move", false)
            .with("delete", false)
            .with("portal", false)
            .with("fields", Node::List(Vec::new()))
            .with("fields_more", Node::count(0))
            .into(),
        (Some(catalog), Some(uid)) => Map::new()
            .with("move", grants::can_move(world, catalog, uid))
            .with("delete", catalog.can_delete(world, uid))
            .with("portal", catalog.can_edit_portal(world, uid))
            .bounded(
                "fields",
                catalog.editable_fields(world, uid),
                MAXIMUM_FIELDS,
                |field| Node::Text(field.into()),
            )
            .into(),
    };
    Map::new()
        .optional_text("uid", object.uid.as_deref())
        .text("proto", &object.proto)
        .optional_text("render_layer", object.layer.as_deref())
        .with("sub_layer", object.sub_layer)
        .with("z", object.z)
        .bounded("fields", &object.fields, MAXIMUM_FIELDS, |(name, value)| {
            field_node(name, value).into()
        })
        .with(
            "portal",
            object.portal.as_ref().map_or(Node::Null, |portal| {
                target_node(world, &anchors::resolve(plane, portal)).into()
            }),
        )
        .count("inventory", object.inventory.len())
        .count("carried", carried)
        .with("custom", object.custom.is_some())
        .with("allowed", allowed)
}

/// One field override: its name, its kind and its value. Strings and
/// grouped (`json`, `list`, `object`) values are text, the latter as
/// World's canonical JSON; both are clipped.
fn field_node(name: &str, value: &FieldValue) -> Map {
    let field = Map::new().text("name", name);
    match value {
        FieldValue::Null => field
            .with("kind", Node::Text("null".into()))
            .with("value", Node::Null),
        FieldValue::Bool(value) => field
            .with("kind", Node::Text("boolean".into()))
            .with("value", *value),
        FieldValue::Integer(value) => field
            .with("kind", Node::Text("integer".into()))
            .with("value", *value),
        FieldValue::Number(value) => field
            .with("kind", Node::Text("number".into()))
            .with("value", Node::Number(*value)),
        FieldValue::String(value) => field
            .with("kind", Node::Text("string".into()))
            .text("value", value),
        FieldValue::Json(value) => field
            .with("kind", Node::Text("json".into()))
            .text("value", value),
    }
}
