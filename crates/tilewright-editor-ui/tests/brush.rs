// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Drag brush geometry, gesture accumulation and its single reviewed
//! `terrain.set` through Session, History and the World undo check.
use std::{collections::BTreeSet, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::brush::{
    Brush, BrushShape, MAXIMUM_RADIUS, MAXIMUM_SEGMENT, MAXIMUM_STROKE_CELLS, segment,
};
use tilewright_editor_ui::inspector;
use tilewright_editor_ui::tools::{ToolMode, ToolState};
use tilewright_editor_ui::{Action, SelectionKey, Session, SessionPhase};

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
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    (
        dir,
        Session::new(History::new(document), catalog, resources),
    )
}
fn fixture() -> (tempfile::TempDir, Session) {
    fixture_with(|_| {})
}
/// Include the authored chunk west of the default view.
fn widen_view(root: &Path) {
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["view"]["bounds"] = serde_json::json!([-4, 0, 12, 6]);
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
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
fn value(session: &Session, x: i64, y: i64) -> Option<String> {
    let chunk =
        session.world().planes()["main"]
            .chunks
            .get(&(0, x.div_euclid(32), y.div_euclid(32)))?;
    let local = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32)).unwrap();
    chunk.ground[0].value(local).map(str::to_owned)
}
fn stroke_action(session: &Session, cells: Vec<[i64; 2]>, proto: &str) -> Action {
    Action::PaintStroke {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        cells,
        proto: proto.into(),
        expected: session.history().document().revision(),
    }
}
/// Select `start` and begin a stroke there, as a native press does.
fn press(session: &mut Session, tools: &mut ToolState, records: &mut Records, start: [i64; 2]) {
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
        .unwrap();
}
fn sand_tools(brush: Brush) -> ToolState {
    let (_dir, session) = fixture();
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Terrain);
    tools
        .choose_terrain(session.catalog(), "floor", "sand")
        .unwrap();
    tools.set_brush(brush);
    tools
}
fn cells_of(tools: &ToolState) -> Vec<[i64; 2]> {
    tools.stroke().unwrap().cells().collect()
}

#[test]
fn footprints_keep_the_archived_square_and_round_disc_shapes() {
    let square = Brush {
        shape: BrushShape::Square,
        radius: 1,
    };
    assert_eq!(square.footprint([5, 5]).count(), 9);
    let disc: BTreeSet<[i64; 2]> = Brush {
        shape: BrushShape::Circle,
        radius: 2,
    }
    .footprint([0, 0])
    .collect();
    // A 5 x 5 square without its corners.
    assert_eq!(disc.len(), 21);
    assert!(!disc.contains(&[2, 2]) && disc.contains(&[2, 1]) && disc.contains(&[0, 2]));
    let dot = Brush::default();
    assert_eq!(dot.footprint([3, -4]).collect::<Vec<_>>(), [[3, -4]]);
    assert_eq!(square.footprint([i64::MAX, 0]).count(), 6);
    let huge = Brush {
        shape: BrushShape::Square,
        radius: u16::MAX,
    };
    let side = usize::from(MAXIMUM_RADIUS) * 2 + 1;
    assert_eq!(huge.footprint([0, 0]).count(), side * side);
    assert_eq!(huge.diameter(), u32::try_from(side).unwrap());
    assert_eq!(dot.resized(-1), dot);
    assert_eq!(huge.resized(1).radius, MAXIMUM_RADIUS);
    assert_eq!(dot.resized(2).radius, 2);
    assert_eq!(dot.toggled_shape().shape, BrushShape::Circle);
    assert_eq!(dot.toggled_shape().toggled_shape(), dot);
}

#[test]
fn segments_connect_pointer_samples_without_gaps() {
    assert_eq!(segment([0, 0], [0, 0]), [[0, 0]]);
    assert_eq!(segment([0, 0], [3, 1]), [[0, 0], [1, 0], [2, 1], [3, 1]]);
    assert_eq!(segment([2, 2], [2, -1]), [[2, 2], [2, 1], [2, 0], [2, -1]]);
    // Every step of a steep or shallow line moves at most one cell per axis.
    for (from, to) in [([0, 0], [7, 3]), ([5, 9], [-4, 0]), ([1, 1], [2, 40])] {
        let line = segment(from, to);
        assert_eq!((line[0], *line.last().unwrap()), (from, to));
        assert!(
            line.windows(2)
                .all(|w| (w[0][0] - w[1][0]).abs() <= 1 && (w[0][1] - w[1][1]).abs() <= 1)
        );
    }
    let far = i64::try_from(MAXIMUM_SEGMENT).unwrap() + 1;
    assert_eq!(segment([0, 0], [far, 0]), [[far, 0]]);
    assert_eq!(segment([i64::MIN, 0], [i64::MAX, 0]), [[i64::MAX, 0]]);
}

#[test]
fn a_fast_drag_interpolates_between_samples_and_clips_to_the_view() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut tools = sand_tools(Brush::default());
    press(&mut session, &mut tools, &mut records, [0, 1]);
    let generation = session.generation();
    // One sample far across the view still covers every cell in between.
    assert_eq!(tools.extend_stroke(&cell(7, 1), generation).unwrap(), 7);
    assert_eq!(cells_of(&tools), (0..8).map(|x| [x, 1]).collect::<Vec<_>>());
    // The same cell again adds nothing.
    assert_eq!(tools.extend_stroke(&cell(7, 1), generation).unwrap(), 0);
    // A diagonal joins without gaps.
    tools.extend_stroke(&cell(4, 4), generation).unwrap();
    for covered in [[6, 2], [5, 3], [4, 4]] {
        assert!(cells_of(&tools).contains(&covered), "{covered:?}");
    }
    assert!(!tools.stroke().unwrap().capped());

    // A wider brush at the view corner keeps only cells inside the bounds.
    let mut tools = sand_tools(Brush {
        shape: BrushShape::Square,
        radius: 1,
    });
    press(&mut session, &mut tools, &mut records, [0, 0]);
    assert_eq!(cells_of(&tools), [[0, 0], [0, 1], [1, 0], [1, 1]]);
    tools
        .extend_stroke(&cell(7, 5), session.generation())
        .unwrap();
    assert!(
        cells_of(&tools)
            .iter()
            .all(|&[x, y]| (0..8).contains(&x) && (0..6).contains(&y))
    );
    assert!(cells_of(&tools).contains(&[7, 5]) && cells_of(&tools).contains(&[6, 4]));
    let mut circle = sand_tools(Brush {
        shape: BrushShape::Circle,
        radius: 2,
    });
    press(&mut session, &mut circle, &mut records, [7, 5]);
    // The disc's lower-right quarter lies outside the view.
    assert_eq!(cells_of(&circle).len(), 8);
    assert!(session.history().pending().is_none());
}

#[test]
fn a_stroke_samples_only_its_own_frame_and_grid() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut tools = sand_tools(Brush::default());
    press(&mut session, &mut tools, &mut records, [1, 1]);
    let generation = session.generation();
    let other_grid = SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "other".into(),
        x: 2,
        y: 1,
    };
    assert!(tools.extend_stroke(&other_grid, generation).is_err());
    assert!(
        tools
            .extend_stroke(
                &SelectionKey::Object {
                    uid: "marker_1".into()
                },
                generation
            )
            .is_err()
    );
    assert_eq!(cells_of(&tools), [[1, 1]]);
    // A different frame ends the stroke: its mask no longer matches.
    assert!(tools.extend_stroke(&cell(2, 1), generation + 1).is_err());
    assert!(tools.stroke().is_none());
    assert!(tools.finish_stroke(generation, false).unwrap().is_none());

    press(&mut session, &mut tools, &mut records, [1, 1]);
    assert!(tools.finish_stroke(generation + 1, false).is_err());
    assert!(tools.stroke().is_none());
    press(&mut session, &mut tools, &mut records, [1, 1]);
    assert!(tools.finish_stroke(session.generation(), true).is_err());
    assert!(tools.stroke().is_none());
    press(&mut session, &mut tools, &mut records, [1, 1]);
    tools.set_mode(ToolMode::Terrain);
    assert!(tools.stroke().is_none(), "mode change discards the stroke");
    press(&mut session, &mut tools, &mut records, [1, 1]);
    assert!(tools.cancel_stroke());
    assert!(!tools.cancel_stroke());
    assert!(session.history().pending().is_none());
}

#[test]
fn a_stroke_needs_terrain_mode_an_explicit_choice_and_no_pending_change() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: cell(1, 1),
            },
            &mut records,
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
    let mut tools = ToolState::default();
    assert!(tools.begin_stroke(&inspected, &key, false).is_err());
    tools.set_mode(ToolMode::Terrain);
    assert!(
        tools
            .begin_stroke(&inspected, &key, false)
            .unwrap_err()
            .0
            .contains("Choose")
    );
    tools
        .choose_terrain(session.catalog(), "floor", "sand")
        .unwrap();
    assert!(tools.begin_stroke(&inspected, &key, true).is_err());
    assert!(tools.begin_stroke(&inspected, &cell(2, 1), false).is_err());
    assert!(tools.stroke().is_none());
    tools.begin_stroke(&inspected, &key, false).unwrap();
    assert_eq!(cells_of(&tools), [[1, 1]]);
}

#[test]
fn one_stroke_is_one_pending_diff_with_one_undo_check_and_nothing_written() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut records = Records::default();
    let mut tools = sand_tools(Brush::default());
    press(&mut session, &mut tools, &mut records, [0, 1]);
    tools
        .extend_stroke(&cell(5, 1), session.generation())
        .unwrap();
    let action = tools
        .finish_stroke(session.generation(), !session.can_prepare())
        .unwrap()
        .unwrap();
    let Action::PaintStroke { cells, .. } = &action else {
        panic!("{action:?}");
    };
    assert_eq!(cells, &(0..6).map(|x| [x, 1]).collect::<Vec<_>>());
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let pending = session.history().pending().unwrap();
    // One World transaction with one operation for every covered cell, one
    // changed file and one planned inverse, which History has already proven
    // with the toolkit undo check before showing the diff.
    assert_eq!(pending.changed().count(), 1);
    assert_eq!(pending.sealed().matches(r#""op":"terrain.set""#).count(), 1);
    // Cells 4 and 5 already hold sand, so only the four grass cells change.
    assert_eq!(pending.sealed().matches(r#""at":"#).count(), 4);
    assert!(pending.inverse().contains("tilewright.transaction/1"));
    for x in 0..6 {
        assert_eq!(value(&session, x, 1).as_deref(), Some("sand"));
    }
    assert_eq!(value(&session, 0, 2).as_deref(), Some("grass"));
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(records.0.is_empty());
    // The pending stroke fences every further preparation, including strokes.
    let pending_inverse = pending.inverse().to_owned();
    assert!(
        session
            .dispatch(stroke_action(&session, vec![[0, 3]], "sand"), &mut records)
            .unwrap_err()
            .0
            .contains("Accept or Cancel")
    );
    assert_eq!(
        session.history().pending().unwrap().inverse(),
        pending_inverse
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(records.0.is_empty());
    assert_eq!(value(&session, 1, 1).as_deref(), Some("grass"));
}

#[test]
fn stroke_actions_refuse_invalid_cells_and_omit_unchanged_ones() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    for cells in [
        vec![],
        vec![[1, 1], [0, 1]],
        vec![[1, 1], [1, 1]],
        vec![[1, 1], [8, 1]],
        vec![[-1, 0]],
        (0..=i64::try_from(MAXIMUM_STROKE_CELLS).unwrap())
            .map(|x| [x, 0])
            .collect(),
    ] {
        assert!(
            session
                .dispatch(stroke_action(&session, cells, "sand"), &mut records)
                .is_err()
        );
    }
    assert!(
        session
            .dispatch(stroke_action(&session, vec![[1, 1]], "water"), &mut records)
            .is_err()
    );
    let mut stale = stroke_action(&session, vec![[1, 1]], "sand");
    if let Action::PaintStroke { expected, .. } = &mut stale {
        *expected = [0; 32];
    }
    assert!(session.dispatch(stale, &mut records).is_err());
    // Every covered cell already holds sand: nothing to review.
    assert!(
        session
            .dispatch(
                stroke_action(&session, vec![[4, 0], [5, 0]], "sand"),
                &mut records
            )
            .unwrap_err()
            .0
            .contains("changes no cell")
    );
    assert!(session.history().pending().is_none());
    // Mixed: grass at x=3 changes; sand at x=4 is left out of the operation.
    session
        .dispatch(
            stroke_action(&session, vec![[3, 0], [4, 0], [6, 4]], "sand"),
            &mut records,
        )
        .unwrap();
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    assert!(
        sealed.contains(r#"{"at":[3,0],"expect":"grass","value":"sand"}"#),
        "{sealed}"
    );
    assert!(
        sealed.contains(r#"{"at":[6,4],"expect":null,"value":"sand"}"#),
        "{sealed}"
    );
    assert!(!sealed.contains("[4,0]"), "{sealed}");
    assert_eq!(value(&session, 6, 4).as_deref(), Some("sand"));
}

#[cfg(target_os = "linux")]
#[test]
fn accepting_a_cross_chunk_stroke_retains_one_inverse_and_undoes_in_one_step() {
    let (dir, mut session) = fixture_with(widen_view);
    let east = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let west = dir.path().join("planes/main/layers/0/-1_0.jsonc");
    let original = (fs::read(&east).unwrap(), fs::read(&west).unwrap());
    let mut records = Records::default();
    let mut tools = sand_tools(Brush {
        shape: BrushShape::Square,
        radius: 0,
    });
    press(&mut session, &mut tools, &mut records, [-3, 2]);
    tools
        .extend_stroke(&cell(2, 2), session.generation())
        .unwrap();
    let action = tools
        .finish_stroke(session.generation(), false)
        .unwrap()
        .unwrap();
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 2);
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        original
    );
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1, "one stroke retains one exact inverse");
    assert_eq!(session.history().depths(), (1, 0));
    let painted = (fs::read(&east).unwrap(), fs::read(&west).unwrap());
    assert_ne!(painted.0, original.0);
    assert_ne!(painted.1, original.1);
    for x in -3..=2 {
        assert_eq!(value(&session, x, 2).as_deref(), Some("sand"), "{x}");
    }
    session.dispatch(Action::Undo, &mut records).unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 2);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        original
    );
    assert_eq!(session.history().depths(), (0, 1));
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        painted
    );
    assert_eq!(records.0.len(), 3);
    session.dispatch(Action::Reopen, &mut records).unwrap();
    assert_eq!(value(&session, -1, 2).as_deref(), Some("sand"));
}
