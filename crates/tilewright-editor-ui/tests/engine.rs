// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The engine boundary that every front end relies on: the actor reaches
//! every transaction, every edit is addressed without the frame selection,
//! Accept through `Session::dispatch` enforces review completeness, every
//! refusal carries its code, and the pending summary is bounded.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, SelectionKey, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::review::{ChangeKind, MAXIMUM_REVIEW_FILES, REVIEW_LIMIT_MESSAGE};
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, Actor, PortalTarget, RefusalCode, Session, SessionPhase};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
/// The desktop app's actor.
const APP_ACTOR: &str = "editor:prototype";
/// Another actor, with characters JSON must escape.
const AGENT_ACTOR: &str = "agent:engine \"test\" \\ suite";

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
fn open(root: &Path, actor: &str) -> Session {
    let document = WorldDocument::open(root).unwrap();
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
        Actor::new(actor).unwrap(),
    );
    session.set_uid_source(Box::new(SeededUidSource::new(7)));
    session
}
fn fixture_with(configure: impl FnOnce(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    dir
}
/// Parse, change and rewrite a JSON file.
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
/// The neutral fixture with one unused zone, `spare`, to delete.
fn spare_zone(root: &Path) {
    edit_json(root, "planes/main/zones.jsonc", |zones| {
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "spare"}));
    });
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
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn sealed(session: &Session) -> String {
    session.history().pending().unwrap().sealed().to_owned()
}
fn sealed_actor(session: &Session) -> String {
    let sealed: Value = serde_json::from_str(&sealed(session)).unwrap();
    sealed["actor"].as_str().unwrap().to_owned()
}

/// The name of every edit `Action`; `None` for selection, review and
/// history actions. The match has no wildcard, so a new variant does not
/// compile until it is classified here, and an edit must then join
/// [`EDIT_KINDS`] and [`edits`].
fn edit_kind(action: &Action) -> Option<&'static str> {
    match action {
        Action::Move { .. } => Some("Move"),
        Action::SetField { .. } => Some("SetField"),
        Action::Paint { .. } => Some("Paint"),
        Action::PaintStroke { .. } => Some("PaintStroke"),
        Action::PaintZones { .. } => Some("PaintZones"),
        Action::Remove { .. } => Some("Remove"),
        Action::Place { .. } => Some("Place"),
        Action::RenameZone { .. } => Some("RenameZone"),
        Action::SetZoneParent { .. } => Some("SetZoneParent"),
        Action::CreateZone { .. } => Some("CreateZone"),
        Action::DeleteZone { .. } => Some("DeleteZone"),
        Action::CreateAnchor { .. } => Some("CreateAnchor"),
        Action::MoveAnchor { .. } => Some("MoveAnchor"),
        Action::DeleteAnchor { .. } => Some("DeleteAnchor"),
        Action::SetPortal { .. } => Some("SetPortal"),
        Action::Select { .. }
        | Action::ClearSelection
        | Action::Accept
        | Action::Cancel
        | Action::Undo
        | Action::Redo
        | Action::Reopen => None,
    }
}
const EDIT_KINDS: [&str; 15] = [
    "Move",
    "SetField",
    "Paint",
    "PaintStroke",
    "PaintZones",
    "Remove",
    "Place",
    "RenameZone",
    "SetZoneParent",
    "CreateZone",
    "DeleteZone",
    "CreateAnchor",
    "MoveAnchor",
    "DeleteAnchor",
    "SetPortal",
];

/// One action of every edit kind, built only from uids, ids and plane
/// coordinates: no `Select`, no frame generation.
#[allow(clippy::too_many_lines)]
fn edits(expected: [u8; 32]) -> Vec<Action> {
    let main = || String::from("main");
    vec![
        Action::Move {
            uid: "marker_1".into(),
            x: 4,
            y: 2,
            expected,
        },
        Action::SetField {
            uid: "marker_1".into(),
            field: "label".into(),
            value: json!("Renamed marker"),
            expected,
        },
        Action::Paint {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 0,
            y: 0,
            proto: "sand".into(),
            expected,
        },
        Action::PaintStroke {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            cells: vec![[0, 0], [1, 0]],
            proto: "sand".into(),
            expected,
        },
        Action::PaintZones {
            plane: main(),
            layer: 0,
            cells: vec![[6, 0]],
            zone: Some("path".into()),
            expected,
        },
        Action::Remove {
            uid: "marker_2".into(),
            expected,
        },
        Action::Place {
            plane: main(),
            layer: 0,
            x: 2,
            y: 2,
            proto: "marker".into(),
            expected,
        },
        Action::RenameZone {
            plane: main(),
            zone: "path".into(),
            name: Some("Trail".into()),
            expected,
        },
        Action::SetZoneParent {
            plane: main(),
            zone: "path".into(),
            parent: Some("meadow".into()),
            expected,
        },
        Action::CreateZone {
            plane: main(),
            zone: "camp".into(),
            name: Some("Camp".into()),
            parent: None,
            expected,
        },
        Action::DeleteZone {
            plane: main(),
            zone: "spare".into(),
            expected,
        },
        Action::CreateAnchor {
            plane: main(),
            anchor: "camp".into(),
            x: 2,
            y: 2,
            expected,
        },
        Action::MoveAnchor {
            plane: main(),
            anchor: "arrival".into(),
            x: 6,
            y: 3,
            expected,
        },
        Action::DeleteAnchor {
            plane: main(),
            anchor: "lookout".into(),
            expected,
        },
        Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: main(),
                anchor: "arrival".into(),
            }),
            expected,
        },
    ]
}

#[test]
fn the_table_covers_every_edit_action() {
    let kinds: Vec<&str> = edits([0; 32]).iter().filter_map(edit_kind).collect();
    assert_eq!(kinds, EDIT_KINDS);
    for other in [
        Action::ClearSelection,
        Action::Accept,
        Action::Cancel,
        Action::Undo,
        Action::Redo,
        Action::Reopen,
    ] {
        assert_eq!(edit_kind(&other), None);
    }
}

#[test]
fn every_edit_is_addressed_without_the_selection_and_carries_the_sessions_actor() {
    let dir = fixture_with(spare_zone);
    let mut agent = open(dir.path(), AGENT_ACTOR);
    let mut app = open(dir.path(), APP_ACTOR);
    let mut records = Records::default();
    let encoded_agent = Value::String(AGENT_ACTOR.into()).to_string();
    let encoded_app = Value::String(APP_ACTOR.into()).to_string();
    let expected = revision(&agent);
    for (agent_action, app_action) in edits(expected).into_iter().zip(edits(expected)) {
        let kind = edit_kind(&agent_action).unwrap();
        for session in [&agent, &app] {
            assert!(session.selection().is_none(), "{kind}");
        }
        agent
            .dispatch(agent_action, &mut records)
            .unwrap_or_else(|error| panic!("{kind}: {} {error}", error.code()));
        app.dispatch(app_action, &mut records)
            .unwrap_or_else(|error| panic!("{kind}: {} {error}", error.code()));
        assert_eq!(agent.status().phase, SessionPhase::Pending, "{kind}");
        assert_eq!(sealed_actor(&agent), AGENT_ACTOR, "{kind}");
        assert_eq!(sealed_actor(&app), APP_ACTOR, "{kind}");
        // The actor is the only difference between the two transactions.
        assert_eq!(
            sealed(&agent).replace(&encoded_agent, &encoded_app),
            sealed(&app),
            "{kind}"
        );
        for session in [&mut agent, &mut app] {
            session.dispatch(Action::Cancel, &mut records).unwrap();
            assert!(session.selection().is_none(), "{kind}");
        }
    }
    assert!(records.inverses.is_empty());
}

/// The text a transaction writer wrote with the fixed actor, from the
/// sealed transaction of the same action.
type FixedText = Box<dyn Fn(&Value) -> String>;

#[test]
fn the_apps_transactions_are_byte_identical_to_the_fixed_actor_text() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path(), APP_ACTOR);
    let mut records = Records::default();
    let expected = revision(&session);
    // Each of the three transaction writers, with the text it wrote when the
    // actor was the fixed string `editor:prototype`.
    let mut cases: Vec<(Action, FixedText)> = Vec::new();
    cases.push((
        Action::Paint {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 0,
            y: 0,
            proto: "sand".into(),
            expected,
        },
        Box::new(|_| {
            json!({"format":"tilewright.transaction/1","title":"Editor semantic action","actor":"editor:prototype","operations":[{"op":"terrain.set","plane":"main","layer":0,"grid":"floor","cells":[{"at":[0,0],"value":"sand","expect":"grass"}]}]}).to_string()
        }),
    ));
    cases.push((
        Action::Place {
            plane: "main".into(),
            layer: 0,
            x: 2,
            y: 2,
            proto: "marker".into(),
            expected,
        },
        Box::new(|sealed| {
            let placed = sealed["operations"][0]["object"]["uid"].as_str().unwrap();
            format!(
                "{{\"format\":\"tilewright.transaction/1\",\"title\":\"Editor semantic action\",\"actor\":\"editor:prototype\",\"operations\":[{{\"op\":\"object.insert\",\"plane\":\"main\",\"layer\":0,\"at\":[2,2],\"object\":{{\"uid\":\"{placed}\",\"proto\":\"marker\",\"layer\":\"item\"}}}}]}}"
            )
        }),
    ));
    cases.push((
        Action::RenameZone {
            plane: "main".into(),
            zone: "path".into(),
            name: Some("Trail".into()),
            expected,
        },
        Box::new(|_| {
            let operation = json!({"op": "set", "path": "planes/main/zones.jsonc",
                "pointer": ["zones", {"id": "path"}, "name"], "expect": "Stone path", "value": "Trail"});
            format!(
                "{{\"format\":\"tilewright.transaction/1\",\"title\":\"Editor semantic action\",\"actor\":\"editor:prototype\",\"operations\":[{operation}]}}"
            )
        }),
    ));
    for (action, before) in cases {
        session.dispatch(action, &mut records).unwrap();
        let text = before(&serde_json::from_str(&sealed(&session)).unwrap());
        let mut history = History::new(WorldDocument::open(dir.path()).unwrap());
        let fixed = history
            .prepare(expected, "editor:action", &text)
            .unwrap()
            .sealed()
            .to_owned();
        assert_eq!(sealed(&session), fixed);
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
}

#[test]
fn a_redo_keeps_the_edits_actor_and_an_undo_records_none() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path(), AGENT_ACTOR);
    let mut records = Records::default();
    let expected = revision(&session);
    session
        .dispatch(edits(expected).swap_remove(2), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    session.dispatch(Action::Undo, &mut records).unwrap();
    let undo: Value = serde_json::from_str(&sealed(&session)).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    session.dispatch(Action::Redo, &mut records).unwrap();
    assert_eq!(sealed_actor(&session), AGENT_ACTOR);
    // An undo replays World's inverse of the accepted edit, which records
    // no actor; a redo replays the edit itself, with its actor.
    assert!(undo["actor"].is_null());
    session.dispatch(Action::Cancel, &mut records).unwrap();
}

/// A fixture whose `marker_1` label is too large for a complete review.
fn oversized_review() -> (tempfile::TempDir, Session) {
    let dir = fixture_with(|root| {
        edit_json(root, "profile/schema/object.jsonc", |schema| {
            schema["fields"]["label"]["max_length"] = Value::from(80_000);
        });
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"][0]["fields"]["label"] = Value::String("a".repeat(70_000));
        });
    });
    let mut session = open(dir.path(), AGENT_ACTOR);
    let mut records = Records::default();
    session
        .dispatch(
            Action::SetField {
                uid: "marker_1".into(),
                field: "label".into(),
                value: json!("b".repeat(100)),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    (dir, session)
}

#[test]
fn accept_through_dispatch_refuses_an_incomplete_review() {
    let (dir, mut session) = oversized_review();
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let pending = sealed(&session);
    assert!(!session.review().complete);
    assert_eq!(session.review().lines[0], REVIEW_LIMIT_MESSAGE);
    let summary = session.pending_summary().unwrap();
    assert!(!summary.complete);
    assert_eq!(summary.files.len(), 1);
    let mut records = Records::default();
    let refused = session.dispatch(Action::Accept, &mut records).unwrap_err();
    assert_eq!(refused.code(), RefusalCode::ReviewIncomplete);
    assert_eq!(refused.message(), REVIEW_LIMIT_MESSAGE);
    // Nothing was retained or published; the change stays for Cancel.
    assert!(records.inverses.is_empty());
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
    assert_eq!(sealed(&session), pending);
    assert_eq!(session.status().phase, SessionPhase::Pending);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(session.review().complete);
    assert!(session.pending_summary().is_none());
}

#[test]
fn the_pending_summary_reports_files_review_warnings_and_revisions() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path(), AGENT_ACTOR);
    let mut records = Records::default();
    assert!(session.pending_summary().is_none());
    let base = revision(&session);
    session
        .dispatch(
            Action::Remove {
                uid: "marker_2".into(),
                expected: base,
            },
            &mut records,
        )
        .unwrap();
    let summary = session.pending_summary().unwrap();
    assert_eq!(summary.base, base);
    assert_eq!(summary.result, session.world().source_digest());
    assert_ne!(summary.result, base);
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files_omitted, 0);
    let file = summary.files[0];
    assert_eq!(file.path, CHUNK);
    assert_eq!(file.kind, ChangeKind::Modified);
    assert!(file.after_bytes.unwrap() < file.before_bytes.unwrap());
    assert!(summary.complete);
    assert_eq!(summary.review, session.review());
    assert_eq!(summary.warnings, session.pending_warnings());
    assert_eq!(summary.warnings_omitted, 0);
    assert!(summary.files.len() <= MAXIMUM_REVIEW_FILES);
    session.dispatch(Action::Cancel, &mut records).unwrap();
}

/// Dispatch `action` and expect a refusal with `code`.
fn refused(session: &mut Session, action: Action, records: &mut Records) -> RefusalCode {
    let error = session.dispatch(action, records).unwrap_err();
    assert!(!error.code().as_str().is_empty());
    assert!(!error.message().is_empty());
    assert_eq!(error.to_string(), error.message());
    error.code()
}

#[test]
#[allow(clippy::too_many_lines)]
fn refusals_carry_codes_by_meaning() {
    let dir = fixture_with(|_| {});
    let mut session = open(dir.path(), AGENT_ACTOR);
    let mut records = Records::default();
    let expected = revision(&session);
    let paint = |x: i64, proto: &str, expected: [u8; 32]| Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y: 0,
        proto: proto.into(),
        expected,
    };
    let stroke = |cells: Vec<[i64; 2]>| Action::PaintStroke {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        cells,
        proto: "sand".into(),
        expected,
    };
    let cases: Vec<(Action, RefusalCode)> = vec![
        (paint(0, "sand", [0; 32]), RefusalCode::RevisionStale),
        (paint(100, "sand", expected), RefusalCode::ScopeOutsideView),
        (paint(0, "stone", expected), RefusalCode::GrantMissing),
        (
            Action::Remove {
                uid: "nowhere".into(),
                expected,
            },
            RefusalCode::TargetUnknown,
        ),
        (
            Action::CreateZone {
                plane: "main".into(),
                zone: "meadow".into(),
                name: None,
                parent: None,
                expected,
            },
            RefusalCode::TargetExists,
        ),
        (
            Action::CreateZone {
                plane: "main".into(),
                zone: "Not An Id".into(),
                name: None,
                parent: None,
                expected,
            },
            RefusalCode::InputIdentifier,
        ),
        (
            Action::DeleteAnchor {
                plane: "main".into(),
                anchor: "arrival".into(),
                expected,
            },
            RefusalCode::TargetInUse,
        ),
        (
            Action::SetZoneParent {
                plane: "main".into(),
                zone: "meadow".into(),
                parent: Some("meadow.pond".into()),
                expected,
            },
            RefusalCode::HierarchyCycle,
        ),
        (stroke(vec![[1, 0], [0, 0]]), RefusalCode::InputInvalid),
        (stroke(Vec::new()), RefusalCode::InputBudget),
        (
            Action::MoveAnchor {
                plane: "main".into(),
                anchor: "arrival".into(),
                x: 7,
                y: 3,
                expected,
            },
            RefusalCode::ChangeNone,
        ),
        (
            Action::SetZoneParent {
                plane: "other".into(),
                zone: "meadow".into(),
                parent: None,
                expected,
            },
            RefusalCode::ScopeOutsideView,
        ),
        (
            Action::Select {
                generation: u64::MAX,
                key: SelectionKey::Object {
                    uid: "marker_1".into(),
                },
            },
            RefusalCode::SelectionStale,
        ),
        (Action::Undo, RefusalCode::HistoryNothingToUndo),
        (Action::Redo, RefusalCode::HistoryNothingToRedo),
        (Action::Accept, RefusalCode::PendingNone),
        (
            Action::SetField {
                uid: "marker_1".into(),
                field: "label".into(),
                value: json!(12),
                expected,
            },
            RefusalCode::WorldRefused,
        ),
    ];
    let mut seen = BTreeSet::new();
    for (action, code) in cases {
        let name = format!("{action:?}");
        assert_eq!(refused(&mut session, action, &mut records), code, "{name}");
        assert!(session.history().pending().is_none(), "{name}");
        seen.insert(code);
    }
    // A pending change fences the next edit.
    session
        .dispatch(paint(0, "sand", expected), &mut records)
        .unwrap();
    assert_eq!(
        refused(&mut session, paint(1, "sand", expected), &mut records),
        RefusalCode::PendingExists
    );
    // A failed recovery retention publishes nothing.
    records.fail = true;
    assert_eq!(
        refused(&mut session, Action::Accept, &mut records),
        RefusalCode::RecoveryFailed
    );
    records.fail = false;
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // A source change behind the session's back is found before preparing.
    let chunk = dir.path().join(CHUNK);
    let mut text = fs::read(&chunk).unwrap();
    text.extend_from_slice(b"\n");
    fs::write(&chunk, text).unwrap();
    assert_eq!(
        refused(&mut session, paint(0, "sand", expected), &mut records),
        RefusalCode::SourceChanged
    );
    assert!(!seen.contains(&RefusalCode::Internal));
}
