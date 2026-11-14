// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The review text is an engine contract: these tests pin `Review::of`
//! and the bounds of `PendingSummary`.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, Preview, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::review::{
    ChangeKind, MAXIMUM_REVIEW_FILES, PendingSummary, REVIEW_LIMIT_MESSAGE, REVIEW_WARNING_BYTES,
    REVIEW_WARNINGS_OMITTED, Review,
};
use tilewright_editor_ui::{Action, Actor, Session};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

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
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
/// The neutral fixture with its main chunk rewritten in one known layout,
/// whatever the checkout's line endings.
fn fixture_with(configure: impl FnOnce(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    edit_json(dir.path(), CHUNK, |_| {});
    configure(dir.path());
    dir
}
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new("agent:review-test").unwrap(),
    )
}
#[derive(Default)]
struct Records;
impl RecoveryStore for Records {
    fn retain(&mut self, _inverse: &str) -> Result<(), String> {
        Ok(())
    }
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
/// FNV-1a over the lines joined by newlines: a stable digest for pinning
/// reviews too long to spell out.
fn digest(lines: &[String]) -> u64 {
    lines
        .join("\n")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}
fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}
/// A raw transaction prepared by History alone.
fn prepared(root: &Path, operations: &Value) -> (History, [u8; 32]) {
    let mut history = History::new(WorldDocument::open(root).unwrap());
    let base = history.document().revision();
    let text = json!({"format": "tilewright.transaction/1", "actor": "test",
        "operations": operations})
    .to_string();
    history.prepare(base, "review test", &text).unwrap();
    (history, base)
}
/// One `terrain.set` that paints one cell in each of `count` new chunks,
/// so it creates `count` chunk documents.
fn new_chunks(count: i64) -> Value {
    let cells: Vec<Value> = (1..=count)
        .map(|chunk| json!({"at": [chunk * 32, 0], "value": "grass", "expect": null}))
        .collect();
    json!([{"op": "terrain.set", "plane": "main", "layer": 0, "grid": "floor", "cells": cells}])
}
fn pending(history: &History) -> &Preview {
    history.pending().unwrap()
}

const OMITTED: &str = "[...] marks unchanged source omitted from this excerpt.";
const HEADER: &str = "planes/main/layers/0/0_0.jsonc / source excerpt; scroll for more";

#[test]
fn a_small_edit_reviews_a_change_centred_excerpt() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path());
    let action = Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: 0,
        y: 0,
        proto: "sand".into(),
        expected: revision(&session),
    };
    session.dispatch(action, &mut Records).unwrap();
    let review = session.review();
    assert!(review.complete);
    assert_eq!(
        review.lines,
        strings(&[
            HEADER,
            OMITTED,
            r#"- [...]      "rows": [\u{a}        "ggggssss................. [...]"#,
            r#"+ [...]      "rows": [\u{a}        "sgggssss................. [...]"#,
        ])
    );
}

#[test]
fn warnings_come_first_and_long_excerpts_split_into_256_byte_rows() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path());
    let action = Action::Remove {
        uid: "marker_2".into(),
        expected: revision(&session),
    };
    session.dispatch(action, &mut Records).unwrap();
    let review = session.review();
    assert!(review.complete);
    assert_eq!(review.warnings_shown, 1);
    assert_eq!(
        review.lines,
        strings(&[
            "Uid marker_2 is removed with the object; do not reuse it for other objects.",
            HEADER,
            OMITTED,
            r#"- [...] {\u{a}      "at": [\u{a}        1,\u{a}        4\u{a}      ],\u{a}      "fields": {\u{a}        "label": "Spare marker"\u{a}      },\u{a}      "layer": "item",\u{a}      "proto": "marker",\u{a}      "uid": "marker_2"\u{a}    },\u{a}    {\u{a}      "at": [\"#,
            r#"+ [...] {\u{a}      "at": [\u{a}        7,\u{a}        0\u{a}      ],\u{a}   [...]"#,
            r#"- u{a}        7,\u{a}        0\u{a}      ],\u{a}   [...]"#,
        ])
    );
}

#[test]
fn a_long_change_is_cut_into_rows_of_at_most_256_escaped_bytes() {
    let dir = fixture_with(|root| {
        edit_json(root, "profile/schema/object.jsonc", |schema| {
            schema["fields"]["label"]["max_length"] = Value::from(4096);
        });
    });
    let mut session = open(dir.path());
    let action = Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: json!("x".repeat(600)),
        expected: revision(&session),
    };
    session.dispatch(action, &mut Records).unwrap();
    let start = r#"s": {\u{a}        "label": ""#;
    let end = r#"",\u{a}        "settings": {"#;
    assert_eq!(start.len() + 228, 256);
    assert_eq!(
        session.review().lines,
        vec![
            HEADER.to_owned(),
            OMITTED.to_owned(),
            format!(r#"- [...] {start}Trail marker{end} [...]"#),
            format!("+ [...] {start}{}", "x".repeat(228)),
            format!("+ {}", "x".repeat(256)),
            format!("+ {}{end} [...]", "x".repeat(116)),
        ]
    );
}

#[test]
fn thirty_two_files_are_reviewed_and_a_thirty_third_makes_the_review_incomplete() {
    let headers = |review: &Review| {
        review
            .lines
            .iter()
            .filter(|line| line.ends_with("/ source excerpt; scroll for more"))
            .count()
    };
    let dir = fixture_with(|_| {});
    let (history, base) = prepared(dir.path(), &new_chunks(32));
    let review = Review::of(Some(pending(&history)), &[]);
    assert!(review.complete);
    assert_eq!(headers(&review), 32);
    assert_eq!(review.lines.len(), 320);
    assert_eq!(
        review.lines[..4],
        strings(&[
            "planes/main/layers/0/10_0.jsonc / source excerpt; scroll for more",
            OMITTED,
            "- <absent>",
            r#"+ {\u{a}  "format": "tilewright.chunk/1",\u{a}  "plane": "main",\u{a}  "layer": 0,\u{a}  "chunk": [10, 0],\u{a}  "terrain": {\u{a}    "floor": {\u{a}      "palette": {"g": "grass"},\u{a}      "rows": [\u{a}        "g...............................",\u{a}    "#,
        ])
    );
    assert_eq!(digest(&review.lines), 0x929e_3392_851d_5823);
    let summary = PendingSummary::of(base, pending(&history), &review, &[]);
    assert_eq!(summary.files.len(), MAXIMUM_REVIEW_FILES);
    assert_eq!(summary.files_omitted, 0);
    assert!(summary.complete);

    let dir = fixture_with(|_| {});
    let (history, base) = prepared(dir.path(), &new_chunks(33));
    let review = Review::of(Some(pending(&history)), &[]);
    assert!(!review.complete);
    assert_eq!(review.lines[0], REVIEW_LIMIT_MESSAGE);
    assert_eq!(headers(&review), 32);
    assert_eq!(review.lines.len(), 321);
    assert_eq!(digest(&review.lines), 0xac16_32ff_a667_c5f2);
    let summary = PendingSummary::of(base, pending(&history), &review, &[]);
    assert_eq!(summary.files.len(), MAXIMUM_REVIEW_FILES);
    assert_eq!(summary.files_omitted, 1);
    assert!(!summary.complete);
    assert!(
        summary
            .files
            .iter()
            .all(|file| file.kind == ChangeKind::Created
                && file.before_bytes.is_none()
                && file.after_bytes.is_some_and(|bytes| bytes > 0))
    );
    assert_eq!(summary.result, pending(&history).world().source_digest());
}

#[test]
fn a_deleted_document_is_a_removed_file() {
    let dir = fixture_with(|_| {});
    let path = "planes/main/layers/0/-1_0.jsonc";
    let size = fs::read(dir.path().join(path)).unwrap().len();
    let (history, base) = prepared(
        dir.path(),
        &json!([{"op": "document.delete", "path": path}]),
    );
    let review = Review::of(Some(pending(&history)), &[]);
    let summary = PendingSummary::of(base, pending(&history), &review, &[]);
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, path);
    assert_eq!(summary.files[0].kind, ChangeKind::Removed);
    assert_eq!(summary.files[0].before_bytes, Some(size));
    assert_eq!(summary.files[0].after_bytes, None);
    assert_eq!(review.lines[3], "+ <absent>");
}

#[test]
fn warnings_past_the_allowance_are_omitted_and_counted() {
    let dir = fixture_with(|_| {});
    let (history, base) = prepared(dir.path(), &new_chunks(1));
    let warnings: Vec<String> = (0..20)
        .map(|n| format!("{n:04} {}", "w".repeat(995)))
        .collect();
    let fits = REVIEW_WARNING_BYTES / 1000;
    let review = Review::of(Some(pending(&history)), &warnings);
    assert_eq!(review.warnings_shown, fits);
    assert_eq!(review.lines[..fits], warnings[..fits]);
    assert_eq!(review.lines[fits], REVIEW_WARNINGS_OMITTED);
    // Warnings never make a review incomplete.
    assert!(review.complete);
    let summary = PendingSummary::of(base, pending(&history), &review, &warnings);
    assert_eq!(summary.warnings, &warnings[..fits]);
    assert_eq!(summary.warnings_omitted, warnings.len() - fits);
}
