// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The ground of a plane as a renderer scene. Until the editor reads a
//! game's art, every prototype draws as a deterministic colour swatch: a
//! solid tile in the first ground layer, a centred square in the second,
//! and a frame in the others, so the terrain under a transition tile stays
//! visible. Every tile in view without ground draws as a faint empty tile,
//! so the pointer finds a tile everywhere through the renderer's semantic
//! mask, also where nothing is painted yet.
//!
//! At some zooms the renderer leaves one-pixel seams between neighbouring
//! tiles, where the background shows through and the mask holds no tile.
//! That is a known renderer limitation, fixed in the renderer by snapping
//! both edges of each quad to the same pixel; until then a pick on a seam
//! finds the nearest tile.

use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};
use tilewright_render_api::FrameOutput;
use tilewright_render_resources::{
    Error as ResourceError, ResourceBlob, ResourceProvider, ResourceRequest,
};
use tilewright_render_world::{
    Anchor, Camera, HeightField, Projection, Stretch, TextureRef, WorldObject, WorldView,
};
use tilewright_scene::{Digest256, ResourceId, Sampling, SceneBundle, SceneLimits, SemanticId};

/// Most tiles a ground view covers. A view that would cover more, such as
/// one zoomed far out, draws none and asks to zoom in.
pub const MAXIMUM_VIEW_TILES: usize = 30_000;

/// Most objects a ground view draws: its tiles of every ground layer and its
/// empty tiles, within the renderer's quad budget.
pub const MAXIMUM_VIEW_OBJECTS: usize = 120_000;

/// How far, in pixels, a pick looks for a tile where the semantic mask holds
/// none under the pointer.
pub const PICK_REACH: i64 = 2;

/// Largest top-down swatch side, in pixels. Top-down ground is drawn at the
/// tile size whatever its art's, so a larger tile draws this swatch scaled.
pub const MAXIMUM_SWATCH_SIDE: u32 = 256;

/// Largest isometric tile, in pixels: isometric ground is drawn at its art's
/// size, so its swatch is the tile's size, and wider tiles are refused.
pub const MAXIMUM_ISOMETRIC_TILE: u32 = 1_024;

/// Background of the view, outside the plane's tiles.
const CLEAR: [f32; 4] = [0.04, 0.05, 0.07, 1.0];

/// What an empty tile draws: a faint grey.
const EMPTY: [u8; 4] = [0x2a, 0x2e, 0x36, 0xff];

/// The camera of a ground view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundCamera {
    pub projection: Projection,
    /// Plane tile coordinates at the view's centre.
    pub center: [f64; 2],
    /// Screen pixels per art pixel.
    pub zoom: f32,
    pub width: u32,
    pub height: u32,
}

impl GroundCamera {
    /// Moves the centre so the view follows a drag of `delta` screen
    /// pixels: what was under the pointer stays under it. Only the centre
    /// moves; the renderer still places every tile.
    pub fn pan(&mut self, [dx, dy]: [f32; 2]) {
        let zoom = f64::from(self.zoom.max(f32::MIN_POSITIVE));
        let (dx, dy) = (f64::from(dx) / zoom, f64::from(dy) / zoom);
        let (x, y) = match self.projection {
            Projection::TopDown { tile_size } => {
                let side = f64::from(tile_size.max(1));
                (dx / side, dy / side)
            }
            Projection::Isometric {
                tile_width,
                tile_height,
            } => {
                // A tile step moves half a tile across and half down.
                let (across, down) = (
                    dx / (f64::from(tile_width.max(1)) / 2.0),
                    dy / (f64::from(tile_height.max(1)) / 2.0),
                );
                (f64::midpoint(across, down), f64::midpoint(down, -across))
            }
        };
        self.center = [self.center[0] - x, self.center[1] - y];
    }
}

/// One tile of one ground layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroundTile<'a> {
    pub at: [i64; 2],
    /// The ground layer's index in the profile.
    pub ground: usize,
    pub proto: &'a str,
}

/// How a swatch fills its tile.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Pattern {
    Solid,
    Centre,
    Frame,
}

impl Pattern {
    const fn of(ground: usize) -> Self {
        match ground {
            0 => Self::Solid,
            1 => Self::Centre,
            _ => Self::Frame,
        }
    }

    /// Whether the pattern covers the point `(u, v)` of its tile, each from
    /// -1 to 1 across it.
    fn covers(self, u: f64, v: f64) -> bool {
        let extent = u.abs().max(v.abs());
        match self {
            Self::Solid => true,
            Self::Centre => extent <= 0.34,
            Self::Frame => extent >= 0.75,
        }
    }
}

/// The colour a prototype's swatch draws: a hue and lightness from a hash
/// of its ID, so the same prototype always looks the same.
#[must_use]
pub fn swatch_color(proto: &str) -> [u8; 3] {
    let hash = fnv1a(proto.as_bytes());
    let hue = f64::from(u32::try_from(hash % 360).unwrap_or(0));
    let lightness = 0.42 + f64::from(u32::try_from((hash >> 16) % 5).unwrap_or(0)) * 0.06;
    hsl(hue, 0.55, lightness)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hsl(hue: f64, saturation: f64, lightness: f64) -> [u8; 3] {
    let chroma = (1.0 - 2.0f64.mul_add(lightness, -1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lightness - chroma / 2.0;
    [red, green, blue].map(|channel| ((channel + base) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// The swatch textures of a scene, served to the renderer, each with its
/// digest.
#[derive(Debug, Default)]
pub struct Swatches {
    textures: BTreeMap<ResourceId, (Vec<u8>, Digest256)>,
}

impl ResourceProvider for Swatches {
    fn load(&self, request: ResourceRequest) -> Result<ResourceBlob, ResourceError> {
        let request = request.validate()?;
        let (bytes, _) = self
            .textures
            .get(&request.id)
            .filter(|_| request.revision == 1)
            .ok_or(ResourceError::NotFound)?;
        ResourceBlob::for_request(request, bytes.clone())
    }
}

/// A texture's identity: a hash of what it shows and its size, so a frame
/// that redraws the same swatch finds it in the renderer's cache.
fn texture_id(key: &str, pattern: Pattern, width: u32, height: u32) -> ResourceId {
    let mut bytes = key.as_bytes().to_vec();
    bytes.extend_from_slice(&[pattern as u8]);
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    ResourceId(fnv1a(&bytes).max(1))
}

/// The swatch size for a projection: an isometric tile's own size, which is
/// how large the renderer draws it, or a top-down tile's at most
/// [`MAXIMUM_SWATCH_SIDE`], which the renderer scales to the tile.
fn swatch_size(projection: Projection) -> Result<(u32, u32), Error> {
    match projection {
        Projection::Isometric {
            tile_width,
            tile_height,
        } if tile_width.max(tile_height) <= MAXIMUM_ISOMETRIC_TILE => {
            Ok((tile_width.max(1), tile_height.max(1)))
        }
        Projection::Isometric { .. } => Err(Error::TileTooLarge),
        Projection::TopDown { tile_size } => {
            let side = tile_size.clamp(1, MAXIMUM_SWATCH_SIDE);
            Ok((side, side))
        }
    }
}

/// The `ATX1` envelope of a swatch: `color` where the pattern covers the
/// tile's shape, a diamond in isometric projection, and clear elsewhere.
fn swatch_pixels(
    projection: Projection,
    (width, height): (u32, u32),
    pattern: Pattern,
    color: [u8; 4],
) -> Vec<u8> {
    let diamond = matches!(projection, Projection::Isometric { .. });
    let mut bytes = Vec::with_capacity(12 + 4 * usize::try_from(width * height).unwrap_or(0));
    bytes.extend_from_slice(b"ATX1");
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            let u = (f64::from(x) + 0.5) / f64::from(width) * 2.0 - 1.0;
            let v = (f64::from(y) + 0.5) / f64::from(height) * 2.0 - 1.0;
            // A diamond's own coordinates, so its patterns follow its edges.
            let (u, v, inside) = if diamond {
                (u + v, v - u, u.abs() + v.abs() <= 1.0)
            } else {
                (u, v, true)
            };
            let pixel = if inside && pattern.covers(u, v) {
                color
            } else {
                [0, 0, 0, 0]
            };
            bytes.extend_from_slice(&pixel);
        }
    }
    bytes
}

/// Which plane tile each pickable object of a ground scene stands for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PickMap {
    /// The tile of semantic ID `index + 1`.
    tiles: Vec<[i64; 2]>,
}

impl PickMap {
    /// The tile an object of the scene stands for.
    #[must_use]
    pub fn tile(&self, id: SemanticId) -> Option<[i64; 2]> {
        let index = usize::try_from(id.0).ok()?.checked_sub(1)?;
        self.tiles.get(index).copied()
    }

    /// The tile under pixel `(x, y)` of a frame rendered from the scene,
    /// read from the frame's semantic mask. Where the mask holds no tile, as
    /// on the one-pixel seams the renderer leaves between tiles at some
    /// zooms, the nearest tile within [`PICK_REACH`] pixels is the one
    /// under the pointer.
    #[must_use]
    pub fn tile_at(&self, frame: &FrameOutput, x: u32, y: u32) -> Option<[i64; 2]> {
        if x >= frame.width || y >= frame.height {
            return None;
        }
        let id = |x: i64, y: i64| -> Option<u32> {
            let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
            if x >= frame.width || y >= frame.height {
                return None;
            }
            let index =
                usize::try_from(u64::from(y) * u64::from(frame.width) + u64::from(x)).ok()?;
            frame.semantic_ids.get(index).copied().filter(|id| *id != 0)
        };
        let (x, y) = (i64::from(x), i64::from(y));
        // Rings of growing distance, each read in a fixed order, so the
        // same pixel always picks the same tile.
        let found = (0..=PICK_REACH).find_map(|reach| {
            (-reach..=reach)
                .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
                .filter(|(dx, dy)| dx.abs().max(dy.abs()) == reach)
                .find_map(|(dx, dy)| id(x + dx, y + dy))
        })?;
        self.tile(SemanticId(found))
    }

    fn push(&mut self, at: [i64; 2]) -> Option<SemanticId> {
        self.tiles.push(at);
        u32::try_from(self.tiles.len()).ok().map(SemanticId)
    }
}

/// A ground view ready to render.
#[derive(Debug)]
pub struct GroundScene {
    pub scene: SceneBundle,
    pub picks: PickMap,
    pub resources: Arc<Swatches>,
    /// How many tiles the view drew, empty ones included.
    pub tiles: usize,
}

/// The tiles a camera can show, as an inclusive range, with a margin of a
/// tile. It is a bound for choosing what to send to the renderer, which
/// projects and culls them itself; it is never used to place or pick.
#[must_use]
pub fn view_bounds(camera: &GroundCamera) -> ([i64; 2], [i64; 2]) {
    let zoom = f64::from(camera.zoom.max(f32::MIN_POSITIVE));
    let (width, height) = (f64::from(camera.width), f64::from(camera.height));
    let (half_x, half_y) = match camera.projection {
        Projection::TopDown { tile_size } => {
            let side = f64::from(tile_size.max(1)) * zoom;
            (width / side / 2.0, height / side / 2.0)
        }
        Projection::Isometric {
            tile_width,
            tile_height,
        } => {
            // A screen point at (sx, sy) from the centre lies on tile
            // offsets ((sx / w + sy / h) / 2, (sy / h - sx / w) / 2) for
            // the half sizes w and h, so each offset is at most half the sum
            // of the two.
            let across = width / 2.0 / (f64::from(tile_width.max(1)) * zoom / 2.0);
            let down = height / 2.0 / (f64::from(tile_height.max(1)) * zoom / 2.0);
            let reach = f64::midpoint(across, down);
            (reach, reach)
        }
    };
    // Float to integer casts saturate, so a far camera stays in range.
    #[allow(clippy::cast_possible_truncation)]
    let tile = |value: f64| value as i64;
    let [x, y] = camera.center;
    (
        [
            tile((x - half_x).floor()).saturating_sub(1),
            tile((y - half_y).floor()).saturating_sub(1),
        ],
        [
            tile((x + half_x).ceil()).saturating_add(1),
            tile((y + half_y).ceil()).saturating_add(1),
        ],
    )
}

/// Builds the scene of a ground view: `tiles` drawn as swatches, each
/// ground layer at its render rank in `ranks`, over empty tiles everywhere
/// else in view.
///
/// # Errors
/// Fails when the view holds more than [`MAXIMUM_VIEW_TILES`] tiles, a
/// ground layer has no rank, or the renderer rejects the view.
pub fn ground_scene(
    camera: &GroundCamera,
    ranks: &[u16],
    tiles: &[GroundTile<'_>],
    revision: u64,
) -> Result<GroundScene, Error> {
    let (low, high) = view_bounds(camera);
    let span = |low: i64, high: i64| u64::try_from(i128::from(high) - i128::from(low) + 1).ok();
    let area = span(low[0], high[0])
        .zip(span(low[1], high[1]))
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|area| usize::try_from(area).ok())
        .filter(|area| *area <= MAXIMUM_VIEW_TILES)
        .ok_or(Error::TooManyTiles)?;
    let inside =
        |[x, y]: [i64; 2]| (low[0]..=high[0]).contains(&x) && (low[1]..=high[1]).contains(&y);
    let mut shown: Vec<&GroundTile<'_>> = tiles.iter().filter(|tile| inside(tile.at)).collect();
    if shown.len().saturating_add(area) > MAXIMUM_VIEW_OBJECTS {
        return Err(Error::TooManyTiles);
    }
    shown.sort_by_key(|tile| (tile.at[1], tile.at[0], tile.ground));
    let (width, height) = swatch_size(camera.projection)?;
    let mut swatches = Swatches::default();
    // Each texture is made and hashed once, however many tiles show it.
    let mut texture = |key: &str, pattern: Pattern, color: [u8; 4]| -> TextureRef {
        let id = texture_id(key, pattern, width, height);
        let (_, digest) = swatches.textures.entry(id).or_insert_with(|| {
            let bytes = swatch_pixels(camera.projection, (width, height), pattern, color);
            let digest = Digest256(Sha256::digest(bytes.as_slice()).into());
            (bytes, digest)
        });
        TextureRef {
            id,
            revision: 1,
            digest: *digest,
            width,
            height,
        }
    };
    let mut picks = PickMap::default();
    let mut objects = Vec::with_capacity(shown.len() + area);
    let mut covered = BTreeSet::new();
    for tile in &shown {
        let rank = *ranks
            .get(tile.ground)
            .ok_or(Error::UnknownGround(tile.ground))?;
        let [red, green, blue] = swatch_color(tile.proto);
        let texture = texture(
            tile.proto,
            Pattern::of(tile.ground),
            [red, green, blue, 0xff],
        );
        let id = picks.push(tile.at).ok_or(Error::TooManyTiles)?;
        objects.push(WorldObject::new(id, tile.at, rank, Anchor::Ground, texture));
        // Only a solid swatch covers its whole tile; under the others an
        // empty tile keeps every pixel of the tile pickable.
        if Pattern::of(tile.ground) == Pattern::Solid {
            covered.insert(tile.at);
        }
    }
    let empty = texture("", Pattern::Solid, EMPTY);
    let lowest = ranks.iter().copied().min().unwrap_or(0);
    for y in low[1]..=high[1] {
        for x in low[0]..=high[0] {
            if covered.contains(&[x, y]) {
                continue;
            }
            let id = picks.push([x, y]).ok_or(Error::TooManyTiles)?;
            objects.push(WorldObject::new(id, [x, y], lowest, Anchor::Ground, empty));
        }
    }
    let count = objects.len();
    let view = WorldView {
        revision,
        clock_millis: 0,
        camera: Camera {
            projection: camera.projection,
            center: camera.center,
            zoom: camera.zoom,
            width: camera.width,
            height: camera.height,
            sampling: Sampling::Nearest,
        },
        clear_color: CLEAR,
        heights: HeightField::empty(),
        stretch: Stretch::default(),
        ground_layer_ranks: ranks
            .iter()
            .copied()
            .max()
            .map_or(0, |rank| rank.saturating_add(1)),
        objects,
    };
    let scene =
        tilewright_render_world::compile(&view, SceneLimits::default()).map_err(Error::Render)?;
    Ok(GroundScene {
        scene,
        picks,
        resources: Arc::new(swatches),
        tiles: count,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The view would hold more than [`MAXIMUM_VIEW_TILES`] tiles.
    TooManyTiles,
    /// Isometric tiles wider or taller than [`MAXIMUM_ISOMETRIC_TILE`].
    TileTooLarge,
    UnknownGround(usize),
    Render(tilewright_render_world::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyTiles => write!(
                formatter,
                "the view holds more than {MAXIMUM_VIEW_TILES} tiles; zoom in"
            ),
            Self::TileTooLarge => write!(
                formatter,
                "the view shows isometric tiles up to {MAXIMUM_ISOMETRIC_TILE} pixels across"
            ),
            Self::UnknownGround(ground) => {
                write!(formatter, "ground layer {ground} has no render layer")
            }
            Self::Render(error) => write!(formatter, "the renderer rejected the view: {error:?}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{
        Error, GroundCamera, GroundTile, MAXIMUM_VIEW_TILES, ground_scene, swatch_color,
        view_bounds,
    };
    use tilewright_render_api::{
        BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
    };
    use tilewright_render_testkit::ReferenceRenderer;
    use tilewright_render_world::Projection;

    const TOP_DOWN: Projection = Projection::TopDown { tile_size: 32 };

    fn camera(projection: Projection, center: [f64; 2], zoom: f32) -> GroundCamera {
        GroundCamera {
            projection,
            center,
            zoom,
            width: 96,
            height: 96,
        }
    }

    fn render(camera: &GroundCamera, tiles: &[GroundTile<'_>]) -> (FrameOutput, super::PickMap) {
        let built = ground_scene(camera, &[0, 1, 2], tiles, 1).unwrap();
        let mut renderer = ReferenceRenderer::new(TargetDescriptor {
            kind: TargetKind::Offscreen,
            width: camera.width,
            height: camera.height,
            backend: BackendPreference::Automatic,
        })
        .unwrap();
        let frame = renderer
            .render(RenderRequest {
                scene: &built.scene,
                resources: built.resources.clone(),
            })
            .unwrap();
        (frame, built.picks)
    }

    fn pixel(frame: &FrameOutput, x: u32, y: u32) -> [u8; 3] {
        let index = usize::try_from((y * frame.width + x) * 4).unwrap();
        [
            frame.rgba8[index],
            frame.rgba8[index + 1],
            frame.rgba8[index + 2],
        ]
    }

    #[test]
    fn swatches_are_stable_and_tell_prototypes_apart() {
        assert_eq!(swatch_color("grass"), swatch_color("grass"));
        assert_ne!(swatch_color("grass"), swatch_color("water"));
        assert_ne!(swatch_color("sand"), swatch_color("grass_edge_e"));
    }

    #[test]
    fn the_semantic_mask_finds_painted_and_empty_tiles() {
        let tiles = [
            GroundTile {
                at: [2, 2],
                ground: 0,
                proto: "grass",
            },
            GroundTile {
                at: [2, 2],
                ground: 2,
                proto: "grass_edge_e",
            },
            GroundTile {
                at: [3, 2],
                ground: 0,
                proto: "water",
            },
        ];
        let view = camera(TOP_DOWN, [2.5, 2.5], 1.0);
        let (frame, picks) = render(&view, &tiles);
        // The view's centre is the middle of tile (2, 2).
        assert_eq!(picks.tile_at(&frame, 48, 48), Some([2, 2]));
        assert_eq!(picks.tile_at(&frame, 48 + 32, 48), Some([3, 2]));
        // An unpainted tile is found too.
        assert_eq!(picks.tile_at(&frame, 1, 1), Some([1, 1]));
        assert_eq!(picks.tile_at(&frame, 96, 0), None);
        // The ground shows inside the edge's frame, and the edge around it.
        let [red, green, blue] = swatch_color("grass");
        assert_eq!(pixel(&frame, 48, 48), [red, green, blue]);
        let [red, green, blue] = swatch_color("grass_edge_e");
        assert_eq!(pixel(&frame, 33, 33), [red, green, blue]);
        let [red, green, blue] = swatch_color("water");
        assert_eq!(pixel(&frame, 48 + 32, 48), [red, green, blue]);
    }

    #[test]
    fn isometric_views_pick_their_diamonds() {
        let iso = Projection::Isometric {
            tile_width: 48,
            tile_height: 24,
        };
        let tiles = [GroundTile {
            at: [0, 0],
            ground: 0,
            proto: "grass",
        }];
        let view = camera(iso, [0.5, 0.5], 1.0);
        let (frame, picks) = render(&view, &tiles);
        assert_eq!(picks.tile_at(&frame, 48, 48), Some([0, 0]));
        let (low, high) = view_bounds(&view);
        assert!(low[0] <= -2 && high[0] >= 2 && low[1] <= -2 && high[1] >= 2);
    }

    #[test]
    fn a_view_zoomed_far_out_asks_to_zoom_in() {
        let view = GroundCamera {
            width: 4_000,
            height: 4_000,
            ..camera(TOP_DOWN, [0.0, 0.0], 0.05)
        };
        assert_eq!(
            ground_scene(&view, &[0], &[], 1).unwrap_err(),
            Error::TooManyTiles
        );
        let near = GroundCamera {
            width: 4_000,
            height: 4_000,
            ..camera(TOP_DOWN, [0.0, 0.0], 1.0)
        };
        let built = ground_scene(&near, &[0], &[], 1).unwrap();
        assert!(built.tiles <= MAXIMUM_VIEW_TILES);
    }

    #[test]
    fn panning_keeps_the_dragged_tile_under_the_pointer() {
        // Top-down: a drag of one tile to the right shows the tile left of
        // the old centre.
        let mut view = camera(TOP_DOWN, [2.5, 2.5], 2.0);
        view.pan([64.0, 0.0]);
        assert!((view.center[0] - 1.5).abs() < 1e-9 && (view.center[1] - 2.5).abs() < 1e-9);
        let iso = Projection::Isometric {
            tile_width: 48,
            tile_height: 24,
        };
        let tiles = [GroundTile {
            at: [0, 0],
            ground: 0,
            proto: "grass",
        }];
        let mut moved = camera(iso, [0.5, 0.5], 1.0);
        moved.pan([24.0, 12.0]);
        // Tile (0, 0) was at the centre; now it is half a tile right and
        // down, where tile (1, 0)'s diamond was.
        let (frame, picks) = render(&moved, &tiles);
        assert_eq!(picks.tile_at(&frame, 48 + 24, 48 + 12), Some([0, 0]));
    }

    /// How many pixels of a frame no tile covers.
    fn holes(frame: &FrameOutput) -> usize {
        frame.semantic_ids.iter().filter(|id| **id == 0).count()
    }

    fn sized(projection: Projection, zoom: f32, width: u32, height: u32) -> GroundCamera {
        GroundCamera {
            projection,
            center: [0.5, 0.5],
            zoom,
            width,
            height,
        }
    }

    #[test]
    fn isometric_bounds_stay_close_to_what_is_visible() {
        let iso = Projection::Isometric {
            tile_width: 48,
            tile_height: 24,
        };
        let view = sized(iso, 1.0, 960, 640);
        let (low, high) = view_bounds(&view);
        let area = (high[0] - low[0] + 1) * (high[1] - low[1] + 1);
        let built = ground_scene(&view, &[0], &[], 1).unwrap();
        let visible = i64::try_from(built.scene.quads().len()).unwrap();
        // The bounds are a square around a diamond of visible tiles, about
        // twice its area, with a margin.
        assert!(area < visible * 3, "area {area}, visible {visible}");
        // A large window at zoom 1 fits within the cap.
        let large = sized(iso, 1.0, 2_340, 1_500);
        assert!(ground_scene(&large, &[0], &[], 1).is_ok());
    }

    #[test]
    fn picks_on_seams_find_the_neighbouring_tile() {
        let iso = Projection::Isometric {
            tile_width: 48,
            tile_height: 24,
        };
        // At a fractional zoom the renderer leaves one-pixel seams.
        let view = sized(iso, 1.1, 201, 161);
        let (frame, picks) = render(&view, &[]);
        let seams: Vec<(u32, u32)> = (0..frame.height)
            .flat_map(|y| (0..frame.width).map(move |x| (x, y)))
            .filter(|(x, y)| frame.semantic_ids[(y * frame.width + x) as usize] == 0)
            .collect();
        assert!(!seams.is_empty(), "the renderer drew no seam to test");
        for (x, y) in seams {
            let tile = picks.tile_at(&frame, x, y);
            assert!(tile.is_some(), "no tile near seam pixel ({x}, {y})");
        }
    }

    #[test]
    fn large_isometric_tiles_are_covered_or_refused() {
        let wide = Projection::Isometric {
            tile_width: 320,
            tile_height: 160,
        };
        let (frame, _) = render(&sized(wide, 1.0, 600, 400), &[]);
        assert_eq!(holes(&frame), 0);
        let huge = Projection::Isometric {
            tile_width: 2_048,
            tile_height: 1_024,
        };
        assert_eq!(
            ground_scene(&sized(huge, 0.25, 600, 400), &[0], &[], 1).unwrap_err(),
            Error::TileTooLarge
        );
    }

    #[test]
    fn upper_layer_tiles_alone_stay_pickable_everywhere() {
        for ground in [1, 2] {
            let tiles = [GroundTile {
                at: [2, 2],
                ground,
                proto: "grass_edge_e",
            }];
            let (frame, picks) = render(&camera(TOP_DOWN, [2.5, 2.5], 1.0), &tiles);
            assert_eq!(picks.tile_at(&frame, 34, 34), Some([2, 2]));
            assert_eq!(picks.tile_at(&frame, 48, 48), Some([2, 2]));
        }
    }

    #[test]
    fn a_far_camera_saturates_instead_of_overflowing() {
        let view = camera(TOP_DOWN, [f64::MAX, -f64::MAX], 1.0);
        let (low, high) = view_bounds(&view);
        assert!(low[0] <= high[0] && low[1] <= high[1]);
    }
}
