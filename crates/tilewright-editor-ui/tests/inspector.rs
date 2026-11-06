// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT
use std::{fs, path::Path};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewCatalog, PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey,
    inspector::{MAX_DISPLAY_BYTES, MAX_INPUT_BYTES, SelectionSummary, display_text, inspect},
    tools::{ToolMode, ToolState, field_choices, terrain_palette},
};

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
fn fixture() -> (tempfile::TempDir, WorldDocument, PreviewCatalog) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    let schema_path = dir.path().join("profile/schema/object.jsonc");
    // Order is intentional: the inspector must follow schema declaration order.
    fs::write(schema_path, r#"{"format":"tilewright.schema/1","fields":{
      "flag":{"type":"boolean"},
      "count":{"type":"integer","min":-2,"max":7},
      "weight":{"type":"number","min":-1,"max":2},
      "choice":{"type":"string","max_length":8,"values":["red","青"]},
      "reference":{"type":"reference","target":"proto"},
      "label":{"type":"string","max_length":4096},
      "settings":{"type":"object","required":true,"fields":{"active":{"type":"boolean","required":true},"visits":{"type":"integer","min":0,"max":99,"required":true}}}
    }}"#).unwrap();
    let path = dir.path().join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["view"]["bounds"] = serde_json::json!([-1, 0, 9, 6]);
    catalog["editing"]["fields"] = serde_json::json!(
        [
            "flag",
            "count",
            "weight",
            "choice",
            "reference",
            "label",
            "settings"
        ]
        .map(|field| serde_json::json!({"uid":"marker_1","field":field}))
    );
    fs::write(path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    // The profile requirement applies to every fixture object, including
    // the off-view marker in the negative chunk.
    let chunk_path = dir.path().join("planes/main/layers/0/-1_0.jsonc");
    let mut chunk: serde_json::Value =
        serde_json::from_slice(&fs::read(&chunk_path).unwrap()).unwrap();
    for object in chunk["objects"].as_array_mut().unwrap() {
        object["fields"]["settings"] = serde_json::json!({"active":true,"visits":0});
    }
    fs::write(chunk_path, serde_json::to_vec(&chunk).unwrap()).unwrap();
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    (dir, document, catalog)
}
fn key() -> SelectionKey {
    SelectionKey::Object {
        uid: "marker_1".into(),
    }
}
fn replace(tools: &mut ToolState, value: &str) {
    while tools.draft_text().is_some_and(|s| !s.is_empty()) {
        tools.backspace();
    }
    tools.input(value).unwrap();
}
#[test]
fn authoritative_rows_missing_fields_and_structured_read_only() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    assert_eq!(
        view.rows.iter().map(|r| r.name).collect::<Vec<_>>(),
        [
            "flag",
            "count",
            "weight",
            "choice",
            "reference",
            "label",
            "settings"
        ]
    );
    assert!(view.rows[0].current.is_none());
    assert!(view.rows[0].editable);
    assert!(view.rows[0].reason.unwrap().contains("absent"));
    assert!(!view.rows[6].editable);
    assert!(view.rows[6].spec.required);
    let mut tools = ToolState::default();
    assert!(tools.begin_field(&view, "settings", false).is_err());
    assert!(tools.begin_field(&view, "unknown", false).is_err());
    assert!(matches!(
        view.selection,
        Some(SelectionSummary::Object {
            at: Some([3, 2]),
            ..
        })
    ));
    assert_eq!(field_choices(&view, "choice"), ["red", "青"]);
    assert!(field_choices(&view, "reference").contains(&"marker"));
}
#[test]
fn scalar_drafts_obey_world_types_without_json_fallback() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    for (field, valid, invalid, expected) in [
        ("flag", "true", "1", serde_json::json!(true)),
        ("count", "7", "8", serde_json::json!(7)),
        ("weight", "1.5", "NaN", serde_json::json!(1.5)),
        ("choice", "青", "blue", serde_json::json!("青")),
        (
            "reference",
            "marker",
            "unknown",
            serde_json::json!("marker"),
        ),
        (
            "label",
            "{\"raw\":true}",
            "",
            serde_json::json!("{\"raw\":true}"),
        ),
    ] {
        tools.begin_field(&view, field, false).unwrap();
        replace(&mut tools, valid);
        let Action::SetField {
            value,
            expected: revision,
            ..
        } = tools.build_action(&view, false).unwrap()
        else {
            panic!("expected field")
        };
        assert_eq!(value, expected);
        assert_eq!(revision, document.revision());
        if field != "label" {
            replace(&mut tools, invalid);
            assert!(tools.build_action(&view, false).is_err(), "{field}");
        }
    }
    tools.begin_field(&view, "weight", false).unwrap();
    for invalid in ["inf", "-inf", "2.01", "-1.01", "{}"] {
        replace(&mut tools, invalid);
        assert!(tools.build_action(&view, false).is_err());
    }
}
#[test]
fn stale_uid_frame_revision_and_pending_drafts_cannot_authorize_actions() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    tools.begin_field(&view, "label", false).unwrap();
    replace(&mut tools, "changed");
    assert!(tools.build_action(&view, true).is_err());
    assert!(tools.begin_move(&view, true).is_err());
    let frame = inspect(
        document.world(),
        &catalog,
        Some(&key),
        5,
        document.revision(),
    );
    assert!(tools.build_action(&frame, false).is_err());
    let revision = inspect(document.world(), &catalog, Some(&key), 4, [9; 32]);
    assert!(tools.build_action(&revision, false).is_err());
    let missing_key = SelectionKey::Object {
        uid: "missing".into(),
    };
    let missing = inspect(
        document.world(),
        &catalog,
        Some(&missing_key),
        4,
        document.revision(),
    );
    assert!(missing.stamp.is_none());
    assert!(tools.build_action(&missing, false).is_err());
    assert_eq!(tools.draft_text(), Some("changed"));
    tools.cancel();
    assert!(tools.draft_text().is_none());
}
#[test]
fn coordinates_and_explicit_terrain_palette_produce_only_semantic_actions() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Move);
    tools.begin_move(&view, false).unwrap();
    replace(&mut tools, "-1, 3");
    assert!(matches!(
        tools.build_action(&view, false).unwrap(),
        Action::Move { x: -1, y: 3, .. }
    ));
    replace(&mut tools, "9223372036854775808,0");
    assert!(tools.build_action(&view, false).is_err());
    let cell = SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: -1,
        y: 0,
    };
    let cell_view = inspect(
        document.world(),
        &catalog,
        Some(&cell),
        4,
        document.revision(),
    );
    assert!(cell_view.selection.is_some());
    assert_eq!(terrain_palette(&catalog).len(), 2);
    assert!(tools.choose_terrain(&catalog, "floor", "marker").is_err());
    tools.choose_terrain(&catalog, "floor", "sand").unwrap();
    assert!(tools.paint_action(&cell_view, &cell, false).is_err());
    tools.set_mode(ToolMode::Terrain);
    assert!(
        matches!(tools.paint_action(&cell_view,&cell,false).unwrap(),Action::Paint{proto,..} if proto == "sand")
    );
    assert!(tools.paint_action(&cell_view, &cell, true).is_err());
}
#[test]
fn utf8_input_boundaries_are_atomic_and_backspace_removes_a_character() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    tools.begin_field(&view, "label", false).unwrap();
    replace(&mut tools, &"é".repeat(MAX_INPUT_BYTES / 2));
    assert!(tools.input("☃").is_err());
    assert_eq!(tools.draft_text().unwrap().len(), MAX_INPUT_BYTES);
    tools.backspace();
    assert_eq!(tools.draft_text().unwrap().len(), MAX_INPUT_BYTES - 2);
    tools.input("青").unwrap_err();
    tools.input("é").unwrap();
    assert!(
        matches!(tools.build_action(&view,false).unwrap(),Action::SetField{value,..} if value.as_str().unwrap().ends_with('é'))
    );
    let display = display_text(&"☃".repeat(300));
    assert!(display.len() <= MAX_DISPLAY_BYTES);
    assert!(display.ends_with('…'));
}

#[test]
fn composition_never_reaches_an_action_and_draft_identity_fences_old_frames() {
    let (_dir, document, catalog) = fixture();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    tools.begin_field(&view, "label", false).unwrap();
    tools.select_all().unwrap();
    tools.input("e\u{301} العربية עברית क्ष 👨‍👩‍👧‍👦").unwrap();
    let committed = tools.draft_text().unwrap().to_owned();
    let old = tools.input_revision().unwrap();
    let source = document.revision();
    tools.update_preedit("青", 1, 0).unwrap();
    assert!(tools.build_action(&view, false).is_err());
    assert_eq!(tools.draft_text(), Some(committed.as_str()));
    assert_eq!(document.revision(), source);
    tools.cancel_preedit().unwrap();
    assert!(
        matches!(tools.build_action(&view, false).unwrap(), Action::SetField { value, .. } if value.as_str() == Some(committed.as_str()))
    );
    tools.begin_field(&view, "label", false).unwrap();
    assert!(
        tools
            .set_selection(
                tilewright_editor_ui::text_input::TextSelection::default(),
                old
            )
            .is_err()
    );
    assert!(tools.build_action(&view, false).is_ok());
}

#[test]
fn large_authoritative_schemas_are_bounded_and_report_truncation() {
    let (dir, document, _catalog) = fixture();
    drop(document);
    let path = dir.path().join("profile/schema/object.jsonc");
    let mut schema: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for index in 0..150 {
        schema["fields"][format!("extra_{index}")] = serde_json::json!({"type":"boolean"});
    }
    fs::write(path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let key = key();
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        1,
        document.revision(),
    );
    assert!(view.truncated);
    assert_eq!(
        view.rows.len(),
        tilewright_editor_ui::inspector::MAX_INSPECTOR_ROWS
    );
}
