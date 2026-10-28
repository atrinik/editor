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
                self.workbench.forget_frame();
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
    /// Runs also while the window is minimized or hidden, when `ui` does
    /// not, so a request to close is always answered here.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Closing with unsaved edits asks first: the window stays open until
        // they are saved or discarded, and comes back into view to ask where
        // the platform allows it. Wayland lets a window neither unminimize
        // nor focus itself, so the window also asks for attention, which
        // the compositor shows.
        if ctx.input(|input| input.viewport().close_requested())
            && !self.workbench.request_close(Instant::now())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

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
        if self.workbench.may_close() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.draw_view(ui.ctx());
    }
}

/// Opens the editor window on the world at `root`.
///
/// # Errors
/// Fails when the world does not open or the window cannot be created.
pub fn run(root: &Path, plane: Option<&str>, accepted: bool) -> Result<(), String> {
    // The root is resolved before the process leaves the current folder;
    // where it cannot be resolved through links, as on some network
    // shares, it is made absolute as given.
    let root = root
        .canonicalize()
        .or_else(|_| std::path::absolute(root))
        .map_err(|error| format!("cannot resolve the world's root: {error}"))?;
    leave_world_directory()?;
    let workbench = Workbench::open(&root, plane, accepted)?;
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tilewright editor")
            .with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    };
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        // FXC ships with Windows; the default would look for the DXC
        // compiler's DLL by name, and so in the current directory too.
        // Everything else stays as eframe sets it up.
        setup
            .instance_descriptor
            .backend_options
            .dx12
            .shader_compiler = eframe::wgpu::Dx12Compiler::Fxc;
    }
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

/// On Windows, moves the process out of the directory it was started in,
/// into the editor's own: the GPU libraries load DLLs such as
/// `dxcompiler.dll` and `vulkan-1.dll` by name, and Windows searches the
/// current directory for those, so a world opened from its own folder could
/// otherwise plant one. Paths the editor was given are resolved first.
/// Elsewhere libraries are not searched for in the current directory.
#[cfg_attr(not(windows), allow(clippy::unnecessary_wraps))]
fn leave_world_directory() -> Result<(), String> {
    #[cfg(windows)]
    {
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot find the editor's own folder: {error}"))?;
        let folder = executable
            .parent()
            .ok_or("cannot find the editor's own folder")?;
        std::env::set_current_dir(folder)
            .map_err(|error| format!("cannot leave the current folder: {error}"))?;
    }
    Ok(())
}
