// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use std::sync::Arc;
use tilewright_render_api::{Error as RenderError, FrameOutput, RenderRequest, Renderer};
use tilewright_render_resources::ResourceProvider;
use tilewright_scene::{SceneBundle, SceneLimits, Viewport};

pub const RENDERER_COMPATIBILITY: &str = "scene-bundle-v3/0.7.0";

pub fn empty_scene(
    width: u32,
    height: u32,
    revision: u64,
) -> Result<SceneBundle, tilewright_scene::Error> {
    SceneBundle::new(
        revision,
        0,
        Viewport {
            width,
            height,
            scale_milli: 1_000,
        },
        [0.04, 0.05, 0.07, 1.0],
        [],
        SceneLimits::default(),
    )
}

pub fn render_scene<R: Renderer>(
    renderer: &mut R,
    scene: &SceneBundle,
    resources: Arc<dyn ResourceProvider>,
) -> Result<FrameOutput, RenderError> {
    renderer.render(RenderRequest { scene, resources })
}

mod catalog;
pub mod loader;
mod prepare;
pub use catalog::{EditingCapabilities, ObjectGrant, PreviewCatalog, ZoneGrant};
pub use loader::ReadyResources;
pub use prepare::prepare;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};
use tilewright_render_world::Camera;
use tilewright_scene::SemanticId;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SelectionKey {
    Object {
        uid: String,
    },
    Cell {
        plane: String,
        layer: i32,
        grid: String,
        x: i64,
        y: i64,
    },
}

#[derive(Clone, Debug)]
pub struct ViewSpec {
    pub plane: String,
    pub layer: i32,
    pub bounds: [i64; 4],
    pub camera: Camera,
    pub frame_revision: u64,
    /// Terrain mask target. Required when terrain picking is desired.
    pub grid: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewLimits {
    pub maximum_catalog_bytes: usize,
    pub maximum_catalog_values: usize,
    pub maximum_entries: usize,
    pub maximum_resource_bytes: usize,
    pub maximum_total_resource_bytes: usize,
    pub maximum_cells: usize,
    pub maximum_objects: usize,
    pub maximum_chunks: usize,
    pub scene: SceneLimits,
}
impl Default for PreviewLimits {
    fn default() -> Self {
        Self {
            maximum_catalog_bytes: 1024 * 1024,
            maximum_catalog_values: 65536,
            maximum_entries: 1024,
            maximum_resource_bytes: tilewright_render_resources::DEFAULT_MAXIMUM_RESOURCE_BYTES,
            maximum_total_resource_bytes: 64 * 1024 * 1024,
            maximum_cells: 4096,
            maximum_objects: 16384,
            maximum_chunks: 8192,
            scene: SceneLimits::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewDiagnostic {
    pub code: &'static str,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewError(pub String);
impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for PreviewError {}
pub(crate) fn failure(s: impl Into<String>) -> PreviewError {
    PreviewError(s.into())
}
pub(crate) fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub struct PreparedPreview {
    pub scene: SceneBundle,
    pub cell_scene: SceneBundle,
    /// See [`PreparedPreview::cell_slots`]: filled on first use.
    cell_slots: std::sync::OnceLock<Option<CellSlots>>,
    slot_inputs: prepare::SlotInputs,
    pub resources: Arc<dyn ResourceProvider>,
    pub picks: BTreeMap<SemanticId, SelectionKey>,
    pub cell_picks: BTreeMap<SemanticId, SelectionKey>,
    pub source_revision: [u8; 32],
    pub catalog_revision: [u8; 32],
    pub frame_revision: u64,
    pub diagnostics: Vec<PreviewDiagnostic>,
}
/// For each cell-scene identity, where that cell falls in the World scene's
/// draw order: the `(depth, painter order)` of the last World quad drawn
/// before it, or `None` before the whole World.
pub type CellSlots = BTreeMap<SemanticId, Option<(i16, u32)>>;

impl PreparedPreview {
    /// Where each cell falls in the World scene's draw order (see
    /// [`CellSlots`]), from the shared renderer's own ordering of the World
    /// and the cell reference objects compiled together. Computed on first
    /// use and kept with this prepared scene, so a session that never shows
    /// an overlay never pays for it. `None` when that combined scene cannot
    /// be compiled within twice the scene limits.
    pub fn cell_slots(&self) -> Option<&CellSlots> {
        self.cell_slots
            .get_or_init(|| prepare::cell_slots(&self.scene, &self.slot_inputs))
            .as_ref()
    }
    /// Make this prepared scene's cell slots unavailable, exactly as a
    /// failed ordering compile does, unless they were already computed.
    /// Returns whether they are now unavailable. For tests of the fallback.
    #[doc(hidden)]
    pub fn withhold_cell_slots(&self) -> bool {
        self.cell_slots.get_or_init(|| None).is_none()
    }
    /// Exact reference masks on demand; the caller revalidates rooted catalog/resources
    /// before accepting input. Coordinates are physical pixels in this compiled viewport.
    pub fn pick(
        &self,
        generation: u64,
        x: u32,
        y: u32,
        terrain: bool,
    ) -> Result<Option<SelectionKey>, PreviewError> {
        self.check_generation(generation)?;
        let viewport = self.scene_for(terrain).viewport();
        if x >= viewport.width || y >= viewport.height {
            return Err(failure("pick outside viewport"));
        }
        self.mask(generation, terrain)?.pick(x, y)
    }

    /// Render one exact reference mask for this frame, so a gesture can map
    /// many pointer samples without a render per sample. The mask belongs to
    /// `generation` only; callers discard it when the frame or viewport changes.
    pub fn mask(&self, generation: u64, terrain: bool) -> Result<PickMask, PreviewError> {
        self.check_generation(generation)?;
        let scene = self.scene_for(terrain);
        let viewport = scene.viewport();
        let mut renderer = tilewright_render_testkit::ReferenceRenderer::new(
            tilewright_render_api::TargetDescriptor {
                kind: tilewright_render_api::TargetKind::Offscreen,
                width: viewport.width,
                height: viewport.height,
                backend: tilewright_render_api::BackendPreference::Automatic,
            },
        )
        .map_err(|e| failure(e.to_string()))?;
        let frame = render_scene(&mut renderer, scene, self.resources.clone())
            .map_err(|e| failure(e.to_string()))?;
        let expected = u64::from(frame.width) * u64::from(frame.height);
        if usize::try_from(expected).ok() != Some(frame.semantic_ids.len()) {
            return Err(failure("invalid renderer mask"));
        }
        Ok(PickMask {
            generation,
            width: frame.width,
            height: frame.height,
            ids: frame.semantic_ids,
            picks: if terrain {
                self.cell_picks.clone()
            } else {
                self.picks.clone()
            },
        })
    }

    fn check_generation(&self, generation: u64) -> Result<(), PreviewError> {
        if generation == 0 || generation != self.frame_revision {
            return Err(failure("stale preview frame"));
        }
        Ok(())
    }

    fn scene_for(&self, terrain: bool) -> &SceneBundle {
        if terrain {
            &self.cell_scene
        } else {
            &self.scene
        }
    }
}

/// An exact semantic mask for one prepared frame and its pick identities.
#[derive(Clone, Debug)]
pub struct PickMask {
    generation: u64,
    width: u32,
    height: u32,
    ids: Vec<u32>,
    picks: BTreeMap<SemanticId, SelectionKey>,
}
impl PickMask {
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    /// Physical `[width, height]` of the frame this mask was rendered for.
    pub const fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }
    /// The semantic selection under physical pixel `x, y`, if any.
    pub fn pick(&self, x: u32, y: u32) -> Result<Option<SelectionKey>, PreviewError> {
        if x >= self.width || y >= self.height {
            return Err(failure("pick outside viewport"));
        }
        let index = usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x))
            .map_err(|_| failure("pixel index overflow"))?;
        let id = SemanticId(
            *self
                .ids
                .get(index)
                .ok_or_else(|| failure("invalid renderer mask"))?,
        );
        if id.0 == 0 {
            return Ok(None);
        }
        self.picks
            .get(&id)
            .cloned()
            .map(Some)
            .ok_or_else(|| failure("renderer returned unknown semantic ID"))
    }
}

pub(crate) fn validate_bounds(bounds: [i64; 4], limits: PreviewLimits) -> Result<(), PreviewError> {
    if bounds[2] <= 0
        || bounds[3] <= 0
        || bounds[0].checked_add(bounds[2]).is_none()
        || bounds[1].checked_add(bounds[3]).is_none()
        || bounds[2]
            .checked_mul(bounds[3])
            .and_then(|v| usize::try_from(v).ok())
            .is_none_or(|v| v > limits.maximum_cells)
    {
        return Err(failure("invalid or over-budget view bounds"));
    }
    Ok(())
}
