// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Application composition for the bounded provisioned Linux prototype.
use crate::workspace_ui::{Control, Hit, Rect, WorkspaceUi};
use sdl3::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tilewright_editor_commands::History;
use tilewright_editor_document::{WorldDocument, terminal_field, terminal_safe};
use tilewright_editor_preview::{
    PickMask, PreparedPreview, PreviewLimits, SelectionKey, ViewSpec, loader,
};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_ui::inspector::{self, SelectionSummary};
use tilewright_editor_ui::tools::{self, ToolMode, ToolState};
use tilewright_editor_ui::{Action, Session, SessionPhase};
use tilewright_render_api::{
    BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_render_wgpu::WgpuRenderer;
use tilewright_scene::SemanticId;
use tilewright_source::SourceRoot;

const HELP: &str = "Select / Move / Terrain / Place (P) | / filters the palette | click a tile places | Delete removes selection | drag paints | [ ] brush size | B brush shape | Enter accept | Esc cancel | Z undo | Y redo | WASD pan | +/- zoom | Q quit";
const STROKE_INTERRUPTED: &str = "Brush stroke ended before release; nothing was prepared.";

type Result<T> = std::result::Result<T, String>;
type UiRows = Vec<(String, Option<Control>, bool, bool)>;
const MAXIMUM_REVIEW_BYTES: usize = 64 << 10;
const REVIEW_CONTEXT_BYTES: usize = 24;
const REVIEW_ROW_BYTES: usize = 256;
/// Separate allowance for deletion warnings, which are already bounded.
const REVIEW_WARNING_BYTES: usize = 16 << 10;
const REVIEW_WARNINGS_OMITTED: &str = "Further warnings are omitted from this review.";
const REVIEW_LIMIT_MESSAGE: &str = "Change exceeds the visible review limit. Cancel this edit.";
const NOTHING_TO_PLACE: &str = "Place is unavailable: the catalog grants no prototype to place.";
/// Sidebar row heights and the gap between rows, in logical pixels. Place
/// palette rows are taller so their thumbnails stay legible.
const TEXT_ROW_HEIGHT: u32 = 26;
const PALETTE_ROW_HEIGHT: u32 = 40;
const ROW_GAP: u32 = 2;
const NO_PALETTE_MATCH: &str = "No prototype matches the filter.";
const FILTER_PLACEHOLDER: &str = "Filter types: click or press /";
const FILTER_HINT: &str = "Enter keeps the filter / Esc clears it";
/// Seed of the deterministic uids the scripted demonstration places with.
const DEMO_UID_SEED: u64 = 0x5eed_0b1e_c7ed_0001;
/// The empty tile of the neutral sample the scripted demonstration places on.
const DEMO_PLACE_TILE: (i64, i64) = (6, 1);
/// The palette filter the scripted demonstration types, and the empty tile
/// it places the one matching prototype on.
const DEMO_FILTER: &str = "A";
const DEMO_SEARCH_TILE: (i64, i64) = (2, 1);

/// Which granted edit a scripted selection needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Capability {
    Move,
    Field,
    Delete,
}

struct SourceReview {
    lines: Vec<String>,
    complete: bool,
}

struct Application {
    session: Session,
    recovery: DurableRecovery,
    evidence: Evidence,
    view: ViewSpec,
    prepared: PreparedPreview,
    tools: ToolState,
    ui: WorkspaceUi,
    notice: Option<String>,
    sidebar_scroll: usize,
    review_scroll: usize,
    unsupported_runs: usize,
    presented_generation: Option<u64>,
    blocked: Option<String>,
    text: tilewright_render_ui::TextSystem,
    displayed_text: Option<crate::workspace_render::EditableGeometry>,
    presented_text_revision: Option<tilewright_editor_ui::text_input::TextRevision>,
    /// The exact cell mask of the frame a running brush stroke samples.
    stroke_mask: Option<PickMask>,
}

/// This command is intentionally a deployment seam, not arbitrary recovery-path
/// selection. The trusted provision helper passes fresh separate retained Linux
/// directory descriptors, whose identity/separation receipt it retains.
pub fn run(arguments: &[String]) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(
            "checked prototype editing requires Linux; other platforms remain read-only".into(),
        );
    }
    let world = descriptor(arguments, "--world-fd")?;
    let recovery = descriptor(arguments, "--recovery-fd")?;
    let evidence = descriptor(arguments, "--evidence-fd")?;
    if world == recovery || world == evidence || recovery == evidence {
        return Err("provisioned descriptor numbers must be distinct".into());
    }
    let reference = arguments.iter().any(|argument| argument == "--reference");
    let demo = arguments.iter().any(|argument| argument == "--demo-script");
    let font_path = arguments
        .iter()
        .position(|arg| arg == "--font-pack")
        .and_then(|index| arguments.get(index + 1))
        .ok_or("missing --font-pack PATH; use the trusted packaged font pack")?;
    let text = crate::font_pack::load(std::path::Path::new(font_path), &world)?;
    let document = WorldDocument::open_prototype(&world).map_err(|error| error.to_string())?;
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let view = catalog.default_view(document.world(), 1280, 800, 1);
    let prepared = tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let mut session = Session::new(History::new(document), catalog, resources);
    if demo {
        // Scripted evidence is reproducible: its placements get the same uids
        // on every run. Interactive placement uses the OS random source.
        session.set_uid_source(Box::new(tilewright_editor_ui::uid::SeededUidSource::new(
            DEMO_UID_SEED,
        )));
    }
    let recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&recovery),
        RecoveryLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let mut app = Application {
        session,
        recovery,
        evidence: Evidence::new(SourceRoot::new(&evidence))?,
        view,
        prepared,
        tools: ToolState::default(),
        ui: WorkspaceUi::new((1280, 800), (1280, 800)),
        notice: None,
        sidebar_scroll: 0,
        review_scroll: 0,
        unsupported_runs: 0,
        presented_generation: None,
        blocked: None,
        text,
        displayed_text: None,
        presented_text_revision: None,
        stroke_mask: None,
    };
    println!(
        "{}\n{}",
        HELP,
        terminal_field(app.session.catalog().presentation())
    );
    app.report();
    if reference {
        if !demo {
            return Err(
                "reference mode requires --demo-script; it is evidence, not native acceptance"
                    .into(),
            );
        }
        app.demo_all()?;
        return Ok(());
    }
    native(app, demo)
}

fn descriptor(arguments: &[String], name: &str) -> Result<PathBuf> {
    let index = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing {name}; use tools/provision-prototype.py"))?;
    let value: u32 = arguments
        .get(index + 1)
        .ok_or_else(|| format!("missing {name} value"))?
        .parse()
        .map_err(|_| format!("invalid {name}"))?;
    if value < 3 {
        return Err("provisioned descriptors must not be standard streams".into());
    }
    Ok(PathBuf::from(format!("/proc/self/fd/{value}")))
}

impl Application {
    fn rebuild(&mut self) -> Result<()> {
        let result = self.rebuild_inner();
        if let Err(reason) = &result {
            self.blocked = Some(reason.clone());
        }
        result
    }

    fn rebuild_inner(&mut self) -> Result<()> {
        self.presented_generation = None;
        self.interrupt_stroke();
        self.tools.cancel();
        self.session
            .invalidate_frame()
            .map_err(|error| error.to_string())?;
        self.view.frame_revision = self.session.generation();
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        self.prepared = tilewright_editor_preview::prepare(
            self.session.world(),
            self.session.catalog(),
            self.session.resources(),
            &self.view,
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn reflow(&mut self) -> Result<()> {
        self.presented_generation = None;
        self.interrupt_stroke();
        self.prepared = tilewright_editor_preview::prepare(
            self.session.world(),
            self.session.catalog(),
            self.session.resources(),
            &self.view,
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn action(&mut self, action: Action) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        if matches!(action, Action::Accept) && !self.source_review().complete {
            self.notice = Some(REVIEW_LIMIT_MESSAGE.into());
            return Err(REVIEW_LIMIT_MESSAGE.into());
        }
        let result = self
            .session
            .dispatch(action, &mut self.recovery)
            .map_err(|error| error.to_string());
        if let Some(error) = self.recovery.last_error() {
            self.session.set_recovery_descriptor(error.to_string());
        } else if let Some(record) = self.recovery.records().last() {
            self.session.set_recovery_descriptor(format!(
                "{} sha256={}",
                record.name,
                tilewright_editor_document::revision_hex(&record.sha256)
            ));
        }
        // Failed commands may still invalidate pending state (for example an
        // empty Undo). Always rebuild from Session's actual current World.
        let refreshed = self.rebuild();
        self.report();
        match result {
            Err(original) => Err(original),
            Ok(()) => refreshed,
        }
    }

    fn report(&self) {
        for diagnostic in &self.prepared.diagnostics {
            println!(
                "diagnostic {}: {}",
                terminal_field(diagnostic.code),
                terminal_field(&diagnostic.message)
            );
        }
        println!(
            "state={} selection={:?} history={:?}",
            terminal_field(&self.session.status().to_string()),
            self.session.selection(),
            self.session.history().depths()
        );
        if let Some(pending) = self.session.history().pending() {
            let mut remaining = 128 << 10;
            for file in pending.changed() {
                let text = format!(
                    "--- {}\n{}\n+++ {}\n{}\n",
                    file.path,
                    file.before.as_deref().unwrap_or("<absent>"),
                    file.path,
                    file.after.as_deref().unwrap_or("<absent>")
                );
                if text.len() > remaining {
                    println!("diff display truncated; plan retained");
                    break;
                }
                print!("{}", terminal_safe(&text));
                remaining -= text.len();
            }
        }
    }

    fn click(&mut self, x: u32, y: u32) -> Result<()> {
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        let key = self
            .prepared
            .pick(
                self.view.frame_revision,
                x,
                y,
                self.tools.mode() == ToolMode::Terrain,
            )
            .map_err(|error| error.to_string())?
            .ok_or("no selectable semantic hit at this pixel")?;
        self.action(Action::Select {
            generation: self.view.frame_revision,
            key,
        })
    }

    fn nudge(&mut self, delta: [i64; 2]) -> Result<()> {
        if !self.session.can_prepare() {
            return Err("accept or cancel the pending edit first".into());
        }
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an editable object first".into());
        };
        let location = self
            .session
            .world()
            .object_location(&uid)
            .ok_or("selection no longer exists")?;
        let at = self
            .session
            .world()
            .object(&uid)
            .and_then(|object| object.at)
            .ok_or("selection is not placed")?;
        let x = location
            .chunk
            .1
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[0])))
            .and_then(|value| value.checked_add(delta[0]))
            .ok_or("coordinate overflow")?;
        let y = location
            .chunk
            .2
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[1])))
            .and_then(|value| value.checked_add(delta[1]))
            .ok_or("coordinate overflow")?;
        self.action(Action::Move {
            uid,
            x,
            y,
            expected: self.session.history().document().revision(),
        })
    }

    fn field(&mut self) -> Result<()> {
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an object with a field capability first".into());
        };
        let field = self
            .session
            .catalog()
            .editable_fields(self.session.world(), &uid)
            .first()
            .map(|field| (*field).to_owned())
            .ok_or("selected object has no field capability")?;
        self.control(Control::Field(field))?;
        while self.tools.draft_text().is_some_and(|text| !text.is_empty()) {
            self.tools.backspace();
        }
        self.tools
            .input("Edited offline")
            .map_err(|error| error.to_string())?;
        self.control(Control::ApplyDraft)
    }

    fn paint(&mut self) -> Result<()> {
        let key = self
            .session
            .selection()
            .ok_or("select a cell in terrain mode first")?;
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        let action = self
            .tools
            .paint_action(
                &inspected,
                key,
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Delete the selected object through one reviewed `object.remove`.
    fn delete_selected(&mut self) -> Result<()> {
        if self.tools.stroke().is_some() {
            return Err("finish the brush stroke before deleting".into());
        }
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        let action = self
            .tools
            .remove_action(
                &inspected,
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Place mode: the tile under physical world pixel `x, y` in the exact
    /// cell mask (empty tiles included) receives one new object of the
    /// chosen prototype, through one reviewed `object.insert`.
    fn place_at(&mut self, x: u32, y: u32) -> Result<()> {
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        let cell = self
            .prepared
            .pick(self.view.frame_revision, x, y, true)
            .map_err(|error| error.to_string())?
            .ok_or("no tile under the pointer")?;
        let action = self
            .tools
            .place_action(
                self.session.catalog(),
                &cell,
                self.session.history().document().revision(),
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Enter Place mode, choosing the first palette prototype when none is
    /// chosen yet. Refused, with a plain reason, when nothing is placeable.
    fn enter_place_mode(&mut self) -> Result<()> {
        let palette = tools::object_palette(self.session.catalog());
        let Some(first) = palette.first().map(|proto| (*proto).to_owned()) else {
            return Err(NOTHING_TO_PLACE.into());
        };
        self.tools.set_mode(ToolMode::Place);
        if self.tools.placement_choice().is_none() {
            self.tools
                .choose_placement(self.session.catalog(), &first)
                .map_err(|error| error.to_string())?;
        }
        self.sidebar_scroll = 0;
        Ok(())
    }

    /// Whether the current selection is an object the catalog lets the
    /// author delete.
    fn selection_deletable(&self) -> bool {
        matches!(
            inspector::inspect(
                self.session.world(),
                self.session.catalog(),
                self.session.selection(),
                self.session.generation(),
                self.session.history().document().revision(),
            )
            .selection,
            Some(SelectionSummary::Object {
                deletable: true,
                ..
            })
        )
    }

    /// Press: select the cell under the pointer through the ordinary exact
    /// mask, then begin a stroke there and keep that frame's cell mask for
    /// the drag samples. Nothing is prepared until release.
    fn begin_stroke(&mut self, x: u32, y: u32) -> Result<()> {
        self.click(x, y)?;
        let key = self
            .session
            .selection()
            .ok_or("select a cell in terrain mode first")?;
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        self.tools
            .begin_stroke(
                &inspected,
                key,
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        match self.prepared.mask(self.view.frame_revision, true) {
            Ok(mask) => {
                self.stroke_mask = Some(mask);
                Ok(())
            }
            Err(error) => {
                self.tools.cancel_stroke();
                Err(error.to_string())
            }
        }
    }

    /// One drag sample at a physical world pixel. Samples off the map add
    /// nothing; the next hit joins the line from the last covered cell.
    fn stroke_sample(&mut self, x: u32, y: u32) -> Result<()> {
        if self.tools.stroke().is_none() {
            return Ok(());
        }
        let Some(mask) = &self.stroke_mask else {
            self.interrupt_stroke();
            return Err(STROKE_INTERRUPTED.into());
        };
        // Reflow keeps the generation, so also require the mask's own size;
        // reflow and rebuild end the stroke before either can change.
        let viewport = self.prepared.cell_scene.viewport();
        if mask.generation() != self.view.frame_revision
            || mask.size() != [viewport.width, viewport.height]
        {
            self.interrupt_stroke();
            return Err(STROKE_INTERRUPTED.into());
        }
        let Some(cell) = mask.pick(x, y).map_err(|error| error.to_string())? else {
            return Ok(());
        };
        self.tools
            .extend_stroke(&cell, self.view.frame_revision)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// Release: the whole gesture becomes one pending reviewed transaction.
    fn finish_stroke(&mut self) -> Result<()> {
        self.stroke_mask = None;
        let action = self
            .tools
            .finish_stroke(
                self.view.frame_revision,
                self.blocked.is_some() || !self.session.can_prepare(),
            )
            .map_err(|error| error.to_string())?;
        match action {
            Some(action) => self.action(action),
            None => Ok(()),
        }
    }

    /// Abandon a running stroke without preparing anything.
    fn interrupt_stroke(&mut self) {
        self.stroke_mask = None;
        if self.tools.cancel_stroke() {
            self.notice = Some(STROKE_INTERRUPTED.into());
        }
    }

    fn displayed_input_ready(&self) -> bool {
        self.presented_generation == Some(self.view.frame_revision)
            && self.presented_text_revision == self.tools.input_revision()
    }

    fn refresh_ui(&mut self) {
        let mut ui = WorkspaceUi::new(self.ui.logical_size(), self.ui.size);
        self.toolbar(&mut ui);
        self.sidebar(&mut ui);
        self.review(&mut ui);
        for widget in &mut ui.widgets {
            if widget.editable.is_none() {
                let indent = if widget.thumbnail.is_some() {
                    crate::workspace_render::thumbnail_indent(widget.layout.height)
                } else {
                    0
                };
                widget.text = crate::workspace_render::fit_text(
                    &mut self.text,
                    &widget.text,
                    widget.layout.width.saturating_sub(indent),
                    widget.layout.height,
                );
            }
        }
        self.ui = ui;
    }

    fn toolbar(&self, ui: &mut WorkspaceUi) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        for (x, width, label, control, enabled, active) in [
            (
                8,
                84,
                "Select",
                Control::Select,
                true,
                mode == ToolMode::Select,
            ),
            (98, 76, "Move", Control::Move, true, mode == ToolMode::Move),
            (
                180,
                96,
                "Terrain",
                Control::Terrain,
                true,
                mode == ToolMode::Terrain,
            ),
            (
                282,
                76,
                "Place",
                Control::Place,
                !tools::object_palette(self.session.catalog()).is_empty(),
                mode == ToolMode::Place,
            ),
            (
                364,
                76,
                "Clear",
                Control::Clear,
                self.session.selection().is_some(),
                false,
            ),
            (
                446,
                72,
                "Undo",
                Control::Undo,
                ready && self.session.history().depths().0 > 0,
                false,
            ),
            (
                524,
                72,
                "Redo",
                Control::Redo,
                ready && self.session.history().depths().1 > 0,
                false,
            ),
            (
                602,
                80,
                "Delete",
                Control::Delete,
                ready && self.selection_deletable(),
                false,
            ),
        ] {
            ui.widget(
                Rect {
                    x,
                    y: 8,
                    width,
                    height: 32,
                },
                label,
                Some(control),
                enabled,
                active,
            );
        }
        if self.unsupported_runs > 0 {
            ui.widget(
                Rect {
                    x: 692,
                    y: 8,
                    width: ui.logical_size().0.saturating_sub(702),
                    height: 32,
                },
                format!(
                    "Unsupported text: {} marks / fields read-only",
                    self.unsupported_runs
                ),
                None,
                true,
                false,
            );
        }
    }

    fn inspector_rows(&self) -> UiRows {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        let mut rows = Vec::new();
        let inspected = inspector::inspect(
            self.session.world(),
            self.session.catalog(),
            self.session.selection(),
            self.session.generation(),
            self.session.history().document().revision(),
        );
        rows.push(("Inspector".into(), None, true, false));
        rows.push(("Mouse wheel: more rows".into(), None, true, false));
        match inspected.selection {
            Some(SelectionSummary::Object {
                uid,
                prototype,
                plane,
                layer,
                at,
                movable,
                deletable,
            }) => {
                self.object_rows(
                    &mut rows,
                    &inspected,
                    &SelectionSummary::Object {
                        uid,
                        prototype,
                        plane,
                        layer,
                        at,
                        movable,
                        deletable,
                    },
                );
            }
            Some(SelectionSummary::Cell {
                plane,
                layer,
                grid,
                x,
                y,
                prototype,
            }) => {
                rows.push((format!("Cell {x},{y} / {grid}"), None, true, false));
                rows.push((format!("{plane} / layer {layer}"), None, true, false));
                rows.push((
                    format!("Value: {}", prototype.unwrap_or("<empty>")),
                    None,
                    true,
                    false,
                ));
            }
            None => rows.push((
                inspected
                    .reason
                    .unwrap_or("Select an object or terrain cell")
                    .into(),
                None,
                true,
                false,
            )),
        }
        if mode == ToolMode::Terrain {
            self.brush_rows(&mut rows);
            rows.push(("Terrain palette".into(), None, true, false));
            for choice in tools::terrain_palette(self.session.catalog()) {
                let active = self.tools.terrain_choice() == Some(&choice);
                rows.push((
                    format!("{} / {}", choice.grid, choice.prototype),
                    Some(Control::Palette {
                        grid: choice.grid,
                        prototype: choice.prototype,
                    }),
                    ready,
                    active,
                ));
            }
        }
        if mode == ToolMode::Place {
            self.place_rows(&mut rows, ready);
        }
        rows
    }

    /// The Place palette: one row per placeable prototype the filter keeps,
    /// sorted by prototype id, the chosen one marked when it is shown. The
    /// sidebar gives each row its prototype's thumbnail.
    fn place_rows(&self, rows: &mut UiRows, ready: bool) {
        let catalog = self.session.catalog();
        let total = tools::object_palette(catalog).len();
        if total == 0 {
            rows.push((NOTHING_TO_PLACE.into(), None, false, false));
            return;
        }
        let palette = self.tools.filtered_palette(catalog);
        rows.push((
            if self.tools.palette_filter().is_empty() {
                "Place palette: click a tile".into()
            } else {
                format!("Place palette: {} of {total} match", palette.len())
            },
            None,
            true,
            false,
        ));
        if palette.is_empty() {
            rows.push((NO_PALETTE_MATCH.into(), None, false, false));
        }
        for proto in palette {
            let layer = catalog
                .editing()
                .objects
                .get(proto)
                .map_or("", |grant| grant.layer.as_str());
            rows.push((
                format!("{proto} / {layer}"),
                Some(Control::PlacePalette(proto.to_owned())),
                ready,
                self.tools.placement_choice() == Some(proto),
            ));
        }
    }

    fn brush_rows(&self, rows: &mut UiRows) {
        let brush = self.tools.brush();
        rows.push((
            format!(
                "Brush: {} {}x{}",
                brush.shape.as_str(),
                brush.diameter(),
                brush.diameter()
            ),
            None,
            true,
            false,
        ));
        rows.push((
            "Brush smaller  [".into(),
            Some(Control::BrushSize(-1)),
            brush.radius > 0,
            false,
        ));
        rows.push((
            "Brush larger  ]".into(),
            Some(Control::BrushSize(1)),
            brush.radius < tilewright_editor_ui::brush::MAXIMUM_RADIUS,
            false,
        ));
        rows.push((
            format!("Use {} brush  B", brush.toggled_shape().shape.as_str()),
            Some(Control::BrushShape),
            true,
            false,
        ));
    }

    fn object_rows(
        &self,
        rows: &mut UiRows,
        inspected: &inspector::Inspector<'_>,
        summary: &SelectionSummary<'_>,
    ) {
        let SelectionSummary::Object {
            uid,
            prototype,
            plane,
            layer,
            at,
            movable,
            deletable,
        } = summary
        else {
            return;
        };
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        rows.push((format!("Object: {uid}"), None, true, false));
        rows.push((format!("Type: {prototype}"), None, true, false));
        rows.push((
            format!("Plane: {plane} / Layer: {layer}"),
            None,
            true,
            false,
        ));
        let position = at.map_or_else(
            || "Position: not placed".into(),
            |[x, y]| format!("Position  X {x}  Y {y}"),
        );
        rows.push((position, None, true, false));
        rows.push((
            if *deletable {
                "Delete object  Del".into()
            } else {
                "Delete: not allowed for this type".into()
            },
            Some(Control::Delete),
            ready && *deletable,
            false,
        ));
        rows.push((
            "Edit position (x,y)".into(),
            Some(Control::Coordinates),
            ready && *movable,
            false,
        ));
        if mode == ToolMode::Move {
            for (label, delta) in [
                ("Move left", [-1, 0]),
                ("Move right", [1, 0]),
                ("Move up", [0, -1]),
                ("Move down", [0, 1]),
            ] {
                rows.push((
                    label.into(),
                    Some(Control::Nudge(delta)),
                    ready && *movable,
                    false,
                ));
            }
        }
        for row in &inspected.rows {
            let value = row.current.map_or_else(
                || "<absent>".into(),
                |value| match value {
                    tilewright_source::FieldValue::String(text) => text.clone(),
                    tilewright_source::FieldValue::Json(_) => "Grouped values".into(),
                    _ => inspector::scalar_text(value),
                },
            );
            let supported = true;
            rows.push((
                format!("{} = {}", row.name, value),
                Some(Control::Field(row.name.into())),
                ready && row.editable && supported,
                false,
            ));
            let detail = if supported {
                field_description(row)
            } else {
                "Read-only: unsupported text".into()
            };
            rows.push((detail, None, row.editable, false));
            let choices = tools::field_choices(inspected, row.name);
            if !choices.is_empty() {
                rows.push((
                    format!("Choices: {}", choices.join(", ")),
                    None,
                    true,
                    false,
                ));
            }
        }
        if inspected.truncated {
            rows.push((
                "More fields are unavailable in this view".into(),
                None,
                false,
                false,
            ));
        }
    }

    fn sidebar(&mut self, ui: &mut WorkspaceUi) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        let rows = self.inspector_rows();
        let mut y = 56;
        if self.tools.mode() == ToolMode::Place
            && !tools::object_palette(self.session.catalog()).is_empty()
        {
            y = self.filter_field(ui, y);
        }
        if let Some(snapshot) = self
            .tools
            .draft_snapshot()
            .filter(|_| !self.tools.filter_focused())
        {
            let supported = self
                .text
                .layout(&snapshot.display_text, 18.0, None)
                .is_ok_and(|layout| layout.missing_glyphs().is_empty());
            ui.editable(
                Rect {
                    x,
                    y,
                    width,
                    height: 30,
                },
                snapshot,
            );
            y += 34;
            ui.widget(
                Rect {
                    x,
                    y,
                    width,
                    height: 22,
                },
                if supported {
                    "Ctrl+A selects / Enter reviews"
                } else {
                    "Unsupported text; cannot review"
                },
                None,
                supported,
                false,
            );
            y += 26;
            ui.widget(
                Rect {
                    x,
                    y,
                    width: width / 2 - 3,
                    height: 28,
                },
                "Review",
                Some(Control::ApplyDraft),
                ready && supported,
                false,
            );
            ui.widget(
                Rect {
                    x: x + width / 2 + 3,
                    y,
                    width: width / 2 - 3,
                    height: 28,
                },
                "Discard",
                Some(Control::CancelDraft),
                true,
                false,
            );
            y += 36;
        }
        self.sidebar_rows(ui, rows, y);
    }

    /// The scrolled sidebar rows from logical `y` down. Text rows are
    /// 26 px and Place palette rows, which carry their prototype's
    /// thumbnail, are 40 px, each with a 2 px gap; only rows that fit whole,
    /// gap included, are laid out. The scroll offset counts rows and stops where the last
    /// row is visible.
    fn sidebar_rows(&mut self, ui: &mut WorkspaceUi, rows: UiRows, mut y: u32) {
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        let bottom = ui.logical_height();
        let rows: Vec<_> = rows
            .into_iter()
            .map(|(text, control, enabled, active)| {
                let thumbnail = match &control {
                    Some(Control::PlacePalette(proto)) => self
                        .session
                        .resources()
                        .prototype_texture(self.session.catalog(), proto),
                    _ => None,
                };
                (text, control, enabled, active, thumbnail)
            })
            .collect();
        let height = |row: &(String, Option<Control>, bool, bool, Option<_>)| {
            if row.4.is_some() {
                PALETTE_ROW_HEIGHT
            } else {
                TEXT_ROW_HEIGHT
            }
        };
        // The furthest scroll that still fills the panel from the last row up.
        let available = bottom.saturating_sub(y);
        let mut used = 0_u32;
        let mut furthest = rows.len();
        while furthest > 0 {
            let pitch = height(&rows[furthest - 1]) + ROW_GAP;
            if used + pitch > available {
                break;
            }
            used += pitch;
            furthest -= 1;
        }
        self.sidebar_scroll = self.sidebar_scroll.min(furthest);
        for row in rows.into_iter().skip(self.sidebar_scroll) {
            let row_height = height(&row);
            // Like the furthest scroll, a row needs its gap below it too.
            if y + row_height + ROW_GAP > bottom {
                break;
            }
            let (text, control, enabled, active, thumbnail) = row;
            let before = ui.widgets.len();
            ui.widget(
                Rect {
                    x,
                    y,
                    width,
                    height: row_height,
                },
                text,
                control,
                enabled,
                active,
            );
            if ui.widgets.len() > before
                && let Some(widget) = ui.widgets.last_mut()
            {
                widget.thumbnail = thumbnail;
            }
            y += row_height + ROW_GAP;
        }
    }

    /// The Place palette filter, fixed at the top of the sidebar above the
    /// scrolled rows: the focused text field with a hint, or a plain row
    /// showing the filter that focuses it. Returns the next free row.
    fn filter_field(&self, ui: &mut WorkspaceUi, mut y: u32) -> u32 {
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        if let Some(snapshot) = self
            .tools
            .draft_snapshot()
            .filter(|_| self.tools.filter_focused())
        {
            ui.editable(
                Rect {
                    x,
                    y,
                    width,
                    height: 30,
                },
                snapshot,
            );
            y += 34;
            ui.widget(
                Rect {
                    x,
                    y,
                    width,
                    height: 22,
                },
                FILTER_HINT,
                None,
                true,
                false,
            );
            return y + 26;
        }
        let filter = self.tools.palette_filter();
        ui.widget(
            Rect {
                x,
                y,
                width,
                height: 30,
            },
            if filter.is_empty() {
                FILTER_PLACEHOLDER.into()
            } else {
                format!("Filter: {filter}")
            },
            Some(Control::PaletteFilter),
            true,
            !filter.is_empty(),
        );
        y + 34
    }

    fn review(&mut self, ui: &mut WorkspaceUi) {
        let logical = ui.logical_size();
        let pending = self.session.history().pending().is_some()
            && self.session.status().phase != SessionPhase::Blocked;
        let y = ui.review_y();
        ui.widget(
            Rect {
                x: 10,
                y: y + 6,
                width: 100,
                height: 28,
            },
            "Accept",
            Some(Control::Accept),
            pending && self.blocked.is_none() && self.source_review().complete,
            false,
        );
        ui.widget(
            Rect {
                x: 118,
                y: y + 6,
                width: 100,
                height: 28,
            },
            "Cancel",
            Some(Control::Cancel),
            pending && self.blocked.is_none(),
            false,
        );
        let status = self.status_label();
        ui.widget(
            Rect {
                x: 10,
                y: y + 38,
                width: logical.0.saturating_sub(320.min(logical.0 / 2) + 20),
                height: 22,
            },
            status,
            None,
            true,
            false,
        );
        let diff = self.review_lines(logical.0.saturating_sub(320.min(logical.0 / 2) + 20));
        let lines = usize::try_from(logical.1.saturating_sub(y + 64) / 20).unwrap_or(0);
        self.review_scroll = self.review_scroll.min(diff.len().saturating_sub(lines));
        for (index, line) in diff
            .into_iter()
            .skip(self.review_scroll)
            .take(lines)
            .enumerate()
        {
            ui.widget(
                Rect {
                    x: 10,
                    y: y + 64 + u32::try_from(index).unwrap_or(0) * 20,
                    width: logical.0.saturating_sub(320.min(logical.0 / 2) + 20),
                    height: 20,
                },
                line,
                None,
                true,
                false,
            );
        }
    }

    fn status_label(&self) -> String {
        if self.blocked.is_some() || self.session.status().phase == SessionPhase::Blocked {
            return "Editing blocked. Recovery details below.".into();
        }
        if let Some(stroke) = self.tools.stroke() {
            return format!(
                "Painting {} cells with {}; release to review.{}",
                stroke.len(),
                stroke.prototype(),
                if stroke.capped() {
                    " Brush limit reached."
                } else {
                    ""
                }
            );
        }
        if self.notice.is_some() {
            return "Input not applied. Details below.".into();
        }
        if self.session.status().phase == SessionPhase::Pending && !self.source_review().complete {
            return REVIEW_LIMIT_MESSAGE.into();
        }
        match self.session.status().phase {
            SessionPhase::Ready if self.session.status().applied == Some(true) => {
                "Changes saved. Ready to edit."
            }
            SessionPhase::Ready if self.tools.mode() == ToolMode::Place => {
                return self.tools.placement_choice().map_or_else(
                    || "Choose a type to place in the sidebar.".into(),
                    |proto| {
                        if self
                            .tools
                            .filtered_palette(self.session.catalog())
                            .contains(&proto)
                        {
                            format!("Ready. Click a tile to place {proto}.")
                        } else {
                            format!("Ready. Click a tile to place {proto} (hidden by the filter).")
                        }
                    },
                );
            }
            SessionPhase::Ready => "Ready. Select an object or choose Terrain.",
            SessionPhase::Pending => "Review changes, then Accept or Cancel.",
            SessionPhase::Retaining => "Saving recovery information...",
            SessionPhase::Publishing => "Saving reviewed changes...",
            SessionPhase::Blocked => "Editing blocked. Recovery details below.",
        }
        .into()
    }

    fn review_lines(&mut self, width: u32) -> Vec<String> {
        let mut details = Vec::new();
        if let Some(reason) = &self.blocked {
            details.push(format!("Rendering blocked: {reason}"));
        }
        if let Some(reason) = &self.notice {
            details.push(reason.clone());
        }
        let status = self.session.status();
        if status.phase == SessionPhase::Blocked {
            details.push(format!("Reason: {}", status.message));
            for (label, value) in [
                ("Source changed", status.applied),
                ("Durable on disk", status.durable),
                ("Cleanup finished", status.cleaned),
            ] {
                details.push(format!(
                    "{label}: {}",
                    value.map_or("not reported", |value| if value { "yes" } else { "no" })
                ));
            }
            if let Some(record) = &status.recovery_descriptor {
                details.push(format!("Recovery record: {record}"));
            }
            details.push("Keep the recovery record. Inspect source before reopening.".into());
        }
        details.extend(self.diff_lines());
        details
            .into_iter()
            .flat_map(|line| crate::workspace_render::wrap_text(&mut self.text, &line, width, 20))
            .collect()
    }

    fn diff_lines(&self) -> Vec<String> {
        self.source_review().lines
    }

    fn source_review(&self) -> SourceReview {
        let Some(plan) = self.session.history().pending() else {
            return SourceReview {
                lines: vec!["No pending changes. Edits are reviewed before saving.".into()],
                complete: true,
            };
        };
        let mut review = SourceReview {
            lines: Vec::new(),
            complete: true,
        };
        let mut remaining = MAXIMUM_REVIEW_BYTES;
        // Deletion warnings come first, with their own allowance outside the
        // diff budget: they inform the author and never block Accept.
        let mut allowance = REVIEW_WARNING_BYTES;
        for warning in self.session.pending_warnings() {
            let warning = terminal_field(warning);
            if warning.len() > allowance {
                review.lines.push(REVIEW_WARNINGS_OMITTED.into());
                break;
            }
            allowance -= warning.len();
            review.lines.push(warning);
        }
        for (index, file) in plan.changed().enumerate() {
            if index >= 32 {
                review.complete = false;
                break;
            }
            review
                .lines
                .push(format!("{} / source excerpt; scroll for more", file.path));
            if !append_change_review(
                &mut review.lines,
                file.before.as_deref().unwrap_or("<absent>"),
                file.after.as_deref().unwrap_or("<absent>"),
                &mut remaining,
            ) {
                review.complete = false;
                break;
            }
        }
        if !review.complete {
            review.lines.insert(0, REVIEW_LIMIT_MESSAGE.into());
        }
        review
    }

    fn control(&mut self, control: Control) -> Result<()> {
        self.presented_generation = None;
        self.notice = None;
        match control {
            Control::Select | Control::Move | Control::Terrain => {
                self.tools.set_mode(match control {
                    Control::Move => ToolMode::Move,
                    Control::Terrain => ToolMode::Terrain,
                    _ => ToolMode::Select,
                });
                self.sidebar_scroll = 0;
                Ok(())
            }
            Control::Place => self.enter_place_mode(),
            Control::PaletteFilter => self.tools.focus_filter().map_err(|error| error.to_string()),
            // Enter in the focused filter keeps its text and releases focus;
            // the filter is never reviewed.
            Control::ApplyDraft if self.tools.filter_focused() => {
                self.tools.release_filter();
                Ok(())
            }
            Control::PlacePalette(prototype) => self
                .tools
                .choose_placement(self.session.catalog(), &prototype)
                .map_err(|error| error.to_string()),
            Control::Clear => self.action(Action::ClearSelection),
            Control::Delete => self.delete_selected(),
            Control::Accept => self.action(Action::Accept),
            Control::Cancel => self.action(Action::Cancel),
            Control::Undo => self.action(Action::Undo),
            Control::Redo => self.action(Action::Redo),
            Control::Nudge(delta) => self.nudge(delta),
            Control::BrushSize(delta) => {
                self.tools
                    .set_brush(self.tools.brush().resized(i32::from(delta)));
                Ok(())
            }
            Control::BrushShape => {
                self.tools.set_brush(self.tools.brush().toggled_shape());
                Ok(())
            }
            Control::Palette { grid, prototype } => {
                self.tools
                    .choose_terrain(self.session.catalog(), &grid, &prototype)
                    .map_err(|error| error.to_string())?;
                if self.view.grid.as_deref() != Some(grid.as_str()) {
                    self.view.grid = Some(grid);
                    self.action(Action::ClearSelection)?;
                }
                Ok(())
            }
            Control::EditDraft => Ok(()),
            Control::CancelDraft => {
                self.tools.cancel();
                Ok(())
            }
            Control::Coordinates | Control::Field(_) | Control::ApplyDraft => {
                let inspected = inspector::inspect(
                    self.session.world(),
                    self.session.catalog(),
                    self.session.selection(),
                    self.session.generation(),
                    self.session.history().document().revision(),
                );
                let unavailable = self.blocked.is_some() || !self.session.can_prepare();
                match control {
                    Control::Coordinates => self
                        .tools
                        .begin_move(&inspected, unavailable)
                        .map_err(|error| error.to_string()),
                    Control::Field(field) => self
                        .tools
                        .begin_field(&inspected, &field, unavailable)
                        .map_err(|error| error.to_string()),
                    _ => {
                        let snapshot = self.tools.draft_snapshot().ok_or("no draft")?;
                        let layout = self
                            .text
                            .layout(&snapshot.display_text, 18.0, None)
                            .map_err(|error| error.to_string())?;
                        if !layout.missing_glyphs().is_empty() {
                            return Err(
                                "missing font glyphs: review blocked; draft preserved".into()
                            );
                        }
                        let action = self
                            .tools
                            .build_action(&inspected, unavailable)
                            .map_err(|error| error.to_string())?;
                        self.action(action)
                    }
                }
            }
        }
    }

    fn frame(&self, cells: bool) -> Result<FrameOutput> {
        let mut renderer = ReferenceRenderer::new(offscreen_target(
            self.view.camera.width,
            self.view.camera.height,
        ))
        .map_err(|error| error.to_string())?;
        renderer
            .render(RenderRequest {
                scene: if cells {
                    &self.prepared.cell_scene
                } else {
                    &self.prepared.scene
                },
                resources: self.prepared.resources.clone(),
            })
            .map_err(|error| error.to_string())
    }

    fn workspace_frame(&mut self) -> Result<FrameOutput> {
        self.refresh_ui();
        let presentation = workspace_presentation(self)?;
        let mut renderer = ReferenceRenderer::new(offscreen_target(
            self.view.camera.width,
            self.view.camera.height,
        ))
        .map_err(|error| error.to_string())?;
        renderer
            .render(RenderRequest {
                scene: &presentation.scene,
                resources: presentation.resources,
            })
            .map_err(|error| error.to_string())
    }

    fn capture(&mut self, name: &str) -> Result<()> {
        // CPU evidence includes the exact shared-renderer workspace composition;
        // the separate World-only frame() remains authoritative for semantic picks.
        let frame = self.workspace_frame()?;
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|error| error.to_string())?
                .write_image_data(&frame.rgba8)
                .map_err(|error| error.to_string())?;
        }
        self.evidence.retain(&format!("{name}.png"), &bytes)?;
        self.evidence.retain(&format!("{name}.json"), serde_json::to_string_pretty(&serde_json::json!({
            "format":"tilewright.prototype-frame/1", "path":"cpu-reference", "frame_generation":self.view.frame_revision,
            "source_revision":tilewright_editor_document::revision_hex(&self.session.world().source_digest()),
            "committed_revision":tilewright_editor_document::revision_hex(&self.session.history().document().revision()),
            "catalog_revision":tilewright_editor_document::revision_hex(&self.prepared.catalog_revision),
            "width":frame.width,"height":frame.height,"status":self.session.status().to_string(),
            "toolkit":"0.2.0","renderer":"0.7.0","pending":self.session.history().pending().is_some()
        })).map_err(|error| error.to_string())?.as_bytes())
    }

    /// Select a visible object with `capability` through the ordinary mask.
    /// Move and field edits take the first such object; deletion takes the
    /// last, so a scripted deletion leaves the earlier edited object alone.
    fn select_visible_object(&mut self, capability: Capability) -> Result<()> {
        let frame = self.frame(false)?;
        let eligible = |(id, key): (&SemanticId, &SelectionKey)| {
            let SelectionKey::Object { uid } = key else {
                return None;
            };
            let (world, catalog) = (self.session.world(), self.session.catalog());
            let granted = match capability {
                Capability::Move => catalog.can_move(world, uid),
                Capability::Field => !catalog.editable_fields(world, uid).is_empty(),
                Capability::Delete => catalog.can_delete(world, uid),
            };
            if !granted {
                return None;
            }
            pixel_for(&frame, *id)
        };
        let hit = if capability == Capability::Delete {
            self.prepared.picks.iter().rev().find_map(eligible)
        } else {
            self.prepared.picks.iter().find_map(eligible)
        }
        .ok_or("no visible object with the requested edit capability in reference mask")?;
        self.tools.set_mode(ToolMode::Select);
        self.click(hit.0, hit.1)
    }

    fn select_empty_cell(&mut self) -> Result<()> {
        let frame = self.frame(true)?;
        // The authored neutral sample explicitly leaves (6,4) empty. The
        // ordinary mask path must find and select that cell before painting.
        let hit = self
            .prepared
            .cell_picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Cell { x: 6, y: 4, .. } => pixel_for(&frame, *id),
                _ => None,
            })
            .ok_or("neutral empty cell is not selectable")?;
        self.tools.set_mode(ToolMode::Terrain);
        self.click(hit.0, hit.1)
    }

    /// Enter Place mode, choose the last palette prototype through its row
    /// control, and click the neutral sample's empty demonstration tile
    /// through the exact cell mask.
    fn place_demo(&mut self) -> Result<()> {
        self.control(Control::Place)?;
        let proto = tools::object_palette(self.session.catalog())
            .last()
            .map(|proto| (*proto).to_owned())
            .ok_or(NOTHING_TO_PLACE)?;
        self.control(Control::PlacePalette(proto))?;
        self.place_on_tile(DEMO_PLACE_TILE)
    }

    /// Palette search: focus the filter, type the demonstration filter,
    /// capture the narrowed palette with its thumbnails, then choose the
    /// first matching row and place it on the search demonstration tile.
    fn search_demo(&mut self) -> Result<()> {
        self.control(Control::Place)?;
        self.control(Control::PaletteFilter)?;
        self.tools
            .input(DEMO_FILTER)
            .map_err(|error| error.to_string())?;
        self.capture("palette")?;
        let proto = self
            .tools
            .filtered_palette(self.session.catalog())
            .first()
            .map(|proto| (*proto).to_owned())
            .ok_or("the demonstration filter matches no prototype")?;
        self.control(Control::ApplyDraft)?;
        self.control(Control::PlacePalette(proto))?;
        self.place_on_tile(DEMO_SEARCH_TILE)
    }

    /// Click the empty neutral tile `(x, y)` in Place mode through the
    /// exact cell mask.
    fn place_on_tile(&mut self, (x, y): (i64, i64)) -> Result<()> {
        let frame = self.frame(true)?;
        let hit = self
            .prepared
            .cell_picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Cell { x: cx, y: cy, .. } if (*cx, *cy) == (x, y) => {
                    pixel_for(&frame, *id)
                }
                _ => None,
            })
            .ok_or("neutral demonstration tile is not in the cell mask")?;
        self.place_at(hit.0, hit.1)
    }

    fn demo_step(&mut self, step: usize) -> Result<bool> {
        match step {
            0 => {
                self.capture("before")?;
                self.select_visible_object(Capability::Move)?;
            }
            1 => {
                self.tools.set_mode(ToolMode::Move);
                self.nudge([1, 0])?;
                self.capture("pending")?;
            }
            2 => {
                self.action(Action::Cancel)?;
                self.capture("cancel")?;
                self.nudge([1, 0])?;
                self.action(Action::Accept)?;
                self.capture("after")?;
            }
            3 => {
                self.action(Action::Undo)?;
                self.action(Action::Accept)?;
                self.capture("undo")?;
            }
            4 => {
                self.action(Action::Redo)?;
                self.action(Action::Accept)?;
                self.capture("redo")?;
            }
            5 if grants_fields(self.session.catalog().editing()) => {
                self.select_visible_object(Capability::Field)?;
                self.field()?;
                self.action(Action::Accept)?;
                self.capture("field")?;
            }
            6 if !self.session.catalog().editing().terrain.is_empty() => {
                self.select_empty_cell()?;
                let choice = tools::terrain_palette(self.session.catalog())
                    .into_iter()
                    .rfind(|choice| match self.session.selection() {
                        Some(SelectionKey::Cell { grid, .. }) => *grid == choice.grid,
                        _ => false,
                    })
                    .ok_or("empty demo terrain palette")?;
                self.tools
                    .choose_terrain(self.session.catalog(), &choice.grid, &choice.prototype)
                    .map_err(|error| error.to_string())?;
                self.paint()?;
                self.action(Action::Accept)?;
                self.capture("terrain")?;
            }
            7 if !self.session.catalog().editing().objects.is_empty() => {
                self.select_visible_object(Capability::Delete)?;
                self.delete_selected()?;
                self.capture("delete")?;
                self.action(Action::Accept)?;
                self.capture("deleted")?;
            }
            8 if !tools::object_palette(self.session.catalog()).is_empty() => {
                self.place_demo()?;
                self.capture("place")?;
                self.action(Action::Accept)?;
                self.capture("placed")?;
            }
            9 if !tools::object_palette(self.session.catalog()).is_empty() => {
                self.search_demo()?;
                self.capture("search-place")?;
                self.action(Action::Accept)?;
                self.capture("search-placed")?;
            }
            10 => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn demo_all(&mut self) -> Result<()> {
        for step in 0..11 {
            if self.demo_step(step)? {
                break;
            }
        }
        Ok(())
    }
}

/// Read-only display ranges from exact prepared images; no source parsing or writing.
fn changed_context(before: &str, after: &str) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    let context = |text: &str| {
        let mut start = prefix.saturating_sub(REVIEW_CONTEXT_BYTES);
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let mut end = (text.len() - suffix)
            .saturating_add(REVIEW_CONTEXT_BYTES)
            .min(text.len());
        while !text.is_char_boundary(end) {
            end += 1;
        }
        start..end
    };
    (context(before), context(after))
}

fn review_chunks(text: &str, range: std::ops::Range<usize>) -> Vec<String> {
    let escaped = terminal_field(&text[range.clone()]);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < escaped.len() {
        let mut end = (start + REVIEW_ROW_BYTES).min(escaped.len());
        while !escaped.is_char_boundary(end) {
            end -= 1;
        }
        let prefix = if start == 0 && range.start > 0 {
            "[...] "
        } else {
            ""
        };
        let suffix = if end == escaped.len() && range.end < text.len() {
            " [...]"
        } else {
            ""
        };
        rows.push(format!("{prefix}{}{suffix}", &escaped[start..end]));
        start = end;
    }
    if rows.is_empty() {
        rows.push("<empty>".into());
    }
    rows
}

fn append_change_review(
    lines: &mut Vec<String>,
    before: &str,
    after: &str,
    remaining: &mut usize,
) -> bool {
    let (old, new) = changed_context(before, after);
    let bytes = old.len().saturating_add(new.len());
    if bytes > *remaining {
        return false;
    }
    let old_rows = review_chunks(before, old);
    let new_rows = review_chunks(after, new);
    let displayed = old_rows
        .iter()
        .chain(&new_rows)
        .map(String::len)
        .sum::<usize>();
    if displayed > *remaining {
        return false;
    }
    *remaining -= displayed;
    lines.push("[...] marks unchanged source omitted from this excerpt.".into());
    for index in 0..old_rows.len().max(new_rows.len()) {
        if let Some(row) = old_rows.get(index) {
            lines.push(format!("- {row}"));
        }
        if let Some(row) = new_rows.get(index) {
            lines.push(format!("+ {row}"));
        }
    }
    true
}

fn field_description(row: &inspector::FieldRow<'_>) -> String {
    use tilewright_source::FieldType;
    if let Some(reason) = row.reason {
        return match reason {
            "Structured field is read-only" => "Read-only: grouped fields".into(),
            "Required field is absent; source must be repaired" => "Required value missing".into(),
            "Field has no editing capability" => "Read-only: editing not allowed".into(),
            "Field choices exceed input budget" => "Read-only: too many choices".into(),
            "Current value exceeds input budget" => "Read-only: value too long".into(),
            "Optional field is absent; enter a value to add it" => {
                "Optional: enter a value to add".into()
            }
            other => other.into(),
        };
    }
    match &row.spec.field_type {
        FieldType::Boolean => "Boolean: true or false".into(),
        FieldType::Integer { minimum, maximum } => format!("Integer ({minimum} to {maximum})"),
        FieldType::Number { minimum, maximum } => format!("Number ({minimum} to {maximum})"),
        FieldType::String { maximum_length, .. } => {
            format!("Text (max {maximum_length} characters)")
        }
        FieldType::Reference { target } => format!("Reference: {target}"),
        FieldType::Json => "Read-only: JSON value".into(),
        FieldType::List { .. } => "Read-only: list of values".into(),
        FieldType::Object { .. } => "Read-only: grouped fields".into(),
    }
}

fn pixel_for(frame: &FrameOutput, id: SemanticId) -> Option<(u32, u32)> {
    let index = u64::try_from(frame.semantic_ids.iter().position(|value| *value == id.0)?).ok()?;
    Some((
        u32::try_from(index % u64::from(frame.width)).ok()?,
        u32::try_from(index / u64::from(frame.width)).ok()?,
    ))
}

struct Evidence {
    root: SourceRoot,
    _lock: tilewright_source::SourceLock,
}
impl Evidence {
    fn new(root: SourceRoot) -> Result<Self> {
        let lock = root.lock().map_err(|error| error.to_string())?;
        Ok(Self { root, _lock: lock })
    }
    fn retain(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if self.root.exists(name).map_err(|error| error.to_string())? {
            return Err(format!("evidence output already exists: {name}"));
        }
        let directory = self
            .root
            .directory("", false)
            .map_err(|error| error.to_string())?
            .ok_or("evidence root missing")?;
        let access = directory
            .own_access()
            .map_err(|error| error.to_string())?
            .new_file();
        directory
            .write(name, Some(&access), bytes)
            .map_err(|error| error.to_string())?;
        if !directory.sync().map_err(|error| error.to_string())? {
            return Err("evidence directory sync unavailable".into());
        }
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
fn native(mut app: Application, demo: bool) -> Result<()> {
    crate::native_input::configure_ime_ui()?;
    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let mut window = video
        .window("Tilewright offline prototype", 1280, 800)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    window
        .set_minimum_size(720, 480)
        .map_err(|error| error.to_string())?;
    let (width, height) = window.size_in_pixels();
    app.ui = WorkspaceUi::new(window.size(), (width, height));
    app.view.camera.width = width;
    app.view.camera.height = height;
    app.rebuild()?;
    let mut renderer = WgpuRenderer::new(TargetDescriptor {
        kind: TargetKind::Window,
        width,
        height,
        backend: BackendPreference::Automatic,
    })
    .map_err(|error| error.to_string())?;
    println!("native adapter={:?}", renderer.adapter());
    let mut events = sdl.event_pump().map_err(|error| error.to_string())?;
    let mut demo_step = 0;
    let mut next_demo = Instant::now() + Duration::from_millis(800);
    let mut dirty = true;
    let mut phase = None;
    let mut minimized = false;
    let mut input = crate::native_input::NativeInput::new(window.id());
    'running: loop {
        let batch: Vec<Event> = events.poll_iter().collect();
        input.begin_batch(batch.iter().any(|event| matches!(event,
            Event::TextInput { window_id, .. } | Event::TextEditing { window_id, .. } if *window_id == input.window_id)), app.tools.input_revision().map(|revision| revision.draft));
        for event in batch {
            if event
                .get_window_id()
                .is_some_and(|id| id != input.window_id)
            {
                continue;
            }
            if matches!(
                event,
                Event::Quit { .. }
                    | Event::Window {
                        win_event: WindowEvent::CloseRequested,
                        ..
                    }
            ) {
                break 'running;
            }
            if matches!(
                event,
                Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    ..
                }
            ) && app.tools.draft_text().is_none()
            {
                break 'running;
            }
            if matches!(
                event,
                Event::Window {
                    win_event: WindowEvent::FocusLost | WindowEvent::Minimized,
                    ..
                }
            ) && app.tools.stroke().is_some()
            {
                app.interrupt_stroke();
                dirty = true;
            }
            let stroke_before = app.tools.stroke().map(|s| (s.len(), s.capped()));
            let scroll_before = (app.sidebar_scroll, app.review_scroll);
            let composing = app.tools.is_composing();
            let draft_before = app.tools.input_revision().map(|revision| revision.draft);
            let refocus = if let Event::MouseButtonDown { x, y, .. } = event {
                crate::viewport::physical_pixel([x, y], window.size(), window.size_in_pixels())
                    .is_some_and(|(x, y)| app.ui.hit(x, y) == Hit::Control(Control::EditDraft))
            } else {
                false
            };
            let result = native_clipboard_event(
                &mut app,
                &event,
                input.focused,
                || {
                    video
                        .clipboard()
                        .clipboard_text()
                        .map_err(|error| error.to_string())
                },
                |text| {
                    video
                        .clipboard()
                        .set_clipboard_text(text)
                        .map_err(|error| error.to_string())
                },
            )
            .or_else(|| native_text_event(&mut app, &event, &mut input))
            .unwrap_or_else(|| {
                native_event(
                    &mut app,
                    &mut renderer,
                    &window,
                    &event,
                    &mut minimized,
                    input.shift,
                )
            });
            if composing
                && !app.tools.is_composing()
                && !matches!(event, Event::TextInput { .. } | Event::TextEditing { .. })
            {
                input.cancel_composition();
                crate::native_input::clear_composition(&window)?;
            }
            let draft_after = app.tools.input_revision().map(|revision| revision.draft);
            if draft_before != draft_after {
                crate::native_input::clear_composition(&window)?;
            }
            if result.is_ok() && (refocus || (draft_before != draft_after && draft_after.is_some()))
            {
                input.refocus(event.get_timestamp());
            }
            let should_start = input.focused && !minimized && app.tools.draft_text().is_some();
            if should_start != input.started {
                if should_start {
                    video
                        .text_input()
                        .start_with_options(
                            &window,
                            sdl3::keyboard::TextInputOptions {
                                multiline: Some(false),
                                ..Default::default()
                            },
                        )
                        .map_err(|error| error.to_string())?;
                } else {
                    crate::native_input::clear_composition(&window)?;
                    video.text_input().stop(&window);
                }
                input.started = should_start;
            }
            dirty |= update_input_presentation(&mut app, scroll_before)
                || result.is_err()
                || stroke_before != app.tools.stroke().map(|s| (s.len(), s.capped()));
            if let Err(error) = result {
                eprintln!("{}", terminal_field(&error));
                app.notice = Some(error);
            }
        }
        if demo && !minimized && Instant::now() >= next_demo {
            if app.demo_step(demo_step)? {
                break 'running;
            }
            phase = demo_phase_label(&app, demo_step);
            demo_step += 1;
            next_demo = Instant::now() + Duration::from_millis(2500);
            dirty = true;
        }
        if dirty && !minimized {
            if native_present(&mut app, &mut renderer, &mut window)?
                && let Some(name) = phase.take()
            {
                println!("native-phase={name}");
            }
            if input.started
                && let Some(geometry) = &app.displayed_text
            {
                update_ime_area(&video, &window, geometry);
            }
            dirty = false;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    if input.started {
        video.text_input().stop(&window);
    }
    pollster::block_on(renderer.finish()).map_err(|error| error.to_string())?;
    println!("native window closed cleanly");
    Ok(())
}

/// Whether any field edit is granted, per uid or per prototype.
fn grants_fields(editing: &tilewright_editor_preview::EditingCapabilities) -> bool {
    !editing.fields.is_empty()
        || editing
            .objects
            .values()
            .any(|grant| !grant.fields.is_empty())
}

/// Labels are logged after successful presentation; this does not own native handles.
fn demo_phase_label(app: &Application, step: usize) -> Option<&'static str> {
    let editing = app.session.catalog().editing();
    if (step == 5 && !grants_fields(editing))
        || (step == 6 && editing.terrain.is_empty())
        || ((7..=9).contains(&step) && editing.objects.is_empty())
    {
        None
    } else {
        [
            "before", "pending", "after", "undo", "redo", "field", "terrain", "delete", "place",
            "palette",
        ]
        .get(step)
        .copied()
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn update_ime_area(
    video: &sdl3::VideoSubsystem,
    window: &sdl3::video::Window,
    geometry: &crate::workspace_render::EditableGeometry,
) {
    if let Some(caret) = geometry
        .layout
        .caret(crate::workspace_render::renderer_cursor(
            geometry.snapshot.display_selection.focus,
        ))
    {
        // Shared layout and clip are logical window coordinates, never DPI pixels.
        let area = sdl3::rect::Rect::new(
            geometry.clip.x as i32,
            geometry.clip.y as i32,
            geometry.clip.width as u32,
            geometry.clip.height as u32,
        );
        let cursor =
            (geometry.origin[0] + caret.x - geometry.clip.x).clamp(0.0, geometry.clip.width) as i32;
        video.text_input().set_rect(window, area, cursor);
    }
}

fn native_clipboard_event(
    app: &mut Application,
    event: &Event,
    focused: bool,
    read: impl FnOnce() -> Result<String>,
    write: impl FnOnce(&str) -> Result<()>,
) -> Option<Result<()>> {
    let Event::KeyDown {
        keycode: Some(key),
        keymod,
        repeat: false,
        ..
    } = event
    else {
        return None;
    };
    if !focused
        || app.tools.draft_text().is_none()
        || app.tools.is_composing()
        || !keymod.intersects(
            sdl3::keyboard::Mod::LCTRLMOD
                | sdl3::keyboard::Mod::RCTRLMOD
                | sdl3::keyboard::Mod::LGUIMOD
                | sdl3::keyboard::Mod::RGUIMOD,
        )
    {
        return None;
    }
    if !matches!(key, Keycode::C | Keycode::X | Keycode::V) {
        return None;
    }
    Some((|| {
        if *key == Keycode::V {
            let text = read()?;
            app.tools.input(&text).map_err(|error| error.to_string())
        } else {
            let snapshot = app.tools.draft_snapshot().ok_or("no draft")?;
            let range = snapshot.selection.range();
            if range.is_empty() {
                return Ok(());
            }
            write(&snapshot.committed[range])?;
            if *key == Keycode::X {
                app.tools.input("").map_err(|error| error.to_string())?;
            }
            Ok(())
        }
    })())
}

/// Testable routing uses real SDL event values without initializing SDL/GPU.
#[allow(clippy::too_many_lines)]
fn native_text_event(
    app: &mut Application,
    event: &Event,
    input: &mut crate::native_input::NativeInput,
) -> Option<Result<()>> {
    if event
        .get_window_id()
        .is_some_and(|id| id != input.window_id)
    {
        return Some(Ok(()));
    }
    if let Event::KeyDown { keymod, .. } | Event::KeyUp { keymod, .. } = event {
        input.shift =
            keymod.intersects(sdl3::keyboard::Mod::LSHIFTMOD | sdl3::keyboard::Mod::RSHIFTMOD);
    }
    match event {
        Event::Window {
            win_event: WindowEvent::FocusLost | WindowEvent::Minimized,
            ..
        } => {
            input.lose_focus();
            let result = if app.tools.draft_text().is_some() {
                app.tools
                    .cancel_preedit()
                    .map_err(|error| error.to_string())
            } else {
                Ok(())
            };
            if matches!(
                event,
                Event::Window {
                    win_event: WindowEvent::Minimized,
                    ..
                }
            ) {
                return None;
            }
            Some(result)
        }
        Event::Window {
            win_event: WindowEvent::FocusGained,
            timestamp,
            ..
        } => {
            input.gain_focus(*timestamp);
            None
        }
        Event::KeyUp {
            keycode: Some(Keycode::Return | Keycode::KpEnter),
            ..
        } => {
            input.return_released(app.tools.is_composing());
            Some(Ok(()))
        }
        Event::TextInput {
            text, timestamp, ..
        } => {
            if !input.text_allowed(
                app.tools.input_revision().map(|revision| revision.draft),
                *timestamp,
            ) {
                return Some(Ok(()));
            }
            if app.tools.is_composing() {
                input.text_activity();
            }
            Some(if input.focused && app.tools.draft_text().is_some() {
                app.tools.input(text).map_err(|error| error.to_string())
            } else {
                Ok(())
            })
        }
        Event::TextEditing {
            text,
            start,
            length,
            timestamp,
            ..
        } => {
            if !input.text_allowed(
                app.tools.input_revision().map(|revision| revision.draft),
                *timestamp,
            ) {
                return Some(Ok(()));
            }
            input.text_activity();
            Some(if input.focused && app.tools.draft_text().is_some() {
                app.tools
                    .update_preedit(text, *start, *length)
                    .map_err(|error| error.to_string())
            } else {
                Ok(())
            })
        }
        Event::KeyDown {
            keycode: Some(key),
            keymod,
            repeat,
            ..
        } => {
            if matches!(key, Keycode::Return | Keycode::KpEnter)
                && (!input.submit_allowed(app.tools.is_composing()) || *repeat)
            {
                return Some(Ok(()));
            }
            app.tools.draft_text()?;
            if !input.focused {
                return Some(Ok(()));
            }
            let command = keymod.intersects(
                sdl3::keyboard::Mod::LCTRLMOD
                    | sdl3::keyboard::Mod::RCTRLMOD
                    | sdl3::keyboard::Mod::LGUIMOD
                    | sdl3::keyboard::Mod::RGUIMOD,
            );
            let shift =
                keymod.intersects(sdl3::keyboard::Mod::LSHIFTMOD | sdl3::keyboard::Mod::RSHIFTMOD);
            Some(match key {
                Keycode::A if command => app.tools.select_all().map_err(|error| error.to_string()),
                Keycode::Left | Keycode::Right | Keycode::Home | Keycode::End
                    if !app.tools.is_composing() =>
                {
                    visual_move(app, *key, shift)
                }
                Keycode::Delete => app.tools.delete().map_err(|error| error.to_string()),
                Keycode::Escape if app.tools.is_composing() => {
                    input.cancel_composition();
                    app.tools
                        .cancel_preedit()
                        .map_err(|error| error.to_string())
                }
                _ => native_key(app, *key),
            })
        }
        _ => None,
    }
}
/// Local text revisions already fence presented input. Only actual panel
/// scrolling additionally changes pointer geometry without a model revision.
fn update_input_presentation(app: &mut Application, scroll_before: (usize, usize)) -> bool {
    if scroll_before != (app.sidebar_scroll, app.review_scroll) {
        app.presented_generation = None;
    }
    !app.displayed_input_ready()
}

fn visual_move(app: &mut Application, key: Keycode, extend: bool) -> Result<()> {
    use tilewright_editor_ui::text_input::TextSelection;
    use tilewright_render_ui::VisualMotion;
    let snapshot = app.tools.draft_snapshot().ok_or("no current text draft")?;
    let layout = app
        .text
        .layout(
            &snapshot.display_text,
            crate::workspace_render::text_size(30),
            None,
        )
        .map_err(|error| error.to_string())?;
    let motion = match key {
        Keycode::Left => VisualMotion::Left,
        Keycode::Right => VisualMotion::Right,
        Keycode::Home => VisualMotion::Home,
        _ => VisualMotion::End,
    };
    let anchor = crate::workspace_render::renderer_cursor(snapshot.display_selection.anchor);
    let current = crate::workspace_render::renderer_cursor(snapshot.display_selection.focus);
    let focus = if !extend
        && anchor.byte != current.byte
        && matches!(motion, VisualMotion::Left | VisualMotion::Right)
    {
        let a = layout.caret(anchor).ok_or("invalid anchor geometry")?;
        let f = layout.caret(current).ok_or("invalid focus geometry")?;
        let anchor_first =
            a.y.total_cmp(&f.y).then(a.x.total_cmp(&f.x)) != std::cmp::Ordering::Greater;
        if anchor_first == (motion == VisualMotion::Left) {
            anchor
        } else {
            current
        }
    } else {
        layout.move_visual(current, motion)
    };
    let focus = crate::workspace_render::model_cursor(focus);
    let focus = snapshot
        .committed_cursor(focus)
        .map_err(|error| error.to_string())?;
    let selection = if extend {
        TextSelection {
            anchor: snapshot.selection.anchor,
            focus,
        }
    } else {
        TextSelection::caret(focus)
    };
    app.tools
        .set_selection(selection, snapshot.revision)
        .map_err(|error| error.to_string())
}

fn native_event(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &sdl3::video::Window,
    event: &Event,
    minimized: &mut bool,
    extend_selection: bool,
) -> Result<()> {
    match event {
        Event::Window {
            win_event: WindowEvent::Minimized,
            ..
        } => {
            *minimized = true;
            Ok(())
        }
        Event::Window {
            win_event:
                WindowEvent::Restored | WindowEvent::PixelSizeChanged(..) | WindowEvent::Resized(..),
            ..
        } => {
            let size = window.size_in_pixels();
            *minimized = size.0 == 0 || size.1 == 0;
            if *minimized {
                return Ok(());
            }
            renderer
                .resize(size.0, size.1)
                .map_err(|error| error.to_string())?;
            app.ui = WorkspaceUi::new(window.size(), size);
            app.view.camera.width = size.0;
            app.view.camera.height = size.1;
            app.reflow()
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            ..
        } if !*minimized => native_pointer_event(
            app,
            event,
            window.size(),
            window.size_in_pixels(),
            extend_selection,
        ),
        Event::MouseMotion {
            mousestate, x, y, ..
        } if !*minimized && app.tools.stroke().is_some() => native_stroke_motion(
            app,
            [*x, *y],
            mousestate.left(),
            window.size(),
            window.size_in_pixels(),
        ),
        Event::MouseButtonUp {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } if app.tools.stroke().is_some() => {
            native_stroke_release(app, [*x, *y], window.size(), window.size_in_pixels())
        }
        Event::MouseWheel {
            integer_y,
            mouse_x,
            mouse_y,
            ..
        } if !*minimized => {
            let Some((x, y)) = crate::viewport::physical_pixel(
                [*mouse_x, *mouse_y],
                window.size(),
                window.size_in_pixels(),
            ) else {
                return Ok(());
            };
            let amount = usize::try_from(integer_y.unsigned_abs())
                .unwrap_or(0)
                .min(32);
            let scroll = if app.ui.sidebar.contains(x, y) {
                &mut app.sidebar_scroll
            } else if app.ui.review.contains(x, y) {
                &mut app.review_scroll
            } else {
                return Ok(());
            };
            *scroll = if *integer_y > 0 {
                scroll.saturating_sub(amount)
            } else {
                scroll.saturating_add(amount)
            };
            Ok(())
        }
        Event::KeyDown {
            keycode: Some(key),
            repeat: false,
            ..
        } => {
            if matches!(key, Keycode::Return | Keycode::KpEnter)
                && app.tools.draft_text().is_none()
                && !app.displayed_input_ready()
            {
                return Err("accept waits for the current displayed pending diff".into());
            }
            native_key(app, *key)
        }
        _ => Ok(()),
    }
}

fn native_pointer_event(
    app: &mut Application,
    event: &Event,
    logical: (u32, u32),
    physical: (u32, u32),
    extend: bool,
) -> Result<()> {
    let Event::MouseButtonDown {
        mouse_btn: MouseButton::Left,
        x,
        y,
        ..
    } = event
    else {
        return Ok(());
    };
    native_pointer_selection(app, [*x, *y], logical, physical, extend)
}

#[cfg(test)]
fn native_pointer(
    app: &mut Application,
    position: [f32; 2],
    logical: (u32, u32),
    physical: (u32, u32),
) -> Result<()> {
    native_pointer_selection(app, position, logical, physical, false)
}
fn native_pointer_selection(
    app: &mut Application,
    position: [f32; 2],
    logical: (u32, u32),
    physical: (u32, u32),
    extend: bool,
) -> Result<()> {
    // A press while a stroke runs means its release was missed. It only
    // ends that stroke, visibly; the press itself does nothing else.
    if app.tools.stroke().is_some() {
        app.interrupt_stroke();
        return Err(STROKE_INTERRUPTED.into());
    }
    if !app.displayed_input_ready() {
        return Err("input waits for the current displayed frame".into());
    }
    let (px, py) = crate::viewport::physical_pixel(position, logical, physical)
        .ok_or("pointer outside physical viewport")?;
    app.refresh_ui();
    let hit = app.ui.hit(px, py);
    // A press anywhere but the focused filter field releases its focus and
    // keeps its text; the press then does what it would do anyway.
    if app.tools.filter_focused() && hit != Hit::Control(Control::EditDraft) {
        app.tools.release_filter();
    }
    match hit {
        Hit::Control(Control::EditDraft) => {
            let geometry = app
                .displayed_text
                .as_ref()
                .ok_or("no displayed text layout")?;
            if Some(geometry.revision) != app.tools.input_revision() {
                return Err("stale text layout".into());
            }
            if app.tools.is_composing() {
                return app
                    .tools
                    .cancel_preedit()
                    .map_err(|error| error.to_string());
            }
            let cursor = geometry.layout.hit([
                position[0] - geometry.origin[0],
                position[1] - geometry.origin[1],
            ]);
            let cursor = geometry
                .snapshot
                .committed_cursor(crate::workspace_render::model_cursor(cursor))
                .map_err(|error| error.to_string())?;
            let selection = if extend {
                tilewright_editor_ui::text_input::TextSelection {
                    anchor: geometry.snapshot.selection.anchor,
                    focus: cursor,
                }
            } else {
                tilewright_editor_ui::text_input::TextSelection::caret(cursor)
            };
            app.tools
                .set_selection(selection, geometry.revision)
                .map_err(|error| error.to_string())
        }
        Hit::Control(control) => app.control(control),
        Hit::World(x, y) => {
            app.tools.cancel();
            if app.tools.mode() == ToolMode::Terrain {
                app.begin_stroke(x, y)
            } else if app.tools.mode() == ToolMode::Place {
                app.place_at(x, y)
            } else {
                app.click(x, y)
            }
        }
        Hit::Panel | Hit::Outside => Ok(()),
    }
}

/// A drag sample while a stroke runs. Motion without the left button means
/// the release was missed (for example outside the window): finish there.
fn native_stroke_motion(
    app: &mut Application,
    position: [f32; 2],
    held: bool,
    logical: (u32, u32),
    physical: (u32, u32),
) -> Result<()> {
    if !held {
        return app.finish_stroke();
    }
    stroke_position(app, position, logical, physical)
}

fn native_stroke_release(
    app: &mut Application,
    position: [f32; 2],
    logical: (u32, u32),
    physical: (u32, u32),
) -> Result<()> {
    // The finish outcome is what the user sees: a last-sample problem must
    // neither hide a refused stroke nor mark a prepared one as not applied.
    if let Err(error) = stroke_position(app, position, logical, physical) {
        eprintln!("{}", terminal_field(&error));
    }
    app.finish_stroke()
}

/// Panels and pixels outside the window are misses, like map background.
fn stroke_position(
    app: &mut Application,
    position: [f32; 2],
    logical: (u32, u32),
    physical: (u32, u32),
) -> Result<()> {
    match crate::viewport::physical_pixel(position, logical, physical)
        .map(|(x, y)| app.ui.hit(x, y))
    {
        Some(Hit::World(x, y)) => app.stroke_sample(x, y),
        _ => Ok(()),
    }
}

fn native_key(app: &mut Application, key: Keycode) -> Result<()> {
    if matches!(key, Keycode::Return | Keycode::KpEnter) && !app.displayed_input_ready() {
        return Err("review/accept waits for the current displayed input".into());
    }
    // Text focus owns every key before global shortcuts. Unicode text arrives
    // exclusively through SDL TextInput, never by guessing keycode characters.
    if app.tools.filter_focused() {
        // The focused palette filter: Esc clears it, Enter keeps it; both
        // release focus. Every other key is text or ignored.
        return match key {
            Keycode::Return | Keycode::KpEnter => app.control(Control::ApplyDraft),
            Keycode::Escape => {
                app.tools.clear_filter();
                Ok(())
            }
            Keycode::Backspace => {
                app.tools.backspace();
                Ok(())
            }
            _ => Ok(()),
        };
    }
    if app.tools.draft_text().is_some() {
        return match key {
            Keycode::Return | Keycode::KpEnter => app.control(Control::ApplyDraft),
            Keycode::Escape => app.control(Control::CancelDraft),
            Keycode::Backspace => {
                app.tools.backspace();
                Ok(())
            }
            _ => Ok(()),
        };
    }
    if app.tools.stroke().is_some() {
        // A running stroke owns the keyboard: Escape abandons it, and other
        // world shortcuts are ignored so the gesture keeps its frame.
        if key == Keycode::Escape {
            app.interrupt_stroke();
            app.notice = Some("Brush stroke cancelled; nothing was prepared.".into());
        }
        return Ok(());
    }
    // Esc clears a palette filter before it cancels a pending edit.
    if key == Keycode::Escape && app.tools.clear_filter() {
        app.presented_generation = None;
        return Ok(());
    }
    match key {
        Keycode::Return | Keycode::KpEnter => app.action(Action::Accept),
        Keycode::Escape => app.action(Action::Cancel),
        Keycode::Z => app.action(Action::Undo),
        Keycode::Y => app.action(Action::Redo),
        Keycode::R => app.action(Action::Reopen),
        Keycode::Delete => app.control(Control::Delete),
        Keycode::Left if app.tools.mode() == ToolMode::Move => app.nudge([-1, 0]),
        Keycode::Right if app.tools.mode() == ToolMode::Move => app.nudge([1, 0]),
        Keycode::Up if app.tools.mode() == ToolMode::Move => app.nudge([0, -1]),
        Keycode::Down if app.tools.mode() == ToolMode::Move => app.nudge([0, 1]),
        Keycode::T => app.control(Control::Terrain),
        Keycode::P => app.control(Control::Place),
        Keycode::Slash if app.tools.mode() == ToolMode::Place => {
            app.control(Control::PaletteFilter)
        }
        Keycode::LeftBracket if app.tools.mode() == ToolMode::Terrain => {
            app.control(Control::BrushSize(-1))
        }
        Keycode::RightBracket if app.tools.mode() == ToolMode::Terrain => {
            app.control(Control::BrushSize(1))
        }
        Keycode::B if app.tools.mode() == ToolMode::Terrain => app.control(Control::BrushShape),
        Keycode::W | Keycode::A | Keycode::S | Keycode::D => {
            let change = match key {
                Keycode::W => [0.0, -1.0],
                Keycode::A => [-1.0, 0.0],
                Keycode::S => [0.0, 1.0],
                _ => [1.0, 0.0],
            };
            app.view.camera.center[0] += change[0];
            app.view.camera.center[1] += change[1];
            app.rebuild()
        }
        Keycode::Plus | Keycode::Equals | Keycode::Minus => {
            let factor = if key == Keycode::Minus { 0.8 } else { 1.25 };
            app.view.camera.zoom = (app.view.camera.zoom * factor).clamp(0.25, 8.0);
            app.rebuild()
        }
        _ => Ok(()),
    }
}

fn native_present(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &mut sdl3::video::Window,
) -> Result<bool> {
    app.refresh_ui();
    let title = if let Some(reason) = &app.blocked {
        format!("Tilewright BLOCKED: {reason} | Q quit")
    } else {
        format!(
            "Tilewright {} | {} | {}",
            app.session.catalog().presentation(),
            app.session.status(),
            HELP
        )
    };
    window
        .set_title(&terminal_field(&title))
        .map_err(|error| error.to_string())?;
    let presentation = match workspace_presentation(app) {
        Ok(presentation) => presentation,
        Err(reason) => {
            app.blocked = Some(reason.clone());
            app.presented_generation = None;
            window
                .set_title(&format!(
                    "Tilewright BLOCKED: {} | Q quit",
                    terminal_field(&reason)
                ))
                .map_err(|error| error.to_string())?;
            eprintln!("{}", terminal_field(&reason));
            return Ok(false);
        }
    };
    match tilewright_render_sdl3::present_existing(
        renderer,
        window,
        RenderRequest {
            scene: &presentation.scene,
            resources: presentation.resources,
        },
    ) {
        Ok(proof) => {
            app.presented_generation = Some(app.view.frame_revision);
            app.presented_text_revision = app.tools.input_revision();
            app.displayed_text = presentation.editable;
            println!(
                "native presented {}x{} backend={} adapter={}",
                proof.width,
                proof.height,
                terminal_field(&proof.backend),
                terminal_field(&proof.adapter_name)
            );
            Ok(true)
        }
        Err(error) => {
            let reason = error.to_string();
            app.blocked = Some(reason.clone());
            window
                .set_title(&format!(
                    "Tilewright BLOCKED: {} | Q quit",
                    terminal_field(&reason)
                ))
                .map_err(|error| error.to_string())?;
            eprintln!("{}", terminal_field(&reason));
            Ok(false)
        }
    }
}

fn workspace_presentation(app: &mut Application) -> Result<crate::workspace_render::Presentation> {
    let mut presentation = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .map_err(|error| error.to_string())?;
    if app.unsupported_runs != presentation.unsupported_runs {
        app.unsupported_runs = presentation.unsupported_runs;
        app.refresh_ui();
        presentation = crate::workspace_render::compose(
            &mut app.text,
            &app.ui,
            &app.prepared.scene,
            app.prepared.resources.clone(),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(presentation)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "tests/prototype.rs"]
mod tests;
