// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The zone overlay, legend, inspector lines and zone brush through real
//! SDL pointer and key events, on the CPU scene path the scripted
//! demonstration captures.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::{BRUSH_WINDOW, acknowledge_ui, application_with_source, cell_pixel};
use crate::prototype::Application;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{event::Event, keyboard::Keycode, mouse::MouseButton};
use std::collections::BTreeSet;
use std::{fs, path::Path};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::tools::{ToolMode, ZoneChoice};
use tilewright_editor_ui::zones;
use tilewright_render_api::{FrameOutput, RenderRequest, Renderer};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_scene::{Layer, Quad, SceneBundle, SceneLimits};

fn button(down: bool, at: [f32; 2]) -> Event {
    let [x, y] = at;
    if down {
        Event::MouseButtonDown {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        }
    } else {
        Event::MouseButtonUp {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mouse_btn: MouseButton::Left,
            clicks: 1,
            x,
            y,
        }
    }
}
fn drag(at: [f32; 2]) -> Event {
    Event::MouseMotion {
        timestamp: 0,
        window_id: 7,
        which: 0,
        mousestate: sdl3::mouse::MouseState::from_sdl_state(1),
        x: at[0],
        y: at[1],
        xrel: 0.0,
        yrel: 0.0,
    }
}
fn key(keycode: Keycode) -> Event {
    super::input_key(7, keycode, false)
}
fn route(app: &mut Application, event: &Event) -> Result<(), String> {
    let mut input = crate::native_input::NativeInput::new(7);
    crate::prototype::native_text_event(app, event, &mut input)
        .or_else(|| {
            crate::prototype::native_input_event(
                app,
                event,
                BRUSH_WINDOW,
                BRUSH_WINDOW,
                false,
                false,
            )
        })
        .unwrap_or(Ok(()))
}
/// A 1280x800 workspace in Select mode with the frame acknowledged.
fn zone_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = application_with_source(prepare);
    app.ui = WorkspaceUi::new(BRUSH_WINDOW, BRUSH_WINDOW);
    app.view.camera.width = BRUSH_WINDOW.0;
    app.view.camera.height = BRUSH_WINDOW.1;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}
/// The centre of the sidebar row whose control is `control`.
fn row_pixel(app: &mut Application, control: &Control) -> [f32; 2] {
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(control))
        .unwrap_or_else(|| panic!("no row for {control:?}"));
    #[allow(clippy::cast_precision_loss)]
    [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ]
}
fn sidebar_texts(app: &mut Application) -> Vec<String> {
    app.refresh_ui();
    let x = app.ui.sidebar_x();
    app.ui
        .widgets
        .iter()
        .filter(|widget| widget.layout.x >= x)
        .map(|widget| widget.text.clone())
        .collect()
}
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
/// The presentation's quads beyond the World scene and the interface: the
/// zone tints and the stroke preview.
fn overlay_quads(app: &mut Application) -> Vec<Quad> {
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    let world = app.prepared.scene.quads();
    presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.layer != Layer::Interface && !world.contains(quad))
        .copied()
        .collect()
}

#[test]
fn the_overlay_shows_in_zone_mode_and_after_o_and_never_otherwise() {
    let (_temporary, mut app) = zone_application(|_| {});
    // Other modes draw exactly the frame they drew before zones existed.
    for mode in [Control::Select, Control::Move, Control::Terrain] {
        app.control(mode).unwrap();
        assert!(overlay_quads(&mut app).is_empty());
        assert!(crate::prototype::zone_overlay(&app).is_empty());
    }
    app.control(Control::Place).unwrap();
    assert!(overlay_quads(&mut app).is_empty());
    app.control(Control::Select).unwrap();
    // O shows it outside Zone mode, and the sidebar row says so.
    route(&mut app, &key(Keycode::O)).unwrap();
    assert_eq!(overlay_quads(&mut app).len(), 48, "every tile of 8 x 6");
    assert!(sidebar_texts(&mut app).contains(&"Hide zones  O".to_owned()));
    // N enters Zone mode: shown whatever the toggle says, and O there only
    // explains itself.
    route(&mut app, &key(Keycode::N)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Zone);
    assert_eq!(overlay_quads(&mut app).len(), 48);
    let error = route(&mut app, &key(Keycode::O)).unwrap_err();
    assert!(error.contains("always"), "{error}");
    assert_eq!(overlay_quads(&mut app).len(), 48);
    // The toggle is kept across modes; O again hides it.
    route(&mut app, &key(Keycode::T)).unwrap();
    assert_eq!(overlay_quads(&mut app).len(), 48);
    route(&mut app, &key(Keycode::O)).unwrap();
    assert!(overlay_quads(&mut app).is_empty());
    assert!(sidebar_texts(&mut app).contains(&"Show zones  O".to_owned()));
    // The toolbar's Zone button enters Zone mode too.
    acknowledge_ui(&mut app);
    let at = row_pixel(&mut app, &Control::Zone);
    route(&mut app, &button(true, at)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Zone);
}

#[test]
fn zone_tints_sit_under_objects_and_never_reach_the_semantic_plane() {
    let (_temporary, mut app) = zone_application(|_| {});
    app.control(Control::Zone).unwrap();
    let overlay = crate::prototype::zone_overlay(&app);
    // One unpickable World-layer copy of each tile's cell quad, in the
    // World's draw order like the stroke preview, in its zone's tint.
    let tints = zones::assign_tints(["meadow", "meadow.pond", "path"]);
    let distinct: BTreeSet<usize> = tints.values().copied().collect();
    assert_eq!(distinct.len(), 3, "three zones, three tints");
    for copy in &overlay {
        assert!(!copy.pickable && copy.layer == Layer::World && copy.glow.is_none());
        let source = app
            .prepared
            .cell_scene
            .quads()
            .iter()
            .find(|quad| quad.positions == copy.positions)
            .unwrap();
        let key = &app.prepared.cell_picks[&source.semantic_id];
        let SelectionKey::Cell { x, y, .. } = key else {
            panic!("not a cell quad");
        };
        // The same World draw slot as the stroke preview's mark of that cell,
        // so a mark composed after the tint draws over it.
        let mark =
            crate::workspace_render::stroke_overlay(&app.prepared, &BTreeSet::from([key.clone()]));
        assert!(mark.iter().any(|m| m.positions == copy.positions
            && (m.depth, m.painter_order) == (copy.depth, copy.painter_order)));
        let zone = zones::tile_zone(&app.session.world().planes()["main"], 0, *x, *y).unwrap();
        assert_eq!(copy.color, zones::tint_color(tints[zone.id]));
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
    assert_ne!(shown.rgba8, plain.rgba8, "the tints are visible");
    assert_eq!(shown.semantic_ids, plain.semantic_ids);
    assert_eq!(shown.depth, plain.depth);
    assert_eq!(shown.coverage, plain.coverage);
    // Pixels an opaque object covers keep the object's colour: the tint is
    // drawn after its tile's terrain and before the object.
    let objects: BTreeSet<u32> = app
        .prepared
        .picks
        .iter()
        .filter(|(_, key)| matches!(key, SelectionKey::Object { .. }))
        .map(|(id, _)| id.0)
        .collect();
    let mut covered = 0;
    for (index, id) in plain.semantic_ids.iter().enumerate() {
        if objects.contains(id) && plain.rgba8[index * 4 + 3] == 255 {
            assert_eq!(
                shown.rgba8[index * 4..index * 4 + 4],
                plain.rgba8[index * 4..index * 4 + 4]
            );
            covered += 1;
        }
    }
    assert!(covered > 0, "the sample's markers stand in the view");
    // The comparison can fail: the same copies made pickable would write
    // their identities over the World's.
    let pickable = SceneBundle::from_quads(
        with.scene.revision(),
        0,
        with.scene.viewport(),
        with.scene.clear_color(),
        with.scene.quads().iter().map(|quad| Quad {
            pickable: quad.pickable
                || overlay
                    .iter()
                    .any(|o| o.positions == quad.positions && o.color == quad.color),
            ..*quad
        }),
        SceneLimits::default(),
    )
    .unwrap();
    assert_ne!(
        render(&pickable, with.resources).semantic_ids,
        plain.semantic_ids
    );
    // A click on a tinted tile still picks the same cell.
    let mask = app.prepared.mask(app.view.frame_revision, true).unwrap();
    let at = cell_pixel(&mut app, 1, 1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let picked = mask.pick(at[0] as u32, at[1] as u32).unwrap();
    assert!(matches!(
        picked,
        Some(SelectionKey::Cell { x: 1, y: 1, .. })
    ));
}

#[test]
fn the_legend_and_inspector_name_each_zone_and_tile() {
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::N)).unwrap();
    acknowledge_ui(&mut app);
    let texts = sidebar_texts(&mut app);
    for line in [
        "Zones: 3 of 3 in view (tiles)",
        "meadow = Quiet meadow (36)",
        "meadow.pond = Quiet meadow (4)",
        "path = Stone path (8)",
        "Default zone (clear): meadow",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    // Every zone row and the default row carry their overlay tint.
    app.refresh_ui();
    let tints = zones::assign_tints(["meadow", "meadow.pond", "path"]);
    for (control, zone) in [
        (
            Control::ZonePalette(ZoneChoice::Zone("meadow.pond".into())),
            "meadow.pond",
        ),
        (
            Control::ZonePalette(ZoneChoice::Zone("path".into())),
            "path",
        ),
        (Control::ZonePalette(ZoneChoice::Default), "meadow"),
    ] {
        let widget = app
            .ui
            .widgets
            .iter()
            .find(|widget| widget.control.as_ref() == Some(&control))
            .unwrap();
        assert_eq!(widget.swatch, Some(zones::swatch_color(tints[zone])));
    }
    // A press selects the tile even before a zone is chosen, and the
    // inspector shows its zone, the chain up and the inherited name.
    let at = cell_pixel(&mut app, 2, 2);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    let texts = sidebar_texts(&mut app);
    for line in [
        "Zone: meadow.pond",
        "Path: meadow > meadow.pond",
        "Zone name: Quiet meadow",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 6, 0);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Zone: meadow (plane default)".to_owned()));
    assert!(app.status_label().contains("Choose a zone"));
}

#[test]
fn a_zone_drag_through_sdl_events_is_one_reviewed_change() {
    let (temporary, mut app) = zone_application(|_| {});
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    route(&mut app, &key(Keycode::N)).unwrap();
    acknowledge_ui(&mut app);
    let row = row_pixel(
        &mut app,
        &Control::ZonePalette(ZoneChoice::Zone("meadow.pond".into())),
    );
    route(&mut app, &button(true, row)).unwrap();
    assert_eq!(
        app.tools.zone_choice(),
        Some(&ZoneChoice::Zone("meadow.pond".into()))
    );
    assert!(
        app.status_label()
            .contains("Drag to paint zone meadow.pond")
    );
    acknowledge_ui(&mut app);
    let (start, end) = (cell_pixel(&mut app, 6, 0), cell_pixel(&mut app, 6, 5));
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    let preview = app
        .session
        .stroke_changes(app.tools.stroke().unwrap())
        .unwrap();
    assert_eq!(preview, (0..6).map(|y| [6, y]).collect::<Vec<_>>());
    assert!(
        app.status_label()
            .starts_with("Painting 6 cells into zone meadow.pond, 6 to change"),
        "{}",
        app.status_label()
    );
    // The preview shows the result: each previewed cell is drawn once, in
    // the target zone's tint at the stronger preview opacity, replacing its
    // resting tint.
    let tints = zones::plane_tints(&app.session.world().planes()["main"])
        .into_iter()
        .map(|(zone, tint)| (zone.to_owned(), tint))
        .collect::<std::collections::BTreeMap<_, _>>();
    let cell_quad = |app: &Application, x: i64, y: i64| {
        *app.prepared
            .cell_scene
            .quads()
            .iter()
            .find(|quad| {
                matches!(app.prepared.cell_picks.get(&quad.semantic_id),
                    Some(SelectionKey::Cell { x: cx, y: cy, .. }) if (*cx, *cy) == (x, y))
            })
            .unwrap()
    };
    let previewed = cell_quad(&app, 6, 2);
    let on_cell: Vec<Quad> = overlay_quads(&mut app)
        .into_iter()
        .filter(|quad| quad.positions == previewed.positions)
        .collect();
    assert_eq!(on_cell.len(), 1, "{on_cell:?}");
    assert_eq!(on_cell[0].color, zones::preview_color(tints["meadow.pond"]));
    const { assert!(zones::PREVIEW_ALPHA >= 2.0 * zones::TINT_ALPHA) };
    route(&mut app, &button(false, end)).unwrap();
    assert!(app.tools.stroke().is_none());
    let sealed: serde_json::Value =
        serde_json::from_str(app.session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(sealed["operations"].as_array().unwrap().len(), 1);
    assert_eq!(sealed["operations"][0]["grid"], "zone");
    let cells: Vec<[i64; 2]> = sealed["operations"][0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            [
                cell["at"][0].as_i64().unwrap(),
                cell["at"][1].as_i64().unwrap(),
            ]
        })
        .collect();
    assert_eq!(cells, preview);
    assert!(
        app.review_lines(900)
            .iter()
            .any(|line| line.contains("Zones: 6 tiles move to meadow.pond (Quiet meadow)."))
    );
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(app.recovery.records().is_empty());
    // Enter accepts once the pending diff is displayed: one record.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(app.recovery.records().len(), 1);
    assert_ne!(fs::read(&chunk).unwrap(), original);
    let plane = &app.session.world().planes()["main"];
    assert!((0..6).all(|y| zones::cell_zone(plane, 0, 6, y) == Some("meadow.pond")));
    // Z undoes it byte for byte.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
fn without_the_grant_zone_mode_shows_zones_but_paints_nothing() {
    let (temporary, mut app) = zone_application(|world| {
        let path = world.join("preview.jsonc");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        catalog["editing"].as_object_mut().unwrap().remove("zones");
        fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    });
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    route(&mut app, &key(Keycode::N)).unwrap();
    assert_eq!(crate::prototype::zone_overlay(&app).len(), 48);
    assert!(app.status_label().contains("does not grant zone painting"));
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Zone painting not granted".to_owned()));
    assert!(!texts.iter().any(|text| text.starts_with("Brush")));
    // Palette rows are disabled: a press on one is a panel press.
    acknowledge_ui(&mut app);
    let row = row_pixel(
        &mut app,
        &Control::ZonePalette(ZoneChoice::Zone("path".into())),
    );
    route(&mut app, &button(true, row)).unwrap();
    assert_eq!(app.tools.zone_choice(), None);
    // A drag only selects the press tile; nothing is prepared or written.
    acknowledge_ui(&mut app);
    let (start, end) = (cell_pixel(&mut app, 6, 0), cell_pixel(&mut app, 6, 3));
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    route(&mut app, &button(false, end)).unwrap();
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    assert!(matches!(
        app.session.selection(),
        Some(SelectionKey::Cell { x: 6, y: 0, .. })
    ));
    // The brush keys do nothing here.
    let brush = app.tools.brush();
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    assert_eq!(app.tools.brush(), brush);
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

/// The colour of the composed frame at the centre of `cell`'s cell quad.
fn shown_at(app: &mut Application, x: i64, y: i64) -> [u8; 4] {
    let at = cell_pixel(app, x, y);
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    let frame = render(&presentation.scene, presentation.resources);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (at[1] as usize) * frame.width as usize + at[0] as usize;
    frame.rgba8[index * 4..index * 4 + 4].try_into().unwrap()
}
fn differs_visibly(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).map(|(a, b)| a.abs_diff(b)).max().unwrap() >= 24
}

#[test]
fn a_zone_stroke_previews_its_result_visibly_and_tints_never_move() {
    // A fourth zone that is in no tile: painting it must not recolour any
    // other zone, pending, accepted, cancelled or undone.
    let (_temporary, mut app) = zone_application(|world| {
        let path = world.join("planes/main/zones.jsonc");
        let text = fs::read_to_string(&path).unwrap().replace(
            "\"zones\": [",
            "\"zones\": [\n    {\"id\": \"forest\", \"name\": \"Old forest\"},",
        );
        fs::write(path, text).unwrap();
    });
    route(&mut app, &key(Keycode::N)).unwrap();
    acknowledge_ui(&mut app);
    let tints_of = |app: &mut Application| -> std::collections::BTreeMap<[i64; 2], [u32; 4]> {
        crate::prototype::zone_overlay(app);
        app.refresh_ui();
        let mut by_cell = std::collections::BTreeMap::new();
        for quad in crate::prototype::zone_overlay(app) {
            let cell = app
                .prepared
                .cell_scene
                .quads()
                .iter()
                .find(|cell| cell.positions == quad.positions)
                .and_then(|cell| app.prepared.cell_picks.get(&cell.semantic_id))
                .cloned();
            if let Some(SelectionKey::Cell { x, y, .. }) = cell {
                by_cell.insert([x, y], quad.color.map(f32::to_bits));
            }
        }
        by_cell
    };
    let before = tints_of(&mut app);
    assert_eq!(before.len(), 48);
    let sand_meadow = shown_at(&mut app, 6, 1);
    let resting_pond = shown_at(&mut app, 1, 1);
    let row = row_pixel(
        &mut app,
        &Control::ZonePalette(ZoneChoice::Zone("forest".into())),
    );
    route(&mut app, &button(true, row)).unwrap();
    acknowledge_ui(&mut app);
    let (start, end) = (cell_pixel(&mut app, 6, 0), cell_pixel(&mut app, 6, 3));
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    // Mid-gesture the previewed cell differs visibly from its old tint and
    // from a resting tile; every other tile keeps its colour.
    let previewed = shown_at(&mut app, 6, 1);
    assert!(
        differs_visibly(previewed, sand_meadow),
        "{previewed:?} {sand_meadow:?}"
    );
    assert!(differs_visibly(previewed, resting_pond));
    let during = tints_of(&mut app);
    assert!(
        (0..4).all(|y| !during.contains_key(&[6, y])),
        "previewed cells are not tinted twice"
    );
    assert!(during.iter().all(|(cell, tint)| before[cell] == *tint));
    route(&mut app, &button(false, end)).unwrap();
    // Pending: forest appears in view; nothing else changes colour.
    let unchanged = |app: &mut Application| {
        let now = tints_of(app);
        now.iter()
            .filter(|(cell, _)| !(cell[0] == 6 && cell[1] < 4))
            .all(|(cell, tint)| before[cell] == *tint)
    };
    assert!(unchanged(&mut app));
    let pending = tints_of(&mut app);
    assert_ne!(pending[&[6, 0]], before[&[6, 0]]);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert_eq!(tints_of(&mut app), before, "cancel");
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    route(&mut app, &button(false, end)).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(unchanged(&mut app), "accepted");
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    assert!(unchanged(&mut app), "undo pending");
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(tints_of(&mut app), before, "undone");
    // The legend's swatch is the overlay's colour for every zone.
    let tints = zones::plane_tints(&app.session.history().document().world().planes()["main"])
        .into_iter()
        .map(|(zone, tint)| (zone.to_owned(), tint))
        .collect::<std::collections::BTreeMap<_, _>>();
    app.refresh_ui();
    for zone in ["forest", "meadow", "meadow.pond", "path"] {
        let widget = app
            .ui
            .widgets
            .iter()
            .find(|w| w.control == Some(Control::ZonePalette(ZoneChoice::Zone(zone.into()))))
            .unwrap();
        assert_eq!(
            widget.swatch,
            Some(zones::swatch_color(tints[zone])),
            "{zone}"
        );
    }
    let meadow = zones::tint_color(tints["meadow"]).map(f32::to_bits);
    assert_eq!(before[&[6, 0]], meadow);
}

#[test]
fn the_terrain_mark_draws_over_the_zone_tint_of_the_same_cell() {
    let (_temporary, mut app) = zone_application(|_| {});
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    route(&mut app, &key(Keycode::O)).unwrap();
    acknowledge_ui(&mut app);
    let (start, end) = (cell_pixel(&mut app, 0, 1), cell_pixel(&mut app, 2, 1));
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    let quads = presentation.scene.quads();
    let marked = app
        .prepared
        .cell_scene
        .quads()
        .iter()
        .find(|quad| {
            matches!(
                app.prepared.cell_picks.get(&quad.semantic_id),
                Some(SelectionKey::Cell { x: 1, y: 1, .. })
            )
        })
        .unwrap()
        .positions;
    let same_cell = |color: [f32; 4]| {
        quads
            .iter()
            .position(|quad| quad.positions == marked && quad.color == color)
            .unwrap()
    };
    let tints = zones::plane_tints(&app.session.world().planes()["main"]);
    let tint = same_cell(zones::tint_color(tints["meadow.pond"]));
    let mark = same_cell(crate::workspace_render::STROKE_PREVIEW_TINT);
    assert!(
        mark > tint,
        "the mark is drawn after the tint of its own cell"
    );
}

#[test]
fn long_ids_that_differ_at_the_end_stay_distinguishable_at_the_minimum_width() {
    const A: &str = "highlands.lower_river_valley.northern_bank_section_a";
    const B: &str = "highlands.lower_river_valley.northern_bank_section_b";
    let (_temporary, mut app) = zone_application(|world| {
        fs::write(
            world.join("planes/main/zones.jsonc"),
            serde_json::json!({
                "format": "tilewright.zones/1",
                "zones": [
                    {"id": "highlands", "name": "Highlands"},
                    {"id": "highlands.lower_river_valley", "parent": "highlands"},
                    {"id": A, "parent": "highlands.lower_river_valley"},
                    {"id": B, "parent": "highlands.lower_river_valley"},
                    {"id": "path", "name": "Stone path"},
                ]
            })
            .to_string(),
        )
        .unwrap();
        let plane = world.join("planes/main/plane.jsonc");
        let text = fs::read_to_string(&plane)
            .unwrap()
            .replace("\"meadow\"", &format!("\"{A}\""));
        fs::write(plane, text).unwrap();
        let chunk = world.join("planes/main/layers/0/0_0.jsonc");
        let text = fs::read_to_string(&chunk)
            .unwrap()
            .replace("\"meadow.pond\"", &format!("\"{B}\""));
        fs::write(chunk, text).unwrap();
    });
    app.ui = WorkspaceUi::new((720, 480), (720, 480));
    app.view.camera.width = 720;
    app.view.camera.height = 480;
    app.rebuild().unwrap();
    app.control(Control::Zone).unwrap();
    let rows = |app: &mut Application, zone: &str| {
        app.refresh_ui();
        app.ui
            .widgets
            .iter()
            .find(|w| w.control == Some(Control::ZonePalette(ZoneChoice::Zone(zone.into()))))
            .map(|w| w.text.clone())
            .unwrap()
    };
    let (a, b) = (rows(&mut app, A), rows(&mut app, B));
    assert_ne!(a, b);
    assert!(a.ends_with("_a") && b.ends_with("_b"), "{a} / {b}");
    assert!(a.starts_with("highlands") && a.contains("..."), "{a}");
    // The inspector's path line keeps the last zone whole.
    let path_line = |app: &mut Application, x: i64, y: i64| {
        let generation = app.view.frame_revision;
        app.action(tilewright_editor_ui::Action::Select {
            generation,
            key: SelectionKey::Cell {
                plane: "main".into(),
                layer: 0,
                grid: "floor".into(),
                x,
                y,
            },
        })
        .unwrap();
        sidebar_texts(app)
            .into_iter()
            .find(|text| text.starts_with("Path: "))
            .unwrap()
    };
    let (a, b) = (path_line(&mut app, 6, 0), path_line(&mut app, 1, 1));
    assert_ne!(a, b);
    assert!(a.ends_with("_a") && b.ends_with("_b"), "{a} / {b}");
    let zone_line = sidebar_texts(&mut app)
        .into_iter()
        .find(|text| text.starts_with("Zone: "))
        .unwrap();
    assert!(zone_line.ends_with("_b"), "{zone_line}");
}

/// The neutral sample as bent isometric terrain whose row 2 has no ground
/// east of the marker, in the `path` zone, next to the marker and the
/// painted tiles around it.
fn isometric_with_empty_run(world: &Path) {
    let profile = world.join("profile/profile.jsonc");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    value["projection"] = serde_json::json!({"kind":"isometric","tile_width":64,"tile_height":32});
    fs::write(&profile, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let chunk = world.join("planes/main/layers/0/0_0.jsonc");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&chunk).unwrap()).unwrap();
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y| (0..32).map(|x| (x * 7 + y * 13) % 5 * 6).collect())
        .collect();
    value["terrain"]["height"] = serde_json::json!({ "rows": rows });
    value["terrain"]["floor"]["rows"][2] = serde_json::json!(format!("gggg{}", ".".repeat(28)));
    value["terrain"]["zone"]["rows"][2] = serde_json::json!(format!(".pptttt{}", ".".repeat(25)));
    fs::write(&chunk, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn tints_of_tiles_without_ground_keep_their_own_place_in_the_draw_order() {
    let (_temporary, mut app) = zone_application(isometric_with_empty_run);
    app.control(Control::Zone).unwrap();
    let empty: Vec<[i64; 2]> = (4..8).map(|x| [x, 2]).collect();
    let overlay = crate::prototype::zone_overlay(&app);
    let is_empty = |quad: &Quad| {
        app.prepared
            .cell_scene
            .quads()
            .iter()
            .find(|cell| cell.positions == quad.positions)
            .and_then(|cell| app.prepared.cell_picks.get(&cell.semantic_id))
            .is_some_and(
                |key| matches!(key, SelectionKey::Cell { x, y, .. } if empty.contains(&[*x, *y])),
            )
    };
    let others: Vec<Quad> = overlay.iter().copied().filter(|q| !is_empty(q)).collect();
    assert!(others.len() < overlay.len(), "the empty run has tints");
    let world = &app.prepared.scene;
    let identity = world.quads().iter().map(|q| q.semantic_id.0).max().unwrap();
    let draw = |quads: &[Quad]| {
        let scene = SceneBundle::from_quads(
            world.revision(),
            0,
            world.viewport(),
            world.clear_color(),
            world
                .quads()
                .iter()
                .copied()
                .chain(quads.iter().zip(identity + 1..).map(|(quad, id)| Quad {
                    semantic_id: tilewright_scene::SemanticId(id),
                    part: 0,
                    ..*quad
                })),
            SceneLimits::default(),
        )
        .unwrap();
        render(&scene, app.prepared.resources.clone())
    };
    let with_run = draw(&overlay);
    let without_run = draw(&others);
    let world_mask = app.frame(false).unwrap();
    let cell_mask = app.frame(true).unwrap();
    let diagonal = |key: Option<&SelectionKey>| match key {
        Some(SelectionKey::Cell { x, y, .. }) => Some(x + y),
        _ => None,
    };
    let (mut visible, mut guarded, mut behind) = (0, 0, 0);
    for index in 0..world_mask.semantic_ids.len() {
        let pixel = index * 4..index * 4 + 4;
        let changed = with_run.rgba8[pixel.clone()] != without_run.rgba8[pixel.clone()];
        let owner = app.prepared.picks.get(&tilewright_scene::SemanticId(
            world_mask.semantic_ids[index],
        ));
        let cell = app
            .prepared
            .cell_picks
            .get(&tilewright_scene::SemanticId(cell_mask.semantic_ids[index]));
        let in_run =
            matches!(cell, Some(SelectionKey::Cell { x, y, .. }) if empty.contains(&[*x, *y]));
        match owner {
            // No run tint ever lands on an object...
            Some(SelectionKey::Object { .. }) => {
                assert!(!changed, "a run tint covers an object at {index}");
                guarded += 1;
            }
            // ...or on a tile nearer than the run cell there.
            Some(tile @ SelectionKey::Cell { .. }) if diagonal(Some(tile)) > diagonal(cell) => {
                assert!(!changed, "a run tint covers a nearer tile at {index}");
                guarded += 1;
            }
            // A farther tile showing through a run cell is drawn before
            // that cell, so the run's tint covers it as ground there would.
            Some(tile @ SelectionKey::Cell { .. })
                if in_run && diagonal(Some(tile)) < diagonal(cell) =>
            {
                assert!(changed, "a run tint is hidden by a farther tile at {index}");
                behind += 1;
            }
            // Where nothing of the World is drawn, the run's tint shows.
            None if in_run => {
                assert!(changed, "a run tint is hidden at {index}");
                visible += 1;
            }
            _ => {}
        }
    }
    assert!(
        visible > 0 && guarded > 0 && behind > 0,
        "{visible} {guarded} {behind}"
    );
}

#[test]
fn an_empty_tile_is_drawn_after_farther_objects_not_at_an_earlier_tiles_place() {
    let (_temporary, mut app) = zone_application(|world| {
        isometric_with_empty_run(world);
        // No ground east of x = 3 below row 0, and a lamp standing on the
        // empty tile (6, 4), two diagonals behind the empty tile (7, 5).
        let chunk = world.join("planes/main/layers/0/0_0.jsonc");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&chunk).unwrap()).unwrap();
        for y in 1..6 {
            value["terrain"]["floor"]["rows"][y] =
                serde_json::json!(format!("gggg{}", ".".repeat(28)));
        }
        value["objects"].as_array_mut().unwrap().push(
            serde_json::json!({"uid": "lamp_far", "at": [6, 4], "proto": "lamp", "layer": "item"}),
        );
        fs::write(&chunk, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    });
    app.control(Control::Zone).unwrap();
    let lamp = app
        .prepared
        .picks
        .iter()
        .find(|(_, key)| matches!(key, SelectionKey::Object { uid } if uid == "lamp_far"))
        .map(|(id, _)| *id)
        .unwrap();
    let lamp_order = app
        .prepared
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.semantic_id == lamp)
        .map(|quad| quad.painter_order)
        .max()
        .unwrap();
    let near = app
        .prepared
        .cell_picks
        .iter()
        .find(|(_, key)| matches!(key, SelectionKey::Cell { x: 7, y: 5, .. }))
        .map(|(id, _)| *id)
        .unwrap();
    let tint = crate::prototype::zone_overlay(&app)
        .into_iter()
        .find(|quad| {
            app.prepared
                .cell_scene
                .quads()
                .iter()
                .any(|cell| cell.semantic_id == near && cell.positions == quad.positions)
        })
        .unwrap();
    // The farther lamp is drawn before the nearer empty tile's tint, as it
    // would be before ground there; nearest earlier ground is at (3, 5),
    // two diagonals further back, which the lamp is drawn after.
    assert!(
        tint.painter_order >= lamp_order,
        "{} < {lamp_order}",
        tint.painter_order
    );
}
