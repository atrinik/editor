// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The editor window's interface, drawn with egui. It holds only view state
//! (the chosen terrain, brush and layer) and turns input into [`Action`]s;
//! the application carries them out through the session, so the interface
//! never edits a document itself.

use egui::{
    Align2, Color32, Key, KeyboardShortcut, Modifiers, PointerButton, Pos2, Rect, Sense, TextureId,
    Ui, Vec2,
};

/// Largest brush radius the interface offers, in tiles.
pub const MAXIMUM_RADIUS: u16 = 32;

/// Most history entries listed.
const HISTORY_SHOWN: usize = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrushShape {
    Square,
    Circle,
}

/// What the user asked for this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The primary button went down on a tile of the view: a stroke begins
    /// there with the chosen terrain and brush.
    StrokeBegin {
        tile: [i64; 2],
        terrain: String,
        shape: BrushShape,
        radius: u16,
        layer: i32,
    },
    /// The pointer moved onto another tile while the stroke runs.
    StrokeMove([i64; 2]),
    /// The primary button was released.
    StrokeEnd,
    /// Escape during a stroke: its tiles go back.
    StrokeCancel,
    Undo,
    Redo,
    /// Save again edits whose save could not be published.
    SaveNow,
    /// Drop edits whose save could not be published.
    Discard,
    /// Move the view by screen points, dragged with the secondary or
    /// middle button.
    Pan(Vec2),
    /// Scale the view's zoom by this factor.
    Zoom(f32),
    /// The user accepted what saving here does not check.
    AcceptUncheckedAccess,
}

/// One terrain of the palette, with the colour its swatch draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteEntry {
    pub terrain: String,
    pub color: [u8; 3],
}

/// What the application shows this frame.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// The world, plane and layer, for the title bar.
    pub title: String,
    /// Whether edits are not saved yet.
    pub dirty: bool,
    /// Whether a save could not be published and waits to be saved again
    /// or discarded.
    pub waiting: bool,
    /// Undo titles, oldest first, and redo titles, next first.
    pub history: (Vec<String>, Vec<String>),
    /// The last problem or notice.
    pub message: Option<String>,
    /// What saving here does not check, while the user has not
    /// accepted it.
    pub unchecked_access: Option<String>,
    /// The rendered view and its size in points.
    pub view: Option<(TextureId, Vec2)>,
    /// Why there is no rendered view.
    pub view_error: Option<String>,
}

/// The interface state of the editor window.
#[derive(Clone, Debug)]
pub struct EditorUi {
    pub palette: Vec<PaletteEntry>,
    /// The chosen terrain, an index into the palette.
    pub terrain: Option<usize>,
    pub shape: BrushShape,
    pub radius: u16,
    pub layer: i32,
    /// The view's size in points, as laid out in the last frame.
    pub view_size: Vec2,
    stroking: bool,
    last_tile: Option<[i64; 2]>,
}

impl EditorUi {
    #[must_use]
    pub fn new(palette: Vec<PaletteEntry>) -> Self {
        let terrain = (!palette.is_empty()).then_some(0);
        Self {
            palette,
            terrain,
            shape: BrushShape::Circle,
            radius: 1,
            layer: 0,
            view_size: Vec2::ZERO,
            stroking: false,
            last_tile: None,
        }
    }

    /// Whether a stroke is running.
    #[must_use]
    pub const fn is_stroking(&self) -> bool {
        self.stroking
    }

    /// Draws the window into `ui` and returns what the user asked for.
    /// `pick` finds the tile under a point of the view, measured in points
    /// from its top-left corner.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        status: &Status,
        pick: &dyn Fn(Vec2) -> Option<[i64; 2]>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        self.shortcuts(ui, &mut actions);
        egui::Panel::top("tilewright.top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(&status.title);
                ui.separator();
                let idle = !self.stroking;
                if ui
                    .add_enabled(
                        idle && (status.dirty || !status.history.0.is_empty()),
                        egui::Button::new("Undo"),
                    )
                    .clicked()
                {
                    actions.push(Action::Undo);
                }
                if ui
                    .add_enabled(
                        idle && !status.history.1.is_empty(),
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                {
                    actions.push(Action::Redo);
                }
                if ui
                    .add_enabled(idle && status.waiting, egui::Button::new("Save again"))
                    .clicked()
                {
                    actions.push(Action::SaveNow);
                }
                if ui
                    .add_enabled(idle && status.waiting, egui::Button::new("Discard"))
                    .clicked()
                {
                    actions.push(Action::Discard);
                }
            });
        });
        egui::Panel::bottom("tilewright.status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let state = if status.waiting {
                    "Unsaved: the last save failed"
                } else if status.dirty {
                    "Painting"
                } else {
                    "Saved"
                };
                ui.label(state);
                if let Some(message) = &status.message {
                    ui.separator();
                    ui.label(message);
                }
            });
        });
        egui::Panel::left("tilewright.tools").show(ui, |ui| {
            self.tools(ui, status);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.view(ui, status, pick, &mut actions);
        });
        if let Some(unchecked) = &status.unchecked_access {
            Self::notice(ui, unchecked, &mut actions);
        }
        actions
    }

    fn shortcuts(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let redo_y = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        ui.input_mut(|input| {
            // The longer shortcut first, so Shift+Command+Z is not also an undo.
            if input.consume_shortcut(&redo) || input.consume_shortcut(&redo_y) {
                actions.push(Action::Redo);
            } else if input.consume_shortcut(&undo) {
                actions.push(Action::Undo);
            }
            if input.consume_shortcut(&save) {
                actions.push(Action::SaveNow);
            }
            if self.stroking && input.key_pressed(Key::Escape) {
                actions.push(Action::StrokeCancel);
                self.stroking = false;
                self.last_tile = None;
            }
        });
    }

    fn tools(&mut self, ui: &mut Ui, status: &Status) {
        ui.heading("Terrain");
        if self.palette.is_empty() {
            ui.label("The profile has no terrain transitions to paint.");
        }
        for (index, entry) in self.palette.iter().enumerate() {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                let [red, green, blue] = entry.color;
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgb(red, green, blue));
                if ui
                    .selectable_label(self.terrain == Some(index), &entry.terrain)
                    .clicked()
                {
                    self.terrain = Some(index);
                }
            });
        }
        ui.separator();
        ui.heading("Brush");
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.shape, BrushShape::Circle, "Circle");
            ui.radio_value(&mut self.shape, BrushShape::Square, "Square");
        });
        ui.add(egui::Slider::new(&mut self.radius, 0..=MAXIMUM_RADIUS).text("radius"));
        ui.horizontal(|ui| {
            ui.label("Layer");
            ui.add(egui::DragValue::new(&mut self.layer));
        });
        ui.separator();
        ui.heading("History");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for title in status.history.1.iter().take(HISTORY_SHOWN).rev() {
                ui.weak(title);
            }
            for title in status.history.0.iter().rev().take(HISTORY_SHOWN) {
                ui.label(title);
            }
        });
    }

    fn view(
        &mut self,
        ui: &mut Ui,
        status: &Status,
        pick: &dyn Fn(Vec2) -> Option<[i64; 2]>,
        actions: &mut Vec<Action>,
    ) {
        let size = ui.available_size().max(Vec2::splat(1.0));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.view_size = rect.size();
        match &status.view {
            Some((texture, drawn)) => {
                let shown = Rect::from_min_size(rect.min, *drawn);
                ui.painter().image(
                    *texture,
                    shown,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    status
                        .view_error
                        .as_deref()
                        .unwrap_or("Drawing the view..."),
                    egui::FontId::proportional(14.0),
                    ui.visuals().weak_text_color(),
                );
            }
        }
        let (pressed, down, released, position, scroll) = ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.pointer.interact_pos().or(input.pointer.hover_pos()),
                input.smooth_scroll_delta.y,
            )
        });
        let tile = position
            .filter(|position| rect.contains(*position))
            .and_then(|position| pick(position - rect.min));
        if !self.stroking && pressed && response.hovered() {
            if let (Some(tile), Some(entry)) =
                (tile, self.terrain.and_then(|index| self.palette.get(index)))
            {
                actions.push(Action::StrokeBegin {
                    tile,
                    terrain: entry.terrain.clone(),
                    shape: self.shape,
                    radius: self.radius,
                    layer: self.layer,
                });
                self.stroking = true;
                self.last_tile = Some(tile);
            }
        } else if self.stroking
            && down
            && let Some(tile) = tile.filter(|tile| self.last_tile != Some(*tile))
        {
            actions.push(Action::StrokeMove(tile));
            self.last_tile = Some(tile);
        }
        if self.stroking && (released || !down) {
            actions.push(Action::StrokeEnd);
            self.stroking = false;
            self.last_tile = None;
        }
        if response.dragged_by(PointerButton::Secondary)
            || response.dragged_by(PointerButton::Middle)
        {
            actions.push(Action::Pan(response.drag_delta()));
        }
        if response.hovered() && scroll != 0.0 {
            actions.push(Action::Zoom((scroll / 400.0).exp()));
        }
    }

    fn notice(ui: &Ui, unchecked: &str, actions: &mut Vec<Action>) {
        egui::Modal::new(egui::Id::unique("tilewright.unchecked-access")).show(ui.ctx(), |ui| {
            ui.set_max_width(420.0);
            ui.heading("Saving here is not fully checked");
            ui.label(format!(
                "The editor saves through the world toolkit, which cannot check everything for this world here: {unchecked}."
            ));
            ui.label("Your edits stay unsaved until you accept this.");
            ui.horizontal(|ui| {
                if ui.button("Accept and save").clicked() {
                    actions.push(Action::AcceptUncheckedAccess);
                }
                if ui.button("Discard the unsaved edits").clicked() {
                    actions.push(Action::Discard);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, BrushShape, EditorUi, PaletteEntry, Status};
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    fn palette() -> Vec<PaletteEntry> {
        ["water", "sand", "grass"]
            .map(|terrain| PaletteEntry {
                terrain: terrain.to_owned(),
                color: [10, 20, 30],
            })
            .to_vec()
    }

    /// Tiles of 32 points from the view's corner.
    #[allow(clippy::cast_possible_truncation)]
    fn pick(at: Vec2) -> [i64; 2] {
        [(at.x / 32.0).floor() as i64, (at.y / 32.0).floor() as i64]
    }

    struct Window {
        context: egui::Context,
        ui: EditorUi,
        status: Status,
    }

    impl Window {
        fn new(palette: Vec<PaletteEntry>) -> Self {
            Self {
                context: egui::Context::default(),
                ui: EditorUi::new(palette),
                status: Status::default(),
            }
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
                events,
                ..RawInput::default()
            };
            let mut actions = Vec::new();
            let (ui, status) = (&mut self.ui, &self.status);
            let mut output = self.context.run_ui(input, |root| {
                actions = ui.show(root, status, &|at| Some(pick(at)));
            });
            // No renderer takes the frame's textures here.
            output.textures_delta.clear();
            actions
        }
    }

    fn button(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn a_drag_over_the_view_is_one_stroke() {
        let mut window = Window::new(palette());
        window.ui.terrain = Some(1);
        window.ui.shape = BrushShape::Square;
        window.ui.radius = 2;
        let at = Pos2::new(600.0, 300.0);
        window.frame(vec![Event::PointerMoved(at)]);
        let begun = window.frame(vec![button(at, true)]);
        let [
            Action::StrokeBegin {
                tile,
                terrain,
                shape,
                radius,
                layer,
            },
        ] = begun.as_slice()
        else {
            panic!("no stroke began: {begun:?}");
        };
        assert_eq!(
            (terrain.as_str(), *shape, *radius, *layer),
            ("sand", BrushShape::Square, 2, 0)
        );
        assert!(window.ui.is_stroking());
        // The same tile again moves nothing; the next tile is a stamp.
        assert_eq!(window.frame(vec![Event::PointerMoved(at)]), []);
        let moved = window.frame(vec![Event::PointerMoved(at + Vec2::new(64.0, 0.0))]);
        assert_eq!(moved, [Action::StrokeMove([tile[0] + 2, tile[1]])]);
        let ended = window.frame(vec![button(at + Vec2::new(64.0, 0.0), false)]);
        assert_eq!(ended, [Action::StrokeEnd]);
        assert!(!window.ui.is_stroking());
        assert!(window.ui.view_size.x > 0.0 && window.ui.view_size.y > 0.0);
    }

    #[test]
    fn escape_cancels_a_stroke() {
        let mut window = Window::new(palette());
        let at = Pos2::new(600.0, 300.0);
        window.frame(vec![Event::PointerMoved(at)]);
        assert!(matches!(
            window.frame(vec![button(at, true)]).as_slice(),
            [Action::StrokeBegin { .. }]
        ));
        assert_eq!(
            window.frame(vec![key(Key::Escape, Modifiers::NONE)]),
            [Action::StrokeCancel]
        );
        assert_eq!(window.frame(vec![button(at, false)]), []);
    }

    #[test]
    fn without_terrains_the_view_does_not_paint() {
        let mut window = Window::new(Vec::new());
        let at = Pos2::new(600.0, 300.0);
        window.frame(vec![Event::PointerMoved(at)]);
        assert_eq!(window.frame(vec![button(at, true)]), []);
        assert!(!window.ui.is_stroking());
    }

    #[test]
    fn shortcuts_undo_redo_and_save() {
        let mut window = Window::new(palette());
        assert_eq!(
            window.frame(vec![key(Key::Z, Modifiers::COMMAND)]),
            [Action::Undo]
        );
        assert_eq!(
            window.frame(vec![key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT)]),
            [Action::Redo]
        );
        assert_eq!(
            window.frame(vec![key(Key::Y, Modifiers::COMMAND)]),
            [Action::Redo]
        );
        assert_eq!(
            window.frame(vec![key(Key::S, Modifiers::COMMAND)]),
            [Action::SaveNow]
        );
    }

    #[test]
    fn the_platform_notice_draws_over_the_window() {
        let mut window = Window::new(palette());
        window.status.dirty = true;
        window.status.unchecked_access = Some("access control lists are not checked".to_owned());
        window.status.message = Some("Saving needs your consent".to_owned());
        assert_eq!(window.frame(Vec::new()), []);
    }
}
