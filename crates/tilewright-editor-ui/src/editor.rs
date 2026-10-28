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
    /// Show and paint another layer.
    Layer(i32),
    /// Drop edits whose save could not be published, and close the window.
    DiscardAndClose,
    /// Keep the window open after a request to close it.
    KeepEditing,
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
    /// Whether the user asked to close the window while edits are unsaved.
    pub closing: bool,
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
    /// Where the open dialog's buttons were last drawn, first to last.
    dialog_buttons: Vec<Rect>,
    /// Where the layer field was last drawn.
    layer_field: Rect,
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
            dialog_buttons: Vec::new(),
            layer_field: Rect::NOTHING,
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
            let state = if status.waiting {
                "Unsaved: the last save failed"
            } else if status.dirty {
                "Painting"
            } else {
                "Saved"
            };
            ui.label(state);
            // A reason can be long; it wraps rather than run off the window.
            if let Some(message) = &status.message {
                ui.add(egui::Label::new(message).wrap());
            }
        });
        egui::Panel::left("tilewright.tools").show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.tools(ui, status, &mut actions);
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            self.view(ui, status, pick, &mut actions);
        });
        self.dialog_buttons.clear();
        // Asked to close, the question about closing comes first; it says
        // what saving would not check when that is what a save waits for.
        if status.closing {
            self.closing(ui, status, &mut actions);
        } else if let Some(unchecked) = &status.unchecked_access {
            self.notice(ui, unchecked, &mut actions);
        }
        actions
    }

    fn shortcuts(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        // A focused text field, such as the layer being typed, keeps its
        // own undo, redo and keys.
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
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

    fn tools(&mut self, ui: &mut Ui, status: &Status, actions: &mut Vec<Action>) {
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
            let layer = self.layer;
            self.layer_field = ui
                .add_enabled(!self.stroking, egui::DragValue::new(&mut self.layer))
                .rect;
            if self.layer != layer {
                actions.push(Action::Layer(self.layer));
            }
        });
        ui.separator();
        ui.heading("History");
        for title in status.history.1.iter().take(HISTORY_SHOWN).rev() {
            ui.weak(title);
        }
        for title in status.history.0.iter().rev().take(HISTORY_SHOWN) {
            ui.label(title);
        }
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
        // While a save waits, painting would only add to what cannot be
        // saved, so the view takes no strokes until it is answered.
        let blocked = status.waiting || status.unchecked_access.is_some() || status.closing;
        // A question that opens mid-stroke, as when the window is asked to
        // close, ends the stroke: the button's release then belongs to
        // nothing.
        if self.stroking && blocked {
            actions.push(Action::StrokeEnd);
            self.stroking = false;
            self.last_tile = None;
        }
        if !self.stroking && !blocked && pressed && response.hovered() {
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

    /// Adds a dialog button and remembers where it was drawn.
    fn dialog_button(&mut self, ui: &mut Ui, text: &str) -> bool {
        let response = ui.button(text);
        self.dialog_buttons.push(response.rect);
        response.clicked()
    }

    fn notice(&mut self, ui: &Ui, unchecked: &str, actions: &mut Vec<Action>) {
        egui::Modal::new(egui::Id::unique("tilewright.unchecked-access")).show(ui.ctx(), |ui| {
            ui.set_max_width(420.0);
            ui.heading("Saving here is not fully checked");
            ui.add(egui::Label::new(format!(
                "The editor saves through the world toolkit, which cannot check everything for this world here: {unchecked}."
            )).wrap());
            ui.label("Your edits stay unsaved until you accept this.");
            ui.horizontal(|ui| {
                if self.dialog_button(ui, "Accept and save") {
                    actions.push(Action::AcceptUncheckedAccess);
                }
                if self.dialog_button(ui, "Discard the unsaved edits") {
                    actions.push(Action::Discard);
                }
            });
        });
    }

    fn closing(&mut self, ui: &Ui, status: &Status, actions: &mut Vec<Action>) {
        egui::Modal::new(egui::Id::unique("tilewright.closing")).show(ui.ctx(), |ui| {
            ui.set_max_width(420.0);
            ui.heading("Unsaved edits");
            if let Some(unchecked) = &status.unchecked_access {
                ui.add(egui::Label::new(format!(
                    "The last save waits for your consent: the world toolkit cannot check everything for this world here: {unchecked}."
                )).wrap());
            } else {
                ui.label(if status.waiting {
                    "The last save failed, so its edits are not in the world yet."
                } else {
                    "Some edits are not saved yet."
                });
                if let Some(message) = &status.message {
                    ui.add(egui::Label::new(message).wrap());
                }
            }
            ui.horizontal(|ui| {
                if status.unchecked_access.is_some() {
                    if self.dialog_button(ui, "Accept and save") {
                        actions.push(Action::AcceptUncheckedAccess);
                    }
                } else if status.waiting && self.dialog_button(ui, "Save again") {
                    actions.push(Action::SaveNow);
                }
                if self.dialog_button(ui, "Discard and close") {
                    actions.push(Action::DiscardAndClose);
                }
                if self.dialog_button(ui, "Keep editing") {
                    actions.push(Action::KeepEditing);
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

        /// Clicks at `at`: a move, a press and a release, in three frames,
        /// returning what all three asked for.
        fn click(&mut self, at: Pos2) -> Vec<Action> {
            let mut actions = self.frame(vec![Event::PointerMoved(at)]);
            actions.extend(self.frame(vec![button(at, true)]));
            actions.extend(self.frame(vec![button(at, false)]));
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

    const IN_VIEW: Pos2 = Pos2::new(600.0, 300.0);

    #[test]
    fn a_drag_over_the_view_is_one_stroke() {
        let mut window = Window::new(palette());
        window.ui.terrain = Some(1);
        window.ui.shape = BrushShape::Square;
        window.ui.radius = 2;
        let at = IN_VIEW;
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
        window.frame(vec![Event::PointerMoved(IN_VIEW)]);
        assert!(matches!(
            window.frame(vec![button(IN_VIEW, true)]).as_slice(),
            [Action::StrokeBegin { .. }]
        ));
        assert_eq!(
            window.frame(vec![key(Key::Escape, Modifiers::NONE)]),
            [Action::StrokeCancel]
        );
        assert_eq!(window.frame(vec![button(IN_VIEW, false)]), []);
    }

    #[test]
    fn without_terrains_the_view_does_not_paint() {
        let mut window = Window::new(Vec::new());
        assert_eq!(window.click(IN_VIEW), []);
        assert!(!window.ui.is_stroking());
    }

    #[test]
    fn the_view_takes_no_strokes_while_a_save_waits() {
        let mut window = Window::new(palette());
        window.status.waiting = true;
        assert_eq!(window.click(IN_VIEW), []);
        window.status.waiting = false;
        window.status.unchecked_access = Some("lists are not checked".to_owned());
        assert_eq!(window.click(IN_VIEW), []);
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
    fn the_layer_field_changes_the_layer_and_keeps_its_keys() {
        let mut window = Window::new(palette());
        window.frame(Vec::new());
        let field = window.ui.layer_field.center();
        // Clicking the field edits it as text; then Ctrl+Z belongs to it.
        window.click(field);
        assert!(window.context.egui_wants_keyboard_input());
        let typed = window.frame(vec![
            Event::Key {
                key: Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            },
            Event::Text("3".to_owned()),
            key(Key::Z, Modifiers::COMMAND),
        ]);
        assert!(!typed.contains(&Action::Undo), "{typed:?}");
        let entered = window.frame(vec![
            Event::Text("3".to_owned()),
            key(Key::Enter, Modifiers::NONE),
        ]);
        let all: Vec<Action> = typed.into_iter().chain(entered).collect();
        assert!(
            all.iter().any(|action| matches!(action, Action::Layer(_))),
            "{all:?}"
        );
        assert_ne!(window.ui.layer, 0);
    }

    #[test]
    fn the_consent_notice_asks_before_saving() {
        for (choice, expected) in [(0, Action::AcceptUncheckedAccess), (1, Action::Discard)] {
            let mut window = Window::new(palette());
            window.status.dirty = true;
            window.status.waiting = true;
            window.status.unchecked_access =
                Some("access control lists are not checked".to_owned());
            // The dialog settles its size and position over its first frames.
            for _ in 0..5 {
                window.frame(Vec::new());
            }
            assert_eq!(window.ui.dialog_buttons.len(), 2);
            let at = window.ui.dialog_buttons[choice].center();
            assert_eq!(window.click(at), [expected]);
        }
    }

    #[test]
    fn closing_with_unsaved_edits_asks_first() {
        for (choice, expected) in [
            (0, Action::SaveNow),
            (1, Action::DiscardAndClose),
            (2, Action::KeepEditing),
        ] {
            let mut window = Window::new(palette());
            window.status.dirty = true;
            window.status.waiting = true;
            window.status.closing = true;
            for _ in 0..5 {
                window.frame(Vec::new());
            }
            assert_eq!(window.ui.dialog_buttons.len(), 3);
            let at = window.ui.dialog_buttons[choice].center();
            assert_eq!(window.click(at), [expected]);
        }
    }

    #[test]
    fn closing_while_a_save_waits_for_consent_asks_both_at_once() {
        for (choice, expected) in [
            (0, Action::AcceptUncheckedAccess),
            (1, Action::DiscardAndClose),
            (2, Action::KeepEditing),
        ] {
            let mut window = Window::new(palette());
            window.status.dirty = true;
            window.status.waiting = true;
            window.status.unchecked_access =
                Some("access control lists are not checked".to_owned());
            window.status.closing = true;
            for _ in 0..5 {
                window.frame(Vec::new());
            }
            assert_eq!(window.ui.dialog_buttons.len(), 3);
            let at = window.ui.dialog_buttons[choice].center();
            assert_eq!(window.click(at), [expected]);
        }
    }

    #[test]
    fn releasing_a_stroke_over_the_question_answers_nothing() {
        let mut window = Window::new(palette());
        window.ui.terrain = Some(0);
        window.frame(vec![Event::PointerMoved(IN_VIEW)]);
        let begun = window.frame(vec![button(IN_VIEW, true)]);
        assert!(
            matches!(begun[..], [Action::StrokeBegin { .. }]),
            "{begun:?}"
        );
        // Closing mid-stroke asks while the button is still held.
        window.status.dirty = true;
        window.status.waiting = true;
        window.status.closing = true;
        // The question ends the stroke.
        let mut actions = Vec::new();
        for _ in 0..5 {
            actions.extend(window.frame(Vec::new()));
        }
        assert_eq!(actions, [Action::StrokeEnd]);
        // Releasing the button over "Discard and close" is not a click.
        let discard = window.ui.dialog_buttons[1].center();
        let mut actions = window.frame(vec![Event::PointerMoved(discard)]);
        actions.extend(window.frame(vec![button(discard, false)]));
        actions.extend(window.frame(Vec::new()));
        assert_eq!(actions, []);
    }

    #[test]
    fn a_long_palette_scrolls_instead_of_hiding_the_brush() {
        let many: Vec<PaletteEntry> = (0..60)
            .map(|index| PaletteEntry {
                terrain: format!("terrain{index}"),
                color: [1, 2, 3],
            })
            .collect();
        let mut window = Window::new(many);
        window.frame(Vec::new());
        // The layer field is laid out below every terrain, in the scrolled
        // area, not cut off by the panel.
        assert!(window.ui.layer_field.top() > 600.0);
        let scroll = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -5_000.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        };
        window.frame(vec![Event::PointerMoved(Pos2::new(40.0, 300.0))]);
        for _ in 0..10 {
            window.frame(vec![scroll.clone()]);
        }
        assert!(
            window.ui.layer_field.bottom() < 600.0,
            "{:?}",
            window.ui.layer_field
        );
    }
}
