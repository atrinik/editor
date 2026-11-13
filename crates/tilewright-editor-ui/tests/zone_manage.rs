// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Zone management through Session, History and the World undo check: a
//! display name, a parent, a new zone and the deletion of an unused zone are
//! each one reviewed transaction on `planes/<p>/zones.jsonc`, granted only by
//! `editing.zones.manage` and undone byte for byte. Zone ids are fixed.
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Session, SessionPhase, zones};

const ZONES: &str = "planes/main/zones.jsonc";

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
fn edit_catalog(root: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut catalog);
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
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn rename(session: &Session, zone: &str, name: Option<&str>) -> Action {
    Action::RenameZone {
        plane: "main".into(),
        zone: zone.into(),
        name: name.map(Into::into),
        expected: revision(session),
    }
}
fn reparent(session: &Session, zone: &str, parent: Option<&str>) -> Action {
    Action::SetZoneParent {
        plane: "main".into(),
        zone: zone.into(),
        parent: parent.map(Into::into),
        expected: revision(session),
    }
}
fn create(session: &Session, zone: &str, name: Option<&str>, parent: Option<&str>) -> Action {
    Action::CreateZone {
        plane: "main".into(),
        zone: zone.into(),
        name: name.map(Into::into),
        parent: parent.map(Into::into),
        expected: revision(session),
    }
}
fn delete(session: &Session, zone: &str) -> Action {
    Action::DeleteZone {
        plane: "main".into(),
        zone: zone.into(),
        expected: revision(session),
    }
}
fn operations(session: &Session) -> Vec<serde_json::Value> {
    let sealed: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    sealed["operations"].as_array().unwrap().clone()
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
/// pending and the session ready.
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
fn zone<'s>(session: &'s Session, id: &str) -> Option<&'s tilewright_source::Zone> {
    session.world().planes()["main"].zones.get(id)
}

#[test]
fn management_needs_the_manage_grant_and_paint_alone_is_not_enough() {
    for grant in [
        Some(serde_json::json!({"paint": true, "manage": false})),
        Some(serde_json::json!({"paint": false, "manage": false})),
        None,
    ] {
        let (_dir, mut session) = fixture_with(|root| {
            edit_catalog(root, |catalog| match &grant {
                Some(grant) => catalog["editing"]["zones"] = grant.clone(),
                None => {
                    catalog["editing"].as_object_mut().unwrap().remove("zones");
                }
            });
        });
        for action in [
            rename(&session, "meadow", Some("Open meadow")),
            reparent(&session, "path", Some("meadow")),
            create(&session, "grove", None, None),
            delete(&session, "meadow.pond"),
        ] {
            refused!(&mut session, action, "editing.zones.manage");
        }
    }
    // With the grant (the fixture's), the same rename prepares.
    let (_dir, mut session) = fixture();
    assert!(session.catalog().editing().zones.manage);
    session
        .dispatch(
            rename(&session, "meadow", Some("Open meadow")),
            &mut Records::default(),
        )
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
}

#[test]
fn a_pending_change_fences_management_and_a_stale_revision_is_refused() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    session
        .dispatch(
            rename(&session, "meadow", Some("Open meadow")),
            &mut records,
        )
        .unwrap();
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    for action in [
        rename(&session, "path", Some("Wide path")),
        reparent(&session, "path", Some("meadow")),
        create(&session, "grove", None, None),
        delete(&session, "meadow.pond"),
    ] {
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert!(error.to_string().contains("Accept or Cancel"), "{error}");
        // The reviewed diff is untouched.
        assert_eq!(session.history().pending().unwrap().sealed(), sealed);
    }
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // An author's revision from before another change is stale.
    let mut stale = rename(&session, "path", Some("Wide path"));
    if let Action::RenameZone { expected, .. } = &mut stale {
        *expected = [0; 32];
    }
    refused!(&mut session, stale, "stale source revision");
    // Only the active view's plane.
    refused!(
        &mut session,
        Action::RenameZone {
            plane: "other".into(),
            zone: "meadow".into(),
            name: Some("x".into()),
            expected: revision(&session),
        },
        "active view",
    );
}

#[test]
fn an_external_edit_and_a_failed_expect_guard_prepare_nothing() {
    let (dir, mut session) = fixture();
    // The source changed on disk since the session loaded it.
    let path = dir.path().join(ZONES);
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, original.replace("Quiet meadow", "Changed meadow")).unwrap();
    refused!(
        &mut session,
        rename(&session, "meadow", Some("Open meadow")),
        "external source change",
    );
    fs::write(&path, &original).unwrap();
    // The operation Session writes carries the committed name as `expect`;
    // the same operation with a guard that does not hold is refused by
    // World as a whole.
    let mut records = Records::default();
    session
        .dispatch(
            rename(&session, "meadow", Some("Open meadow")),
            &mut records,
        )
        .unwrap();
    let operation = operations(&session)[0].clone();
    assert_eq!(operation["op"], "set");
    assert_eq!(operation["expect"], "Quiet meadow");
    assert_eq!(
        operation["pointer"],
        serde_json::json!(["zones", {"id": "meadow"}, "name"])
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let mut conflicting = serde_json::json!({
        "format": "tilewright.transaction/1",
        "title": "Editor semantic action",
        "actor": "editor:prototype",
        "operations": [operation],
    });
    conflicting["operations"][0]["expect"] = serde_json::json!("Other name");
    let mut history = History::new(WorldDocument::open(dir.path()).unwrap());
    let base = history.document().revision();
    assert!(
        history
            .prepare(base, "editor:action", &conflicting.to_string())
            .is_err()
    );
    assert!(history.pending().is_none());
    // The delete guard is the zone's whole canonical entry.
    let (_other, mut fresh) = fixture_with(|root| {
        // Parsed and rewritten, so the checkout's line endings do not matter.
        let path = root.join(ZONES);
        let mut zones: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id": "spare", "name": "Spare"}));
        fs::write(path, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
    });
    fresh
        .dispatch(delete(&fresh, "spare"), &mut records)
        .unwrap();
    let operation = operations(&fresh)[0].clone();
    assert_eq!(operation["op"], "remove");
    assert_eq!(
        operation["expect"],
        serde_json::json!({"id": "spare", "name": "Spare"})
    );
}

#[cfg(target_os = "linux")]
/// Accept, undo and redo `action`; every step publishes one recovery record,
/// the result loads as a valid World and each undo restores `files` byte for
/// byte (an absent file reads as `None`).
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
    // The result is a valid World: a fresh open validates every file.
    tilewright_source::load(root).unwrap();
    open(root);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(read(), original, "undo is byte-exact");
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(read(), changed, "redo is byte-exact");
    assert_eq!(records.0.len(), 3);
    changed
}

#[cfg(target_os = "linux")]
#[test]
fn display_names_are_set_added_and_removed_with_guards_and_undo_byte_for_byte() {
    let (dir, mut session) = fixture();
    let root = dir.path().to_owned();
    let mut records = Records::default();
    // A named zone: one guarded `set`.
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        rename(&session, "meadow", Some("Open meadow")),
        &[ZONES],
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(text.contains("\"name\": \"Open meadow\""), "{text}");
    assert_eq!(
        zone(&session, "meadow").unwrap().name.as_deref(),
        Some("Open meadow")
    );
    // An unnamed zone: an absent test and a `set` that adds the member.
    session
        .dispatch(
            rename(&session, "meadow.pond", Some("Still pond")),
            &mut records,
        )
        .unwrap();
    let ops = operations(&session);
    assert_eq!(
        (ops[0]["op"].as_str(), ops[0]["absent"].as_bool()),
        (Some("test"), Some(true))
    );
    assert_eq!(ops[1]["op"], "set");
    assert!(ops[1].get("expect").is_none());
    assert!(
        session
            .pending_warnings()
            .contains(&"Names zone meadow.pond 'Still pond'.".to_owned()),
        "{:?}",
        session.pending_warnings()
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // An empty name is no name: the member is removed, and the zone shows
    // the name its parents give it, if any.
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        rename(&session, "path", Some("")),
        &[ZONES]
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(!text.contains("Stone path"), "{text}");
    session
        .dispatch(rename(&session, "meadow", None), &mut records)
        .unwrap();
    assert_eq!(operations(&session)[0]["op"], "remove");
    assert_eq!(operations(&session)[0]["expect"], "Open meadow");
    assert!(
        session.pending_warnings().contains(
            &"Removes the display name 'Open meadow' of zone meadow; it shows no display name."
                .to_owned()
        ),
        "{:?}",
        session.pending_warnings()
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // The same name, and no name for an unnamed zone, change nothing.
    refused!(
        &mut session,
        rename(&session, "meadow", Some("Open meadow")),
        "already has this display name",
    );
    refused!(
        &mut session,
        rename(&session, "meadow.pond", None),
        "already has this display name",
    );
    refused!(
        &mut session,
        rename(&session, "nowhere", Some("x")),
        "unknown zone"
    );
    // The format's limit of 256 characters counts characters, not bytes.
    refused!(
        &mut session,
        rename(&session, "meadow", Some(&"n".repeat(257))),
        "at most 256 characters",
    );
    session
        .dispatch(
            rename(&session, "meadow", Some(&"é".repeat(256))),
            &mut records,
        )
        .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn parents_change_with_guards_and_cycles_and_unknown_parents_are_refused_by_session() {
    let (dir, mut session) = fixture_with(|root| {
        let path = root.join(ZONES);
        let text = fs::read_to_string(&path).unwrap();
        // meadow > meadow.pond > meadow.pond.reeds
        fs::write(
            path,
            text.replace(
                "  ]\n}",
                "  ,\n    {\"id\": \"meadow.pond.reeds\", \"parent\": \"meadow.pond\"}\n  ]\n}",
            )
            .replace("}\n  ,\n", "},\n"),
        )
        .unwrap();
    });
    let root = dir.path().to_owned();
    assert!(zone(&session, "meadow.pond.reeds").is_some());
    let mut records = Records::default();
    // A top-level zone gets a parent: an absent test and a `set`.
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        reparent(&session, "path", Some("meadow")),
        &[ZONES],
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(text.contains("\"parent\": \"meadow\""), "{text}");
    // Another parent: a guarded `set`; and back to the top level: a
    // guarded `remove`.
    session
        .dispatch(
            reparent(&session, "meadow.pond.reeds", Some("path")),
            &mut records,
        )
        .unwrap();
    let ops = operations(&session);
    assert_eq!(
        (ops[0]["op"].as_str(), ops[0]["expect"].as_str()),
        (Some("set"), Some("meadow.pond"))
    );
    assert!(
        session
            .pending_warnings()
            .contains(&"Moves zone meadow.pond.reeds under path.".to_owned())
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    accept_undo_redo!(
        &root,
        &mut session,
        reparent(&session, "meadow.pond", None),
        &[ZONES],
    );
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    // Session refuses a cycle on its own, whatever list the UI offered:
    // itself, a child and a grandchild.
    for parent in ["meadow", "meadow.pond", "meadow.pond.reeds"] {
        refused!(
            &mut session,
            reparent(&session, "meadow", Some(parent)),
            "cycle",
        );
    }
    refused!(
        &mut session,
        reparent(&session, "meadow", Some("missing")),
        "unknown parent zone",
    );
    refused!(
        &mut session,
        reparent(&session, "meadow.pond", Some("meadow")),
        "already has this parent",
    );
    // The UI's list leaves out the zone and its descendants.
    let plane = &session.world().planes()["main"];
    // `path` is under `meadow` now, so `meadow` has no possible parent.
    assert!(zones::parent_choices(plane, "meadow").is_empty());
    assert_eq!(
        zones::parent_choices(plane, "meadow.pond"),
        ["meadow", "path"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_new_zone_is_inserted_at_the_end_and_can_be_painted() {
    let (dir, mut session) = fixture();
    let root = dir.path().to_owned();
    let mut records = Records::default();
    session
        .dispatch(
            create(&session, "meadow.grove", Some("Grove"), Some("meadow")),
            &mut records,
        )
        .unwrap();
    let ops = operations(&session);
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0]["op"], "test");
    assert_eq!(
        ops[0]["pointer"],
        serde_json::json!(["zones", {"id": "meadow.grove"}])
    );
    assert_eq!(ops[0]["absent"], true);
    assert_eq!(ops[1]["op"], "insert");
    assert_eq!(ops[1]["pointer"], serde_json::json!(["zones", 3]));
    assert_eq!(
        session.pending_warnings(),
        ["Creates zone meadow.grove under meadow, named 'Grove'."]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        create(&session, "meadow.grove", Some("Grove"), Some("meadow")),
        &[ZONES],
    );
    let text = String::from_utf8(changed[0].clone().unwrap()).unwrap();
    assert!(
        text.ends_with(
            "    {\"id\": \"meadow.grove\", \"parent\": \"meadow\", \"name\": \"Grove\"}\n  ]\n}\n"
        ),
        "{text}"
    );
    // It is in the palette and the brush paints it.
    assert!(
        tilewright_editor_ui::tools::zone_palette(session.world(), session.catalog())
            .contains(&"meadow.grove")
    );
    let usage = zones::zone_use(&session.world().planes()["main"], "meadow.grove");
    assert!(usage.unused(), "{usage:?}");
    session
        .dispatch(
            Action::PaintZones {
                plane: "main".into(),
                layer: 0,
                cells: vec![[4, 4]],
                zone: Some("meadow.grove".into()),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        zones::tile_zone(&session.world().planes()["main"], 0, 4, 4)
            .unwrap()
            .name,
        Some("Grove")
    );
    // A top-level zone without a name.
    session
        .dispatch(create(&session, "north", None, None), &mut records)
        .unwrap();
    assert_eq!(
        session.pending_warnings(),
        ["Creates zone north at the top level."]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Refusals: the identifier shape, a used id, an unknown parent.
    for id in ["", "Grove", "-grove", "a b", &"x".repeat(129)] {
        refused!(
            &mut session,
            create(&session, id, None, None),
            "invalid zone id"
        );
    }
    refused!(
        &mut session,
        create(&session, "path", Some("Again"), None),
        "already used",
    );
    refused!(
        &mut session,
        create(&session, "east", None, Some("missing")),
        "unknown parent zone",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_plane_without_zones_jsonc_gets_one_and_undo_removes_it() {
    let (dir, mut session) = fixture_with(|root| {
        fs::remove_file(root.join(ZONES)).unwrap();
        // The default zone and the painted cells name zones that no longer
        // exist: drop both.
        let plane = root.join("planes/main/plane.jsonc");
        let text = fs::read_to_string(&plane).unwrap();
        fs::write(
            &plane,
            text.replace(",\n  \"default_zone\": \"meadow\"", ""),
        )
        .unwrap();
        let chunk = root.join("planes/main/layers/0/0_0.jsonc");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&chunk).unwrap()).unwrap();
        value["terrain"].as_object_mut().unwrap().remove("zone");
        fs::write(&chunk, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    });
    let root = dir.path().to_owned();
    assert!(session.world().planes()["main"].zones.is_empty());
    let mut records = Records::default();
    session
        .dispatch(create(&session, "first", Some("First"), None), &mut records)
        .unwrap();
    let ops = operations(&session);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["op"], "document.create");
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let changed = accept_undo_redo!(
        &root,
        &mut session,
        create(&session, "first", Some("First"), None),
        &[ZONES],
    );
    // The helper's undo found the document absent again; it ends redone.
    assert!(changed[0].is_some(), "created");
    assert!(root.join(ZONES).exists());
    // Deleting the only zone leaves an empty array; the document stays.
    accept_undo_redo!(&root, &mut session, delete(&session, "first"), &[ZONES]);
    let text = fs::read_to_string(root.join(ZONES)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["zones"], serde_json::json!([]), "{text}");
    tilewright_source::load(&root).unwrap();
}

#[cfg(target_os = "linux")]
/// The fixture plus a chunk outside the view on layer 0 and one on layer 1,
/// each holding `meadow.pond` on one tile, so the pond is used three times
/// over.
fn with_far_chunks(root: &Path) {
    let source = root.join("planes/main/layers/0/0_0.jsonc");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    value["objects"] = serde_json::json!([]);
    let mut rows = vec![".".repeat(32); 32];
    rows[0] = format!("p{}", ".".repeat(31));
    value["terrain"]["zone"] = serde_json::json!({
        "palette": {"p": "meadow.pond"},
        "rows": rows
    });
    for (layer, name) in [(0, "5_5"), (1, "0_0")] {
        let mut chunk = value.clone();
        chunk["chunk"] = if layer == 0 {
            serde_json::json!([5, 5])
        } else {
            value["chunk"].clone()
        };
        if chunk.get("layer").is_some() {
            chunk["layer"] = serde_json::json!(layer);
        }
        let directory = root.join(format!("planes/main/layers/{layer}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{name}.jsonc")),
            serde_json::to_vec_pretty(&chunk).unwrap(),
        )
        .unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn deleting_a_used_zone_is_refused_with_each_reason_and_an_unused_one_is_removed() {
    let (dir, mut session) = fixture_with(with_far_chunks);
    let root = dir.path().to_owned();
    tilewright_source::load(&root).unwrap();
    // Tiles in view, outside it and on another layer; a child.
    let usage = zones::zone_use(&session.world().planes()["main"], "meadow.pond");
    assert_eq!(
        (usage.tiles, usage.chunks, usage.layers),
        (6, 3, 2),
        "{usage:?}"
    );
    let error = refused!(&mut session, delete(&session, "meadow.pond"), "in use");
    assert!(
        error.contains("6 tiles hold it, in 3 chunks on 2 layers"),
        "{error}"
    );
    // The default zone with a child and no painted tile.
    let error = refused!(&mut session, delete(&session, "meadow"), "in use");
    assert!(
        error.contains("1 child zone (meadow.pond)")
            && error.contains("it is the plane's default zone")
            && !error.contains("tile"),
        "{error}"
    );
    // `path` holds tiles only in the view.
    let error = refused!(&mut session, delete(&session, "path"), "in use");
    assert!(
        error.contains("8 tiles hold it, in 1 chunk on 1 layer"),
        "{error}"
    );
    refused!(&mut session, delete(&session, "nowhere"), "unknown zone");

    // Only tiles outside the view keep the pond: still refused.
    let mut records = Records::default();
    session
        .dispatch(
            Action::PaintZones {
                plane: "main".into(),
                layer: 0,
                cells: vec![[0, 0], [1, 1], [1, 2], [2, 1], [2, 2]],
                zone: None,
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let error = refused!(&mut session, delete(&session, "meadow.pond"), "in use");
    assert!(
        error.contains("2 tiles hold it, in 2 chunks on 2 layers"),
        "{error}"
    );

    // A palette that still lists the zone with no tile is a reason too.
    let (_listed, mut listed) = fixture_with(|root| {
        let chunk = root.join("planes/main/layers/0/0_0.jsonc");
        let text = fs::read_to_string(&chunk).unwrap();
        fs::write(&chunk, text.replace(".pp.....", "........")).unwrap();
    });
    let error = refused!(&mut listed, delete(&listed, "meadow.pond"), "in use");
    assert!(
        error.contains("the zone palette of 1 chunk still lists it"),
        "{error}"
    );

    // An unused zone is removed with one guarded `remove`, and back.
    let (dir, mut session) = fixture();
    let root = dir.path().to_owned();
    session
        .dispatch(create(&session, "spare", None, Some("path")), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let before = fs::read(root.join(ZONES)).unwrap();
    accept_undo_redo!(&root, &mut session, delete(&session, "spare"), &[ZONES]);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(root.join(ZONES)).unwrap(), before);
    session
        .dispatch(delete(&session, "spare"), &mut records)
        .unwrap();
    assert_eq!(session.pending_warnings(), ["Deletes unused zone spare."]);
}

#[test]
fn a_world_refusal_surfaces_clearly_with_nothing_pending_and_properties_are_read_only() {
    // A profile whose zone properties require `music`: every existing zone
    // has it, so the World loads, but a zone created without properties is
    // refused by World's own schema check when the change is prepared.
    let (_dir, mut session) = fixture_with(|root| {
        fs::write(
            root.join("profile/schema/zone.jsonc"),
            r#"{"format": "tilewright.schema/1", "fields": {"music": {"type": "string", "required": true}}}"#,
        )
        .unwrap();
        let profile = root.join("profile/profile.jsonc");
        let text = fs::read_to_string(&profile).unwrap();
        fs::write(
            &profile,
            text.replace(
                "\"zone_properties\": \"schema/empty.jsonc\"",
                "\"zone_properties\": \"schema/zone.jsonc\"",
            ),
        )
        .unwrap();
        let zones = root.join(ZONES);
        let text = fs::read_to_string(&zones).unwrap();
        fs::write(
            &zones,
            text.replace(
                "\"id\": \"meadow\",",
                "\"id\": \"meadow\",\n      \"properties\": {\"music\": \"calm\"},",
            )
            .replace(
                "\"id\": \"meadow.pond\",",
                "\"id\": \"meadow.pond\",\n      \"properties\": {\"music\": \"water\"},",
            )
            .replace(
                "\"id\": \"path\",",
                "\"id\": \"path\",\n      \"properties\": {\"music\": \"steps\"},",
            ),
        )
        .unwrap();
    });
    let error = refused!(
        &mut session,
        create(&session, "grove", Some("Grove"), None),
        "zone change refused",
    );
    assert!(error.contains("music"), "{error}");
    assert!(error.len() < 4096, "{error}");
    assert!(zone(&session, "grove").is_none());
    // Properties show read-only, member by member, as compact JSON.
    assert_eq!(
        zones::properties(zone(&session, "meadow").unwrap()),
        [("music".to_owned(), "\"calm\"".to_owned())]
    );
    // Editing a name keeps them.
    session
        .dispatch(
            rename(&session, "meadow", Some("Open meadow")),
            &mut Records::default(),
        )
        .unwrap();
    assert_eq!(
        zones::properties(zone(&session, "meadow").unwrap()),
        [("music".to_owned(), "\"calm\"".to_owned())]
    );
}

#[test]
fn tints_follow_definitions_in_pending_and_after_cancel() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let before = zones::plane_tints(&session.world().planes()["main"])
        .into_iter()
        .map(|(zone, tint)| (zone.to_owned(), tint))
        .collect::<std::collections::BTreeMap<_, _>>();
    session
        .dispatch(create(&session, "grove", None, None), &mut records)
        .unwrap();
    let pending = zones::plane_tints(&session.world().planes()["main"]);
    assert!(pending.contains_key("grove"));
    assert_eq!(pending.len(), before.len() + 1);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let after = zones::plane_tints(&session.world().planes()["main"])
        .into_iter()
        .map(|(zone, tint)| (zone.to_owned(), tint))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(after, before);
}
