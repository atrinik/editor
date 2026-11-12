// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Object deletion: one reviewed `object.remove` per gesture, granted per
//! prototype by the optional `editing.objects` catalog key.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Session, SessionPhase};

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
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn without_object_grants(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("objects");
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
fn remove(session: &Session, uid: &str) -> Action {
    Action::Remove {
        uid: uid.into(),
        expected: session.history().document().revision(),
    }
}
fn movement(session: &Session, uid: &str, x: i64, y: i64) -> Action {
    Action::Move {
        uid: uid.into(),
        x,
        y,
        expected: session.history().document().revision(),
    }
}
fn label(session: &Session, uid: &str, field: &str, value: Value) -> Action {
    Action::SetField {
        uid: uid.into(),
        field: field.into(),
        value,
        expected: session.history().document().revision(),
    }
}
/// Dispatch, require a pending diff, then cancel it.
fn prepares(session: &mut Session, action: impl FnOnce(&Session) -> Action, records: &mut Records) {
    let action = action(session);
    session.dispatch(action, records).unwrap();
    assert!(session.history().pending().is_some());
    session.dispatch(Action::Cancel, records).unwrap();
}

#[test]
fn deletion_needs_a_prototype_grant_and_an_object_in_the_active_view() {
    let (dir, mut session) = fixture_with(without_object_grants);
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let mut records = Records::default();
    // Per-uid movement and field grants never imply deletion.
    let error = session
        .dispatch(remove(&session, "marker_1"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("deletion capability"), "{error}");
    assert!(session.history().pending().is_none());
    assert_eq!(session.status().phase, SessionPhase::Ready);

    let (_dir, mut session) = fixture();
    let error = session
        .dispatch(remove(&session, "unknown_uid"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("unknown UID"), "{error}");
    // Granted by prototype, but outside the catalog's view bounds.
    let error = session
        .dispatch(remove(&session, "marker_negative"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("active view"), "{error}");
    let error = session
        .dispatch(
            Action::Remove {
                uid: "marker_2".into(),
                expected: [7; 32],
            },
            &mut records,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("stale source revision"),
        "{error}"
    );
    assert!(session.history().pending().is_none());
    assert!(records.0.is_empty());
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
}

#[test]
fn prototype_grants_move_and_edit_fields_alongside_per_uid_grants() {
    let mut records = Records::default();
    let (_dir, mut session) = fixture();
    // `marker_2` has no per-uid grant; the `marker` entry grants it.
    prepares(
        &mut session,
        |s| movement(s, "marker_2", 2, 4),
        &mut records,
    );
    prepares(
        &mut session,
        |s| label(s, "marker_2", "label", json!("Moved")),
        &mut records,
    );
    // Fields outside the entry's list stay read-only.
    let error = session
        .dispatch(
            label(&session, "marker_2", "settings", json!({"active": true})),
            &mut records,
        )
        .unwrap_err();
    assert!(error.to_string().contains("field capability"), "{error}");

    let (_dir, mut session) = fixture_with(without_object_grants);
    for action in [
        movement(&session, "marker_2", 2, 4),
        label(&session, "marker_2", "label", json!("Moved")),
    ] {
        assert!(session.dispatch(action, &mut records).is_err());
        assert!(session.history().pending().is_none());
    }
    // The per-uid grants behave exactly as before.
    prepares(
        &mut session,
        |s| movement(s, "marker_1", 4, 2),
        &mut records,
    );
    prepares(
        &mut session,
        |s| label(s, "marker_1", "label", json!("Kept")),
        &mut records,
    );
    assert!(records.0.is_empty());
}

#[test]
fn one_deletion_is_one_reviewed_object_remove_with_byte_exact_undo_and_redo() {
    let (dir, mut session) = fixture();
    let path = dir.path().join(CHUNK);
    let original = fs::read(&path).unwrap();
    let mut records = Records::default();

    // Cancel writes nothing and retains nothing.
    prepares(&mut session, |s| remove(s, "marker_2"), &mut records);
    assert_eq!(fs::read(&path).unwrap(), original);

    session
        .dispatch(remove(&session, "marker_2"), &mut records)
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let pending = session.history().pending().unwrap();
    let sealed: Value = serde_json::from_str(pending.sealed()).unwrap();
    assert_eq!(
        sealed["operations"],
        json!([{"op": "object.remove", "uid": "marker_2", "expect": {"proto": "marker"}}])
    );
    assert_eq!(pending.changed().count(), 1);
    assert!(session.world().object("marker_2").is_none());
    assert_eq!(fs::read(&path).unwrap(), original);
    let warnings = session.pending_warnings().to_vec();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("marker_2") && warnings[0].contains("do not reuse"));

    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    session.dispatch(Action::Accept, &mut records).unwrap();
    let deleted = fs::read(&path).unwrap();
    assert_ne!(deleted, original);
    assert_eq!(records.0.len(), 1);
    assert!(session.world().object("marker_2").is_none());
    assert!(session.world().object("marker_1").is_some());
    assert_eq!(session.history().depths(), (1, 0));
    assert!(session.pending_warnings().is_empty());

    session.dispatch(Action::Undo, &mut records).unwrap();
    // Undo reinserts the object: nothing is deleted, so nothing is warned.
    assert!(session.pending_warnings().is_empty());
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(records.0.len(), 2);

    session.dispatch(Action::Redo, &mut records).unwrap();
    assert_eq!(session.pending_warnings().len(), 1);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&path).unwrap(), deleted);
    assert_eq!(records.0.len(), 3);
    assert_eq!(session.history().depths(), (1, 0));
}

#[test]
fn the_prototype_guard_and_revision_fence_refuse_a_changed_object() {
    let (dir, _session) = fixture();
    let document = WorldDocument::open(dir.path()).unwrap();
    let revision = document.revision();
    let mut history = History::new(document);
    let transaction = |proto: &str| {
        json!({"format": "tilewright.transaction/1", "title": "t", "actor": "test",
            "operations": [{"op": "object.remove", "uid": "marker_2", "expect": {"proto": proto}}]})
        .to_string()
    };
    assert!(
        history
            .prepare(revision, "test", &transaction("grass"))
            .is_err()
    );
    assert!(history.pending().is_none());
    assert!(
        history
            .prepare(revision, "test", &transaction("marker"))
            .is_ok()
    );

    // An external change of the object after the frame was observed is
    // refused before anything is planned.
    let (dir, mut session) = fixture();
    let action = remove(&session, "marker_2");
    let path = dir.path().join(CHUNK);
    let changed = fs::read_to_string(&path)
        .unwrap()
        .replace("Spare marker", "Spare marker, edited elsewhere");
    fs::write(&path, &changed).unwrap();
    let error = session
        .dispatch(action, &mut Records::default())
        .unwrap_err();
    assert!(
        error.to_string().contains("external source change"),
        "{error}"
    );
    assert!(session.history().pending().is_none());
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
}

#[test]
fn deleting_a_chunks_last_content_deletes_the_file_and_undo_restores_its_bytes() {
    let lone = "planes/main/layers/0/1_0.jsonc";
    let (dir, mut session) = fixture_with(|root| {
        let chunk = json!({"format": "tilewright.chunk/1", "plane": "main", "layer": 0,
            "chunk": [1, 0], "objects": [{"uid": "lone_1", "at": [0, 1], "proto": "marker", "layer": "item"}]});
        fs::write(
            root.join(lone),
            format!("{}\n", serde_json::to_string_pretty(&chunk).unwrap()),
        )
        .unwrap();
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([0, 0, 40, 6]);
        });
    });
    let path = dir.path().join(lone);
    let original = fs::read(&path).unwrap();
    let mut records = Records::default();
    session
        .dispatch(remove(&session, "lone_1"), &mut records)
        .unwrap();
    let pending = session.history().pending().unwrap();
    let change = pending.changed().next().unwrap();
    assert_eq!(change.path, lone);
    assert!(change.after.is_none());
    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(!path.exists());
    assert_eq!(records.0.len(), 1);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(records.0.len(), 2);
    assert!(session.world().object("lone_1").is_some());
}

#[test]
fn review_warns_about_inventory_portals_and_anchors_but_never_blocks_accept() {
    let (dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            let objects = chunk["objects"].as_array_mut().unwrap();
            objects[1]["inventory"] = json!([{"uid": "marker_2_item", "proto": "marker",
                "inventory": [{"proto": "marker"}]}]);
            // Targets (1, 4) in another plane: not this object's tile.
            objects.push(
                json!({"uid": "gate_3", "at": [7, 5], "proto": "marker", "layer": "item",
                "portal": {"plane": "other", "layer": 0, "at": [1, 4]}}),
            );
            // Targets its own tile, so deleting it warns about nothing.
            objects.push(
                json!({"uid": "loop_1", "at": [7, 4], "proto": "marker", "layer": "item",
                "portal": {"layer": 0, "at": [7, 4]}}),
            );
            objects.push(
                json!({"uid": "gate_1", "at": [5, 5], "proto": "marker", "layer": "item",
                "portal": {"layer": 0, "at": [1, 4]}}),
            );
            objects.push(
                json!({"uid": "gate_2", "at": [6, 5], "proto": "marker", "layer": "item",
                "portal": {"anchor": "landing"}}),
            );
        });
        fs::write(
            root.join("planes/main/anchors.jsonc"),
            json!({"format": "tilewright.anchors/1", "anchors": [
                {"id": "landing", "layer": 0, "at": [1, 4]},
                {"id": "elsewhere", "layer": 0, "at": [0, 0]},
                {"id": "upstairs", "layer": 1, "at": [1, 4]}]})
            .to_string(),
        )
        .unwrap();
        edit_json(root, "world.jsonc", |world| {
            world["planes"] = json!(["main", "other"]);
        });
        fs::create_dir_all(root.join("planes/other")).unwrap();
        fs::write(
            root.join("planes/other/plane.jsonc"),
            r#"{"format":"tilewright.plane/1","id":"other","kind":"shared","chunk_size":32}"#,
        )
        .unwrap();
    });
    let mut records = Records::default();
    session
        .dispatch(remove(&session, "marker_2"), &mut records)
        .unwrap();
    let warnings = session.pending_warnings().to_vec();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(
        warnings[0].contains("holds 2 inventory objects"),
        "{warnings:?}"
    );
    for part in ["anchor landing", "portal gate_1", "portal gate_2", "(1, 4)"] {
        assert!(warnings[1].contains(part), "{part}: {warnings:?}");
    }
    // Exactly three dependents: no anchor on another layer, no portal into
    // another plane.
    for part in ["elsewhere", "upstairs", "gate_3", "more"] {
        assert!(!warnings[1].contains(part), "{part}: {warnings:?}");
    }
    assert!(warnings[1].contains("will still target that tile"));
    // The object's uid and the uids in its inventory are removed.
    assert!(
        warnings[2].starts_with("Uids marker_2, marker_2_item are removed")
            && warnings[2].contains("do not reuse"),
        "{warnings:?}"
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(session.pending_warnings().is_empty());

    // A deleted portal never warns about its own target tile.
    session
        .dispatch(remove(&session, "loop_1"), &mut records)
        .unwrap();
    assert_eq!(
        session.pending_warnings(),
        ["Uid loop_1 is removed with the object; do not reuse it for other objects."]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();

    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    // Warnings inform; Accept publishes as usual.
    session
        .dispatch(remove(&session, "marker_2"), &mut records)
        .unwrap();
    assert_eq!(session.pending_warnings().len(), 3);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1);
    assert!(session.world().object("marker_2").is_none());
    assert!(session.world().object("gate_1").is_some());
    assert!(
        fs::read_to_string(dir.path().join(CHUNK))
            .unwrap()
            .contains("gate_2")
    );
}

/// An object on spatial layer 1 inside the view's x/y bounds, and an
/// inventory entry with a uid inside `marker_2`.
fn out_of_scope_objects(root: &Path) {
    let upper = json!({"format": "tilewright.chunk/1", "plane": "main", "layer": 1, "chunk": [0, 0],
        "objects": [{"uid": "upper_1", "at": [2, 2], "proto": "marker", "layer": "item",
            "fields": {"label": "Upper"}}]});
    fs::create_dir_all(root.join("planes/main/layers/1")).unwrap();
    fs::write(
        root.join("planes/main/layers/1/0_0.jsonc"),
        serde_json::to_vec_pretty(&upper).unwrap(),
    )
    .unwrap();
    edit_json(root, CHUNK, |chunk| {
        chunk["objects"][1]["inventory"] =
            json!([{"uid": "marker_2_item", "proto": "marker", "fields": {"label": "Inside"}}]);
    });
}

#[test]
fn prototype_grants_reach_only_top_level_objects_in_the_active_view() {
    let mut records = Records::default();
    let (_dir, mut session) = fixture_with(out_of_scope_objects);
    // Another spatial layer (inside the x/y bounds), an inventory entry, and
    // a top-level object outside the view bounds.
    for uid in ["upper_1", "marker_2_item", "marker_negative"] {
        let error = session
            .dispatch(label(&session, uid, "label", json!("No")), &mut records)
            .unwrap_err();
        assert!(
            error.to_string().contains("field capability"),
            "{uid}: {error}"
        );
        let error = session
            .dispatch(remove(&session, uid), &mut records)
            .unwrap_err();
        assert!(error.to_string().contains("active view"), "{uid}: {error}");
        assert!(session.history().pending().is_none());
    }
    for (uid, x, y) in [("upper_1", 2, 2), ("marker_negative", 4, 2)] {
        let error = session
            .dispatch(movement(&session, uid, x, y), &mut records)
            .unwrap_err();
        assert!(
            error.to_string().contains("movement capability"),
            "{uid}: {error}"
        );
        assert!(session.history().pending().is_none());
    }

    // Explicit per-uid grants keep the reach they had before the key.
    let (_dir, mut session) = fixture_with(|root| {
        out_of_scope_objects(root);
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["movable_uids"] = json!(["marker_1", "marker_negative"]);
            let fields = catalog["editing"]["fields"].as_array_mut().unwrap();
            for uid in ["upper_1", "marker_2_item", "marker_negative"] {
                fields.push(json!({"uid": uid, "field": "label"}));
            }
        });
    });
    for uid in ["upper_1", "marker_2_item", "marker_negative"] {
        prepares(
            &mut session,
            |s| label(s, uid, "label", json!("Yes")),
            &mut records,
        );
        // Deletion is never granted per uid.
        assert!(
            session
                .dispatch(remove(&session, uid), &mut records)
                .is_err()
        );
    }
    prepares(
        &mut session,
        |s| movement(s, "marker_negative", 4, 2),
        &mut records,
    );
    assert!(records.0.is_empty());
}

#[test]
fn inspector_capabilities_use_the_same_active_view_scope() {
    use tilewright_editor_ui::{
        SelectionKey,
        inspector::{SelectionSummary, inspect},
    };
    let summary = |session: &Session, uid: &str| {
        let key = SelectionKey::Object { uid: uid.into() };
        let view = inspect(
            session.world(),
            session.catalog(),
            Some(&key),
            session.generation(),
            session.history().document().revision(),
        );
        match view.selection {
            Some(SelectionSummary::Object {
                movable, deletable, ..
            }) => {
                let label = view
                    .rows
                    .iter()
                    .find(|row| row.name == "label")
                    .unwrap()
                    .editable;
                (movable, deletable, label)
            }
            other => panic!("{other:?}"),
        }
    };
    let (_dir, session) = fixture();
    assert_eq!(summary(&session, "marker_2"), (true, true, true));
    // Same plane and layer, but outside the view bounds.
    assert_eq!(summary(&session, "marker_negative"), (false, false, false));
    let (_dir, session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["movable_uids"] = json!(["marker_1", "marker_negative"]);
        });
    });
    assert_eq!(summary(&session, "marker_negative"), (true, false, false));
}
