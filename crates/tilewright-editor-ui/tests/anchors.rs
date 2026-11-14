// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Anchor editing through Session, History and the World undo check: create,
//! move and delete are each one reviewed transaction on
//! `planes/<p>/anchors.jsonc`, granted only by `editing.anchors.edit`,
//! limited to the active view and undone byte for byte. Anchor ids are
//! fixed, and an anchor a portal names anywhere in the world is not deleted.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Session, SessionPhase, anchors};

const ANCHORS: &str = "planes/main/anchors.jsonc";
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
        tilewright_editor_ui::Actor::new("editor:prototype").unwrap(),
    )
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
/// Parse, change and rewrite a JSON file, so the checkout's line endings do
/// not matter.
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn create(session: &Session, anchor: &str, x: i64, y: i64) -> Action {
    Action::CreateAnchor {
        plane: "main".into(),
        anchor: anchor.into(),
        x,
        y,
        expected: revision(session),
    }
}
fn relocate(session: &Session, anchor: &str, x: i64, y: i64) -> Action {
    Action::MoveAnchor {
        plane: "main".into(),
        anchor: anchor.into(),
        x,
        y,
        expected: revision(session),
    }
}
fn delete(session: &Session, anchor: &str) -> Action {
    Action::DeleteAnchor {
        plane: "main".into(),
        anchor: anchor.into(),
        expected: revision(session),
    }
}
fn operations(session: &Session) -> Vec<Value> {
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    sealed["operations"].as_array().unwrap().clone()
}
fn anchor(session: &Session, id: &str) -> Option<tilewright_source::Anchor> {
    session.world().planes()["main"].anchors.get(id).copied()
}
/// Build the action first, then borrow the session mutably.
macro_rules! refused {
    ($session:expr, $action:expr, $reason:expr $(,)?) => {{
        let action = $action;
        refused_now($session, action, $reason)
    }};
}
#[cfg(target_os = "linux")]
macro_rules! accept_undo_redo {
    ($root:expr, $session:expr, $action:expr, $files:expr $(,)?) => {{
        let action = $action;
        accept_undo_redo_now($root, $session, action, $files)
    }};
}

/// Dispatch `action` and expect a refusal containing `reason`, with nothing
/// pending, the session ready and no recovery record.
fn refused_now(session: &mut Session, action: Action, reason: &str) -> String {
    let mut records = Records::default();
    let error = session
        .dispatch(action, &mut records)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "{error}");
    assert!(session.history().pending().is_none(), "{error}");
    assert_eq!(session.status().phase, SessionPhase::Ready, "{error}");
    assert!(records.0.is_empty());
    error
}

#[cfg(target_os = "linux")]
/// Accept, undo and redo `action`; every step publishes exactly one
/// recovery record, the result loads as a valid World and each undo
/// restores `files` byte for byte (an absent file reads as `None`).
fn accept_undo_redo_now(
    root: &Path,
    session: &mut Session,
    action: Action,
    files: &[&str],
) -> Vec<Option<Vec<u8>>> {
    let mut records = Records::default();
    let read = || -> Vec<Option<Vec<u8>>> {
        files
            .iter()
            .map(|file| fs::read(root.join(file)).ok())
            .collect()
    };
    let original = read();
    session.dispatch(action, &mut records).unwrap();
    assert!(
        session.history().pending().is_some(),
        "{}",
        session.status()
    );
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1, "one change retains one inverse");
    let changed = read();
    assert_ne!(changed, original);
    // The result is a valid World: a fresh load validates every file.
    tilewright_source::load(root).unwrap();
    open(root);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 2);
    assert_eq!(read(), original, "undo is byte-exact");
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(read(), changed, "redo is byte-exact");
    assert_eq!(records.0.len(), 3);
    changed
}

#[test]
fn the_sample_resolves_anchors_portals_and_their_references() {
    let (_dir, session) = fixture();
    let world = session.world();
    let catalog = session.catalog();
    // `arrival` is in view; `lookout` is on the view's layer, left of it.
    assert_eq!(anchors::view_anchors(world, catalog), [("arrival", [7, 3])]);
    let counts = anchors::reference_counts(world, "main");
    assert_eq!(counts.get("arrival"), Some(&1));
    assert_eq!(counts.get("lookout"), None);
    let holders = anchors::references(world, "main", "arrival");
    assert_eq!(holders.len(), 1);
    assert_eq!(
        (holders[0].uid, holders[0].tile, holders[0].depth),
        (Some("gate_east"), Some([7, 0]), 0)
    );
    // Both gates hold a portal; one targets a location out of view.
    assert_eq!(
        anchors::view_portal_tiles(world, catalog)
            .into_iter()
            .collect::<Vec<_>>(),
        [[0, 3], [7, 0]]
    );
    let west = anchors::portals(world)
        .into_iter()
        .find(|holder| holder.uid == Some("gate_west"))
        .unwrap();
    assert_eq!(
        west.target,
        anchors::Target::Location {
            plane: "main",
            layer: 0,
            at: [-2, 2]
        }
    );
    assert_eq!(
        anchors::describe_target(world, &west.target),
        "main layer 0 (-2, 2)"
    );
    assert_eq!(
        anchors::describe_target(world, &holders[0].target),
        "anchor arrival = main layer 0 (7, 3)"
    );
}

#[test]
fn editing_needs_the_anchor_grant_and_its_absence_changes_nothing_else() {
    for grant in [Some(json!({"edit": false})), None] {
        let (_dir, mut session) = fixture_with(|root| {
            edit_json(root, "preview.jsonc", |catalog| match &grant {
                Some(grant) => catalog["editing"]["anchors"] = grant.clone(),
                None => {
                    catalog["editing"]
                        .as_object_mut()
                        .unwrap()
                        .remove("anchors");
                }
            });
        });
        for action in [
            create(&session, "camp", 2, 3),
            relocate(&session, "arrival", 6, 3),
            delete(&session, "lookout"),
        ] {
            refused!(&mut session, action, "editing.anchors.edit");
        }
        // Zone management still works as the catalog grants it.
        session
            .dispatch(
                Action::RenameZone {
                    plane: "main".into(),
                    zone: "meadow".into(),
                    name: Some("Open meadow".into()),
                    expected: revision(&session),
                },
                &mut Records::default(),
            )
            .unwrap();
    }
    let (_dir, mut session) = fixture();
    assert!(session.catalog().editing().anchors.edit);
    session
        .dispatch(create(&session, "camp", 2, 3), &mut Records::default())
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
}

#[test]
fn a_pending_change_fences_anchor_edits_and_a_stale_revision_is_refused() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    session
        .dispatch(create(&session, "camp", 2, 3), &mut records)
        .unwrap();
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    for action in [
        create(&session, "other", 2, 2),
        relocate(&session, "arrival", 6, 3),
        delete(&session, "lookout"),
    ] {
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert!(error.to_string().contains("Accept or Cancel"), "{error}");
        assert_eq!(session.history().pending().unwrap().sealed(), sealed);
    }
    session.dispatch(Action::Cancel, &mut records).unwrap();
    for mut stale in [
        create(&session, "camp", 2, 3),
        relocate(&session, "arrival", 6, 3),
        delete(&session, "lookout"),
    ] {
        if let Action::CreateAnchor { expected, .. }
        | Action::MoveAnchor { expected, .. }
        | Action::DeleteAnchor { expected, .. } = &mut stale
        {
            *expected = [0; 32];
        }
        refused!(&mut session, stale, "stale source revision");
    }
}

#[test]
fn ids_targets_and_planes_are_checked_before_world_sees_them() {
    let (_dir, mut session) = fixture();
    let long = "c".repeat(129);
    for id in ["", "Camp", "-camp", "camp site", long.as_str()] {
        refused!(
            &mut session,
            create(&session, id, 2, 3),
            "invalid anchor id"
        );
    }
    refused!(
        &mut session,
        create(&session, "arrival", 2, 3),
        "already used in this plane"
    );
    // An anchor out of view still holds its id.
    refused!(
        &mut session,
        create(&session, "lookout", 2, 3),
        "already used"
    );
    // Outside the view bounds `[0, 0, 8, 6]`.
    for (x, y) in [(8, 0), (-1, 0), (0, 6), (0, -1)] {
        refused!(
            &mut session,
            create(&session, "camp", x, y),
            "tiles of the active view"
        );
        refused!(
            &mut session,
            relocate(&session, "arrival", x, y),
            "tiles of the active view"
        );
    }
    // Another plane than the view's.
    for action in [
        Action::CreateAnchor {
            plane: "other".into(),
            anchor: "camp".into(),
            x: 2,
            y: 3,
            expected: revision(&session),
        },
        Action::MoveAnchor {
            plane: "other".into(),
            anchor: "arrival".into(),
            x: 2,
            y: 3,
            expected: revision(&session),
        },
        Action::DeleteAnchor {
            plane: "other".into(),
            anchor: "lookout".into(),
            expected: revision(&session),
        },
    ] {
        refused!(&mut session, action, "active view's plane");
    }
    // An anchor out of view is listed, but not moved by tile.
    refused!(
        &mut session,
        relocate(&session, "lookout", 2, 3),
        "only an anchor on a tile of the active view"
    );
    refused!(
        &mut session,
        relocate(&session, "arrival", 7, 3),
        "already on this tile"
    );
    refused!(
        &mut session,
        relocate(&session, "nowhere", 2, 3),
        "unknown anchor"
    );
    refused!(&mut session, delete(&session, "nowhere"), "unknown anchor");
}

#[test]
fn an_external_edit_and_a_failed_expect_guard_prepare_nothing() {
    let (dir, mut session) = fixture();
    let path = dir.path().join(ANCHORS);
    let original = fs::read(&path).unwrap();
    edit_json(dir.path(), ANCHORS, |anchors| {
        anchors["anchors"][0]["at"] = json!([6, 3]);
    });
    refused!(
        &mut session,
        relocate(&session, "arrival", 5, 3),
        "external source change"
    );
    fs::write(&path, &original).unwrap();
    // Move: one `set` of `at` expecting the committed position.
    let mut records = Records::default();
    session
        .dispatch(relocate(&session, "arrival", 5, 3), &mut records)
        .unwrap();
    let operation = operations(&session)[0].clone();
    assert_eq!(operation["op"], "set");
    assert_eq!(
        operation["pointer"],
        json!(["anchors", {"id": "arrival"}, "at"])
    );
    assert_eq!(operation["expect"], json!([7, 3]));
    assert_eq!(operation["value"], json!([5, 3]));
    assert_eq!(operations(&session).len(), 1, "layer is unchanged");
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // The same operation with a guard that does not hold is refused by
    // World as a whole, with nothing pending.
    let mut conflicting = json!({
        "format": "tilewright.transaction/1",
        "title": "Editor semantic action",
        "actor": "editor:prototype",
        "operations": [operation],
    });
    conflicting["operations"][0]["expect"] = json!([6, 3]);
    let mut history = History::new(WorldDocument::open(dir.path()).unwrap());
    let base = history.document().revision();
    assert!(
        history
            .prepare(base, "editor:action", &conflicting.to_string())
            .is_err()
    );
    assert!(history.pending().is_none());
    // Delete: one `remove` expecting the whole committed entry.
    session
        .dispatch(delete(&session, "lookout"), &mut records)
        .unwrap();
    let operation = operations(&session)[0].clone();
    assert_eq!(operation["op"], "remove");
    assert_eq!(operation["pointer"], json!(["anchors", {"id": "lookout"}]));
    assert_eq!(
        operation["expect"],
        json!({"id": "lookout", "layer": 0, "at": [-2, 0]})
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Create: an absent test, then an insert at the end.
    session
        .dispatch(create(&session, "camp", 2, 3), &mut records)
        .unwrap();
    let ops = operations(&session);
    assert_eq!(
        ops[0],
        json!({"op": "test", "path": ANCHORS, "pointer": ["anchors", {"id": "camp"}], "absent": true})
    );
    assert_eq!(ops[1]["op"], "insert");
    assert_eq!(ops[1]["pointer"], json!(["anchors", 2]));
    assert_eq!(
        ops[1]["value"],
        json!({"id": "camp", "layer": 0, "at": [2, 3]})
    );
}

/// Another plane, `other`, whose chunk holds `objects` and whose anchors
/// are `anchors`.
fn with_other_plane(root: &Path, objects: &Value, anchors: &Value) {
    edit_json(root, "world.jsonc", |world| {
        world["planes"] = json!(["main", "other"]);
    });
    let directory = root.join("planes/other/layers/0");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        root.join("planes/other/plane.jsonc"),
        r#"{"format":"tilewright.plane/1","id":"other","kind":"shared","chunk_size":32}"#,
    )
    .unwrap();
    fs::write(
        root.join("planes/other/anchors.jsonc"),
        json!({"format": "tilewright.anchors/1", "anchors": anchors}).to_string(),
    )
    .unwrap();
    fs::write(
        directory.join("0_0.jsonc"),
        json!({"format": "tilewright.chunk/1", "plane": "other", "layer": 0,
            "chunk": [0, 0], "objects": objects})
        .to_string(),
    )
    .unwrap();
}

#[test]
fn delete_is_refused_for_every_kind_of_reference_and_names_the_objects() {
    // The fixture's own: a placed gate names `arrival`.
    let (_dir, mut session) = fixture();
    let error = refused!(
        &mut session,
        delete(&session, "arrival"),
        "named by portals and cannot be deleted"
    );
    assert!(
        error.contains("1 portal: object gate_east on main layer 0 (7, 0)"),
        "{error}"
    );
    assert!(anchor(&session, "arrival").is_some());

    // From another plane, which names the plane explicitly.
    let (_dir, mut session) = fixture_with(|root| {
        with_other_plane(
            root,
            &json!([{"uid": "far_gate", "at": [4, 5], "proto": "gate", "layer": "item",
                "portal": {"plane": "main", "anchor": "lookout"}}]),
            &json!([]),
        );
    });
    let error = refused!(
        &mut session,
        delete(&session, "lookout"),
        "cannot be deleted"
    );
    assert!(
        error.contains("object far_gate on other layer 0 (4, 5)"),
        "{error}"
    );

    // A portal without `plane` targets its own plane: an anchor of the same
    // id there is a different anchor, and does not hold this one.
    let (_dir, mut session) = fixture_with(|root| {
        with_other_plane(
            root,
            &json!([{"uid": "far_gate", "at": [4, 5], "proto": "gate", "layer": "item",
                "portal": {"anchor": "lookout"}}]),
            &json!([{"id": "lookout", "layer": 0, "at": [1, 1]}]),
        );
    });
    session
        .dispatch(delete(&session, "lookout"), &mut Records::default())
        .unwrap();
    session
        .dispatch(Action::Cancel, &mut Records::default())
        .unwrap();

    // From an inventory entry two levels down, and from objects without a
    // uid, placed or carried.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            let objects = chunk["objects"].as_array_mut().unwrap();
            objects[1]["inventory"] = json!([{"uid": "pouch", "proto": "marker",
                "inventory": [{"uid": "token", "proto": "marker",
                    "portal": {"anchor": "lookout"}}]}]);
            objects.push(json!({"at": [5, 1], "proto": "gate", "layer": "item",
                "portal": {"plane": "main", "anchor": "lookout"}}));
            objects.push(json!({"at": [5, 4], "proto": "gate", "layer": "item",
                "inventory": [{"proto": "marker", "portal": {"anchor": "lookout"}}]}));
            objects.push(json!({"uid": "far_post", "at": [6, 0], "proto": "gate",
                "layer": "item", "portal": {"anchor": "lookout"}}));
        });
    });
    let error = refused!(
        &mut session,
        delete(&session, "lookout"),
        "cannot be deleted"
    );
    assert!(error.contains("4 portals: "), "{error}");
    assert!(
        error.contains("object token on main layer 0 (1, 4), in an inventory"),
        "{error}"
    );
    assert!(
        error.contains("an object without a uid on main layer 0 (5, 1)"),
        "{error}"
    );
    assert!(
        error.contains("an object without a uid on main layer 0 (5, 4), in an inventory"),
        "{error}"
    );
    // Bounded: three named, the rest counted.
    assert!(error.ends_with("and 1 more"), "{error}");
    assert!(!error.contains("far_post"), "{error}");
}

#[cfg(target_os = "linux")]
#[test]
fn create_move_and_delete_publish_once_and_undo_byte_for_byte() {
    let (dir, mut session) = fixture();
    let root = dir.path().to_owned();
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        create(&session, "camp", 2, 3),
        &[ANCHORS]
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(
        text.contains("{\"id\": \"camp\", \"layer\": 0, \"at\": [2, 3]}"),
        "{text}"
    );
    assert_eq!(
        anchor(&session, "camp"),
        Some(tilewright_source::Anchor {
            layer: 0,
            at: [2, 3]
        })
    );
    // Moving an anchor a portal names: the review lists it and warns.
    let mut records = Records::default();
    session
        .dispatch(relocate(&session, "arrival", 6, 2), &mut records)
        .unwrap();
    let notes = session.pending_warnings().to_vec();
    assert_eq!(
        notes,
        [
            "Moves anchor arrival from layer 0 (7, 3) to layer 0 (6, 2).",
            "Portals naming it now lead to the new tile (1 portal: object gate_east on main layer 0 (7, 0)).",
            "Warning: scripts or saves outside the world may refer to the anchor id arrival; they will lead to the new tile too.",
        ]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        relocate(&session, "camp", 3, 4),
        &[ANCHORS]
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(text.contains("\"at\": [3, 4]"), "{text}");
    // The portal still names `arrival`; nothing in the chunk changes.
    let chunk = fs::read(root.join(CHUNK)).unwrap();
    accept_undo_redo!(
        &root,
        &mut session,
        relocate(&session, "arrival", 6, 2),
        &[ANCHORS]
    );
    assert_eq!(fs::read(root.join(CHUNK)).unwrap(), chunk);
    session
        .dispatch(delete(&session, "camp"), &mut records)
        .unwrap();
    assert!(
        session.pending_warnings().contains(
            &"Deletes anchor camp (layer 0, (3, 4)); no portal in the world names it.".to_owned()
        ),
        "{:?}",
        session.pending_warnings()
    );
    assert!(
        session
            .pending_warnings()
            .iter()
            .any(|line| line.starts_with(
                "Warning: scripts or saves outside the world may still use the anchor id camp"
            )),
        "{:?}",
        session.pending_warnings()
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    accept_undo_redo!(&root, &mut session, delete(&session, "camp"), &[ANCHORS]);
    assert!(anchor(&session, "camp").is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn a_plane_without_anchors_jsonc_gets_one_undo_removes_it_and_the_last_delete_leaves_an_empty_array()
 {
    let (dir, mut session) = fixture_with(|root| {
        fs::remove_file(root.join(ANCHORS)).unwrap();
        // The gate's portal names an anchor that no longer exists: drop it.
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"]
                .as_array_mut()
                .unwrap()
                .retain(|object| object["uid"] != "gate_east");
        });
    });
    let root = dir.path().to_owned();
    assert!(session.world().planes()["main"].anchors.is_empty());
    let mut records = Records::default();
    session
        .dispatch(create(&session, "first", 1, 1), &mut records)
        .unwrap();
    let ops = operations(&session);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["op"], "document.create");
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        create(&session, "first", 1, 1),
        &[ANCHORS]
    );
    // The helper's undo found the document absent again; it ends redone.
    assert!(changed[0].is_some());
    accept_undo_redo!(&root, &mut session, delete(&session, "first"), &[ANCHORS]);
    let value: Value = serde_json::from_slice(&fs::read(root.join(ANCHORS)).unwrap()).unwrap();
    assert_eq!(value["anchors"], json!([]));
    tilewright_source::load(&root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn crlf_comments_and_compact_spacing_survive_every_edit_byte_for_byte() {
    let text = "{\r\n  // The plane's anchors.\r\n  \"format\": \"tilewright.anchors/1\",\r\n  \"anchors\": [\r\n\t{\"id\":\"arrival\",\"layer\":0,\"at\":[7,3]}, // by the gate\r\n\t{ \"id\" : \"lookout\" , \"layer\" : 0 , \"at\" : [ -2 , 0 ] },\r\n\t{\"id\":\"spare\",\"layer\":0,\"at\":[1,1]} /* last */\r\n  ]\r\n}\r\n";
    let (dir, mut session) = fixture_with(|root| fs::write(root.join(ANCHORS), text).unwrap());
    let root = dir.path().to_owned();
    assert_eq!(fs::read_to_string(root.join(ANCHORS)).unwrap(), text);
    for action in [
        create(&session, "camp", 2, 3),
        relocate(&session, "spare", 4, 4),
        relocate(&session, "arrival", 6, 3),
        delete(&session, "spare"),
        delete(&session, "lookout"),
    ] {
        let changed = accept_undo_redo!(&root, &mut session, action, &[ANCHORS]);
        let after = String::from_utf8(changed[0].clone().unwrap()).unwrap();
        assert!(after.contains("// The plane's anchors.\r\n"), "{after}");
        // World's `remove` takes an entry with the separator after it, so a
        // comment there goes with it; the rest of the file stays.
        assert!(after.contains("\t{\"id\":\"arrival\""), "{after}");
        let mut records = Records::default();
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read_to_string(root.join(ANCHORS)).unwrap(), text);
    }
}

#[test]
fn a_carried_portal_badges_its_holders_tile() {
    let (_dir, session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            // `marker_2` at (1, 4) carries an entry that carries a portal.
            chunk["objects"][1]["inventory"] = json!([{"proto": "marker",
                "inventory": [{"uid": "token", "proto": "marker",
                    "portal": {"anchor": "lookout"}}]}]);
        });
    });
    let tiles = anchors::view_portal_tiles(session.world(), session.catalog());
    assert!(tiles.contains(&[1, 4]), "{tiles:?}");
    assert_eq!(tiles.len(), 3);
}

#[test]
fn tile_anchors_and_target_tiles_resolve_as_the_world_holds_them() {
    let document = WorldDocument::open(&fixture_world("prototype-neutral")).unwrap();
    let world = document.world();
    let plane = &world.planes()["main"];
    assert_eq!(anchors::tile_anchors(plane, 0, [7, 3]), ["arrival"]);
    assert!(anchors::tile_anchors(plane, 1, [7, 3]).is_empty());
    assert!(anchors::tile_anchors(plane, 0, [7, 2]).is_empty());
    let targets: Vec<_> = anchors::portals(world)
        .iter()
        .map(|holder| anchors::target_tile(world, &holder.target))
        .collect();
    assert_eq!(
        targets,
        [Some(("main", 0, [7, 3])), Some(("main", 0, [-2, 2]))]
    );
    let missing = anchors::Target::Anchor {
        plane: "main",
        anchor: "nowhere",
    };
    assert_eq!(anchors::target_tile(world, &missing), None);
}
