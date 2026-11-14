// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Every bound of the read-only reports, in one place. `docs/CLI.md` lists
//! the same numbers. A list cut at its bound is followed by `<name>_more`
//! with how many items it left out; free text cut at its bound is followed
//! by `<name>_clipped: true`.

/// Most characters of one free-text value: a title, a name, a version, a
/// field value, a diagnostic message. Identifiers are at most 128
/// characters, so they are never clipped.
pub const MAXIMUM_TEXT_CHARACTERS: usize = 256;

/// Most bytes of one printed report. The lists' bounds keep every report
/// well below it; a report past it is refused with `output.budget` rather
/// than cut.
pub const MAXIMUM_OUTPUT_BYTES: usize = 1 << 20;

/// `overview`: planes, spatial layers of a plane, terrains of the profile.
pub const MAXIMUM_PLANES: usize = 64;
pub const MAXIMUM_LAYERS: usize = 64;
pub const MAXIMUM_TERRAINS: usize = 64;

/// `overview`: zones of the catalog's view, and the entries of each grant
/// list (per-uid moves and fields, terrain grids and their values,
/// per-prototype object grants).
pub const MAXIMUM_VIEW_ZONES: usize = 64;
pub const MAXIMUM_GRANTS: usize = 64;

/// `tile`: objects on the tile, fields of one object, granted field names
/// of one object, and portals that lead to the tile.
pub const MAXIMUM_TILE_OBJECTS: usize = 16;
pub const MAXIMUM_FIELDS: usize = 16;
pub const MAXIMUM_ARRIVALS: usize = 8;

/// `tile`: anchors on the tile, as the desktop inspector lists them.
pub const MAXIMUM_TILE_ANCHORS: usize = tilewright_editor_ui::anchors::MAXIMUM_TILE_ANCHORS;

/// `catalog`: prototypes, terrain choices and zones, as many as a desktop
/// palette offers.
pub const MAXIMUM_PALETTE: usize = tilewright_editor_ui::inspector::MAX_CHOICES;

/// `catalog`: most UTF-8 bytes of `--match`, as the Place palette filter.
pub const MAXIMUM_MATCH_BYTES: usize = tilewright_editor_ui::tools::MAX_FILTER_BYTES;

/// `lint`: findings listed; every finding is still counted by severity.
pub const MAXIMUM_FINDINGS: usize = 256;

/// Load failures: diagnostics listed. World keeps at most
/// `tilewright_editor_document::MAXIMUM_DIAGNOSTICS` (256) itself.
pub const MAXIMUM_DIAGNOSTICS: usize = tilewright_editor_document::MAXIMUM_DIAGNOSTICS;
