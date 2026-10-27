// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sessions save each interaction to a copy of the fixture world when it
//! ends, coalesce interactions within their window into one undo step, and
//! undo and redo through the transactions' inverses. They run where the
//! toolkit publishes; where it cannot check everything, the copies accept
//! that.

#![cfg(any(target_os = "linux", target_os = "macos", windows))]

use std::{
    path::Path,
    time::{Duration, Instant},
};
use tilewright_edit::{Options as PlanOptions, PublishOptions, Transaction};
use tilewright_editor_commands::{
    MAXIMUM_TRANSACTION_CELLS, MAXIMUM_TRANSACTION_CHUNKS, MAXIMUM_TRANSACTION_ROWS,
    brush::{Brush, MAXIMUM_STROKE_TILES, Shape},
    json_string,
    session::{Command, Error, MAXIMUM_COALESCE, Options, Session},
    transaction,
};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_testkit::MEADOW_FILES;

/// Copies are made in a temporary directory no one else changes, so the
/// tests accept what the toolkit cannot check on some platforms.
const ACCEPTED: PublishOptions = PublishOptions {
    unchecked_access: true,
};

const DOT: Brush = Brush {
    shape: Shape::Square,
    radius: 0,
};

/// A fresh copy of the meadow world, made by publishing a transaction that
/// creates its files.
fn meadow_copy() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let operations: Vec<String> = MEADOW_FILES
        .iter()
        .map(|(path, text)| {
            format!(
                "{{\"op\": \"document.create\", \"path\": {}, \"text\": {}}}",
                json_string(path),
                json_string(text)
            )
        })
        .collect();
    let text = transaction("Copy the meadow", &operations, &[]);
    let created = Transaction::parse("copy.jsonc", text).unwrap();
    tilewright_edit::plan(directory.path(), &created, &PlanOptions::default())
        .unwrap()
        .publish_with(&ACCEPTED)
        .unwrap();
    directory
}

fn accepted() -> Options {
    Options {
        unchecked_access: true,
        ..Options::default()
    }
}

/// An outside program sets ground tile `at` to `value`.
fn outside_edit(root: &Path, at: [i64; 2], value: &str) {
    let operation = format!(
        "{{\"op\": \"terrain.set\", \"plane\": \"meadow\", \"layer\": 0, \"grid\": \"ground\", \"cells\": [{{\"at\": [{}, {}], \"value\": \"{value}\"}}]}}",
        at[0], at[1]
    );
    let text = transaction("Outside edit", &[operation], &[]);
    let outside = Transaction::parse("outside.jsonc", text).unwrap();
    tilewright_edit::plan(root, &outside, &PlanOptions::default())
        .unwrap()
        .publish_with(&ACCEPTED)
        .unwrap();
}

fn stroke(session: &mut Session, now: Instant, terrain: &str, points: &[[i64; 2]]) -> String {
    paint(session, now, terrain, points);
    session.end_stroke(now).unwrap().unwrap().title
}

fn revision(root: &Path) -> [u8; 32] {
    WorldDocument::open(root).unwrap().revision()
}

/// The ground value at `at` of layer 0 of the meadow, as on disk.
fn on_disk(root: &Path, at: [i64; 2]) -> Option<String> {
    let document = WorldDocument::open(root).unwrap();
    let mut session = Session::new(document, Options::default());
    session
        .terrain("meadow")
        .unwrap()
        .value(0, at, 0)
        .map(str::to_owned)
}

fn paint(session: &mut Session, now: Instant, terrain: &str, points: &[[i64; 2]]) {
    session
        .begin_stroke(now, "meadow", 0, terrain, DOT)
        .unwrap();
    for point in points {
        session.stamp(*point).unwrap();
    }
}

#[test]
fn each_release_saves_and_undo_restores_every_byte() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[10, 10], [12, 10]]);
    assert!(session.is_dirty());
    // Nothing is on disk until the release.
    assert_eq!(on_disk(root.path(), [11, 10]).as_deref(), Some("water"));
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert_eq!(saved.title, "Paint sand on 3 tiles");
    assert_eq!(saved.files, ["planes/meadow/layers/0/0_0.jsonc"]);
    // Windows cannot sync a directory, so there every save warns that its
    // names may not be durable yet.
    assert!(
        saved
            .warning
            .as_deref()
            .is_none_or(|warning| warning.contains("durable")),
        "{saved:?}"
    );
    assert_eq!(saved.warning.is_some(), cfg!(windows));
    assert!(!session.is_dirty());
    assert_eq!(on_disk(root.path(), [11, 10]).as_deref(), Some("sand"));
    let first = revision(root.path());
    assert_eq!(session.document().revision(), first);
    // Sand from the beach into the chunk east of it, which is created.
    paint(&mut session, now, "sand", &[[30, 3], [33, 3]]);
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert_eq!(saved.title, "Paint sand on 2 tiles");
    assert_eq!(saved.files, ["planes/meadow/layers/0/1_0.jsonc"]);
    let second = revision(root.path());

    // A window of zero keeps the two apart; undo goes back a step at a
    // time, byte for byte.
    assert_eq!(
        session.undo().unwrap().unwrap().title,
        "Paint sand on 2 tiles"
    );
    assert_eq!(revision(root.path()), first);
    assert_eq!(on_disk(root.path(), [33, 3]), None);
    assert_eq!(
        session.undo().unwrap().unwrap().title,
        "Paint sand on 3 tiles"
    );
    assert_eq!(revision(root.path()), opened, "undo restores every byte");
    assert_eq!(session.document().revision(), opened);
    assert_eq!(
        session.terrain("meadow").unwrap().value(0, [11, 10], 0),
        Some("water")
    );
    assert_eq!(session.undo().unwrap(), None);
    assert_eq!(
        session.history(),
        (
            vec![],
            vec![
                "Paint sand on 3 tiles".to_owned(),
                "Paint sand on 2 tiles".to_owned()
            ]
        )
    );
    session.redo().unwrap().unwrap();
    session.redo().unwrap().unwrap();
    assert_eq!(revision(root.path()), second);
    assert_eq!(session.redo().unwrap(), None);
    assert_eq!(session.history().0.len(), 2);
}

#[test]
fn interactions_within_the_window_are_one_undo_step() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let start = Instant::now();
    stroke(&mut session, start, "sand", &[[1, 1]]);
    // Each release saves at once.
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    let second = start + Duration::from_millis(1_500);
    let saved = session.begin_stroke(second, "meadow", 0, "water", DOT);
    assert!(saved.is_ok());
    session.stamp([2, 2]).unwrap();
    session.stamp([3, 2]).unwrap();
    let saved = session.end_stroke(second).unwrap().unwrap();
    assert_eq!(saved.title, "Paint sand on 1 tile and 1 more edit");
    assert_eq!(on_disk(root.path(), [3, 2]).as_deref(), Some("water"));
    // The window runs from the last release: a third joins too.
    let third = second + Duration::from_secs(2);
    stroke(&mut session, third, "sand", &[[5, 5]]);
    assert_eq!(
        session.history().0,
        ["Paint sand on 1 tile and 2 more edits"]
    );
    // One undo reverts all three, byte for byte; one redo redoes them.
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
    session.redo().unwrap().unwrap();
    assert_eq!(on_disk(root.path(), [5, 5]).as_deref(), Some("sand"));
    assert_eq!(session.history().0.len(), 1);
    // After an undo or redo the next interaction starts a new step.
    stroke(&mut session, third, "water", &[[6, 6]]);
    assert_eq!(session.history().0.len(), 2);
}

#[test]
fn an_interaction_after_the_window_is_a_new_step() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let start = Instant::now();
    stroke(&mut session, start, "sand", &[[1, 1]]);
    stroke(
        &mut session,
        start + Duration::from_millis(2_001),
        "water",
        &[[5, 5]],
    );
    assert_eq!(
        session.history().0,
        ["Paint sand on 1 tile", "Paint water on 1 tile"]
    );
}

#[test]
fn the_window_is_at_most_a_minute() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::MAX,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let start = Instant::now();
    stroke(&mut session, start, "sand", &[[1, 1]]);
    let joined = start + MAXIMUM_COALESCE;
    stroke(&mut session, joined, "sand", &[[2, 1]]);
    stroke(
        &mut session,
        joined + MAXIMUM_COALESCE + Duration::from_secs(1),
        "sand",
        &[[3, 1]],
    );
    assert_eq!(session.history().0.len(), 2);
}

#[test]
fn history_keeps_its_newest_steps() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        history: 2,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    for x in 1..=3 {
        stroke(&mut session, now, "sand", &[[x, 1]]);
    }
    assert_eq!(session.history().0.len(), 2);
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.undo().unwrap(), None);
    // The oldest step was forgotten, so its tile stays.
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    assert_eq!(on_disk(root.path(), [2, 1]).as_deref(), Some("grass"));
}

#[test]
fn a_new_edit_clears_redo() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    stroke(&mut session, now, "sand", &[[1, 1]]);
    session.undo().unwrap();
    assert_eq!(session.history().1.len(), 1);
    stroke(&mut session, now, "water", &[[2, 2]]);
    assert_eq!(
        session.history(),
        (vec!["Paint water on 1 tile".to_owned()], vec![])
    );
    assert_eq!(session.redo().unwrap(), None);
}

#[test]
fn cancelling_a_stroke_puts_its_tiles_back_and_saves_nothing() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1], [40, 1]]);
    let dirty = session.cancel_stroke().unwrap();
    assert!(dirty.contains(&(0, 0, 0)) && dirty.contains(&(0, 1, 0)));
    let terrain = session.terrain("meadow").unwrap();
    assert_eq!(terrain.value(0, [1, 1], 0), Some("grass"));
    assert_eq!(terrain.value(0, [40, 1], 0), None);
    assert!(!session.is_dirty() && !session.is_interacting());
    assert_eq!(revision(root.path()), opened);
    stroke(&mut session, now, "sand", &[[2, 2]]);
}

#[test]
fn a_rejected_save_drops_only_its_own_edits_and_reloads_the_world() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    // Another program changes the tile after the session read it.
    outside_edit(root.path(), [1, 1], "water");
    paint(&mut session, now, "sand", &[[1, 1]]);
    let Err(Error::Rejected(reason)) = session.end_stroke(now) else {
        panic!("a save over an outside change was accepted");
    };
    assert!(reason.to_string().contains("tx.conflict"), "{reason}");
    let rejected = session.rejected().unwrap();
    assert_eq!(rejected.title, "Paint sand on 1 tile");
    assert!(rejected.transaction.contains("\"expect\": \"grass\""));
    assert!(!session.is_dirty() && !session.is_waiting());
    // The world was read again: the preview shows the outside change.
    assert_eq!(
        session.terrain("meadow").unwrap().value(0, [1, 1], 0),
        Some("water")
    );
    // Later edits save on their own, even on that tile.
    stroke(&mut session, now, "sand", &[[20, 20]]);
    stroke(&mut session, now, "sand", &[[1, 1]]);
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    assert_eq!(session.rejected(), None);
}

#[test]
fn undo_and_redo_over_an_outside_change_conflict_and_keep_their_step() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    stroke(&mut session, now, "sand", &[[1, 1]]);
    outside_edit(root.path(), [2, 2], "water");
    let Err(Error::Rejected(rejected)) = session.undo() else {
        panic!("undo over an outside change was accepted");
    };
    assert!(rejected.to_string().contains("tx.conflict"), "{rejected}");
    assert_eq!(session.history().0, ["Paint sand on 1 tile"]);
    assert_eq!(on_disk(root.path(), [2, 2]).as_deref(), Some("water"));
    // Undone on a clean copy, a redo over an outside change conflicts too.
    let root = meadow_copy();
    let mut session = Session::open(root.path(), options).unwrap();
    stroke(&mut session, now, "sand", &[[1, 1]]);
    session.undo().unwrap();
    outside_edit(root.path(), [2, 2], "water");
    assert!(matches!(session.redo(), Err(Error::Rejected(_))));
    assert_eq!(session.history().1, ["Paint sand on 1 tile"]);
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("grass"));
}

#[test]
fn a_maximum_stroke_saves_and_one_past_it_saves_what_it_painted() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    let wide = Brush {
        shape: Shape::Square,
        radius: 32,
    };
    // About 95,000 tiles, in some 190 chunks, most of them new.
    session
        .begin_stroke(now, "meadow", 0, "sand", wide)
        .unwrap();
    session.stamp([0, 0]).unwrap();
    session.stamp([1_400, 0]).unwrap();
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert!(
        saved.title.starts_with("Paint sand on 9"),
        "{}",
        saved.title
    );
    assert_eq!(on_disk(root.path(), [1_400, 30]).as_deref(), Some("sand"));
    // A stroke that would pass the cap stops at it, says which chunks it
    // changed, and saves the tiles it painted.
    session
        .begin_stroke(now, "meadow", 0, "water", wide)
        .unwrap();
    session.stamp([0, 0]).unwrap();
    let Err(Error::Stamp(partial)) = session.stamp([2_000, 0]) else {
        panic!("a stroke past the cap went on");
    };
    assert!(!partial.dirty.is_empty());
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert_eq!(
        saved.title,
        format!("Paint water on {MAXIMUM_STROKE_TILES} tiles")
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(on_disk(root.path(), [1_400, 30]), None);
}

#[test]
fn an_interaction_changes_at_most_what_one_save_holds() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    session.begin_interaction(now).unwrap();
    let command = |cells, rows, chunks| Command {
        title: "Big".to_owned(),
        operation: String::new(),
        record: "{}".to_owned(),
        cells,
        rows,
        chunks,
    };
    // Each limit counts the interaction's commands together.
    let half = MAXIMUM_TRANSACTION_CELLS / 2;
    session.record(command(half, 1, 1)).unwrap();
    assert!(matches!(
        session.record(command(half + 1, 1, 1)),
        Err(Error::TooLarge)
    ));
    assert!(matches!(
        session.record(command(1, MAXIMUM_TRANSACTION_ROWS, 1)),
        Err(Error::TooLarge)
    ));
    assert!(matches!(
        session.record(command(1, 1, MAXIMUM_TRANSACTION_CHUNKS)),
        Err(Error::TooLarge)
    ));
    session.cancel_stroke().unwrap_err();
    assert!(session.discard().is_err(), "the interaction still runs");
}

#[test]
fn a_thin_stroke_over_painted_chunks_saves_within_its_inverse() {
    // A column of 700 chunks below the meadow, each with rows of terrain,
    // so the inverse sets every changed row back.
    let root = meadow_copy();
    let row = format!("\"{}\"", "gggs".repeat(8));
    let rows = vec![row; 32].join(", ");
    let operations: Vec<String> = (1..=700)
        .map(|cy| {
            let text = format!(
                "{{\"format\": \"tilewright.chunk/1\", \"plane\": \"meadow\", \"layer\": 0, \"chunk\": [0, {cy}], \"terrain\": {{\"ground\": {{\"palette\": {{\"g\": \"grass\", \"s\": \"sand\"}}, \"rows\": [{rows}]}}}}}}\n"
            );
            format!(
                "{{\"op\": \"document.create\", \"path\": \"planes/meadow/layers/0/0_{cy}.jsonc\", \"text\": {}}}",
                json_string(&text)
            )
        })
        .collect();
    let created = transaction("Column", &operations, &[]);
    let created = Transaction::parse("column.jsonc", created).unwrap();
    tilewright_edit::plan(root.path(), &created, &PlanOptions::default())
        .unwrap()
        .publish_with(&ACCEPTED)
        .unwrap();
    let opened = revision(root.path());
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    session
        .begin_stroke(now, "meadow", 0, "water", DOT)
        .unwrap();
    // One tile a row, straight down: the row cap stops it first.
    let mut stopped = None;
    for step in 0..7 {
        if let Err(error) = session.stamp([3, 32 + step * 4_000]) {
            stopped = Some(error);
            break;
        }
    }
    assert!(matches!(stopped, Some(Error::Stamp(_))), "{stopped:?}");
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert_eq!(
        saved.title,
        format!("Paint water on {MAXIMUM_TRANSACTION_ROWS} tiles")
    );
    assert_eq!(
        on_disk(root.path(), [3, 32 + 19_999]).as_deref(),
        Some("water")
    );
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
}

#[test]
fn an_outside_edit_elsewhere_in_a_saved_file_is_kept() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    // The session read the world before this edit to the same chunk.
    outside_edit(root.path(), [2, 2], "water");
    stroke(&mut session, Instant::now(), "sand", &[[1, 1]]);
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    assert_eq!(on_disk(root.path(), [2, 2]).as_deref(), Some("water"));
}

#[test]
fn a_coalesced_step_that_fails_partway_is_split() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let start = Instant::now();
    stroke(&mut session, start, "sand", &[[1, 1]]);
    stroke(
        &mut session,
        start + Duration::from_secs(1),
        "sand",
        &[[40, 1]],
    );
    assert_eq!(session.history().0.len(), 1);
    // An outside edit to the first save's chunk blocks undoing it, not the
    // second save, which undoes first.
    outside_edit(root.path(), [5, 5], "water");
    assert!(matches!(session.undo(), Err(Error::Rejected(_))));
    assert_eq!(session.history().0, ["Paint sand on 1 tile"]);
    assert_eq!(session.history().1, ["Paint sand on 1 tile"]);
    assert_eq!(on_disk(root.path(), [40, 1]), None);
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    // Redo gives the second save back.
    session.redo().unwrap().unwrap();
    assert_eq!(on_disk(root.path(), [40, 1]).as_deref(), Some("sand"));
}

/// File system faults that keep a save from being made, which the session
/// keeps waiting. Permissions do not stop a privileged user, so such a run
/// proves nothing and stops early.
#[cfg(unix)]
mod faults {
    use super::{DOT, Error, Options, Session, accepted, meadow_copy, on_disk, paint, stroke};
    use std::{
        os::unix::fs::PermissionsExt as _,
        path::Path,
        time::{Duration, Instant},
    };

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_save_that_cannot_be_published_waits_until_flushed_or_dropped() {
        let root = meadow_copy();
        let options = Options {
            coalesce: Duration::ZERO,
            ..accepted()
        };
        let mut session = Session::open(root.path(), options).unwrap();
        let now = Instant::now();
        let layer = root.path().join("planes/meadow/layers/0");
        set_mode(&layer, 0o555);
        paint(&mut session, now, "sand", &[[1, 1]]);
        let result = session.end_stroke(now);
        if result.is_ok() {
            set_mode(&layer, 0o755);
            return;
        }
        let Err(Error::Publish(error)) = result else {
            panic!("unexpected outcome: {result:?}");
        };
        assert!(!error.applied);
        assert!(session.is_waiting() && session.is_dirty());
        assert!(session.recovery().is_some());
        assert!(matches!(
            session.begin_stroke(now, "meadow", 0, "sand", DOT),
            Err(Error::Waiting)
        ));
        assert!(matches!(session.redo(), Err(Error::Waiting)));
        assert!(matches!(session.flush(now), Err(Error::Publish(_))));
        // Undo drops the waiting edits, and the preview with them.
        let undone = session.undo().unwrap().unwrap();
        assert!(undone.files.is_empty());
        assert!(!session.is_waiting());
        assert_eq!(
            session.terrain("meadow").unwrap().value(0, [1, 1], 0),
            Some("grass")
        );
        // So does discard.
        paint(&mut session, now, "sand", &[[2, 2]]);
        assert!(session.end_stroke(now).is_err());
        session.discard().unwrap();
        assert!(!session.is_waiting());
        assert_eq!(
            session.terrain("meadow").unwrap().value(0, [2, 2], 0),
            Some("grass")
        );
        // A flush once the folder can be written saves what waited.
        paint(&mut session, now, "sand", &[[3, 3]]);
        assert!(session.end_stroke(now).is_err());
        set_mode(&layer, 0o755);
        session.flush(now).unwrap().unwrap();
        assert_eq!(on_disk(root.path(), [3, 3]).as_deref(), Some("sand"));
        // The inverse of the failed publish stays until it is taken.
        stroke(&mut session, now, "sand", &[[4, 4]]);
        assert!(session.take_recovery().is_some());
        assert_eq!(session.recovery(), None);
    }

    #[test]
    fn a_file_that_cannot_be_read_keeps_the_save_waiting() {
        let root = meadow_copy();
        let mut session = Session::open(root.path(), accepted()).unwrap();
        let now = Instant::now();
        let chunk = root.path().join("planes/meadow/layers/0/0_0.jsonc");
        set_mode(&chunk, 0o000);
        paint(&mut session, now, "sand", &[[1, 1]]);
        let result = session.end_stroke(now);
        set_mode(&chunk, 0o644);
        if result.is_ok() {
            return;
        }
        assert!(matches!(result, Err(Error::Publish(_))), "{result:?}");
        assert!(session.is_waiting());
        assert_eq!(session.rejected(), None);
        session.flush(now).unwrap().unwrap();
        assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    }
}

#[test]
fn saving_waits_for_consent_where_the_toolkit_cannot_check_everything() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let options = Options {
        coalesce: Duration::ZERO,
        unchecked_access: false,
        ..Options::default()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1]]);
    match session.end_stroke(now) {
        // Linux, on a file system that shows its access control lists.
        Ok(Some(_)) => assert!(tilewright_edit::UNCHECKED_ACCESS.is_none()),
        Err(Error::NeedsConsent(unchecked)) => {
            assert!(!unchecked.is_empty());
            assert_eq!(revision(root.path()), opened, "nothing was written");
            assert!(session.is_waiting());
            // Nothing else starts until the save is answered.
            assert!(matches!(
                session.begin_stroke(now, "meadow", 0, "sand", DOT),
                Err(Error::Waiting)
            ));
            session.set_unchecked_access(true);
            session.flush(now).unwrap().unwrap();
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    assert!(!session.is_waiting());
}

#[test]
fn interactions_follow_their_order() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    assert!(matches!(session.end_interaction(now), Err(Error::Idle)));
    assert!(matches!(session.stamp([0, 0]), Err(Error::NoStroke)));
    session.begin_interaction(now).unwrap();
    assert!(matches!(session.begin_interaction(now), Err(Error::Busy)));
    assert!(matches!(session.undo(), Err(Error::Busy)));
    assert!(matches!(session.flush(now), Err(Error::Busy)));
    assert_eq!(session.end_interaction(now).unwrap(), None);
    // A stroke that cannot begin leaves nothing running.
    assert!(matches!(
        session.begin_stroke(now, "meadow", 0, "lava", DOT),
        Err(Error::Brush(_))
    ));
    assert!(!session.is_interacting());
}
