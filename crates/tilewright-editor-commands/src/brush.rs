// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The terrain brush. A stroke stamps the brush's footprint along the
//! pointer's path and paints one terrain of the profile's transitions: the
//! terrain's first prototype, in the terrain layer, one cell per tile
//! (FORMAT.md section 13). While the stroke runs it paints the editor's
//! unsaved terrain, so the resolved transitions show at once; when it ends
//! it becomes one `terrain.set` operation whose cells expect the values they
//! had, so a tile that changed on disk meanwhile is a conflict, not an
//! overwrite.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fmt::Write as _,
};
use tilewright_editor_document::terrain::{self, ChunkKey, PlaneTerrain, neighbourhood};
use tilewright_source::is_identifier;

/// Largest brush radius, in tiles.
pub const MAXIMUM_RADIUS: u16 = 32;

/// Most tiles one stroke paints: within the cells one save can hold,
/// [`crate::MAXIMUM_TRANSACTION_CELLS`].
pub const MAXIMUM_STROKE_TILES: usize = 100_000;

/// Most chunks one stroke paints in: the toolkit's bound on the chunks one
/// terrain operation names.
pub const MAXIMUM_STROKE_CHUNKS: usize = 10_000;

/// Most tiles between two pointer samples that a stroke fills in. A jump
/// further than this paints only where the pointer lands.
pub const MAXIMUM_SEGMENT: u64 = 4_096;

/// The namespace of the editor's `custom` data in transactions: the
/// project's address under its organisation's GitHub Pages domain.
pub const NAMESPACE: &str = "seraphinastudios.github.io/tilewright-editor";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    Square,
    Circle,
}

impl Shape {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Circle => "circle",
        }
    }
}

/// A brush footprint: a square or a disc around the pointer's tile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Brush {
    pub shape: Shape,
    /// Tiles from the centre to the edge, 0 for one tile; at most
    /// [`MAXIMUM_RADIUS`].
    pub radius: u16,
}

impl Brush {
    /// The tiles the brush covers around `center`, row by row. A radius
    /// above [`MAXIMUM_RADIUS`] counts as that radius; tiles beyond the
    /// integer range are left out.
    pub fn footprint(self, [x, y]: [i64; 2]) -> impl Iterator<Item = [i64; 2]> {
        let radius = i64::from(self.radius.min(MAXIMUM_RADIUS));
        let shape = self.shape;
        (-radius..=radius).flat_map(move |dy| {
            (-radius..=radius).filter_map(move |dx| {
                // A disc of tiles whose centres lie within half a tile past
                // the radius, so small discs are round rather than
                // diamond-shaped.
                let inside = match shape {
                    Shape::Square => true,
                    Shape::Circle => dx * dx + dy * dy <= radius * radius + radius,
                };
                if !inside {
                    return None;
                }
                Some([x.checked_add(dx)?, y.checked_add(dy)?])
            })
        })
    }
}

/// The tiles of the line from `from` to `to`, both included, in order. A
/// line longer than [`MAXIMUM_SEGMENT`] tiles is just `to`.
#[must_use]
pub fn segment(from: [i64; 2], to: [i64; 2]) -> Vec<[i64; 2]> {
    let [x0, y0] = from.map(i128::from);
    let [x1, y1] = to.map(i128::from);
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    if dx.max(-dy) > i128::from(MAXIMUM_SEGMENT) {
        return vec![to];
    }
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut error) = (x0, y0, dx + dy);
    let mut tiles = Vec::new();
    loop {
        // Every tile lies between the two ends, so it fits in an i64.
        if let (Ok(tx), Ok(ty)) = (i64::try_from(x), i64::try_from(y)) {
            tiles.push([tx, ty]);
        }
        if x == x1 && y == y1 {
            return tiles;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

/// One brush stroke on one layer of a plane, from pointer down to pointer up.
#[derive(Clone, Debug)]
pub struct Stroke {
    plane: String,
    layer: i32,
    /// The terrain layer's index among the profile's ground layers, and its
    /// name.
    ground: (usize, String),
    terrain: String,
    paint: String,
    brush: Brush,
    last: Option<[i64; 2]>,
    /// Each tile the stroke changed, with the value it had before.
    before: BTreeMap<[i64; 2], Option<String>>,
    /// The chunks of those tiles.
    chunks: BTreeSet<ChunkKey>,
}

/// A stamp that failed partway: why, and the chunks whose resolved tiles
/// may have changed before it did, since the tiles it painted stay painted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StampError {
    pub error: Error,
    pub dirty: BTreeSet<ChunkKey>,
}

impl Stroke {
    /// Starts a stroke that paints terrain `terrain` with `brush` on `layer`
    /// of the plane `unsaved` holds.
    ///
    /// # Errors
    /// Fails when the profile has no transitions or no such terrain.
    pub fn begin(
        unsaved: &PlaneTerrain,
        layer: i32,
        terrain: &str,
        brush: Brush,
    ) -> Result<Self, Error> {
        let transitions = unsaved.transitions().ok_or(Error::NoTransitions)?;
        let index = transitions
            .terrain(terrain)
            .ok_or_else(|| Error::UnknownTerrain(terrain.to_owned()))?;
        let paint = transitions
            .paint(index)
            .ok_or_else(|| Error::UnknownTerrain(terrain.to_owned()))?;
        let ground = transitions.terrain_layer();
        let grid = unsaved
            .world()
            .profile()
            .ground_layers
            .get(ground)
            .ok_or(Error::NoTransitions)?;
        Ok(Self {
            plane: unsaved.plane().to_owned(),
            layer,
            ground: (ground, grid.clone()),
            terrain: terrain.to_owned(),
            paint: paint.to_owned(),
            brush,
            last: None,
            before: BTreeMap::new(),
            chunks: BTreeSet::new(),
        })
    }

    /// Stamps the brush at plane tile `point`, and along the line from the
    /// previous point, into `unsaved`. Returns the chunks whose resolved
    /// tiles may have changed: those it painted in and their neighbours.
    /// Tiles outside the plane's range are skipped.
    ///
    /// # Errors
    /// Fails if the stroke would paint more than [`MAXIMUM_STROKE_TILES`]
    /// tiles or in more than [`MAXIMUM_STROKE_CHUNKS`] chunks, a grid has no
    /// room for the terrain's prototype, or `unsaved` holds another plane
    /// than the stroke began on. Tiles painted before the failure stay
    /// painted and recorded, and the error names the chunks they changed.
    pub fn stamp(
        &mut self,
        unsaved: &mut PlaneTerrain,
        point: [i64; 2],
    ) -> Result<BTreeSet<ChunkKey>, StampError> {
        if unsaved.plane() != self.plane {
            return Err(StampError {
                error: Error::OtherPlane(unsaved.plane().to_owned()),
                dirty: BTreeSet::new(),
            });
        }
        let path = segment(self.last.unwrap_or(point), point);
        self.last = Some(point);
        let mut dirty = BTreeSet::new();
        let mut painted = BTreeSet::new();
        for center in path {
            for tile in self.brush.footprint(center) {
                if self.before.contains_key(&tile) || !painted.insert(tile) {
                    continue;
                }
                let Some((chunk, _)) = terrain::chunk_of(self.layer, tile) else {
                    continue;
                };
                if unsaved.value(self.layer, tile, self.ground.0) == Some(self.paint.as_str()) {
                    continue;
                }
                let new_chunk = !self.chunks.contains(&chunk);
                if self.before.len() >= MAXIMUM_STROKE_TILES
                    || (new_chunk && self.chunks.len() >= MAXIMUM_STROKE_CHUNKS)
                {
                    return Err(StampError {
                        error: Error::TooLarge,
                        dirty,
                    });
                }
                let before = match unsaved.set(self.layer, tile, self.ground.0, Some(&self.paint)) {
                    Ok(before) => before,
                    Err(error) => {
                        return Err(StampError {
                            error: Error::Terrain(error),
                            dirty,
                        });
                    }
                };
                self.before.insert(tile, before);
                self.chunks.insert(chunk);
                dirty.extend(neighbourhood(chunk));
            }
        }
        Ok(dirty)
    }

    /// The plane the stroke paints.
    #[must_use]
    pub fn plane(&self) -> &str {
        &self.plane
    }

    /// The chunks the stroke changed tiles in, in address order.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        self.chunks.iter().copied()
    }

    /// How many tiles the stroke changed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.before.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.before.is_empty()
    }

    #[must_use]
    pub fn terrain(&self) -> &str {
        &self.terrain
    }

    /// Puts back every tile the stroke changed in `unsaved`, as when the
    /// stroke is cancelled.
    ///
    /// # Errors
    /// Fails if `unsaved` holds another plane than the stroke's.
    pub fn revert(&self, unsaved: &mut PlaneTerrain) -> Result<(), Error> {
        if unsaved.plane() != self.plane {
            return Err(Error::OtherPlane(unsaved.plane().to_owned()));
        }
        for (tile, before) in &self.before {
            unsaved
                .set(self.layer, *tile, self.ground.0, before.as_deref())
                .map_err(Error::Terrain)?;
        }
        Ok(())
    }

    /// The `terrain.set` operation that makes the stroke's change: every
    /// tile it changed, in tile order, expecting the value it had. `None`
    /// when it changed nothing.
    ///
    /// # Errors
    /// Fails if a name the operation holds is not an identifier, which the
    /// toolkit's validation of the world rules out.
    pub fn operation(&self) -> Result<Option<String>, Error> {
        if self.before.is_empty() {
            return Ok(None);
        }
        let names = [&self.plane, &self.ground.1, &self.paint];
        if let Some(name) = names
            .into_iter()
            .chain(self.before.values().flatten())
            .find(|name| !is_identifier(name))
        {
            return Err(Error::NotIdentifier(name.clone()));
        }
        let mut text = format!(
            "{{\"op\": \"terrain.set\", \"plane\": \"{}\", \"layer\": {}, \"grid\": \"{}\", \"cells\": [",
            self.plane, self.layer, self.ground.1
        );
        for (index, ([x, y], before)) in self.before.iter().enumerate() {
            let separator = if index == 0 { "\n" } else { ",\n" };
            let expect = before
                .as_ref()
                .map_or_else(|| "null".to_owned(), |value| format!("\"{value}\""));
            let _ = write!(
                text,
                "{separator}  {{\"at\": [{x}, {y}], \"value\": \"{}\", \"expect\": {expect}}}",
                self.paint
            );
        }
        text.push_str("\n]}");
        Ok(Some(text))
    }

    /// What the stroke did, for history and the transaction's title.
    #[must_use]
    pub fn title(&self) -> String {
        let tiles = self.before.len();
        let plural = if tiles == 1 { "" } else { "s" };
        format!("Paint {} on {tiles} tile{plural}", self.terrain)
    }

    /// The stroke as the editor records it in a transaction's `custom`
    /// data: the command and its parameters.
    #[must_use]
    pub fn record(&self) -> String {
        format!(
            "{{\"command\": \"terrain.brush\", \"terrain\": \"{}\", \"shape\": \"{}\", \"radius\": {}}}",
            self.terrain,
            self.brush.shape.as_str(),
            self.brush.radius.min(MAXIMUM_RADIUS)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// The profile has no terrain transitions, so there is nothing to paint.
    NoTransitions,
    UnknownTerrain(String),
    OtherPlane(String),
    TooLarge,
    NotIdentifier(String),
    Terrain(terrain::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTransitions => write!(formatter, "the profile has no terrain transitions"),
            Self::UnknownTerrain(terrain) => {
                write!(formatter, "the profile has no terrain {terrain:?}")
            }
            Self::OtherPlane(plane) => {
                write!(
                    formatter,
                    "the stroke began on another plane than {plane:?}"
                )
            }
            Self::TooLarge => write!(
                formatter,
                "a stroke paints at most {MAXIMUM_STROKE_TILES} tiles in at most {MAXIMUM_STROKE_CHUNKS} chunks"
            ),
            Self::NotIdentifier(name) => write!(formatter, "{name:?} is not an identifier"),
            Self::Terrain(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Display for StampError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for StampError {}

#[cfg(test)]
mod tests {
    use super::{
        Brush, Error, MAXIMUM_RADIUS, MAXIMUM_SEGMENT, MAXIMUM_STROKE_TILES, Shape, Stroke, segment,
    };
    use std::collections::BTreeSet;
    use tilewright_editor_document::{WorldDocument, terrain::PlaneTerrain};
    use tilewright_editor_testkit::fixture_world;

    fn meadow() -> PlaneTerrain {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        PlaneTerrain::new(document.world().clone(), "meadow").unwrap()
    }

    #[test]
    fn footprints_are_squares_and_round_discs() {
        let square = Brush {
            shape: Shape::Square,
            radius: 1,
        };
        assert_eq!(square.footprint([5, 5]).count(), 9);
        let disc: BTreeSet<[i64; 2]> = Brush {
            shape: Shape::Circle,
            radius: 2,
        }
        .footprint([0, 0])
        .collect();
        // A 5 x 5 square without its corners.
        assert_eq!(disc.len(), 21);
        assert!(!disc.contains(&[2, 2]) && disc.contains(&[2, 1]) && disc.contains(&[0, 2]));
        let dot = Brush {
            shape: Shape::Circle,
            radius: 0,
        };
        assert_eq!(dot.footprint([3, -4]).collect::<Vec<_>>(), [[3, -4]]);
        let edge = Brush {
            shape: Shape::Square,
            radius: 1,
        };
        assert_eq!(edge.footprint([i64::MAX, 0]).count(), 6);
        let huge = Brush {
            shape: Shape::Square,
            radius: u16::MAX,
        };
        assert_eq!(huge.footprint([0, 0]).count(), 65 * 65);
    }

    #[test]
    fn segments_connect_pointer_samples() {
        assert_eq!(segment([0, 0], [0, 0]), [[0, 0]]);
        assert_eq!(segment([0, 0], [3, 1]), [[0, 0], [1, 0], [2, 1], [3, 1]]);
        assert_eq!(segment([2, 2], [2, -1]), [[2, 2], [2, 1], [2, 0], [2, -1]]);
        let far = i64::try_from(MAXIMUM_SEGMENT).unwrap() + 1;
        assert_eq!(segment([0, 0], [far, 0]), [[far, 0]]);
        assert_eq!(segment([i64::MIN, 0], [i64::MAX, 0]), [[i64::MAX, 0]]);
    }

    #[test]
    fn a_stroke_paints_the_terrain_and_records_what_it_replaced() {
        let mut unsaved = meadow();
        let brush = Brush {
            shape: Shape::Square,
            radius: 0,
        };
        let mut stroke = Stroke::begin(&unsaved, 0, "sand", brush).unwrap();
        // From the pond's edge east into grass, then into the missing chunk.
        let dirty = stroke.stamp(&mut unsaved, [14, 12]).unwrap();
        assert_eq!(dirty.len(), 9, "{dirty:?}");
        stroke.stamp(&mut unsaved, [16, 12]).unwrap();
        stroke.stamp(&mut unsaved, [33, 0]).unwrap();
        assert_eq!(unsaved.value(0, [15, 12], 0), Some("sand"));
        assert_eq!(unsaved.value(0, [33, 0], 0), Some("sand"));
        assert_eq!(
            stroke.title(),
            format!("Paint sand on {} tiles", stroke.len())
        );
        let operation = stroke.operation().unwrap().unwrap();
        assert!(operation.starts_with(
            "{\"op\": \"terrain.set\", \"plane\": \"meadow\", \"layer\": 0, \"grid\": \"ground\", \"cells\": [\n"
        ));
        assert!(
            operation.contains("{\"at\": [14, 12], \"value\": \"sand\", \"expect\": \"water\"}")
        );
        assert!(
            operation.contains("{\"at\": [15, 12], \"value\": \"sand\", \"expect\": \"grass\"}")
        );
        assert!(operation.contains("{\"at\": [33, 0], \"value\": \"sand\", \"expect\": null}"));
        // Tiles that already hold sand are not part of the change.
        assert!(!operation.contains("[26, "));
        stroke.revert(&mut unsaved).unwrap();
        assert_eq!(unsaved.value(0, [14, 12], 0), Some("water"));
        assert_eq!(unsaved.value(0, [33, 0], 0), None);
    }

    #[test]
    fn a_stroke_that_changes_nothing_has_no_operation() {
        let mut unsaved = meadow();
        let brush = Brush {
            shape: Shape::Circle,
            radius: 1,
        };
        let mut stroke = Stroke::begin(&unsaved, 0, "grass", brush).unwrap();
        stroke.stamp(&mut unsaved, [3, 3]).unwrap();
        assert!(stroke.is_empty());
        assert_eq!(stroke.operation().unwrap(), None);
        assert!(!unsaved.is_edited());
    }

    #[test]
    fn strokes_need_transitions_and_a_known_terrain() {
        let unsaved = meadow();
        let brush = Brush {
            shape: Shape::Square,
            radius: 0,
        };
        assert_eq!(
            Stroke::begin(&unsaved, 0, "lava", brush).unwrap_err(),
            Error::UnknownTerrain("lava".to_owned())
        );
        let plain = WorldDocument::open(&fixture_world("plain")).unwrap();
        let unsaved = PlaneTerrain::new(plain.world().clone(), "flat").unwrap();
        assert_eq!(unsaved.resolve((0, 0, 0)), None);
        assert_eq!(
            Stroke::begin(&unsaved, 0, "stone", brush).unwrap_err(),
            Error::NoTransitions
        );
    }

    #[test]
    fn a_stroke_stops_at_its_cap_and_keeps_what_it_painted() {
        let mut unsaved = meadow();
        let wide = Brush {
            shape: Shape::Square,
            radius: MAXIMUM_RADIUS,
        };
        let mut stroke = Stroke::begin(&unsaved, 0, "water", wide).unwrap();
        stroke.stamp(&mut unsaved, [0, 0]).unwrap();
        let partial = stroke.stamp(&mut unsaved, [4_000, 0]).unwrap_err();
        assert_eq!(partial.error, Error::TooLarge);
        assert!(partial.dirty.contains(&(0, 2, 0)));
        assert_eq!(stroke.len(), MAXIMUM_STROKE_TILES);
        assert_eq!(unsaved.value(0, [64, 0], 0), Some("water"));
        assert!(stroke.operation().unwrap().is_some());
    }

    #[test]
    fn an_operation_holds_identifiers_only() {
        let mut unsaved = meadow();
        // Only the toolkit's validation keeps such a value out of a world.
        unsaved
            .set(0, [3, 3], 0, Some("Not an identifier"))
            .unwrap();
        let mut stroke = Stroke::begin(
            &unsaved,
            0,
            "sand",
            Brush {
                shape: Shape::Square,
                radius: 0,
            },
        )
        .unwrap();
        stroke.stamp(&mut unsaved, [3, 3]).unwrap();
        assert_eq!(
            stroke.operation().unwrap_err(),
            Error::NotIdentifier("Not an identifier".to_owned())
        );
    }
}
