// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sessions save edits to a copy of the fixture world when interactions
//! end, as one undo step per coalesced group, and undo and redo through
//! the transactions' inverses. Publishing keeps every guarantee only on
//! Linux, so these tests run there.

#![cfg(target_os = "linux")]

use std::{
    path::Path,
    time::{Duration, Instant},
};
use tilewright_edit::{Options as PlanOptions, Transaction};
use tilewright_editor_commands::{
    brush::{Brush, Shape},
    json_string,
    session::{Error, Options, Session},
    transaction,
};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_testkit::MEADOW_FILES;

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
        .publish()
        .unwrap();
    directory
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
fn a_release_saves_at_once_without_a_window_and_undo_restores_every_byte() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let options = Options {
        coalesce: Duration::ZERO,
        ..Options::default()
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
    assert!(!session.is_dirty());
    assert_eq!(on_disk(root.path(), [11, 10]).as_deref(), Some("sand"));
    let first = revision(root.path());
    assert_eq!(session.document().revision(), first);
    // Sand from the beach into the chunk east of it, which is created.
    paint(&mut session, now, "sand", &[[30, 3], [33, 3]]);
    let saved = session.end_stroke(now).unwrap().unwrap();
    assert_eq!(saved.title, "Paint sand on 2 tiles");
    assert_eq!(saved.files, ["planes/meadow/layers/0/1_0.jsonc"]);
    assert_eq!(on_disk(root.path(), [33, 3]).as_deref(), Some("sand"));
    let second = revision(root.path());

    // Undo goes back a step at a time, byte for byte.
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
            vec!["Paint sand on 3 tiles", "Paint sand on 2 tiles"]
        )
    );

    session.redo().unwrap().unwrap();
    session.redo().unwrap().unwrap();
    assert_eq!(revision(root.path()), second);
    assert_eq!(session.redo().unwrap(), None);
    assert_eq!(
        session.history(),
        (
            vec!["Paint sand on 3 tiles", "Paint sand on 2 tiles"],
            vec![]
        )
    );
}

#[test]
fn interactions_within_the_window_are_one_save_and_one_undo_step() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let mut session = Session::open(root.path(), Options::default()).unwrap();
    let start = Instant::now();
    paint(&mut session, start, "sand", &[[1, 1]]);
    assert_eq!(session.end_stroke(start).unwrap(), None);
    // The next stroke starts within the window, so it joins the first.
    let second = start + Duration::from_millis(1_500);
    paint(&mut session, second, "water", &[[2, 2], [3, 2]]);
    assert_eq!(session.tick(second).unwrap(), None, "a stroke is running");
    assert_eq!(session.end_stroke(second).unwrap(), None);
    assert_eq!(revision(root.path()), opened, "nothing saved yet");
    let pending = session.pending_transaction().unwrap();
    Transaction::parse("pending.jsonc", pending).unwrap();
    // Not yet: the window runs from the last release.
    assert_eq!(
        session.tick(second + Duration::from_millis(1_999)).unwrap(),
        None
    );
    let saved = session
        .tick(second + Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(saved.title, "Paint sand on 1 tile and 1 more edit");
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    assert_eq!(on_disk(root.path(), [3, 2]).as_deref(), Some("water"));
    assert_eq!(session.history().0.len(), 1);
    // One undo reverts both strokes.
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
}

#[test]
fn an_interaction_after_the_window_saves_the_last_one_first() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), Options::default()).unwrap();
    let start = Instant::now();
    paint(&mut session, start, "sand", &[[1, 1]]);
    session.end_stroke(start).unwrap();
    // No tick came, but the window has passed when the next press does.
    let later = start + Duration::from_secs(3);
    let saved = session
        .begin_stroke(later, "meadow", 0, "water", DOT)
        .unwrap()
        .unwrap();
    assert_eq!(saved.title, "Paint sand on 1 tile");
    session.stamp([5, 5]).unwrap();
    session.end_stroke(later).unwrap();
    session.flush().unwrap().unwrap();
    assert_eq!(
        session.history().0,
        ["Paint sand on 1 tile", "Paint water on 1 tile"]
    );
}

#[test]
fn undo_drops_pending_edits_without_writing_and_redo_saves_them() {
    let root = meadow_copy();
    let opened = revision(root.path());
    let mut session = Session::open(root.path(), Options::default()).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1]]);
    session.end_stroke(now).unwrap();
    let undone = session.undo().unwrap().unwrap();
    assert!(undone.files.is_empty());
    assert!(!session.is_dirty());
    assert_eq!(
        session.terrain("meadow").unwrap().value(0, [1, 1], 0),
        Some("grass")
    );
    assert_eq!(revision(root.path()), opened);
    let redone = session.redo().unwrap().unwrap();
    assert_eq!(redone.files, ["planes/meadow/layers/0/0_0.jsonc"]);
    assert_eq!(on_disk(root.path(), [1, 1]).as_deref(), Some("sand"));
    // The redone step was saved, so it undoes through its inverse.
    session.undo().unwrap().unwrap();
    assert_eq!(revision(root.path()), opened);
}

#[test]
fn a_new_edit_clears_redo() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        ..Options::default()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1]]);
    session.end_stroke(now).unwrap();
    session.undo().unwrap();
    assert_eq!(session.history().1.len(), 1);
    paint(&mut session, now, "water", &[[2, 2]]);
    session.end_stroke(now).unwrap();
    assert_eq!(session.history(), (vec!["Paint water on 1 tile"], vec![]));
    assert_eq!(session.redo().unwrap(), None);
}

#[test]
fn cancelling_a_stroke_puts_its_tiles_back() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), Options::default()).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1], [40, 1]]);
    let dirty = session.cancel_stroke(now).unwrap();
    assert!(dirty.contains(&(0, 0, 0)) && dirty.contains(&(0, 1, 0)));
    let terrain = session.terrain("meadow").unwrap();
    assert_eq!(terrain.value(0, [1, 1], 0), Some("grass"));
    assert_eq!(terrain.value(0, [40, 1], 0), None);
    assert!(!session.is_dirty());
    assert_eq!(session.flush().unwrap(), None);
}

#[test]
fn undo_after_an_outside_change_is_a_conflict_and_keeps_the_step() {
    let root = meadow_copy();
    let options = Options {
        coalesce: Duration::ZERO,
        ..Options::default()
    };
    let mut session = Session::open(root.path(), options).unwrap();
    let now = Instant::now();
    paint(&mut session, now, "sand", &[[1, 1]]);
    session.end_stroke(now).unwrap();
    // Another program changes the same chunk.
    let outside = transaction(
        "Outside edit",
        &["{\"op\": \"terrain.set\", \"plane\": \"meadow\", \"layer\": 0, \"grid\": \"ground\", \"cells\": [{\"at\": [2, 2], \"value\": \"water\"}]}".to_owned()],
        &[],
    );
    let outside = Transaction::parse("outside.jsonc", outside).unwrap();
    tilewright_edit::plan(root.path(), &outside, &PlanOptions::default())
        .unwrap()
        .publish()
        .unwrap();
    let Err(Error::Rejected(rejected)) = session.undo() else {
        panic!("undo over an outside change was accepted");
    };
    assert!(rejected.to_string().contains("tx.conflict"), "{rejected}");
    assert_eq!(session.history().0, ["Paint sand on 1 tile"]);
    assert_eq!(on_disk(root.path(), [2, 2]).as_deref(), Some("water"));
}

#[test]
fn interactions_follow_their_order() {
    let root = meadow_copy();
    let mut session = Session::open(root.path(), Options::default()).unwrap();
    let now = Instant::now();
    assert!(matches!(session.end_interaction(now), Err(Error::Idle)));
    assert!(matches!(session.stamp([0, 0]), Err(Error::NoStroke)));
    session.begin_interaction(now).unwrap();
    assert!(matches!(session.begin_interaction(now), Err(Error::Busy)));
    assert!(matches!(session.undo(), Err(Error::Busy)));
    assert!(matches!(session.flush(), Err(Error::Busy)));
    assert_eq!(session.end_interaction(now).unwrap(), None);
}
