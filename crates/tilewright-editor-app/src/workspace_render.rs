// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Scene assembly only. Typography, clipping and quad generation belong to the
//! shared renderer. Palette thumbnails are whole, unclipped, unpickable quads
//! that sample ready World textures; rows that are not wholly visible get
//! none. A running brush stroke's live preview is unpickable copies of the
//! prepared exact cell quads, placed in the World's own draw order. The
//! World scene and
//! its exact picking masks remain untouched.
use crate::workspace_ui::{Rect, Widget, WorkspaceUi};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};
use tilewright_editor_preview::{PreparedPreview, SelectionKey};
use tilewright_editor_ui::text_input::{TextInputSnapshot, TextRevision};
use tilewright_render_resources::{
    Error as ResourceError, ResourceBlob, ResourceProvider, ResourceRequest,
};
use tilewright_render_ui::{
    TextAffinity, TextCursor, TextSystem, UiDisplayList, UiLimits, UiPaint, UiPrimitive, UiRect,
    UiTexture, UiViewport, UnicodeLayout, UnicodeTexture, builtin,
};
use tilewright_scene::{
    Digest256, Layer, Quad, ResourceId, Sampling, SceneBundle, SceneLimits, SemanticId,
};

type Result<T> = std::result::Result<T, CompositionError>;

#[derive(Debug)]
pub enum CompositionError {
    TargetMismatch,
    SemanticIdentityExhausted,
    ResourceIdentityExhausted,
    PainterOrderExhausted,
    Text(tilewright_render_ui::TextError),
    Scene(tilewright_scene::Error),
}
impl fmt::Display for CompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(error) => fmt::Display::fmt(error, formatter),
            Self::Scene(error) => fmt::Display::fmt(error, formatter),
            other => write!(formatter, "UI composition refused: {other:?}"),
        }
    }
}
impl std::error::Error for CompositionError {}

#[derive(Clone, Debug)]
pub struct EditableGeometry {
    pub revision: TextRevision,
    pub layout: UnicodeLayout,
    pub origin: [f32; 2],
    pub clip: UiRect,
    pub snapshot: TextInputSnapshot,
}
pub struct Presentation {
    pub scene: SceneBundle,
    pub resources: Arc<dyn ResourceProvider>,
    pub unsupported_runs: usize,
    pub editable: Option<EditableGeometry>,
}
struct Resources {
    world: Arc<dyn ResourceProvider>,
    textures: Vec<UnicodeTexture>,
    solid: UiTexture,
}
impl ResourceProvider for Resources {
    fn load(&self, request: ResourceRequest) -> std::result::Result<ResourceBlob, ResourceError> {
        request.validate()?;
        let expected = self
            .textures
            .iter()
            .find(|texture| texture.identity.id == request.id)
            .map(|texture| (texture.identity, texture.bytes.as_ref()))
            .or_else(|| {
                (request.id == self.solid.id).then_some((self.solid, builtin::SOLID_BYTES))
            });
        if let Some((identity, bytes)) = expected {
            if request.revision != identity.revision || request.digest != identity.digest {
                return Err(ResourceError::DigestMismatch);
            }
            if bytes.len() > request.maximum_bytes {
                return Err(ResourceError::LimitExceeded);
            }
            return ResourceBlob::for_request(request, bytes.to_vec());
        }
        self.world.load(request)
    }
}
/// Logical inset of a row thumbnail's square box from the row's top-left.
pub const THUMBNAIL_INSET: u32 = 2;
/// Most thumbnails one frame draws. Only visible rows carry one, and the
/// sidebar shows far fewer rows than this at any supported size.
pub const MAX_THUMBNAILS: usize = 64;

/// Logical text indent of a row that starts with a thumbnail: the square
/// box plus a gap.
pub const fn thumbnail_indent(row_height: u32) -> u32 {
    row_height
}

/// Physical pixel rectangle of the thumbnail of an image of `size` texels
/// in the logical `row`: a square box inset at the row's start, the image
/// scaled to fit it with its aspect ratio kept, centred, and snapped to
/// whole physical pixels. `None` unless the row lies wholly inside the
/// logical `clip` (the sidebar). Rows are whole, so a thumbnail is drawn
/// whole or not at all; it never needs partial clipping and can never
/// reach the toolbar, the review panel or the World view.
pub fn thumbnail_rect(ui: &WorkspaceUi, row: Rect, clip: Rect, size: (u32, u32)) -> Option<Rect> {
    let end = |start: u32, length: u32| u64::from(start) + u64::from(length);
    if row.x < clip.x
        || row.y < clip.y
        || end(row.x, row.width) > end(clip.x, clip.width)
        || end(row.y, row.height) > end(clip.y, clip.height)
        || size.0 == 0
        || size.1 == 0
    {
        return None;
    }
    let square = row.height.checked_sub(2 * THUMBNAIL_INSET)?;
    if square == 0 || row.width < thumbnail_indent(row.height) {
        return None;
    }
    let boxed = ui.scale(Rect {
        x: row.x + THUMBNAIL_INSET,
        y: row.y + THUMBNAIL_INSET,
        width: square,
        height: square,
    });
    if boxed.width == 0 || boxed.height == 0 {
        return None;
    }
    let (width, height) = (u64::from(size.0), u64::from(size.1));
    let (box_width, box_height) = (u64::from(boxed.width), u64::from(boxed.height));
    // Round to nearest; the limiting side fills the box exactly.
    let (fitted_width, fitted_height) = if width * box_height <= height * box_width {
        ((width * box_height * 2 + height) / (height * 2), box_height)
    } else {
        (box_width, (height * box_width * 2 + width) / (width * 2))
    };
    let fitted_width = u32::try_from(fitted_width.clamp(1, box_width)).ok()?;
    let fitted_height = u32::try_from(fitted_height.clamp(1, box_height)).ok()?;
    Some(Rect {
        x: boxed.x + (boxed.width - fitted_width) / 2,
        y: boxed.y + (boxed.height - fitted_height) / 2,
        width: fitted_width,
        height: fitted_height,
    })
}

/// One textured, unpickable interface quad per visible thumbnail row, at
/// most [`MAX_THUMBNAILS`]. Each samples the prototype's whole ready
/// texture, the same identity the World scene uses. Semantic identities
/// start after `identity`; painter order matches the row's text, above the
/// row's background.
#[allow(clippy::cast_precision_loss)]
fn thumbnail_quads(ui: &WorkspaceUi, identity: u32) -> Result<Vec<Quad>> {
    let mut quads = Vec::new();
    let mut next = identity;
    for (index, widget) in ui.widgets.iter().enumerate() {
        let Some(texture) = widget.thumbnail else {
            continue;
        };
        if quads.len() == MAX_THUMBNAILS {
            break;
        }
        let Some(target) = thumbnail_rect(
            ui,
            widget.layout,
            ui.logical_sidebar,
            (texture.width, texture.height),
        ) else {
            continue;
        };
        let order = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(2))
            .and_then(|index| index.checked_add(2))
            .ok_or(CompositionError::PainterOrderExhausted)?;
        next = next
            .checked_add(1)
            .ok_or(CompositionError::SemanticIdentityExhausted)?;
        let (left, top) = (target.x as f32, target.y as f32);
        let right = (target.x + target.width) as f32;
        let bottom = (target.y + target.height) as f32;
        quads.push(Quad {
            semantic_id: SemanticId(next),
            part: 0,
            resource_id: texture.id,
            resource_revision: texture.revision,
            resource_digest: texture.digest,
            frame: None,
            positions: [[left, top], [right, top], [right, bottom], [left, bottom]],
            uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            depth: 0,
            painter_order: order,
            color: if widget.enabled {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [1.0, 1.0, 1.0, 0.5]
            },
            grayscale: if widget.enabled { 0.0 } else { 1.0 },
            glow: None,
            pickable: false,
            sampling: Sampling::Linear,
            layer: Layer::Interface,
        });
    }
    Ok(quads)
}

/// Tint of the live brush-stroke preview: neutral white at 45 % opacity,
/// multiplied with the opaque cell reference art, so each marked cell is
/// lightened evenly whatever its terrain. It has little contrast on very
/// light terrain.
pub const STROKE_PREVIEW_TINT: [f32; 4] = [1.0, 1.0, 1.0, 0.45];

/// The live brush-stroke preview: a copy of every quad of the prepared
/// exact cell scene whose cell is in `cells`, tinted with
/// [`STROKE_PREVIEW_TINT`] and unpickable. The cell scene is what the exact
/// cell mask is rasterised from, compiled by the shared renderer with the
/// same camera and height field as the terrain, so the copies have each
/// cell's projected shape, including height-bent and stretched tiles split
/// into several quads. Nothing is projected here.
///
/// The copies are World-layer quads placed in the prepared World scene's
/// own draw order. `PreparedPreview::cell_slots`, computed on first use per
/// prepared scene by compiling the World's terrain, the cell reference
/// objects and the World's placed objects together with the shared
/// renderer, gives each cell the `(depth, painter order)` of the last World
/// quad drawn before it; composition gives the copy a semantic identity
/// above every World quad, so it draws directly after that quad and before
/// anything the World draws later: objects on the tile (including objects
/// on the grid's own ground render layer), nearer tiles and higher ground
/// layers cover it as they cover the terrain. Cells without terrain get
/// their own position in the order, not an earlier cell's. A cell drawn
/// before every World quad draws first. When the slots are unavailable
/// (the combined scene could not be compiled) there are no copies at all;
/// callers say so. The copies are a subset of a scene that already met the
/// scene limits; semantic identities are assigned when composed. Copies of
/// two overlays in one slot draw in the order the overlays are passed to
/// [`compose_with_overlay`].
pub fn stroke_overlay(prepared: &PreparedPreview, cells: &BTreeSet<SelectionKey>) -> Vec<Quad> {
    world_ordered_copies(prepared, |key| {
        cells.contains(key).then_some(STROKE_PREVIEW_TINT)
    })
}

/// Unpickable copies of the prepared cell quads of every cell `tint` gives
/// a colour, in that colour, placed in the World scene's draw order as
/// [`stroke_overlay`] describes.
fn world_ordered_copies(
    prepared: &PreparedPreview,
    tint: impl Fn(&SelectionKey) -> Option<[f32; 4]>,
) -> Vec<Quad> {
    world_ordered_cells(prepared, tint)
        .into_iter()
        .map(|(_, quad)| quad)
        .collect()
}

/// [`world_ordered_copies`] with the cell each copy belongs to.
fn world_ordered_cells(
    prepared: &PreparedPreview,
    tint: impl Fn(&SelectionKey) -> Option<[f32; 4]>,
) -> Vec<(&SelectionKey, Quad)> {
    let Some(slots) = prepared.cell_slots() else {
        return Vec::new();
    };
    prepared
        .cell_scene
        .quads()
        .iter()
        .filter_map(|quad| {
            let key = prepared.cell_picks.get(&quad.semantic_id)?;
            let color = tint(key)?;
            let (depth, painter_order) = slots
                .get(&quad.semantic_id)
                .copied()
                .flatten()
                .unwrap_or((quad.depth.saturating_sub(1), 0));
            Some((
                key,
                Quad {
                    color,
                    grayscale: 0.0,
                    glow: None,
                    pickable: false,
                    layer: Layer::World,
                    depth,
                    painter_order,
                    ..*quad
                },
            ))
        })
        .collect()
}

/// The badge on a tile that holds an anchor: light, in the tile's first
/// corner (the leftmost vertex, the top one of two).
pub const ANCHOR_BADGE: [f32; 4] = [0.97, 0.97, 0.94, 0.95];
/// The badge on a tile that holds an object with a portal: dark, in the
/// tile's last corner (the rightmost vertex, the top one of two).
pub const PORTAL_BADGE: [f32; 4] = [0.10, 0.10, 0.16, 0.92];
/// A badge is its tile's shape scaled by this much toward its corner.
const BADGE_SCALE: f32 = 0.22;
/// The corner a badge is scaled toward sits this far in from the tile's
/// vertex toward the tile's centre, so the badge does not touch the edge.
const BADGE_INSET: f32 = 0.12;

/// Which corner of its tile a badge takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Corner {
    First,
    Last,
}

/// The badges: on every tile of `anchors` a small light copy of the tile in
/// its first corner, and on every tile of `portals` a small dark copy in its
/// last corner. Each copy is the tile's own prepared cell shape, scaled
/// toward a vertex of the tile, so it stays inside the tile in either
/// projection, and is placed in the World scene's draw order exactly like
/// the zone tints ([`zone_overlay`]): drawn after its tile's terrain and
/// before anything the World draws later, so a badge never covers an object
/// on its tile or a nearer tile. Unpickable; composed after the zone
/// overlay, so it draws over its tile's tint. Nothing is drawn when the
/// cell slots are unavailable.
pub fn badge_overlay(
    prepared: &PreparedPreview,
    anchors: &BTreeSet<[i64; 2]>,
    portals: &BTreeSet<[i64; 2]>,
) -> Vec<Quad> {
    let mut badges = Vec::new();
    for (tiles, corner, color) in [
        (anchors, Corner::First, ANCHOR_BADGE),
        (portals, Corner::Last, PORTAL_BADGE),
    ] {
        if tiles.is_empty() {
            continue;
        }
        let mut cells: BTreeMap<[i64; 2], Vec<Quad>> = BTreeMap::new();
        for (key, quad) in world_ordered_cells(prepared, |key| match key {
            SelectionKey::Cell { x, y, .. } if tiles.contains(&[*x, *y]) => Some(color),
            _ => None,
        }) {
            if let SelectionKey::Cell { x, y, .. } = key {
                cells.entry([*x, *y]).or_default().push(quad);
            }
        }
        for quads in cells.into_values() {
            badges.extend(shrink(quads, corner));
        }
    }
    badges
}

/// The quads of one tile scaled by [`BADGE_SCALE`] toward its `corner`.
fn shrink(quads: Vec<Quad>, corner: Corner) -> Vec<Quad> {
    let points = || quads.iter().flat_map(|quad| quad.positions);
    // Leftmost then topmost; rightmost then topmost.
    let vertex = match corner {
        Corner::First => points().min_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1]))),
        Corner::Last => points().max_by(|a, b| a[0].total_cmp(&b[0]).then(b[1].total_cmp(&a[1]))),
    };
    let Some(vertex) = vertex else {
        return Vec::new();
    };
    #[allow(clippy::cast_precision_loss)]
    let count = points().count() as f32;
    let centre = points().fold([0.0, 0.0], |sum, p| [sum[0] + p[0], sum[1] + p[1]]);
    let centre = [centre[0] / count, centre[1] / count];
    let fixed = [
        vertex[0] + BADGE_INSET * (centre[0] - vertex[0]),
        vertex[1] + BADGE_INSET * (centre[1] - vertex[1]),
    ];
    quads
        .into_iter()
        .map(|quad| Quad {
            positions: quad.positions.map(|p| {
                [
                    fixed[0] + BADGE_SCALE * (p[0] - fixed[0]),
                    fixed[1] + BADGE_SCALE * (p[1] - fixed[1]),
                ]
            }),
            ..quad
        })
        .collect()
}

/// The zone overlay: unpickable copies of the prepared cell quads of every
/// tile that has a tint in `tints` (keyed by plane tile), in that tint,
/// placed in the World scene's draw order exactly like the stroke preview
/// ([`stroke_overlay`]): each draws directly after its tile's terrain and
/// before anything the World draws later, so a tint never covers an object
/// or a nearer tile, and the part of a tile an object hides shows no tint.
/// Composed before the stroke preview, a preview mark of the same cell
/// draws over the tint. Tiles without a tint get no quad.
pub fn zone_overlay(prepared: &PreparedPreview, tints: &BTreeMap<[i64; 2], [f32; 4]>) -> Vec<Quad> {
    world_ordered_copies(prepared, |key| match key {
        SelectionKey::Cell { x, y, .. } => tints.get(&[*x, *y]).copied(),
        SelectionKey::Object { .. } => None,
    })
}

/// Logical rectangle of a row's colour swatch: the thumbnail box.
pub fn swatch_rect(row: Rect) -> Option<Rect> {
    let square = row.height.checked_sub(2 * THUMBNAIL_INSET)?;
    (square > 0 && row.width >= thumbnail_indent(row.height)).then_some(Rect {
        x: row.x + THUMBNAIL_INSET,
        y: row.y + THUMBNAIL_INSET,
        width: square,
        height: square,
    })
}

/// Whether `text` fits a row of `width` by `height` logical pixels whole,
/// by the same measure [`fit_text`] cuts with.
pub fn fits(system: &mut TextSystem, text: &str, width: u32, height: u32) -> bool {
    fit_text(system, text, width, height) == text
}

/// Text naming the identifier `id` that keeps it distinguishable in a row
/// of `width` by `height`: the first `(prefix, suffix)` of `variants` whose
/// whole text fits; else the last variant's prefix and as much of `id` as
/// fits, elided in the middle so its head and its tail both stay.
/// Hierarchical ids share prefixes, so cutting only the end would hide what
/// tells two of them apart.
pub fn fit_id(
    system: &mut TextSystem,
    id: &str,
    variants: &[(String, String)],
    width: u32,
    height: u32,
) -> String {
    for (prefix, suffix) in variants {
        let text = format!("{prefix}{id}{suffix}");
        if fits(system, &text, width, height) {
            return text;
        }
    }
    let prefix = variants.last().map_or("", |(prefix, _)| prefix.as_str());
    middle_elided(system, prefix, id, width, height)
}

/// `prefix` and the longest middle elision of `id` that fits, keeping as
/// many characters of the tail as of the head, or one more.
fn middle_elided(
    system: &mut TextSystem,
    prefix: &str,
    id: &str,
    width: u32,
    height: u32,
) -> String {
    let chars: Vec<char> = id.chars().collect();
    let elide = |kept: usize| {
        let head = kept / 2;
        let tail = kept - head;
        format!(
            "{prefix}{}...{}",
            chars[..head].iter().collect::<String>(),
            chars[chars.len() - tail..].iter().collect::<String>()
        )
    };
    // Largest kept count that fits, by bisection over 0..len.
    let (mut low, mut high) = (0, chars.len());
    if !fits(system, &elide(0), width, height) {
        return fit_text(system, &format!("{prefix}{id}"), width, height);
    }
    while low + 1 < high {
        let middle = usize::midpoint(low, high);
        if fits(system, &elide(middle), width, height) {
            low = middle;
        } else {
            high = middle;
        }
    }
    elide(low)
}

/// `Path: ` and a zone chain, outermost first, joined by ` > `, fitted to
/// the row: the outer zones give way to `...` first; the last (the tile's
/// own) zone is kept whole, or elided in its middle when even it alone does
/// not fit.
pub fn fit_path(system: &mut TextSystem, path: &[String], width: u32, height: u32) -> String {
    let whole = format!("Path: {}", path.join(" > "));
    if fits(system, &whole, width, height) || path.is_empty() {
        return whole;
    }
    for start in 1..path.len() {
        let text = format!("Path: ... > {}", path[start..].join(" > "));
        if fits(system, &text, width, height) {
            return text;
        }
    }
    let prefix = if path.len() > 1 {
        "Path: ... > "
    } else {
        "Path: "
    };
    middle_elided(system, prefix, &path[path.len() - 1], width, height)
}

pub fn text_size(height: u32) -> f32 {
    if height <= 22 { 14.0 } else { 18.0 }
}

/// Elision uses shared shaped measurements and original grapheme boundaries.
#[allow(clippy::cast_precision_loss)]
pub fn fit_text(system: &mut TextSystem, text: &str, width: u32, height: u32) -> String {
    let available = width.saturating_sub(8) as f32;
    let size = text_size(height);
    let Ok(layout) = system.layout(text, size, None) else {
        return "Text unavailable".into();
    };
    if layout.size()[0] <= available {
        return text.into();
    }
    fitted_end(system, &layout, size, available, "...")
        .map_or_else(String::new, |end| format!("{}...", &text[..end]))
}
#[allow(clippy::cast_precision_loss)]
pub fn wrap_text(system: &mut TextSystem, text: &str, width: u32, height: u32) -> Vec<String> {
    let available = width.saturating_sub(8) as f32;
    let mut remaining = text;
    let mut lines = Vec::new();
    while !remaining.is_empty() && lines.len() < 256 {
        let size = text_size(height);
        let Ok(layout) = system.layout(remaining, size, None) else {
            break;
        };
        let end = if layout.size()[0] <= available {
            Some(remaining.len())
        } else {
            fitted_end(system, &layout, size, available, "")
        };
        let Some(end) = end.filter(|end| *end > 0) else {
            break;
        };
        lines.push(remaining[..end].into());
        remaining = &remaining[end..];
    }
    lines
}
/// At most logarithmically many shared shaping requests, independent of bidi
/// glyph order. Contextual shaping need not make widths monotonic: every kept
/// candidate is measured and fits; the search may conservatively elide extra.
fn fitted_end(
    system: &mut TextSystem,
    layout: &UnicodeLayout,
    size: f32,
    available: f32,
    suffix: &str,
) -> Option<usize> {
    let mut ranges: Vec<_> = layout
        .clusters()
        .iter()
        .map(|cluster| cluster.bytes.clone())
        .collect();
    ranges.sort_by_key(|range| range.start);
    let (mut next, mut occupied_end) = (0, 0);
    let mut cuts = Vec::new();
    for &byte in layout.grapheme_boundaries() {
        while next < ranges.len() && ranges[next].start < byte {
            occupied_end = occupied_end.max(ranges[next].end);
            next += 1;
        }
        if occupied_end <= byte {
            cuts.push(byte);
        }
    }
    let (mut low, mut high, mut best) = (0, cuts.len(), None);
    while low < high {
        let middle = low + (high - low) / 2;
        let end = cuts[middle];
        let candidate = format!("{}{suffix}", &layout.text()[..end]);
        if system
            .layout(&candidate, size, None)
            .is_ok_and(|candidate| candidate.size()[0] <= available)
        {
            best = Some(end);
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    best
}
#[cfg(test)]
fn cluster_boundary(layout: &UnicodeLayout, byte: usize) -> bool {
    !layout
        .clusters()
        .iter()
        .any(|cluster| cluster.bytes.start < byte && byte < cluster.bytes.end)
}

pub fn renderer_cursor(cursor: tilewright_editor_ui::text_input::TextCursor) -> TextCursor {
    TextCursor {
        byte: cursor.byte,
        affinity: match cursor.affinity {
            tilewright_editor_ui::text_input::TextAffinity::Before => TextAffinity::Before,
            tilewright_editor_ui::text_input::TextAffinity::After => TextAffinity::After,
        },
    }
}
pub fn model_cursor(cursor: TextCursor) -> tilewright_editor_ui::text_input::TextCursor {
    tilewright_editor_ui::text_input::TextCursor {
        byte: cursor.byte,
        affinity: match cursor.affinity {
            TextAffinity::Before => tilewright_editor_ui::text_input::TextAffinity::Before,
            TextAffinity::After => tilewright_editor_ui::text_input::TextAffinity::After,
        },
    }
}

#[allow(clippy::cast_precision_loss)]
fn rect(rect: Rect) -> UiRect {
    // SDL dimensions are constrained by SceneLimits before scene construction.
    UiRect {
        x: rect.x as f32,
        y: rect.y as f32,
        width: rect.width as f32,
        height: rect.height as f32,
    }
}

fn paint(next: &mut u32, order: u32, color: [f32; 4]) -> Result<UiPaint> {
    let id = next
        .checked_add(1)
        .ok_or(CompositionError::SemanticIdentityExhausted)?;
    *next = id;
    Ok(UiPaint {
        semantic_id: SemanticId(id),
        painter_order: order,
        color,
    })
}

/// The raw IME splice may join adjacent graphemes (e.g. a combining mark).
/// Underline the complete displayed graphemes touched by that exact splice.
fn composition_rects(layout: &UnicodeLayout, range: std::ops::Range<usize>) -> Vec<UiRect> {
    let boundaries = layout.grapheme_boundaries();
    let start = boundaries
        .iter()
        .copied()
        .take_while(|byte| *byte <= range.start)
        .last()
        .unwrap_or(0);
    let end = boundaries
        .iter()
        .copied()
        .find(|byte| *byte >= range.end)
        .unwrap_or(layout.text().len());
    layout.selection_rects(
        TextCursor {
            byte: start,
            affinity: TextAffinity::After,
        },
        TextCursor {
            byte: end,
            affinity: TextAffinity::Before,
        },
    )
}

/// The background of a control or active row.
fn row_background<'a>(
    widget: &Widget,
    identity: &mut u32,
    order: u32,
) -> Result<Option<UiPrimitive<'a>>> {
    if widget.control.is_none() && !widget.active {
        return Ok(None);
    }
    let color = if widget.active {
        [0.12, 0.32, 0.47, 1.0]
    } else if widget.enabled {
        [0.14, 0.18, 0.24, 1.0]
    } else {
        [0.09, 0.11, 0.14, 1.0]
    };
    let bounds = rect(widget.layout);
    Ok(Some(UiPrimitive::Solid {
        bounds,
        clip: bounds,
        paint: paint(identity, order, color)?,
        pickable: false,
    }))
}

/// A legend row's colour swatch, above the row's background.
fn swatch<'a>(widget: &Widget, identity: &mut u32, order: u32) -> Result<Option<UiPrimitive<'a>>> {
    let (Some(color), Some(square)) = (widget.swatch, swatch_rect(widget.layout)) else {
        return Ok(None);
    };
    Ok(Some(UiPrimitive::Solid {
        bounds: rect(square),
        clip: rect(widget.layout),
        paint: paint(identity, order, color)?,
        pickable: false,
    }))
}

#[allow(clippy::cast_precision_loss)]
fn display_list<'a>(
    ui: &'a WorkspaceUi,
    mut identity: u32,
    system: &mut TextSystem,
) -> Result<(Vec<UiPrimitive<'a>>, Option<EditableGeometry>)> {
    let mut editable = None;
    let mut primitives = Vec::new();
    for bounds in [
        Rect {
            x: 0,
            y: 0,
            width: ui.logical_size().0,
            height: ui.logical_viewport.y,
        },
        ui.logical_sidebar,
        ui.logical_review,
    ] {
        let bounds = rect(bounds);
        primitives.push(UiPrimitive::Solid {
            bounds,
            clip: bounds,
            paint: paint(&mut identity, 0, [0.065, 0.08, 0.11, 1.0])?,
            pickable: false,
        });
    }
    for (index, widget) in ui.widgets.iter().enumerate() {
        let bounds = rect(widget.layout);
        let order = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(2))
            .and_then(|index| index.checked_add(1))
            .ok_or(CompositionError::PainterOrderExhausted)?;
        primitives.extend(row_background(widget, &mut identity, order)?);
        let color = if widget.enabled {
            [0.91, 0.94, 0.98, 1.0]
        } else {
            [0.50, 0.56, 0.63, 1.0]
        };
        primitives.extend(swatch(widget, &mut identity, order)?);
        let mut origin = [bounds.x + 4.0, bounds.y + 1.0];
        if widget.thumbnail.is_some() || widget.swatch.is_some() {
            // Text sits beside the thumbnail, centred on a 26 px text row.
            origin[0] += thumbnail_indent(widget.layout.height) as f32;
            origin[1] += (widget.layout.height.saturating_sub(26) / 2) as f32;
        }
        if let Some(snapshot) = &widget.editable {
            let layout = system
                .layout(&widget.text, text_size(widget.layout.height), None)
                .map_err(CompositionError::Text)?;
            let cursor = renderer_cursor(snapshot.display_selection.focus);
            if let Some(caret) = layout.caret(cursor) {
                origin[0] -= (caret.x - (bounds.width - 12.0).max(0.0)).max(0.0);
            }
            let mut selection =
                layout.selection_rects(renderer_cursor(snapshot.display_selection.anchor), cursor);
            if let Some(composition) = &snapshot.composition {
                for mut underline in composition_rects(&layout, composition.display_range.clone()) {
                    underline.y += underline.height - 2.0;
                    underline.height = 1.0;
                    selection.push(underline);
                }
            }
            if let Some(caret) = layout.caret(cursor) {
                selection.push(caret);
            }
            for mut selected in selection {
                selected.x += origin[0];
                selected.y += origin[1];
                primitives.push(UiPrimitive::Solid {
                    bounds: selected,
                    clip: bounds,
                    paint: paint(&mut identity, order, [0.28, 0.48, 0.66, 1.0])?,
                    pickable: false,
                });
            }
            editable = Some(EditableGeometry {
                revision: snapshot.revision,
                layout,
                origin,
                clip: bounds,
                snapshot: snapshot.clone(),
            });
        }
        primitives.push(UiPrimitive::Text {
            origin,
            clip: bounds,
            paint: paint(&mut identity, order + 1, color)?,
            font: builtin::FONT_ID,
            size: text_size(widget.layout.height),
            text: &widget.text,
        });
    }
    Ok((primitives, editable))
}

/// [`compose_with_overlay`] with no overlay and the renderer's scene limits.
#[cfg(test)]
pub fn compose(
    system: &mut TextSystem,
    ui: &WorkspaceUi,
    world: &SceneBundle,
    resources: Arc<dyn ResourceProvider>,
) -> Result<Presentation> {
    compose_with_overlay(system, ui, world, resources, &[], SceneLimits::default())
}

/// [`compose`] with `overlay` quads (see [`stroke_overlay`]), within
/// `limits`. The overlay keeps its geometry, resource, layer, depth and
/// painter order; it gets fresh semantic identities above the World's, so
/// it sorts after World quads of the same order, and must sample textures
/// `resources` serves. The World quads are copied unchanged.
pub fn compose_with_overlay(
    system: &mut TextSystem,
    ui: &WorkspaceUi,
    world: &SceneBundle,
    resources: Arc<dyn ResourceProvider>,
    overlay: &[Quad],
    limits: SceneLimits,
) -> Result<Presentation> {
    if (world.viewport().width, world.viewport().height) != ui.size {
        return Err(CompositionError::TargetMismatch);
    }
    let mut identity = world
        .quads()
        .iter()
        .map(|quad| quad.semantic_id.0)
        .max()
        .unwrap_or(0);
    let overlay = overlay
        .iter()
        .map(|quad| {
            identity = identity
                .checked_add(1)
                .ok_or(CompositionError::SemanticIdentityExhausted)?;
            Ok(Quad {
                semantic_id: SemanticId(identity),
                part: 0,
                pickable: false,
                ..*quad
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let thumbnails = thumbnail_quads(ui, identity)?;
    // Thumbnails reuse ready World textures that need not be in the World
    // scene, so UI resource identities start above both.
    let resource = world
        .quads()
        .iter()
        .chain(&thumbnails)
        .map(|quad| quad.resource_id.0)
        .max()
        .unwrap_or(0);
    let solid = UiTexture {
        id: ResourceId(
            resource
                .checked_add(1)
                .ok_or(CompositionError::ResourceIdentityExhausted)?,
        ),
        revision: 1,
        digest: Digest256(builtin::SOLID_SHA256),
    };
    let first_resource = resource
        .checked_add(2)
        .ok_or(CompositionError::ResourceIdentityExhausted)?;
    let identity = thumbnails
        .last()
        .map_or(identity, |quad| quad.semantic_id.0);
    let (primitives, editable) = display_list(ui, identity, system)?;
    let logical = ui.logical_size();
    let compiled = system
        .compile(
            UiViewport {
                logical_size: [logical.0, logical.1],
                physical_size: [ui.size.0, ui.size.1],
            },
            UiDisplayList {
                primitives: &primitives,
            },
            solid,
            first_resource,
            UiLimits::default(),
        )
        .map_err(CompositionError::Text)?;
    // The overlay samples World-provided textures (the cell reference art
    // has the largest identity), so interface textures must not reuse one.
    if overlay.iter().any(|quad| {
        quad.resource_id == solid.id
            || compiled
                .resources
                .iter()
                .any(|texture| texture.identity.id == quad.resource_id)
    }) {
        return Err(CompositionError::ResourceIdentityExhausted);
    }
    let scene = SceneBundle::from_quads(
        world.revision(),
        world.clock_millis(),
        world.viewport(),
        world.clear_color(),
        world
            .quads()
            .iter()
            .copied()
            .chain(overlay)
            .chain(thumbnails)
            .chain(compiled.quads),
        limits,
    )
    .map_err(CompositionError::Scene)?;
    Ok(Presentation {
        scene,
        resources: Arc::new(Resources {
            world: resources,
            textures: compiled.resources,
            solid,
        }),
        unsupported_runs: compiled.unsupported.len(),
        editable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Missing;
    impl ResourceProvider for Missing {
        fn load(&self, _: ResourceRequest) -> std::result::Result<ResourceBlob, ResourceError> {
            Err(ResourceError::NotFound)
        }
    }
    #[test]
    fn immutable_unicode_resources_validate_complete_identity_and_budget() {
        use sha2::{Digest, Sha256};
        let digest: [u8; 32] = Sha256::digest([1_u8, 2, 3]).into();
        let identity = UiTexture {
            id: ResourceId(100),
            revision: 4,
            digest: Digest256(digest),
        };
        let resources = Resources {
            world: Arc::new(Missing),
            textures: vec![UnicodeTexture {
                identity,
                bytes: Arc::from([1_u8, 2, 3]),
            }],
            solid: UiTexture {
                id: ResourceId(101),
                revision: 1,
                digest: Digest256(builtin::SOLID_SHA256),
            },
        };
        let request = ResourceRequest {
            id: identity.id,
            revision: identity.revision,
            digest: identity.digest,
            maximum_bytes: 100,
        };
        assert_eq!(resources.load(request).unwrap().bytes(), &[1, 2, 3]);
        assert_eq!(
            resources.load(ResourceRequest {
                revision: 3,
                ..request
            }),
            Err(ResourceError::DigestMismatch)
        );
        assert_eq!(
            resources.load(ResourceRequest {
                maximum_bytes: 1,
                ..request
            }),
            Err(ResourceError::LimitExceeded)
        );
        assert_eq!(
            resources.load(ResourceRequest {
                id: ResourceId(102),
                ..request
            }),
            Err(ResourceError::NotFound)
        );
    }
    fn texture(id: u64, width: u32, height: u32) -> tilewright_render_world::TextureRef {
        tilewright_render_world::TextureRef {
            id: ResourceId(id),
            revision: 1,
            digest: Digest256([7; 32]),
            width,
            height,
        }
    }
    fn row(y: u32) -> Rect {
        Rect {
            x: 970,
            y,
            width: 300,
            height: 26,
        }
    }
    #[test]
    fn thumbnails_fit_their_row_box_keep_aspect_and_snap_to_whole_pixels() {
        let ui = WorkspaceUi::new((1280, 800), (1280, 800));
        let clip = ui.logical_sidebar;
        // The box is 22x22 at (972, 102). Tall art fills its height.
        assert_eq!(
            thumbnail_rect(&ui, row(100), clip, (24, 40)),
            Some(Rect {
                x: 976,
                y: 102,
                width: 13,
                height: 22
            })
        );
        // Wide art fills its width, rounded to nearest, and is centred.
        assert_eq!(
            thumbnail_rect(&ui, row(100), clip, (28, 24)),
            Some(Rect {
                x: 972,
                y: 103,
                width: 22,
                height: 19
            })
        );
        // Square and tiny art fill the box; a 1x64 sliver keeps one pixel.
        assert_eq!(
            thumbnail_rect(&ui, row(100), clip, (4, 4)).map(|r| (r.width, r.height)),
            Some((22, 22))
        );
        assert_eq!(
            thumbnail_rect(&ui, row(100), clip, (1, 64)).map(|r| (r.width, r.height)),
            Some((1, 22))
        );
        assert_eq!(thumbnail_rect(&ui, row(100), clip, (0, 4)), None);
        // At twice the pixel density the box is 44 physical pixels.
        let dense = WorkspaceUi::new((640, 400), (1280, 800));
        let dense_row = Rect {
            x: 330,
            y: 60,
            width: 300,
            height: 26,
        };
        assert_eq!(
            thumbnail_rect(&dense, dense_row, dense.logical_sidebar, (24, 40)),
            Some(Rect {
                x: 673,
                y: 124,
                width: 26,
                height: 44
            })
        );
    }
    #[test]
    fn rows_not_wholly_inside_the_sidebar_get_no_thumbnail() {
        let ui = WorkspaceUi::new((1280, 800), (1280, 800));
        let clip = ui.logical_sidebar;
        // The sidebar starts below the 48 px toolbar and ends at 800.
        assert!(thumbnail_rect(&ui, row(48), clip, (24, 24)).is_some());
        assert!(thumbnail_rect(&ui, row(47), clip, (24, 24)).is_none());
        assert!(thumbnail_rect(&ui, row(774), clip, (24, 24)).is_some());
        assert!(thumbnail_rect(&ui, row(775), clip, (24, 24)).is_none());
        // Left of the sidebar is the World view or the review panel.
        let left = Rect { x: 950, ..row(100) };
        assert!(thumbnail_rect(&ui, left, clip, (24, 24)).is_none());
        let narrow = Rect {
            width: 20,
            ..row(100)
        };
        assert!(thumbnail_rect(&ui, narrow, clip, (24, 24)).is_none());
    }
    #[test]
    #[allow(clippy::float_cmp)]
    fn thumbnail_quads_are_bounded_unpickable_and_above_their_row() {
        let mut ui = WorkspaceUi::new((1280, 4000), (1280, 4000));
        for index in 0..100 {
            ui.widget(row(56 + index * 28), "row", None, true, false);
            ui.widgets.last_mut().unwrap().thumbnail = Some(texture(3, 24, 24));
        }
        ui.widgets[1].enabled = false;
        let quads = thumbnail_quads(&ui, 10).unwrap();
        assert_eq!(quads.len(), MAX_THUMBNAILS);
        // Disabled rows draw their thumbnail grey and half transparent.
        assert_eq!(
            (quads[0].color, quads[0].grayscale),
            ([1.0, 1.0, 1.0, 1.0], 0.0)
        );
        assert_eq!(
            (quads[1].color, quads[1].grayscale),
            ([1.0, 1.0, 1.0, 0.5], 1.0)
        );
        for (index, quad) in quads.iter().enumerate() {
            assert!(!quad.pickable);
            assert_eq!(quad.layer, Layer::Interface);
            assert_eq!(quad.semantic_id.0, 11 + u32::try_from(index).unwrap());
            // Above the row background (2i+1), level with its text (2i+2).
            assert_eq!(quad.painter_order, u32::try_from(index * 2 + 2).unwrap());
            assert_eq!(quad.frame, None);
            for corner in quad.positions {
                assert_eq!(corner, corner.map(f32::round));
            }
        }
    }
    #[test]
    fn composed_ui_resources_start_above_thumbnail_textures() {
        let mut system = crate::font_pack::test_system();
        let mut ui = WorkspaceUi::new((320, 240), (320, 240));
        let at = Rect {
            x: 170,
            y: 90,
            width: 140,
            height: 26,
        };
        ui.widget(at, "lamp / item", None, true, false);
        ui.widgets.last_mut().unwrap().thumbnail = Some(texture(50, 16, 40));
        let world = tilewright_editor_preview::empty_scene(320, 240, 1).unwrap();
        let presentation = compose(&mut system, &ui, &world, Arc::new(Missing)).unwrap();
        let quads = presentation.scene.quads();
        let thumbnails: Vec<_> = quads
            .iter()
            .filter(|quad| quad.resource_id == ResourceId(50))
            .collect();
        assert_eq!(thumbnails.len(), 1);
        assert!(
            quads
                .iter()
                .filter(|quad| quad.resource_id != ResourceId(50))
                .all(|quad| quad.resource_id.0 > 50)
        );
        // Thumbnail and UI semantic identities are distinct.
        let identities: std::collections::BTreeSet<_> =
            quads.iter().map(|quad| quad.semantic_id).collect();
        assert!(identities.contains(&thumbnails[0].semantic_id));
        assert_eq!(
            quads
                .iter()
                .filter(|quad| quad.semantic_id == thumbnails[0].semantic_id)
                .count(),
            1
        );
    }
    #[test]
    fn combining_preedit_underline_uses_display_grapheme_bounds() {
        let mut system = crate::font_pack::test_system();
        let layout = system.layout("e\u{301}", 18.0, None).unwrap();
        assert_eq!(layout.grapheme_boundaries(), &[0, 3]);
        assert!(!composition_rects(&layout, 1..3).is_empty());
    }
    #[test]
    fn elision_preserves_original_grapheme_and_shaping_cluster_boundaries() {
        let mut system = crate::font_pack::test_system();
        let text = "office e\u{301} العربية";
        let layout = system.layout(text, 18.0, None).unwrap();
        for width in 20..80 {
            let fitted = fit_text(&mut system, text, width, 26);
            if let Some(prefix) = fitted.strip_suffix("...") {
                assert!(layout.grapheme_boundaries().contains(&prefix.len()));
                assert!(cluster_boundary(&layout, prefix.len()));
            }
        }
    }
    #[test]
    fn long_label_elision_uses_bounded_shared_layout_requests() {
        let mut system = crate::font_pack::test_system();
        let text = "a".repeat(4096);
        let fitted = fit_text(&mut system, &text, 290, 26);
        assert!(fitted.ends_with("..."));
        assert!(
            system.cached_layouts() <= 15,
            "each retained trial is one shared shaping request"
        );
        assert!(system.layout(&fitted, 18.0, None).unwrap().size()[0] <= 282.0);
    }
}
