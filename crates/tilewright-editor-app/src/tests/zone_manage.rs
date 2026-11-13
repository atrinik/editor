// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Zone management through real SDL pointer, key and text events: the zone
//! inspector, the display-name and new-zone drafts, the parent list and
//! deletion, each reviewed and accepted as one World change.
use super::acknowledge_ui;
use super::zones::{button, key, route, sidebar_texts, zone_application};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::{event::Event, keyboard::Keycode};
use std::fs;
use tilewright_editor_ui::tools::{ToolMode, ZoneChoice, ZoneDraft};
use tilewright_editor_ui::zones;

const ZONES: &str = "world/planes/main/zones.jsonc";

/// Press the sidebar row whose control is `control`, scrolling the sidebar
/// until it is laid out, and fail when it is not enabled.
fn press(app: &mut Application, control: &Control) {
    acknowledge_ui(app);
    let widget = (0..64)
        .find_map(|scroll| {
            app.sidebar_scroll = scroll;
            app.refresh_ui();
            app.ui
                .widgets
                .iter()
                .find(|widget| widget.control.as_ref() == Some(control))
                .cloned()
        })
        .unwrap_or_else(|| panic!("no row for {control:?}"));
    assert!(widget.enabled, "{control:?} is disabled: {}", widget.text);
    acknowledge_ui(app);
    #[allow(clippy::cast_precision_loss)]
    let at = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    route(app, &button(true, at)).unwrap();
    route(app, &button(false, at)).unwrap();
    app.sidebar_scroll = 0;
}
/// Whether a row with `control` is laid out (at any scroll) and enabled.
fn row_enabled(app: &mut Application, control: &Control) -> Option<bool> {
    let found = (0..64).find_map(|scroll| {
        app.sidebar_scroll = scroll;
        app.refresh_ui();
        app.ui
            .widgets
            .iter()
            .find(|widget| widget.control.as_ref() == Some(control))
            .map(|widget| widget.enabled)
    });
    app.sidebar_scroll = 0;
    found
}
/// Every sidebar text at every scroll.
fn all_texts(app: &mut Application) -> Vec<String> {
    let mut texts = Vec::new();
    for scroll in 0..64 {
        app.sidebar_scroll = scroll;
        texts.extend(sidebar_texts(app));
    }
    app.sidebar_scroll = 0;
    texts
}
/// One committed SDL text event for the current draft.
fn type_text(app: &mut Application, text: &str) {
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        app,
        &Event::TextInput {
            timestamp: 1,
            window_id: 7,
            text: text.into(),
        },
        &mut input,
    )
    .unwrap()
    .unwrap();
}
fn enter(app: &mut Application) {
    acknowledge_ui(app);
    route(app, &key(Keycode::Return)).unwrap();
}
fn choose(app: &mut Application, zone: &str) {
    press(app, &Control::ZonePalette(ZoneChoice::Zone(zone.into())));
    assert_eq!(
        app.tools.zone_choice(),
        Some(&ZoneChoice::Zone(zone.into()))
    );
}
fn review_has(app: &mut Application, line: &str) -> bool {
    app.review_lines(900).iter().any(|text| text.contains(line))
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_display_name_edit_through_sdl_events_is_one_reviewed_change() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(ZONES);
    let original = fs::read(&path).unwrap();
    route(&mut app, &key(Keycode::N)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Zone);
    // Without a chosen zone: a hint and the new top-level zone row.
    let texts = all_texts(&mut app);
    assert!(texts.contains(&"Choose a zone above to manage it".to_owned()));
    assert!(texts.contains(&"Zone ids are fixed once created".to_owned()));
    choose(&mut app, "meadow");
    let texts = all_texts(&mut app);
    for line in [
        "Zone meadow (id is fixed)",
        "Name: Quiet meadow",
        "Parent: none (change)",
        "Shown on 31 tiles in view",
        "Holds no tile in the plane",
        "Child zone: meadow.pond",
        "It is the plane's default zone",
        "Delete: not while in use",
        "New zone under meadow",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    assert_eq!(row_enabled(&mut app, &Control::ZoneDelete), Some(false));
    // The child shows the name it inherits.
    choose(&mut app, "meadow.pond");
    assert!(all_texts(&mut app).contains(&"Name: none (shows Quiet meadow)".to_owned()));
    choose(&mut app, "meadow");
    // The display name row opens a draft holding the current name.
    press(&mut app, &Control::ZoneName);
    assert_eq!(app.tools.zone_draft(), Some(ZoneDraft::Name));
    assert_eq!(app.tools.draft_text(), Some("Quiet meadow"));
    assert!(sidebar_texts(&mut app).contains(&"Display name; empty = none / Enter".to_owned()));
    app.tools.select_all().unwrap();
    acknowledge_ui(&mut app);
    type_text(&mut app, "Open meadow");
    // A line separator is refused like in every single-line draft.
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    let refused = crate::prototype::native_text_event(
        &mut app,
        &Event::TextInput {
            timestamp: 2,
            window_id: 7,
            text: "\u{2028}".into(),
        },
        &mut input,
    )
    .unwrap();
    assert!(refused.is_err());
    assert_eq!(app.tools.draft_text(), Some("Open meadow"));
    enter(&mut app);
    assert!(app.tools.draft_text().is_none());
    assert!(app.session.history().pending().is_some());
    assert!(review_has(
        &mut app,
        "Renames zone meadow from 'Quiet meadow' to 'Open meadow'."
    ));
    // Pending: the legend already shows the new inherited names; nothing
    // is written yet.
    assert!(sidebar_texts(&mut app).contains(&"meadow.pond = Open meadow (4)".to_owned()));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(app.recovery.records().is_empty());
    enter(&mut app);
    assert_eq!(app.recovery.records().len(), 1);
    let changed = fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&changed).contains("\"name\": \"Open meadow\""));
    tilewright_source::load(&temporary.path().join("world")).unwrap();
    // Z undoes it byte for byte, Y redoes it.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    enter(&mut app);
    assert_eq!(fs::read(&path).unwrap(), original);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Y)).unwrap();
    enter(&mut app);
    assert_eq!(fs::read(&path).unwrap(), changed);
    assert_eq!(app.recovery.records().len(), 3);
    // An empty draft clears the name.
    press(&mut app, &Control::ZoneName);
    app.tools.select_all().unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Backspace)).unwrap();
    assert_eq!(app.tools.draft_text(), Some(""));
    enter(&mut app);
    assert!(review_has(
        &mut app,
        "Removes the display name 'Open meadow' of zone meadow"
    ));
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
}

#[test]
fn the_parent_list_leaves_out_the_zone_and_its_descendants() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(ZONES);
    route(&mut app, &key(Keycode::N)).unwrap();
    choose(&mut app, "meadow");
    press(&mut app, &Control::ZoneParents);
    assert!(app.tools.parent_choices_open());
    // `meadow` itself and its child are not offered.
    assert_eq!(
        row_enabled(&mut app, &Control::ZoneParent(Some("path".into()))),
        Some(true)
    );
    for zone in ["meadow", "meadow.pond"] {
        assert_eq!(
            row_enabled(&mut app, &Control::ZoneParent(Some(zone.into()))),
            None,
            "{zone}"
        );
    }
    // Already top level: that row is marked and disabled.
    assert_eq!(
        row_enabled(&mut app, &Control::ZoneParent(None)),
        Some(false)
    );
    // The child can move under `path`.
    choose(&mut app, "meadow.pond");
    assert!(
        !app.tools.parent_choices_open(),
        "another zone closes the list"
    );
    press(&mut app, &Control::ZoneParents);
    assert_eq!(
        row_enabled(&mut app, &Control::ZoneParent(Some("meadow".into()))),
        Some(false)
    );
    press(&mut app, &Control::ZoneParent(Some("path".into())));
    assert!(review_has(&mut app, "Moves zone meadow.pond under path."));
    enter(&mut app);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"parent\": \"path\""), "{text}");
    assert_eq!(
        zones::display_name(&app.session.world().planes()["main"], "meadow.pond"),
        Some("Stone path")
    );
    // Back to the top level.
    press(&mut app, &Control::ZoneParents);
    press(&mut app, &Control::ZoneParent(None));
    assert!(review_has(
        &mut app,
        "Makes zone meadow.pond a top-level zone."
    ));
    enter(&mut app);
    assert!(
        app.session.world().planes()["main"].zones["meadow.pond"]
            .parent
            .is_none()
    );
    assert_eq!(app.recovery.records().len(), 2);
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_new_zone_is_typed_reviewed_painted_and_deleted_once_unused() {
    let (temporary, mut app) = zone_application(|_| {});
    let path = temporary.path().join(ZONES);
    let original = fs::read(&path).unwrap();
    route(&mut app, &key(Keycode::N)).unwrap();
    choose(&mut app, "path");
    press(&mut app, &Control::ZoneCreate(Some("path".into())));
    assert_eq!(app.tools.zone_draft(), Some(ZoneDraft::Create));
    assert!(sidebar_texts(&mut app).contains(&"New id (fixed), space, name / Enter".to_owned()));
    // An invalid or used id keeps the draft open to fix it.
    acknowledge_ui(&mut app);
    type_text(&mut app, "Path");
    acknowledge_ui(&mut app);
    let error = route(&mut app, &key(Keycode::Return)).unwrap_err();
    assert!(error.contains("lowercase"), "{error}");
    assert_eq!(app.tools.draft_text(), Some("Path"));
    app.tools.select_all().unwrap();
    acknowledge_ui(&mut app);
    type_text(&mut app, "meadow");
    acknowledge_ui(&mut app);
    let error = route(&mut app, &key(Keycode::Return)).unwrap_err();
    assert!(error.contains("already has a zone"), "{error}");
    app.tools.select_all().unwrap();
    acknowledge_ui(&mut app);
    type_text(&mut app, "path.bridge Old bridge");
    enter(&mut app);
    assert!(review_has(
        &mut app,
        "Creates zone path.bridge under path, named 'Old bridge'."
    ));
    // The legend lists it while pending, with a swatch from the same tints
    // as the overlay.
    let tints = zones::plane_tints(&app.session.world().planes()["main"])
        .into_iter()
        .map(|(zone, tint)| (zone.to_owned(), tint))
        .collect::<std::collections::BTreeMap<_, _>>();
    app.refresh_ui();
    let swatch = |app: &Application, zone: &str| {
        app.ui
            .widgets
            .iter()
            .find(|widget| {
                widget.control == Some(Control::ZonePalette(ZoneChoice::Zone(zone.into())))
            })
            .and_then(|widget| widget.swatch)
    };
    for zone in ["meadow", "meadow.pond", "path", "path.bridge"] {
        assert_eq!(
            swatch(&app, zone),
            Some(zones::swatch_color(tints[zone])),
            "{zone}"
        );
    }
    // Cancel: the legend is as before.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    app.refresh_ui();
    assert_eq!(swatch(&app, "path.bridge"), None);
    assert_eq!(fs::read(&path).unwrap(), original);
    // Again, and accept.
    press(&mut app, &Control::ZoneCreate(Some("path".into())));
    acknowledge_ui(&mut app);
    type_text(&mut app, "path.bridge Old bridge");
    enter(&mut app);
    enter(&mut app);
    assert_eq!(app.recovery.records().len(), 1);
    tilewright_source::load(&temporary.path().join("world")).unwrap();
    // Paint one tile with it through the brush.
    choose(&mut app, "path.bridge");
    acknowledge_ui(&mut app);
    let at = super::cell_pixel(&mut app, 3, 3);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    enter(&mut app);
    assert_eq!(
        zones::cell_zone(&app.session.world().planes()["main"], 0, 3, 3),
        Some("path.bridge")
    );
    // Used: the Delete row is disabled and the session refuses it too.
    assert!(all_texts(&mut app).contains(&"Holds 1 tile in the plane".to_owned()));
    assert_eq!(row_enabled(&mut app, &Control::ZoneDelete), Some(false));
    let error = app.control(Control::ZoneDelete).unwrap_err();
    assert!(error.contains("1 tile holds it"), "{error}");
    assert!(app.session.history().pending().is_none());
    // Undo the paint: unused, so the row deletes it.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Z)).unwrap();
    enter(&mut app);
    assert!(all_texts(&mut app).contains(&"Delete zone (unused)".to_owned()));
    press(&mut app, &Control::ZoneDelete);
    assert!(review_has(&mut app, "Deletes unused zone path.bridge."));
    enter(&mut app);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(app.recovery.records().len(), 4);
    tilewright_source::load(&temporary.path().join("world")).unwrap();
}

#[test]
fn without_manage_there_are_no_management_rows_and_manage_alone_inspects() {
    let edit = |paint: bool, manage: bool| {
        move |world: &std::path::Path| {
            let path = world.join("preview.jsonc");
            let mut catalog: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            catalog["editing"]["zones"] = serde_json::json!({"paint": paint, "manage": manage});
            fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
        }
    };
    // Paint only: exactly as before this change.
    let (_temporary, mut app) = zone_application(edit(true, false));
    route(&mut app, &key(Keycode::N)).unwrap();
    choose(&mut app, "meadow");
    let texts = all_texts(&mut app);
    assert!(
        !texts
            .iter()
            .any(|text| text.contains("fixed") || text.starts_with("New "))
    );
    for control in [
        Control::ZoneName,
        Control::ZoneParents,
        Control::ZoneDelete,
        Control::ZoneCreate(None),
    ] {
        assert_eq!(row_enabled(&mut app, &control), None, "{control:?}");
        assert!(app.control(control).is_err());
    }
    assert!(app.session.history().pending().is_none());
    // Manage only: rows choose a zone to inspect; nothing paints.
    let (_temporary, mut app) = zone_application(edit(false, true));
    route(&mut app, &key(Keycode::N)).unwrap();
    assert!(app.status_label().contains("manage"));
    choose(&mut app, "path");
    assert!(all_texts(&mut app).contains(&"Zone path (id is fixed)".to_owned()));
    assert!(
        !sidebar_texts(&mut app)
            .iter()
            .any(|text| text.starts_with("Brush"))
    );
    acknowledge_ui(&mut app);
    let at = super::cell_pixel(&mut app, 3, 3);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    assert!(app.session.history().pending().is_none());
}

#[test]
fn a_focused_zone_draft_consumes_global_shortcuts() {
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::N)).unwrap();
    choose(&mut app, "meadow");
    press(&mut app, &Control::ZoneName);
    let generation = app.session.generation();
    let draft = app.tools.draft_text().unwrap().to_owned();
    for keycode in [
        Keycode::N,
        Keycode::O,
        Keycode::Z,
        Keycode::Y,
        Keycode::T,
        Keycode::P,
        Keycode::B,
        Keycode::LeftBracket,
        Keycode::Delete,
        Keycode::Slash,
        Keycode::W,
    ] {
        acknowledge_ui(&mut app);
        let _ = route(&mut app, &key(keycode));
    }
    assert_eq!(app.tools.draft_text(), Some(draft.as_str()));
    assert_eq!(app.tools.mode(), ToolMode::Zone);
    assert_eq!(app.session.generation(), generation);
    assert!(app.session.history().pending().is_none());
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.tools.draft_text().is_none());
}
