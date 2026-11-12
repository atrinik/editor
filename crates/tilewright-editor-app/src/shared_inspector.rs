// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! One opt-in shared field, embedded in the existing host. Domain edits remain
//! ToolState/Session actions; this adapter has no filesystem or World authority.
use tilewright_app::{App, PresentationStamp};
use tilewright_editor_ui::inspector::SnapshotStamp;
use tilewright_ui::{Composition, Input, Limits, TextSelection, WidgetRef};
use tilewright_ui_render::{CompiledFrame, FontPack, Namespace, TextLimits, UiLimits, UiRenderer};

type Result<T> = std::result::Result<T, String>;

pub struct SharedInspector {
    pub app: App,
    renderer: UiRenderer,
    pub binding: Option<(WidgetRef, SnapshotStamp, String)>,
    pending: Option<(PresentationStamp, CompiledFrame)>,
    displayed: Option<(PresentationStamp, CompiledFrame)>,
}
impl SharedInspector {
    pub fn new(fonts: FontPack, limits: TextLimits) -> Result<Self> {
        Ok(Self {
            app: App::with_composition(Limits::default(), Composition::SingleField)
                .map_err(|e| e.to_string())?,
            renderer: UiRenderer::new(
                fonts,
                limits,
                Namespace {
                    first_semantic: 0xf000_0000,
                    solid_resource: 0xffff_ffff_ffff_ff00,
                    atlas_resource: 0xffff_ffff_ffff_ff01,
                },
                UiLimits::default(),
            )
            .map_err(|e| e.to_string())?,
            binding: None,
            pending: None,
            displayed: None,
        })
    }
    pub fn active(&self) -> bool {
        self.binding.is_some()
    }
    pub fn bind(&mut self, text: String, stamp: SnapshotStamp, field: String) -> Result<()> {
        self.app.bind_field(text).map_err(|e| e.to_string())?;
        self.binding = Some((self.app.ui().composer_widget(), stamp, field));
        self.app
            .apply(Input::ComposerVisible(true))
            .map_err(|e| e.to_string())?;
        // Showing a retired field may itself advance widget generation.
        self.binding.as_mut().expect("just assigned").0 = self.app.ui().composer_widget();
        self.invalidate()?;
        self.focus()
    }
    pub fn focus(&mut self) -> Result<()> {
        self.app
            .apply(Input::Focus(Some(tilewright_ui::COMPOSER)))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn retire(&mut self) -> Result<()> {
        self.app
            .bind_field(String::new())
            .map_err(|e| e.to_string())?;
        self.app
            .apply(Input::ComposerVisible(false))
            .map_err(|e| e.to_string())?;
        self.binding = None;
        self.invalidate()
    }
    pub fn invalidate(&mut self) -> Result<()> {
        self.app.invalidate_surface().map_err(|e| e.to_string())?;
        self.pending = None;
        self.displayed = None;
        Ok(())
    }
    pub fn layout(
        &mut self,
        logical: (u32, u32),
        physical: (u32, u32),
        bounds: crate::workspace_ui::Rect,
    ) -> Result<()> {
        #[allow(clippy::cast_precision_loss)]
        let viewport = tilewright_ui::Viewport::new(
            logical.0 as f32,
            logical.1 as f32,
            physical.0 as f32 / logical.0.max(1) as f32,
        )
        .map_err(|e| e.to_string())?;
        #[allow(clippy::cast_precision_loss)]
        let bounds = tilewright_ui::Rect {
            x: bounds.x as f32,
            y: bounds.y as f32,
            width: bounds.width as f32,
            height: bounds.height as f32,
        };
        let before = self.app.surface_epoch();
        self.app
            .set_layout(viewport, bounds, bounds)
            .map_err(|e| e.to_string())?;
        if before != self.app.surface_epoch() {
            self.pending = None;
            self.displayed = None;
        }
        Ok(())
    }
    pub fn compose(
        &mut self,
        scene: &tilewright_scene::SceneBundle,
        resources: std::sync::Arc<dyn tilewright_render_resources::ResourceProvider>,
    ) -> Result<tilewright_ui_render::Presentation> {
        if let Some((stamp, _)) = self.pending.take() {
            self.app
                .discard_prepared(stamp)
                .map_err(|e| e.to_string())?;
        }
        let prepared = self.app.prepare_current().map_err(|e| e.to_string())?;
        let compiled = match self.renderer.compile(
            &prepared.snapshot,
            &prepared.layout,
            [scene.viewport().width, scene.viewport().height],
        ) {
            Ok(frame) => frame,
            Err(error) => {
                self.app
                    .discard_prepared(prepared.stamp)
                    .map_err(|e| e.to_string())?;
                return Err(error.to_string());
            }
        };
        let presentation = match compiled.compose(scene, resources) {
            Ok(presentation) => presentation,
            Err(error) => {
                self.app
                    .discard_prepared(prepared.stamp)
                    .map_err(|e| e.to_string())?;
                return Err(error.to_string());
            }
        };
        self.pending = Some((prepared.stamp, compiled));
        Ok(presentation)
    }
    pub fn ack_presented(&mut self) -> Result<()> {
        if let Some((stamp, frame)) = self.pending.take() {
            self.app.ack_presented(stamp).map_err(|e| e.to_string())?;
            self.displayed = Some((stamp, frame));
        }
        Ok(())
    }
    pub fn ready(&self) -> bool {
        !self.active()
            || self
                .displayed
                .as_ref()
                .is_some_and(|(stamp, _)| self.app.validate_displayed(*stamp).is_ok())
    }
    pub fn geometry(&self) -> Option<&tilewright_ui_render::TextGeometry> {
        let (stamp, frame) = self.displayed.as_ref()?;
        self.app.validate_displayed(*stamp).ok()?;
        frame.editable()
    }
    pub fn pointer(&mut self, position: [f32; 2], extend: bool) -> Result<bool> {
        if !self.active() {
            return Ok(false);
        }
        let stamp = self
            .displayed
            .as_ref()
            .ok_or("shared input waits for displayed geometry")?
            .0;
        let Some(widget) = self
            .app
            .hit_test(stamp, position[0], position[1])
            .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        if widget.id == tilewright_ui::SUBMIT {
            self.app
                .apply(Input::Activate(tilewright_ui::SUBMIT))
                .map_err(|e| e.to_string())?;
        } else if widget.id == tilewright_ui::COMPOSER {
            if self.app.ui().snapshot().composer.composition.is_some() {
                let target = self
                    .app
                    .ui()
                    .text_target()
                    .ok_or("no shared composition owner")?;
                self.app
                    .apply(Input::CancelComposition { target })
                    .map_err(|e| e.to_string())?;
                return Ok(true);
            }
            // Focus changes the target epoch; get fresh identity after it.
            let geometry = self.geometry().ok_or("stale shared geometry")?;
            let snapshot = geometry.snapshot.as_ref().ok_or("no editable snapshot")?;
            let cursor = geometry.hit(position).ok_or("outside shared text")?;
            let cursor = snapshot
                .committed_cursor(cursor)
                .map_err(|e| e.to_string())?;
            let selection = if extend {
                TextSelection {
                    anchor: snapshot.selection.anchor,
                    focus: cursor,
                }
            } else {
                TextSelection::caret(cursor)
            };
            self.focus()?;
            let target = self.app.ui().text_target().ok_or("no shared focus")?;
            self.app
                .apply(Input::Selection { target, selection })
                .map_err(|e| e.to_string())?;
        }
        Ok(true)
    }
    /// Keyboard movement shapes the current draft with the exact same renderer
    /// and fonts; it does not borrow stale displayed geometry after an edit.
    pub fn move_visual(
        &mut self,
        motion: tilewright_ui_render::VisualMotion,
        extend: bool,
    ) -> Result<()> {
        let target = self.app.ui().text_target().ok_or("no shared focus")?;
        let snapshot = self.app.ui().snapshot();
        if snapshot.composer.composition.is_some() {
            return Ok(());
        }
        let prepared = self.app.prepare_current().map_err(|e| e.to_string())?;
        let viewport = prepared.layout.viewport;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let compiled = self.renderer.compile(
            &prepared.snapshot,
            &prepared.layout,
            [viewport.width as u32, viewport.height as u32],
        );
        self.app
            .discard_prepared(prepared.stamp)
            .map_err(|e| e.to_string())?;
        let compiled = compiled.map_err(|e| e.to_string())?;
        let geometry = compiled.editable().ok_or("missing shared text geometry")?;
        let anchor = snapshot.composer.selection.anchor;
        let current = snapshot.composer.selection.focus;
        let focus = if !extend
            && anchor.byte != current.byte
            && matches!(
                motion,
                tilewright_ui_render::VisualMotion::Left
                    | tilewright_ui_render::VisualMotion::Right
            ) {
            let cursor = |value: tilewright_ui::TextCursor| tilewright_ui_render::TextCursor {
                byte: value.byte,
                affinity: match value.affinity {
                    tilewright_ui::TextAffinity::Before => {
                        tilewright_ui_render::TextAffinity::Before
                    }
                    tilewright_ui::TextAffinity::After => tilewright_ui_render::TextAffinity::After,
                },
            };
            let a = geometry
                .layout
                .caret(cursor(anchor))
                .ok_or("invalid shared anchor")?;
            let f = geometry
                .layout
                .caret(cursor(current))
                .ok_or("invalid shared focus")?;
            let anchor_first =
                a.y.total_cmp(&f.y).then(a.x.total_cmp(&f.x)) != std::cmp::Ordering::Greater;
            if anchor_first == (motion == tilewright_ui_render::VisualMotion::Left) {
                anchor
            } else {
                current
            }
        } else {
            geometry.move_visual(current, motion)
        };
        let selection = if extend {
            TextSelection {
                anchor: snapshot.composer.selection.anchor,
                focus,
            }
        } else {
            TextSelection::caret(focus)
        };
        self.app
            .apply(Input::Selection { target, selection })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
