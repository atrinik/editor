// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The editor window: eframe draws the egui interface, and the Tilewright
//! renderer draws the plane offscreen on its own GPU device. The window
//! shows the rendered frame as an image and keeps it for picking.

use crate::workbench::Workbench;
use eframe::egui;
use std::{path::Path, time::Instant};
use tilewright_editor_ui::editor::EditorUi;
use tilewright_render_api::{
    BackendPreference, RenderRequest, Renderer, TargetDescriptor, TargetKind,
};
use tilewright_render_wgpu::WgpuRenderer;

struct EditorWindow {
    workbench: Workbench,
    ui: EditorUi,
    renderer: Option<WgpuRenderer>,
    texture: Option<egui::TextureHandle>,
    /// The size in points the texture was drawn for.
    drawn: egui::Vec2,
    view_error: Option<String>,
}

impl EditorWindow {
    /// Draws the view again if it changed or the window resized.
    fn draw_view(&mut self, ctx: &egui::Context) {
        let size = self.ui.view_size;
        let scale = ctx.pixels_per_point();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (width, height) = (
            (size.x * scale).round().max(1.0) as u32,
            (size.y * scale).round().max(1.0) as u32,
        );
        if size.x < 1.0 || size.y < 1.0 || !self.workbench.is_stale(width, height) {
            return;
        }
        match self.render(width, height) {
            Ok(frame) => {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width as usize, frame.height as usize],
                    &frame.rgba8,
                );
                match &mut self.texture {
                    Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                    None => {
                        self.texture = Some(ctx.load_texture(
                            "tilewright.view",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                }
                self.drawn = size;
                self.view_error = None;
                self.workbench.rendered(frame);
            }
            Err(error) => {
                self.view_error = Some(error);
                self.texture = None;
            }
        }
    }

    fn render(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<tilewright_render_api::FrameOutput, String> {
        let built = self.workbench.scene(width, height)?;
        let renderer = match &mut self.renderer {
            Some(renderer) => {
                if renderer.target().width != width || renderer.target().height != height {
                    renderer
                        .resize(width, height)
                        .map_err(|error| format!("the renderer cannot resize: {error:?}"))?;
                }
                renderer
            }
            None => self.renderer.insert(
                WgpuRenderer::new(TargetDescriptor {
                    kind: TargetKind::Offscreen,
                    width,
                    height,
                    backend: BackendPreference::Automatic,
                })
                .map_err(|error| format!("no GPU renderer is available: {error:?}"))?,
            ),
        };
        renderer
            .render(RenderRequest {
                scene: &built.scene,
                resources: built.resources,
            })
            .map_err(|error| format!("the view did not render: {error:?}"))
    }
}

impl eframe::App for EditorWindow {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let view = self
            .texture
            .as_ref()
            .map(|texture| (texture.id(), self.drawn));
        let status = self.workbench.status(view, self.view_error.clone());
        let workbench = &self.workbench;
        let actions = self.ui.show(ui, &status, &|at| workbench.pick(at));
        self.workbench.set_view_size(self.ui.view_size);
        self.workbench.apply(actions, now);
        self.draw_view(ui.ctx());
    }
}

/// Opens the editor window on the world at `root`.
///
/// # Errors
/// Fails when the world does not open or the window cannot be created.
pub fn run(root: &Path, plane: Option<&str>, accepted: bool) -> Result<(), String> {
    let workbench = Workbench::open(root, plane, accepted)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tilewright editor")
            .with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Tilewright editor",
        options,
        Box::new(|_| {
            Ok(Box::new(EditorWindow {
                ui: EditorUi::new(workbench.palette()),
                workbench,
                renderer: None,
                texture: None,
                drawn: egui::Vec2::ZERO,
                view_error: None,
            }))
        }),
    )
    .map_err(|error| format!("the window failed: {error}"))
}
