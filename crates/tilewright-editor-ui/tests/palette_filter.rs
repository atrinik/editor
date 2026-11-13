// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The Place palette filter: local text input bound to no selection or World
//! revision. It narrows which palette rows are shown and never becomes an
//! action, a transaction or a grant.
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey, Session, SessionPhase, inspector,
    tools::{MAX_FILTER_BYTES, ToolMode, ToolState, object_palette, palette_matches},
    uid::SeededUidSource,
};

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
fn fixture() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(History::new(document), catalog, resources);
    session.set_uid_source(Box::new(SeededUidSource::new(9)));
    (dir, session)
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn place_tools() -> ToolState {
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Place);
    tools
}
fn cell(x: i64, y: i64) -> SelectionKey {
    SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
    }
}

#[test]
fn the_filter_needs_no_selection_or_revision_and_never_becomes_an_action() {
    let (dir, session) = fixture();
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let generation = session.generation();
    let revision = session.history().document().revision();
    let mut tools = ToolState::default();
    // Only Place mode has a palette to filter.
    assert!(tools.focus_filter().is_err());
    tools.set_mode(ToolMode::Place);
    assert_eq!(session.selection(), None);
    tools.focus_filter().unwrap();
    assert!(tools.filter_focused());
    tools.input("st").unwrap();
    assert_eq!(tools.palette_filter(), "st");
    assert_eq!(tools.draft_text(), Some("st"));
    // A filter draft is never reviewed, with or without a selection.
    let inspected = inspector::inspect(
        session.world(),
        session.catalog(),
        session.selection(),
        session.generation(),
        revision,
    );
    let error = tools.build_action(&inspected, false).unwrap_err();
    assert_eq!(error.0, "The palette filter is never reviewed");
    assert!(session.history().pending().is_none());
    assert_eq!(session.generation(), generation);
    assert_eq!(session.history().document().revision(), revision);
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
}

#[test]
fn filter_text_survives_focus_changes_and_session_actions_but_not_leaving_place() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut tools = place_tools();
    tools.focus_filter().unwrap();
    tools.input("ma").unwrap();
    // Enter or a click elsewhere: focus goes, the text stays.
    tools.release_filter();
    assert!(!tools.filter_focused());
    assert_eq!(tools.palette_filter(), "ma");
    // A session action (here a selection, which bumps nothing in the
    // filter) and the app's per-action cancel keep it too.
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: cell(6, 1),
            },
            &mut records,
        )
        .unwrap();
    tools.cancel();
    assert_eq!(tools.palette_filter(), "ma");
    // Refocusing starts a new draft identity with the kept text and the
    // caret at its end.
    let first = tools.input_revision();
    tools.focus_filter().unwrap();
    assert_ne!(tools.input_revision(), first);
    let snapshot = tools.draft_snapshot().unwrap();
    assert_eq!(snapshot.committed, "ma");
    assert_eq!(snapshot.selection.focus.byte, 2);
    tools.input("r").unwrap();
    // Focusing a focused filter keeps its draft.
    let revision = tools.input_revision();
    tools.focus_filter().unwrap();
    assert_eq!(tools.input_revision(), revision);
    // Re-entering Place keeps the filter; any other mode clears it.
    tools.set_mode(ToolMode::Place);
    assert_eq!(tools.palette_filter(), "mar");
    assert!(!tools.filter_focused());
    tools.set_mode(ToolMode::Select);
    assert_eq!(tools.palette_filter(), "");
    tools.set_mode(ToolMode::Place);
    assert_eq!(tools.palette_filter(), "");
    assert!(records.0.is_empty());
}

#[test]
fn escape_clears_focus_and_text_first_and_then_reports_nothing_to_clear() {
    let mut tools = place_tools();
    // Nothing to clear: Esc keeps its next meaning (cancel a pending edit).
    assert!(!tools.clear_filter());
    tools.focus_filter().unwrap();
    // A focused empty filter still takes one Esc, to release focus.
    assert!(tools.clear_filter());
    assert!(!tools.filter_focused());
    assert!(!tools.clear_filter());
    tools.focus_filter().unwrap();
    tools.input("crate").unwrap();
    assert!(tools.clear_filter());
    assert_eq!(
        (tools.palette_filter(), tools.filter_focused()),
        ("", false)
    );
    // An unfocused filter with text also takes one Esc.
    tools.focus_filter().unwrap();
    tools.input("lamp").unwrap();
    tools.release_filter();
    assert!(tools.clear_filter());
    assert_eq!(tools.palette_filter(), "");
    assert!(!tools.clear_filter());
}

#[test]
fn single_line_rules_and_the_byte_cap_refuse_input_without_changing_the_filter() {
    let mut tools = place_tools();
    tools.focus_filter().unwrap();
    tools.input("a").unwrap();
    for refused in [
        "\u{2028}", "\u{2029}", "a\nb", "\r", "\t", "\u{7}", "\u{85}",
    ] {
        assert!(tools.input(refused).is_err(), "{refused:?}");
        assert!(tools.update_preedit(refused, 0, 0).is_err(), "{refused:?}");
        assert_eq!(tools.palette_filter(), "a");
    }
    // The cap counts UTF-8 bytes: 1 + 63 fits, one more byte does not.
    tools.input(&"b".repeat(MAX_FILTER_BYTES - 1)).unwrap();
    assert_eq!(tools.palette_filter().len(), MAX_FILTER_BYTES);
    assert!(tools.input("c").is_err());
    assert!(tools.update_preedit("c", 0, 1).is_err());
    tools.select_all().unwrap();
    tools.input(&"é".repeat(MAX_FILTER_BYTES / 2)).unwrap();
    assert_eq!(tools.palette_filter().len(), MAX_FILTER_BYTES);
    assert!(tools.input("é").is_err());
    assert_eq!(tools.palette_filter().len(), MAX_FILTER_BYTES);
}

#[test]
fn matching_is_a_case_insensitive_substring_with_per_character_unicode_lowercase() {
    assert!(palette_matches("stone", ""));
    assert!(palette_matches("stone", "ST"));
    assert!(palette_matches("Stone", "tOn"));
    assert!(!palette_matches("stone", "stones"));
    assert!(palette_matches("ÉCLAIR", "é"));
    // Sigma folds the same everywhere, word-final or not.
    assert!(palette_matches("ΣΑΣ", "σ"));
    assert!(palette_matches("ΣΑΣ", "ΑΣ"));
    // Final sigma is its own character: σ does not match ς.
    assert!(!palette_matches("σας", "ΑΣ"));
    // No normalization: precomposed and decomposed forms differ.
    assert!(!palette_matches("caf\u{e9}", "cafe\u{301}"));
    // Composition is not part of the filter until committed.
    let mut tools = place_tools();
    tools.focus_filter().unwrap();
    tools.update_preedit("zz", -1, -1).unwrap();
    assert!(tools.is_composing());
    assert_eq!(tools.palette_filter(), "");
}

#[test]
fn the_filter_narrows_rows_only_and_never_what_session_allows() {
    let (dir, mut session) = fixture();
    let mut records = Records::default();
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let catalog = session.catalog();
    let mut tools = place_tools();
    tools.choose_placement(catalog, "marker").unwrap();
    tools.focus_filter().unwrap();
    tools.input("ST").unwrap();
    assert_eq!(tools.filtered_palette(catalog), ["stone"]);
    // The chosen prototype stays chosen while filtered out of view, and a
    // hidden but granted prototype can still be chosen.
    assert_eq!(tools.placement_choice(), Some("marker"));
    tools.choose_placement(catalog, "lamp").unwrap();
    tools.choose_placement(catalog, "marker").unwrap();
    // A matching filter never adds an ungranted prototype.
    tools.select_all().unwrap();
    tools.input("gr").unwrap();
    assert!(tools.filtered_palette(catalog).is_empty());
    assert!(tools.choose_placement(catalog, "grass").is_err());
    // An empty result: no rows at all (the app shows one neutral row).
    tools.select_all().unwrap();
    tools.input("zzz").unwrap();
    assert!(tools.filtered_palette(catalog).is_empty());
    assert_eq!(
        object_palette(catalog).len(),
        6,
        "bush, crate, gate, lamp, marker, stone"
    );
    // Placing the hidden choice works and goes through the session's own
    // grant, view and revision checks.
    tools.release_filter();
    let revision = session.history().document().revision();
    let action = tools
        .place_action(catalog, &cell(6, 1), revision, false)
        .unwrap();
    let Action::Place { proto, .. } = &action else {
        panic!("not a placement");
    };
    assert_eq!(proto, "marker");
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(records.0.is_empty());
}

#[test]
fn while_a_diff_is_pending_the_filter_stays_editable_and_prepares_nothing() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let revision = session.history().document().revision();
    let mut tools = place_tools();
    tools.choose_placement(session.catalog(), "stone").unwrap();
    let action = tools
        .place_action(session.catalog(), &cell(6, 1), revision, false)
        .unwrap();
    session.dispatch(action, &mut records).unwrap();
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    // Focus and typing need no free history: the filter touches no World.
    tools.focus_filter().unwrap();
    tools.input("bu").unwrap();
    assert_eq!(tools.filtered_palette(session.catalog()), ["bush"]);
    // Placing and reviewing stay refused while the diff is pending.
    tools.release_filter();
    assert!(
        tools
            .place_action(session.catalog(), &cell(5, 1), revision, true)
            .is_err()
    );
    tools.focus_filter().unwrap();
    let inspected = inspector::inspect(
        session.world(),
        session.catalog(),
        session.selection(),
        session.generation(),
        revision,
    );
    assert!(tools.build_action(&inspected, true).is_err());
    assert_eq!(session.history().pending().unwrap().sealed(), sealed);
    // Esc's first press clears the filter; the pending diff is untouched.
    assert!(tools.clear_filter());
    assert_eq!(session.history().pending().unwrap().sealed(), sealed);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(records.0.is_empty());
}

#[test]
fn beginning_a_field_edit_ends_filter_focus_but_keeps_its_text() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: SelectionKey::Object {
                    uid: "marker_1".into(),
                },
            },
            &mut records,
        )
        .unwrap();
    let mut tools = place_tools();
    tools.focus_filter().unwrap();
    tools.input("la").unwrap();
    let inspected = inspector::inspect(
        session.world(),
        session.catalog(),
        session.selection(),
        session.generation(),
        session.history().document().revision(),
    );
    tools.begin_field(&inspected, "label", false).unwrap();
    assert!(!tools.filter_focused());
    assert_eq!(tools.draft_text(), Some("Trail marker"));
    assert_eq!(tools.palette_filter(), "la");
    // Focusing the filter discards the field draft, as another field would.
    tools.focus_filter().unwrap();
    assert_eq!(tools.draft_text(), Some("la"));
    tools.cancel();
    assert_eq!(tools.draft_text(), None);
    assert_eq!(tools.palette_filter(), "la");
}
