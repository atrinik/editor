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
        .filter(|quad| is_preview(quad))
        .copied()
        .collect()
}
/// Whether a composed quad is a stroke-preview copy.
fn is_preview(quad: &Quad) -> bool {
    quad.color == crate::workspace_render::STROKE_PREVIEW_TINT && !quad.pickable
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
    // The prepared scenes themselves stay the same objects: a re-prepare,
    // even one that is not counted, would allocate new ones.
    let identity = (
        app.prepared.scene.quads().as_ptr(),
        app.prepared.cell_scene.quads().as_ptr(),
    );
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
        assert!(next_is_unpickable_world(&overlay));
        assert_eq!(app.prepared_scenes, prepared, "no World prepare per sample");
        assert_eq!(
            (
                app.prepared.scene.quads().as_ptr(),
                app.prepared.cell_scene.quads().as_ptr()
            ),
            identity
        );
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

fn next_is_unpickable_world(overlay: &[Quad]) -> bool {
    overlay
        .iter()
        .all(|quad| !quad.pickable && quad.layer == Layer::World && quad.glow.is_none())
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
    // The preview tints only pixels the marked cells pick, at least one of
    // each, and only where the World shows that cell's terrain: the marker
    // standing on (1, 4) reaches up over (1, 3) and hides the mark there.
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
    let world = app.frame(false).unwrap();
    let mut tinted = BTreeSet::new();
    let mut hidden = 0;
    for (index, id) in cells.semantic_ids.iter().enumerate() {
        let (x, y) = (
            u32::try_from(index % 1280).unwrap(),
            u32::try_from(index / 1280).unwrap(),
        );
        let changed =
            shown.rgba8[index * 4..index * 4 + 4] != plain.rgba8[index * 4..index * 4 + 4];
        if !marked.contains(id) {
            assert!(!changed, "pixel {x},{y} of an unmarked cell");
            continue;
        }
        // A click on a marked cell still picks that cell.
        assert_eq!(
            mask.pick(x, y).unwrap(),
            app.prepared.cell_picks.get(&SemanticId(*id)).cloned()
        );
        if app.ui.hit(x, y) != crate::workspace_ui::Hit::World(x, y) {
            continue;
        }
        let owner = app
            .prepared
            .picks
            .get(&SemanticId(world.semantic_ids[index]));
        if changed {
            assert!(matches!(owner, Some(SelectionKey::Cell { y: 3, .. })));
            tinted.insert(*id);
        } else {
            assert!(matches!(owner, Some(SelectionKey::Object { .. })));
            hidden += 1;
        }
    }
    assert_eq!(tinted, marked);
    assert!(hidden > 0);
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
            pickable: quad.pickable || is_preview(quad),
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
#[allow(clippy::too_many_lines)]
fn the_preview_sits_on_bent_isometric_cells_under_nearer_tiles_and_objects() {
    let (_temporary, app) = application_with_source(isometric_heights);
    // Cells around both markers, and the sample's one empty floor cell.
    let cells: BTreeSet<SelectionKey> = app
        .prepared
        .cell_picks
        .values()
        .filter(|key| matches!(key, SelectionKey::Cell { x, y, .. } if (1..8).contains(x) && (1..5).contains(y)))
        .cloned()
        .collect();
    assert_eq!(cells.len(), 28);
    let empty = cells
        .iter()
        .find(|key| matches!(key, SelectionKey::Cell { x: 6, y: 4, .. }))
        .unwrap();
    assert!(!app.prepared.picks.values().any(|key| key == empty));
    let overlay = crate::workspace_render::stroke_overlay(&app.prepared, &cells);
    let source: Vec<Quad> = app
        .prepared
        .cell_scene
        .quads()
        .iter()
        .filter(|quad| cells.contains(&app.prepared.cell_picks[&quad.semantic_id]))
        .copied()
        .collect();
    // Height differences split tiles into several bent quads, and the
    // preview copies each one: same corners, art, frame and part.
    assert!(source.len() > cells.len(), "{} quads", source.len());
    assert_eq!(overlay.len(), source.len());
    let world_order = |key: &SelectionKey| {
        app.prepared
            .scene
            .quads()
            .iter()
            .filter(|quad| app.prepared.picks.get(&quad.semantic_id) == Some(key))
            .map(|quad| quad.painter_order)
            .max()
    };
    for (copy, quad) in overlay.iter().zip(&source) {
        assert_eq!(
            Quad {
                color: quad.color,
                pickable: quad.pickable,
                layer: quad.layer,
                depth: quad.depth,
                painter_order: quad.painter_order,
                ..*copy
            },
            *quad
        );
        assert!(!copy.pickable && copy.layer == Layer::World);
        // Each copy takes its cell's own place in the World's draw order.
        let key = &app.prepared.cell_picks[&quad.semantic_id];
        if let Some(order) = world_order(key) {
            assert_eq!(copy.painter_order, order);
        }
    }
    let (changed, covered) = changed_and_covered(&app, &overlay);
    // Every tinted pixel is one the World shows as a changed cell's own
    // terrain, or as nothing (an empty changed cell), or one the exact
    // click mask gives to a changed cell while the World shows a farther
    // tile there: the sample's square terrain art and the mask's opaque
    // diamonds differ at tile edges. A tint never lies on an object or on
    // a nearer unchanged tile.
    let mask = app.frame(true).unwrap();
    let world = app.frame(false).unwrap();
    assert!(!changed.is_empty());
    let owner = |i: usize| app.prepared.picks.get(&SemanticId(world.semantic_ids[i]));
    let clicked = |i: usize| {
        app.prepared
            .cell_picks
            .get(&SemanticId(mask.semantic_ids[i]))
    };
    let diagonal = |key: &SelectionKey| match key {
        SelectionKey::Cell { x, y, .. } => x + y,
        SelectionKey::Object { .. } => unreachable!(),
    };
    let mut edges = 0;
    for &i in &changed {
        match owner(i) {
            None => {}
            Some(key) if cells.contains(key) => {}
            Some(SelectionKey::Object { .. }) => panic!("pixel {i} tints an object"),
            Some(farther) => {
                let changed_cell = clicked(i).filter(|key| cells.contains(key));
                let changed_cell = changed_cell.unwrap_or_else(|| panic!("pixel {i}: {farther:?}"));
                assert!(
                    diagonal(farther) < diagonal(changed_cell),
                    "{farther:?} is nearer"
                );
                edges += 1;
            }
        }
    }
    // Those edge pixels are few; the rest show the changed cells themselves.
    assert!(edges * 20 < changed.len(), "{edges} of {}", changed.len());
    // Those cells' shapes do reach under objects and nearer tiles, which
    // hide the mark there, as they hide the terrain.
    let hidden: Vec<usize> = covered.difference(&changed).copied().collect();
    assert!(
        hidden
            .iter()
            .any(|&i| matches!(owner(i), Some(SelectionKey::Object { .. })))
    );
    assert!(
        hidden
            .iter()
            .any(|&i| owner(i).is_some_and(|key| !cells.contains(key)))
    );
    // The empty cell is marked.
    let empty_id = app
        .prepared
        .cell_picks
        .iter()
        .find(|(_, key)| *key == empty)
        .unwrap()
        .0
        .0;
    assert!(changed.iter().any(|&i| mask.semantic_ids[i] == empty_id));
    let shown = shown_with(&app, &overlay);
    let alpha = crate::workspace_render::STROKE_PREVIEW_TINT[3];
    let mut doubled = Vec::new();
    for &i in &changed {
        let once = (0..3).all(|channel| {
            let base = f32::from(world.rgba8[i * 4 + channel]);
            let actual = f32::from(shown.rgba8[i * 4 + channel]);
            (actual - (base + (255.0 - base) * alpha)).abs() <= 2.0
        });
        if !once {
            doubled.push(i);
        }
    }
    // Two marks overlap only in those edge slivers, where a nearer changed
    // cell's mask shape reaches past what its terrain art covers; wherever
    // the World and the mask agree on the cell, it is tinted exactly once.
    assert!(doubled.iter().all(|&i| owner(i) != clicked(i)));
    assert!(doubled.len() * 20 < changed.len(), "{}", doubled.len());
}

/// The prepared World with `overlay` drawn in it.
fn shown_with(app: &Application, overlay: &[Quad]) -> FrameOutput {
    let world = &app.prepared.scene;
    let identity = world.quads().iter().map(|q| q.semantic_id.0).max().unwrap();
    let quads = world
        .quads()
        .iter()
        .copied()
        .chain(overlay.iter().zip(identity + 1..).map(|(quad, id)| Quad {
            semantic_id: SemanticId(id),
            part: 0,
            ..*quad
        }));
    let scene = SceneBundle::from_quads(
        world.revision(),
        0,
        world.viewport(),
        world.clear_color(),
        quads,
        SceneLimits::default(),
    )
    .unwrap();
    render(&scene, app.prepared.resources.clone())
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
    assert!(matches!(
        crate::workspace_render::compose_with_overlay(
            &mut app.text,
            &app.ui,
            &app.prepared.scene,
            app.prepared.resources.clone(),
            &overlay,
            SceneLimits::default(),
        ),
        Err(crate::workspace_render::CompositionError::Scene(
            tilewright_scene::Error::LimitExceeded
        ))
    ));
    let (presentation, omitted) = crate::prototype::compose_with_preview(
        &mut app.text,
        &app.ui,
        &app.prepared,
        &overlay,
        SceneLimits::default(),
    )
    .unwrap();
    assert!(omitted);
    assert!(!presentation.scene.quads().iter().any(is_preview));
    // Any other composition failure is not mistaken for a too-large
    // preview: identities colliding with an interface texture fail as is.
    let mut colliding = overlay[0];
    colliding.resource_id = presentation
        .scene
        .quads()
        .iter()
        .map(|quad| quad.resource_id)
        .filter(|id| id.0 != u64::MAX)
        .max()
        .unwrap();
    assert!(
        crate::prototype::compose_with_preview(
            &mut app.text,
            &app.ui,
            &app.prepared,
            &[colliding],
            SceneLimits::default(),
        )
        .is_err()
    );
}

#[test]
fn the_status_line_says_when_the_preview_is_too_large_to_draw() {
    use crate::workspace_ui::Control;
    let (_temporary, mut app) = brush_application();
    app.control(Control::BrushSize(8)).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 3, 3);
    route(&mut app, &button(true, start)).unwrap();
    let preview = previewed(&app);
    assert_eq!(presented_overlay(&mut app).len(), preview.len());
    assert!(!app.preview_omitted);
    assert!(!app.status_label().contains("preview"));
    // A frame budget that holds the World and the interface with the note,
    // but not the preview: the frame is drawn without the preview and the
    // status line says so.
    app.preview_omitted = true;
    app.refresh_ui();
    let noted = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap()
    .scene
    .quads()
    .len();
    app.preview_omitted = false;
    app.refresh_ui();
    let plain = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap()
    .scene
    .quads()
    .len();
    assert!(plain + preview.len() > noted && noted > plain);
    app.frame_limits.maximum_quads = noted;
    assert!(presented_overlay(&mut app).is_empty());
    assert!(app.preview_omitted);
    assert!(
        app.status_label().ends_with(" Too many cells to preview."),
        "{}",
        app.status_label()
    );
    assert!(app.tools.stroke().is_some() && app.blocked.is_none());
    // With room again the preview returns and the note goes.
    app.frame_limits = SceneLimits::default();
    assert_eq!(presented_overlay(&mut app).len(), preview.len());
    assert!(!app.status_label().contains("preview"));
    route(&mut app, &button(false, start)).unwrap();
    assert_eq!(prepared_cells(&app), preview);
}

#[test]
fn no_preview_is_drawn_from_a_frame_the_stroke_did_not_sample() {
    let (_temporary, mut app) = brush_application();
    let start = cell_pixel(&mut app, 0, 1);
    route(&mut app, &button(true, start)).unwrap();
    assert_eq!(presented_overlay(&mut app).len(), 1);
    let generation = app.view.frame_revision;
    // The stroke's frame and the prepared frame must both be the view's.
    app.view.frame_revision = generation + 1;
    assert!(presented_overlay(&mut app).is_empty());
    app.view.frame_revision = generation;
    app.prepared.frame_revision = generation + 1;
    assert!(presented_overlay(&mut app).is_empty());
    app.prepared.frame_revision = generation;
    assert_eq!(presented_overlay(&mut app).len(), 1);
}
