// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Place palette search and thumbnails through real SDL key, text and
//! pointer events, on the same CPU scene path as the scripted demonstration.
use super::{
    BRUSH_WINDOW, acknowledge_ui, cell_pixel, delete_application, input_key, widget_point,
};
use crate::prototype::Application;
use crate::workspace_ui::{Control, Rect, WorkspaceUi};
use sdl3::{event::Event, keyboard::Keycode};
use std::fs;
use tilewright_render_api::{RenderRequest, Renderer};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_scene::{Layer, Quad};

/// One key press routed as the native loop routes it: the text path first,
/// then world shortcuts when no draft owns the keyboard.
fn press(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    key: Keycode,
) -> Result<(), String> {
    match crate::prototype::native_text_event(app, &input_key(7, key, false), input) {
        Some(result) => result,
        None => crate::prototype::native_key(app, key),
    }
}
/// One committed SDL text event in a fresh drain for the current draft.
fn type_text(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    text: &str,
) -> Result<(), String> {
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        app,
        &Event::TextInput {
            timestamp: 1,
            window_id: 7,
            text: text.into(),
        },
        input,
    )
    .unwrap()
}
fn quiet_batch(app: &Application, input: &mut crate::native_input::NativeInput) {
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
}
fn palette_rows(app: &mut Application) -> Vec<(String, bool, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
        .map(|widget| (widget.text.clone(), widget.enabled, widget.active))
        .collect()
}
fn row_text(app: &mut Application, prefix: &str) -> Option<String> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.text.starts_with(prefix))
        .map(|widget| widget.text.clone())
}

#[test]
#[allow(clippy::too_many_lines, clippy::float_cmp)]
fn slash_typing_enter_and_escape_order_through_real_sdl_events() {
    let (temporary, mut app) = delete_application(|_| {});
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut input = crate::native_input::NativeInput::new(7);
    quiet_batch(&app, &mut input);
    // Outside Place mode `/` is no command and focuses nothing.
    press(&mut app, &mut input, Keycode::Slash).unwrap();
    assert!(!app.tools.filter_focused());
    press(&mut app, &mut input, Keycode::P).unwrap();
    assert_eq!(app.tools.placement_choice(), Some("bush"));
    assert_eq!(
        row_text(&mut app, "Filter").as_deref(),
        Some(super::super::FILTER_PLACEHOLDER)
    );

    // `/` focuses the filter; a focused Esc releases it again.
    acknowledge_ui(&mut app);
    press(&mut app, &mut input, Keycode::Slash).unwrap();
    assert!(app.tools.filter_focused());
    acknowledge_ui(&mut app);
    quiet_batch(&app, &mut input);
    press(&mut app, &mut input, Keycode::Escape).unwrap();
    assert!(!app.tools.filter_focused());
    assert!(app.session.history().pending().is_none());

    // Tool and pan keys typed into the focused filter are text.
    acknowledge_ui(&mut app);
    press(&mut app, &mut input, Keycode::Slash).unwrap();
    acknowledge_ui(&mut app);
    let camera = app.view.camera.center;
    quiet_batch(&app, &mut input);
    for key in [
        Keycode::S,
        Keycode::T,
        Keycode::P,
        Keycode::Q,
        Keycode::Delete,
    ] {
        press(&mut app, &mut input, key).unwrap();
    }
    type_text(&mut app, &mut input, "St").unwrap();
    assert_eq!(app.tools.palette_filter(), "St");
    assert_eq!(
        app.tools.mode(),
        tilewright_editor_ui::tools::ToolMode::Place
    );
    assert_eq!(app.view.camera.center, camera);
    // Line and paragraph separators and controls are refused as text.
    for refused in ["\u{2028}", "\u{2029}", "a\tb"] {
        assert!(type_text(&mut app, &mut input, refused).is_err());
    }
    assert_eq!(app.tools.palette_filter(), "St");
    // Only `stone` matches; the hidden choice stays chosen but unmarked.
    assert_eq!(
        palette_rows(&mut app),
        [("stone / item".to_owned(), true, false)]
    );
    assert_eq!(
        row_text(&mut app, "Place palette").as_deref(),
        Some("Place palette: 1 of 5 match")
    );
    assert_eq!(
        app.status_label(),
        "Ready. Click a tile to place bush (hidden by the filter)."
    );

    // Enter keeps the text and releases focus; it reviews nothing.
    acknowledge_ui(&mut app);
    quiet_batch(&app, &mut input);
    press(&mut app, &mut input, Keycode::Return).unwrap();
    assert!(!app.tools.filter_focused());
    assert_eq!(app.tools.palette_filter(), "St");
    assert_eq!(row_text(&mut app, "Filter").as_deref(), Some("Filter: St"));
    assert!(app.session.history().pending().is_none());

    // The hidden choice still places, through the session's grant checks.
    acknowledge_ui(&mut app);
    let tile = cell_pixel(&mut app, 6, 1);
    crate::prototype::native_pointer(&mut app, tile, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let sealed = app.session.history().pending().unwrap().sealed().to_owned();
    assert!(sealed.contains(r#""proto":"bush""#), "{sealed}");

    // While the diff is pending the filter stays editable; rows stay
    // disabled, and an empty result shows one neutral row.
    acknowledge_ui(&mut app);
    let field = widget_point(&mut app, &Control::PaletteFilter);
    crate::prototype::native_pointer(&mut app, field, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.tools.filter_focused());
    type_text(&mut app, &mut input, "o").unwrap();
    assert_eq!(
        palette_rows(&mut app),
        [("stone / item".to_owned(), false, false)]
    );
    type_text(&mut app, &mut input, "zz").unwrap();
    assert!(palette_rows(&mut app).is_empty());
    assert_eq!(
        row_text(&mut app, "No prototype").as_deref(),
        Some(super::super::NO_PALETTE_MATCH)
    );
    // Enter in the focused filter never accepts the pending diff.
    acknowledge_ui(&mut app);
    quiet_batch(&app, &mut input);
    press(&mut app, &mut input, Keycode::Return).unwrap();
    assert_eq!(app.session.history().pending().unwrap().sealed(), sealed);
    assert_eq!(
        (app.tools.palette_filter(), app.tools.filter_focused()),
        ("Stozz", false)
    );

    // Esc clears the filter first; only the next Esc cancels the diff.
    acknowledge_ui(&mut app);
    press(&mut app, &mut input, Keycode::Escape).unwrap();
    assert_eq!(app.tools.palette_filter(), "");
    assert_eq!(app.session.history().pending().unwrap().sealed(), sealed);
    assert_eq!(palette_rows(&mut app).len(), 5);
    acknowledge_ui(&mut app);
    press(&mut app, &mut input, Keycode::Escape).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(app.recovery.records().is_empty());
}

/// The thumbnail quads of the composed presentation: interface quads that
/// sample one of the catalog's ready textures, by full identity (UI glyph
/// textures may reuse a catalog id the frame does not use, never its digest).
fn thumbnail_quads(app: &mut Application) -> Vec<Quad> {
    let ids: Vec<_> = tilewright_editor_ui::tools::object_palette(app.session.catalog())
        .into_iter()
        .filter_map(|proto| {
            app.session
                .resources()
                .prototype_texture(app.session.catalog(), proto)
        })
        .map(|texture| (texture.id, texture.revision, texture.digest))
        .collect();
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| {
            quad.layer == Layer::Interface
                && ids.contains(&(
                    quad.resource_id,
                    quad.resource_revision,
                    quad.resource_digest,
                ))
        })
        .copied()
        .collect()
}

#[test]
#[allow(clippy::cast_precision_loss, clippy::too_many_lines, clippy::float_cmp)]
fn a_thumbnail_click_chooses_its_row_and_never_reaches_the_world_mask() {
    let (_temporary, mut app) = delete_application(|_| {});
    app.control(Control::Place).unwrap();
    acknowledge_ui(&mut app);
    let field = widget_point(&mut app, &Control::PaletteFilter);
    crate::prototype::native_pointer(&mut app, field, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let mut input = crate::native_input::NativeInput::new(7);
    type_text(&mut app, &mut input, "a").unwrap();
    let rows: Vec<_> = palette_rows(&mut app)
        .into_iter()
        .map(|(text, ..)| text)
        .collect();
    assert_eq!(rows, ["crate / item", "lamp / item", "marker / item"]);

    // Thumbnails add nothing to picking: the World masks are the prepared
    // scene's own, before and after the workspace is composed.
    let generation = app.view.frame_revision;
    let objects = app.prepared.mask(generation, false).unwrap();
    let cells = app.prepared.mask(generation, true).unwrap();
    let thumbnails = thumbnail_quads(&mut app);
    assert_eq!(thumbnails.len(), 3);
    assert!(thumbnails.iter().all(|quad| !quad.pickable));
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::PlacePalette("lamp".into())))
        .unwrap()
        .clone();
    let lamp = app
        .session
        .resources()
        .prototype_texture(app.session.catalog(), "lamp")
        .unwrap();
    let target = crate::workspace_render::thumbnail_rect(
        &app.ui,
        widget.layout,
        app.ui.logical_sidebar,
        (lamp.width, lamp.height),
    )
    .unwrap();
    assert!(thumbnails.iter().any(|quad| quad.resource_id == lamp.id
        && quad.positions[0] == [target.x as f32, target.y as f32]));
    let (x, y) = (target.x + target.width / 2, target.y + target.height / 2);
    // The composed frame's semantic plane at the thumbnail is the World
    // mask's value there: the thumbnail wrote nothing into it.
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    let mut renderer =
        ReferenceRenderer::new(offscreen_target(BRUSH_WINDOW.0, BRUSH_WINDOW.1)).unwrap();
    let frame = renderer
        .render(RenderRequest {
            scene: &presentation.scene,
            resources: presentation.resources,
        })
        .unwrap();
    let index = usize::try_from(y * frame.width + x).unwrap();
    let world_frame = app.frame(false).unwrap();
    assert_eq!(frame.semantic_ids[index], world_frame.semantic_ids[index]);
    assert_eq!(objects.pick(x, y).unwrap(), None);

    // A press on the thumbnail is a press on its row: it chooses `lamp`,
    // releases the filter's focus, keeps its text, and selects nothing.
    acknowledge_ui(&mut app);
    let point = [x as f32, y as f32];
    crate::prototype::native_pointer(&mut app, point, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.placement_choice(), Some("lamp"));
    assert_eq!(
        (app.tools.palette_filter(), app.tools.filter_focused()),
        ("a", false)
    );
    assert_eq!(app.session.selection(), None);
    assert!(app.session.history().pending().is_none());
    assert_eq!(
        palette_rows(&mut app)
            .into_iter()
            .filter(|(.., active)| *active)
            .map(|(text, ..)| text)
            .collect::<Vec<_>>(),
        ["lamp / item"]
    );
    // Composing again changed neither World mask.
    let generation = app.view.frame_revision;
    for (before, terrain) in [(objects, false), (cells, true)] {
        let after = app.prepared.mask(generation, terrain).unwrap();
        assert_eq!(before.size(), after.size());
        let [width, height] = after.size();
        for (px, py) in [(x, y), (width / 2, height / 2), (10, height - 10)] {
            assert_eq!(before.pick(px, py).unwrap(), after.pick(px, py).unwrap());
        }
    }
}

/// A small workspace where the palette must scroll: `logical` at `scale`.
fn scrolling_application(logical: (u32, u32), scale: u32) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = super::application();
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.rebuild().unwrap();
    app.control(Control::Place).unwrap();
    (temporary, app)
}

#[test]
fn only_wholly_visible_rows_get_thumbnails_inside_the_sidebar_while_scrolling() {
    for scale in [1, 2] {
        let (_temporary, mut app) = scrolling_application((320, 200), scale);
        let (mut above, mut below) = (false, false);
        for scroll in 0..12 {
            app.sidebar_scroll = scroll;
            let quads = thumbnail_quads(&mut app);
            let rows: Vec<_> = app
                .ui
                .widgets
                .iter()
                .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
                .cloned()
                .collect();
            // One thumbnail per laid-out palette row, never more.
            assert_eq!(quads.len(), rows.len(), "scroll {scroll} scale {scale}");
            assert!(rows.iter().all(|row| row.thumbnail.is_some()));
            above |= !rows.is_empty() && rows.len() < 5 && app.sidebar_scroll > 4;
            below |= !rows.is_empty() && rows.len() < 5 && app.sidebar_scroll <= 4;
            let sidebar = app.ui.sidebar;
            for quad in &quads {
                assert!(!quad.pickable);
                let [left, top] = quad.positions[0];
                let [right, bottom] = quad.positions[2];
                for value in [left, top, right, bottom] {
                    assert!((value - value.round()).abs() < f32::EPSILON);
                }
                #[allow(clippy::cast_precision_loss)]
                let inside = |rect: Rect| {
                    left >= rect.x as f32
                        && top >= rect.y as f32
                        && right <= (rect.x + rect.width) as f32
                        && bottom <= (rect.y + rect.height) as f32
                };
                assert!(inside(sidebar), "{quad:?}");
                assert!(rows.iter().any(|row| inside(row.bounds)), "{quad:?}");
            }
        }
        // Rows were cut off below the sidebar at the top of the list, and
        // above it once scrolled down.
        assert!(above && below, "scale {scale}");
    }
}
