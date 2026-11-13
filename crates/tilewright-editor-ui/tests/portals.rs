// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Portal editing through Session, History and the World undo check: set,
//! change and clear are each one reviewed transaction on the object's
//! `portal` member, addressed by its uid, granted only by `portal: true` on
//! its prototype's `editing.objects` entry, limited to top-level objects of
//! the active view and undone byte for byte. The target may be anywhere.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, PortalTarget, Session, SessionPhase, anchors, portals};
use tilewright_source::Portal;

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
const WEST_CHUNK: &str = "planes/main/layers/0/-1_0.jsonc";

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
/// Parse, change and rewrite a JSON file, so the checkout's line endings do
/// not matter.
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
/// The `editing.objects` entry of `proto` in the catalog.
fn grant(catalog: &mut Value, proto: &str) -> usize {
    catalog["editing"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .position(|entry| entry["proto"] == proto)
        .unwrap()
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
fn to_anchor(session: &Session, object: &str, plane: &str, anchor: &str) -> Action {
    Action::SetPortal {
        uid: object.into(),
        target: Some(PortalTarget::Anchor {
            plane: plane.into(),
            anchor: anchor.into(),
        }),
        expected: revision(session),
    }
}
fn to_tile(session: &Session, object: &str, plane: &str, layer: i32, at: [i64; 2]) -> Action {
    Action::SetPortal {
        uid: object.into(),
        target: Some(PortalTarget::Location {
            plane: plane.into(),
            layer,
            at,
        }),
        expected: revision(session),
    }
}
fn clear(session: &Session, object: &str) -> Action {
    Action::SetPortal {
        uid: object.into(),
        target: None,
        expected: revision(session),
    }
}
fn operations(session: &Session) -> Vec<Value> {
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    sealed["operations"].as_array().unwrap().clone()
}
fn portal(session: &Session, object: &str) -> Option<Portal> {
    session.world().object(object).unwrap().portal.clone()
}
/// Build the action first, then borrow the session mutably.
macro_rules! refused {
    ($session:expr, $action:expr, $reason:expr $(,)?) => {{
        let action = $action;
        refused_now($session, action, $reason)
    }};
}
/// Build the action first, then borrow the session mutably.
macro_rules! pending {
    ($session:expr, $action:expr $(,)?) => {{
        let action = $action;
        pending_now($session, action)
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
fn pending_now(session: &mut Session, action: Action) {
    session.dispatch(action, &mut Records::default()).unwrap();
    assert_eq!(
        session.status().phase,
        SessionPhase::Pending,
        "{}",
        session.status()
    );
}
fn warnings(session: &Session) -> Vec<String> {
    session.pending_warnings().to_vec()
}

#[test]
fn editing_needs_the_portal_member_of_the_objects_grant() {
    // No member, `false`, no gate entry and no `editing.objects` at all.
    for change in [
        |catalog: &mut Value| {
            let index = grant(catalog, "gate");
            catalog["editing"]["objects"][index]
                .as_object_mut()
                .unwrap()
                .remove("portal");
        },
        |catalog: &mut Value| {
            let index = grant(catalog, "gate");
            catalog["editing"]["objects"][index]["portal"] = json!(false);
        },
        |catalog: &mut Value| {
            let index = grant(catalog, "gate");
            catalog["editing"]["objects"]
                .as_array_mut()
                .unwrap()
                .remove(index);
        },
        |catalog: &mut Value| {
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("objects");
        },
    ] {
        let (_dir, mut session) = fixture_with(|root| edit_json(root, "preview.jsonc", change));
        for action in [
            to_anchor(&session, "gate_east", "main", "lookout"),
            to_tile(&session, "gate_west", "main", 0, [2, 2]),
            clear(&session, "gate_east"),
        ] {
            refused!(&mut session, action, "editing.objects portal");
        }
    }
    // A marker's entry grants deletion and fields, not portals.
    let (_dir, mut session) = fixture();
    refused!(
        &mut session,
        to_anchor(&session, "marker_1", "main", "arrival"),
        "does not grant portal editing"
    );
    refused!(
        &mut session,
        to_anchor(&session, "no_such_object", "main", "arrival"),
        "unknown UID"
    );
    pending!(&mut session, to_anchor(&session, "gate_east", "main", "lookout"));
}

#[test]
fn only_top_level_objects_of_the_active_view_are_edited_but_targets_may_be_anywhere() {
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            let objects = chunk["objects"].as_array_mut().unwrap();
            // Out of the view's bounds, on another layer's chunk, and
            // carried by a marker.
            objects.push(json!({"uid": "far_gate", "at": [12, 1], "proto": "gate", "layer": "item"}));
            objects[0]["inventory"] = json!([{"uid": "carried_gate", "proto": "gate"}]);
        });
        let directory = root.join("planes/main/layers/1");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("0_0.jsonc"),
            json!({"format": "tilewright.chunk/1", "plane": "main", "layer": 1, "chunk": [0, 0],
                "objects": [{"uid": "upper_gate", "at": [2, 2], "proto": "gate", "layer": "item"}]})
            .to_string(),
        )
        .unwrap();
    });
    for object in ["far_gate", "upper_gate"] {
        refused!(
            &mut session,
            to_tile(&session, object, "main", 0, [1, 1]),
            "only placed objects in the active view"
        );
    }
    refused!(
        &mut session,
        to_tile(&session, "carried_gate", "main", 0, [1, 1]),
        "not objects carried in an inventory"
    );
    // A target out of view, on another layer, is allowed.
    pending!(&mut session, to_tile(&session, "gate_east", "main", 1, [2, 2]));
}

#[test]
fn a_pending_change_fences_portal_edits_and_a_stale_revision_is_refused() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    pending!(&mut session, to_anchor(&session, "gate_east", "main", "lookout"));
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    for action in [
        clear(&session, "gate_west"),
        to_tile(&session, "gate_west", "main", 0, [2, 2]),
    ] {
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert!(error.to_string().contains("Accept or Cancel"), "{error}");
        assert_eq!(session.history().pending().unwrap().sealed(), sealed);
    }
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let mut stale = clear(&session, "gate_west");
    if let Action::SetPortal { expected, .. } = &mut stale {
        *expected = [0; 32];
    }
    refused!(&mut session, stale, "stale source revision");
}

#[test]
fn planes_anchors_and_no_op_changes_are_refused_before_world_sees_them() {
    let (_dir, mut session) = fixture();
    refused!(
        &mut session,
        to_anchor(&session, "gate_east", "elsewhere", "arrival"),
        "unknown target plane"
    );
    refused!(
        &mut session,
        to_tile(&session, "gate_east", "elsewhere", 0, [1, 1]),
        "unknown target plane"
    );
    refused!(
        &mut session,
        to_anchor(&session, "gate_east", "main", "camp"),
        "unknown anchor"
    );
    refused!(
        &mut session,
        to_anchor(&session, "gate_east", "main", "arrival"),
        "already leads there"
    );
    refused!(
        &mut session,
        to_tile(&session, "gate_west", "main", 0, [-2, 2]),
        "already leads there"
    );
    // Nothing to clear on a gate without a portal.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"][2].as_object_mut().unwrap().remove("portal");
        });
    });
    refused!(&mut session, clear(&session, "gate_east"), "no portal to clear");
}

#[test]
fn each_edit_is_one_guarded_operation_on_the_portal_member() {
    let (dir, mut session) = fixture();
    let mut records = Records::default();
    // Change: one `set` expecting the committed portal; the own plane is
    // left out.
    pending!(&mut session, to_tile(&session, "gate_east", "main", 0, [3, 4]));
    assert_eq!(
        operations(&session),
        [json!({"op": "set", "object": {"uid": "gate_east"}, "pointer": ["portal"],
            "expect": {"anchor": "arrival"}, "value": {"layer": 0, "at": [3, 4]}})]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Clear: one `remove` expecting it.
    pending!(&mut session, clear(&session, "gate_west"));
    assert_eq!(
        operations(&session),
        [json!({"op": "remove", "object": {"uid": "gate_west"}, "pointer": ["portal"],
            "expect": {"layer": 0, "at": [-2, 2]}})]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Another plane is named explicitly.
    let (_other, mut session) = fixture_with(|root| with_other_plane(root, "far_anchor"));
    pending!(&mut session, to_anchor(&session, "gate_east", "other", "far_anchor"));
    assert_eq!(
        operations(&session)[0]["value"],
        json!({"plane": "other", "anchor": "far_anchor"})
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Add: an absent test, then a `set` that adds the member.
    let (_added, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"][2].as_object_mut().unwrap().remove("portal");
        });
    });
    pending!(&mut session, to_anchor(&session, "gate_east", "main", "lookout"));
    assert_eq!(
        operations(&session),
        [
            json!({"op": "test", "object": {"uid": "gate_east"}, "pointer": ["portal"], "absent": true}),
            json!({"op": "set", "object": {"uid": "gate_east"}, "pointer": ["portal"],
                "value": {"anchor": "lookout"}}),
        ]
    );
    // A guard that does not hold is refused by World as a whole.
    let mut conflicting = json!({
        "format": "tilewright.transaction/1",
        "title": "Editor semantic action",
        "actor": "editor:prototype",
        "operations": [{"op": "set", "object": {"uid": "gate_east"}, "pointer": ["portal"],
            "expect": {"anchor": "lookout"}, "value": {"layer": 0, "at": [1, 1]}}],
    });
    let mut history = History::new(WorldDocument::open(dir.path()).unwrap());
    let base = history.document().revision();
    assert!(
        history
            .prepare(base, "editor:action", &conflicting.to_string())
            .is_err()
    );
    assert!(history.pending().is_none());
    // A World refusal (here an anchor the target plane lacks, past the
    // editor's own check) surfaces with nothing pending.
    conflicting["operations"][0] = json!({"op": "set", "object": {"uid": "gate_east"},
        "pointer": ["portal"], "expect": {"anchor": "arrival"}, "value": {"anchor": "nowhere"}});
    assert!(
        history
            .prepare(base, "editor:action", &conflicting.to_string())
            .is_err()
    );
    assert!(history.pending().is_none());
}

/// Another plane, `other`, with one chunk and the anchor `anchor`.
fn with_other_plane(root: &Path, anchor: &str) {
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
        json!({"format": "tilewright.anchors/1",
            "anchors": [{"id": anchor, "layer": 0, "at": [1, 1]}]})
        .to_string(),
    )
    .unwrap();
    fs::write(
        directory.join("0_0.jsonc"),
        json!({"format": "tilewright.chunk/1", "plane": "other", "layer": 0,
            "chunk": [0, 0], "objects": []})
        .to_string(),
    )
    .unwrap();
}

#[cfg(target_os = "linux")]
/// Accept, undo and redo `action`; every step publishes exactly one
/// recovery record, the result loads as a valid World and undo and redo
/// are byte-exact. Returns the changed chunk text.
fn accept_undo_redo(root: &Path, session: &mut Session, action: Action) -> String {
    let mut records = Records::default();
    let read = || fs::read(root.join(CHUNK)).unwrap();
    let original = read();
    session.dispatch(action, &mut records).unwrap();
    assert!(session.history().pending().is_some(), "{}", session.status());
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1, "one change retains one inverse");
    let changed = read();
    assert_ne!(changed, original);
    tilewright_source::load(root).unwrap();
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 2);
    assert_eq!(read(), original, "undo is byte-exact");
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(read(), changed, "redo is byte-exact");
    assert_eq!(records.0.len(), 3);
    tilewright_source::load(root).unwrap();
    String::from_utf8(changed).unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn set_change_and_clear_publish_once_and_undo_byte_for_byte() {
    let (dir, mut session) = fixture();
    let root = dir.path().to_owned();
    let mut records = Records::default();
    // Change to another anchor, accepted: the old anchor is named by no
    // portal now, so its deletion is allowed.
    let action = to_anchor(&session, "gate_east", "main", "lookout");
    accept_undo_redo(&root, &mut session, action);
    assert_eq!(
        portal(&session, "gate_east"),
        Some(Portal::Anchor {
            plane: None,
            anchor: "lookout".into()
        })
    );
    // Clear, then set on the cleared gate: an absent test and an added
    // member.
    let action = clear(&session, "gate_west");
    accept_undo_redo(&root, &mut session, action);
    assert_eq!(portal(&session, "gate_west"), None);
    let action = to_tile(&session, "gate_west", "main", 0, [4, 3]);
    let text = accept_undo_redo(&root, &mut session, action);
    assert!(text.contains("\"portal\": {\"layer\": 0, \"at\": [4, 3]}"), "{text}");
    session
        .dispatch(
            Action::DeleteAnchor {
                plane: "main".into(),
                anchor: "arrival".into(),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Cancel, &mut records).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn crlf_comments_and_compact_spacing_survive_every_edit_byte_for_byte() {
    let text = "{\r\n  // A small chunk.\r\n  \"format\": \"tilewright.chunk/1\", \"plane\": \"main\", \"layer\": 0, \"chunk\": [0, 0],\r\n  \"objects\": [\r\n\t{\"uid\":\"marker_1\",\"at\":[3,2],\"proto\":\"marker\",\"layer\":\"item\"},\r\n\t{\"uid\":\"gate_east\",\"at\":[7,0],\"proto\":\"gate\",\"layer\":\"item\", // the east gate\r\n\t \"portal\":{\"anchor\":\"arrival\"} /* to the arrival */},\r\n\t{ \"uid\" : \"gate_west\" , \"at\" : [ 0 , 3 ] , \"proto\" : \"gate\" , \"layer\" : \"item\" }\r\n  ]\r\n}\r\n";
    let (dir, mut session) = fixture_with(|root| fs::write(root.join(CHUNK), text).unwrap());
    let root = dir.path().to_owned();
    // World removes a last member with the separator before it, and a
    // comment between them goes with it; the rest of the file stays.
    for (action, keeps) in [
        (to_anchor(&session, "gate_east", "main", "lookout"), true),
        (to_tile(&session, "gate_east", "main", 0, [-1, 1]), true),
        (clear(&session, "gate_east"), false),
        (to_anchor(&session, "gate_west", "main", "arrival"), true),
    ] {
        let changed = accept_undo_redo(&root, &mut session, action);
        assert!(changed.contains("// A small chunk.\r\n"), "{changed}");
        assert!(changed.contains("/* to the arrival */"), "{changed}");
        assert_eq!(changed.contains("// the east gate\r\n"), keeps, "{changed}");
        let mut records = Records::default();
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read_to_string(root.join(CHUNK)).unwrap(), text);
    }
}

#[test]
fn review_notes_describe_each_change_and_warn_without_blocking() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    pending!(&mut session, to_anchor(&session, "gate_west", "main", "arrival"));
    assert_eq!(
        warnings(&session),
        ["gate_west now leads to anchor arrival (main, layer 0, (7, 3)); it led to main, layer 0, (-2, 2)."]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    pending!(&mut session, clear(&session, "gate_east"));
    assert_eq!(
        warnings(&session),
        ["gate_east no longer has a portal; it led to anchor arrival (main, layer 0, (7, 3))."]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // A tile with no ground on the pick grid, a tile in a chunk that does
    // not exist, and the gate's own tile: warnings that never block Accept.
    assert_eq!(
        portals::pick_grid(session.world(), session.catalog()),
        Some("floor")
    );
    for (at, warning) in [
        ([6, 4], "has no ground on the floor grid"),
        ([40, 2], "in a map chunk that does not exist"),
        ([7, 0], "leads to the tile the object stands on"),
    ] {
        pending!(&mut session, to_tile(&session, "gate_east", "main", 0, at));
        let lines = warnings(&session);
        assert!(
            lines.iter().any(|line| line.starts_with("Warning: gate_east's portal")
                && line.contains(warning)),
            "{lines:?}"
        );
        #[cfg(target_os = "linux")]
        {
            session.dispatch(Action::Accept, &mut records).unwrap();
            assert_eq!(
                portal(&session, "gate_east"),
                Some(Portal::Location {
                    plane: None,
                    layer: 0,
                    at
                })
            );
            session.dispatch(Action::Undo, &mut records).unwrap();
            session.dispatch(Action::Accept, &mut records).unwrap();
        }
        #[cfg(not(target_os = "linux"))]
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    // A target with ground and a chunk: no warning. A tile on another
    // layer is checked in the loaded world and said to be out of sight.
    pending!(&mut session, to_tile(&session, "gate_east", "main", 0, [-1, 1]));
    assert_eq!(warnings(&session).len(), 1, "{:?}", warnings(&session));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    pending!(&mut session, to_tile(&session, "gate_east", "main", 3, [1, 1]));
    let lines = warnings(&session);
    assert!(lines.iter().any(|line| line.contains("map chunk that does not exist")), "{lines:?}");
    assert!(lines.iter().any(|line| line.contains("the view cannot show it")), "{lines:?}");
}

#[test]
fn anchor_reference_counts_and_deletion_follow_a_portal_change() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let counts = |session: &Session| {
        let counts = anchors::reference_counts(session.world(), "main");
        (counts.get("arrival").copied(), counts.get("lookout").copied())
    };
    assert_eq!(counts(&session), (Some(1), None));
    // The pending World is what the sidebar shows: the counts move with it.
    pending!(&mut session, to_anchor(&session, "gate_east", "main", "lookout"));
    assert_eq!(counts(&session), (None, Some(1)));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(counts(&session), (Some(1), None));
    #[cfg(target_os = "linux")]
    {
        let delete = |session: &Session| Action::DeleteAnchor {
            plane: "main".into(),
            anchor: "arrival".into(),
            expected: revision(session),
        };
        refused!(&mut session, delete(&session), "cannot be deleted");
        session
            .dispatch(to_anchor(&session, "gate_east", "main", "lookout"), &mut records)
            .unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(counts(&session), (None, Some(1)));
        // Retargeted away, `arrival` is deletable; `lookout` is not.
        pending!(&mut session, delete(&session));
        session.dispatch(Action::Cancel, &mut records).unwrap();
        refused!(
            &mut session,
            Action::DeleteAnchor {
                plane: "main".into(),
                anchor: "lookout".into(),
                expected: revision(&session),
            },
            "cannot be deleted"
        );
    }
}

#[test]
fn the_anchor_list_puts_the_view_plane_first_and_counts_every_anchor() {
    let (_dir, session) = fixture_with(|root| {
        with_other_plane(root, "aaa");
        edit_json(root, "world.jsonc", |world| {
            world["planes"] = json!(["other", "main"]);
        });
        edit_json(root, "planes/main/anchors.jsonc", |anchors| {
            let list = anchors["anchors"].as_array_mut().unwrap();
            for n in 0..300 {
                list.push(json!({"id": format!("z{n:03}"), "layer": 0, "at": [n, 9]}));
            }
        });
    });
    let (listed, total) = portals::target_anchors(session.world(), session.catalog());
    assert_eq!(total, 303);
    assert_eq!(listed.len(), portals::MAXIMUM_TARGET_ANCHORS);
    assert!(listed.iter().all(|anchor| anchor.plane == "main"), "the view's plane first");
    assert_eq!((listed[0].anchor, listed[1].anchor), ("arrival", "lookout"));
    let (listed, _) = {
        let (_dir, session) = fixture_with(|root| with_other_plane(root, "aaa"));
        let (listed, total) = portals::target_anchors(session.world(), session.catalog());
        let names: Vec<(String, String)> = listed
            .iter()
            .map(|anchor| (anchor.plane.to_owned(), anchor.anchor.to_owned()))
            .collect();
        (names, total)
    };
    assert_eq!(
        listed,
        [
            ("main".to_owned(), "arrival".to_owned()),
            ("main".to_owned(), "lookout".to_owned()),
            ("other".to_owned(), "aaa".to_owned()),
        ]
    );
}

#[test]
fn a_target_in_the_west_chunk_with_ground_is_not_warned() {
    // The fixture's own location target, (-2, 2), has ground in chunk
    // [-1, 0]: retargeting gate_east there warns about nothing.
    let (dir, mut session) = fixture();
    assert!(dir.path().join(WEST_CHUNK).is_file());
    pending!(&mut session, to_tile(&session, "gate_east", "main", 0, [-2, 2]));
    assert!(
        warnings(&session).iter().all(|line| !line.starts_with("Warning:")),
        "{:?}",
        warnings(&session)
    );
}
