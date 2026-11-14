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

/// Whether the chunk holding plane tile `x, y` on `layer` exists. The zone
/// brush and the overlay leave tiles of missing chunks alone: painting one
/// would create a chunk document that holds only a zone grid.
pub fn chunk_exists(plane: &Plane, layer: i32, x: i64, y: i64) -> bool {
    let size = i64::from(plane.chunk_size);
    size > 0
        && plane
            .chunks
            .contains_key(&(layer, x.div_euclid(size), y.div_euclid(size)))
}

/// The zone id of every tile of the active view that has one, in row-major
/// order; tiles of chunks that do not exist are left out. The view holds at
/// most the preview's cell budget (4,096 tiles).
pub fn view_zones<'w>(world: &'w World, catalog: &PreviewCatalog) -> Vec<([i64; 2], &'w str)> {
    let (plane, layer, [min_x, min_y, width, height]) = catalog.view();
    let Some(plane) = world.planes().get(plane) else {
        return Vec::new();
    };
    let mut zones = Vec::new();
    for y in min_y..min_y.saturating_add(height) {
        for x in min_x..min_x.saturating_add(width) {
            if !chunk_exists(plane, layer, x, y) {
                continue;
            }
            let zone = cell_zone(plane, layer, x, y).or(plane.default_zone.as_deref());
            if let Some(zone) = zone {
                zones.push(([x, y], zone));
            }
        }
    }
    zones
}

/// How many of `zones` are each zone, for the tiles of the view: the zone
/// legend's tile counts, from the zones [`view_zones`] lists.
pub fn tile_counts<'z>(zones: impl IntoIterator<Item = &'z str>) -> BTreeMap<&'z str, usize> {
    let mut counts = BTreeMap::new();
    for zone in zones {
        *counts.entry(zone).or_default() += 1;
    }
    counts
}

/// The overlay tints: eight saturated colours that stay distinguishable
/// from each other (they follow the Okabe–Ito colour-blind-safe set, with
/// black replaced by indigo), drawn at [`TINT_ALPHA`] over the tile.
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

/// The tint of every zone the plane defines: [`assign_tints`] over all its
/// zone definitions, sorted by id. Callers pass the committed World's plane,
/// so a zone's colour changes only when zone definitions change, never
/// because of a stroke, a pending diff or what is in view. Two zones share a
/// tint only when the plane defines more zones than there are tints.
pub fn plane_tints(plane: &Plane) -> BTreeMap<&str, usize> {
    assign_tints(plane.zones.keys().map(String::as_str))
}

/// The RGBA colour of tint `index` drawn over a tile.
pub fn tint_color(index: usize) -> [f32; 4] {
    let [r, g, b] = TINTS[index % TINTS.len()];
    [r, g, b, TINT_ALPHA]
}

/// Opacity of a zone stroke's preview of its result: twice the resting
/// [`TINT_ALPHA`], so previewed cells stand out from both their old tint and
/// resting tiles of the target zone.
pub const PREVIEW_ALPHA: f32 = 0.8;

/// The RGBA colour of tint `index` in a zone stroke's preview.
pub fn preview_color(index: usize) -> [f32; 4] {
    let [r, g, b] = TINTS[index % TINTS.len()];
    [r, g, b, PREVIEW_ALPHA]
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
    // Tile counts per (plane, target zone, whether an empty cell is written
    // its plane's default zone explicitly); `None` empties the cell.
    let mut moves: BTreeMap<(String, Option<String>, bool), usize> = BTreeMap::new();
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
            let was_empty = cell
                .get("expect")
                .is_none_or(|expect| matches!(expect, tilewright_jsonc::Value::Null { .. }));
            let default = committed
                .planes()
                .get(&plane)
                .and_then(|plane| plane.default_zone.as_deref());
            let explicit = was_empty && zone.is_some() && zone.as_deref() == default;
            *moves.entry((plane.clone(), zone, explicit)).or_default() += 1;
        }
    }
    let mut lines = Vec::new();
    for (index, ((plane, zone, explicit), count)) in moves.into_iter().enumerate() {
        if index == MAXIMUM_DESCRIBED {
            lines.push("Further zone changes are not described here.".into());
            break;
        }
        let one = count == 1;
        let tiles = if one {
            "1 tile".to_owned()
        } else {
            format!("{count} tiles")
        };
        let verb =
            |singular: &'static str, plural: &'static str| if one { singular } else { plural };
        let plane = committed.planes().get(&plane);
        let named = |id: &str| match plane.and_then(|plane| display_name(plane, id)) {
            Some(name) => format!("{} ({})", short(id), short(name)),
            None => format!("{} (no display name)", short(id)),
        };
        let default = plane.and_then(|p| p.default_zone.as_deref());
        lines.push(match (zone, default) {
            // An empty cell shows the default zone already; writing it only
            // makes it explicit.
            (Some(zone), Some(_)) if explicit => format!(
                "Zones: {tiles} now {} {} explicitly ({} showed it as the plane default).",
                verb("holds", "hold"),
                named(&zone),
                verb("it", "they")
            ),
            (Some(zone), _) => format!(
                "Zones: {tiles} {} to {}.",
                verb("moves", "move"),
                named(&zone)
            ),
            (None, Some(default)) => format!(
                "Zones: {tiles} {} to the default zone, {}.",
                verb("returns", "return"),
                named(default)
            ),
            (None, None) => format!(
                "Zones: {tiles} {} {} zone; the plane has no default zone.",
                verb("loses", "lose"),
                verb("its", "their")
            ),
        });
    }
    lines
}

/// How zone `id` is used in its plane: everything that keeps World from
/// deleting it. It reads the whole loaded plane, every chunk of every
/// layer, not just the view.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneUse<'w> {
    /// Tiles whose `zone` cell holds the zone.
    pub tiles: usize,
    /// Chunks with at least one such tile.
    pub chunks: usize,
    /// How many layers those chunks are on.
    pub layers: usize,
    /// Chunks whose zone palette lists the zone although no tile of theirs
    /// holds it. Only a hand edit leaves such an entry; a zone stroke that
    /// empties or repaints the last tile holding a zone drops its entry, so
    /// painting one tile with the zone and then back removes it.
    pub listed: usize,
    /// The first [`MAXIMUM_NAMED_CHUNKS`] of those chunks, as `(layer, chunk
    /// x, chunk y)` in layer and chunk order, so a refusal says where they
    /// are.
    pub listed_chunks: Vec<(i32, i64, i64)>,
    /// Zones whose parent is this zone, sorted by id.
    pub children: Vec<&'w str>,
    /// Whether it is the plane's `default_zone`.
    pub default: bool,
}

/// Most child zones named in a refusal; the rest are counted.
const MAXIMUM_NAMED_CHILDREN: usize = 3;
/// Most palette-only chunks named in a refusal; the rest are counted.
pub const MAXIMUM_NAMED_CHUNKS: usize = 3;

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

impl ZoneUse<'_> {
    /// Whether World would let the zone be deleted: no chunk lists it, no
    /// zone has it as parent and it is not the plane's default.
    pub fn unused(&self) -> bool {
        self.tiles == 0 && self.listed == 0 && self.children.is_empty() && !self.default
    }
    /// Why the zone cannot be deleted, one line per reason, in the order
    /// tiles, palette-only chunks, child zones, default zone. Empty when it
    /// is unused.
    pub fn reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.tiles > 0 {
            reasons.push(format!(
                "{} {} it, in {} on {}",
                plural(self.tiles, "tile", "tiles"),
                if self.tiles == 1 { "holds" } else { "hold" },
                plural(self.chunks, "chunk", "chunks"),
                plural(self.layers, "layer", "layers")
            ));
        }
        if self.listed > 0 {
            let mut named: Vec<String> = self
                .listed_chunks
                .iter()
                .map(|(layer, x, y)| format!("layer {layer} chunk {x}_{y}"))
                .collect();
            let more = self.listed.saturating_sub(named.len());
            if more > 0 {
                named.push(format!("{more} more"));
            }
            reasons.push(format!(
                "the zone palette of {} ({}) still {} it with no tile (to drop the entry, paint one of its tiles with the zone, then back)",
                plural(self.listed, "chunk", "chunks"),
                named.join(", "),
                if self.listed == 1 { "lists" } else { "list" }
            ));
        }
        if !self.children.is_empty() {
            let mut named: Vec<String> = self
                .children
                .iter()
                .take(MAXIMUM_NAMED_CHILDREN)
                .map(|child| short(child))
                .collect();
            let more = self.children.len().saturating_sub(MAXIMUM_NAMED_CHILDREN);
            if more > 0 {
                named.push(format!("{more} more"));
            }
            reasons.push(format!(
                "{} ({})",
                plural(self.children.len(), "child zone", "child zones"),
                named.join(", ")
            ));
        }
        if self.default {
            reasons.push("it is the plane's default zone".into());
        }
        reasons
    }
    /// The same reasons, short enough for one sidebar row each.
    pub fn short_reasons(&self) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.tiles > 0 {
            reasons.push(format!(
                "{} in {}",
                plural(self.tiles, "tile", "tiles"),
                plural(self.chunks, "chunk", "chunks")
            ));
        }
        if self.listed > 0 {
            reasons.push(format!(
                "listed by {}",
                plural(self.listed, "chunk palette", "chunk palettes")
            ));
        }
        if !self.children.is_empty() {
            reasons.push(plural(self.children.len(), "child zone", "child zones"));
        }
        if self.default {
            reasons.push("the plane default".into());
        }
        reasons
    }
}

/// How zone `id` is used anywhere in `plane` (see [`ZoneUse`]). Only chunks
/// whose zone palette lists the zone have their cells counted.
pub fn zone_use<'w>(plane: &'w Plane, id: &str) -> ZoneUse<'w> {
    let mut usage = ZoneUse {
        default: plane.default_zone.as_deref() == Some(id),
        children: plane
            .zones
            .values()
            .filter(|zone| zone.parent.as_deref() == Some(id))
            .map(|zone| zone.id.as_str())
            .collect(),
        ..ZoneUse::default()
    };
    let mut layers = BTreeSet::new();
    for (&(layer, cx, cy), chunk) in &plane.chunks {
        let Some(index) = chunk.zones.palette.iter().position(|zone| zone == id) else {
            continue;
        };
        let tiles = chunk
            .zones
            .cells
            .iter()
            .filter(|&&cell| usize::from(cell) == index + 1)
            .count();
        if tiles == 0 {
            usage.listed += 1;
            if usage.listed_chunks.len() < MAXIMUM_NAMED_CHUNKS {
                usage.listed_chunks.push((layer, cx, cy));
            }
        } else {
            usage.tiles += tiles;
            usage.chunks += 1;
            layers.insert(layer);
        }
    }
    usage.layers = layers.len();
    usage
}

/// The zones of `plane` World would let be deleted, sorted by id: exactly
/// those whose [`zone_use`] is [`ZoneUse::unused`], found in one pass over
/// the plane's chunks rather than one per zone.
pub fn unused_zones(plane: &Plane) -> Vec<&str> {
    let listed: BTreeSet<&str> = plane
        .chunks
        .values()
        .flat_map(|chunk| chunk.zones.palette.iter().map(String::as_str))
        .collect();
    let parents: BTreeSet<&str> = plane
        .zones
        .values()
        .filter_map(|zone| zone.parent.as_deref())
        .collect();
    plane
        .zones
        .keys()
        .map(String::as_str)
        .filter(|id| {
            !listed.contains(id)
                && !parents.contains(id)
                && plane.default_zone.as_deref() != Some(*id)
        })
        .collect()
}

/// Whether `ancestor` is `zone` itself or one of its ancestors. Unlike
/// [`zone_path`] the walk has no depth bound, so a long chain is followed to
/// its end; it stops at an unknown parent or a repeated zone.
pub fn is_within(plane: &Plane, zone: &str, ancestor: &str) -> bool {
    let mut seen = BTreeSet::new();
    let mut current = Some(zone);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        current = plane.zones.get(id).and_then(|zone| zone.parent.as_deref());
    }
    false
}

/// The zones that may become `id`'s parent without making a cycle: every
/// zone of the plane but `id` itself and its descendants, sorted by id.
pub fn parent_choices<'w>(plane: &'w Plane, id: &str) -> Vec<&'w str> {
    plane
        .zones
        .keys()
        .map(String::as_str)
        .filter(|zone| !is_within(plane, zone, id))
        .collect()
}

/// A zone's `properties`, member by member in source order, each value as
/// compact JSON. Read from the zone's canonical JSON, the only place the
/// loaded World keeps them.
pub fn properties(zone: &tilewright_source::Zone) -> Vec<(String, String)> {
    let limits = tilewright_jsonc::Limits::default();
    let Ok(document) = tilewright_jsonc::Document::parse(zone.canonical.clone(), &limits) else {
        return Vec::new();
    };
    let Some(members) = document
        .root()
        .get("properties")
        .and_then(tilewright_jsonc::Value::as_object)
    else {
        return Vec::new();
    };
    members
        .iter()
        .map(|member| (member.key.clone(), document.canonical(&member.value)))
        .collect()
}

/// Neutral review notes for every zone definition the pending World
/// creates, deletes, renames or moves under another parent, against the
/// committed World, in plane and zone id order. They describe undo and redo
/// as well as edits, since they compare definitions, not operations.
pub(crate) fn definition_notes(committed: &World, pending: &World) -> Vec<String> {
    let mut lines = Vec::new();
    let planes: BTreeSet<&String> = committed
        .planes()
        .keys()
        .chain(pending.planes().keys())
        .collect();
    let empty = BTreeMap::new();
    for plane in planes {
        let after_plane = pending.planes().get(plane);
        let before = committed.planes().get(plane).map_or(&empty, |p| &p.zones);
        let after = after_plane.map_or(&empty, |p| &p.zones);
        let ids: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        for id in ids {
            let shown = short(id);
            let quoted = |name: &str| format!("'{}'", short(name));
            let mut changes = Vec::new();
            match (before.get(id), after.get(id)) {
                (None, Some(zone)) => changes.push(format!(
                    "Creates zone {shown}{}{}.",
                    zone.parent.as_deref().map_or_else(
                        || " at the top level".into(),
                        |p| format!(" under {}", short(p))
                    ),
                    zone.name
                        .as_deref()
                        .map_or_else(String::new, |name| format!(", named {}", quoted(name)))
                )),
                (Some(_), None) => changes.push(format!("Deletes unused zone {shown}.")),
                (Some(old), Some(new)) => {
                    match (old.name.as_deref(), new.name.as_deref()) {
                        (Some(a), Some(b)) if a != b => changes.push(format!(
                            "Renames zone {shown} from {} to {}.",
                            quoted(a),
                            quoted(b)
                        )),
                        (None, Some(b)) => {
                            changes.push(format!("Names zone {shown} {}.", quoted(b)));
                        }
                        (Some(a), None) => {
                            // The name now comes from the first named
                            // zone above it, which is named here.
                            let inherited = after_plane
                                .and_then(|plane| {
                                    zone_path(plane, id).into_iter().find_map(|zone| {
                                        Some((zone, plane.zones.get(zone)?.name.as_deref()?))
                                    })
                                })
                                .map_or_else(
                                    || "no display name".into(),
                                    |(from, name)| {
                                        format!("{} from zone {}", quoted(name), short(from))
                                    },
                                );
                            changes.push(format!(
                                "Removes the display name {} of zone {shown}; it shows {inherited}.",
                                quoted(a)
                            ));
                        }
                        _ => {}
                    }
                    match (old.parent.as_deref(), new.parent.as_deref()) {
                        (old, Some(parent)) if old != Some(parent) => {
                            changes.push(format!("Moves zone {shown} under {}.", short(parent)));
                        }
                        (Some(_), None) => {
                            changes.push(format!("Makes zone {shown} a top-level zone."));
                        }
                        _ => {}
                    }
                }
                (None, None) => {}
            }
            for change in changes {
                if lines.len() == MAXIMUM_DESCRIBED {
                    lines.push("Further zone definition changes are not described here.".into());
                    return lines;
                }
                lines.push(change);
            }
        }
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
    fn unused_zones_are_exactly_those_zone_use_finds_unused() {
        let mut plane = with_cell(
            plane(
                vec![
                    zone("root", None, None),
                    zone("child", Some("root"), None),
                    zone("painted", None, None),
                    zone("listed", None, None),
                    zone("spare", None, None),
                    zone("other", None, None),
                ],
                Some("other"),
            ),
            0,
            "painted",
        );
        // A palette entry no cell uses still keeps its zone.
        plane = with_cell(plane, 1, "listed");
        if let Some(chunk) = plane.chunks.get_mut(&(0, 1, 0)) {
            chunk.zones.cells.fill(0);
        }
        let expected: Vec<&str> = plane
            .zones
            .keys()
            .map(String::as_str)
            .filter(|id| zone_use(&plane, id).unused())
            .collect();
        assert_eq!(expected, ["child", "spare"]);
        assert_eq!(unused_zones(&plane), expected);
        assert_eq!(
            tile_counts(["a", "b", "a"]),
            BTreeMap::from([("a", 2), ("b", 1)])
        );
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
