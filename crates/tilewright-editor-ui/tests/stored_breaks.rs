// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Stored line breaks and controls in single-line drafts: shown as markers,
//! never changed by being shown.
use std::collections::BTreeSet;

use tilewright_editor_ui::text_input::{
    CONTROL_MARKER, LINE_BREAK_MARKER, PARAGRAPH_MARKER, TAB_MARKER, TextCursor, TextInput,
    TextSelection, mark_stored_breaks, stored_break_hint,
};
use unicode_segmentation::UnicodeSegmentation;

fn caret(input: &mut TextInput, byte: usize) {
    input
        .set_selection(
            TextSelection::caret(TextCursor::new(byte)),
            input.revision(),
        )
        .unwrap();
}

fn boundaries(text: &str) -> BTreeSet<usize> {
    text.grapheme_indices(true)
        .map(|(byte, _)| byte)
        .chain([text.len()])
        .collect()
}

/// Every character class and its marker, a CR LF pair as one.
const CLASSES: [(&str, char); 12] = [
    ("\n", LINE_BREAK_MARKER),
    ("\r", LINE_BREAK_MARKER),
    ("\r\n", LINE_BREAK_MARKER),
    ("\u{b}", LINE_BREAK_MARKER),
    ("\u{c}", LINE_BREAK_MARKER),
    ("\u{85}", LINE_BREAK_MARKER),
    ("\u{2028}", LINE_BREAK_MARKER),
    ("\u{2029}", PARAGRAPH_MARKER),
    ("\t", TAB_MARKER),
    ("\u{0}", CONTROL_MARKER),
    ("\u{1b}", CONTROL_MARKER),
    ("\u{7f}", CONTROL_MARKER),
];

#[test]
fn each_class_shows_one_marker_at_start_middle_end_and_side_by_side() {
    for (stored, marker) in CLASSES {
        for (value, shown) in [
            (format!("{stored}A"), format!("{marker}A")),
            (format!("A{stored}B"), format!("A{marker}B")),
            (format!("A{stored}"), format!("A{marker}")),
            (format!("{stored}{stored}"), format!("{marker}{marker}")),
            (stored.to_owned(), marker.to_string()),
        ] {
            let input = TextInput::new(value.clone()).unwrap();
            let snapshot = input.snapshot();
            assert_eq!(snapshot.display_text, shown, "{value:?}");
            assert_eq!(
                snapshot.committed, value,
                "showing must not change the draft"
            );
            assert_eq!(input.text(), value);
            assert_eq!(mark_stored_breaks(&value).0, shown);
            for unit in &snapshot.markers {
                assert_eq!(&value[unit.source.clone()], stored);
                assert_eq!(
                    unit.display, unit.glyph,
                    "no guard without a joining neighbour"
                );
            }
        }
    }
    // Mixed neighbours of different classes keep their own markers.
    let value = "\r\n\n\u{2029}\t\u{7}";
    assert_eq!(
        mark_stored_breaks(value).0,
        format!(
            "{LINE_BREAK_MARKER}{LINE_BREAK_MARKER}{PARAGRAPH_MARKER}{TAB_MARKER}{CONTROL_MARKER}"
        )
    );
    // Text without stored breaks, and format characters such as bidi marks
    // or joiners that are not controls, display unchanged.
    for plain in [
        "",
        "Trail marker",
        "e\u{301}",
        "\u{200f}עברית\u{200d}",
        "👨‍👩‍👧",
    ] {
        let (shown, markers) = mark_stored_breaks(plain);
        assert_eq!(shown, plain);
        assert!(markers.is_empty());
    }
}

/// Pieces that exercise every grapheme rule a marker could disturb: stored
/// breaks and controls of each class, prepended, combining, spacing and
/// joining marks, Hangul jamo, an Indic consonant and virama, emoji
/// modifiers and selectors, regional indicators, a stored zero-width space
/// and the marker symbols themselves typed as ordinary text.
const PARTS: [&str; 34] = [
    "\n",
    "\r\n",
    "\r",
    "\u{2028}",
    "\u{2029}",
    "\t",
    "\u{85}",
    "\u{1}",
    "\u{9f}",
    "a",
    "\u{301}",
    "\u{600}",
    "\u{110bd}",
    "\u{94d}",
    "\u{93f}",
    "\u{903}",
    "\u{915}\u{94d}",
    "\u{915}",
    "\u{1100}",
    "\u{1161}",
    "\u{11a8}",
    "\u{ac00}",
    "👍",
    "\u{1f3fd}",
    "\u{fe0f}",
    "👍\u{200d}",
    "\u{200d}",
    "🇺",
    "\u{200b}",
    "⏎",
    "¶",
    "→",
    "⬚",
    "e\u{301}",
];

/// A tiny deterministic generator, so the sample is the same on every run.
struct Seeded(u64);
impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(self.0 >> 33).unwrap() % bound
    }
}

/// Every committed boundary is shown and maps back exactly; the only extra
/// display positions are inside marker units and map to the stored
/// character's edge on the glyph's side; each glyph is its own grapheme.
fn check_round_trip(value: &str) {
    let input = TextInput::new(value.into()).unwrap();
    let snapshot = input.snapshot();
    let display = boundaries(&snapshot.display_text);
    let committed = boundaries(value);
    let mut mapped = BTreeSet::new();
    for &byte in &committed {
        let mut probe = input.clone();
        caret(&mut probe, byte);
        let shown = probe.snapshot().display_selection.focus.byte;
        assert!(display.contains(&shown), "{value:?} {byte} -> {shown}");
        assert_eq!(
            snapshot
                .committed_cursor(TextCursor::new(shown))
                .unwrap()
                .byte,
            byte,
            "{value:?}"
        );
        mapped.insert(shown);
    }
    for &byte in display.difference(&mapped) {
        let unit = snapshot
            .markers
            .iter()
            .find(|unit| unit.display.start < byte && byte < unit.display.end)
            .unwrap_or_else(|| panic!("{value:?}: stray boundary {byte}"));
        let back = snapshot
            .committed_cursor(TextCursor::new(byte))
            .unwrap()
            .byte;
        let expected = if byte <= unit.glyph.start {
            unit.source.start
        } else {
            unit.source.end
        };
        assert_eq!(back, expected, "{value:?}");
    }
    for unit in &snapshot.markers {
        assert!(display.contains(&unit.glyph.start), "{value:?}");
        assert!(display.contains(&unit.glyph.end), "{value:?}");
    }
}

#[test]
fn display_boundaries_match_committed_boundaries_exactly() {
    // Every value of up to three pieces, then a seeded sample of longer ones.
    let mut values = vec![String::new()];
    let mut checked = 0;
    for _ in 0..3 {
        values = values
            .iter()
            .flat_map(|value| PARTS.iter().map(move |part| format!("{value}{part}")))
            .collect();
        for value in &values {
            check_round_trip(value);
            checked += 1;
        }
    }
    let mut random = Seeded(0x5eed);
    for _ in 0..20_000 {
        let length = 4 + random.below(5);
        let value: String = (0..length)
            .map(|_| PARTS[random.below(PARTS.len())])
            .collect();
        check_round_trip(&value);
        checked += 1;
    }
    assert!(checked > 60_000);
}

/// The committed position of a display position outside the preedit,
/// computed independently of `committed_cursor`: markers shift positions,
/// and the preedit stands in for the replaced selection.
fn expected_committed(
    snapshot: &tilewright_editor_ui::text_input::TextInputSnapshot,
    shown: usize,
) -> usize {
    let composition = snapshot.composition.as_ref().unwrap();
    let raw = snapshot
        .markers
        .iter()
        .rev()
        .find(|unit| unit.display.end <= shown)
        .map_or(shown, |unit| unit.source.end + (shown - unit.display.end));
    let preedit =
        composition.replacement.start..composition.replacement.start + composition.text.len();
    if raw <= preedit.start {
        raw
    } else {
        assert!(raw >= preedit.end);
        composition.replacement.end + (raw - preedit.end)
    }
}

#[test]
fn positions_around_a_preedit_map_back_exactly_with_markers_on_either_side() {
    // The reviewed case: four markers before a preedit at a caret.
    let mut input = TextInput::new("\n\n\n\nbc".into()).unwrap();
    caret(&mut input, 4);
    input.update_preedit("a", 1, 0).unwrap();
    let snapshot = input.snapshot();
    let markers = LINE_BREAK_MARKER.to_string().repeat(4);
    assert_eq!(snapshot.display_text, format!("{markers}abc"));
    let range = snapshot.composition.as_ref().unwrap().display_range.clone();
    assert_eq!(range, 12..13);
    for (shown, committed) in [(0, 0), (3, 1), (12, 4), (13, 4), (14, 5), (15, 6)] {
        assert_eq!(
            snapshot
                .committed_cursor(TextCursor::new(shown))
                .unwrap()
                .byte,
            committed,
            "display {shown}"
        );
    }
    // A non-empty replaced selection, with markers before and after it.
    let mut input = TextInput::new("x\ny\u{2029}z\tw".into()).unwrap();
    input
        .set_selection(
            TextSelection {
                anchor: TextCursor::new(2),
                focus: TextCursor::new(6),
            },
            input.revision(),
        )
        .unwrap();
    input.update_preedit("QR", 2, 0).unwrap();
    let snapshot = input.snapshot();
    assert_eq!(
        snapshot.display_text,
        format!("x{LINE_BREAK_MARKER}QRz{TAB_MARKER}w")
    );
    for (shown, committed) in [(1, 1), (4, 2), (6, 6), (7, 7), (10, 8), (11, 9)] {
        assert_eq!(
            snapshot
                .committed_cursor(TextCursor::new(shown))
                .unwrap()
                .byte,
            committed,
            "display {shown}"
        );
    }
    assert!(snapshot.committed_cursor(TextCursor::new(5)).is_err());
    // A seeded sample: stored values from the pieces, a selection on
    // committed boundaries and a plain preedit, every display boundary
    // outside the preedit checked against the independent mapping.
    let mut random = Seeded(0xc0de);
    let mut checked = 0;
    while checked < 5_000 {
        let length = 1 + random.below(6);
        let value: String = (0..length)
            .map(|_| PARTS[random.below(PARTS.len())])
            .collect();
        let committed: Vec<_> = boundaries(&value).into_iter().collect();
        let a = committed[random.below(committed.len())];
        let b = committed[random.below(committed.len())];
        let mut input = TextInput::new(value.clone()).unwrap();
        input
            .set_selection(
                TextSelection {
                    anchor: TextCursor::new(a),
                    focus: TextCursor::new(b),
                },
                input.revision(),
            )
            .unwrap();
        let preedit = ["a", "青", "QR"][random.below(3)];
        input.update_preedit(preedit, 0, 0).unwrap();
        let snapshot = input.snapshot();
        let range = snapshot.composition.as_ref().unwrap().display_range.clone();
        for shown in boundaries(&snapshot.display_text) {
            if range.start < shown && shown < range.end {
                continue;
            }
            // Positions beside a guard are covered by the round-trip test.
            if snapshot
                .markers
                .iter()
                .any(|unit| unit.display.start < shown && shown < unit.display.end)
            {
                continue;
            }
            let expected = if shown == range.start {
                snapshot.composition.as_ref().unwrap().replacement.start
            } else {
                expected_committed(&snapshot, shown)
            };
            assert_eq!(
                snapshot
                    .committed_cursor(TextCursor::new(shown))
                    .map(|cursor| cursor.byte),
                Ok(expected),
                "{value:?} {a}..{b} {preedit:?} display {shown}"
            );
        }
        checked += 1;
    }
}

#[test]
fn a_guard_appears_only_beside_a_joining_neighbour() {
    let (shown, markers) = mark_stored_breaks("\n\u{301}");
    assert_eq!(shown, format!("{LINE_BREAK_MARKER}\u{200b}\u{301}"));
    assert_eq!(markers[0].display, 0..6);
    assert_eq!(markers[0].glyph, 0..3);
    let (shown, markers) = mark_stored_breaks("\u{600}\n");
    assert_eq!(shown, format!("\u{600}\u{200b}{LINE_BREAK_MARKER}"));
    assert_eq!(markers[0].display, 2..8);
    assert_eq!(markers[0].glyph, 5..8);
}

#[test]
fn backspace_and_delete_remove_exactly_one_stored_character() {
    for (stored, _) in CLASSES {
        let value = format!("Ab{stored}{stored}c");
        let first = 2;
        let second = first + stored.len();
        let expected = format!("Ab{stored}c");
        // Backspace after the second marker.
        let mut input = TextInput::new(value.clone()).unwrap();
        caret(&mut input, second + stored.len());
        input.backspace().unwrap();
        assert_eq!(input.text().as_bytes(), expected.as_bytes(), "{stored:?}");
        assert_eq!(
            input.selection(),
            TextSelection::caret(TextCursor::new(second))
        );
        // Delete before the first marker.
        let mut input = TextInput::new(value.clone()).unwrap();
        caret(&mut input, first);
        input.delete().unwrap();
        assert_eq!(input.text().as_bytes(), expected.as_bytes(), "{stored:?}");
        // The display loses exactly one marker.
        let markers = input.snapshot().markers.len();
        assert_eq!(markers, 1);
    }
}

#[test]
fn a_selection_over_a_marker_covers_the_real_characters() {
    let value = "A\r\nB\u{2028}C";
    let mut input = TextInput::new(value.into()).unwrap();
    input
        .set_selection(
            TextSelection {
                anchor: TextCursor::new(1),
                focus: TextCursor::new(7),
            },
            input.revision(),
        )
        .unwrap();
    let snapshot = input.snapshot();
    // What copy writes: the committed bytes, never the markers.
    assert_eq!(
        &snapshot.committed[snapshot.selection.range()],
        "\r\nB\u{2028}"
    );
    let shown = snapshot.display_selection.anchor.byte..snapshot.display_selection.focus.byte;
    assert_eq!(
        &snapshot.display_text[shown],
        format!("{LINE_BREAK_MARKER}B{LINE_BREAK_MARKER}")
    );
    // The caret can never sit inside a CR LF pair.
    assert!(
        input
            .set_selection(TextSelection::caret(TextCursor::new(2)), input.revision())
            .is_err()
    );
}

#[test]
fn composition_beside_a_marker_maps_both_ways() {
    let mut input = TextInput::new("A\nB".into()).unwrap();
    caret(&mut input, 2);
    input.update_preedit("\u{301}x", 2, 0).unwrap();
    let snapshot = input.snapshot();
    // The combining preedit cannot join the marker: a guard separates them.
    assert_eq!(
        snapshot.display_text,
        format!("A{LINE_BREAK_MARKER}\u{200b}\u{301}xB")
    );
    let composition = snapshot.composition.as_ref().unwrap();
    assert_eq!(
        &snapshot.display_text[composition.display_range.clone()],
        "\u{301}x"
    );
    assert_eq!(
        snapshot.display_selection.focus.byte,
        composition.display_range.end
    );
    // Before the preedit maps to its committed replacement start.
    assert_eq!(
        snapshot
            .committed_cursor(TextCursor::new(composition.display_range.start))
            .unwrap()
            .byte,
        2
    );
    assert_eq!(
        snapshot.committed_cursor(TextCursor::new(1)).unwrap().byte,
        1
    );
    input.input("\u{301}x").unwrap();
    assert_eq!(input.text(), "A\n\u{301}xB");
    assert!(input.snapshot().markers[0].display.len() > LINE_BREAK_MARKER.len_utf8());
}

#[test]
fn typed_pasted_or_composed_breaks_and_controls_are_still_refused() {
    let mut input = TextInput::new("A\nB".into()).unwrap();
    caret(&mut input, 1);
    let before = input.clone();
    for refused in [
        "\n", "\r\n", "\t", "\u{2028}", "\u{2029}", "\u{85}", "x\u{1b}",
    ] {
        assert!(input.input(refused).is_err());
        assert!(input.update_preedit(refused, 0, 0).is_err());
        assert_eq!(input, before);
    }
    // The marker glyphs themselves are ordinary text, accepted as typed.
    input.input(&LINE_BREAK_MARKER.to_string()).unwrap();
    assert_eq!(input.text(), format!("A{LINE_BREAK_MARKER}\nB"));
}

#[test]
fn the_hint_counts_breaks_and_controls_with_a_cr_lf_pair_once() {
    assert_eq!(stored_break_hint("plain"), None);
    assert_eq!(
        stored_break_hint("a\r\nb").as_deref(),
        Some("Contains 1 line break")
    );
    assert_eq!(
        stored_break_hint("a\nb\u{2029}").as_deref(),
        Some("Contains 2 line breaks")
    );
    assert_eq!(
        stored_break_hint("\t").as_deref(),
        Some("Contains 1 control character")
    );
    assert_eq!(
        stored_break_hint("\u{2028}\t\u{0}").as_deref(),
        Some("Contains 1 line break and 2 control characters")
    );
}
