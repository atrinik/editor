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
    GroundCamera, GroundScene, GroundTile, PickMap, ground_scene, snap_zoom, swatch_color,
    view_bounds,
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
    /// Whether the interaction the interface began is a stroke the session
    /// started; when it refused one, what follows it is ignored.
    stroking: bool,
    closing: Closing,
}

/// Where a request to close the window stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Closing {
    /// Nobody asked to close.
    Open,
    /// The user asked while edits were unsaved, and is being asked first.
    Asking,
    /// The window may close.
    Now,
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
            stroking: false,
            closing: Closing::Open,
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
            } => self.begin(now, tile, &terrain, (shape, radius), layer)?,
            // A stroke the session refused has nothing to stamp, end or
            // cancel, and its reason stays shown.
            Action::StrokeMove(_) | Action::StrokeEnd | Action::StrokeCancel if !self.stroking => {}
            Action::StrokeMove(tile) => self.stamp(tile)?,
            Action::StrokeEnd => {
                self.stroking = false;
                self.stale = true;
                let result = self.session.end_stroke(now);
                if let Some(saved) = self.checked(result)? {
                    self.saved(&saved);
                }
            }
            Action::StrokeCancel => {
                self.stroking = false;
                self.stale = true;
                let result = self.session.cancel_stroke();
                self.checked(result)?;
            }
            Action::Undo => self.undo_or_redo(false)?,
            Action::Redo => self.undo_or_redo(true)?,
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
            Action::DiscardAndClose => {
                if self.session.is_interacting() {
                    self.stroking = false;
                    let result = self.session.cancel_stroke();
                    self.checked(result)?;
                }
                let result = self.session.discard();
                self.checked(result)?;
                self.consent = None;
                self.closing = Closing::Now;
            }
            Action::KeepEditing => self.closing = Closing::Open,
            Action::Layer(layer) => {
                if layer != self.layer {
                    self.layer = layer;
                    self.stale = true;
                }
            }
            Action::Pan(delta) => {
                self.camera.pan([
                    delta.x * self.pixels_per_point(),
                    delta.y * self.pixels_per_point(),
                ]);
                self.stale = true;
            }
            Action::Zoom(factor) => {
                // Zoom moves in steps at which every tile spans whole
                // pixels, so no seam opens between tiles.
                let current = self.camera.zoom;
                self.camera.zoom =
                    snap_zoom(self.camera.projection, current, current * factor, ZOOM);
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

    fn begin(
        &mut self,
        now: Instant,
        tile: [i64; 2],
        terrain: &str,
        (shape, radius): (BrushShape, u16),
        layer: i32,
    ) -> Result<(), String> {
        if layer != self.layer {
            self.layer = layer;
            self.stale = true;
        }
        self.stroking = false;
        let brush = Brush {
            shape: match shape {
                BrushShape::Square => Shape::Square,
                BrushShape::Circle => Shape::Circle,
            },
            radius,
        };
        let result = self
            .session
            .begin_stroke(now, &self.plane, layer, terrain, brush);
        self.checked(result)?;
        self.stroking = true;
        self.message = None;
        self.stamp(tile)
    }

    fn undo_or_redo(&mut self, redo: bool) -> Result<(), String> {
        self.stale = true;
        let (undo, redone) = self.session.history();
        let (what, verb, title, result) = if redo {
            (
                "redo",
                "Redid",
                redone.first().cloned(),
                self.session.redo(),
            )
        } else {
            ("undo", "Undid", undo.last().cloned(), self.session.undo())
        };
        match self.step(result, what, title)? {
            Some(saved) => {
                if !redo {
                    self.consent = None;
                }
                self.message = Some(described(verb, &saved));
            }
            None => self.message = Some(format!("Nothing to {what}")),
        }
        Ok(())
    }

    /// An undo or redo result in words: a rejected one says which step it
    /// could not `what`, and that the world was read again.
    fn step(
        &mut self,
        result: Result<Option<Saved>, SessionError>,
        what: &str,
        title: Option<String>,
    ) -> Result<Option<Saved>, String> {
        match result {
            Err(SessionError::Rejected(reason)) => {
                self.stale = true;
                Err(format!(
                    "Could not {what} {}, so it stays in the history and the world was read again: {reason}",
                    title.unwrap_or_default()
                ))
            }
            other => self.checked(other),
        }
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
                match self.session.take_rejected() {
                    Some(rejected) => format!(
                        "Could not save {}, so it was dropped and the world read again: {reason}",
                        rejected.title
                    ),
                    None => reason.to_string(),
                }
            }
            SessionError::TooLarge(reason) => {
                self.stale = true;
                match self.session.take_rejected() {
                    Some(rejected) => format!(
                        "Could not save {}, so it was dropped: the edit is too large to save as one step: {reason}",
                        rejected.title
                    ),
                    None => format!("The edit is too large to save as one step: {reason}"),
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
        self.message = Some(described("Saved", saved));
    }

    /// Asks to close the window: it may close at once when nothing is
    /// unsaved, and otherwise asks the user first.
    pub fn request_close(&mut self) -> bool {
        if self.closing == Closing::Now || !self.session.is_dirty() {
            return true;
        }
        self.closing = Closing::Asking;
        false
    }

    /// Whether the window may close now: the user discarded the unsaved
    /// edits, or they were saved while the question was open.
    pub fn may_close(&mut self) -> bool {
        if self.closing == Closing::Asking && !self.session.is_dirty() {
            self.closing = Closing::Now;
        }
        self.closing == Closing::Now
    }

    /// Forgets the frame shown last, as when the view could not be drawn,
    /// so the pointer picks nothing until a new one is.
    pub fn forget_frame(&mut self) {
        self.frame = None;
        self.picks = PickMap::default();
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
            closing: self.closing == Closing::Asking,
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
        // Until this view is rendered, nothing may be picked from the last.
        self.forget_frame();
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

/// A save, undo or redo in words, with a problem the toolkit reported after
/// it published every file.
fn described(verb: &str, saved: &Saved) -> String {
    match &saved.warning {
        Some(warning) => format!("{verb} {}, but: {warning}", saved.title),
        None => format!("{verb} {}", saved.title),
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

    /// An outside program sets ground tile `at` of the meadow to `value`.
    fn outside_edit(root: &std::path::Path, at: [i64; 2], value: &str) {
        let operation = format!(
            "{{\"op\": \"terrain.set\", \"plane\": \"meadow\", \"layer\": 0, \"grid\": \"ground\", \"cells\": [{{\"at\": [{}, {}], \"value\": \"{value}\"}}]}}",
            at[0], at[1]
        );
        let text = transaction("Outside edit", &[operation], &[]);
        let parsed = tilewright_edit::Transaction::parse("outside.jsonc", text).unwrap();
        tilewright_edit::plan(root, &parsed, &tilewright_edit::Options::default())
            .unwrap()
            .publish_with(&tilewright_edit::PublishOptions {
                unchecked_access: true,
            })
            .unwrap();
    }

    #[test]
    fn a_view_that_cannot_be_drawn_picks_nothing() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        workbench.set_view_size(egui::vec2(1_600.0, 1_000.0));
        let built = workbench.scene(3_200, 2_000);
        assert!(built.is_ok());
        workbench.rendered(FrameOutput {
            width: 1,
            height: 1,
            rgba8: vec![0; 4],
            semantic_ids: vec![1],
            depth: vec![0],
            coverage: vec![0],
            metrics: tilewright_render_api::RecoveryMetrics::default(),
        });
        // Zoomed out past what a view holds, the view fails to build.
        workbench.apply(vec![Action::Zoom(0.01)], Instant::now());
        assert!(workbench.scene(3_200, 2_000).is_err());
        assert_eq!(workbench.pick(egui::vec2(0.0, 0.0)), None);
        // So does forgetting the frame after a failed render.
        workbench.apply(vec![Action::Zoom(4.0)], Instant::now());
        workbench.set_view_size(egui::vec2(160.0, 160.0));
        draw(&mut workbench);
        assert!(workbench.pick(egui::vec2(80.0, 80.0)).is_some());
        workbench.forget_frame();
        assert_eq!(workbench.pick(egui::vec2(80.0, 80.0)), None);
    }

    #[test]
    fn the_layer_control_switches_the_view() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        workbench.set_view_size(egui::vec2(160.0, 160.0));
        draw(&mut workbench);
        workbench.apply(vec![Action::Layer(1)], Instant::now());
        assert!(workbench.is_stale(SIZE, SIZE));
        assert!(workbench.status(None, None).title.ends_with("layer 1"));
        // Layer 1 has no chunks, so the view shows empty tiles.
        assert_ne!(center(&draw(&mut workbench)), swatch_color("grass"));
    }

    #[test]
    fn zoom_moves_in_whole_pixel_steps() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        workbench.apply(vec![Action::Zoom(1.13)], Instant::now());
        let tile = f64::from(workbench.camera.zoom) * 32.0;
        assert!((tile - tile.round()).abs() < 1e-3, "{tile}");
        assert!(workbench.camera.zoom > 1.0);
    }

    #[test]
    fn a_refused_stroke_keeps_its_reason() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        let now = Instant::now();
        workbench.apply(vec![begin([1, 1], "lava")], now);
        let refused = workbench.status(None, None).message.unwrap();
        assert!(refused.contains("lava"), "{refused}");
        workbench.apply(vec![Action::StrokeMove([2, 1]), Action::StrokeEnd], now);
        assert_eq!(workbench.status(None, None).message.unwrap(), refused);
    }

    #[test]
    fn rejected_saves_and_undos_say_what_happened() {
        let root = meadow_copy();
        let mut workbench = Workbench::open(root.path(), None, true).unwrap();
        let now = Instant::now();
        workbench.apply(vec![begin([20, 20], "sand"), Action::StrokeEnd], now);
        outside_edit(root.path(), [1, 1], "water");
        // A save over the outside change is dropped and says so.
        let later = now + std::time::Duration::from_secs(5);
        workbench.apply(vec![begin([1, 1], "sand"), Action::StrokeEnd], later);
        let message = workbench.status(None, None).message.unwrap();
        assert!(
            message.starts_with("Could not save Paint sand on 1 tile, so it was dropped"),
            "{message}"
        );
        // An undo over it names the step it could not undo, not the save.
        workbench.apply(vec![Action::Undo], later);
        let message = workbench.status(None, None).message.unwrap();
        assert!(
            message.starts_with("Could not undo Paint sand on 1 tile, so it stays"),
            "{message}"
        );
        assert_eq!(workbench.status(None, None).history.0.len(), 1);
    }

    #[test]
    fn closing_with_unsaved_edits_asks_first() {
        let mut workbench = Workbench::open(&fixture_world("meadow"), None, false).unwrap();
        assert!(workbench.request_close(), "nothing unsaved");
        let now = Instant::now();
        // A stroke still being painted is unsaved.
        workbench.apply(vec![begin([16, 16], "water")], now);
        assert!(!workbench.request_close());
        assert!(workbench.status(None, None).closing);
        assert!(!workbench.may_close());
        workbench.apply(vec![Action::KeepEditing], now);
        assert!(!workbench.status(None, None).closing);
        assert!(!workbench.request_close());
        // Discarding drops the stroke, so the fixture is never written.
        workbench.apply(vec![Action::DiscardAndClose], now);
        assert!(workbench.may_close());
        assert!(!workbench.status(None, None).dirty);
    }

    #[test]
    fn saving_asks_for_consent_where_it_cannot_check_everything() {
        let root = meadow_copy();
        let mut workbench = Workbench::open(root.path(), None, false).unwrap();
        let now = Instant::now();
        workbench.apply(vec![begin([1, 1], "sand"), Action::StrokeEnd], now);
        let status = workbench.status(None, None);
        if let Some(unchecked) = status.unchecked_access {
            // macOS, Windows, or a file system that hides access lists.
            assert!(!unchecked.is_empty());
            assert!(status.waiting);
            workbench.apply(vec![Action::AcceptUncheckedAccess], now);
        }
        let status = workbench.status(None, None);
        assert!(!status.waiting && status.unchecked_access.is_none());
        assert_eq!(status.history.0, ["Paint sand on 1 tile"]);
    }
}
