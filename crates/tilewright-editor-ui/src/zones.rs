// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only zone resolution for display: which zone a tile is in, its
//! parent chain and its display name, and the overlay tint each zone in the
//! view gets. World stores a zone per tile and layer in each chunk's `zone`
//! grid; an empty cell, or a chunk without a zone grid, is in the plane's
//! default zone. The display name is the first named zone walking up from
//! the tile's zone (FORMAT.md section 8). World has no helper for that walk,
//! so it is here; nothing in this module edits anything.
use std::collections::{BTreeMap, BTreeSet};
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::{Plane, World};

/// Most zones a parent chain is walked through. World refuses cycles, so a
/// real chain ends long before this; the bound keeps the walk finite on any
/// input.
pub const MAXIMUM_DEPTH: usize = 64;

/// The zone of one tile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileZone<'w> {
    /// The tile's zone id: its cell's value, else the plane's default zone.
    pub id: &'w str,
    /// Whether the tile's cell is empty and the zone is the plane's default.
    pub from_default: bool,
    /// The zone and its ancestors, innermost first. It stops before an
    /// unknown parent, a repeated zone or [`MAXIMUM_DEPTH`] entries.
    pub path: Vec<&'w str>,
    /// The first `name` along `path`, if any zone on it has one.
    pub name: Option<&'w str>,
}

/// The value of the `zone` grid cell of plane tile `x, y` on `layer`: `None`
/// for an empty cell, a missing grid or a missing chunk.
pub fn cell_zone(plane: &Plane, layer: i32, x: i64, y: i64) -> Option<&str> {
    let size = i64::from(plane.chunk_size);
    if size <= 0 {
        return None;
    }
    let chunk = plane
        .chunks
        .get(&(layer, x.div_euclid(size), y.div_euclid(size)))?;
    let local = usize::try_from(y.rem_euclid(size) * size + x.rem_euclid(size)).ok()?;
    chunk.zones.value(local)
}

/// `id` and its ancestors, innermost first. Cycle-safe and bounded: the walk
/// stops at a zone without a parent, before a parent the plane does not
/// define or one already on the path, and after [`MAXIMUM_DEPTH`] zones.
pub fn zone_path<'w>(plane: &'w Plane, id: &'w str) -> Vec<&'w str> {
    let mut path = vec![id];
    let mut seen = BTreeSet::from([id]);
    let mut current = plane.zones.get(id);
    while let Some(zone) = current {
        if path.len() >= MAXIMUM_DEPTH {
            break;
        }
        let Some(parent) = zone.parent.as_deref() else {
            break;
        };
        let Some(next) = plane.zones.get(parent) else {
            break;
        };
        if !seen.insert(parent) {
            break;
        }
        path.push(parent);
        current = Some(next);
    }
    path
}

/// The display name of zone `id`: the first `name` walking up from it.
pub fn display_name<'w>(plane: &'w Plane, id: &'w str) -> Option<&'w str> {
    zone_path(plane, id)
        .into_iter()
        .find_map(|zone| plane.zones.get(zone)?.name.as_deref())
}

/// The zone of plane tile `x, y` on `layer`, or `None` when its cell is empty
/// and the plane has no default zone.
pub fn tile_zone(plane: &Plane, layer: i32, x: i64, y: i64) -> Option<TileZone<'_>> {
    let (id, from_default) = match cell_zone(plane, layer, x, y) {
        Some(id) => (id, false),
        None => (plane.default_zone.as_deref()?, true),
    };
    Some(TileZone {
        id,
        from_default,
        path: zone_path(plane, id),
        name: display_name(plane, id),
    })
}

/// The zone id of every tile of the active view that has one, in row-major
/// order. The view holds at most the preview's cell budget (4,096 tiles).
pub fn view_zones<'w>(world: &'w World, catalog: &PreviewCatalog) -> Vec<([i64; 2], &'w str)> {
    let (plane, layer, [min_x, min_y, width, height]) = catalog.view();
    let Some(plane) = world.planes().get(plane) else {
        return Vec::new();
    };
    let mut zones = Vec::new();
    for y in min_y..min_y.saturating_add(height) {
        for x in min_x..min_x.saturating_add(width) {
            let zone = cell_zone(plane, layer, x, y).or(plane.default_zone.as_deref());
            if let Some(zone) = zone {
                zones.push(([x, y], zone));
            }
        }
    }
    zones
}

/// The overlay tints: eight saturated colours that stay distinguishable
/// from each other (they follow the Okabe–Ito colour-blind-safe set, with
/// black replaced by indigo), drawn at [`TINT_ALPHA`] over the tile. None is
/// white or grey, so the brush preview's white lightening stays visible on
/// top of every tint.
pub const TINTS: [[f32; 3]; 8] = [
    [0.90, 0.62, 0.00], // orange
    [0.34, 0.71, 0.91], // sky blue
    [0.00, 0.62, 0.45], // bluish green
    [0.94, 0.89, 0.26], // yellow
    [0.00, 0.45, 0.70], // blue
    [0.84, 0.37, 0.00], // vermilion
    [0.80, 0.47, 0.65], // reddish purple
    [0.36, 0.27, 0.62], // indigo
];
/// Opacity of a zone tint over its tile.
pub const TINT_ALPHA: f32 = 0.4;

/// The tint a zone prefers: a stable FNV-1a hash of its id into [`TINTS`],
/// the same in every view and session.
pub fn preferred_tint(id: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    usize::try_from(hash % TINTS.len() as u64).unwrap_or(0)
}

/// The tint of each of `zones` (the zones shown together). In zone id order,
/// each zone takes its [`preferred_tint`], or if an earlier zone already took
/// that one, the next free tint after it. So two zones share a tint only
/// when more than [`TINTS`]`.len()` zones are shown together, and a zone
/// keeps its preferred tint unless a zone sorted before it collides.
pub fn assign_tints<'z>(zones: impl IntoIterator<Item = &'z str>) -> BTreeMap<&'z str, usize> {
    let zones: BTreeSet<&str> = zones.into_iter().collect();
    let mut taken = [false; TINTS.len()];
    let mut tints = BTreeMap::new();
    for zone in zones {
        let preferred = preferred_tint(zone);
        let tint = (0..TINTS.len())
            .map(|step| (preferred + step) % TINTS.len())
            .find(|&tint| !taken[tint])
            .unwrap_or(preferred);
        taken[tint] = true;
        tints.insert(zone, tint);
    }
    tints
}

/// The RGBA colour of tint `index` drawn over a tile.
pub fn tint_color(index: usize) -> [f32; 4] {
    let [r, g, b] = TINTS[index % TINTS.len()];
    [r, g, b, TINT_ALPHA]
}

/// The RGBA colour of tint `index` as an opaque legend swatch.
pub fn swatch_color(index: usize) -> [f32; 4] {
    let [r, g, b] = TINTS[index % TINTS.len()];
    [r, g, b, 1.0]
}

/// At most this many target zones of one transaction are described.
const MAXIMUM_DESCRIBED: usize = 8;

/// Neutral review notes for every `terrain.set` on the grid `zone` in
/// `sealed`: how many tiles move to which zone, or back to the plane's
/// default zone, judged against the `committed` World. They inform the
/// author and never refuse or block the reviewed transaction.
pub(crate) fn notes(committed: &World, sealed: &str) -> Vec<String> {
    let limits = tilewright_jsonc::Limits::default();
    let Ok(document) = tilewright_jsonc::Document::parse(sealed.to_owned(), &limits) else {
        return Vec::new();
    };
    let Some(operations) = document
        .root()
        .get("operations")
        .and_then(tilewright_jsonc::Value::as_array)
    else {
        return Vec::new();
    };
    let text = |value: &tilewright_jsonc::Value, key: &str| {
        value
            .get(key)
            .and_then(tilewright_jsonc::Value::as_str)
            .map(str::to_owned)
    };
    // Tile counts per (plane, target zone); `None` empties the cell.
    let mut moves: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
    for operation in operations {
        if text(operation, "op").as_deref() != Some("terrain.set")
            || text(operation, "grid").as_deref() != Some("zone")
        {
            continue;
        }
        let Some(plane) = text(operation, "plane") else {
            continue;
        };
        let Some(cells) = operation
            .get("cells")
            .and_then(tilewright_jsonc::Value::as_array)
        else {
            continue;
        };
        for cell in cells {
            let zone = cell
                .get("value")
                .and_then(|value| value.as_str().map(str::to_owned));
            *moves.entry((plane.clone(), zone)).or_default() += 1;
        }
    }
    let mut lines = Vec::new();
    for (index, ((plane, zone), count)) in moves.into_iter().enumerate() {
        if index == MAXIMUM_DESCRIBED {
            lines.push("Further zone changes are not described here.".into());
            break;
        }
        let tiles = if count == 1 { "tile" } else { "tiles" };
        let plane = committed.planes().get(&plane);
        let named = |id: &str| match plane.and_then(|plane| display_name(plane, id)) {
            Some(name) => format!("{} ({})", short(id), short(name)),
            None => format!("{} (no display name)", short(id)),
        };
        lines.push(
            match (zone, plane.and_then(|p| p.default_zone.as_deref())) {
                (Some(zone), _) => format!("Zones: {count} {tiles} move to {}.", named(&zone)),
                (None, Some(default)) => format!(
                    "Zones: {count} {tiles} return to the default zone, {}.",
                    named(default)
                ),
                (None, None) => format!(
                    "Zones: {count} {tiles} lose their zone; the plane has no default zone."
                ),
            },
        );
    }
    lines
}

/// Displayed identifiers and names are cut to 64 characters.
fn short(text: &str) -> String {
    let mut chars = text.chars();
    let mut shown: String = chars.by_ref().take(64).collect();
    if chars.next().is_some() {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tilewright_source::{Chunk, Grid, PlaneKind, Zone};

    fn zone(id: &str, parent: Option<&str>, name: Option<&str>) -> (String, Zone) {
        (
            id.into(),
            Zone {
                id: id.into(),
                parent: parent.map(Into::into),
                name: name.map(Into::into),
                canonical: String::new(),
            },
        )
    }
    fn plane(zones: Vec<(String, Zone)>, default_zone: Option<&str>) -> Plane {
        Plane {
            id: "main".into(),
            title: None,
            kind: PlaneKind::Shared,
            chunk_size: 32,
            default_zone: default_zone.map(Into::into),
            canonical: String::new(),
            zones: zones.into_iter().collect(),
            anchors: BTreeMap::new(),
            stairwells: BTreeMap::new(),
            chunks: BTreeMap::new(),
        }
    }
    /// A chunk at `(layer 0, cx, 0)` whose zone grid holds `zone` at local
    /// tile `(1, 0)` and nothing else.
    fn with_cell(mut plane: Plane, cx: i64, zone: &str) -> Plane {
        let mut cells = vec![0; 32 * 32];
        cells[1] = 1;
        plane.chunks.insert(
            (0, cx, 0),
            Chunk {
                path: String::new(),
                ground: Vec::new(),
                heights: Vec::new(),
                zones: Grid {
                    palette: vec![zone.into()],
                    cells,
                },
                objects: Vec::new(),
            },
        );
        plane
    }

    #[test]
    fn a_painted_cell_names_its_zone_and_an_unnamed_child_inherits_its_parents_name() {
        let plane = with_cell(
            plane(
                vec![
                    zone("meadow", None, Some("Quiet meadow")),
                    zone("meadow.pond", Some("meadow"), None),
                    zone("path", None, Some("Stone path")),
                ],
                Some("path"),
            ),
            0,
            "meadow.pond",
        );
        let tile = tile_zone(&plane, 0, 1, 0).unwrap();
        assert_eq!(tile.id, "meadow.pond");
        assert!(!tile.from_default);
        assert_eq!(tile.path, ["meadow.pond", "meadow"]);
        assert_eq!(tile.name, Some("Quiet meadow"));
        // A named zone is its own display name.
        assert_eq!(display_name(&plane, "path"), Some("Stone path"));
        assert_eq!(display_name(&plane, "meadow"), Some("Quiet meadow"));
    }

    #[test]
    fn an_empty_cell_and_a_missing_grid_take_the_planes_default_zone() {
        let plane = with_cell(
            plane(
                vec![
                    zone("meadow", None, Some("Quiet meadow")),
                    zone("meadow.pond", Some("meadow"), None),
                ],
                Some("meadow.pond"),
            ),
            0,
            "meadow",
        );
        // (0, 0) is an empty cell of a chunk that has a zone grid.
        let empty = tile_zone(&plane, 0, 0, 0).unwrap();
        assert_eq!(empty.id, "meadow.pond");
        assert!(empty.from_default);
        assert_eq!(empty.path, ["meadow.pond", "meadow"]);
        assert_eq!(empty.name, Some("Quiet meadow"));
        // No chunk at all, and another layer, read as empty too.
        assert_eq!(tile_zone(&plane, 0, -5, 3).unwrap().id, "meadow.pond");
        assert_eq!(tile_zone(&plane, 1, 1, 0).unwrap().id, "meadow.pond");
        assert_eq!(cell_zone(&plane, 0, -5, 3), None);
        // The painted cell keeps its own zone.
        assert_eq!(tile_zone(&plane, 0, 1, 0).unwrap().id, "meadow");
    }

    #[test]
    fn a_tile_with_no_zone_has_none_and_unnamed_chains_have_no_name() {
        let plane = with_cell(
            plane(
                vec![
                    zone("north", None, None),
                    zone("north.yard", Some("north"), None),
                ],
                None,
            ),
            -1,
            "north.yard",
        );
        assert_eq!(tile_zone(&plane, 0, 0, 0), None);
        assert_eq!(tile_zone(&plane, 0, 40, 40), None);
        // Chunk (-1, 0) holds tile -32 + 1 = -31.
        let tile = tile_zone(&plane, 0, -31, 0).unwrap();
        assert_eq!(tile.path, ["north.yard", "north"]);
        assert_eq!(tile.name, None);
    }

    #[test]
    fn walks_stop_at_cycles_unknown_parents_and_the_depth_bound() {
        // World refuses these; the walk still ends.
        let cyclic = plane(
            vec![
                zone("a", Some("b"), None),
                zone("b", Some("c"), None),
                zone("c", Some("a"), Some("C")),
                zone("self", Some("self"), None),
                zone("orphan", Some("missing"), None),
            ],
            None,
        );
        assert_eq!(zone_path(&cyclic, "a"), ["a", "b", "c"]);
        assert_eq!(display_name(&cyclic, "a"), Some("C"));
        assert_eq!(zone_path(&cyclic, "self"), ["self"]);
        assert_eq!(display_name(&cyclic, "self"), None);
        assert_eq!(zone_path(&cyclic, "orphan"), ["orphan"]);
        // An id the plane does not define has itself as its whole path.
        assert_eq!(zone_path(&cyclic, "unknown"), ["unknown"]);
        assert_eq!(display_name(&cyclic, "unknown"), None);

        let ids: Vec<String> = (0..100).map(|n| format!("z{n}")).collect();
        let chain = plane(
            ids.iter()
                .enumerate()
                .map(|(n, id)| {
                    let parent = ids.get(n + 1).map(String::as_str);
                    let name = (n == 99).then_some("Root");
                    zone(id, parent, name)
                })
                .collect(),
            None,
        );
        assert_eq!(zone_path(&chain, "z0").len(), MAXIMUM_DEPTH);
        assert_eq!(display_name(&chain, "z0"), None, "beyond the bound");
        assert_eq!(display_name(&chain, "z50"), Some("Root"));
    }

    #[test]
    fn tints_are_stable_and_distinct_until_the_set_runs_out() {
        assert_eq!(preferred_tint("meadow"), preferred_tint("meadow"));
        let few = assign_tints(["path", "meadow", "meadow.pond"]);
        let distinct: BTreeSet<usize> = few.values().copied().collect();
        assert_eq!(distinct.len(), 3);
        // The first zone in id order always gets its preferred tint, and a
        // zone's tint does not depend on the order it is listed in.
        assert_eq!(few["meadow"], preferred_tint("meadow"));
        assert_eq!(few, assign_tints(["meadow.pond", "meadow", "path"]));

        let ids: Vec<String> = (0..TINTS.len()).map(|n| format!("zone{n}")).collect();
        let full = assign_tints(ids.iter().map(String::as_str));
        assert_eq!(
            full.values().copied().collect::<BTreeSet<_>>().len(),
            TINTS.len()
        );
        let ids: Vec<String> = (0..=TINTS.len()).map(|n| format!("zone{n}")).collect();
        let over = assign_tints(ids.iter().map(String::as_str));
        assert_eq!(over.len(), TINTS.len() + 1);
        assert_eq!(
            over.values().copied().collect::<BTreeSet<_>>().len(),
            TINTS.len(),
            "one more zone than tints shares exactly one tint"
        );
        assert!(tint_color(3)[3] < 1.0 && swatch_color(3)[3] > 0.99);
    }
}
