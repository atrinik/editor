// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Every bound of the read-only reports, in one place. `docs/CLI.md` lists
//! the same numbers. A list cut at its bound is followed by `<name>_more`
//! with how many items it left out; free text cut at its bound is followed
//! by `<name>_clipped: true`.
//!
//! The list bounds are chosen so that every report fits the output budget
//! even at its worst: every list full, every identifier at World's longest
//! (128 characters, field names 64), and every free-text value 256
//! characters that each print as a 12-byte JSON surrogate-pair escape.
//! Tests build exactly that (`reports_at_every_bound_...` in
//! `tests/worlds.rs`, `the_largest_load_failure_fits_the_budget`); their
//! JSON is about 437 KB for `tile`, 387 KB for `catalog`, 279 KB for
//! `overview`, 117 KB for `lint` and 827 KB for a load failure, against a
//! budget of 1,048,576 bytes. The text form is smaller.

/// Most characters of one free-text value: a title, a name, a version, a
/// field value, a diagnostic message. Identifiers are at most 128
/// characters, so they are never clipped.
pub const MAXIMUM_TEXT_CHARACTERS: usize = 256;

/// Most bytes of one printed report. A report past it is refused with
/// `output.budget` rather than cut; the bounds below keep every report
/// under it.
pub const MAXIMUM_OUTPUT_BYTES: usize = 1 << 20;

/// `overview`: planes, spatial layers of a plane, render layers and
/// terrains of the profile.
pub const MAXIMUM_PLANES: usize = 32;
pub const MAXIMUM_LAYERS: usize = 16;
pub const MAXIMUM_RENDER_LAYERS: usize = 64;
pub const MAXIMUM_TERRAINS: usize = 64;

/// `overview`: zones of the catalog's view, and the entries of each grant
/// list (per-uid moves and fields, terrain grids and their values,
/// per-prototype object grants).
pub const MAXIMUM_VIEW_ZONES: usize = 64;
pub const MAXIMUM_GRANTS: usize = 64;

/// Field names, default names and granted field names listed for one
/// object or one prototype, and fields of one object.
pub const MAXIMUM_FIELDS: usize = 8;

/// `tile`: objects on the tile and portals that lead to it.
pub const MAXIMUM_TILE_OBJECTS: usize = 16;
pub const MAXIMUM_ARRIVALS: usize = 8;

/// `tile`: anchors on the tile, as the desktop inspector lists them.
pub const MAXIMUM_TILE_ANCHORS: usize = tilewright_editor_ui::anchors::MAXIMUM_TILE_ANCHORS;

/// `catalog`: prototypes and terrain choices, and zones (each zone carries a
/// free-text name). `--match` narrows them.
pub const MAXIMUM_PROTOTYPES: usize = 128;
pub const MAXIMUM_TERRAIN_CHOICES: usize = 128;
pub const MAXIMUM_ZONES: usize = 64;

/// `catalog`: most UTF-8 bytes of `--match`, as the Place palette filter.
pub const MAXIMUM_MATCH_BYTES: usize = tilewright_editor_ui::tools::MAX_FILTER_BYTES;

/// `lint`: findings listed; every finding is still counted by severity.
pub const MAXIMUM_FINDINGS: usize = 256;

/// Load failures: diagnostics listed. World keeps at most
/// `tilewright_editor_document::MAXIMUM_DIAGNOSTICS` (256) itself;
/// `problems` counts them all.
pub const MAXIMUM_DIAGNOSTICS: usize = 128;
