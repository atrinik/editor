// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The live brush-stroke preview through real SDL pointer and key events,
//! on the same CPU scene path as the scripted demonstration.
use super::{BRUSH_WINDOW, acknowledge_ui, application_with_source, brush_application, cell_pixel};
use crate::prototype::Application;
use sdl3::{event::Event, keyboard::Keycode, mouse::MouseButton};
use std::collections::BTreeSet;
use std::{fs, path::Path};
use tilewright_editor_preview::SelectionKey;
use tilewright_render_api::{FrameOutput, RenderRequest, Renderer};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_scene::{Layer, Quad, SceneBundle, SceneLimits, SemanticId};

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
/// Motion with the left button held, as SDL reports a drag.
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
/// Route one event as the native loop does: the text path first, then the
/// window-free pointer and key routing.
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
/// The overlay quads of the frame the native loop would present now.
fn presented_overlay(app: &mut Application) -> Vec<Quad> {
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.layer == Layer::Effect)
        .copied()
        .collect()
}
/// The exact cell-scene geometry of `cells`, in a comparable order.
fn geometry(quads: impl IntoIterator<Item = Quad>) -> Vec<[[u32; 2]; 4]> {
    let mut corners: Vec<_> = quads
        .into_iter()
        .map(|quad| quad.positions.map(|corner| corner.map(f32::to_bits)))
        .collect();
    corners.sort_unstable();
    corners
}
fn cell_geometry(app: &Application, cells: &[[i64; 2]]) -> Vec<[[u32; 2]; 4]> {
    geometry(
        app.prepared
            .cell_scene
            .quads()
            .iter()
            .copied()
            .filter(|quad| {
                matches!(
                    app.prepared.cell_picks.get(&quad.semantic_id),
                    Some(SelectionKey::Cell { x, y, .. }) if cells.contains(&[*x, *y])
                )
            }),
    )
}
/// What the stroke would change if released now.
fn previewed(app: &Application) -> Vec<[i64; 2]> {
    app.session
        .stroke_changes(app.tools.stroke().unwrap())
        .unwrap()
}
/// The `at` of every cell in the pending `terrain.set`, in order.
fn prepared_cells(app: &Application) -> Vec<[i64; 2]> {
    let sealed: serde_json::Value =
        serde_json::from_str(app.session.history().pending().unwrap().sealed()).unwrap();
    sealed["operations"][0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            [
                cell["at"][0].as_i64().unwrap(),
                cell["at"][1].as_i64().unwrap(),
            ]
        })
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

#[test]
fn a_drag_previews_what_its_release_prepares_without_preparing_the_world_again() {
    let (temporary, mut app) = brush_application();
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    assert_eq!(app.tools.brush().radius, 1);
    acknowledge_ui(&mut app);
    let points = [(0, 2), (2, 2), (5, 2), (6, 4)].map(|(x, y)| cell_pixel(&mut app, x, y));
    route(&mut app, &button(true, points[0])).unwrap();
    // The press selects and prepares through the ordinary path once; from
    // here every pointer sample reuses that prepared World scene.
    let prepared = app.prepared_scenes;
    let world = app.prepared.scene.quads().to_vec();
    let mut overlay = presented_overlay(&mut app);
    assert_eq!(previewed(&app).len(), 6, "a 3x3 square clipped at x = 0");
    assert_eq!(
        geometry(overlay.clone()),
        cell_geometry(&app, &previewed(&app))
    );
    for sample in [points[1], [1270.0, 400.0], points[2], points[2], points[3]] {
        route(&mut app, &drag(sample)).unwrap();
        let next = presented_overlay(&mut app);
        let cells = previewed(&app);
        assert_eq!(geometry(next.clone()), cell_geometry(&app, &cells));
        assert!(next.len() >= overlay.len(), "a stroke only grows");
        overlay = next;
        assert!(next_is_unpickable_effect(&overlay));
        assert_eq!(app.prepared_scenes, prepared, "no World prepare per sample");
        assert_eq!(app.prepared.scene.quads(), world.as_slice());
        assert!(app.session.history().pending().is_none());
        assert_eq!(app.session.history().depths(), (0, 0));
        assert!(app.recovery.records().is_empty());
        assert!(
            app.status_label()
                .contains(&format!(", {} to change;", cells.len())),
            "{}",
            app.status_label()
        );
    }
    // Sand cells the stroke covers would not change and are not marked;
    // the empty cell (6, 4) would and is.
    let preview = previewed(&app);
    let covered: Vec<_> = app.tools.stroke().unwrap().cells().collect();
    assert!(covered.contains(&[5, 2]) && !preview.contains(&[5, 2]));
    assert!(preview.contains(&[6, 4]) && preview.contains(&[0, 1]));
    assert_eq!(fs::read(&chunk).unwrap(), original);
    // Release prepares exactly the previewed cells, and the preview ends.
    route(&mut app, &button(false, points[3])).unwrap();
    assert!(app.tools.stroke().is_none());
    assert_eq!(prepared_cells(&app), preview);
    assert!(presented_overlay(&mut app).is_empty());
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

fn next_is_unpickable_effect(overlay: &[Quad]) -> bool {
    overlay
        .iter()
        .all(|quad| !quad.pickable && quad.layer == Layer::Effect && quad.glow.is_none())
}

#[test]
fn escape_removes_the_preview_and_a_pending_change_allows_none() {
    let (_temporary, mut app) = brush_application();
    let revision = app.session.world().source_digest();
    let start = cell_pixel(&mut app, 1, 1);
    let end = cell_pixel(&mut app, 3, 1);
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
    assert_eq!(presented_overlay(&mut app).len(), 3);
    let prepared = app.prepared_scenes;
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.tools.stroke().is_none());
    assert!(presented_overlay(&mut app).is_empty());
    // The release that follows Esc does nothing either.
    route(&mut app, &button(false, end)).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.world().source_digest(), revision);
    assert_eq!(app.prepared_scenes, prepared);
    assert!(app.notice.as_deref().unwrap().contains("cancelled"));

    // While a released stroke is pending, no new stroke (and so no preview)
    // can begin, and the pending diff is untouched.
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &button(false, start)).unwrap();
    let pending = app.session.history().pending().unwrap().sealed().to_owned();
    acknowledge_ui(&mut app);
    let error = route(&mut app, &button(true, end)).unwrap_err();
    assert!(error.contains("Accept or cancel"), "{error}");
    route(&mut app, &drag(start)).unwrap();
    assert!(app.tools.stroke().is_none());
    assert!(presented_overlay(&mut app).is_empty());
    assert_eq!(app.session.history().pending().unwrap().sealed(), pending);
}

#[test]
fn the_preview_never_reaches_a_semantic_plane_or_changes_what_a_click_picks() {
    let (_temporary, mut app) = brush_application();
    let start = cell_pixel(&mut app, 0, 3);
    let end = cell_pixel(&mut app, 3, 3);
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &drag(end)).unwrap();
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
    // The preview is visible on every pixel the marked cells pick...
    let marked: BTreeSet<u32> = app
        .prepared
        .cell_picks
        .iter()
        .filter(|(_, key)| matches!(key, SelectionKey::Cell { x, y: 3, .. } if (0..4).contains(x)))
        .map(|(id, _)| id.0)
        .collect();
    assert_eq!(marked.len(), 4);
    let mask = app.prepared.mask(app.view.frame_revision, true).unwrap();
    let cells = app.frame(true).unwrap();
    let mut tinted = 0;
    for (index, id) in cells.semantic_ids.iter().enumerate() {
        let (x, y) = (
            u32::try_from(index % 1280).unwrap(),
            u32::try_from(index / 1280).unwrap(),
        );
        if marked.contains(id) && app.ui.hit(x, y) == crate::workspace_ui::Hit::World(x, y) {
            assert_ne!(
                shown.rgba8[index * 4..index * 4 + 4],
                plain.rgba8[index * 4..index * 4 + 4]
            );
            // ...and a click there still picks the same cell.
            assert_eq!(
                mask.pick(x, y).unwrap(),
                app.prepared.cell_picks.get(&SemanticId(*id)).cloned()
            );
            tinted += 1;
        }
    }
    assert!(tinted > 0);
    // ...yet the semantic, depth and coverage planes are exactly those of
    // the frame without it.
    assert_eq!(shown.semantic_ids, plain.semantic_ids);
    assert_eq!(shown.depth, plain.depth);
    assert_eq!(shown.coverage, plain.coverage);
    // The comparison can fail: the same quads made pickable would write
    // their identities over the World's.
    let pickable = SceneBundle::from_quads(
        with.scene.revision(),
        0,
        with.scene.viewport(),
        with.scene.clear_color(),
        with.scene.quads().iter().map(|quad| Quad {
            pickable: quad.pickable || quad.layer == Layer::Effect,
            ..*quad
        }),
        SceneLimits::default(),
    )
    .unwrap();
    let picked = render(&pickable, with.resources);
    assert_ne!(picked.semantic_ids, plain.semantic_ids);
}

/// The neutral sample as an isometric view whose tiles have uneven
/// heights, so the shared compiler bends and stretches them.
fn isometric_heights(world: &Path) {
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
    fs::write(&chunk, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// The pixels `overlay` changes when drawn over the prepared World, and
/// the pixels its quads cover when rendered alone as pickable quads.
fn changed_and_covered(app: &Application, overlay: &[Quad]) -> (BTreeSet<usize>, BTreeSet<usize>) {
    let world = &app.prepared.scene;
    let identity = world.quads().iter().map(|q| q.semantic_id.0).max().unwrap();
    let scene = |quads: Vec<Quad>| {
        SceneBundle::from_quads(
            world.revision(),
            0,
            world.viewport(),
            world.clear_color(),
            quads,
            SceneLimits::default(),
        )
        .unwrap()
    };
    let numbered = |offset: u32| {
        overlay
            .iter()
            .zip(offset..)
            .map(|(quad, id)| Quad {
                semantic_id: SemanticId(id),
                part: 0,
                ..*quad
            })
            .collect::<Vec<_>>()
    };
    let plain = render(world, app.prepared.resources.clone());
    let shown = render(
        &scene(
            world
                .quads()
                .iter()
                .copied()
                .chain(numbered(identity + 1))
                .collect(),
        ),
        app.prepared.resources.clone(),
    );
    let alone = render(
        &scene(
            numbered(1)
                .into_iter()
                .map(|quad| Quad {
                    pickable: true,
                    ..quad
                })
                .collect(),
        ),
        app.prepared.resources.clone(),
    );
    let changed: BTreeSet<usize> = (0..plain.semantic_ids.len())
        .filter(|&i| plain.rgba8[i * 4..i * 4 + 4] != shown.rgba8[i * 4..i * 4 + 4])
        .collect();
    let covered: BTreeSet<usize> = (0..alone.semantic_ids.len())
        .filter(|&i| alone.semantic_ids[i] != 0)
        .collect();
    (changed, covered)
}

#[test]
fn the_preview_sits_exactly_on_bent_isometric_cells() {
    let (_temporary, app) = application_with_source(isometric_heights);
    let cells: BTreeSet<SelectionKey> = app
        .prepared
        .cell_picks
        .values()
        .filter(|key| matches!(key, SelectionKey::Cell { x, y, .. } if (1..5).contains(x) && (1..4).contains(y)))
        .cloned()
        .collect();
    assert_eq!(cells.len(), 12);
    let overlay = crate::workspace_render::stroke_overlay(
        &app.prepared.cell_scene,
        &app.prepared.cell_picks,
        &cells,
    );
    let source: Vec<Quad> = app
        .prepared
        .cell_scene
        .quads()
        .iter()
        .filter(|quad| cells.contains(&app.prepared.cell_picks[&quad.semantic_id]))
        .copied()
        .collect();
    // Height differences split tiles into several bent quads, and the
    // preview copies each one: same corners, art, frame, part and order.
    assert!(source.len() > cells.len(), "{} quads", source.len());
    assert_eq!(overlay.len(), source.len());
    for (copy, quad) in overlay.iter().zip(&source) {
        assert_eq!(
            Quad {
                color: quad.color,
                pickable: quad.pickable,
                layer: quad.layer,
                ..*copy
            },
            *quad
        );
        assert!(!copy.pickable && copy.layer == Layer::Effect);
    }
    // Drawn over the World, it changes exactly the pixels those cells'
    // projected quads cover: rendered on their own they mark the same set.
    let (changed, covered) = changed_and_covered(&app, &overlay);
    assert!(!covered.is_empty());
    assert_eq!(changed, covered);
    // Every pixel the exact cell mask gives one of these cells is marked.
    let mask = app.frame(true).unwrap();
    let ids: BTreeSet<u32> = app
        .prepared
        .cell_picks
        .iter()
        .filter(|(_, key)| cells.contains(key))
        .map(|(id, _)| id.0)
        .collect();
    assert!(
        (0..mask.semantic_ids.len())
            .filter(|&i| ids.contains(&mask.semantic_ids[i]))
            .all(|i| changed.contains(&i))
    );
}

#[test]
fn a_preview_past_the_scene_limits_is_left_out_and_reported_not_blocking() {
    let (_temporary, mut app) = brush_application();
    app.refresh_ui();
    // Far more cell copies than a frame may hold.
    let quads = app.prepared.cell_scene.quads();
    let overlay: Vec<Quad> = quads
        .iter()
        .cycle()
        .take(SceneLimits::default().maximum_quads + 1)
        .copied()
        .collect();
    assert!(
        crate::workspace_render::compose_with_overlay(
            &mut app.text,
            &app.ui,
            &app.prepared.scene,
            app.prepared.resources.clone(),
            &overlay,
        )
        .is_err()
    );
    let (presentation, omitted) =
        crate::prototype::compose_with_preview(&mut app.text, &app.ui, &app.prepared, &overlay)
            .unwrap();
    assert!(omitted);
    assert!(
        presentation
            .scene
            .quads()
            .iter()
            .all(|quad| quad.layer != Layer::Effect)
    );
    // A real stroke's preview fits and is drawn.
    let start = cell_pixel(&mut app, 0, 0);
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, start)).unwrap();
    assert_eq!(presented_overlay(&mut app).len(), 1);
    assert!(!app.preview_omitted);
    assert!(!app.status_label().contains("preview"));
}
