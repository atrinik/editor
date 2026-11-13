// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The zone brush through Session, History and the World undo check: one
//! gesture is one reviewed `terrain.set` on the grid `zone`, guarded per cell,
//! granted only by `editing.zones.paint` and undone byte for byte.
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::brush::{Brush, BrushShape, MAXIMUM_STROKE_CELLS, StrokeTarget};
use tilewright_editor_ui::tools::{ToolMode, ToolState, ZoneChoice, zone_palette};
use tilewright_editor_ui::{Action, SelectionKey, Session, SessionPhase, inspector, zones};

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    Session::new(History::new(document), catalog, resources)
}
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    let session = open(dir.path());
    (dir, session)
}
fn fixture() -> (tempfile::TempDir, Session) {
    fixture_with(|_| {})
}
fn edit_catalog(root: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut catalog);
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
}
/// Include the authored chunk west of the default view, which has no zone
/// grid.
fn widen_view(root: &Path) {
    edit_catalog(root, |catalog| {
        catalog["view"]["bounds"] = serde_json::json!([-4, 0, 12, 6]);
    });
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn cell(x: i64, y: i64) -> SelectionKey {
    SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
    }
}
/// The `zone` cell value of tile `x, y` in the session's current World.
fn zone_at(session: &Session, x: i64, y: i64) -> Option<String> {
    zones::cell_zone(&session.world().planes()["main"], 0, x, y).map(str::to_owned)
}
fn zone_action(session: &Session, cells: Vec<[i64; 2]>, zone: Option<&str>) -> Action {
    Action::PaintZones {
        plane: "main".into(),
        layer: 0,
        cells,
        zone: zone.map(str::to_owned),
        expected: session.history().document().revision(),
    }
}
/// The single `terrain.set` of the pending transaction.
fn prepared_operation(session: &Session) -> serde_json::Value {
    let sealed: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let operations = sealed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1, "{sealed}");
    operations[0].clone()
}
fn prepared_cells(session: &Session) -> Vec<[i64; 2]> {
    prepared_operation(session)["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let at = cell["at"].as_array().unwrap();
            [at[0].as_i64().unwrap(), at[1].as_i64().unwrap()]
        })
        .collect()
}
fn zone_tools(session: &Session, choice: ZoneChoice, radius: u16) -> ToolState {
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Zone);
    tools
        .choose_zone(session.world(), session.catalog(), choice)
        .unwrap();
    tools.set_brush(Brush {
        shape: BrushShape::Square,
        radius,
    });
    tools
}
/// Select `start` and begin a stroke there, as a native press does.
fn press(
    session: &mut Session,
    tools: &mut ToolState,
    records: &mut Records,
    start: [i64; 2],
) -> Result<(), String> {
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: cell(start[0], start[1]),
            },
            records,
        )
        .unwrap();
    let key = session.selection().unwrap().clone();
    let inspected = inspector::inspect(
        session.world(),
        session.catalog(),
        session.selection(),
        session.generation(),
        session.history().document().revision(),
    );
    tools
        .begin_stroke(&inspected, &key, !session.can_prepare())
        .map_err(|error| error.to_string())
}
/// Press at `from`, drag to `to`, and release: the stroke's action.
fn drag(
    session: &mut Session,
    tools: &mut ToolState,
    records: &mut Records,
    from: [i64; 2],
    to: [i64; 2],
) -> Action {
    press(session, tools, records, from).unwrap();
    tools
        .extend_stroke(&cell(to[0], to[1]), session.generation())
        .unwrap();
    tools
        .finish_stroke(session.generation(), !session.can_prepare())
        .unwrap()
        .unwrap()
}

#[test]
fn the_fixture_resolves_painted_child_and_default_zones() {
    let (_dir, session) = fixture();
    let plane = &session.world().planes()["main"];
    let pond = zones::tile_zone(plane, 0, 1, 1).unwrap();
    assert_eq!(pond.id, "meadow.pond");
    assert_eq!(pond.path, ["meadow.pond", "meadow"]);
    assert_eq!(pond.name, Some("Quiet meadow"), "the unnamed pond inherits");
    let path = zones::tile_zone(plane, 0, 4, 5).unwrap();
    assert_eq!((path.id, path.name), ("path", Some("Stone path")));
    let open = zones::tile_zone(plane, 0, 6, 0).unwrap();
    assert!(open.from_default);
    assert_eq!(open.id, "meadow");
    assert_eq!(
        zone_palette(session.world(), session.catalog()),
        ["meadow", "meadow.pond", "path"]
    );
    // Every tile of the 8 x 6 view has a zone, through the default zone.
    let view = zones::view_zones(session.world(), session.catalog());
    assert_eq!(view.len(), 48);
    assert_eq!(view.iter().filter(|(_, id)| *id == "path").count(), 8);
    assert_eq!(
        view.iter().filter(|(_, id)| *id == "meadow.pond").count(),
        4
    );
}

#[test]
fn zone_painting_needs_the_paint_grant() {
    let mut records = Records::default();
    // Without `editing.zones` nothing about zones is granted.
    let (_dir, mut ungranted) = fixture_with(|root| {
        edit_catalog(root, |catalog| {
            catalog["editing"].as_object_mut().unwrap().remove("zones");
        });
    });
    assert!(!ungranted.catalog().editing().zones.paint);
    let refused = ungranted
        .dispatch(
            zone_action(&ungranted, vec![[6, 0]], Some("path")),
            &mut records,
        )
        .unwrap_err()
        .0;
    assert!(refused.contains("editing.zones.paint"), "{refused}");
    // The tool refuses the gesture too, before anything is prepared.
    let mut tools = zone_tools(&ungranted, ZoneChoice::Zone("path".into()), 0);
    let refused = press(&mut ungranted, &mut tools, &mut records, [6, 0]).unwrap_err();
    assert!(refused.contains("editing.zones.paint"), "{refused}");
    assert!(tools.stroke().is_none());
    // `manage` alone grants no painting.
    let (_dir, mut manage_only) = fixture_with(|root| {
        edit_catalog(root, |catalog| {
            catalog["editing"]["zones"] = serde_json::json!({"paint": false, "manage": true});
        });
    });
    assert!(
        manage_only
            .dispatch(
                zone_action(&manage_only, vec![[6, 0]], Some("path")),
                &mut records
            )
            .is_err()
    );
}

#[test]
fn zone_painting_needs_the_view_a_defined_zone_a_change_and_no_pending_change() {
    let mut records = Records::default();
    let (_dir, mut session) = fixture();
    for (cells, zone) in [
        (vec![[8, 0]], Some("path")),
        (vec![[-1, 0]], Some("path")),
        (vec![[0, 6]], None),
        (vec![], Some("path")),
        (vec![[2, 0], [1, 0]], Some("path")),
        (vec![[1, 0], [1, 0]], Some("path")),
        (vec![[1, 0]], Some("unknown")),
        (vec![[1, 0]], Some("Path")),
        (
            (0..=i64::try_from(MAXIMUM_STROKE_CELLS).unwrap())
                .map(|x| [x, 0])
                .collect(),
            Some("path"),
        ),
    ] {
        assert!(
            session
                .dispatch(zone_action(&session, cells.clone(), zone), &mut records)
                .is_err(),
            "{cells:?} {zone:?}"
        );
    }
    let refused = session
        .dispatch(
            zone_action(&session, vec![[1, 0]], Some("unknown")),
            &mut records,
        )
        .unwrap_err()
        .0;
    assert!(refused.contains("unknown zone"), "{refused}");
    // Another plane or layer than the active view is refused.
    let mut elsewhere = zone_action(&session, vec![[1, 0]], Some("path"));
    if let Action::PaintZones { layer, .. } = &mut elsewhere {
        *layer = 1;
    }
    assert!(session.dispatch(elsewhere, &mut records).is_err());
    let mut stale = zone_action(&session, vec![[1, 0]], Some("path"));
    if let Action::PaintZones { expected, .. } = &mut stale {
        *expected = [0; 32];
    }
    assert!(session.dispatch(stale, &mut records).is_err());
    // Every covered cell already has the zone: nothing to review.
    let refused = session
        .dispatch(
            zone_action(&session, vec![[0, 5], [1, 5]], Some("path")),
            &mut records,
        )
        .unwrap_err()
        .0;
    assert!(refused.contains("changes no cell"), "{refused}");
    // Emptying cells that are already empty changes nothing either.
    assert!(
        session
            .dispatch(zone_action(&session, vec![[6, 0]], None), &mut records)
            .is_err()
    );
    assert!(session.history().pending().is_none());

    // A pending zone stroke fences every further preparation.
    session
        .dispatch(
            zone_action(&session, vec![[6, 0]], Some("path")),
            &mut records,
        )
        .unwrap();
    let inverse = session.history().pending().unwrap().inverse().to_owned();
    let refused = session
        .dispatch(
            zone_action(&session, vec![[6, 1]], Some("path")),
            &mut records,
        )
        .unwrap_err()
        .0;
    assert!(refused.contains("Accept or Cancel"), "{refused}");
    assert_eq!(session.history().pending().unwrap().inverse(), inverse);
    // And no stroke begins while it is pending.
    let mut tools = zone_tools(&session, ZoneChoice::Zone("path".into()), 0);
    assert!(press(&mut session, &mut tools, &mut records, [6, 2]).is_err());
    assert!(records.0.is_empty());
}

#[test]
fn one_zone_stroke_is_one_terrain_set_on_the_zone_grid_with_a_guard_per_cell() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut records = Records::default();
    let mut tools = zone_tools(&session, ZoneChoice::Zone("path".into()), 0);
    // Down column 5, from open meadow onto the path row.
    let action = drag(&mut session, &mut tools, &mut records, [5, 0], [5, 5]);
    let Action::PaintZones { cells, zone, .. } = &action else {
        panic!("{action:?}");
    };
    assert_eq!(cells, &(0..6).map(|y| [5, y]).collect::<Vec<_>>());
    assert_eq!(zone.as_deref(), Some("path"));
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let pending = session.history().pending().unwrap();
    assert_eq!(pending.changed().count(), 1);
    let operation = prepared_operation(&session);
    assert_eq!(operation["op"], "terrain.set");
    assert_eq!(operation["grid"], "zone");
    assert_eq!(operation["plane"], "main");
    assert_eq!(operation["layer"], 0);
    // (5, 5) is already on the path and is left out; every other cell
    // expects its empty value.
    assert_eq!(
        prepared_cells(&session),
        (0..5).map(|y| [5, y]).collect::<Vec<_>>()
    );
    for cell in operation["cells"].as_array().unwrap() {
        assert_eq!(cell["value"], "path");
        assert!(cell["expect"].is_null(), "{cell}");
    }
    assert_eq!(
        session.pending_warnings(),
        ["Zones: 5 tiles move to path (Stone path)."]
    );
    for y in 0..6 {
        assert_eq!(zone_at(&session, 5, y).as_deref(), Some("path"));
    }
    // The pick grid is untouched: the floor still reads as before.
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(records.0.is_empty());
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(zone_at(&session, 5, 0), None);
    assert!(session.pending_warnings().is_empty());

    // The default-zone row writes `null`, expecting each cell's zone.
    let mut tools = zone_tools(&session, ZoneChoice::Default, 0);
    let action = drag(&mut session, &mut tools, &mut records, [0, 5], [2, 5]);
    session.dispatch(action, &mut records).unwrap();
    let operation = prepared_operation(&session);
    assert_eq!(operation["grid"], "zone");
    for cell in operation["cells"].as_array().unwrap() {
        assert!(cell["value"].is_null(), "{cell}");
        assert_eq!(cell["expect"], "path");
    }
    assert_eq!(
        session.pending_warnings(),
        ["Zones: 3 tiles return to the default zone, meadow (Quiet meadow)."]
    );
    assert_eq!(zone_at(&session, 1, 5), None);
    let plane = &session.world().planes()["main"];
    assert_eq!(zones::tile_zone(plane, 0, 1, 5).unwrap().id, "meadow");
    session.dispatch(Action::Cancel, &mut records).unwrap();

    // Painting the default zone into an empty cell writes it explicitly:
    // the cell's own value changes, so it is a change to review.
    session
        .dispatch(
            zone_action(&session, vec![[6, 0]], Some("meadow")),
            &mut records,
        )
        .unwrap();
    assert_eq!(prepared_cells(&session), [[6, 0]]);
    assert_eq!(zone_at(&session, 6, 0).as_deref(), Some("meadow"));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(records.0.is_empty());
}

#[test]
fn a_cell_guard_that_no_longer_matches_is_refused_by_world() {
    let (dir, mut session) = fixture();
    let mut records = Records::default();
    session
        .dispatch(
            zone_action(&session, vec![[1, 1], [6, 0]], Some("path")),
            &mut records,
        )
        .unwrap();
    let sealed: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let operation = &sealed["operations"][0];
    assert_eq!(operation["cells"][0]["expect"], "meadow.pond");
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // The same operation with one guard that does not hold: World refuses
    // the whole transaction and nothing is prepared.
    let mut conflicting = serde_json::json!({
        "format": "tilewright.transaction/1",
        "title": "Editor semantic action",
        "actor": "editor:prototype",
        "operations": [operation],
    });
    conflicting["operations"][0]["cells"][0]["expect"] = serde_json::json!("path");
    let mut history = History::new(WorldDocument::open(dir.path()).unwrap());
    let revision = history.document().revision();
    assert!(
        history
            .prepare(revision, "editor:action", &conflicting.to_string())
            .is_err()
    );
    assert!(history.pending().is_none());
    // An empty cell expected as holding a zone conflicts too.
    conflicting["operations"][0]["cells"][0]["expect"] = serde_json::json!("meadow.pond");
    conflicting["operations"][0]["cells"][1]["expect"] = serde_json::json!("meadow");
    assert!(
        history
            .prepare(revision, "editor:action", &conflicting.to_string())
            .is_err()
    );
    // With the guards as Session wrote them it prepares.
    conflicting["operations"][0]["cells"][1]["expect"] = serde_json::Value::Null;
    history
        .prepare(revision, "editor:action", &conflicting.to_string())
        .unwrap();
}

/// Accept, undo and redo `action`; every step publishes one recovery record
/// and each undo restores `files` byte for byte.
fn accept_undo_redo(
    root: &Path,
    session: &mut Session,
    records: &mut Records,
    action: Action,
    files: &[&str],
) -> Vec<Vec<u8>> {
    let read = || -> Vec<Vec<u8>> {
        files
            .iter()
            .map(|file| fs::read(root.join(file)).unwrap_or_default())
            .collect()
    };
    let original = read();
    let before = records.0.len();
    session.dispatch(action, records).unwrap();
    session.dispatch(Action::Accept, records).unwrap();
    assert_eq!(
        records.0.len(),
        before + 1,
        "one stroke retains one inverse"
    );
    let painted = read();
    assert_ne!(painted, original);
    // The result is a valid World: a fresh open validates every file.
    tilewright_source::load(root).unwrap();
    open(root);
    session.dispatch(Action::Undo, records).unwrap();
    session.dispatch(Action::Accept, records).unwrap();
    assert_eq!(read(), original, "undo is byte-exact");
    session.dispatch(Action::Redo, records).unwrap();
    session.dispatch(Action::Accept, records).unwrap();
    assert_eq!(read(), painted, "redo is byte-exact");
    assert_eq!(records.0.len(), before + 3);
    session.dispatch(Action::Undo, records).unwrap();
    session.dispatch(Action::Accept, records).unwrap();
    assert_eq!(read(), original);
    painted
}

#[cfg(target_os = "linux")]
#[test]
fn undo_and_redo_restore_zone_palettes_and_grids_byte_for_byte() {
    let (dir, mut session) = fixture_with(widen_view);
    let root = dir.path().to_owned();
    let east = "planes/main/layers/0/0_0.jsonc";
    let west = "planes/main/layers/0/-1_0.jsonc";
    let mut records = Records::default();
    let text = |file: &str| fs::read_to_string(root.join(file)).unwrap();
    assert!(
        !text(west).contains("\"zone\""),
        "the west chunk has no zone grid"
    );

    // A zone the east chunk's palette lacks gets a new palette entry.
    let action = zone_action(&session, vec![[6, 0], [7, 0]], Some("meadow"));
    let painted = accept_undo_redo(&root, &mut session, &mut records, action, &[east]);
    let painted = String::from_utf8(painted[0].clone()).unwrap();
    assert!(painted.contains("\"meadow\""), "{painted}");
    assert!(!text(east).contains("\"meadow\""));

    // Emptying every pond cell removes the pond's palette entry.
    let action = zone_action(&session, vec![[1, 1], [1, 2], [2, 1], [2, 2]], None);
    let painted = accept_undo_redo(&root, &mut session, &mut records, action, &[east]);
    let painted = String::from_utf8(painted[0].clone()).unwrap();
    assert!(!painted.contains("meadow.pond"), "{painted}");
    assert!(text(east).contains("meadow.pond"));

    // A chunk without a zone grid gains one, in a stroke across both
    // chunks, and loses it again on undo.
    let mut tools = zone_tools(&session, ZoneChoice::Zone("meadow.pond".into()), 0);
    let action = drag(&mut session, &mut tools, &mut records, [-2, 3], [1, 3]);
    let painted = accept_undo_redo(&root, &mut session, &mut records, action, &[east, west]);
    let west_painted = String::from_utf8(painted[1].clone()).unwrap();
    assert!(west_painted.contains("\"zone\""), "{west_painted}");
    assert!(!text(west).contains("\"zone\""));

    // And a grid whose last cell is emptied is removed: paint one west
    // cell, accept, then empty it again.
    session
        .dispatch(
            zone_action(&session, vec![[-1, 0]], Some("path")),
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(text(west).contains("\"zone\""));
    session
        .dispatch(zone_action(&session, vec![[-1, 0]], None), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(!text(west).contains("\"zone\""));
    tilewright_source::load(&root).unwrap();
    session.dispatch(Action::Reopen, &mut records).unwrap();
}

#[test]
fn the_live_preview_is_exactly_the_cells_the_zone_release_prepares() {
    let (_dir, mut session) = fixture_with(widen_view);
    let mut records = Records::default();
    for (choice, radius, from, to) in [
        (ZoneChoice::Zone("path".into()), 0, [0, 1], [7, 1]),
        (ZoneChoice::Zone("meadow.pond".into()), 1, [-3, 0], [3, 4]),
        (ZoneChoice::Zone("meadow".into()), 8, [2, 3], [3, 3]),
        (ZoneChoice::Default, 2, [0, 5], [7, 2]),
    ] {
        let mut tools = zone_tools(&session, choice, radius);
        press(&mut session, &mut tools, &mut records, from).unwrap();
        tools
            .extend_stroke(&cell(to[0], to[1]), session.generation())
            .unwrap();
        let stroke = tools.stroke().unwrap().clone();
        assert!(matches!(stroke.target(), StrokeTarget::Zone(_)));
        let preview = session.stroke_changes(&stroke).unwrap();
        assert!(session.history().pending().is_none());
        let action = tools
            .finish_stroke(session.generation(), false)
            .unwrap()
            .unwrap();
        session.dispatch(action, &mut records).unwrap();
        assert_eq!(preview, prepared_cells(&session));
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    // A stroke that changes nothing previews nothing and is refused alike.
    let mut tools = zone_tools(&session, ZoneChoice::Zone("path".into()), 0);
    press(&mut session, &mut tools, &mut records, [0, 5]).unwrap();
    tools
        .extend_stroke(&cell(7, 5), session.generation())
        .unwrap();
    let stroke = tools.stroke().unwrap().clone();
    let refused = session.stroke_changes(&stroke).unwrap_err().0;
    let action = tools
        .finish_stroke(session.generation(), false)
        .unwrap()
        .unwrap();
    assert_eq!(
        session.dispatch(action, &mut records).unwrap_err().0,
        refused
    );
    assert!(records.0.is_empty());
}

#[test]
fn zone_mode_needs_a_choice_and_the_overlay_toggle_is_per_mode() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut tools = ToolState::default();
    // The overlay is off outside Zone mode until toggled, always on in it.
    assert!(!tools.zones_visible());
    tools.set_mode(ToolMode::Zone);
    assert!(tools.zones_visible());
    assert!(tools.toggle_zones().is_err());
    assert!(tools.zones_visible());
    tools.set_mode(ToolMode::Terrain);
    assert!(!tools.zones_visible());
    assert_eq!(tools.toggle_zones(), Ok(true));
    assert!(tools.zones_visible());
    tools.set_mode(ToolMode::Select);
    assert!(tools.zones_visible(), "the toggle outlasts a mode change");
    assert_eq!(tools.toggle_zones(), Ok(false));
    assert!(!tools.zones_visible());

    // No stroke without a chosen zone; only defined zones can be chosen.
    tools.set_mode(ToolMode::Zone);
    let refused = press(&mut session, &mut tools, &mut records, [1, 1]).unwrap_err();
    assert!(refused.contains("Choose a zone"), "{refused}");
    assert!(
        tools
            .choose_zone(
                session.world(),
                session.catalog(),
                ZoneChoice::Zone("unknown".into())
            )
            .is_err()
    );
    assert_eq!(tools.zone_choice(), None);
    tools
        .choose_zone(session.world(), session.catalog(), ZoneChoice::Default)
        .unwrap();
    press(&mut session, &mut tools, &mut records, [1, 1]).unwrap();
    assert_eq!(tools.stroke().unwrap().target(), &StrokeTarget::Zone(None));
    // Samples come from the ground cell mask, not the zone grid.
    let zone_sample = SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "zone".into(),
        x: 2,
        y: 1,
    };
    assert!(
        tools
            .extend_stroke(&zone_sample, session.generation())
            .is_err()
    );
    tools.cancel_stroke();
    // Terrain mode still makes terrain strokes.
    tools.set_mode(ToolMode::Terrain);
    tools
        .choose_terrain(session.catalog(), "floor", "sand")
        .unwrap();
    press(&mut session, &mut tools, &mut records, [1, 1]).unwrap();
    assert_eq!(
        tools.stroke().unwrap().target(),
        &StrokeTarget::Terrain("sand".into())
    );
}
