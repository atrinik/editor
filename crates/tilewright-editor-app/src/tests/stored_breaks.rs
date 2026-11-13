// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A field value stored with line breaks: shown as markers on one line,
//! edited through real SDL events, reviewed with the real characters.
use super::{acknowledge_ui, application_with_source, input_key, modified_input_key};
use crate::prototype::Application;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{
    event::Event,
    keyboard::{Keycode, Mod},
};
use std::fs;
use tilewright_editor_ui::text_input::{LINE_BREAK_MARKER, PARAGRAPH_MARKER};

/// The label of `marker_1`: a line feed, a line separator and a final
/// paragraph separator.
const STORED: &str = "Trail\nmar\u{2028}ker\u{2029}";

fn stored_application() -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = application_with_source(|world| {
        let path = world.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        chunk["objects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|object| object["uid"] == "marker_1")
            .unwrap()["fields"]["label"] = serde_json::json!(STORED);
        fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
    });
    app.ui = WorkspaceUi::new((960, 640), (320, 240));
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    (temporary, app)
}

fn shown() -> String {
    format!("Trail{LINE_BREAK_MARKER}mar{LINE_BREAK_MARKER}ker{PARAGRAPH_MARKER}")
}

fn label(app: &Application) -> String {
    let value = app
        .session
        .world()
        .object("marker_1")
        .unwrap()
        .fields
        .iter()
        .find(|(name, _)| name == "label")
        .map(|(_, value)| value.clone())
        .unwrap();
    let tilewright_source::FieldValue::String(text) = value else {
        panic!("label is text")
    };
    text
}

fn key(app: &mut Application, input: &mut crate::native_input::NativeInput, key: Keycode) {
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    match crate::prototype::native_text_event(app, &input_key(7, key, false), input) {
        Some(result) => result.unwrap(),
        None => crate::prototype::native_key(app, key).unwrap(),
    }
}

fn caret(app: &Application) -> usize {
    let selection = app.tools.draft_snapshot().unwrap().selection;
    assert_eq!(selection.anchor, selection.focus);
    selection.focus.byte
}

#[test]
fn stored_breaks_show_as_markers_on_one_line_in_the_row_and_the_draft() {
    let (temporary, mut app) = stored_application();
    app.refresh_ui();
    let texts: Vec<_> = app.ui.widgets.iter().map(|w| w.text.clone()).collect();
    let row = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::Field("label".into())))
        .unwrap();
    assert_eq!(row.text, format!("label = {}", shown()));
    assert!(
        texts.iter().any(|text| text == "Contains 3 line breaks"),
        "{texts:?}"
    );
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    assert_eq!(presentation.unsupported_runs, 0, "markers are never tofu");

    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let snapshot = app.tools.draft_snapshot().unwrap();
    assert_eq!(snapshot.committed, STORED);
    assert_eq!(snapshot.display_text, shown());
    let editable = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.editable.is_some())
        .unwrap();
    assert_eq!(editable.text, shown());
    let geometry = app.displayed_text.as_ref().unwrap();
    assert!(geometry.layout.missing_glyphs().is_empty());
    let plain = app.text.layout("Trail", 18.0, None).unwrap();
    assert_eq!(
        geometry.layout.size()[1].to_bits(),
        plain.size()[1].to_bits(),
        "the field stays one line high"
    );
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    assert_eq!(presentation.unsupported_runs, 0);
    app.capture("stored-breaks").unwrap();
    assert!(
        temporary
            .path()
            .join("evidence/stored-breaks.png")
            .is_file()
    );
    // Showing it changed nothing.
    assert!(app.session.history().pending().is_none());
    assert_eq!(label(&app), STORED);
}

#[test]
fn arrows_step_over_a_marker_and_backspace_and_delete_remove_one_character() {
    let (_temporary, mut app) = stored_application();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    assert_eq!(caret(&app), STORED.len());
    // Left: before the paragraph separator, then over "ker", then over the
    // line separator in one press.
    for expected in [12 + 3, 14, 13, 12, 9] {
        key(&mut app, &mut input, Keycode::Left);
        assert_eq!(caret(&app), expected);
    }
    key(&mut app, &mut input, Keycode::Right);
    assert_eq!(caret(&app), 12);
    // Shift+Left selects exactly the separator; copy writes it, not a marker.
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        &mut app,
        &modified_input_key(Keycode::Left, false, Mod::LSHIFTMOD),
        &mut input,
    )
    .unwrap()
    .unwrap();
    let mut copied = String::new();
    crate::prototype::native_clipboard_event(
        &mut app,
        &modified_input_key(Keycode::C, false, Mod::LCTRLMOD),
        true,
        || panic!("copy must not read the clipboard"),
        |text| {
            copied = text.into();
            Ok(())
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(copied, "\u{2028}");
    key(&mut app, &mut input, Keycode::Right);
    assert_eq!(caret(&app), 12);
    key(&mut app, &mut input, Keycode::Backspace);
    assert_eq!(
        app.tools.draft_text().unwrap().as_bytes(),
        "Trail\nmarker\u{2029}".as_bytes()
    );
    assert_eq!(caret(&app), 9);
    key(&mut app, &mut input, Keycode::Home);
    for _ in 0..5 {
        key(&mut app, &mut input, Keycode::Right);
    }
    assert_eq!(caret(&app), 5);
    key(&mut app, &mut input, Keycode::Delete);
    // Typing a line feed or tab is still refused and changes nothing.
    for refused in ["a\nb", "\t"] {
        let before = app.tools.draft_snapshot().unwrap();
        input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
        let typed = Event::TextInput {
            timestamp: 0,
            window_id: 7,
            text: refused.into(),
        };
        assert!(
            crate::prototype::native_text_event(&mut app, &typed, &mut input)
                .unwrap()
                .is_err()
        );
        assert_eq!(app.tools.draft_snapshot().unwrap(), before);
    }
    acknowledge_ui(&mut app);
    app.control(Control::ApplyDraft).unwrap();
    assert!(app.session.history().pending().is_some());
    // The kept paragraph separator is valid stored data and is not removed.
    assert_eq!(label(&app).as_bytes(), "Trailmarker\u{2029}".as_bytes());
}

#[test]
fn a_click_on_either_half_of_a_marker_places_the_caret_beside_it() {
    let (_temporary, mut app) = stored_application();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let geometry = app.displayed_text.as_ref().unwrap();
    let marker = geometry.snapshot.markers[1].clone();
    assert_eq!(marker.source, 9..12);
    let cluster = geometry
        .layout
        .clusters()
        .iter()
        .find(|cluster| cluster.bytes == marker.glyph)
        .unwrap()
        .bounds;
    assert!(cluster.width > 0.0, "the marker is drawn");
    let y = geometry.origin[1] + cluster.y + cluster.height / 2.0;
    let left = geometry.origin[0] + cluster.x + cluster.width * 0.25;
    let right = geometry.origin[0] + cluster.x + cluster.width * 0.75;
    for (x, expected) in [(left, marker.source.start), (right, marker.source.end)] {
        crate::prototype::native_pointer(&mut app, [x, y], (960, 640), (320, 240)).unwrap();
        assert_eq!(caret(&app), expected);
        acknowledge_ui(&mut app);
    }
    assert_eq!(app.tools.draft_text(), Some(STORED));
}

#[test]
fn a_guarded_marker_is_never_tofu_and_arrows_cross_it_both_ways() {
    // A combining or spacing mark after a stored break would join a visible
    // marker; a zero-width guard keeps them apart, as the break did.
    for stored in ["A\n\u{301}B", "A\n\u{93f}B"] {
        let (_temporary, mut app) = application_with_source(|world| {
            let path = world.join("planes/main/layers/0/0_0.jsonc");
            let mut chunk: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            chunk["objects"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|object| object["uid"] == "marker_1")
                .unwrap()["fields"]["label"] = serde_json::json!(stored);
            fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
        });
        app.ui = WorkspaceUi::new((960, 640), (320, 240));
        app.select_visible_object(crate::prototype::Capability::Field)
            .unwrap();
        app.control(Control::Field("label".into())).unwrap();
        acknowledge_ui(&mut app);
        let geometry = app.displayed_text.as_ref().unwrap();
        assert!(geometry.snapshot.markers[0].display.len() > 3, "{stored:?}");
        assert!(geometry.layout.missing_glyphs().is_empty(), "{stored:?}");
        let mut input = crate::native_input::NativeInput::new(7);
        key(&mut app, &mut input, Keycode::Home);
        let mut previous = caret(&app);
        assert_eq!(previous, 0);
        // Arrows never stop inside a marker unit and cross it both ways. A
        // zero-width mark or a bidi affinity change may take a press of its
        // own or share a position, as the shared layout does for any text.
        for _ in 0..12 {
            key(&mut app, &mut input, Keycode::Right);
            assert!(
                caret(&app) >= previous,
                "{stored:?} went back from {previous}"
            );
            previous = caret(&app);
        }
        assert_eq!(previous, stored.len());
        for _ in 0..12 {
            key(&mut app, &mut input, Keycode::Left);
            assert!(
                caret(&app) <= previous,
                "{stored:?} went forward from {previous}"
            );
            previous = caret(&app);
        }
        assert_eq!(previous, 0);
        // From just after the break, Backspace removes the break alone.
        key(&mut app, &mut input, Keycode::Right);
        key(&mut app, &mut input, Keycode::Right);
        assert_eq!(caret(&app), 2);
        key(&mut app, &mut input, Keycode::Backspace);
        assert_eq!(
            app.tools.draft_text(),
            Some(stored.replacen('\n', "", 1).as_str())
        );
    }
}

#[test]
fn every_marker_class_lays_out_on_one_line_with_the_real_font_pack() {
    let (_temporary, mut app) = stored_application();
    let single = app.text.layout("Trail", 18.0, None).unwrap().size()[1];
    for stored in [
        // Tab, NUL, ESC, DEL and a C1 control: `→` and `⬚`.
        "a\tb\u{0}c\u{1b}d\u{7f}e\u{9f}f",
        // Guarded: a combining and a spacing mark after a break.
        "A\n\u{301}B",
        "A\n\u{93f}B",
        // Right-to-left text with a line feed and a tab.
        "\u{5e2}\u{5d1}\n\u{5e8}\u{5d9}\t\u{5ea}",
        // An Indic conjunct broken by a stored line separator.
        "\u{915}\u{94d}\u{2028}\u{937}",
        "\r\n\u{2029}\u{85}\u{b}\u{c}",
    ] {
        let display = tilewright_editor_ui::text_input::TextInput::new(stored.into())
            .unwrap()
            .snapshot()
            .display_text;
        assert!(
            !display
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')),
            "{stored:?}"
        );
        let layout = app.text.layout(&display, 18.0, None).unwrap();
        assert!(layout.missing_glyphs().is_empty(), "{stored:?} {display:?}");
        assert_eq!(
            layout.size()[1].to_bits(),
            single.to_bits(),
            "{stored:?} stays one line"
        );
    }
}
