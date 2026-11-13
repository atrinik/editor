// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Anchor mode through real SDL pointer, key and text events, on the CPU
//! scene path the scripted demonstration captures: the badges, the anchor
//! list and inspector lines, and create, move and delete, each reviewed and
//! accepted as one World change.
use super::acknowledge_ui;
use super::zone_manage::{all_texts, enter, press, review_has, row_enabled, type_text};
use super::zones::{button, key, route, sidebar_texts, zone_application};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use std::collections::BTreeSet;
use std::fs;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::tools::ToolMode;
use tilewright_render_api::{FrameOutput, RenderRequest, Renderer};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_scene::{Layer, SceneBundle};

const ANCHORS: &str = "world/planes/main/anchors.jsonc";

fn render(
    scene: &SceneBundle,
    resources: std::sync::Arc<dyn tilewright_render_resources::ResourceProvider>,
) -> FrameOutput {
    ReferenceRenderer::new(offscreen_target(
        scene.viewport().width,
        scene.viewport().height,
    ))
    .unwrap()
    .render(RenderRequest { scene, resources })
    .unwrap()
}
/// Press and release on tile `(x, y)` of the view.
fn click_tile(app: &mut Application, x: i64, y: i64) {
    acknowledge_ui(app);
    let at = super::cell_pixel(app, x, y);
    route(app, &button(true, at)).unwrap();
    route(app, &button(false, at)).unwrap();
}
/// Press and release on a pixel of the placed object `uid`.
fn click_object(app: &mut Application, uid: &str) {
    acknowledge_ui(app);
    let frame = app.frame(false).unwrap();
    let id = app
        .prepared
        .picks
        .iter()
        .find_map(|(id, key)| match key {
            SelectionKey::Object { uid: found } if found == uid => Some(*id),
            _ => None,
        })
        .unwrap();
    let (x, y) = crate::prototype::pixel_for(&frame, id).unwrap();
    #[allow(clippy::cast_precision_loss)]
    let at = [x as f32, y as f32];
    route(app, &button(true, at)).unwrap();
    route(app, &button(false, at)).unwrap();
}

#[test]
fn badges_show_in_anchor_mode_under_objects_and_never_reach_the_semantic_plane() {
    let (_temporary, mut app) = zone_application(|_| {});
    // Other modes draw no badge until O shows the overlay.
    assert!(crate::prototype::badge_overlay(&app).is_empty());
    route(&mut app, &key(Keycode::K)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Anchor);
    let badges = crate::prototype::badge_overlay(&app);
    // One anchor in view, (7, 3); two gates with portals, (7, 0) and
    // (0, 3). The out-of-view anchor gets none. Each tile is one cell quad.
    assert_eq!(badges.len(), 3);
    assert!(crate::prototype::zone_overlay(&app).is_empty(), "no tints");
    let anchor_badges = badges
        .iter()
        .filter(|badge| badge.color == crate::workspace_render::ANCHOR_BADGE)
        .count();
    assert_eq!(anchor_badges, 1);
    for badge in &badges {
        assert!(!badge.pickable && badge.layer == Layer::World && badge.glow.is_none());
        // Smaller than its tile and inside it, in the World slot the zone
        // tint of that tile takes.
        let source = app
            .prepared
            .cell_scene
            .quads()
            .iter()
            .find(|quad| {
                let [min, max] = bounds(quad.positions);
                let [inner_min, inner_max] = bounds(badge.positions);
                inner_min[0] >= min[0]
                    && inner_min[1] >= min[1]
                    && inner_max[0] <= max[0]
                    && inner_max[1] <= max[1]
                    && quad.uvs == badge.uvs
            })
            .unwrap();
        let [min, max] = bounds(source.positions);
        let [inner_min, inner_max] = bounds(badge.positions);
        let size = max[0] - min[0];
        assert!((inner_max[0] - inner_min[0]) * 3.0 < size);
        // An anchor badge sits in its tile's top-left corner, a portal
        // badge in the top-right one.
        assert!(inner_min[1] - min[1] < size / 4.0);
        if badge.color == crate::workspace_render::ANCHOR_BADGE {
            assert!(inner_min[0] - min[0] < size / 4.0);
        } else {
            assert!(max[0] - inner_max[0] < size / 4.0);
        }
        let key = app.prepared.cell_picks[&source.semantic_id].clone();
        let mark = crate::workspace_render::stroke_overlay(&app.prepared, &BTreeSet::from([key]));
        assert!(
            mark.iter()
                .any(|m| (m.depth, m.painter_order) == (badge.depth, badge.painter_order))
        );
    }

    app.refresh_ui();
    let with = crate::prototype::workspace_presentation(&mut app).unwrap();
    let without = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap();
    let shown = render(&with.scene, with.resources.clone());
    let plain = render(&without.scene, without.resources);
    assert_ne!(shown.rgba8, plain.rgba8, "the badges are visible");
    assert_eq!(shown.semantic_ids, plain.semantic_ids);
    assert_eq!(shown.depth, plain.depth);
    assert_eq!(shown.coverage, plain.coverage);
    // Pixels a gate covers keep the gate's colour: a badge is drawn after
    // its tile's terrain and before the object on it.
    let gates: BTreeSet<u32> = app
        .prepared
        .picks
        .iter()
        .filter(|(_, key)| matches!(key, SelectionKey::Object { uid } if uid.starts_with("gate_")))
        .map(|(id, _)| id.0)
        .collect();
    let mut covered = 0;
    for (index, id) in plain.semantic_ids.iter().enumerate() {
        if gates.contains(id) && plain.rgba8[index * 4 + 3] == 255 {
            assert_eq!(
                shown.rgba8[index * 4..index * 4 + 4],
                plain.rgba8[index * 4..index * 4 + 4]
            );
            covered += 1;
        }
    }
    assert!(covered > 0, "the sample's gates stand in the view");
    // A click on a badged tile still picks that tile.
    let mask = app.prepared.mask(app.view.frame_revision, true).unwrap();
    let at = super::cell_pixel(&mut app, 7, 3);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let picked = mask.pick(at[0] as u32, at[1] as u32).unwrap();
    assert!(matches!(
        picked,
        Some(SelectionKey::Cell { x: 7, y: 3, .. })
    ));
    // O adds the zone tints in Anchor mode; the badges stay.
    route(&mut app, &key(Keycode::O)).unwrap();
    assert_eq!(crate::prototype::zone_overlay(&app).len(), 48);
    assert_eq!(crate::prototype::badge_overlay(&app).len(), 3);
    assert!(sidebar_texts(&mut app).contains(&"Hide zones  O".to_owned()));
}

/// Whether `texts` holds `line` whole. On failure the texts are printed
/// once, not per scroll position.
fn has(texts: &[String], line: &str) -> bool {
    let found = texts.iter().any(|text| text == line);
    if !found {
        let distinct: BTreeSet<&String> = texts.iter().collect();
        eprintln!("{line:?} not in {distinct:?}");
    }
    found
}

/// The smallest and largest corner of `positions`.
fn bounds(positions: [[f32; 2]; 4]) -> [[f32; 2]; 2] {
    let mut min = positions[0];
    let mut max = positions[0];
    for [x, y] in positions {
        min = [min[0].min(x), min[1].min(y)];
        max = [max[0].max(x), max[1].max(y)];
    }
    [min, max]
}

#[test]
fn the_list_inspector_and_legend_name_anchors_portals_and_their_targets() {
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::K)).unwrap();
    let texts = all_texts(&mut app);
    for line in [
        "Light badge: anchor",
        "Dark badge: portal object",
        "Anchors: 2 in the plane",
        "New anchor: select a tile first",
        "Anchor ids are fixed in this version",
        "To rename: create a new anchor",
        "and retarget its portals",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    // Each anchor's row keeps its id whole, its position and how many
    // portals name it, whatever else the width leaves out.
    for (id, parts) in [
        ("arrival", ["(7, 3)", "1 portal"]),
        ("lookout", ["(-2, 0)", "0 portals"]),
    ] {
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with(id) && parts.iter().all(|part| text.contains(part))),
            "{id}"
        );
    }
    // The legend rows carry their badge's colour.
    app.refresh_ui();
    for (text, colour) in [
        ("Light badge: anchor", crate::workspace_render::ANCHOR_BADGE),
        (
            "Dark badge: portal object",
            crate::workspace_render::PORTAL_BADGE,
        ),
    ] {
        let widget = app.ui.widgets.iter().find(|w| w.text == text).unwrap();
        assert_eq!(widget.swatch, Some(colour));
    }
    assert_eq!(row_enabled(&mut app, &Control::AnchorCreate), Some(false));
    // A click selects a tile, even one an object stands on, and the
    // inspector lists its anchors.
    click_tile(&mut app, 7, 3);
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    let texts = sidebar_texts(&mut app);
    assert!(has(&texts, "Anchor here: arrival, 1 portal"));
    assert!(has(&texts, "New anchor at (7, 3)"));
    click_tile(&mut app, 7, 0);
    assert!(has(&sidebar_texts(&mut app), "Anchors here: none"));
    // The chosen anchor: its place and the portal that names it.
    press(&mut app, &Control::AnchorChoose("arrival".into()));
    assert_eq!(app.tools.anchor_choice(), Some("arrival"));
    let texts = all_texts(&mut app);
    for line in [
        "Anchor arrival (id is fixed)",
        "Layer 0 at (7, 3), in view",
        "Named by 1 portal:",
        "gate_east: main, layer 0, (7, 0)",
        "Move here: (7, 0)",
        "Delete: 1 portal names it",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), Some(false));
    // An anchor out of view is listed and inspected, not moved by tile.
    press(&mut app, &Control::AnchorChoose("lookout".into()));
    let texts = all_texts(&mut app);
    for line in [
        "Layer 0 at (-2, 0), not in view",
        "Named by no portal",
        "Move: only an anchor in view",
        "Delete anchor (unreferenced)",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    assert_eq!(row_enabled(&mut app, &Control::AnchorMove), Some(false));
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), Some(true));
    // In Select mode a click picks the gate; its portal target shows,
    // read-only.
    app.control(Control::Select).unwrap();
    click_object(&mut app, "gate_east");
    let texts = sidebar_texts(&mut app);
    for line in [
        "Portal target (read-only):",
        "Anchor arrival",
        "at main, layer 0, (7, 3)",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    click_object(&mut app, "gate_west");
    assert!(has(&sidebar_texts(&mut app), "main, layer 0, (-2, 2)"));
    // A gate is not granted to any edit.
    assert!(!app.selection_deletable());
}

#[test]
#[allow(clippy::too_many_lines)]
fn create_move_and_delete_through_sdl_events_are_each_one_reviewed_change() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(ANCHORS);
    let original = fs::read(&path).unwrap();
    route(&mut app, &key(Keycode::K)).unwrap();
    click_tile(&mut app, 2, 3);
    press(&mut app, &Control::AnchorCreate);
    assert_eq!(app.tools.anchor_draft(), Some([2, 3]));
    // A draft owns the keyboard: K and N type nothing and switch nothing.
    route(&mut app, &key(Keycode::N)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Anchor);
    // An id the plane uses is refused in the draft, which stays open.
    type_text(&mut app, "arrival");
    acknowledge_ui(&mut app);
    let error = route(&mut app, &key(Keycode::Return)).unwrap_err();
    assert!(error.contains("already has an anchor"), "{error}");
    assert!(app.tools.anchor_draft().is_some());
    app.tools.select_all().unwrap();
    type_text(&mut app, "camp");
    enter(&mut app);
    assert!(app.session.history().pending().is_some());
    assert!(review_has(
        &mut app,
        "Creates anchor camp on layer 0 at (2, 3)."
    ));
    assert_eq!(fs::read(&path).unwrap(), original, "nothing written yet");
    enter(&mut app);
    assert!(app.session.history().pending().is_none());
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("{\"id\": \"camp\", \"layer\": 0, \"at\": [2, 3]}"),
        "{text}"
    );
    assert_eq!(app.recovery.records().len(), 1);
    // Move: choose it, select its new tile, press Move here.
    press(&mut app, &Control::AnchorChoose("camp".into()));
    assert_eq!(row_enabled(&mut app, &Control::AnchorMove), Some(false));
    click_tile(&mut app, 3, 4);
    assert_eq!(app.tools.anchor_choice(), Some("camp"), "a click keeps it");
    press(&mut app, &Control::AnchorMove);
    assert!(review_has(
        &mut app,
        "Moves anchor camp from layer 0 (2, 3) to layer 0 (3, 4)."
    ));
    assert!(review_has(&mut app, "No portal in the world names it."));
    assert!(review_has(
        &mut app,
        "Warning: scripts or saves outside the world may refer to the anchor id camp"
    ));
    enter(&mut app);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("\"at\": [3, 4]")
    );
    // The badge follows it.
    let badges = crate::prototype::badge_overlay(&app);
    assert_eq!(badges.len(), 4);
    // Moving `arrival` lists the gate whose portal now leads elsewhere.
    press(&mut app, &Control::AnchorChoose("arrival".into()));
    press(&mut app, &Control::AnchorMove);
    assert!(review_has(
        &mut app,
        "Portals naming it now lead to the new tile (1 portal: object gate_east on main layer 0 (7, 0))."
    ));
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    // Delete: the referenced one stays disabled; the new one goes.
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), Some(false));
    press(&mut app, &Control::AnchorChoose("camp".into()));
    press(&mut app, &Control::AnchorDelete);
    assert!(review_has(
        &mut app,
        "Deletes anchor camp (layer 0, (3, 4)); no portal in the world names it."
    ));
    assert!(review_has(
        &mut app,
        "may still use the anchor id camp; they will no longer find it"
    ));
    enter(&mut app);
    assert_eq!(fs::read(&path).unwrap(), original, "back to the sample");
    assert_eq!(
        app.tools.anchor_choice(),
        None,
        "a deleted anchor is not chosen"
    );
    assert_eq!(app.recovery.records().len(), 3);
    // Undo brings it back as one reviewed change.
    route(&mut app, &key(Keycode::Z)).unwrap();
    enter(&mut app);
    assert!(fs::read_to_string(&path).unwrap().contains("\"camp\""));
}

#[test]
fn without_the_grant_anchor_mode_shows_and_lists_but_edits_nothing() {
    let (_temporary, mut app) = zone_application(|world| {
        let path = world.join("preview.jsonc");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("anchors");
        fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    });
    route(&mut app, &key(Keycode::K)).unwrap();
    assert_eq!(crate::prototype::badge_overlay(&app).len(), 3);
    let texts = all_texts(&mut app);
    assert!(has(&texts, "Anchor editing not granted"));
    assert!(texts.iter().any(|text| text.starts_with("arrival (7, 3)")));
    for control in [
        Control::AnchorCreate,
        Control::AnchorMove,
        Control::AnchorDelete,
    ] {
        assert_eq!(row_enabled(&mut app, &control), None, "{control:?}");
    }
    click_tile(&mut app, 2, 3);
    press(&mut app, &Control::AnchorChoose("lookout".into()));
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), None);
    // A direct control still refuses, with the reason.
    let error = app.control(Control::AnchorCreate).unwrap_err();
    assert!(error.contains("editing.anchors.edit"), "{error}");
    let error = app.control(Control::AnchorDelete).unwrap_err();
    assert!(error.contains("editing.anchors.edit"), "{error}");
    assert!(app.session.history().pending().is_none());
    assert!(app.status_label().contains("does not grant anchor editing"));
}
