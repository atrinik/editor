// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The live preview of brush strokes, resolved over the editor's unsaved
//! terrain, shows exactly the tiles the toolkit derives from the world the
//! strokes' transaction produces.

use std::collections::BTreeSet;
use tilewright_edit::Transaction;
use tilewright_editor_commands::{
    brush::{Brush, Shape, Stroke},
    preview, transaction,
};
use tilewright_editor_document::{WorldDocument, terrain::PlaneTerrain};
use tilewright_editor_testkit::fixture_world;

#[test]
fn live_preview_matches_the_planned_world_across_chunks() {
    let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
    let mut unsaved = PlaneTerrain::new(document.world().clone(), "meadow").unwrap();
    // Grass across the sand and into the chunk east of it, which is not on
    // disk; then water out of the pond.
    let strokes = [
        (
            "grass",
            Brush {
                shape: Shape::Circle,
                radius: 2,
            },
            vec![[24, 3], [30, 4], [37, 6]],
        ),
        (
            "water",
            Brush {
                shape: Shape::Square,
                radius: 1,
            },
            vec![[15, 15], [18, 18]],
        ),
    ];
    let (mut dirty, mut operations, mut commands) = (BTreeSet::new(), Vec::new(), Vec::new());
    for (terrain, brush, points) in strokes {
        let mut stroke = Stroke::begin(&unsaved, 0, terrain, brush).unwrap();
        for point in points {
            dirty.extend(stroke.stamp(&mut unsaved, point).unwrap());
        }
        let operation = stroke.operation().unwrap().unwrap();
        let alone = transaction(
            &stroke.title(),
            std::slice::from_ref(&operation),
            &[stroke.record()],
        );
        Transaction::parse("stroke.jsonc", alone.as_str()).unwrap();
        operations.push(operation);
        commands.push(stroke.record());
    }
    // Both strokes as one change, planned against the tree on disk.
    let text = transaction("Two strokes", &operations, &commands);
    let planned = preview(&document, "strokes.jsonc", &text).unwrap();
    let world = planned.world();
    let transitions = world.profile().transitions.as_deref().unwrap();
    let plane = &world.planes()["meadow"];
    assert!(
        plane.chunks.contains_key(&(0, 1, 0)),
        "the new chunk exists"
    );
    assert_eq!(dirty.len(), 12, "{dirty:?}");
    for key in &dirty {
        let (layer, cx, cy) = *key;
        let live = unsaved.resolve(*key).unwrap();
        let saved = transitions.resolve_chunk(plane, layer, cx, cy);
        assert_eq!(live.grids(), saved.grids(), "chunk {key:?}");
    }
    // The preview shows transitions: grass edges on the sand around the new
    // grass, and a shore where the grown pond meets grass on its east.
    let west = unsaved.resolve((0, 0, 0)).unwrap();
    assert!(
        (0..32_u16)
            .flat_map(|y| (0..32_u16).map(move |x| [x, y]))
            .any(|at| west
                .at(2, at)
                .is_some_and(|tile| tile.starts_with("grass_edge")))
    );
    assert_eq!(west.at(2, [19, 18]), Some("shore_e"));
}
