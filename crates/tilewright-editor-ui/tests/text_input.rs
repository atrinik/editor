// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use tilewright_editor_ui::{
    inspector::MAX_INPUT_BYTES,
    text_input::{TextAffinity, TextCursor, TextInput, TextSelection, scalar_selection_bytes},
};

fn select(input: &mut TextInput, anchor: usize, focus: usize) {
    input
        .set_selection(
            TextSelection {
                anchor: TextCursor::new(anchor),
                focus: TextCursor::new(focus),
            },
            input.revision(),
        )
        .unwrap();
}

#[test]
fn editing_preserves_exact_utf8_and_replaces_grapheme_selection_in_middle() {
    let original = "é e\u{301} العربية 123 עברית क्\u{200d}ष 👨‍👩‍👧‍👦 🇺🇳";
    let mut input = TextInput::new(original.into()).unwrap();
    assert_eq!(input.text(), original);
    input.select_all().unwrap();
    assert_eq!(input.text(), original, "select-all must not delete input");
    input.input("Ae\u{301}B").unwrap();
    select(&mut input, 1, 4);
    input.input("青").unwrap();
    assert_eq!(input.text(), "A青B");
    select(&mut input, 1, 1);
    input.input("עברית").unwrap();
    assert_eq!(input.text(), "Aעברית青B");
}

#[test]
fn both_deletions_remove_whole_extended_graphemes() {
    for grapheme in ["e\u{301}", "👨‍👩‍👧‍👦", "🇺🇳", "क्ष", "👍🏽", "שׁ"]
    {
        let mut input = TextInput::new(format!("A{grapheme}B")).unwrap();
        select(&mut input, 1 + grapheme.len(), 1 + grapheme.len());
        input.backspace().unwrap();
        assert_eq!(input.text(), "AB", "backspace split {grapheme}");
        let mut input = TextInput::new(format!("A{grapheme}B")).unwrap();
        select(&mut input, 1, 1);
        input.delete().unwrap();
        assert_eq!(input.text(), "AB", "delete split {grapheme}");
    }
}

#[test]
fn selection_validates_utf8_graphemes_affinity_and_display_revision_atomically() {
    let mut input = TextInput::new("Ae\u{301}🙂B".into()).unwrap();
    for byte in [2, 3, 5, 6, 7, 10, usize::MAX] {
        let before = input.clone();
        let cursor = TextCursor::new(byte);
        assert!(
            input
                .set_selection(TextSelection::caret(cursor), input.revision())
                .is_err()
        );
        assert_eq!(input, before);
    }
    let old = input.revision();
    let cursor = TextCursor {
        byte: 4,
        affinity: TextAffinity::Before,
    };
    input
        .set_selection(TextSelection::caret(cursor), old)
        .unwrap();
    assert_eq!(input.selection().focus, cursor);
    assert!(
        input
            .set_selection(TextSelection::caret(TextCursor::new(0)), old)
            .is_err()
    );
    // Renderer visual navigation supplies either affinity without model-side bidi.
    let selection = TextSelection {
        anchor: cursor,
        focus: TextCursor::new(8),
    };
    input.set_selection(selection, input.revision()).unwrap();
    assert_eq!(input.selection(), selection);
    input.input("中").unwrap();
    assert_eq!(input.text(), "Ae\u{301}中B");
}

#[test]
fn budget_is_replacement_aware_and_rejection_preserves_composition_and_selection() {
    let mut input = TextInput::new("a".repeat(MAX_INPUT_BYTES)).unwrap();
    let before = input.clone();
    assert!(input.input("é").is_err());
    assert_eq!(input, before);
    select(&mut input, 0, 2);
    input.input("é").unwrap();
    assert_eq!(input.text().len(), MAX_INPUT_BYTES);
    select(&mut input, 0, 2);
    input.update_preedit("é", 1, 0).unwrap();
    let before = input.clone();
    assert!(input.update_preedit("青", 1, 0).is_err());
    assert!(input.input("青").is_err());
    assert!(input.input("\n").is_err());
    assert_eq!(input, before);
    input.input("é").unwrap();
    assert!(!input.is_composing());
    assert_eq!(input.text().len(), MAX_INPUT_BYTES);
}

#[test]
fn ime_is_a_virtual_splice_then_one_commit_over_original_selection() {
    let mut input = TextInput::new("AoldZ".into()).unwrap();
    select(&mut input, 4, 1);
    let selection = input.selection();
    input.update_preedit("é🙂青", 1, 1).unwrap();
    assert_eq!(input.text(), "AoldZ");
    assert_eq!(input.selection(), selection);
    let snapshot = input.snapshot();
    assert_eq!(snapshot.display_text, "Aé🙂青Z");
    let composition = snapshot.composition.as_ref().unwrap();
    assert_eq!(composition.replacement, 1..4);
    assert_eq!(composition.selection, Some(2..6));
    assert_eq!(composition.display_range, 1..10);
    assert_eq!(snapshot.display_selection.anchor.byte, 3);
    assert_eq!(snapshot.display_selection.focus.byte, 7);
    assert_eq!(
        snapshot.committed_cursor(TextCursor::new(10)).unwrap().byte,
        4
    );
    assert!(snapshot.committed_cursor(TextCursor::new(3)).is_err());
    input.update_preedit("אב", -1, -1).unwrap();
    assert_eq!(input.snapshot().composition.unwrap().replacement, 1..4);
    let before = input.clone();
    assert!(input.select_all().is_err());
    assert!(input.backspace().is_err());
    assert!(input.delete().is_err());
    assert_eq!(input, before);
    input.input("עברית").unwrap();
    assert_eq!(input.text(), "AעבריתZ");
    assert!(!input.is_composing());
    let revision = input.revision();
    input.cancel_preedit().unwrap();
    assert_eq!(
        input.revision(),
        revision,
        "cancel after commit cannot replay replacement"
    );
    input.update_preedit("مرحبا", 5, 0).unwrap();
    input.cancel_preedit().unwrap();
    assert_eq!(input.text(), "AעבריתZ");
}

#[test]
fn ime_offsets_are_scalars_and_bad_updates_do_not_mutate_any_state() {
    assert_eq!(scalar_selection_bytes("é🙂青", 1, 1).unwrap(), Some(2..6));
    assert_eq!(scalar_selection_bytes("é🙂青", 3, 0).unwrap(), Some(9..9));
    assert_eq!(scalar_selection_bytes("é🙂青", -1, -1).unwrap(), None);
    let mut input = TextInput::new("e\u{301}".into()).unwrap();
    input.update_preedit("🙂", 1, 0).unwrap();
    let before = input.clone();
    for (start, length) in [
        (2, 0),
        (0, 2),
        (-2, 0),
        (0, -2),
        (i32::MAX, i32::MAX),
        (-1, i32::MAX),
        (i32::MAX, -1),
    ] {
        assert!(input.update_preedit("🙂", start, length).is_err());
        assert_eq!(input, before);
    }
    input.update_preedit("", 0, 0).unwrap();
    assert!(!input.is_composing());
    assert_eq!(input.text(), "e\u{301}");
}

#[test]
fn grapheme_joins_keep_caret_valid_without_rewriting_raw_input() {
    let mut input = TextInput::new("eX".into()).unwrap();
    select(&mut input, 1, 1);
    input.input("\u{301}").unwrap();
    assert_eq!(input.text(), "e\u{301}X");
    assert_eq!(input.selection().focus.byte, 3);
    select(&mut input, 0, 0);
    input.update_preedit("\u{301}", 1, 0).unwrap();
    let snapshot = input.snapshot();
    assert_eq!(
        snapshot.display_selection.anchor,
        snapshot.display_selection.focus
    );
    // Format controls, joining marks and bidi marks remain exact user text.
    input.cancel_preedit().unwrap();
    input.input("\u{2067}עברית\u{2069}\u{200d}").unwrap();
    assert!(input.text().starts_with("\u{2067}עברית\u{2069}\u{200d}"));
}
