// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `overview`: what a world holds and what its catalog grants.
use crate::limits::{
    MAXIMUM_FIELDS, MAXIMUM_GRANTS, MAXIMUM_LAYERS, MAXIMUM_PLANES, MAXIMUM_RENDER_LAYERS,
    MAXIMUM_TERRAINS, MAXIMUM_VIEW_ZONES,
};
use crate::node::{Map, Node};
use crate::world::Observed;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_editor_ui::zones;
use tilewright_source::{Plane, Projection, World};

pub const FORMAT: &str = "tilewright.editor-overview/1";

pub fn report(observed: &Observed) -> Map {
    let world = observed.world();
    let summary = observed.document.summary();
    let profile = world.profile();
    let projection = match profile.projection {
        Projection::TopDown { tile_size } => Map::new()
            .with("kind", Node::Text("top_down".into()))
            .with("tile_size", tile_size),
        Projection::Isometric {
            tile_width,
            tile_height,
        } => Map::new()
            .with("kind", Node::Text("isometric".into()))
            .with("tile_width", tile_width)
            .with("tile_height", tile_height),
    };
    let profile = Map::new()
        .text("id", &summary.profile)
        .text("version", &summary.profile_version)
        .with("projection", projection)
        .bounded(
            "render_layers",
            &profile.render_layers,
            MAXIMUM_RENDER_LAYERS,
            |layer| Node::Text(layer.clone()),
        )
        .ids(
            "ground_layers",
            summary.ground_layers.iter().map(String::as_str),
        )
        .optional_text("terrain_layer", summary.terrain_layer.as_deref())
        .bounded("terrains", &summary.terrains, MAXIMUM_TERRAINS, |terrain| {
            Node::Text(terrain.clone())
        })
        .with(
            "layer_height",
            profile.layer_height.map_or(Node::Null, Node::from),
        );
    let mut catalog = observed.catalog_status(true);
    if let Some(loaded) = observed.catalog() {
        catalog = catalog_members(catalog, world, loaded);
    }
    observed
        .header(FORMAT)
        .with("profile", profile)
        .count("definitions", world.definitions().len())
        .count("plane_count", world.planes().len())
        .bounded("planes", world.planes().values(), MAXIMUM_PLANES, |plane| {
            plane_node(plane).into()
        })
        .with("catalog", catalog)
}

/// One plane: its definition, what it holds and, per spatial layer, its
/// chunks and the tiles they cover.
fn plane_node(plane: &Plane) -> Map {
    let (mut placed, mut carried, mut portals) = (0usize, 0usize, 0usize);
    for chunk in plane.chunks.values() {
        placed += chunk.objects.len();
        for object in chunk.depth_first() {
            if object.at.is_none() {
                carried += 1;
            }
            if object.portal.is_some() {
                portals += 1;
            }
        }
    }
    // Chunk keys are sorted by layer, then chunk x and y.
    let mut layers: Vec<(i32, usize, [i64; 4])> = Vec::new();
    for &(layer, cx, cy) in plane.chunks.keys() {
        match layers.last_mut() {
            Some((last, count, extent)) if *last == layer => {
                *count += 1;
                extent[0] = extent[0].min(cx);
                extent[1] = extent[1].min(cy);
                extent[2] = extent[2].max(cx);
                extent[3] = extent[3].max(cy);
            }
            _ => layers.push((layer, 1, [cx, cy, cx, cy])),
        }
    }
    let size = i64::from(plane.chunk_size);
    Map::new()
        .text("id", &plane.id)
        .optional_text("title", plane.title.as_deref())
        .text("kind", plane.kind.as_str())
        .with("chunk_size", plane.chunk_size)
        .optional_text("default_zone", plane.default_zone.as_deref())
        .count("zones", plane.zones.len())
        .count("anchors", plane.anchors.len())
        .count("stairwells", plane.stairwells.len())
        .count("chunks", plane.chunks.len())
        .count("objects", placed)
        .count("carried", carried)
        .count("portals", portals)
        .bounded(
            "layers",
            layers,
            MAXIMUM_LAYERS,
            |(layer, chunks, [min_x, min_y, max_x, max_y])| {
                Map::new()
                    .with("layer", layer)
                    .count("chunks", chunks)
                    .with("bounds", tile_bounds(size, [min_x, min_y, max_x, max_y]))
                    .into()
            },
        )
}

/// The tiles chunks `min..=max` cover, as `[x, y, width, height]`, or null
/// when that does not fit in 64 bits.
fn tile_bounds(size: i64, [min_x, min_y, max_x, max_y]: [i64; 4]) -> Node {
    let span = |min: i64, max: i64| max.checked_sub(min)?.checked_add(1)?.checked_mul(size);
    let bounds = (|| {
        Some([
            min_x.checked_mul(size)?,
            min_y.checked_mul(size)?,
            span(min_x, max_x)?,
            span(min_y, max_y)?,
        ])
    })();
    bounds.map_or(Node::Null, |bounds| {
        Node::List(bounds.map(Node::Integer).to_vec())
    })
}

/// The catalog's counts, the zones its view shows and everything it
/// grants.
fn catalog_members(catalog: Map, world: &World, loaded: &PreviewCatalog) -> Map {
    let editing = loaded.editing();
    let view_zones = zones::view_zones(world, loaded);
    let counts = zones::tile_counts(view_zones.iter().map(|(_, zone)| *zone));
    let grants = Map::new()
        .bounded(
            "movable_uids",
            &editing.movable_uids,
            MAXIMUM_GRANTS,
            |uid| Node::Text(uid.clone()),
        )
        .bounded("fields", &editing.fields, MAXIMUM_GRANTS, |(uid, field)| {
            Map::new().text("uid", uid).text("field", field).into()
        })
        .bounded(
            "terrain",
            &editing.terrain,
            MAXIMUM_GRANTS,
            |(grid, values)| {
                Map::new()
                    .text("grid", grid)
                    .bounded("values", values, MAXIMUM_GRANTS, |value| {
                        Node::Text(value.clone())
                    })
                    .into()
            },
        )
        .bounded(
            "objects",
            &editing.objects,
            MAXIMUM_GRANTS,
            |(proto, grant)| {
                Map::new()
                    .text("proto", proto)
                    .text("layer", &grant.layer)
                    .bounded("fields", &grant.fields, MAXIMUM_FIELDS, |field| {
                        Node::Text(field.clone())
                    })
                    .bounded("defaults", &grant.defaults, MAXIMUM_FIELDS, |(name, _)| {
                        Node::Text(name.clone())
                    })
                    .with("portal", grant.portal)
                    .into()
            },
        )
        .with(
            "zones",
            Map::new()
                .with("paint", editing.zones.paint)
                .with("manage", editing.zones.manage),
        )
        .with("anchors", Map::new().with("edit", editing.anchors.edit));
    catalog
        .count("view_tiles_with_zone", view_zones.len())
        .bounded("view_zones", counts, MAXIMUM_VIEW_ZONES, |(zone, tiles)| {
            Map::new().text("id", zone).count("tiles", tiles).into()
        })
        .count("prototypes", loaded.prototypes().count())
        .count("resources", loaded.resource_count())
        .with("grants", grants)
}
