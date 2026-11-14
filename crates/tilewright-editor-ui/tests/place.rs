// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Object placement: one reviewed `object.insert` per click, granted per
//! prototype by `editing.objects`, with a new uid from an injectable source.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey, Session, SessionPhase,
    tools::{ToolMode, ToolState, object_palette},
    uid::{SeededUidSource, UidError, UidSource, format_uid},
};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
const SEED: u64 = 42;

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
    let mut session = Session::new(
        History::new(document),
        catalog,
        resources,
        tilewright_editor_ui::Actor::new("editor:prototype").unwrap(),
    );
    session.set_uid_source(Box::new(SeededUidSource::new(SEED)));
    (dir, session)
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
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn place_on(session: &Session, plane: &str, layer: i32, x: i64, y: i64, proto: &str) -> Action {
    Action::Place {
        plane: plane.into(),
        layer,
        x,
        y,
        proto: proto.into(),
        expected: session.history().document().revision(),
    }
}
fn place(session: &Session, x: i64, y: i64, proto: &str) -> Action {
    place_on(session, "main", 0, x, y, proto)
}
/// The uids a fresh `SeededUidSource::new(SEED)` gives, in order.
fn seeded(count: usize) -> Vec<String> {
    let mut source = SeededUidSource::new(SEED);
    (0..count)
        .map(|_| {
            let mut bytes = [0; 16];
            source.fill(&mut bytes).unwrap();
            format_uid(bytes)
        })
        .collect()
}

#[test]
fn placement_needs_a_grant_and_a_tile_of_the_active_view() {
    let (dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("objects");
        });
    });
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let mut records = Records::default();
    // A catalog without `editing.objects` places nothing, and offers nothing.
    assert!(object_palette(session.catalog()).is_empty());
    let error = session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap_err();
    assert!(
        error.to_string().contains("placement capability"),
        "{error}"
    );

    let (_dir, mut session) = fixture();
    assert_eq!(
        object_palette(session.catalog()),
        ["bush", "crate", "gate", "lamp", "marker", "stone"]
    );
    // Prototypes without a grant, mapped or not, are refused.
    for proto in ["grass", "unknown"] {
        let error = session
            .dispatch(place(&session, 6, 1, proto), &mut records)
            .unwrap_err();
        assert!(
            error.to_string().contains("placement capability"),
            "{proto}: {error}"
        );
    }
    // The fixture view is [0, 0, 8, 6]: x 0..=7 and y 0..=5. Another plane,
    // another (unknown) spatial layer, the tiles exactly on the upper edges
    // and the tiles just below the lower edges are all outside it.
    for (plane, layer, x, y) in [
        ("other", 0, 1, 1),
        ("main", 1, 1, 1),
        ("main", -1, 1, 1),
        ("main", 0, 8, 1),
        ("main", 0, 1, 6),
        ("main", 0, 8, 6),
        ("main", 0, -1, 1),
        ("main", 0, 1, -1),
    ] {
        let error = session
            .dispatch(
                place_on(&session, plane, layer, x, y, "marker"),
                &mut records,
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("active view"),
            "{plane} {layer} {x} {y}: {error}"
        );
        assert!(session.history().pending().is_none());
    }
    let error = session
        .dispatch(
            Action::Place {
                plane: "main".into(),
                layer: 0,
                x: 6,
                y: 1,
                proto: "marker".into(),
                expected: [7; 32],
            },
            &mut records,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("stale source revision"),
        "{error}"
    );
    // The last tiles inside the view are placeable.
    for (x, y) in [(7, 5), (0, 0), (7, 0), (0, 5)] {
        session
            .dispatch(place(&session, x, y, "marker"), &mut records)
            .unwrap();
        assert!(session.history().pending().is_some());
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    assert!(records.0.is_empty());
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
}

#[test]
fn one_click_is_one_object_insert_with_explicit_layer_uid_and_defaults() {
    let (dir, mut session) = fixture();
    let path = dir.path().join(CHUNK);
    let original = fs::read(&path).unwrap();
    let mut records = Records::default();
    let uids = seeded(2);

    session
        .dispatch(place(&session, 6, 1, "stone"), &mut records)
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let pending = session.history().pending().unwrap();
    let sealed: Value = serde_json::from_str(pending.sealed()).unwrap();
    assert_eq!(
        sealed["operations"],
        json!([{"op": "object.insert", "plane": "main", "layer": 0, "at": [6, 1],
            "object": {"uid": uids[0], "proto": "stone", "layer": "item",
                "fields": {"label": "New stone", "settings": {"active": false, "visits": 0}}}}])
    );
    // No `index`: the object goes on top of the tile's stack.
    assert!(sealed["operations"][0].get("index").is_none());
    assert_eq!(pending.changed().count(), 1);
    // The new object's members keep the written order in source.
    let after = pending.changed().next().unwrap().after.clone().unwrap();
    let written = after.find(&uids[0]).unwrap();
    assert!(
        after[written..].find("\"proto\"") < after[written..].find("\"layer\"")
            && after[written..].find("\"layer\"") < after[written..].find("\"fields\""),
        "{after}"
    );
    assert!(session.world().object(&uids[0]).is_some());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        session.pending_warnings(),
        [format!(
            "Adds stone at tile (6, 1) of main layer 0, with uid {}.",
            uids[0]
        )]
    );

    // A pending placement fences every other preparation, Place included,
    // and Place is refused while another edit is pending.
    for action in [
        place(&session, 5, 1, "marker"),
        Action::Remove {
            uid: "marker_2".into(),
            expected: session.history().document().revision(),
        },
        Action::Undo,
    ] {
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert!(error.to_string().contains("review pending"), "{error}");
    }
    // Cancel writes nothing and retains nothing.
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(session.pending_warnings().is_empty());
    assert!(session.world().object(&uids[0]).is_none());
    assert_eq!(fs::read(&path).unwrap(), original);
    session
        .dispatch(
            Action::Remove {
                uid: "marker_2".into(),
                expected: session.history().document().revision(),
            },
            &mut records,
        )
        .unwrap();
    let error = session
        .dispatch(place(&session, 6, 1, "stone"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("review pending"), "{error}");
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(records.0.is_empty());

    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    // The cancelled placement used the first uid; this one draws the next.
    session
        .dispatch(place(&session, 6, 1, "stone"), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let placed = fs::read(&path).unwrap();
    assert_ne!(placed, original);
    assert_eq!(records.0.len(), 1);
    let object = session.world().object(&uids[1]).unwrap();
    assert_eq!(object.proto, "stone");
    assert_eq!(object.at, Some([6, 1]));
    assert_eq!(object.layer.as_deref(), Some("item"));
    // The accepted placement selects the new object.
    assert_eq!(
        session.selection(),
        Some(&SelectionKey::Object {
            uid: uids[1].clone()
        })
    );

    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(session.world().object(&uids[1]).is_none());
    assert_eq!(records.0.len(), 2);
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&path).unwrap(), placed);
    assert_eq!(records.0.len(), 3);
    assert_eq!(session.history().depths(), (1, 0));
}

/// Fixed uid material: first `colliding` times all zeros, then all ones.
struct Fixed {
    colliding: usize,
}
impl UidSource for Fixed {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), UidError> {
        *bytes = if self.colliding > 0 {
            self.colliding -= 1;
            [0; 16]
        } else {
            [1; 16]
        };
        Ok(())
    }
}
struct Failing;
impl UidSource for Failing {
    fn fill(&mut self, _bytes: &mut [u8; 16]) -> Result<(), UidError> {
        Err(UidError("no randomness here".into()))
    }
}

#[test]
fn uids_are_formatted_deterministic_under_injection_and_drawn_again_on_collision() {
    let first = seeded(1).remove(0);
    assert!(first.starts_with("o."));
    assert_eq!(first.len(), 28);
    assert!(tilewright_source::is_identifier(&first));
    assert!(
        first[2..]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
    );
    assert_eq!(seeded(3)[0], first);
    let distinct: std::collections::BTreeSet<_> = seeded(64).into_iter().collect();
    assert_eq!(distinct.len(), 64);

    // The all-zero uid already names an object in the World.
    let taken = format_uid([0; 16]);
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"].as_array_mut().unwrap().push(
                json!({"uid": format_uid([0; 16]), "at": [0, 0], "proto": "marker", "layer": "item"}),
            );
        });
    });
    let mut records = Records::default();
    session.set_uid_source(Box::new(Fixed { colliding: 3 }));
    session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap();
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(
        sealed["operations"][0]["object"]["uid"],
        json!(format_uid([1; 16]))
    );
    assert_ne!(sealed["operations"][0]["object"]["uid"], json!(taken));
    session.dispatch(Action::Cancel, &mut records).unwrap();

    // A bounded number of draws, then a plain refusal with nothing pending.
    session.set_uid_source(Box::new(Fixed { colliding: 1000 }));
    let error = session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("no unused uid"), "{error}");
    assert!(session.history().pending().is_none());
    session.set_uid_source(Box::new(Failing));
    let error = session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap_err();
    assert!(error.to_string().contains("no randomness"), "{error}");
    assert!(session.history().pending().is_none());
    assert_eq!(session.status().phase, SessionPhase::Ready);
}

#[test]
fn the_same_prototype_on_the_tile_warns_but_never_blocks_accept() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let uids = seeded(3);
    let uid = &uids[0];
    // marker_2 stands on (1, 4).
    session
        .dispatch(place(&session, 1, 4, "marker"), &mut records)
        .unwrap();
    assert_eq!(
        session.pending_warnings(),
        [
            format!("Adds marker at tile (1, 4) of main layer 0, with uid {uid}."),
            "Warning: 1 marker object is already on this tile; the new one is added on top."
                .to_owned(),
        ]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Another prototype on the same tile is no warning.
    session
        .dispatch(place(&session, 1, 4, "stone"), &mut records)
        .unwrap();
    assert_eq!(session.pending_warnings().len(), 1);
    session.dispatch(Action::Cancel, &mut records).unwrap();

    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    session
        .dispatch(place(&session, 1, 4, "marker"), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1);
    // Appended after marker_2: on top of the tile's stack. Two cancelled
    // placements drew the first two uids.
    let chunk = &session.world().planes()["main"].chunks[&(0, 0, 0)];
    let on_tile: Vec<_> = chunk
        .objects
        .iter()
        .filter(|object| object.at == Some([1, 4]))
        .filter_map(|object| object.uid.as_deref())
        .collect();
    assert_eq!(on_tile, ["marker_2", uids[2].as_str()]);
}

#[test]
fn placing_in_a_chunk_without_a_file_creates_it_and_undo_removes_it() {
    let new = "planes/main/layers/0/1_0.jsonc";
    let (dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([0, 0, 40, 6]);
        });
    });
    let path = dir.path().join(new);
    assert!(!path.exists());
    let mut records = Records::default();
    session
        .dispatch(place(&session, 33, 1, "marker"), &mut records)
        .unwrap();
    let pending = session.history().pending().unwrap();
    let change = pending.changed().next().unwrap();
    assert_eq!(change.path, new);
    assert!(change.before.is_none());
    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(path.exists());
    let uid = seeded(1).remove(0);
    assert_eq!(
        session.world().object_location(&uid).unwrap().chunk,
        (0, 1, 0)
    );
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(!path.exists());
    assert_eq!(records.0.len(), 2);
}

#[test]
fn world_refuses_defaults_that_break_the_profile_schema_with_a_clear_error() {
    // The catalog checks that `settings` is an object; World checks inside it.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["objects"][1]["defaults"] = json!({"settings": {"active": true}});
        });
    });
    let error = session
        .dispatch(place(&session, 6, 1, "stone"), &mut Records::default())
        .unwrap_err();
    assert!(
        error.to_string().starts_with("placement refused"),
        "{error}"
    );
    assert!(session.history().pending().is_none());
    assert_eq!(session.status().phase, SessionPhase::Ready);
}

#[test]
fn a_placed_object_is_selectable_movable_editable_and_deletable_through_its_grant() {
    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    let (dir, mut session) = fixture();
    let path = dir.path().join(CHUNK);
    let mut records = Records::default();
    let uid = seeded(1).remove(0);
    session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    // No per-uid grant names the random uid: the prototype entry grants it.
    let world = session.world();
    assert!(!session.catalog().editing().movable_uids.contains(&uid));
    assert!(session.catalog().can_move(world, &uid));
    assert!(session.catalog().can_edit_field(world, &uid, "label"));
    assert!(session.catalog().can_delete(world, &uid));
    session
        .dispatch(Action::ClearSelection, &mut records)
        .unwrap();
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: SelectionKey::Object { uid: uid.clone() },
            },
            &mut records,
        )
        .unwrap();

    let revision = |session: &Session| session.history().document().revision();
    session
        .dispatch(
            Action::Move {
                uid: uid.clone(),
                x: 6,
                y: 2,
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(session.world().object(&uid).unwrap().at, Some([6, 2]));
    session
        .dispatch(
            Action::SetField {
                uid: uid.clone(),
                field: "label".into(),
                value: json!("Placed"),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("Placed"));
    session
        .dispatch(
            Action::Remove {
                uid: uid.clone(),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .pending_warnings()
            .iter()
            .any(|line| line.contains(&uid) && line.contains("do not reuse"))
    );
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(session.world().object(&uid).is_none());
    assert_eq!(records.0.len(), 4);
    assert_eq!(session.history().depths(), (4, 0));
}

#[test]
fn the_place_tool_builds_one_action_from_a_picked_tile_and_a_granted_choice() {
    let (_dir, session) = fixture();
    let catalog = session.catalog();
    let revision = session.history().document().revision();
    let cell = SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: 6,
        y: 1,
    };
    let mut tools = ToolState::default();
    assert!(tools.choose_placement(catalog, "grass").is_err());
    tools.choose_placement(catalog, "stone").unwrap();
    // Only in Place mode, and never while an edit is pending.
    assert!(tools.place_action(catalog, &cell, revision, false).is_err());
    tools.set_mode(ToolMode::Place);
    assert_eq!(tools.placement_choice(), Some("stone"));
    assert!(tools.place_action(catalog, &cell, revision, true).is_err());
    let object = SelectionKey::Object {
        uid: "marker_2".into(),
    };
    assert!(
        tools
            .place_action(catalog, &object, revision, false)
            .is_err()
    );
    // A choice that is no longer in the palette (here: another catalog
    // without the `stone` entry) yields no action.
    let (_dir, without_stone) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["objects"]
                .as_array_mut()
                .unwrap()
                .remove(1);
        });
    });
    assert_eq!(
        object_palette(without_stone.catalog()),
        ["bush", "crate", "gate", "lamp", "marker"]
    );
    assert!(
        tools
            .place_action(without_stone.catalog(), &cell, revision, false)
            .is_err()
    );
    let Action::Place {
        plane,
        layer,
        x,
        y,
        proto,
        expected,
    } = tools.place_action(catalog, &cell, revision, false).unwrap()
    else {
        panic!("not a placement");
    };
    assert_eq!(
        (plane.as_str(), layer, x, y, proto.as_str(), expected),
        ("main", 0, 6, 1, "stone", revision)
    );
}

#[test]
fn a_profile_field_named_like_an_object_member_is_a_valid_default() {
    // Defaults go under the object's `fields`, so a profile field named
    // `layer` never clashes with the object's own render layer.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "profile/schema/object.jsonc", |schema| {
            schema["fields"]["layer"] = json!({"type": "string", "max_length": 16});
        });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["objects"][0]["defaults"] = json!({"layer": "top shelf"});
        });
    });
    assert_eq!(
        session.catalog().editing().objects["marker"].defaults,
        [("layer".to_owned(), r#""top shelf""#.to_owned())]
    );
    let mut records = Records::default();
    session
        .dispatch(place(&session, 6, 1, "marker"), &mut records)
        .unwrap();
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let object = &sealed["operations"][0]["object"];
    assert_eq!(object["layer"], json!("item"));
    assert_eq!(object["fields"], json!({"layer": "top shelf"}));
    let uid = seeded(1).remove(0);
    let placed = session.world().object(&uid).unwrap();
    assert_eq!(placed.layer.as_deref(), Some("item"));
    assert!(placed.fields.contains(&(
        "layer".into(),
        tilewright_source::FieldValue::String("top shelf".into())
    )));
}
