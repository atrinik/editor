// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Painted terrain of one plane as the editor holds it: the opened world's
//! chunks with unsaved cell edits over them. It is the [`TerrainSource`] the
//! toolkit's transition resolver reads, so the editor shows the tiles the
//! compiler will derive before anything is saved, also on chunks that do not
//! exist on disk yet.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};
use tilewright_source::{CHUNK_SIZE, Grid, ResolvedChunk, TerrainSource, Transitions, World};

/// Tiles along a chunk's side.
const SIDE: i64 = CHUNK_SIZE as i64;

/// Tiles in a chunk.
const CELLS: usize = (CHUNK_SIZE as usize) * (CHUNK_SIZE as usize);

/// Largest integer of the format (FORMAT.md section 3).
const MAXIMUM_INTEGER: i64 = 9_007_199_254_740_991;

/// The chunk coordinates a plane can hold: every tile of them stays within
/// the format's integer range (FORMAT.md section 2).
const CHUNKS: std::ops::RangeInclusive<i64> =
    -(MAXIMUM_INTEGER / SIDE)..=(MAXIMUM_INTEGER - (SIDE - 1)) / SIDE;

/// Most distinct values one unsaved grid holds. A cell stores its value's
/// palette index plus one in 16 bits.
pub const MAXIMUM_PALETTE: usize = u16::MAX as usize - 1;

/// A chunk's address in a plane: `(layer, cx, cy)`.
pub type ChunkKey = (i32, i64, i64);

/// The chunk of plane tile `[x, y]` and the tile's row-major index in it, or
/// `None` for a tile outside the chunks a plane can hold.
#[must_use]
pub fn chunk_of(layer: i32, [x, y]: [i64; 2]) -> Option<(ChunkKey, usize)> {
    let (cx, cy) = (x.div_euclid(SIDE), y.div_euclid(SIDE));
    if !CHUNKS.contains(&cx) || !CHUNKS.contains(&cy) {
        return None;
    }
    let index = y.rem_euclid(SIDE) * SIDE + x.rem_euclid(SIDE);
    Some(((layer, cx, cy), usize::try_from(index).ok()?))
}

/// The chunks around `key`, itself included: those whose resolved tiles can
/// change when a tile of `key` changes, since transitions read the eight
/// neighbours of a tile across chunk edges.
pub fn neighbourhood((layer, cx, cy): ChunkKey) -> impl Iterator<Item = ChunkKey> {
    (-1_i64..=1).flat_map(move |dy| {
        (-1_i64..=1).filter_map(move |dx| {
            let (x, y) = (cx.checked_add(dx)?, cy.checked_add(dy)?);
            (CHUNKS.contains(&x) && CHUNKS.contains(&y)).then_some((layer, x, y))
        })
    })
}

/// The ground layers of one plane, as opened, with unsaved cell edits.
#[derive(Clone, Debug)]
pub struct PlaneTerrain {
    world: Arc<World>,
    plane: String,
    /// Every ground grid of each chunk with an edit, a full copy of the
    /// opened chunk's grids or empty grids for a chunk not on disk.
    edited: BTreeMap<ChunkKey, Vec<Grid>>,
}

impl PlaneTerrain {
    /// The terrain of plane `plane` of `world`, with no edits.
    ///
    /// # Errors
    /// Fails if the world has no such plane.
    pub fn new(world: Arc<World>, plane: &str) -> Result<Self, Error> {
        if !world.planes().contains_key(plane) {
            return Err(Error::UnknownPlane(plane.to_owned()));
        }
        Ok(Self {
            world,
            plane: plane.to_owned(),
            edited: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn plane(&self) -> &str {
        &self.plane
    }

    #[must_use]
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// The transitions of the world's profile, if it has any.
    #[must_use]
    pub fn transitions(&self) -> Option<&Transitions> {
        self.world.profile().transitions.as_deref()
    }

    /// The value of ground layer `ground` at plane tile `at` of `layer`, with
    /// the unsaved edits: `None` for an empty tile.
    #[must_use]
    pub fn value(&self, layer: i32, at: [i64; 2], ground: usize) -> Option<&str> {
        let ((layer, cx, cy), index) = chunk_of(layer, at)?;
        self.chunk_ground(layer, cx, cy)?.get(ground)?.value(index)
    }

    /// Sets ground layer `ground` at plane tile `at` of `layer` to `value`,
    /// `None` for empty, and returns the value it had.
    ///
    /// # Errors
    /// Fails for a tile outside the plane's range, a ground layer the
    /// profile does not have, or a grid with no room for another value.
    pub fn set(
        &mut self,
        layer: i32,
        at: [i64; 2],
        ground: usize,
        value: Option<&str>,
    ) -> Result<Option<String>, Error> {
        let (key, index) = chunk_of(layer, at).ok_or(Error::OutOfRange(at))?;
        let layers = self.world.profile().ground_layers.len();
        if ground >= layers {
            return Err(Error::UnknownGround(ground));
        }
        let before = self.value(layer, at, ground).map(str::to_owned);
        if before.as_deref() == value {
            return Ok(before);
        }
        let opened = self
            .world
            .planes()
            .get(&self.plane)
            .and_then(|plane| plane.chunks.get(&key))
            .map(|chunk| chunk.ground.clone());
        let grids = self
            .edited
            .entry(key)
            .or_insert_with(|| opened.unwrap_or_else(|| vec![Grid::default(); layers]));
        let grid = grids.get_mut(ground).ok_or(Error::UnknownGround(ground))?;
        if grid.cells.len() != CELLS {
            grid.cells = vec![0; CELLS];
        }
        let cell = match value {
            None => 0,
            Some(value) => {
                let position = match grid.palette.iter().position(|entry| entry == value) {
                    Some(position) => position,
                    None if grid.palette.len() < MAXIMUM_PALETTE => {
                        grid.palette.push(value.to_owned());
                        grid.palette.len() - 1
                    }
                    None => return Err(Error::PaletteFull(at)),
                };
                u16::try_from(position + 1).map_err(|_| Error::PaletteFull(at))?
            }
        };
        if let Some(slot) = grid.cells.get_mut(index) {
            *slot = cell;
        }
        Ok(before)
    }

    /// Chunks with unsaved edits, in address order.
    pub fn edited(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.edited.keys().copied()
    }

    /// Whether any cell differs from the opened world. A cell set back to
    /// its opened value still counts.
    #[must_use]
    pub fn is_edited(&self) -> bool {
        !self.edited.is_empty()
    }

    /// The chunks that have tiles: the opened plane's and the edited ones,
    /// on `layer`.
    #[must_use]
    pub fn chunks(&self, layer: i32) -> BTreeSet<ChunkKey> {
        let opened = self
            .world
            .planes()
            .get(&self.plane)
            .into_iter()
            .flat_map(|plane| plane.chunks.keys().copied());
        opened
            .chain(self.edited.keys().copied())
            .filter(|(chunk_layer, _, _)| *chunk_layer == layer)
            .collect()
    }

    /// The ground tiles of chunk `key` with the profile's transitions
    /// applied, as the compiler would derive them from these grids; without
    /// transitions, `None`.
    #[must_use]
    pub fn resolve(&self, (layer, cx, cy): ChunkKey) -> Option<ResolvedChunk<'_>> {
        Some(self.transitions()?.resolve_chunk(self, layer, cx, cy))
    }
}

impl TerrainSource for PlaneTerrain {
    fn chunk_ground(&self, layer: i32, cx: i64, cy: i64) -> Option<&[Grid]> {
        if let Some(grids) = self.edited.get(&(layer, cx, cy)) {
            return Some(grids);
        }
        self.world
            .planes()
            .get(&self.plane)?
            .chunk_ground(layer, cx, cy)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    UnknownPlane(String),
    UnknownGround(usize),
    /// A tile outside the chunks a plane can hold.
    OutOfRange([i64; 2]),
    /// The tile's grid already holds [`MAXIMUM_PALETTE`] values.
    PaletteFull([i64; 2]),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPlane(plane) => write!(formatter, "the world has no plane {plane:?}"),
            Self::UnknownGround(ground) => {
                write!(formatter, "the profile has no ground layer {ground}")
            }
            Self::OutOfRange([x, y]) => {
                write!(formatter, "tile ({x}, {y}) is outside the plane's range")
            }
            Self::PaletteFull([x, y]) => write!(
                formatter,
                "the chunk of tile ({x}, {y}) holds too many values to add another"
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{CHUNKS, Error, PlaneTerrain, chunk_of, neighbourhood};
    use crate::WorldDocument;
    use tilewright_editor_testkit::fixture_world;

    fn meadow() -> PlaneTerrain {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        PlaneTerrain::new(document.world().clone(), "meadow").unwrap()
    }

    #[test]
    fn chunk_addresses_follow_the_format() {
        assert_eq!(chunk_of(0, [0, 0]), Some(((0, 0, 0), 0)));
        assert_eq!(chunk_of(2, [33, 1]), Some(((2, 1, 0), 32 + 1)));
        assert_eq!(chunk_of(-1, [-1, -32]), Some(((-1, -1, -1), 31)));
        assert_eq!(chunk_of(0, [i64::MAX, 0]), None);
        assert_eq!(chunk_of(0, [0, i64::MIN]), None);
        let last = *CHUNKS.end() * 32 + 31;
        assert!(chunk_of(0, [last, 0]).is_some());
        assert!(chunk_of(0, [last + 1, 0]).is_none());
        assert_eq!(neighbourhood((0, 0, 0)).count(), 9);
        assert_eq!(neighbourhood((0, *CHUNKS.end(), 0)).count(), 6);
    }

    #[test]
    fn edits_shadow_the_opened_chunks_and_create_missing_ones() {
        let mut terrain = meadow();
        assert_eq!(terrain.value(0, [10, 10], 0), Some("water"));
        assert_eq!(
            terrain
                .set(0, [10, 10], 0, Some("sand"))
                .unwrap()
                .as_deref(),
            Some("water")
        );
        assert_eq!(terrain.value(0, [10, 10], 0), Some("sand"));
        // The world as opened is untouched.
        let plane = &terrain.world().planes()["meadow"];
        assert_eq!(
            plane.chunks[&(0, 0, 0)].ground[0].value(10 * 32 + 10),
            Some("water")
        );
        // A chunk that is not on disk.
        assert_eq!(terrain.value(0, [40, 2], 0), None);
        assert_eq!(terrain.set(0, [40, 2], 0, Some("grass")).unwrap(), None);
        assert_eq!(terrain.value(0, [40, 2], 0), Some("grass"));
        assert_eq!(terrain.edited().collect::<Vec<_>>(), [(0, 0, 0), (0, 1, 0)]);
        assert_eq!(
            terrain.chunks(0).into_iter().collect::<Vec<_>>(),
            [(0, 0, 0), (0, 1, 0)]
        );
        // Setting a cell to its value changes nothing.
        let mut unchanged = meadow();
        unchanged.set(0, [0, 0], 0, Some("grass")).unwrap();
        assert!(!unchanged.is_edited());
    }

    #[test]
    fn rejects_unknown_planes_layers_and_ranges() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        assert_eq!(
            PlaneTerrain::new(document.world().clone(), "nowhere").unwrap_err(),
            Error::UnknownPlane("nowhere".to_owned())
        );
        let mut terrain = meadow();
        assert_eq!(
            terrain.set(0, [0, 0], 3, Some("sand")),
            Err(Error::UnknownGround(3))
        );
        assert_eq!(
            terrain.set(0, [i64::MAX, 0], 0, Some("sand")),
            Err(Error::OutOfRange([i64::MAX, 0]))
        );
    }

    #[test]
    fn resolves_unsaved_edits_across_chunk_edges() {
        let mut terrain = meadow();
        // Sand at x 26..=31 meets grass painted at x 32 of the missing chunk
        // east of it: the sand at x 31 shows the grass edge on its east.
        assert_eq!(terrain.resolve((0, 0, 0)).unwrap().at(2, [31, 5]), None);
        terrain.set(0, [32, 5], 0, Some("grass")).unwrap();
        let west = terrain.resolve((0, 0, 0)).unwrap();
        assert_eq!(west.at(2, [31, 5]), Some("grass_edge_e"));
        let east = terrain.resolve((0, 1, 0)).unwrap();
        assert_eq!(east.at(0, [0, 5]), Some("grass"));
        assert!(matches!(east.at(1, [0, 5]), Some("clover" | "daisies")));
    }
}
