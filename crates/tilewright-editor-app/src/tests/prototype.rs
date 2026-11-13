// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! App regression tests use the same Session, History, durable-store and CPU
//! scene paths as the scripted native demonstration; they initialize no GPU.
use super::{Application, Evidence};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tilewright_editor_commands::History;
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Session};
use tilewright_source::SourceRoot;

fn copy_private(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_private(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
            fs::set_permissions(target, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

fn application() -> (tempfile::TempDir, Application) {
    application_with_source(|_| {})
}

fn application_with_source(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let temporary = std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT").map_or_else(
        || tempfile::tempdir().unwrap(),
        |root| tempfile::tempdir_in(root).unwrap(),
    );
    let world = temporary.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    prepare(&world);
    for name in ["recovery", "evidence"] {
        let path = temporary.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let document = WorldDocument::open_prototype(&world).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let view = catalog.default_view(document.world(), 320, 240, 1);
    let prepared = tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .unwrap();
    let recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&temporary.path().join("recovery")),
        RecoveryLimits::default(),
    )
    .unwrap();
    let evidence = Evidence::new(SourceRoot::new(&temporary.path().join("evidence"))).unwrap();
    let app = Application {
        session: Session::new(History::new(document), catalog, resources),
        recovery,
        evidence,
        view,
        prepared,
        tools: tilewright_editor_ui::tools::ToolState::default(),
        ui: crate::workspace_ui::WorkspaceUi::new((320, 240), (320, 240)),
        notice: None,
        sidebar_scroll: 0,
        review_scroll: 0,
        unsupported_runs: 0,
        presented_generation: None,
        blocked: None,
        text: crate::font_pack::test_system(),
        displayed_text: None,
        presented_text_revision: None,
        stroke_mask: None,
        prepared_scenes: 1,
        preview_omitted: false,
    };
    (temporary, app)
}

#[test]
fn full_neutral_script_reselects_after_commits_and_edits_field_and_empty_cell() {
    let (temporary, mut app) = application();
    // The native demonstration installs the same deterministic uid source.
    app.session
        .set_uid_source(Box::new(tilewright_editor_ui::uid::SeededUidSource::new(
            super::DEMO_UID_SEED,
        )));
    app.demo_all().unwrap();
    let marker = app.session.world().object("marker_1").unwrap();
    assert!(marker.fields.iter().any(|(name, value)| name == "label"
        && value == &tilewright_source::FieldValue::String("Edited offline".into())));
    let chunk = &app.session.world().planes()["main"].chunks[&(0, 0, 0)];
    assert_eq!(chunk.ground[0].value(4 * 32 + 6), Some("sand"));
    // The scripted deletion takes the spare marker and leaves the edited one.
    assert!(app.session.world().object("marker_2").is_none());
    // The scripted placement puts the last palette prototype on the empty
    // demonstration tile, with the first deterministic uid, and selects it.
    let mut source = tilewright_editor_ui::uid::SeededUidSource::new(super::DEMO_UID_SEED);
    let mut next_uid = || {
        let mut bytes = [0; 16];
        tilewright_editor_ui::uid::UidSource::fill(&mut source, &mut bytes).unwrap();
        tilewright_editor_ui::uid::format_uid(bytes)
    };
    let placed = next_uid();
    let stone = app.session.world().object(&placed).unwrap();
    assert_eq!((stone.proto.as_str(), stone.at), ("stone", Some([6, 1])));
    // The search step types "A", which keeps crate, lamp and marker,
    // chooses the first match and places it on (2,1) with the next uid;
    // that object ends selected.
    let searched = next_uid();
    let found = app.session.world().object(&searched).unwrap();
    assert_eq!((found.proto.as_str(), found.at), ("crate", Some([2, 1])));
    assert_eq!(
        app.session.selection(),
        Some(&tilewright_editor_preview::SelectionKey::Object { uid: searched })
    );
    assert_eq!(app.tools.palette_filter(), "A");
    assert!(!app.tools.filter_focused());
    assert_eq!(app.session.history().depths(), (6, 0));
    for phase in [
        "before",
        "pending",
        "cancel",
        "after",
        "undo",
        "redo",
        "field",
        "terrain",
        "delete",
        "deleted",
        "place",
        "placed",
        "palette",
        "search-place",
        "search-placed",
        "stroke-preview",
    ] {
        assert!(
            temporary
                .path()
                .join(format!("evidence/{phase}.png"))
                .is_file()
        );
    }
    assert!(app.session.history().pending().is_none());
    assert_eq!(
        app.prepared.source_revision,
        app.session.world().source_digest()
    );
}

#[test]
fn rejected_pending_undo_refreshes_actual_pending_scene_and_invalidates_old_picks() {
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Move).unwrap();
    app.nudge([1, 0]).unwrap();
    let pending = app.session.world().source_digest();
    let pending_generation = app.view.frame_revision;
    let error = app.action(Action::Undo).unwrap_err();
    assert!(error.contains("pending"), "{error}");
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.session.world().source_digest(), pending);
    assert_eq!(app.prepared.source_revision, pending);
    assert!(app.view.frame_revision > pending_generation);
    assert!(app.prepared.pick(pending_generation, 1, 1, false).is_err());
}

#[test]
fn background_click_cannot_paint_an_earlier_cell_selection() {
    let (_temporary, mut app) = application();
    app.select_empty_cell().unwrap();
    let original = app.session.world().source_digest();
    let frame = app.frame(true).unwrap();
    let background = frame.semantic_ids.iter().position(|id| *id == 0).unwrap();
    let x = u32::try_from(background % usize::try_from(frame.width).unwrap()).unwrap();
    let y = u32::try_from(background / usize::try_from(frame.width).unwrap()).unwrap();
    let result = app.click(x, y).and_then(|()| app.paint());
    assert!(result.unwrap_err().contains("no selectable semantic hit"));
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.world().source_digest(), original);
}

#[test]
fn focused_text_consumes_global_shortcuts_without_invalidating_world_generation() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    let generation = app.session.generation();
    let camera = app.view.camera.center;
    let draft = app.tools.draft_text().unwrap().to_owned();
    for key in [
        Keycode::Q,
        Keycode::Z,
        Keycode::Y,
        Keycode::W,
        Keycode::A,
        Keycode::S,
        Keycode::D,
        Keycode::Left,
    ] {
        super::native_key(&mut app, key).unwrap();
    }
    app.refresh_ui();
    assert_eq!(app.tools.draft_text(), Some(draft.as_str()));
    assert_eq!(app.session.generation(), generation);
    assert_eq!(
        app.view.camera.center.map(f64::to_bits),
        camera.map(f64::to_bits)
    );
    assert!(app.session.history().pending().is_none());
    app.tools.input(" qzywasd").unwrap();
    app.refresh_ui();
    assert_eq!(app.session.generation(), generation);
    assert!(app.tools.draft_text().unwrap().ends_with(" qzywasd"));
    super::native_key(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.draft_text().is_none());
    assert_eq!(app.session.generation(), generation);
}

#[test]
fn terrain_requires_explicit_palette_and_panel_controls_never_publish() {
    use crate::workspace_ui::Control;
    let (_temporary, mut app) = application();
    app.select_empty_cell().unwrap();
    assert!(app.paint().unwrap_err().contains("Choose"));
    let revision = app.session.history().document().revision();
    let choice = tilewright_editor_ui::tools::terrain_palette(app.session.catalog())
        .into_iter()
        .next_back()
        .unwrap();
    app.control(Control::Palette {
        grid: choice.grid,
        prototype: choice.prototype,
    })
    .unwrap();
    app.refresh_ui();
    assert_eq!(app.session.history().document().revision(), revision);
    assert!(app.session.history().pending().is_none());
    app.paint().unwrap();
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.session.history().document().revision(), revision);
}

#[test]
fn combining_text_reaches_review_without_normalization_or_source_write() {
    use crate::workspace_ui::Control;
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.select_all().unwrap();
    app.tools
        .input("Cafe\u{301} العربية עברית हिन्दी தமிழ் 青🙂")
        .unwrap();
    let draft = app.tools.draft_text().unwrap().to_owned();
    let revision = app.session.history().document().revision();
    app.control(Control::ApplyDraft).unwrap();
    assert_eq!(app.session.history().document().revision(), revision);
    assert!(app.session.history().pending().is_some());
    assert!(
        app.session
            .world()
            .object("marker_1")
            .unwrap()
            .fields
            .iter()
            .any(|(name, value)| name == "label"
                && value == &tilewright_source::FieldValue::String(draft.clone()))
    );
}

#[test]
fn input_requires_a_displayed_current_frame_and_panels_consume_blank_pixels() {
    let (_temporary, mut app) = application();
    let revision = app.session.world().source_digest();
    assert!(
        super::native_pointer(&mut app, [20.0, 20.0], (320, 240), (320, 240))
            .unwrap_err()
            .contains("displayed")
    );
    app.presented_generation = Some(app.view.frame_revision);
    super::native_pointer(&mut app, [250.0, 230.0], (320, 240), (320, 240)).unwrap();
    assert!(app.session.selection().is_none());
    assert_eq!(app.session.world().source_digest(), revision);
    app.rebuild().unwrap();
    assert!(!app.displayed_input_ready());
}

#[test]
fn composed_ui_preserves_world_geometry_and_rejects_exhausted_identities() {
    use tilewright_scene::{ResourceId, SceneBundle, SceneLimits, SemanticId};
    let (_temporary, mut app) = application();
    app.refresh_ui();
    let presentation = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap();
    for original in app.prepared.scene.quads() {
        assert!(presentation.scene.quads().contains(original));
    }
    for (resource, semantic) in [(u64::MAX, 1), (1, u32::MAX)] {
        let mut quads = vec![app.prepared.scene.quads()[0]];
        quads[0].resource_id = ResourceId(resource);
        quads[0].semantic_id = SemanticId(semantic);
        let world = SceneBundle::from_quads(
            app.view.frame_revision,
            0,
            app.prepared.scene.viewport(),
            app.prepared.scene.clear_color(),
            quads,
            SceneLimits::default(),
        )
        .unwrap();
        let error = crate::workspace_render::compose(
            &mut app.text,
            &app.ui,
            &world,
            app.prepared.resources.clone(),
        )
        .err()
        .unwrap();
        assert!(matches!(
            error,
            crate::workspace_render::CompositionError::ResourceIdentityExhausted
                | crate::workspace_render::CompositionError::SemanticIdentityExhausted
        ));
    }
}

#[test]
fn cpu_evidence_png_contains_shared_workspace_ui_and_preserves_world_revision_proof() {
    let (temporary, mut app) = application();
    let world = app.frame(false).unwrap();
    let generation = app.session.generation();
    let revision = app.session.world().source_digest();
    let composed = app.workspace_frame().unwrap();
    assert_eq!(
        (composed.width, composed.height),
        (world.width, world.height)
    );
    assert_ne!(composed.rgba8, world.rgba8);
    assert_eq!(app.session.generation(), generation);
    assert_eq!(app.session.world().source_digest(), revision);
    app.capture("workspace-cpu-proof").unwrap();
    let bytes = fs::read(temporary.path().join("evidence/workspace-cpu-proof.png")).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!(&decoded[..info.buffer_size()], composed.rgba8.as_slice());
    let proof: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary.path().join("evidence/workspace-cpu-proof.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(proof["format"], "tilewright.prototype-frame/1");
    assert_eq!(proof["path"], "cpu-reference");
    assert_eq!(proof["renderer"], "0.7.0");
    let actual_keys: std::collections::BTreeSet<_> = proof
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected_keys = [
        "format",
        "path",
        "frame_generation",
        "source_revision",
        "committed_revision",
        "catalog_revision",
        "width",
        "height",
        "status",
        "toolkit",
        "renderer",
        "pending",
    ]
    .into_iter()
    .collect();
    assert_eq!(actual_keys, expected_keys);
    assert_eq!(proof["frame_generation"], generation);
    assert_eq!(
        proof["source_revision"],
        tilewright_editor_document::revision_hex(&revision)
    );
    assert_eq!(
        proof["committed_revision"],
        tilewright_editor_document::revision_hex(&app.session.history().document().revision())
    );
}

#[test]
fn readable_controls_and_inspector_copy_fit_960_and_1280_windows() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    for size in [(960, 640), (1280, 800)] {
        app.ui = WorkspaceUi::new(size, size);
        app.refresh_ui();
        let terrain = app
            .ui
            .widgets
            .iter()
            .find(|widget| widget.control == Some(Control::Terrain))
            .unwrap();
        assert_eq!(terrain.text, "Terrain");
        for widget in &app.ui.widgets {
            let measured = app
                .text
                .layout(
                    &widget.text,
                    crate::workspace_render::text_size(widget.layout.height),
                    None,
                )
                .unwrap();
            #[allow(clippy::cast_precision_loss)]
            let available = widget.layout.width.saturating_sub(8) as f32;
            assert!(
                measured.size()[0] <= available + 0.001,
                "{} exceeds {}",
                widget.text,
                available
            );
            assert!(!widget.text.contains("Some("));
            assert!(!widget.text.contains("maximum_length"));
            assert!(!widget.text.contains("applied="));
        }
        assert!(
            app.ui
                .widgets
                .iter()
                .any(|widget| widget.text == "Inspector")
        );
        assert!(
            app.ui
                .widgets
                .iter()
                .any(|widget| widget.text == "Position  X 3  Y 2")
        );
        assert!(
            app.ui
                .widgets
                .iter()
                .any(|widget| widget.text == "Text (max 64 characters)")
        );
        assert!(app.status_label().starts_with("Ready."));
    }
}

#[test]
fn long_display_labels_elide_using_shared_measurements_and_keep_full_draft() {
    use crate::workspace_ui::{Control, Rect, WorkspaceUi};
    let mut ui = WorkspaceUi::new((960, 640), (960, 640));
    let original = "Extended field label with a long readable value".repeat(4);
    ui.widget(
        Rect {
            x: 650,
            y: 60,
            width: 290,
            height: 26,
        },
        &original,
        None,
        true,
        false,
    );
    let mut system = crate::font_pack::test_system();
    let fitted = crate::workspace_render::fit_text(&mut system, &ui.widgets[0].text, 290, 26);
    assert!(fitted.ends_with("..."));
    assert_ne!(fitted, original);
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.input(" extended visible field draft").unwrap();
    let draft = app.tools.draft_text().unwrap().to_owned();
    app.refresh_ui();
    assert_eq!(app.tools.draft_text(), Some(draft.as_str()));
    assert!(app.session.history().pending().is_none());
}

#[test]
fn blocked_render_reason_precedes_source_excerpt_and_ready_status_is_plain() {
    let (_temporary, mut app) = application();
    assert!(!app.status_label().contains("applied"));
    app.blocked = Some("Font resources could not be verified".into());
    assert_eq!(
        app.status_label(),
        "Editing blocked. Recovery details below."
    );
    let lines = app.review_lines(620);
    assert!(lines[0].starts_with("Rendering blocked:"));
    assert!(
        lines
            .join(" ")
            .contains("Font resources could not be verified")
    );
}

#[test]
fn compact_chunk_review_shows_changed_label_before_accept_and_undo_restores_exact_bytes() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let chunk_path = "planes/main/layers/0/0_0.jsonc";
    let (temporary, mut app) = application_with_source(|world| {
        let path = world.join(chunk_path);
        let source: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // Author a compact fixture retaining terrain before objects so label text
        // occurs well beyond the old 512-byte prefix. World still loads/edits it.
        let compact = format!(
            "{{\"format\":{},\"plane\":{},\"layer\":{},\"chunk\":{},\"terrain\":{},\"objects\":{}}}",
            source["format"],
            source["plane"],
            source["layer"],
            source["chunk"],
            source["terrain"],
            source["objects"]
        );
        assert!(compact.find("Trail marker").unwrap() > 1024);
        fs::write(path, compact).unwrap();
    });
    let path = temporary.path().join("world").join(chunk_path);
    let original = fs::read(&path).unwrap();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    while app.tools.draft_text().is_some_and(|text| !text.is_empty()) {
        app.tools.backspace();
    }
    app.tools.input("Review-visible changed label").unwrap();
    app.control(Control::ApplyDraft).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(app.source_review().complete);
    let rows = app.review_lines(940);
    assert!(
        rows.iter()
            .any(|row| row.starts_with("- ") && row.contains("Trail marker")),
        "{rows:?}"
    );
    let changed_row = rows
        .iter()
        .position(|row| row.contains("Review-visible changed label"))
        .unwrap();
    app.ui = WorkspaceUi::new((1280, 800), (1280, 800));
    app.review_scroll = changed_row;
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .any(|widget| widget.text.contains("Review-visible changed label"))
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    app.action(Action::Accept).unwrap();
    assert_ne!(fs::read(&path).unwrap(), original);
    app.action(Action::Undo).unwrap();
    app.action(Action::Accept).unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn changed_source_context_handles_utf8_insertions_deletions_and_shared_review_budget() {
    for (before, after) in [
        ("café old fin", "café new fin"),
        ("préfixe", "préfixe末"),
        ("préfixe末", "préfixe"),
        ("", "é"),
        ("é", ""),
    ] {
        let mut rows = Vec::new();
        let mut remaining = super::MAXIMUM_REVIEW_BYTES;
        assert!(super::append_change_review(
            &mut rows,
            before,
            after,
            &mut remaining
        ));
        assert!(rows.iter().any(|row| row.starts_with("- ")));
        assert!(rows.iter().any(|row| row.starts_with("+ ")));
        assert!(rows.iter().all(|row| row.len() < 512));
    }
    let mut rows = Vec::new();
    let mut remaining = super::MAXIMUM_REVIEW_BYTES;
    assert!(super::append_change_review(
        &mut rows,
        "old file one",
        "new file one",
        &mut remaining
    ));
    let after_first = remaining;
    assert!(super::append_change_review(
        &mut rows,
        "old file two",
        "new file two",
        &mut remaining
    ));
    assert!(remaining < after_first);
    let large = "x".repeat(super::MAXIMUM_REVIEW_BYTES);
    assert!(!super::append_change_review(
        &mut rows,
        "",
        &large,
        &mut remaining
    ));
}

#[test]
fn oversized_review_blocks_button_enter_and_scripted_accept_without_losing_pending_source() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let (temporary, mut app) = application_with_source(|world| {
        let schema_path = world.join("profile/schema/object.jsonc");
        let mut schema: serde_json::Value =
            serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
        schema["fields"]["label"]["max_length"] = serde_json::Value::from(80_000);
        fs::write(schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let path = world.join("planes/main/layers/0/0_0.jsonc");
        let mut source: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        source["objects"][0]["fields"]["label"] = serde_json::Value::String("a".repeat(70_000));
        fs::write(path, serde_json::to_vec(&source).unwrap()).unwrap();
    });
    let path = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&path).unwrap();
    // A valid large existing value makes the complete before-image exceed the
    // review budget. Keep the new value small enough for unchanged Session admission.
    app.action(Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: serde_json::Value::String("b".repeat(100)),
        expected: app.session.history().document().revision(),
    })
    .unwrap();
    let pending = app.session.world().source_digest();
    assert!(!app.source_review().complete);
    app.ui = WorkspaceUi::new((1280, 800), (320, 240));
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .any(|widget| widget.control == Some(Control::Accept) && !widget.enabled)
    );
    assert!(app.review_lines(940)[0].starts_with("Change exceeds"));
    assert!(
        app.control(Control::Accept)
            .unwrap_err()
            .contains("review limit")
    );
    acknowledge_ui(&mut app);
    assert!(
        super::native_key(&mut app, sdl3::keyboard::Keycode::Return)
            .unwrap_err()
            .contains("review limit")
    );
    assert!(
        app.action(Action::Accept)
            .unwrap_err()
            .contains("review limit")
    );
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.session.world().source_digest(), pending);
    assert_eq!(fs::read(&path).unwrap(), original);
    app.action(Action::Cancel).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn long_utf8_draft_shows_working_tail_and_caret_while_full_value_reaches_review() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let (temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.ui = WorkspaceUi::new((960, 640), (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    while app.tools.draft_text().is_some_and(|text| !text.is_empty()) {
        app.tools.backspace();
    }
    let original = fs::read(
        temporary
            .path()
            .join("world/planes/main/layers/0/0_0.jsonc"),
    )
    .unwrap();
    let prefix = "existing text ".repeat(3);
    let full = format!("{prefix}NEW-éà");
    app.tools.input(&full).unwrap();
    app.refresh_ui();
    let display = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.editable.is_some())
        .unwrap();
    assert_eq!(display.text, full);
    assert_eq!(app.tools.draft_text(), Some(full.as_str()));
    super::native_key(&mut app, sdl3::keyboard::Keycode::Backspace).unwrap();
    app.refresh_ui();
    let display = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.editable.is_some())
        .unwrap();
    assert!(display.text.ends_with("NEW-é"), "{}", display.text);
    app.tools.input("à").unwrap();
    app.refresh_ui();
    assert_eq!(app.tools.draft_text(), Some(full.as_str()));
    app.control(Control::ApplyDraft).unwrap();
    let value = app
        .session
        .world()
        .object("marker_1")
        .unwrap()
        .fields
        .iter()
        .find(|(name, _)| name == "label")
        .unwrap();
    assert_eq!(value.1, tilewright_source::FieldValue::String(full));
    assert!(
        app.review_lines(620)
            .iter()
            .any(|line| line.contains("NEW-éà"))
    );
    assert_eq!(
        fs::read(
            temporary
                .path()
                .join("world/planes/main/layers/0/0_0.jsonc")
        )
        .unwrap(),
        original
    );
}

/// Mirror the acknowledgement made only after native successful presentation.
fn acknowledge_ui(app: &mut Application) {
    app.refresh_ui();
    let presentation = super::workspace_presentation(app).unwrap();
    app.displayed_text = presentation.editable;
    app.presented_generation = Some(app.view.frame_revision);
    app.presented_text_revision = app.tools.input_revision();
}

fn input_key(window_id: u32, key: sdl3::keyboard::Keycode, up: bool) -> sdl3::event::Event {
    if up {
        sdl3::event::Event::KeyUp {
            timestamp: 0,
            window_id,
            keycode: Some(key),
            scancode: None,
            keymod: sdl3::keyboard::Mod::NOMOD,
            repeat: false,
            which: 0,
            raw: 0,
        }
    } else {
        sdl3::event::Event::KeyDown {
            timestamp: 0,
            window_id,
            keycode: Some(key),
            scancode: None,
            keymod: sdl3::keyboard::Mod::NOMOD,
            repeat: false,
            which: 0,
            raw: 0,
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn real_sdl_events_preserve_preedit_offsets_focus_and_submission_order() {
    use crate::workspace_ui::Control;
    use sdl3::{
        event::{Event, WindowEvent},
        keyboard::Keycode,
    };
    let (temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.select_all().unwrap();
    let original = fs::read(
        temporary
            .path()
            .join("world/planes/main/layers/0/0_0.jsonc"),
    )
    .unwrap();
    let committed = app.tools.draft_text().unwrap().to_owned();
    let revision = app.session.history().document().revision();
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    let preedit = Event::TextEditing {
        timestamp: 0,
        window_id: 7,
        text: "é🙂青".into(),
        start: 1,
        length: 1,
    };
    super::native_text_event(&mut app, &preedit, &mut input)
        .unwrap()
        .unwrap();
    let snapshot = app.tools.draft_snapshot().unwrap();
    assert_eq!(snapshot.composition.unwrap().selection, Some(2..6));
    assert_eq!(app.tools.draft_text(), Some(committed.as_str()));
    super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
        .unwrap()
        .unwrap();
    let commit = Event::TextInput {
        timestamp: 0,
        window_id: 7,
        text: "é🙂青".into(),
    };
    super::native_text_event(&mut app, &commit, &mut input)
        .unwrap()
        .unwrap();
    super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.draft_text(), Some("é🙂青"));
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.history().document().revision(), revision);
    assert_eq!(
        fs::read(
            temporary
                .path()
                .join("world/planes/main/layers/0/0_0.jsonc")
        )
        .unwrap(),
        original
    );
    super::native_text_event(&mut app, &input_key(7, Keycode::Return, true), &mut input)
        .unwrap()
        .unwrap();
    acknowledge_ui(&mut app);
    input.begin_batch(
        false,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
        .unwrap()
        .unwrap();
    assert!(app.session.history().pending().is_some());
    // A second Return in the same drain cannot accept the undisplayed diff.
    assert!(
        super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
            .is_none()
    );
    assert!(!app.displayed_input_ready());
    app.action(Action::Cancel).unwrap();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    let prior = app.tools.draft_text().unwrap().to_owned();
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    super::native_text_event(&mut app, &preedit, &mut input)
        .unwrap()
        .unwrap();
    super::native_text_event(
        &mut app,
        &Event::Window {
            timestamp: 0,
            window_id: 7,
            win_event: WindowEvent::FocusLost,
        },
        &mut input,
    )
    .unwrap()
    .unwrap();
    assert!(!app.tools.is_composing());
    super::native_text_event(&mut app, &commit, &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.draft_text(), Some(prior.as_str()));
    input.focused = true;
    let foreign = Event::TextInput {
        timestamp: 0,
        window_id: 8,
        text: "FOREIGN".into(),
    };
    super::native_text_event(&mut app, &foreign, &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.draft_text(), Some(prior.as_str()));
}

#[test]
fn stale_displayed_geometry_blocks_pointer_but_keyboard_uses_current_draft() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.ui = WorkspaceUi::new((960, 640), (320, 240));
    app.control(Control::Field("label".into())).unwrap();
    app.refresh_ui();
    let presentation = super::workspace_presentation(&mut app).unwrap();
    app.displayed_text = presentation.editable;
    app.presented_generation = Some(app.view.frame_revision);
    app.presented_text_revision = app.tools.input_revision();
    assert!(app.displayed_input_ready());
    app.tools.input("e\u{301}").unwrap();
    assert!(!app.displayed_input_ready());
    let expected = app.tools.draft_text().unwrap().len() - "e\u{301}".len();
    super::visual_move(&mut app, sdl3::keyboard::Keycode::Left, false).unwrap();
    assert_eq!(
        app.tools.draft_snapshot().unwrap().selection.focus.byte,
        expected
    );
    assert!(super::native_pointer(&mut app, [650.0, 60.0], (960, 640), (320, 240)).is_err());
    assert!(app.session.history().pending().is_none());
}

#[test]
fn deletion_and_clipboard_paste_return_wait_for_displayed_draft() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::{Keycode, Mod};
    use tilewright_editor_ui::text_input::{TextCursor, TextSelection};
    for key in [Keycode::Backspace, Keycode::Delete, Keycode::V] {
        let (_temporary, mut app) = application();
        app.select_visible_object(super::Capability::Field).unwrap();
        app.control(Control::Field("label".into())).unwrap();
        app.tools.select_all().unwrap();
        app.tools.input("abc").unwrap();
        if key == Keycode::Delete {
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(0)), revision)
                .unwrap();
        }
        acknowledge_ui(&mut app);
        let source = app.session.history().document().revision();
        let mut input = crate::native_input::NativeInput::new(7);
        input.begin_batch(
            false,
            app.tools.input_revision().map(|revision| revision.draft),
        );
        let mut edit = input_key(7, key, false);
        if key == Keycode::V {
            if let sdl3::event::Event::KeyDown { keymod, .. } = &mut edit {
                *keymod = Mod::LCTRLMOD;
            }
            super::native_clipboard_event(
                &mut app,
                &edit,
                true,
                || Ok("青".into()),
                |_| panic!("paste must not write clipboard"),
            )
            .unwrap()
            .unwrap();
        } else {
            super::native_text_event(&mut app, &edit, &mut input)
                .unwrap()
                .unwrap();
        }
        let error =
            super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
                .unwrap()
                .unwrap_err();
        assert!(error.contains("displayed"));
        assert!(app.session.history().pending().is_none());
        assert_eq!(app.session.history().document().revision(), source);
        super::native_text_event(&mut app, &input_key(7, Keycode::Return, true), &mut input)
            .unwrap()
            .unwrap();
        acknowledge_ui(&mut app);
        input.begin_batch(
            false,
            app.tools.input_revision().map(|revision| revision.draft),
        );
        super::native_text_event(&mut app, &input_key(7, Keycode::Return, false), &mut input)
            .unwrap()
            .unwrap();
        assert!(app.session.history().pending().is_some());
        assert_eq!(app.session.history().document().revision(), source);
    }
}

#[test]
fn native_typing_ime_and_paste_reject_line_and_paragraph_separators() {
    use crate::workspace_ui::Control;
    use sdl3::{
        event::Event,
        keyboard::{Keycode, Mod},
    };
    for separator in ["\u{2028}", "\u{2029}"] {
        let (_temporary, mut app) = application();
        app.select_visible_object(super::Capability::Field).unwrap();
        app.control(Control::Field("label".into())).unwrap();
        acknowledge_ui(&mut app);
        let before = app.tools.draft_snapshot().unwrap();
        let mut input = crate::native_input::NativeInput::new(7);
        input.begin_batch(
            true,
            app.tools.input_revision().map(|revision| revision.draft),
        );
        let typed = Event::TextInput {
            timestamp: 0,
            window_id: 7,
            text: format!("a{separator}b"),
        };
        assert!(
            super::native_text_event(&mut app, &typed, &mut input)
                .unwrap()
                .is_err()
        );
        let preedit = Event::TextEditing {
            timestamp: 0,
            window_id: 7,
            text: separator.into(),
            start: 1,
            length: 0,
        };
        assert!(
            super::native_text_event(&mut app, &preedit, &mut input)
                .unwrap()
                .is_err()
        );
        let mut paste = input_key(7, Keycode::V, false);
        if let Event::KeyDown { keymod, .. } = &mut paste {
            *keymod = Mod::LCTRLMOD;
        }
        assert!(
            super::native_clipboard_event(
                &mut app,
                &paste,
                true,
                || Ok(format!("pasted{separator}label")),
                |_| panic!("paste must not write clipboard"),
            )
            .unwrap()
            .is_err()
        );
        assert_eq!(app.tools.draft_snapshot().unwrap(), before);
        assert!(app.session.history().pending().is_none());
    }
}

#[test]
fn switched_or_refocused_draft_rejects_previously_drained_ime_events() {
    use crate::workspace_ui::{Control, WorkspaceUi};
    use sdl3::{event::Event, mouse::MouseButton};
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.ui = WorkspaceUi::new((960, 640), (320, 240));
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    let old = app.tools.input_revision().unwrap().draft;
    // Reopening the same field is a new draft identity, just as switching fields.
    let field = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::Field("label".into())))
        .unwrap()
        .layout;
    let pointer = Event::MouseButtonDown {
        timestamp: 20,
        window_id: 7,
        which: 0,
        mouse_btn: MouseButton::Left,
        clicks: 1,
        x: f32::from(u16::try_from(field.x).unwrap()) + 2.0,
        y: f32::from(u16::try_from(field.y).unwrap()) + 2.0,
    };
    super::native_pointer_event(&mut app, &pointer, (960, 640), (320, 240), false).unwrap();
    assert_ne!(old, app.tools.input_revision().unwrap().draft);
    let original = app.tools.draft_text().unwrap().to_owned();
    for event in [
        Event::TextInput {
            timestamp: 30,
            window_id: 7,
            text: "OLD".into(),
        },
        Event::TextEditing {
            timestamp: 31,
            window_id: 7,
            text: "OLD".into(),
            start: 0,
            length: 1,
        },
    ] {
        super::native_text_event(&mut app, &event, &mut input)
            .unwrap()
            .unwrap();
    }
    assert_eq!(app.tools.draft_text(), Some(original.as_str()));
    assert!(!app.tools.is_composing());
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    input.refocus(40);
    let delayed = Event::TextInput {
        timestamp: 50,
        window_id: 7,
        text: "OLD".into(),
    };
    super::native_text_event(&mut app, &delayed, &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.draft_text(), Some(original.as_str()));
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    let fresh = Event::TextInput {
        timestamp: 60,
        window_id: 7,
        text: "NEW".into(),
    };
    super::native_text_event(&mut app, &fresh, &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(
        app.tools.draft_text(),
        Some(format!("{original}NEW").as_str())
    );
    assert!(app.session.history().pending().is_none());
}

#[test]
fn escape_composition_cancel_fences_queued_text_and_allows_fresh_input() {
    use crate::workspace_ui::Control;
    use sdl3::{event::Event, keyboard::Keycode};
    let (temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.select_all().unwrap();
    let committed = app.tools.draft_snapshot().unwrap();
    let path = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let source = fs::read(&path).unwrap();
    let revision = app.session.history().document().revision();
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(true, Some(committed.revision.draft));
    let preedit = Event::TextEditing {
        timestamp: 1,
        window_id: 7,
        text: "OLD".into(),
        start: 0,
        length: 1,
    };
    super::native_text_event(&mut app, &preedit, &mut input)
        .unwrap()
        .unwrap();
    assert!(app.tools.is_composing());
    super::native_text_event(&mut app, &input_key(7, Keycode::Escape, false), &mut input)
        .unwrap()
        .unwrap();
    assert!(!app.tools.is_composing());
    assert_eq!(
        app.tools.draft_snapshot().unwrap().selection,
        committed.selection
    );
    for queued in [
        Event::TextInput {
            timestamp: 3,
            window_id: 7,
            text: "OLD".into(),
        },
        preedit,
    ] {
        super::native_text_event(&mut app, &queued, &mut input)
            .unwrap()
            .unwrap();
    }
    assert_eq!(app.tools.draft_text(), Some(committed.committed.as_str()));
    assert!(!app.tools.is_composing());
    assert!(input.focused);
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.history().document().revision(), revision);
    assert_eq!(fs::read(&path).unwrap(), source);
    input.begin_batch(
        true,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    let fresh_preedit = Event::TextEditing {
        timestamp: 4,
        window_id: 7,
        text: "NEW".into(),
        start: 1,
        length: 1,
    };
    super::native_text_event(&mut app, &fresh_preedit, &mut input)
        .unwrap()
        .unwrap();
    assert!(app.tools.is_composing());
    // Backends may end preedit with an empty edit before delivering the commit.
    let natural_end = Event::TextEditing {
        timestamp: 5,
        window_id: 7,
        text: String::new(),
        start: -1,
        length: -1,
    };
    super::native_text_event(&mut app, &natural_end, &mut input)
        .unwrap()
        .unwrap();
    assert!(!app.tools.is_composing());
    assert_eq!(app.tools.draft_text(), Some(committed.committed.as_str()));
    let fresh_commit = Event::TextInput {
        timestamp: 5,
        window_id: 7,
        text: "NEW".into(),
    };
    super::native_text_event(&mut app, &fresh_commit, &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.draft_text(), Some("NEW"));
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(path).unwrap(), source);
}

fn dispatch_draft_event(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    event: &sdl3::event::Event,
) -> Result<bool, String> {
    let scroll = (app.sidebar_scroll, app.review_scroll);
    super::native_text_event(app, event, input).unwrap_or(Ok(()))?;
    Ok(super::update_input_presentation(app, scroll))
}
fn modified_input_key(
    key: sdl3::keyboard::Keycode,
    up: bool,
    modifiers: sdl3::keyboard::Mod,
) -> sdl3::event::Event {
    let mut event = input_key(7, key, up);
    if let sdl3::event::Event::KeyDown { keymod, .. } | sdl3::event::Event::KeyUp { keymod, .. } =
        &mut event
    {
        *keymod = modifiers;
    }
    event
}

#[test]
fn real_shift_and_repeated_arrows_use_current_draft_without_intermediate_frames() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::{Keycode, Mod};
    use tilewright_editor_ui::text_input::{TextCursor, TextSelection};
    for (shift, arrows) in [(true, 1), (true, 2), (false, 2)] {
        let (_temporary, mut app) = application();
        app.select_visible_object(super::Capability::Field).unwrap();
        app.control(Control::Field("label".into())).unwrap();
        app.tools.select_all().unwrap();
        app.tools.input("e\u{301}a🙂b").unwrap();
        let revision = app.tools.input_revision().unwrap();
        app.tools
            .set_selection(TextSelection::caret(TextCursor::new(0)), revision)
            .unwrap();
        acknowledge_ui(&mut app);
        let source = app.session.history().document().revision();
        let world = app.session.world().source_digest();
        let camera = app.view.camera.center.map(f64::to_bits);
        let mut input = crate::native_input::NativeInput::new(7);
        input.begin_batch(
            false,
            app.tools.input_revision().map(|revision| revision.draft),
        );
        let modifiers = if shift { Mod::LSHIFTMOD } else { Mod::NOMOD };
        if shift {
            let revision = app.tools.input_revision();
            assert!(
                !dispatch_draft_event(
                    &mut app,
                    &mut input,
                    &modified_input_key(Keycode::LShift, false, modifiers)
                )
                .unwrap()
            );
            assert_eq!(app.tools.input_revision(), revision);
            assert!(app.displayed_input_ready());
        }
        for _ in 0..arrows {
            dispatch_draft_event(
                &mut app,
                &mut input,
                &modified_input_key(Keycode::Right, false, modifiers),
            )
            .unwrap();
            dispatch_draft_event(
                &mut app,
                &mut input,
                &modified_input_key(Keycode::Right, true, modifiers),
            )
            .unwrap();
        }
        if shift {
            dispatch_draft_event(
                &mut app,
                &mut input,
                &modified_input_key(Keycode::LShift, true, Mod::NOMOD),
            )
            .unwrap();
        }
        let selection = app.tools.draft_snapshot().unwrap().selection;
        let expected = if arrows == 1 { 3 } else { 4 };
        assert_eq!(selection.focus.byte, expected);
        assert_eq!(selection.anchor.byte, if shift { 0 } else { expected });
        assert_eq!(app.session.history().document().revision(), source);
        assert_eq!(app.session.world().source_digest(), world);
        assert_eq!(app.view.camera.center.map(f64::to_bits), camera);
        assert!(app.session.history().pending().is_none());
    }
}

#[test]
fn backspace_then_left_uses_current_shaped_text_and_return_stays_fenced() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    let (_temporary, mut app) = application();
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    app.tools.select_all().unwrap();
    app.tools.input("e\u{301}a🙂b").unwrap();
    acknowledge_ui(&mut app);
    let source = app.session.history().document().revision();
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(
        false,
        app.tools.input_revision().map(|revision| revision.draft),
    );
    dispatch_draft_event(
        &mut app,
        &mut input,
        &input_key(7, Keycode::Backspace, false),
    )
    .unwrap();
    dispatch_draft_event(&mut app, &mut input, &input_key(7, Keycode::Left, false)).unwrap();
    let snapshot = app.tools.draft_snapshot().unwrap();
    assert_eq!(snapshot.committed, "e\u{301}a🙂");
    assert_eq!(snapshot.selection.focus.byte, 4);
    assert!(!app.displayed_input_ready());
    assert!(
        dispatch_draft_event(&mut app, &mut input, &input_key(7, Keycode::Return, false))
            .unwrap_err()
            .contains("displayed")
    );
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.history().document().revision(), source);
}

#[test]
fn regained_focus_lets_a_fresh_return_reach_world_actions_without_a_draft() {
    use sdl3::{
        event::{Event, WindowEvent},
        keyboard::Keycode,
    };
    let (_temporary, mut app) = application();
    let mut input = crate::native_input::NativeInput::new(7);
    let window = |win_event| Event::Window {
        timestamp: 5,
        window_id: 7,
        win_event,
    };
    let enter = input_key(7, Keycode::Return, false);
    input.begin_batch(false, None);
    super::native_text_event(&mut app, &window(WindowEvent::FocusLost), &mut input)
        .unwrap()
        .unwrap();
    assert!(super::native_text_event(&mut app, &enter, &mut input).is_some());
    assert!(
        super::native_text_event(&mut app, &window(WindowEvent::FocusGained), &mut input).is_none()
    );
    assert!(input.focused);
    input.begin_batch(false, None);
    // A Return held through the focus change is not a fresh press.
    assert!(super::native_text_event(&mut app, &enter, &mut input).is_some());
    super::native_text_event(&mut app, &input_key(7, Keycode::Return, true), &mut input)
        .unwrap()
        .unwrap();
    assert!(super::native_text_event(&mut app, &enter, &mut input).is_none());
}

const BRUSH_WINDOW: (u32, u32) = (1280, 800);

/// A 1280x800 workspace whose full neutral view lies beside the panels, in
/// Terrain mode with sand chosen and the current frame acknowledged.
fn brush_application() -> (tempfile::TempDir, Application) {
    use crate::workspace_ui::{Control, WorkspaceUi};
    let (temporary, mut app) = application();
    app.ui = WorkspaceUi::new(BRUSH_WINDOW, BRUSH_WINDOW);
    app.view.camera.width = BRUSH_WINDOW.0;
    app.view.camera.height = BRUSH_WINDOW.1;
    app.rebuild().unwrap();
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// A world pixel of `cell` in the exact cell mask, outside every panel.
fn cell_pixel(app: &mut Application, x: i64, y: i64) -> [f32; 2] {
    use crate::workspace_ui::Hit;
    use tilewright_editor_preview::SelectionKey;
    app.refresh_ui();
    let frame = app.frame(true).unwrap();
    let id = app
        .prepared
        .cell_picks
        .iter()
        .find_map(|(id, key)| match key {
            SelectionKey::Cell { x: cx, y: cy, .. } if (*cx, *cy) == (x, y) => Some(id.0),
            _ => None,
        })
        .unwrap();
    let width = usize::try_from(frame.width).unwrap();
    let index = frame
        .semantic_ids
        .iter()
        .enumerate()
        .position(|(index, sample)| {
            let px = u32::try_from(index % width).unwrap();
            let py = u32::try_from(index / width).unwrap();
            *sample == id && app.ui.hit(px, py) == Hit::World(px, py)
        })
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    [(index % width) as f32 + 0.5, (index / width) as f32 + 0.5]
}

fn terrain_value(app: &Application, x: i64, y: i64) -> Option<String> {
    let chunk = &app.session.world().planes()["main"].chunks[&(0, 0, 0)];
    let local = usize::try_from(y * 32 + x).unwrap();
    chunk.ground[0].value(local).map(str::to_owned)
}

#[test]
fn one_drag_paints_every_crossed_cell_into_one_reviewed_change_with_one_recovery_record() {
    let (temporary, mut app) = brush_application();
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let start = cell_pixel(&mut app, 0, 1);
    let end = cell_pixel(&mut app, 3, 1);
    super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.stroke().unwrap().len(), 1);
    assert!(app.session.history().pending().is_none());
    // A single fast sample from the first cell to the fourth leaves no gap.
    super::native_stroke_motion(&mut app, end, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.stroke().unwrap().len(), 4);
    assert!(app.status_label().starts_with("Painting 4 cells with sand"));
    assert!(app.session.history().pending().is_none());
    super::native_stroke_release(&mut app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.tools.stroke().is_none() && app.stroke_mask.is_none());
    // Release prepares exactly one reviewed diff; nothing is on disk yet.
    let pending = app.session.history().pending().unwrap();
    assert_eq!(pending.changed().count(), 1);
    assert_eq!(pending.sealed().matches(r#""op":"terrain.set""#).count(), 1);
    for x in 0..4 {
        assert_eq!(terrain_value(&app, x, 1).as_deref(), Some("sand"));
    }
    assert_eq!(terrain_value(&app, 0, 2).as_deref(), Some("grass"));
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(app.recovery.records().is_empty());
    assert!(app.source_review().complete);
    assert_eq!(app.status_label(), "Review changes, then Accept or Cancel.");
    // Accept retains one durable inverse for the whole gesture, then publishes.
    app.action(Action::Accept).unwrap();
    assert_eq!(app.recovery.records().len(), 1);
    assert_eq!(app.session.history().depths(), (1, 0));
    let painted = fs::read(&chunk).unwrap();
    assert_ne!(painted, original);
    // Undo restores every stroke cell in one reviewed step.
    app.action(Action::Undo).unwrap();
    assert_eq!(terrain_value(&app, 2, 1).as_deref(), Some("grass"));
    app.action(Action::Accept).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(app.recovery.records().len(), 2);
    app.action(Action::Redo).unwrap();
    app.action(Action::Accept).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), painted);
}

#[test]
fn drag_misses_panels_joins_across_them_and_a_missed_release_finishes_the_stroke() {
    let (_temporary, mut app) = brush_application();
    let start = cell_pixel(&mut app, 0, 3);
    let end = cell_pixel(&mut app, 2, 3);
    super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    // Toolbar and sidebar pixels are misses, not cells or controls.
    for panel in [[20.0, 20.0], [1270.0, 400.0], [-5.0, 9000.0]] {
        super::native_stroke_motion(&mut app, panel, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    }
    assert_eq!(
        app.tools.mode(),
        tilewright_editor_ui::tools::ToolMode::Terrain
    );
    assert_eq!(app.tools.stroke().unwrap().len(), 1);
    super::native_stroke_motion(&mut app, end, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.stroke().unwrap().len(), 3);
    // Motion without the button means the release happened elsewhere.
    super::native_stroke_motion(&mut app, start, false, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_some());
    for x in 0..3 {
        assert_eq!(terrain_value(&app, x, 3).as_deref(), Some("sand"));
    }
}

#[test]
fn escape_and_view_changes_abandon_a_stroke_without_preparing_anything() {
    use sdl3::keyboard::Keycode;
    let (_temporary, mut app) = brush_application();
    let revision = app.session.world().source_digest();
    let start = cell_pixel(&mut app, 1, 1);
    let end = cell_pixel(&mut app, 3, 1);
    super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    super::native_stroke_motion(&mut app, end, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    // World shortcuts wait for the release so the stroke keeps its frame.
    let camera = app.view.camera.center;
    for key in [Keycode::W, Keycode::Z, Keycode::Y, Keycode::RightBracket] {
        super::native_key(&mut app, key).unwrap();
    }
    assert_eq!(
        app.view.camera.center.map(f64::to_bits),
        camera.map(f64::to_bits)
    );
    assert_eq!(app.tools.brush().radius, 0);
    assert_eq!(app.tools.stroke().unwrap().len(), 3);
    super::native_key(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.stroke().is_none() && app.stroke_mask.is_none());
    super::native_stroke_release(&mut app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.session.history().pending().is_none());
    assert!(app.notice.as_deref().unwrap().contains("cancelled"));

    // A resize or rebuild replaces the mask: the stroke ends unprepared.
    acknowledge_ui(&mut app);
    super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    app.reflow().unwrap();
    assert!(app.tools.stroke().is_none() && app.stroke_mask.is_none());
    super::native_stroke_release(&mut app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.notice.as_deref(), Some(super::STROKE_INTERRUPTED));
    acknowledge_ui(&mut app);
    super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    app.rebuild().unwrap();
    assert!(app.tools.stroke().is_none());
    super::native_stroke_release(&mut app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.world().source_digest(), revision);
}

#[test]
fn brush_size_controls_clip_at_the_view_edge_and_pending_changes_fence_new_strokes() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    let (_temporary, mut app) = brush_application();
    super::native_key(&mut app, Keycode::RightBracket).unwrap();
    assert_eq!(app.tools.brush().radius, 1);
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .any(|widget| widget.text == "Brush: square 3x3")
    );
    acknowledge_ui(&mut app);
    let corner = cell_pixel(&mut app, 0, 0);
    super::native_pointer(&mut app, corner, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    // A 3x3 square at the view corner keeps its four in-bounds cells.
    assert_eq!(
        app.tools.stroke().unwrap().cells().collect::<Vec<_>>(),
        [[0, 0], [0, 1], [1, 0], [1, 1]]
    );
    super::native_stroke_release(&mut app, corner, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let pending = app.session.history().pending().unwrap().sealed().to_owned();
    assert_eq!(pending.matches(r#""at":"#).count(), 4);
    // Another drag cannot start or merge while that change is pending.
    acknowledge_ui(&mut app);
    let other = cell_pixel(&mut app, 2, 3);
    let error = super::native_pointer(&mut app, other, BRUSH_WINDOW, BRUSH_WINDOW).unwrap_err();
    assert!(error.contains("Accept or cancel"), "{error}");
    assert!(app.tools.stroke().is_none());
    assert_eq!(app.session.history().pending().unwrap().sealed(), pending);
    app.control(Control::BrushShape).unwrap();
    app.control(Control::BrushSize(-1)).unwrap();
    app.control(Control::BrushSize(-1)).unwrap();
    assert_eq!(
        app.tools.brush(),
        tilewright_editor_ui::brush::Brush {
            shape: tilewright_editor_ui::brush::BrushShape::Circle,
            radius: 0
        }
    );
}

#[test]
fn a_press_during_a_stroke_only_ends_it_visibly_wherever_it_lands() {
    let (_temporary, mut app) = brush_application();
    let start = cell_pixel(&mut app, 0, 1);
    let other = cell_pixel(&mut app, 2, 3);
    // A missed release followed by a press on the world, the Select tool,
    // a sidebar panel pixel or outside the window: the old stroke is ended
    // and reported, nothing is prepared, and the press does nothing else.
    for press in [other, [20.0, 20.0], [1270.0, 400.0], [-5.0, -5.0]] {
        acknowledge_ui(&mut app);
        super::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
        assert!(app.tools.stroke().is_some());
        let error = super::native_pointer(&mut app, press, BRUSH_WINDOW, BRUSH_WINDOW).unwrap_err();
        assert_eq!(error, super::STROKE_INTERRUPTED);
        assert_eq!(app.notice.as_deref(), Some(super::STROKE_INTERRUPTED));
        assert!(app.tools.stroke().is_none() && app.stroke_mask.is_none());
        assert_eq!(
            app.tools.mode(),
            tilewright_editor_ui::tools::ToolMode::Terrain
        );
        super::native_stroke_release(&mut app, press, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
        assert!(app.session.history().pending().is_none());
    }
    // The next press starts a fresh stroke.
    acknowledge_ui(&mut app);
    super::native_pointer(&mut app, other, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    super::native_stroke_release(&mut app, other, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(terrain_value(&app, 0, 1).as_deref(), Some("grass"));
    assert_eq!(terrain_value(&app, 2, 3).as_deref(), Some("sand"));
}

fn delete_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    use crate::workspace_ui::WorkspaceUi;
    let (temporary, mut app) = application_with_source(prepare);
    app.ui = WorkspaceUi::new(BRUSH_WINDOW, BRUSH_WINDOW);
    app.view.camera.width = BRUSH_WINDOW.0;
    app.view.camera.height = BRUSH_WINDOW.1;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

fn toolbar_delete_enabled(app: &mut Application) -> bool {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(crate::workspace_ui::Control::Delete))
        .unwrap()
        .enabled
}

#[test]
fn the_delete_key_reviews_one_deletion_with_warnings_and_text_focus_keeps_it() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    use tilewright_editor_preview::SelectionKey;
    let (temporary, mut app) = delete_application(|_| {});
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    // Nothing selected: the key is refused with a plain reason.
    let error = super::native_key(&mut app, Keycode::Delete).unwrap_err();
    assert!(error.contains("Select an object"), "{error}");
    assert!(!toolbar_delete_enabled(&mut app));

    app.select_visible_object(super::Capability::Delete)
        .unwrap();
    assert_eq!(
        app.session.selection(),
        Some(&SelectionKey::Object {
            uid: "marker_2".into()
        })
    );
    assert!(toolbar_delete_enabled(&mut app));

    // A focused text draft owns Delete: it edits the draft, never the World.
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    super::native_text_event(&mut app, &input_key(7, Keycode::Delete, false), &mut input)
        .unwrap()
        .unwrap();
    super::native_key(&mut app, Keycode::Delete).unwrap();
    assert!(app.session.history().pending().is_none());
    app.control(Control::CancelDraft).unwrap();
    acknowledge_ui(&mut app);

    // Without a draft the real key event falls through to world shortcuts.
    assert!(
        super::native_text_event(&mut app, &input_key(7, Keycode::Delete, false), &mut input)
            .is_none()
    );
    super::native_key(&mut app, Keycode::Delete).unwrap();
    assert!(app.session.history().pending().is_some());
    assert_eq!(fs::read(&chunk).unwrap(), original);
    let review = app.diff_lines();
    assert!(
        review[0].contains("marker_2") && review[0].contains("do not reuse"),
        "{review:?}"
    );
    assert!(app.source_review().complete);
    // A pending deletion fences every other preparation, Delete included.
    assert!(!toolbar_delete_enabled(&mut app));
    let error = app.delete_selected().unwrap_err();
    assert!(error.contains("pending"), "{error}");

    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_none());
    assert!(app.session.world().object("marker_2").is_none());
    assert_eq!(app.recovery.records().len(), 1);
    assert!(!fs::read_to_string(&chunk).unwrap().contains("marker_2"));
    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Z).unwrap();
    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Return).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
fn objects_without_a_prototype_grant_show_delete_disabled_and_refuse_the_key() {
    use sdl3::keyboard::Keycode;
    let (temporary, mut app) = delete_application(|world| {
        let path = world.join("preview.jsonc");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("objects");
        fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    });
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    // marker_1 keeps its per-uid movement grant, which never implies deletion.
    app.select_visible_object(super::Capability::Move).unwrap();
    assert!(!toolbar_delete_enabled(&mut app));
    let error = super::native_key(&mut app, Keycode::Delete).unwrap_err();
    assert!(error.contains("does not allow deleting"), "{error}");
    assert!(app.session.history().pending().is_none());
    assert!(app.recovery.records().is_empty());
    assert_eq!(fs::read(chunk).unwrap(), original);
}

/// A neutral fixture whose `marker_2` carries `pad` bytes of custom data,
/// with that marker deleted and pending.
fn padded_deletion(pad: usize) -> (tempfile::TempDir, Application, usize) {
    let (temporary, mut app) = delete_application(|world| {
        let path = world.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        chunk["objects"][1]["custom"] =
            serde_json::json!({"example.org": {"note": "a".repeat(pad)}});
        fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
    });
    app.select_visible_object(super::Capability::Delete)
        .unwrap();
    app.delete_selected().unwrap();
    let plan = app.session.history().pending().unwrap();
    let start = super::MAXIMUM_REVIEW_BYTES * 4;
    let mut remaining = start;
    for file in plan.changed() {
        assert!(super::append_change_review(
            &mut Vec::new(),
            file.before.as_deref().unwrap_or("<absent>"),
            file.after.as_deref().unwrap_or("<absent>"),
            &mut remaining,
        ));
    }
    (temporary, app, start - remaining)
}

#[test]
fn deletion_warnings_never_push_a_diff_just_under_the_budget_past_it() {
    let pad = 60_000;
    let (_temporary, _app, used) = padded_deletion(pad);
    let slack = 4;
    let (temporary, mut app, used) =
        padded_deletion(pad + super::MAXIMUM_REVIEW_BYTES - used - slack);
    assert!(used <= super::MAXIMUM_REVIEW_BYTES, "{used}");
    let warning = app.session.pending_warnings()[0].clone();
    // Sharing the diff budget would have made this review incomplete.
    assert!(warning.len() > super::MAXIMUM_REVIEW_BYTES - used);
    let review = app.source_review();
    assert!(review.complete);
    assert_eq!(review.lines[0], warning);
    app.refresh_ui();
    assert!(app.ui.widgets.iter().any(|widget| widget.control
        == Some(crate::workspace_ui::Control::Accept)
        && widget.enabled));
    app.action(Action::Accept).unwrap();
    assert!(app.session.world().object("marker_2").is_none());
    assert_eq!(app.recovery.records().len(), 1);
    drop(temporary);
}

fn widget_point(app: &mut Application, control: &crate::workspace_ui::Control) -> [f32; 2] {
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(control))
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ]
}

fn toolbar_place_enabled(app: &mut Application) -> bool {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(crate::workspace_ui::Control::Place))
        .unwrap()
        .enabled
}

#[test]
fn p_a_palette_row_and_a_tile_click_review_one_placement_that_accepts_and_undoes() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    use tilewright_editor_preview::SelectionKey;
    use tilewright_editor_ui::tools::ToolMode;
    let (temporary, mut app) = delete_application(|_| {});
    app.session
        .set_uid_source(Box::new(tilewright_editor_ui::uid::SeededUidSource::new(7)));
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    assert!(toolbar_place_enabled(&mut app));

    // A focused text draft keeps P for itself.
    app.select_visible_object(super::Capability::Field).unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    super::native_text_event(&mut app, &input_key(7, Keycode::P, false), &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Select);
    app.control(Control::CancelDraft).unwrap();
    acknowledge_ui(&mut app);

    // Without a draft the real key event reaches the world shortcut.
    assert!(
        super::native_text_event(&mut app, &input_key(7, Keycode::P, false), &mut input).is_none()
    );
    super::native_key(&mut app, Keycode::P).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Place);
    // The first palette row is chosen on entry.
    assert_eq!(app.tools.placement_choice(), Some("bush"));
    app.refresh_ui();
    let rows: Vec<_> = app
        .ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
        .map(|widget| (widget.text.clone(), widget.active))
        .collect();
    assert_eq!(
        rows,
        [
            ("bush / item".to_owned(), true),
            ("crate / item".to_owned(), false),
            ("lamp / item".to_owned(), false),
            ("marker / item".to_owned(), false),
            ("stone / item".to_owned(), false)
        ]
    );
    assert_eq!(app.status_label(), "Ready. Click a tile to place bush.");

    // A palette row click chooses its prototype.
    acknowledge_ui(&mut app);
    let row = widget_point(&mut app, &Control::PlacePalette("stone".into()));
    super::native_pointer(&mut app, row, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.placement_choice(), Some("stone"));
    assert!(app.session.history().pending().is_none());

    // A click on an empty tile prepares exactly one reviewed insertion.
    acknowledge_ui(&mut app);
    let tile = cell_pixel(&mut app, 6, 1);
    super::native_pointer(&mut app, tile, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let pending = app.session.history().pending().unwrap();
    assert_eq!(pending.changed().count(), 1);
    assert_eq!(
        pending.sealed().matches(r#""op":"object.insert""#).count(),
        1
    );
    let sealed = pending.sealed().to_owned();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    let review = app.diff_lines();
    assert!(
        review[0].starts_with("Adds stone at tile (6, 1) of main layer 0, with uid o."),
        "{review:?}"
    );
    assert!(app.source_review().complete);
    // A second click while the review is pending prepares nothing new.
    acknowledge_ui(&mut app);
    let other = cell_pixel(&mut app, 5, 1);
    let error = super::native_pointer(&mut app, other, BRUSH_WINDOW, BRUSH_WINDOW).unwrap_err();
    assert!(error.contains("pending"), "{error}");
    assert_eq!(app.session.history().pending().unwrap().sealed(), sealed);

    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.recovery.records().len(), 1);
    let Some(SelectionKey::Object { uid: placed }) = app.session.selection().cloned() else {
        panic!("the placed object is selected after Accept");
    };
    assert_eq!(app.session.world().object(&placed).unwrap().proto, "stone");
    assert!(fs::read_to_string(&chunk).unwrap().contains(&placed));
    // The placed object is deletable through the same per-prototype grant.
    assert!(toolbar_delete_enabled(&mut app));

    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Z).unwrap();
    acknowledge_ui(&mut app);
    super::native_key(&mut app, Keycode::Return).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
fn without_a_placeable_prototype_place_is_disabled_with_a_plain_reason() {
    use crate::workspace_ui::Control;
    use sdl3::keyboard::Keycode;
    use tilewright_editor_ui::tools::ToolMode;
    let (_temporary, mut app) = delete_application(|world| {
        let path = world.join("preview.jsonc");
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("objects");
        fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    });
    assert!(!toolbar_place_enabled(&mut app));
    let error = super::native_key(&mut app, Keycode::P).unwrap_err();
    assert_eq!(error, super::NOTHING_TO_PLACE);
    assert!(app.control(Control::Place).is_err());
    assert_eq!(app.tools.mode(), ToolMode::Select);
    assert!(app.session.history().pending().is_none());
}

#[path = "palette.rs"]
mod palette;

#[path = "stroke_preview.rs"]
mod stroke_preview;
