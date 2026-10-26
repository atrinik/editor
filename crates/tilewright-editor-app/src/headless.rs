// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `tilewright-editor headless`: the window's interface and view composed
//! for a few frames without a window or GPU, so validation can prove that
//! they work together on a world.

use crate::workbench::Workbench;
use eframe::egui;
use std::{path::Path, time::Instant};
use tilewright_editor_ui::editor::EditorUi;

/// Runs two frames of the interface over the world at `root` and builds the
/// scene of its view. Returns a one-line report.
///
/// # Errors
/// Fails when the world does not open or the view cannot be built.
pub fn run(root: &Path) -> Result<String, String> {
    let mut workbench = Workbench::open(root, None, false)?;
    let mut ui = EditorUi::new(workbench.palette());
    let context = egui::Context::default();
    let now = Instant::now();
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..egui::RawInput::default()
        };
        let status = workbench.status(None, None);
        let mut actions = Vec::new();
        let mut output = context.run_ui(input, |root| {
            actions = ui.show(root, &status, &|at| workbench.pick(at));
        });
        // Nothing draws the frame's textures without a window.
        output.textures_delta.clear();
        workbench.set_view_size(ui.view_size);
        workbench.apply(actions, now);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (width, height) = (ui.view_size.x as u32, ui.view_size.y as u32);
    let scene = workbench.scene(width, height)?;
    Ok(format!(
        "headless terrains={} view={width}x{height} tiles={} quads={}",
        ui.palette.len(),
        scene.tiles,
        scene.scene.quads().len()
    ))
}
