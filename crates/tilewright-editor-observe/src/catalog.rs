// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog`: the editor catalog's prototypes and the terrain and zone
//! palettes, optionally filtered as the desktop Place palette filters.
use crate::limits::{MAXIMUM_FIELDS, MAXIMUM_PROTOTYPES, MAXIMUM_TERRAIN_CHOICES, MAXIMUM_ZONES};
use crate::node::{Map, Node};
use crate::world::Observed;
use tilewright_editor_ui::{tools::palette_matches, zones};

pub const FORMAT: &str = "tilewright.editor-catalog/1";

/// The catalog listing. `filter` keeps what contains it, ignoring case, as
/// [`palette_matches`] decides: prototype ids, terrain values and zone ids.
pub fn report(observed: &Observed, filter: Option<&str>) -> Map {
    let world = observed.world();
    let keep = |id: &str| filter.is_none_or(|filter| palette_matches(id, filter));
    let header = observed
        .header(FORMAT)
        .with("catalog", observed.catalog_status(true))
        .optional_text("match", filter);
    let Some(catalog) = observed.catalog() else {
        return header
            .with("prototypes", Node::List(Vec::new()))
            .with("prototypes_more", Node::count(0))
            .with("terrain_palette", Node::List(Vec::new()))
            .with("terrain_palette_more", Node::count(0))
            .with("zone_palette", Node::List(Vec::new()))
            .with("zone_palette_more", Node::count(0));
    };
    let editing = catalog.editing();
    let prototypes = catalog.prototypes().filter(|(proto, _, _)| keep(proto));
    // Built only for the prototypes listed; the rest are counted.
    let prototype = |(proto, resource, anchor): (&str, &str, &'static str)| -> Node {
        let grant = editing.objects.get(proto);
        let terrain = editing
            .terrain
            .iter()
            .filter(|(_, values)| values.contains(proto))
            .map(|(grid, _)| grid.as_str());
        Map::new()
            .text("proto", proto)
            .text("resource", resource)
            .with("anchor", Node::Text(anchor.into()))
            .with("placeable", grant.is_some())
            .optional_text("layer", grant.map(|grant| grant.layer.as_str()))
            .bounded(
                "fields",
                grant.into_iter().flat_map(|grant| &grant.fields),
                MAXIMUM_FIELDS,
                |field| Node::Text(field.clone()),
            )
            .bounded(
                "defaults",
                grant.into_iter().flat_map(|grant| &grant.defaults),
                MAXIMUM_FIELDS,
                |(name, _)| Node::Text(name.clone()),
            )
            .with("portal", grant.is_some_and(|grant| grant.portal))
            .ids("terrain", terrain)
            .into()
    };
    // The same choices as the desktop palettes (`tools::terrain_palette`
    // and `tools::zone_palette`), filtered before they are bounded.
    let terrain = editing.terrain.iter().flat_map(|(grid, values)| {
        values
            .iter()
            .filter(|value| keep(value))
            .map(move |value| (grid, value))
    });
    let plane = world.planes().get(catalog.view().0);
    let zones = plane
        .into_iter()
        .flat_map(|plane| plane.zones.keys().map(move |id| (plane, id.as_str())))
        .filter(|(_, id)| keep(id));
    header
        .bounded("prototypes", prototypes, MAXIMUM_PROTOTYPES, prototype)
        .bounded(
            "terrain_palette",
            terrain,
            MAXIMUM_TERRAIN_CHOICES,
            |(grid, value)| Map::new().text("grid", grid).text("value", value).into(),
        )
        .bounded("zone_palette", zones, MAXIMUM_ZONES, |(plane, id)| {
            Map::new()
                .text("id", id)
                .optional_text("name", zones::display_name(plane, id))
                .into()
        })
}
