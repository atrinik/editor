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

/// Publishes `text` as an outside program would.
fn apply(root: &Path, text: &str) {
    let parsed = Transaction::parse("outside.jsonc", text).unwrap();
    tilewright_edit::plan(root, &parsed, &PlanOptions::default())
        .unwrap()
        .publish_with(&ACCEPTED)
        .unwrap();
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
fn a_malformed_command_is_rejected_not_kept_waiting() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    session.begin_interaction(now).unwrap();
    session
        .record(Command {
            title: "Broken".to_owned(),
            operation: "{\"op\": ".to_owned(),
            record: "{}".to_owned(),
        })
        .unwrap();
    assert!(matches!(
        session.end_interaction(now),
        Err(Error::Rejected(_))
    ));
    assert!(!session.is_waiting());
    assert_eq!(session.rejected().unwrap().title, "Broken");
}

/// A copy of the meadow whose ground chunks reach down a column of chunks
/// below it, `chunks` of them, each with rows of terrain and padded with a
/// comment of `padding` bytes, made by publishing transactions.
fn meadow_with_column(chunks: i64, padding: usize) -> tempfile::TempDir {
    let root = meadow_copy();
    let row = format!("\"{}\"", "gggs".repeat(8));
    let rows = vec![row; 32].join(", ");
    let comment = format!("// {}\n", "x".repeat(padding));
    for batch in (1..=chunks).collect::<Vec<_>>().chunks(100) {
        let operations: Vec<String> = batch
            .iter()
            .map(|cy| {
                let text = format!(
                    "{comment}{{\"format\": \"tilewright.chunk/1\", \"plane\": \"meadow\", \"layer\": 0, \"chunk\": [0, {cy}], \"terrain\": {{\"ground\": {{\"palette\": {{\"g\": \"grass\", \"s\": \"sand\"}}, \"rows\": [{rows}]}}}}}}\n"
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
    }
    root
}

/// Paints water straight down x = 3 from the top of the column through
/// `tiles` tiles, one a row, in a session that saves each release.
fn thin_stroke(
    root: &Path,
    tiles: i64,
) -> (
    Session,
    Result<Option<tilewright_editor_commands::session::Saved>, Error>,
) {
    let options = Options {
        coalesce: Duration::ZERO,
        ..accepted()
    };
    let mut session = Session::open(root, options).unwrap();
    let now = Instant::now();
    session
        .begin_stroke(now, "meadow", 0, "water", DOT)
        .unwrap();
    // Pointer samples at most 4,000 tiles apart, so the brush joins them.
    let last = 32 + tiles - 1;
    let mut y = 32;
    session.stamp([3, y]).unwrap();
    while y < last {
        y = (y + 4_000).min(last);
        session.stamp([3, y]).unwrap();
    }
    let saved = session.end_stroke(now);
    (session, saved)
}

#[test]
fn a_thin_stroke_saves_only_when_it_can_be_undone() {
    // Each changed row of an existing chunk is one operation of the
    // inverse, and each palette entry the stroke adds one more, so 1,100
    // chunks of rows make inverses near the toolkit's limits.
    let root = meadow_with_column(1_100, 0);
    let opened = revision(root.path());
    // Past them, the save's own inverse is refused.
    let (session, saved) = thin_stroke(root.path(), 35_000);
    assert!(matches!(saved, Err(Error::TooLarge(_))), "{saved:?}");
    assert_eq!(revision(root.path()), opened, "nothing was written");
    assert!(!session.is_waiting() && session.rejected().is_some());
    // Just under them, the save's inverse fits, but undoing it would build
    // an inverse of its own past them, each palette entry removed coming
    // back as a longer insert: refused too, before anything is written.
    let (session, saved) = thin_stroke(root.path(), 29_600);
    let Err(Error::TooLarge(reason)) = saved else {
        panic!("{saved:?}");
    };
    assert!(reason.contains("could not be"), "{reason}");
    assert_eq!(revision(root.path()), opened, "nothing was written");
    assert!(!session.is_waiting() && session.rejected().is_some());
    // A stroke whose undo fits saves, undoes and redoes.
    let (mut session, saved) = thin_stroke(root.path(), 28_000);
    saved.unwrap().unwrap();
    assert_eq!(
        on_disk(root.path(), [3, 32 + 27_999]).as_deref(),
        Some("water")
    );
    let painted = revision(root.path());
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
    session.redo().unwrap().unwrap();
    assert_eq!(revision(root.path()), painted);
}

#[test]
fn a_stroke_over_large_chunks_saves_when_its_undo_fits() {
    // 200 chunks of 200 KB each: undoing a stroke down all of them reads
    // and parses each again, well within the toolkit's budgets, so it saves
    // and undoes byte for byte.
    let root = meadow_with_column(200, 200_000);
    let opened = revision(root.path());
    let (mut session, saved) = thin_stroke(root.path(), 200 * 32);
    saved.unwrap().unwrap();
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
}

#[test]
fn a_save_its_edits_make_invalid_is_rejected_when_the_world_loads_without_them() {
    // A terrain the session read is removed outside the editor; the tree
    // still loads, so a save that paints it is the problem.
    let root = meadow_copy();
    let add = transaction(
        "Add mud",
        &[
            "{\"op\": \"insert\", \"path\": \"profile/transitions.jsonc\", \"pointer\": [\"terrains\", 3], \"value\": {\"id\": \"mud\", \"protos\": [\"mud\"]}}".to_owned(),
            "{\"op\": \"document.create\", \"path\": \"defs/proto/mud.jsonc\", \"text\": \"{\\\"format\\\": \\\"tilewright.def/1\\\", \\\"type\\\": \\\"proto\\\", \\\"id\\\": \\\"mud\\\", \\\"fields\\\": {}}\\n\"}".to_owned(),
        ],
        &[],
    );
    apply(root.path(), &add);
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let remove = transaction(
        "Remove mud",
        &[
            "{\"op\": \"remove\", \"path\": \"profile/transitions.jsonc\", \"pointer\": [\"terrains\", {\"id\": \"mud\"}]}".to_owned(),
            "{\"op\": \"document.delete\", \"path\": \"defs/proto/mud.jsonc\"}".to_owned(),
        ],
        &[],
    );
    apply(root.path(), &remove);
    let now = Instant::now();
    paint(&mut session, now, "mud", &[[1, 1]]);
    let result = session.end_stroke(now);
    assert!(matches!(result, Err(Error::Rejected(_))), "{result:?}");
    assert!(!session.is_waiting());
    // The world was read again, so the terrain is gone from the session.
    assert!(matches!(
        session.begin_stroke(now, "meadow", 0, "mud", DOT),
        Err(Error::Brush(_))
    ));
}

#[test]
fn a_save_with_more_conflicts_than_reported_is_rejected_with_them() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    // An outside program paints chunk 0_0 with water and creates 1_0, which
    // the session does not know of.
    let cells: Vec<String> = (0..32)
        .flat_map(|y| (0..64).map(move |x| format!("{{\"at\": [{x}, {y}], \"value\": \"water\"}}")))
        .collect();
    let operation = format!(
        "{{\"op\": \"terrain.set\", \"plane\": \"meadow\", \"layer\": 0, \"grid\": \"ground\", \"cells\": [{}]}}",
        cells.join(", ")
    );
    apply(root.path(), &transaction("Outside", &[operation], &[]));
    let opened = revision(root.path());
    let now = Instant::now();
    let brush = Brush {
        shape: Shape::Square,
        radius: 32,
    };
    session
        .begin_stroke(now, "meadow", 0, "sand", brush)
        .unwrap();
    session.stamp([32, 16]).unwrap();
    // Thousands of tiles conflict: past the toolkit's cap on what it
    // reports, it says there may be more, which is not a limit the stroke
    // reached.
    let Err(Error::Rejected(rejected)) = session.end_stroke(now) else {
        panic!("not rejected");
    };
    let tilewright_editor_commands::Error::Rejected(problems) = rejected;
    assert_eq!(problems.len(), tilewright_edit::limits::DIAGNOSTICS);
    assert!(problems.iter().any(|problem| problem.code == "tx.conflict"));
    let reason = &session.rejected().unwrap().reason;
    assert!(!reason.contains("too large"), "{reason}");
    assert_eq!(revision(root.path()), opened, "nothing was written");
    assert!(!session.is_waiting());
}

/// The meadow with its profile at the root, as `editor.jsonc`, and the
/// profile's other files beside it.
fn meadow_with_profile_at_the_root() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for (path, text) in MEADOW_FILES {
        let (path, text) = if *path == "world.jsonc" {
            (
                (*path).to_owned(),
                text.replace("profile/profile.jsonc", "editor.jsonc"),
            )
        } else if let Some(rest) = path.strip_prefix("profile/") {
            let rest = if rest == "profile.jsonc" {
                "editor.jsonc"
            } else {
                rest
            };
            (rest.to_owned(), (*text).to_owned())
        } else {
            ((*path).to_owned(), (*text).to_owned())
        };
        let target = directory.path().join(&path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, text).unwrap();
    }
    directory
}

#[test]
fn a_world_file_is_never_taken_for_the_transaction() {
    // Whatever the world names its files, its problems are not the edits'
    // own: with the profile at the root, a profile broken by another
    // program keeps the save waiting, as anywhere else.
    let root = meadow_with_profile_at_the_root();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    std::fs::write(root.path().join("editor.jsonc"), "{ not valid").unwrap();
    paint(&mut session, Instant::now(), "sand", &[[1, 1]]);
    let result = session.end_stroke(Instant::now());
    assert!(matches!(result, Err(Error::Publish(_))), "{result:?}");
    assert!(session.is_waiting() && session.rejected().is_none());
}

#[test]
fn a_problem_elsewhere_in_the_tree_keeps_the_save_waiting() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), accepted()).unwrap();
    let now = Instant::now();
    // A stray file the loader refuses, which the save does not touch.
    let stray = root.path().join("planes/meadow/stray.txt");
    std::fs::write(&stray, "not part of the world").unwrap();
    paint(&mut session, now, "sand", &[[1, 1]]);
    let result = session.end_stroke(now);
    assert!(matches!(result, Err(Error::Publish(_))), "{result:?}");
    assert!(session.is_waiting());
    assert_eq!(session.rejected(), None);
    assert_eq!(
        session.terrain("meadow").unwrap().value(0, [1, 1], 0),
        Some("sand")
    );
    std::fs::remove_file(&stray).unwrap();
    session.flush(now).unwrap().unwrap();
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
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
    use super::{DOT, Error, Options, Session, accepted, meadow_copy, on_disk, paint};
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
        // Nothing was written, so nothing is left to restore.
        assert!(session.recoveries().is_empty());
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
        assert!(session.recoveries().is_empty());
    }

    #[test]
    fn an_unrelated_file_that_cannot_be_read_keeps_the_save_waiting() {
        let root = meadow_copy();
        let mut session = Session::open(root.path(), accepted()).unwrap();
        let now = Instant::now();
        let unrelated = root.path().join("defs/proto/daisies.jsonc");
        set_mode(&unrelated, 0o000);
        paint(&mut session, now, "sand", &[[1, 1]]);
        let result = session.end_stroke(now);
        set_mode(&unrelated, 0o644);
        if result.is_ok() {
            return;
        }
        assert!(matches!(result, Err(Error::Publish(_))), "{result:?}");
        assert!(session.is_waiting());
        assert_eq!(session.rejected(), None);
        session.flush(now).unwrap().unwrap();
        assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
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
