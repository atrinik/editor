// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
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
fn fixture() -> (tempfile::TempDir, Session) {
    fixture_with(|_| {})
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
#[derive(Default)]
struct Records {
    fail: bool,
    inverses: Vec<String>,
}
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        if self.fail {
            return Err("injected durable retention failure".into());
        }
        self.inverses.push(inverse.into());
        Ok(())
    }
}
fn paint(session: &Session, x: i64, y: i64) -> Action {
    Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
        proto: "sand".into(),
        expected: session.history().document().revision(),
    }
}
fn movement(session: &Session, x: i64) -> Action {
    Action::Move {
        uid: "marker_1".into(),
        x,
        y: 2,
        expected: session.history().document().revision(),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct PendingSnapshot {
    sealed: String,
    inverse: String,
    files: Vec<tilewright_edit::FileChange>,
    revision: [u8; 32],
}
fn pending_snapshot(session: &Session) -> PendingSnapshot {
    let pending = session.history().pending().unwrap();
    PendingSnapshot {
        sealed: pending.sealed().into(),
        inverse: pending.inverse().into(),
        files: pending.files().to_vec(),
        revision: pending.world().source_digest(),
    }
}

fn object_key() -> SelectionKey {
    SelectionKey::Object {
        uid: "marker_1".into(),
    }
}
fn cell_key() -> SelectionKey {
    SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: 6,
        y: 4,
    }
}
fn select(session: &mut Session, key: SelectionKey, records: &mut impl RecoveryStore) {
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key,
            },
            records,
        )
        .unwrap();
}

#[test]
fn pending_review_rejects_every_new_preparation_without_replacing_evidence() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut records = Records::default();
    assert!(session.can_prepare());
    select(&mut session, object_key(), &mut records);
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    let pending = pending_snapshot(&session);
    let generation = session.generation();
    assert!(!session.can_prepare());
    for action in [
        movement(&session, 5),
        Action::SetField {
            uid: "marker_1".into(),
            field: "label".into(),
            value: serde_json::json!("replacement"),
            expected: session.history().document().revision(),
        },
        paint(&session, 6, 4),
        Action::Undo,
        Action::Redo,
    ] {
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert!(error.to_string().contains("review pending transaction"));
        assert_eq!(pending_snapshot(&session), pending);
        assert_eq!(session.status().phase, SessionPhase::Pending);
        assert_eq!(session.generation(), generation);
        assert_eq!(session.selection(), Some(&object_key()));
        assert_eq!(session.history().depths(), (0, 0));
        assert_eq!(fs::read(&chunk).unwrap(), original);
        assert!(records.inverses.is_empty());
    }
    // Navigation and deselection do not discard the reviewed transaction.
    session.invalidate_frame().unwrap();
    select(&mut session, cell_key(), &mut records);
    session
        .dispatch(Action::ClearSelection, &mut records)
        .unwrap();
    assert!(session.selection().is_none());
    assert_eq!(pending_snapshot(&session), pending);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(session.can_prepare());
    assert_eq!(fs::read(chunk).unwrap(), original);
    assert!(records.inverses.is_empty());
}

#[test]
fn invalid_selection_keys_preserve_selection_and_explicit_clear_is_idempotent() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    select(&mut session, object_key(), &mut records);
    for key in [
        SelectionKey::Object {
            uid: "missing".into(),
        },
        SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: -1,
            y: 0,
        },
        SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "missing".into(),
            x: 0,
            y: 0,
        },
        SelectionKey::Cell {
            plane: "main".into(),
            layer: 1,
            grid: "floor".into(),
            x: 0,
            y: 0,
        },
    ] {
        assert!(
            session
                .dispatch(
                    Action::Select {
                        generation: session.generation(),
                        key,
                    },
                    &mut records
                )
                .is_err()
        );
        assert_eq!(session.selection(), Some(&object_key()));
    }
    for _ in 0..2 {
        session
            .dispatch(Action::ClearSelection, &mut records)
            .unwrap();
        assert!(session.selection().is_none());
        assert!(session.can_prepare());
    }
    assert!(records.inverses.is_empty());
}
#[test]
fn pending_scene_diff_and_cancel_never_write() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let bytes = fs::read(&chunk).unwrap();
    let generation = session.generation();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    assert!(
        session
            .history()
            .pending()
            .unwrap()
            .changed()
            .next()
            .is_some()
    );
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([4, 2]));
    assert_eq!(
        session
            .history()
            .document()
            .world()
            .object("marker_1")
            .unwrap()
            .at,
        Some([3, 2])
    );
    assert!(session.generation() > generation);
    assert_eq!(fs::read(&chunk).unwrap(), bytes);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(fs::read(chunk).unwrap(), bytes);
    assert!(records.inverses.is_empty());
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([3, 2]));
}
#[test]
fn rejects_stale_frames_revisions_bounds_and_unsupported_capabilities() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let key = SelectionKey::Object {
        uid: "marker_1".into(),
    };
    assert!(
        session
            .dispatch(
                Action::Select {
                    generation: 0,
                    key: key.clone()
                },
                &mut records
            )
            .is_err()
    );
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key,
            },
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .dispatch(movement(&session, i64::MAX), &mut records)
            .is_err()
    );
    assert!(
        session
            .dispatch(
                Action::Move {
                    uid: "marker_1".into(),
                    x: 4,
                    y: 2,
                    expected: [0; 32]
                },
                &mut records
            )
            .is_err()
    );
    assert!(
        session
            .dispatch(
                Action::SetField {
                    uid: "marker_1".into(),
                    field: "proto".into(),
                    value: serde_json::json!("evil"),
                    expected: session.history().document().revision()
                },
                &mut records
            )
            .is_err()
    );
    assert!(
        session
            .dispatch(paint(&session, -1, 0), &mut records)
            .is_err()
    );
    assert!(session.history().pending().is_none());
}
#[test]
fn typed_strings_escape_and_invalid_types_never_publish() {
    let (dir, mut session) = fixture();
    let mut records = Records::default();
    let original = WorldDocument::open(dir.path()).unwrap().revision();
    let label = "quote \" backslash \\ newline\n snowman ☃";
    session
        .dispatch(
            Action::SetField {
                uid: "marker_1".into(),
                field: "label".into(),
                value: serde_json::json!(label),
                expected: original,
            },
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .world()
            .object("marker_1")
            .unwrap()
            .fields
            .iter()
            .any(|(name, value)| name == "label"
                && *value == tilewright_source::FieldValue::String(label.into()))
    );
    assert_eq!(
        WorldDocument::open(dir.path()).unwrap().revision(),
        original
    );
    // Finish reviewing the valid draft first so the next action exercises
    // World's type validation, rather than the one-pending-action fence.
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(
        session
            .dispatch(
                Action::SetField {
                    uid: "marker_1".into(),
                    field: "label".into(),
                    value: serde_json::json!(42),
                    expected: original
                },
                &mut records
            )
            .is_err()
    );
    assert!(session.history().pending().is_none());
}

#[cfg(target_os = "linux")]
fn fixture_without_field_values(
    nested: bool,
    empty_container: bool,
) -> (tempfile::TempDir, Session) {
    fixture_with(|root| {
        let path = root.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let object = if nested {
            chunk["objects"][0]["inventory"] = serde_json::json!([
                {"uid": "nested_marker", "proto": "marker"}
            ]);
            let catalog_path = root.join("preview.jsonc");
            let mut catalog: serde_json::Value =
                serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
            catalog["editing"]["fields"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"uid": "nested_marker", "field": "label"}));
            fs::write(catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
            &mut chunk["objects"][0]["inventory"][0]
        } else {
            &mut chunk["objects"][0]
        };
        object.as_object_mut().unwrap().remove("fields");
        if empty_container {
            object["fields"] = serde_json::json!({});
        }
        let source = serde_json::to_string_pretty(&chunk).unwrap().replace(
            "\"fields\": {}",
            "\"fields\": {/* retained empty fields comment */}",
        );
        fs::write(path, format!("// retained chunk comment\n{source}\n")).unwrap();
    })
}

#[cfg(target_os = "linux")]
#[test]
fn optional_field_addition_handles_absent_or_commented_empty_parents_and_nested_uids() {
    for (nested, empty_container) in [(false, false), (false, true), (true, false), (true, true)] {
        let (dir, mut session) = fixture_without_field_values(nested, empty_container);
        let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
        let original = fs::read(&path).unwrap();
        let uid = if nested { "nested_marker" } else { "marker_1" };
        let revision = session.history().document().revision();
        let mut records = Records::default();
        let action = |value| Action::SetField {
            uid: uid.into(),
            field: "label".into(),
            value,
            expected: revision,
        };
        // An omitted parent is no excuse to bypass the authoritative schema.
        assert!(
            session
                .dispatch(action(serde_json::json!(42)), &mut records)
                .is_err()
        );
        assert!(session.history().pending().is_none());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(records.inverses.is_empty());
        session
            .dispatch(action(serde_json::json!("new label")), &mut records)
            .unwrap();
        let transaction: serde_json::Value =
            serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
        if empty_container {
            assert_eq!(transaction["operations"].as_array().unwrap().len(), 1);
            assert_eq!(
                transaction["operations"][0]["pointer"],
                serde_json::json!(["fields", "label"])
            );
        } else {
            assert_eq!(transaction["operations"][0]["op"], "test");
            assert_eq!(transaction["operations"][0]["absent"], true);
            assert_eq!(
                transaction["operations"][0]["pointer"],
                serde_json::json!(["fields"])
            );
            assert_eq!(
                transaction["operations"][1]["value"],
                serde_json::json!({"label": "new label"})
            );
        }
        assert_eq!(
            session.world().object(uid).unwrap().fields,
            vec![(
                "label".into(),
                tilewright_source::FieldValue::String("new label".into())
            )]
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        session.dispatch(Action::Accept, &mut records).unwrap();
        let added = fs::read(&path).unwrap();
        let added_text = std::str::from_utf8(&added).unwrap();
        assert!(added_text.starts_with("// retained chunk comment\n"));
        if empty_container {
            assert!(added_text.contains("/* retained empty fields comment */"));
        }
        assert!(
            session
                .dispatch(action(serde_json::json!("stale draft")), &mut records)
                .unwrap_err()
                .to_string()
                .contains("stale source revision")
        );
        assert!(session.history().pending().is_none());
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        session.dispatch(Action::Redo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read(&path).unwrap(), added);
        assert_eq!(records.inverses.len(), 3);
    }
}

#[test]
fn changed_catalog_and_source_refuse_before_retention() {
    for catalog in [true, false] {
        let (dir, mut session) = fixture();
        let mut records = Records::default();
        session
            .dispatch(paint(&session, 6, 4), &mut records)
            .unwrap();
        let path = if catalog {
            dir.path().join("preview.jsonc")
        } else {
            dir.path().join("planes/main/layers/0/0_0.jsonc")
        };
        let changed = format!(
            "// changed externally\n{}",
            fs::read_to_string(&path).unwrap()
        );
        fs::write(&path, &changed).unwrap();
        assert!(session.dispatch(Action::Accept, &mut records).is_err());
        assert!(records.inverses.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), changed);
    }
}
#[test]
fn retention_failure_keeps_reviewable_pending_and_source() {
    let (dir, mut session) = fixture();
    let revision = session.history().document().revision();
    let mut records = Records {
        fail: true,
        ..Records::default()
    };
    session
        .dispatch(paint(&session, 6, 4), &mut records)
        .unwrap();
    let pending = pending_snapshot(&session);
    assert!(session.dispatch(Action::Accept, &mut records).is_err());
    assert_eq!(session.status().phase, SessionPhase::Pending);
    assert!(session.status().message.contains("retention failure"));
    assert!(session.history().pending().is_some());
    assert_eq!(pending_snapshot(&session), pending);
    assert!(!session.can_prepare());
    assert_eq!(
        WorldDocument::open(dir.path()).unwrap().revision(),
        revision
    );
    assert!(records.inverses.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn semantic_selection_survives_accept_undo_and_redo_with_original_revision_fences() {
    for key in [object_key(), cell_key()] {
        let (_dir, mut session) = fixture();
        let mut records = Records::default();
        select(&mut session, key.clone(), &mut records);
        let stale_draft = movement(&session, 5);
        session
            .dispatch(movement(&session, 4), &mut records)
            .unwrap();
        let stale_frame = session.generation();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(session.selection(), Some(&key));
        assert!(session.can_prepare());
        assert!(
            session
                .dispatch(stale_draft, &mut records)
                .unwrap_err()
                .to_string()
                .contains("stale source revision")
        );
        assert!(
            session
                .dispatch(
                    Action::Select {
                        generation: stale_frame,
                        key: key.clone(),
                    },
                    &mut records
                )
                .is_err()
        );
        assert_eq!(session.selection(), Some(&key));
        assert!(session.history().pending().is_none());
        for action in [Action::Undo, Action::Redo] {
            session.dispatch(action, &mut records).unwrap();
            assert_eq!(session.selection(), Some(&key));
            let pending = pending_snapshot(&session);
            assert!(
                session
                    .dispatch(movement(&session, 5), &mut records)
                    .is_err()
            );
            assert_eq!(pending_snapshot(&session), pending);
            session.dispatch(Action::Accept, &mut records).unwrap();
            assert_eq!(session.selection(), Some(&key));
            assert!(session.can_prepare());
        }
        assert_eq!(records.inverses.len(), 3);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn accepting_history_that_removes_selected_uid_clears_invalid_identity() {
    let (dir, _session) = fixture();
    let document = WorldDocument::open(dir.path()).unwrap();
    let mut history = History::new(document);
    let mut records = Records::default();
    // History can contain toolkit operations beyond the UI's current edit
    // capabilities. Selection must follow the resulting authoritative World.
    let transaction = serde_json::json!({
        "format": "tilewright.transaction/1", "title": "insert selection fixture",
        "actor": "editor:test", "operations": [{
            "op": "object.insert", "plane": "main", "layer": 0, "at": [4, 2],
            "object": {"uid": "inserted_marker", "proto": "marker", "fields": {"label": "added"}}
        }]
    });
    history
        .prepare(
            history.document().revision(),
            "selection fixture",
            &transaction.to_string(),
        )
        .unwrap();
    history
        .commit(&mut records, &tilewright_edit::PublishOptions::default())
        .unwrap();
    let (catalog, resources) = loader::load(
        history.document().source(),
        history.document().world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(history, catalog, resources);
    select(
        &mut session,
        SelectionKey::Object {
            uid: "inserted_marker".into(),
        },
        &mut records,
    );
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(session.selection().is_none());
    assert!(session.world().object("inserted_marker").is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn accept_undo_redo_reopen_preserve_exact_world_bytes_and_scene() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let moved = fs::read(&chunk).unwrap();
    assert_ne!(moved, original);
    assert_eq!(session.history().depths(), (1, 0));
    assert_eq!(session.status().phase, SessionPhase::Ready);
    assert_eq!(session.status().applied, Some(true));
    assert_eq!(session.status().durable, Some(true));
    session.dispatch(Action::Undo, &mut records).unwrap();
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([3, 2]));
    assert_eq!(fs::read(&chunk).unwrap(), moved);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), moved);
    session.dispatch(Action::Reopen, &mut records).unwrap();
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([4, 2]));
    assert_eq!(records.inverses.len(), 3);
}

struct ConcurrentEdit {
    chunk: std::path::PathBuf,
    inverse: Option<String>,
}
impl RecoveryStore for ConcurrentEdit {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.inverse = Some(inverse.into());
        let bytes = fs::read_to_string(&self.chunk).unwrap();
        fs::write(
            &self.chunk,
            format!("// concurrent writer after retention\n{bytes}"),
        )
        .unwrap();
        Ok(())
    }
}
#[test]
fn publication_failure_exposes_outcome_and_blocks_until_explicit_inspection() {
    let (dir, mut session) = fixture();
    let mut recovery = ConcurrentEdit {
        chunk: dir.path().join("planes/main/layers/0/0_0.jsonc"),
        inverse: None,
    };
    select(&mut session, object_key(), &mut recovery);
    session.set_recovery_descriptor("private recovery record".into());
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    let exact_inverse = session.history().pending().unwrap().inverse().to_owned();
    assert!(session.dispatch(Action::Accept, &mut recovery).is_err());
    assert_eq!(recovery.inverse, Some(exact_inverse));
    assert_eq!(session.status().phase, SessionPhase::Blocked);
    assert!(session.status().applied.is_some());
    assert!(session.status().durable.is_some());
    assert!(session.status().cleaned.is_some());
    assert!(session.status().to_string().contains("applied="));
    assert!(!session.can_prepare());
    let pending = pending_snapshot(&session);
    let bytes = fs::read(&recovery.chunk).unwrap();
    let outcome = (
        session.status().applied,
        session.status().durable,
        session.status().cleaned,
    );
    for action in [
        movement(&session, 5),
        paint(&session, 6, 4),
        Action::Accept,
        Action::Cancel,
        Action::Undo,
        Action::Redo,
        Action::ClearSelection,
        Action::Reopen,
    ] {
        assert!(session.dispatch(action, &mut recovery).is_err());
        assert_eq!(session.status().phase, SessionPhase::Blocked);
        assert_eq!(pending_snapshot(&session), pending);
        assert_eq!(session.history().depths(), (0, 0));
        assert_eq!(session.selection(), Some(&object_key()));
        assert_eq!(fs::read(&recovery.chunk).unwrap(), bytes);
        assert_eq!(
            session.status().recovery_descriptor.as_deref(),
            Some("private recovery record")
        );
        assert_eq!(
            (
                session.status().applied,
                session.status().durable,
                session.status().cleaned
            ),
            outcome
        );
    }
    assert_eq!(recovery.inverse.as_deref(), Some(pending.inverse.as_str()));
}

#[test]
fn semantic_field_encoding_rejects_large_or_deep_values_before_planning() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut deep = serde_json::json!(true);
    for _ in 0..10 {
        deep = serde_json::json!([deep]);
    }
    for value in [deep, serde_json::json!("x".repeat(65536))] {
        assert!(
            session
                .dispatch(
                    Action::SetField {
                        uid: "marker_1".into(),
                        field: "label".into(),
                        value,
                        expected: session.history().document().revision()
                    },
                    &mut records
                )
                .is_err()
        );
        assert!(session.history().pending().is_none());
    }
}

// The prototype deliberately uses checked publication on every platform. Windows
// cannot provide the released toolkit's checked-access guarantee and must refuse.
#[cfg(windows)]
#[test]
fn windows_checked_publication_refuses_without_false_success_or_source_changes() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let original_revision = session.history().document().revision();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    let inverse = session.history().pending().unwrap().inverse().to_owned();

    let error = session.dispatch(Action::Accept, &mut records).unwrap_err();
    assert!(error.to_string().contains("tx.io"), "{error}");
    assert!(error.to_string().contains("unchecked access"), "{error}");
    assert_eq!(session.status().phase, SessionPhase::Blocked);
    assert_eq!(session.status().applied, Some(false));
    assert_eq!(session.status().durable, Some(true));
    assert_eq!(session.status().cleaned, Some(true));
    assert!(
        !session
            .status()
            .message
            .contains("Published World workspace")
    );
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(
        session.history().document().reload().unwrap().revision(),
        original_revision
    );
    assert_eq!(session.history().document().revision(), original_revision);
    assert_eq!(session.history().depths(), (0, 0));
    assert!(session.history().needs_reload());
    assert_eq!(session.history().pending().unwrap().inverse(), inverse);
    // History retains recovery before asking the toolkit to publish. Refusal
    // preserves that exact record; it never acknowledges a successful edit.
    assert_eq!(records.inverses, [inverse]);
    for action in [Action::Accept, Action::Undo, Action::Redo, Action::Reopen] {
        assert!(session.dispatch(action, &mut records).is_err());
        assert_eq!(session.status().phase, SessionPhase::Blocked);
    }
    assert_eq!(records.inverses.len(), 1);
    assert_eq!(fs::read(chunk).unwrap(), original);
}

fn widen_view(root: &Path) {
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["view"]["bounds"] = serde_json::json!([-34, 0, 72, 6]);
    catalog["editing"]["movable_uids"] = serde_json::json!(["marker_1", "marker_negative"]);
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
}
#[cfg(target_os = "linux")]
fn object_order(session: &Session, cx: i64) -> Vec<String> {
    session.world().planes()["main"].chunks[&(0, cx, 0)]
        .objects
        .iter()
        .map(|object| object.uid.clone().unwrap())
        .collect()
}
#[cfg(target_os = "linux")]
#[test]
fn same_chunk_move_preserves_interleaved_object_order_and_only_coordinate_bytes() {
    let (dir, mut session) = fixture_with(|root| {
        let path = root.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut marker = chunk["objects"][0].clone();
        marker["custom"] =
            serde_json::json!({"example.org/editor":{"unknown":[1,true,"retained"]}});
        let mut before = marker.clone();
        before["uid"] = serde_json::json!("before_same");
        let mut after_other = marker.clone();
        after_other["uid"] = serde_json::json!("after_other");
        after_other["at"] = serde_json::json!([7, 4]);
        let mut after_same = marker.clone();
        after_same["uid"] = serde_json::json!("after_same");
        let mut destination = marker.clone();
        destination["uid"] = serde_json::json!("destination");
        destination["at"] = serde_json::json!([4, 2]);
        chunk["objects"] =
            serde_json::json!([before, marker, after_other, after_same, destination]);
        fs::write(
            path,
            format!(
                "// Keep all source comments and extension data\n{}",
                serde_json::to_string_pretty(&chunk).unwrap()
            ),
        )
        .unwrap();
    });
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&path).unwrap();
    let order = object_order(&session, 0);
    assert_eq!(
        session.world().object_location("marker_1").unwrap().path,
        [1]
    );
    let mut recovery = Records::default();
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    assert_eq!(object_order(&session, 0), order);
    let preview = session.history().pending().unwrap();
    let transaction: serde_json::Value = serde_json::from_str(preview.sealed()).unwrap();
    assert_eq!(transaction["operations"][0]["op"], "test");
    assert_eq!(
        transaction["operations"][1]["pointer"],
        serde_json::json!(["objects", 1, "uid"])
    );
    assert_eq!(transaction["operations"][2]["op"], "test");
    assert_eq!(transaction["operations"][3]["op"], "set");
    assert_eq!(
        transaction["operations"][3]["pointer"],
        serde_json::json!(["objects", 1, "at", 0])
    );
    assert!(
        transaction["base"]
            .as_array()
            .is_some_and(|base| !base.is_empty())
    );
    let before_objects =
        &session.history().document().world().planes()["main"].chunks[&(0, 0, 0)].objects;
    let mut expected_objects = before_objects.clone();
    expected_objects[1].at = Some([4, 2]);
    assert_eq!(
        session.world().planes()["main"].chunks[&(0, 0, 0)].objects,
        expected_objects
    );
    let changed = preview.changed().next().unwrap();
    let before = changed.before.as_ref().unwrap();
    let after = changed.after.as_ref().unwrap();
    let start = before.find("\"uid\": \"marker_1\"").unwrap();
    let at_start = before[..start]
        .rfind("[\n        3,\n        2\n      ]")
        .unwrap();
    let mut expected = before.clone();
    expected.replace_range(
        at_start..at_start + "[\n        3,\n        2\n      ]".len(),
        "[\n        4,\n        2\n      ]",
    );
    assert_eq!(after, &expected);
    assert_eq!(fs::read(&path).unwrap(), original);
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    let moved = fs::read(&path).unwrap();
    session.dispatch(Action::Undo, &mut recovery).unwrap();
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(object_order(&session, 0), order);
    session.dispatch(Action::Redo, &mut recovery).unwrap();
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(path).unwrap(), moved);
    assert_eq!(object_order(&session, 0), order);
}

#[test]
fn same_chunk_move_tests_uid_prototype_and_old_coordinates() {
    let (_dir, mut session) = fixture();
    let mut recovery = Records::default();
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    let sealed = session.history().pending().unwrap().sealed();
    for (index, field, wrong) in [
        (0, "value", serde_json::json!("grass")),
        (1, "value", serde_json::json!("missing_uid")),
        // Even the unchanged y axis is guarded by the whole old-at test.
        (2, "value", serde_json::json!([3, 1])),
        (3, "expect", serde_json::json!(0)),
    ] {
        let mut transaction: serde_json::Value = serde_json::from_str(sealed).unwrap();
        transaction["operations"][index][field] = wrong;
        let result = tilewright_editor_commands::preview(
            session.history().document(),
            "wrong-precondition",
            &transaction.to_string(),
        );
        let Err(tilewright_editor_commands::Error::Rejected(diagnostics)) = result else {
            panic!("incorrect move precondition was accepted")
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "tx.conflict")
        );
    }
}

#[cfg(target_os = "linux")]
fn commented_coordinates(x: &str, y: &str) -> String {
    format!(
        "[\n        /* x */ {x} /* keep x comment */,\n        // y stays on its own line\n        {y} /* keep y comment */\n      ]"
    )
}

#[cfg(target_os = "linux")]
#[test]
fn move_changes_only_scalar_tokens_preserving_coordinate_comments_and_spelling() {
    for (negative, change_x, change_y) in [
        (false, true, false),
        (false, false, true),
        (false, true, true),
        (false, false, false),
        (true, true, false),
        (true, false, true),
        (true, true, true),
        (true, false, false),
    ] {
        let old_x = if negative { "31" } else { "3" };
        let new_x = if change_x {
            if negative { "30" } else { "4" }
        } else {
            old_x
        };
        let old_at = commented_coordinates(old_x, "-0");
        let new_at = commented_coordinates(new_x, if change_y { "3" } else { "-0" });
        let relative = if negative {
            "planes/main/layers/0/-1_0.jsonc"
        } else {
            "planes/main/layers/0/0_0.jsonc"
        };
        let (dir, mut session) = fixture_with(|root| {
            if negative {
                widen_view(root);
            }
            let path = root.join(relative);
            let source = fs::read_to_string(&path).unwrap();
            let old_y = if negative { 1 } else { 2 };
            let original_at = format!("[\n        {old_x},\n        {old_y}\n      ]");
            assert_eq!(source.matches(&original_at).count(), 1);
            fs::write(path, source.replace(&original_at, &old_at)).unwrap();
        });
        let path = dir.path().join(relative);
        let original = fs::read_to_string(&path).unwrap();
        let expected = original.replace(&old_at, &new_at);
        let mut records = Records::default();
        session
            .dispatch(
                Action::Move {
                    uid: if negative {
                        "marker_negative"
                    } else {
                        "marker_1"
                    }
                    .into(),
                    x: new_x.parse::<i64>().unwrap() - if negative { 32 } else { 0 },
                    y: if change_y { 3 } else { 0 },
                    expected: session.history().document().revision(),
                },
                &mut records,
            )
            .unwrap();
        let pending = session.history().pending().unwrap();
        let transaction: serde_json::Value = serde_json::from_str(pending.sealed()).unwrap();
        let operations = transaction["operations"].as_array().unwrap();
        assert_eq!(
            operations.len(),
            3 + usize::from(change_x) + usize::from(change_y)
        );
        for operation in &operations[3..] {
            assert_eq!(operation["op"], "set");
            let axis = operation["pointer"][3].as_u64().unwrap();
            assert!((axis == 0 && change_x) || (axis == 1 && change_y));
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        if change_x || change_y {
            assert_eq!(
                pending.changed().next().unwrap().after.as_deref(),
                Some(expected.as_str())
            );
        } else {
            assert_eq!(pending.changed().count(), 0);
        }
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
        if change_x || change_y {
            session.dispatch(Action::Undo, &mut records).unwrap();
            session.dispatch(Action::Accept, &mut records).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            session.dispatch(Action::Redo, &mut records).unwrap();
            session.dispatch(Action::Accept, &mut records).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
            assert_eq!(records.inverses.len(), 3);
        } else {
            assert!(records.inverses.is_empty());
            assert_eq!(session.history().depths(), (0, 0));
        }
    }
}

#[test]
fn negative_coordinates_use_floor_chunks_and_cross_chunk_moves_use_relocation() {
    let (_dir, mut session) = fixture_with(widen_view);
    let mut recovery = Records::default();
    session
        .dispatch(
            Action::Move {
                uid: "marker_negative".into(),
                x: -2,
                y: 1,
                expected: session.history().document().revision(),
            },
            &mut recovery,
        )
        .unwrap();
    assert_eq!(
        session
            .world()
            .object_location("marker_negative")
            .unwrap()
            .chunk,
        (0, -1, 0)
    );
    assert_eq!(
        session.world().object("marker_negative").unwrap().at,
        Some([30, 1])
    );
    let transaction: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(
        transaction["operations"][2]["value"],
        serde_json::json!([31, 1])
    );
    assert_eq!(
        transaction["operations"][3]["pointer"],
        serde_json::json!(["objects", 0, "at", 0])
    );
    assert_eq!(transaction["operations"][3]["value"], serde_json::json!(30));
    session.dispatch(Action::Cancel, &mut recovery).unwrap();
    session
        .dispatch(movement(&session, -1), &mut recovery)
        .unwrap();
    let transaction: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(transaction["operations"][0]["op"], "object.move");
    assert_eq!(
        session.world().object_location("marker_1").unwrap().chunk,
        (0, -1, 0)
    );
    assert_eq!(
        session.world().object("marker_1").unwrap().at,
        Some([31, 2])
    );
}

#[test]
fn same_tile_move_preserves_coordinate_whitespace_without_publication_or_recovery() {
    let (dir, mut session) = fixture();
    let mut recovery = Records::default();
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&path).unwrap();
    session
        .dispatch(movement(&session, 3), &mut recovery)
        .unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 0);
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(recovery.inverses.is_empty());
    assert_eq!(session.history().depths(), (0, 0));
    assert_eq!(session.status().applied, Some(false));
}

#[test]
fn read_only_or_unknown_uids_cannot_prepare_coordinate_edits() {
    let (_dir, mut session) = fixture();
    let mut recovery = Records::default();
    for uid in ["marker_negative", "unknown_uid"] {
        let error = session
            .dispatch(
                Action::Move {
                    uid: uid.into(),
                    x: 4,
                    y: 2,
                    expected: session.history().document().revision(),
                },
                &mut recovery,
            )
            .unwrap_err();
        assert!(error.to_string().contains("movement capability"));
        assert!(session.history().pending().is_none());
    }
    assert!(recovery.inverses.is_empty());
}
