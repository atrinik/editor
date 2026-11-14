// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Portal editing through real SDL pointer, key and text events, on the CPU
//! scene path the scripted demonstration captures: the inspector rows of a
//! selected object, the anchor list, the tile pick step and the typed
//! location, each reviewed and accepted as one World change; and every open
//! step or draft ending when it would go stale.
use super::acknowledge_ui;
use super::anchors::{bounds, click_object, click_tile, has};
use super::zone_manage::{all_texts, enter, press, review_has, row_enabled, type_text};
use super::zones::{key, route, sidebar_texts, zone_application};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use std::fs;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::tools::PortalStep;

const CHUNK: &str = "world/planes/main/layers/0/0_0.jsonc";

fn anchor_row(anchor: &str) -> Control {
    Control::PortalAnchor {
        plane: "main".into(),
        anchor: anchor.into(),
    }
}
fn selected(app: &Application) -> Option<String> {
    match app.session.selection() {
        Some(SelectionKey::Object { uid }) => Some(uid.clone()),
        _ => None,
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn retarget_to_an_anchor_from_the_list_through_sdl_events_is_one_reviewed_change() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(CHUNK);
    let original = fs::read(&path).unwrap();
    click_object(&mut app, "gate_east");
    let texts = sidebar_texts(&mut app);
    for line in [
        "Portal: to anchor arrival",
        "at main, layer 0, (7, 3)",
        "Target an anchor",
        "Target a tile in view",
        "Type a location",
        "Clear portal",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    press(&mut app, &Control::PortalAnchors);
    assert_eq!(
        app.tools.portal_step(),
        Some(("gate_east", PortalStep::Anchors))
    );
    let texts = all_texts(&mut app);
    // The hint names the object, shortened to fit the sidebar.
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("Portal of gate_east")),
        "{texts:?}"
    );
    for line in [
        "2 anchors in the world",
        "Plane main:",
        "arrival = layer 0 (7, 3)",
        "lookout = layer 0 (-2, 0)",
    ] {
        assert!(has(&texts, line), "{line}");
    }
    assert!(app.status_label().contains("portal of gate_east"));
    // The anchor it names already is marked and cannot be chosen again.
    assert_eq!(row_enabled(&mut app, &anchor_row("arrival")), Some(false));
    press(&mut app, &anchor_row("lookout"));
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.tools.portal_step(), None, "a choice ends the list");
    assert!(review_has(
        &mut app,
        "gate_east now leads to anchor lookout (main, layer 0, (-2, 0)); it led to anchor arrival (main, layer 0, (7, 3))."
    ));
    // The anchors' counts and the badges follow the pending World.
    assert_eq!(app.anchor_view().count("arrival"), 0);
    assert_eq!(app.anchor_view().count("lookout"), 1);
    assert_eq!(fs::read(&path).unwrap(), original, "nothing written yet");
    enter(&mut app);
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.recovery.records().len(), 1);
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("\"portal\": {\"anchor\": \"lookout\"}"),
        "{text}"
    );
    assert_eq!(selected(&app).as_deref(), Some("gate_east"));
    // In Anchor mode, `arrival` is now named by no portal and deletable.
    route(&mut app, &key(Keycode::K)).unwrap();
    press(&mut app, &Control::AnchorChoose("arrival".into()));
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), Some(true));
    assert_eq!(
        row_enabled(&mut app, &Control::AnchorChoose("lookout".into())),
        Some(true)
    );
    press(&mut app, &Control::AnchorChoose("lookout".into()));
    assert_eq!(row_enabled(&mut app, &Control::AnchorDelete), Some(false));
    // Undo restores the file byte for byte.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    enter(&mut app);
    assert_eq!(fs::read(&path).unwrap(), original, "undo is byte-exact");
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
fn a_tile_click_in_the_pick_step_sets_a_location_and_warns_without_blocking() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(CHUNK);
    click_object(&mut app, "gate_west");
    press(&mut app, &Control::PortalPick);
    assert!(app.tools.portal_picking());
    assert!(
        app.status_label()
            .starts_with("Click a tile in the view: the portal of gate_west"),
        "{}",
        app.status_label()
    );
    assert!(has(
        &sidebar_texts(&mut app),
        "Click a tile in the view (Esc ends)"
    ));
    // The click picks the tile; the object stays selected.
    click_tile(&mut app, 4, 3);
    assert!(app.session.history().pending().is_some());
    assert_eq!(selected(&app).as_deref(), Some("gate_west"));
    assert!(!app.tools.portal_picking());
    assert!(review_has(
        &mut app,
        "gate_west now leads to main, layer 0, (4, 3); it led to main, layer 0, (-2, 2)."
    ));
    assert!(!review_has(&mut app, "Warning:"));
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    // A tile without ground warns, and Accept still publishes it.
    press(&mut app, &Control::PortalPick);
    click_tile(&mut app, 6, 4);
    assert!(review_has(
        &mut app,
        "Warning: gate_west's portal target has no ground on the floor grid"
    ));
    enter(&mut app);
    assert!(app.session.history().pending().is_none());
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("{\"layer\": 0, \"at\": [6, 4]}"), "{text}");
    // Clear it: one remove; the portal badge leaves the gate's tile.
    assert!(app.anchor_view().portal_tiles.contains(&[0, 3]));
    press(&mut app, &Control::PortalClear);
    assert!(review_has(
        &mut app,
        "gate_west no longer has a portal; it led to main, layer 0, (6, 4)."
    ));
    assert!(!app.anchor_view().portal_tiles.contains(&[0, 3]));
    enter(&mut app);
    assert!(!fs::read_to_string(&path).unwrap().contains("[6, 4]"));
    assert!(has(&sidebar_texts(&mut app), "Portal: none"));
    assert_eq!(row_enabled(&mut app, &Control::PortalClear), Some(false));
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
fn a_typed_location_is_checked_in_the_draft_and_reviewed_as_one_change() {
    let (_temporary, mut app) = zone_application(|_| {});
    click_object(&mut app, "gate_west");
    press(&mut app, &Control::PortalLocation);
    assert_eq!(app.tools.portal_draft(), Some("gate_west"));
    assert!(has(
        &sidebar_texts(&mut app),
        "Portal of gate_west: [plane] layer x y"
    ));
    // Each clear error keeps the draft open with its text.
    for (text, error) in [
        ("1 1", "three or four values"),
        ("elsewhere 0 1 1", "no plane with this id"),
        ("main x 1 1", "layer must be a whole number"),
        ("0 1 99999999999999999", "x and y must be whole numbers"),
        ("0 -9223372036854775808 1", "x and y must be whole numbers"),
        ("0 1 9223372036854775807", "x and y must be whole numbers"),
        ("0 9007199254740992 1", "x and y must be whole numbers"),
        ("0 1 -9007199254740992", "x and y must be whole numbers"),
        ("0 -2 2", "already leads there"),
    ] {
        app.tools.select_all().unwrap();
        type_text(&mut app, text);
        acknowledge_ui(&mut app);
        let found = route(&mut app, &key(Keycode::Return)).unwrap_err();
        assert!(found.contains(error), "{text}: {found}");
        assert_eq!(app.tools.draft_text(), Some(text));
        assert!(app.session.history().pending().is_none());
    }
    // `layer x y` is the object's own plane; `plane layer x y` names it.
    app.tools.select_all().unwrap();
    type_text(&mut app, "main 0, 5, 5");
    enter(&mut app);
    assert!(app.session.history().pending().is_some());
    assert!(app.tools.portal_draft().is_none());
    assert!(review_has(
        &mut app,
        "gate_west now leads to main, layer 0, (5, 5)"
    ));
}

#[test]
fn open_steps_and_drafts_end_before_they_can_go_stale() {
    let (_temporary, mut app) = zone_application(|_| {});
    // Choosing another object ends the anchor list.
    click_object(&mut app, "gate_east");
    press(&mut app, &Control::PortalAnchors);
    click_object(&mut app, "marker_1");
    assert_eq!(app.tools.portal_step(), None);
    // A marker's grant has no portal: no edit rows, not even "none".
    let texts = sidebar_texts(&mut app);
    assert!(
        !texts.iter().any(|text| text.starts_with("Portal")),
        "{texts:?}"
    );
    assert_eq!(row_enabled(&mut app, &Control::PortalAnchors), None);
    // A world click outside a pick step ends the list too.
    click_object(&mut app, "gate_east");
    press(&mut app, &Control::PortalAnchors);
    click_tile(&mut app, 2, 2);
    assert_eq!(app.tools.portal_step(), None);
    // Esc ends a pick step without cancelling or preparing anything.
    click_object(&mut app, "gate_east");
    press(&mut app, &Control::PortalPick);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert_eq!(app.tools.portal_step(), None);
    assert_eq!(selected(&app).as_deref(), Some("gate_east"));
    // Pressing the open step's row again closes it; another step or a
    // draft replaces it.
    press(&mut app, &Control::PortalPick);
    press(&mut app, &Control::PortalPick);
    assert_eq!(app.tools.portal_step(), None);
    press(&mut app, &Control::PortalPick);
    press(&mut app, &Control::PortalLocation);
    assert_eq!(app.tools.portal_step(), None);
    assert_eq!(app.tools.portal_draft(), Some("gate_east"));
    press(&mut app, &Control::PortalAnchors);
    assert!(app.tools.portal_draft().is_none());
    // Clear (the selection), a mode switch and Undo end them as well.
    press(&mut app, &Control::Clear);
    assert_eq!(app.tools.portal_step(), None);
    click_object(&mut app, "gate_east");
    press(&mut app, &Control::PortalLocation);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.tools.portal_draft().is_none());
    press(&mut app, &Control::PortalPick);
    app.control(Control::Anchor).unwrap();
    assert_eq!(app.tools.portal_step(), None);
    app.control(Control::Select).unwrap();
    // An accepted change, then Undo while a step is open: the step ends.
    press(&mut app, &Control::PortalClear);
    enter(&mut app);
    press(&mut app, &Control::PortalAnchors);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    assert_eq!(app.tools.portal_step(), None);
    assert!(app.session.history().pending().is_some());
    // While a change is pending the rows are disabled.
    assert_eq!(row_enabled(&mut app, &Control::PortalAnchors), Some(false));
    enter(&mut app);
    // The object ceasing to exist ends a step and the selection with it.
    press(&mut app, &Control::PortalPick);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Delete)).unwrap();
    assert_eq!(app.tools.portal_step(), None);
    enter(&mut app);
    assert!(app.session.world().object("gate_east").is_none());
    assert_eq!(selected(&app), None);
}

#[test]
fn without_the_portal_grant_the_target_shows_and_nothing_edits_it() {
    let (_temporary, mut app) = zone_application(|world| {
        let path = world.join("preview.jsonc");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for entry in catalog["editing"]["objects"].as_array_mut().unwrap() {
            entry.as_object_mut().unwrap().remove("portal");
        }
        fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    });
    click_object(&mut app, "gate_east");
    let texts = sidebar_texts(&mut app);
    assert!(has(&texts, "Portal: to anchor arrival"));
    for control in [
        Control::PortalAnchors,
        Control::PortalPick,
        Control::PortalLocation,
        Control::PortalClear,
    ] {
        assert_eq!(row_enabled(&mut app, &control), None, "{control:?}");
        let error = app.control(control).unwrap_err();
        assert!(error.contains("editing.objects portal"), "{error}");
    }
    assert!(app.session.history().pending().is_none());
}

/// The neutral sample in an isometric projection.
fn isometric(world: &std::path::Path) {
    let profile = world.join("profile/profile.jsonc");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    value["projection"] = serde_json::json!({"kind":"isometric","tile_width":64,"tile_height":32});
    fs::write(&profile, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn isometric_badge_outlines_are_a_full_pixel_wide_across_every_slanted_edge() {
    use crate::workspace_render::BADGE_OUTLINE;
    let (_temporary, mut app) = zone_application(isometric);
    route(&mut app, &key(Keycode::K)).unwrap();
    let badges = crate::prototype::badge_overlay(&app);
    assert_eq!(badges.len(), 2 * 3);
    // The visible diamond of a cell quad: its edges' midpoints.
    let diamond = |positions: [[f32; 2]; 4]| -> [[f32; 2]; 4] {
        std::array::from_fn(|index| {
            let (a, b) = (positions[index], positions[(index + 1) % 4]);
            [f32::midpoint(a[0], b[0]), f32::midpoint(a[1], b[1])]
        })
    };
    for pair in badges.chunks(2) {
        let [outline, badge] = pair else {
            panic!("an outline and a badge");
        };
        let (inner, outer) = (diamond(badge.positions), diamond(outline.positions));
        for index in 0..4 {
            let (a, b) = (inner[index], inner[(index + 1) % 4]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            // Slanted: no edge of the diamond is axis-aligned.
            assert!(dx.abs() > 0.1 && dy.abs() > 0.1, "slanted: {a:?} {b:?}");
            let length = dx.hypot(dy);
            // The outline's matching edge is parallel and at least one
            // pixel further out, measured perpendicular to the edge.
            let (oa, ob) = (outer[index], outer[(index + 1) % 4]);
            let distance = |p: [f32; 2]| ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / length;
            for point in [oa, ob] {
                assert!(
                    distance(point) >= BADGE_OUTLINE - 0.01,
                    "edge {index}: {}",
                    distance(point)
                );
            }
            assert!((distance(oa) - distance(ob)).abs() < 0.01, "parallel");
        }
        // The axis-only growth this replaces was thinner than a pixel
        // across these edges.
        let [min, max] = bounds(badge.positions);
        let (a, b) = ((max[0] - min[0]) / 2.0, (max[1] - min[1]) / 2.0);
        let w = BADGE_OUTLINE;
        let axis_only = (a + w) * (b + w) / (a + w).hypot(b + w) - a * b / a.hypot(b);
        assert!(axis_only < 0.95 * w, "{axis_only}");
    }
}

#[test]
fn a_frame_past_the_scene_limits_names_the_overlay_it_leaves_out() {
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::K)).unwrap();
    // In Anchor mode only the badges are drawn.
    app.preview_omitted = true;
    assert_eq!(
        app.status_label(),
        "Anchor and portal badges left out: too many for one frame."
    );
    // With the zone overlay shown, both are.
    app.preview_omitted = false;
    route(&mut app, &key(Keycode::O)).unwrap();
    app.preview_omitted = true;
    assert_eq!(
        app.status_label(),
        "Zone overlay and badges left out: too many cells for one frame."
    );
}

/// The neutral sample as bent isometric ground: corner heights differ, so
/// each cell is compiled as a mesh of sub-quads.
fn bent_isometric(world: &std::path::Path) {
    isometric(world);
    let chunk = world.join("planes/main/layers/0/0_0.jsonc");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&chunk).unwrap()).unwrap();
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y| (0..32).map(|x| (x * 7 + y * 13) % 5 * 6).collect())
        .collect();
    value["terrain"]["height"] = serde_json::json!({ "rows": rows });
    fs::write(&chunk, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
#[allow(clippy::float_cmp)]
fn badge_outlines_stay_one_to_about_two_pixels_wide_on_every_kind_of_cell() {
    use crate::workspace_render::{ANCHOR_BADGE, BADGE_OUTLINE, PORTAL_BADGE};
    for (name, prepare) in [
        (
            "top-down",
            (|_: &std::path::Path| {}) as fn(&std::path::Path),
        ),
        ("flat isometric", isometric),
        ("bent isometric", bent_isometric),
    ] {
        let (_temporary, mut app) = zone_application(prepare);
        route(&mut app, &key(Keycode::K)).unwrap();
        let badges = crate::prototype::badge_overlay(&app);
        if name == "bent isometric" {
            assert!(
                badges.len() > 2 * 3,
                "{name}: cells are meshes: {}",
                badges.len()
            );
        }
        // Group each badge's quads by colour run: the outline copies, then
        // the badge copies, in the same count.
        let mut index = 0;
        let mut badges_seen = 0;
        while index < badges.len() {
            let outline_color = badges[index].color;
            let mut end = index;
            while end < badges.len() && badges[end].color == outline_color {
                end += 1;
            }
            let parts = end - index;
            let (outline, badge) = (&badges[index..end], &badges[end..end + parts]);
            assert!(badge.iter().all(|quad| quad.color != outline_color));
            assert!(
                badge
                    .iter()
                    .all(|quad| [ANCHOR_BADGE, PORTAL_BADGE].contains(&quad.color))
            );
            let union = |quads: &[tilewright_scene::Quad]| {
                let mut all = quads.iter().map(|quad| bounds(quad.positions));
                let first = all.next().unwrap();
                all.fold(first, |[min, max], [lo, hi]| {
                    [
                        [min[0].min(lo[0]), min[1].min(lo[1])],
                        [max[0].max(hi[0]), max[1].max(hi[1])],
                    ]
                })
            };
            let ([inner_min, inner_max], [outer_min, outer_max]) = (union(badge), union(outline));
            for axis in 0..2 {
                for extra in [
                    inner_min[axis] - outer_min[axis],
                    outer_max[axis] - inner_max[axis],
                ] {
                    // At least the outline width; along the axes a 2:1
                    // diamond's outline reaches about 2.2 times it across
                    // (a bent cell's bounds a little more), never the 9 to
                    // 14 px that scaling by its sub-quads gave.
                    assert!(
                        (BADGE_OUTLINE - 0.01..=3.0 * BADGE_OUTLINE).contains(&extra),
                        "{name}: axis {axis}: {extra}"
                    );
                }
            }
            badges_seen += 1;
            index = end + parts;
        }
        assert_eq!(badges_seen, 3, "{name}");
    }
}
