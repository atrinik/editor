// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The editor window's state, apart from its window and GPU: the session,
//! the interface, the camera and the scene of the plane in view. The window
//! renders the scene and hands back the frame, whose semantic mask finds the
//! tile under the pointer.

use eframe::egui;
use std::{collections::BTreeSet, path::Path, time::Instant};
use tilewright_editor_commands::{
    brush::{Brush, Shape},
    session::{Error as SessionError, Options, Saved, Session},
};
use tilewright_editor_document::terrain::{ChunkKey, PlaneTerrain};
use tilewright_editor_preview::ground::{
    GroundCamera, GroundScene, GroundTile, PickMap, ground_scene, swatch_color, view_bounds,
};
use tilewright_editor_ui::editor::{Action, BrushShape, PaletteEntry, Status};
use tilewright_render_api::FrameOutput;
use tilewright_render_world::Projection;
use tilewright_source::{CHUNK_SIZE, Projection as WorldProjection, TerrainSource};

/// Zoom limits of the view.
const ZOOM: std::ops::RangeInclusive<f32> = 0.25..=8.0;

/// Tiles along a chunk's side.
const SIDE: i64 = CHUNK_SIZE as i64;

pub struct Workbench {
    session: Session,
    palette: Vec<PaletteEntry>,
    /// The view's size in points, as the interface last laid it out.
    view_size: egui::Vec2,
    plane: String,
    layer: i32,
    /// The render rank of each ground layer.
    ranks: Vec<u16>,
    camera: GroundCamera,
    /// What saving here would not check, while it waits for the user to
    /// accept that.
    consent: Option<String>,
    /// The scene's pick map and the frame rendered from it.
    picks: PickMap,
    frame: Option<FrameOutput>,
    /// Whether the view must be drawn again.
    stale: bool,
    revision: u64,
    message: Option<String>,
}

impl Workbench {
    /// Opens the world at `root` on `plane`, or its first plane.
    ///
    /// # Errors
    /// Fails when the world does not load or has no such plane.
    pub fn open(root: &Path, plane: Option<&str>, accepted: bool) -> Result<Self, String> {
        let session = Session::open(
            root,
            Options {
                unchecked_access: accepted,
                ..Options::default()
            },
        )
        .map_err(|error| error.to_string())?;
        let world = session.document().world().clone();
        let plane = match plane {
            Some(plane) if world.planes().contains_key(plane) => plane.to_owned(),
            Some(plane) => return Err(format!("the world has no plane {plane:?}")),
            None => world
                .planes()
                .keys()
                .next()
                .cloned()
                .ok_or("the world has no plane")?,
        };
        let profile = world.profile();
        let ranks = profile
            .ground_layers
            .iter()
            .map(|layer| {
                profile
                    .render_layer_index(layer)
                    .and_then(|rank| u16::try_from(rank).ok())
                    .ok_or_else(|| format!("ground layer {layer:?} has no render layer"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let palette = profile
            .transitions
            .as_deref()
            .map_or_else(Vec::new, |transitions| {
                transitions
                    .terrains()
                    .iter()
                    .map(|terrain| PaletteEntry {
                        terrain: terrain.id.clone(),
                        color: terrain
                            .protos
                            .first()
                            .map_or([0, 0, 0], |proto| swatch_color(proto)),
                    })
                    .collect()
            });
        let projection = match profile.projection {
            WorldProjection::Isometric {
                tile_width,
                tile_height,
            } => Projection::Isometric {
                tile_width,
                tile_height,
            },
            WorldProjection::TopDown { tile_size } => Projection::TopDown { tile_size },
        };
        // Start over the middle of the plane's chunks on the surface.
        let chunks: Vec<ChunkKey> = world.planes()[&plane]
            .chunks
            .keys()
            .copied()
            .filter(|(layer, _, _)| *layer == 0)
            .collect();
        let center = chunks.first().map_or([0.0, 0.0], |_| {
            #[allow(clippy::cast_precision_loss)]
            let mean = |pick: fn(&ChunkKey) -> i64| {
                chunks.iter().map(|key| pick(key) as f64).sum::<f64>() / chunks.len() as f64
            };
            let side = f64::from(CHUNK_SIZE);
            [
                mean(|key| key.1).mul_add(side, side / 2.0),
                mean(|key| key.2).mul_add(side, side / 2.0),
            ]
        });
        Ok(Self {
            session,
            palette,
            view_size: egui::Vec2::ZERO,
            plane,
            layer: 0,
            ranks,
            camera: GroundCamera {
                projection,
                center,
                zoom: 1.0,
                width: 1,
                height: 1,
            },
            consent: None,
            picks: PickMap::default(),
            frame: None,
            stale: true,
            revision: 0,
            message: None,
        })
    }

    /// The terrains of the profile's transitions, for the palette.
    #[must_use]
    pub fn palette(&self) -> Vec<PaletteEntry> {
        self.palette.clone()
    }

    /// The view's size in points, as the interface laid it out.
    pub const fn set_view_size(&mut self, size: egui::Vec2) {
        self.view_size = size;
    }

    /// Carries out what the user asked for at `now`.
    pub fn apply(&mut self, actions: Vec<Action>, now: Instant) {
        for action in actions {
            if let Err(error) = self.carry_out(action, now) {
                self.message = Some(error);
            }
        }
    }

    fn carry_out(&mut self, action: Action, now: Instant) -> Result<(), String> {
        match action {
            Action::StrokeBegin {
                tile,
                terrain,
                shape,
                radius,
                layer,
            } => {
                if layer != self.layer {
                    self.layer = layer;
                    self.stale = true;
                }
                let brush = Brush {
                    shape: match shape {
                        BrushShape::Square => Shape::Square,
                        BrushShape::Circle => Shape::Circle,
                    },
                    radius,
                };
                let result = self
                    .session
                    .begin_stroke(now, &self.plane, layer, &terrain, brush);
                self.checked(result)?;
                self.message = None;
                self.stamp(tile)?;
            }
            Action::StrokeMove(tile) => self.stamp(tile)?,
            Action::StrokeEnd => {
                self.stale = true;
                let result = self.session.end_stroke(now);
                if let Some(saved) = self.checked(result)? {
                    self.saved(&saved);
                }
            }
            Action::StrokeCancel => {
                self.stale = true;
                let result = self.session.cancel_stroke();
                self.checked(result)?;
            }
            Action::Undo => {
                self.stale = true;
                let result = self.session.undo();
                if let Some(undone) = self.checked(result)? {
                    self.consent = None;
                    self.message = Some(format!("Undid {}", undone.title));
                }
            }
            Action::Redo => {
                self.stale = true;
                let result = self.session.redo();
                if let Some(redone) = self.checked(result)? {
                    self.message = Some(format!("Redid {}", redone.title));
                }
            }
            Action::SaveNow => {
                self.stale = true;
                let result = self.session.flush(now);
                if let Some(saved) = self.checked(result)? {
                    self.saved(&saved);
                }
            }
            Action::Discard => {
                let result = self.session.discard();
                self.checked(result)?;
                self.consent = None;
                self.message = Some("Dropped the unsaved edits".to_owned());
                self.stale = true;
            }
            Action::Pan(delta) => {
                self.camera.pan([
                    delta.x * self.pixels_per_point(),
                    delta.y * self.pixels_per_point(),
                ]);
                self.stale = true;
            }
            Action::Zoom(factor) => {
                self.camera.zoom = (self.camera.zoom * factor).clamp(*ZOOM.start(), *ZOOM.end());
                self.stale = true;
            }
            Action::AcceptUncheckedAccess => {
                self.consent = None;
                self.session.set_unchecked_access(true);
                let result = self.session.flush(now);
                if let Some(saved) = self.checked(result)? {
                    self.saved(&saved);
                }
            }
        }
        Ok(())
    }

    /// A session result in words; a save that waits for consent shows the
    /// notice instead, and a rejected one says what it dropped.
    fn checked<T>(&mut self, result: Result<T, SessionError>) -> Result<T, String> {
        result.map_err(|error| match error {
            SessionError::NeedsConsent(unchecked) => {
                self.consent = Some(unchecked);
                "Saving waits for your consent".to_owned()
            }
            SessionError::Rejected(reason) => {
                self.stale = true;
                match self.session.rejected() {
                    Some(rejected) => format!(
                        "Could not save {}, so it was dropped and the world read again: {reason}",
                        rejected.title
                    ),
                    None => reason.to_string(),
                }
            }
            SessionError::Stamp(partial) => {
                if !partial.dirty.is_empty() {
                    self.stale = true;
                }
                partial.to_string()
            }
            other => other.to_string(),
        })
    }

    /// Notes a save, and a problem the toolkit reported after it.
    fn saved(&mut self, saved: &Saved) {
        self.message = Some(match &saved.warning {
            Some(warning) => format!("Saved {}, but: {warning}", saved.title),
            None => format!("Saved {}", saved.title),
        });
    }

    fn stamp(&mut self, tile: [i64; 2]) -> Result<(), String> {
        let result = self.session.stamp(tile);
        let dirty = self.checked(result)?;
        if !dirty.is_empty() {
            self.stale = true;
        }
        Ok(())
    }

    /// What the interface shows, with the rendered view if there is one.
    pub fn status(
        &self,
        view: Option<(egui::TextureId, egui::Vec2)>,
        view_error: Option<String>,
    ) -> Status {
        let (undo, redo) = self.session.history();
        let summary = self.session.document().summary();
        Status {
            title: format!(
                "{} · plane {} · layer {}",
                summary.profile, self.plane, self.layer
            ),
            dirty: self.session.is_dirty(),
            waiting: self.session.is_waiting(),
            history: (undo, redo),
            message: self.message.clone(),
            unchecked_access: self.consent.clone(),
            view,
            view_error,
        }
    }

    fn pixels_per_point(&self) -> f32 {
        let size = self.view_size;
        if size.x > 0.0 {
            #[allow(clippy::cast_precision_loss)]
            let width = self.camera.width as f32;
            width / size.x
        } else {
            1.0
        }
    }

    /// Whether the view must be drawn again at `width` × `height` pixels.
    pub fn is_stale(&self, width: u32, height: u32) -> bool {
        self.stale || self.camera.width != width || self.camera.height != height
    }

    /// The scene of the view at `width` × `height` pixels: the resolved
    /// ground of every chunk in view, with the unsaved edits.
    ///
    /// # Errors
    /// Fails when the view holds too many tiles or the renderer rejects it.
    pub fn scene(&mut self, width: u32, height: u32) -> Result<GroundScene, String> {
        self.camera.width = width.max(1);
        self.camera.height = height.max(1);
        self.stale = false;
        self.revision = self.revision.wrapping_add(1).max(1);
        let (camera, layer, ranks, revision) =
            (self.camera, self.layer, self.ranks.clone(), self.revision);
        let terrain = self
            .session
            .terrain(&self.plane)
            .map_err(|error| error.to_string())?;
        let (low, high) = view_bounds(&camera);
        let chunks: BTreeSet<ChunkKey> = terrain
            .chunks(layer)
            .into_iter()
            .filter(|(_, cx, cy)| {
                (low[0].div_euclid(SIDE)..=high[0].div_euclid(SIDE)).contains(cx)
                    && (low[1].div_euclid(SIDE)..=high[1].div_euclid(SIDE)).contains(cy)
            })
            .collect();
        let built = {
            let resolved: Vec<(ChunkKey, Vec<Vec<Option<&str>>>)> = chunks
                .iter()
                .map(|key| (*key, ground_of(terrain, *key, ranks.len())))
                .collect();
            let mut tiles = Vec::new();
            for ((_, cx, cy), layers) in &resolved {
                for (ground, cells) in layers.iter().enumerate() {
                    for (index, proto) in cells.iter().enumerate() {
                        let Some(proto) = proto else { continue };
                        let index = i64::try_from(index).unwrap_or(0);
                        tiles.push(GroundTile {
                            at: [cx * SIDE + index % SIDE, cy * SIDE + index / SIDE],
                            ground,
                            proto,
                        });
                    }
                }
            }
            ground_scene(&camera, &ranks, &tiles, revision)
        };
        let built = built.map_err(|error| error.to_string())?;
        self.picks = built.picks.clone();
        self.frame = None;
        Ok(built)
    }

    /// Keeps the frame rendered from the last scene, for picking.
    pub fn rendered(&mut self, frame: FrameOutput) {
        self.frame = Some(frame);
    }

    /// The tile under a point of the view, in points from its top-left.
    pub fn pick(&self, at: egui::Vec2) -> Option<[i64; 2]> {
        let frame = self.frame.as_ref()?;
        let scale = self.pixels_per_point();
        let (x, y) = (at.x * scale, at.y * scale);
        if x < 0.0 || y < 0.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x, y) = (x as u32, y as u32);
        self.picks.tile_at(frame, x, y)
    }
}

/// Each ground layer's tiles of chunk `key`, row-major: resolved with the
/// profile's transitions, or as painted without them.
fn ground_of(terrain: &PlaneTerrain, key: ChunkKey, layers: usize) -> Vec<Vec<Option<&str>>> {
    let cells = usize::from(CHUNK_SIZE) * usize::from(CHUNK_SIZE);
    if let Some(resolved) = terrain.resolve(key) {
        return (0..layers)
            .map(|ground| {
                (0..cells)
                    .map(|index| resolved.value(ground, index))
                    .collect()
            })
            .collect();
    }
    let (layer, cx, cy) = key;
    let grids = terrain.chunk_ground(layer, cx, cy).unwrap_or(&[]);
    (0..layers)
        .map(|ground| {
            (0..cells)
                .map(|index| grids.get(ground).and_then(|grid| grid.value(index)))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Workbench;
    use eframe::egui;
    use std::time::Instant;
    use tilewright_editor_commands::{json_string, transaction};
    use tilewright_editor_preview::ground::swatch_color;
    use tilewright_editor_testkit::{MEADOW_FILES, fixture_world};
    use tilewright_editor_ui::editor::{Action, BrushShape};
    use tilewright_render_api::{
        BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
    };
    use tilewright_render_testkit::ReferenceRenderer;

    const SIZE: u32 = 320;

    fn begin(tile: [i64; 2], terrain: &str) -> Action {
        Action::StrokeBegin {
            tile,
            terrain: terrain.to_owned(),
            shape: BrushShape::Square,
            radius: 0,
            layer: 0,
        }
    }

    /// A copy of the meadow world to save into, made by publishing a
    /// transaction that creates its files; it accepts what the toolkit
    /// cannot check on some platforms.
    fn meadow_copy() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let operations: Vec<String> = MEADOW_FILES
            .iter()
            .map(|(path, text)| {
                format!(
                    "{{\"op\": \"document.create\", \"path\": {}, \"text\": {}}}",
                    json_string(path),
                    json_string(text)
                )
            })
            .collect();
        let text = transaction("Copy the meadow", &operations, &[]);
        let created = tilewright_edit::Transaction::parse("copy.jsonc", text).unwrap();
        tilewright_edit::plan(
            directory.path(),
            &created,
            &tilewright_edit::Options::default(),
        )
        .unwrap()
        .publish_with(&tilewright_edit::PublishOptions {
            unchecked_access: true,
        })
        .unwrap();
        directory
    }

    /// Draws the view with the renderer's CPU reference and keeps the frame.
    fn draw(workbench: &mut Workbench) -> FrameOutput {
        let built = workbench.scene(SIZE, SIZE).unwrap();
        let mut renderer = ReferenceRenderer::new(TargetDescriptor {
            kind: TargetKind::Offscreen,
            width: SIZE,
            height: SIZE,
            backend: BackendPreference::Automatic,
        })
        .unwrap();
        let frame = renderer
            .render(RenderRequest {
                scene: &built.scene,
                resources: built.resources,
            })
            .unwrap();
        workbench.rendered(frame.clone());
        frame
    }

    fn center(frame: &FrameOutput) -> [u8; 3] {
        let index = usize::try_from((SIZE / 2 * SIZE + SIZE / 2) * 4).unwrap();
        [
            frame.rgba8[index],
            frame.rgba8[index + 1],
            frame.rgba8[index + 2],
        ]
    }

    #[test]
    fn a_stroke_on_the_picked_tile_shows_in_the_next_frame() {
        // The stroke is cancelled, not released, so the committed fixture
        // is never written.
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        workbench.set_view_size(egui::vec2(160.0, 160.0));
        assert!(workbench.is_stale(SIZE, SIZE));
        let frame = draw(&mut workbench);
        assert!(!workbench.is_stale(SIZE, SIZE));
        // The view starts over the middle of the plane's one chunk; points
        // are half a pixel each here.
        let tile = workbench.pick(egui::vec2(80.0, 80.0)).unwrap();
        assert_eq!(tile, [16, 16]);
        assert_eq!(center(&frame), swatch_color("grass"));
        let now = Instant::now();
        workbench.apply(vec![begin(tile, "water")], now);
        assert!(workbench.is_stale(SIZE, SIZE));
        assert_eq!(center(&draw(&mut workbench)), swatch_color("water"));
        assert!(workbench.status(None, None).dirty);
        workbench.apply(vec![Action::StrokeCancel], now);
        let status = workbench.status(None, None);
        assert!(!status.dirty && !status.waiting);
        assert_eq!(center(&draw(&mut workbench)), swatch_color("grass"));
    }

    #[test]
    fn a_released_stroke_is_saved_and_undone() {
        let root = meadow_copy();
        let mut workbench = Workbench::open(root.path(), None, true).unwrap();
        workbench.set_view_size(egui::vec2(160.0, 160.0));
        draw(&mut workbench);
        let tile = workbench.pick(egui::vec2(80.0, 80.0)).unwrap();
        let now = Instant::now();
        workbench.apply(vec![begin(tile, "sand"), Action::StrokeEnd], now);
        let status = workbench.status(None, None);
        assert!(!status.dirty && !status.waiting);
        assert_eq!(status.history.0, ["Paint sand on 1 tile"]);
        assert_eq!(
            status.message.as_deref(),
            Some("Saved Paint sand on 1 tile")
        );
        assert_eq!(center(&draw(&mut workbench)), swatch_color("sand"));
        let reopened = Workbench::open(root.path(), None, true).unwrap();
        assert_eq!(
            reopened.session.document().revision(),
            workbench.session.document().revision()
        );
        workbench.apply(vec![Action::Undo], now);
        assert_eq!(
            workbench.status(None, None).message.as_deref(),
            Some("Undid Paint sand on 1 tile")
        );
        assert_eq!(center(&draw(&mut workbench)), swatch_color("grass"));
    }

    #[test]
    fn panning_and_zooming_redraw_the_view() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        workbench.set_view_size(egui::vec2(160.0, 160.0));
        draw(&mut workbench);
        workbench.apply(vec![Action::Pan(egui::vec2(32.0, 0.0))], Instant::now());
        assert!(workbench.is_stale(SIZE, SIZE));
        draw(&mut workbench);
        // A drag of 64 pixels to the right shows two tiles further left.
        assert_eq!(workbench.pick(egui::vec2(80.0, 80.0)), Some([14, 16]));
        workbench.apply(vec![Action::Zoom(100.0)], Instant::now());
        draw(&mut workbench);
        assert_eq!(workbench.pick(egui::vec2(80.0, 80.0)), Some([14, 16]));
    }

    #[test]
    fn an_unknown_plane_is_an_error() {
        assert!(Workbench::open(&fixture_world("meadow"), Some("nowhere"), false).is_err());
    }
}
