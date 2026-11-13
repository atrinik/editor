// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The zone management drafts and lists of the tool state: bound to the zone
//! they were opened for, ended when that zone stops being chosen or defined,
//! and capped like the zone palette.
use std::{fs, path::Path};
use tilewright_editor_commands::History;
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::Session;
use tilewright_editor_ui::tools::{ToolMode, ToolState, ZoneChoice, ZoneDraft};

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
fn tools_for(session: &Session, zone: &str) -> ToolState {
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Zone);
    tools
        .choose_zone(
            session.world(),
            session.catalog(),
            ZoneChoice::Zone(zone.into()),
        )
        .unwrap();
    tools
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}

#[test]
fn choosing_another_zone_ends_its_drafts_and_the_next_edit_targets_the_new_one() {
    let (_dir, session) = fixture_with(|_| {});
    let (world, catalog) = (session.world(), session.catalog());
    let mut tools = tools_for(&session, "meadow");
    tools
        .begin_zone_name(world, catalog, revision(&session), false)
        .unwrap();
    assert_eq!(
        tools.zone_draft_target(),
        Some((ZoneDraft::Name, Some("meadow")))
    );
    tools
        .choose_zone(world, catalog, ZoneChoice::Zone("path".into()))
        .unwrap();
    assert!(tools.draft_text().is_none(), "the meadow draft ended");
    // Choosing the same zone again keeps a draft.
    tools
        .begin_zone_name(world, catalog, revision(&session), false)
        .unwrap();
    tools
        .choose_zone(world, catalog, ZoneChoice::Zone("path".into()))
        .unwrap();
    assert!(tools.draft_text().is_some());
    tools.select_all().unwrap();
    tools.input("Wide path").unwrap();
    let Action::RenameZone { zone, name, .. } = tools.zone_action(world, catalog, false).unwrap()
    else {
        panic!("a rename");
    };
    assert_eq!(
        (zone.as_str(), name.as_deref()),
        ("path", Some("Wide path"))
    );
    // A new-zone draft ends the same way; leaving Zone mode ends both.
    tools
        .begin_zone_create(
            world,
            catalog,
            Some("path".into()),
            revision(&session),
            false,
        )
        .unwrap();
    assert_eq!(
        tools.zone_draft_target(),
        Some((ZoneDraft::Create, Some("path")))
    );
    tools
        .choose_zone(world, catalog, ZoneChoice::Default)
        .unwrap();
    assert!(tools.draft_text().is_none());
    let mut tools = tools_for(&session, "meadow");
    tools
        .begin_zone_name(world, catalog, revision(&session), false)
        .unwrap();
    tools.set_mode(ToolMode::Select);
    assert!(tools.draft_text().is_none());
}

#[test]
fn a_chosen_zone_the_shown_world_no_longer_defines_is_no_longer_chosen() {
    let (dir, session) = fixture_with(|_| {});
    let mut tools = tools_for(&session, "meadow.pond");
    tools
        .toggle_parent_choices(session.world(), session.catalog())
        .unwrap();
    tools
        .begin_zone_name(
            session.world(),
            session.catalog(),
            revision(&session),
            false,
        )
        .unwrap();
    // Still defined: nothing changes.
    tools.revalidate_zones(session.world(), session.catalog());
    assert!(tools.parent_choices_open() && tools.draft_text().is_some());
    // A World where it is gone (as after a delete or an undone creation).
    let (_other, without) = fixture_with(|root| {
        let path = root.join("planes/main/zones.jsonc");
        let mut zones: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .retain(|zone| zone["id"] != "meadow.pond");
        fs::write(path, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
        let chunk = root.join("planes/main/layers/0/0_0.jsonc");
        let text = fs::read_to_string(&chunk).unwrap();
        fs::write(
            &chunk,
            text.replace(".pp.....", "........")
                .replace("\"p\": \"meadow.pond\",\n", ""),
        )
        .unwrap();
    });
    drop(dir);
    tools.revalidate_zones(without.world(), without.catalog());
    assert_eq!(tools.zone_choice(), None);
    assert!(!tools.parent_choices_open());
    assert!(tools.draft_text().is_none());
}

#[test]
fn the_parent_list_is_capped_like_the_palette_and_follows_definition_changes() {
    let (_dir, session) = fixture_with(|root| {
        let path = root.join("planes/main/zones.jsonc");
        let mut zones: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let list = zones["zones"].as_array_mut().unwrap();
        for n in 0..300 {
            list.push(serde_json::json!({"id": format!("z{n:03}")}));
        }
        fs::write(path, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
    });
    let mut tools = tools_for(&session, "meadow");
    tools
        .toggle_parent_choices(session.world(), session.catalog())
        .unwrap();
    let choices = tools.parent_choices().unwrap();
    // Every zone but meadow and its child: 301 in all, 256 listed.
    assert_eq!((choices.zones.len(), choices.total), (256, 301));
    assert!(
        !choices
            .zones
            .iter()
            .any(|zone| zone == "meadow" || zone == "meadow.pond")
    );
    // Closing and reopening recomputes; another choice closes it.
    tools
        .toggle_parent_choices(session.world(), session.catalog())
        .unwrap();
    assert!(!tools.parent_choices_open());
    tools
        .toggle_parent_choices(session.world(), session.catalog())
        .unwrap();
    tools
        .choose_zone(
            session.world(),
            session.catalog(),
            ZoneChoice::Zone("path".into()),
        )
        .unwrap();
    assert!(!tools.parent_choices_open());
}

#[test]
fn a_new_zone_name_is_trimmed_and_only_whitespace_is_no_name() {
    let (_dir, session) = fixture_with(|_| {});
    let (world, catalog) = (session.world(), session.catalog());
    for (typed, expected) in [
        ("grove   ", None),
        ("grove  Old grove  ", Some("Old grove")),
        ("grove \u{2003} ", None),
        ("grove", None),
        ("grove Grove", Some("Grove")),
    ] {
        let mut tools = tools_for(&session, "meadow");
        tools
            .begin_zone_create(world, catalog, None, revision(&session), false)
            .unwrap();
        tools
            .input(typed)
            .unwrap_or_else(|error| panic!("{typed:?}: {error}"));
        let Action::CreateZone { zone, name, .. } =
            tools.zone_action(world, catalog, false).unwrap()
        else {
            panic!("a create");
        };
        assert_eq!(
            (zone.as_str(), name.as_deref()),
            ("grove", expected),
            "{typed:?}"
        );
    }
    // A name draft keeps its text exactly, as every field draft does.
    let mut tools = tools_for(&session, "meadow");
    tools
        .begin_zone_name(world, catalog, revision(&session), false)
        .unwrap();
    tools.select_all().unwrap();
    tools.input(" Meadow ").unwrap();
    let Action::RenameZone { name, .. } = tools.zone_action(world, catalog, false).unwrap() else {
        panic!("a rename");
    };
    assert_eq!(name.as_deref(), Some(" Meadow "));
}
