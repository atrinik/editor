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
use tilewright_editor_ui::brush::StrokeTarget;
use tilewright_editor_ui::inspector::{self, SelectionSummary};
use tilewright_editor_ui::tools::{self, PortalStep, ToolMode, ToolState, ZoneChoice};
use tilewright_editor_ui::{Action, Session, SessionPhase};
use tilewright_editor_ui::{anchors, portals, zones};
use tilewright_render_api::{
    BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_render_wgpu::WgpuRenderer;
use tilewright_scene::SemanticId;
use tilewright_source::SourceRoot;

const HELP: &str = "Select / Move / Terrain / Place (P) / Zone (N) / Anchor (K) | / filters the palette | click a tile places | Delete removes selection | drag paints | [ ] brush size | B brush shape | O shows zones and badges | Zone list: name, parent, new, delete | Anchors: new, move, delete | Object portal: anchor, tile, typed, clear | Enter accept | Esc cancel | Z undo | Y redo | WASD pan | +/- zoom | Q quit";
/// Status when an overlay is wanted but the cells' places in the World's
/// draw order could not be computed for this view.
const OVERLAY_UNAVAILABLE: &str = "Zone overlay and stroke preview unavailable for this view.";
const STROKE_INTERRUPTED: &str = "Brush stroke ended before release; nothing was prepared.";

type Result<T> = std::result::Result<T, String>;
type UiRows = Vec<(String, Option<Control>, bool, bool)>;
// The review text and its completeness limits are the engine's; the app
// lays the lines out.
#[cfg(test)]
use tilewright_editor_ui::review::{MAXIMUM_REVIEW_BYTES, append_change_review};
use tilewright_editor_ui::review::{REVIEW_LIMIT_MESSAGE, Review};
/// The actor of every transaction the desktop app prepares.
const ACTOR: &str = "editor:prototype";
const NOTHING_TO_PLACE: &str = "Place is unavailable: the catalog grants no prototype to place.";
/// Sidebar row heights and the gap between rows, in logical pixels. Place
/// palette rows are taller so their thumbnails stay legible.
const TEXT_ROW_HEIGHT: u32 = 26;
const PALETTE_ROW_HEIGHT: u32 = 40;
const ROW_GAP: u32 = 2;
/// Zone legend and palette rows and anchor list rows: compact, so an id, a
/// display name and a tile count, or an id, a place and a portal count, fit
/// on one line in the smaller type.
const ZONE_ROW_HEIGHT: u32 = 22;
/// Shown with every zone management row set: ids cannot be changed later.
const ZONE_IDS_FIXED: &str = "Zone ids are fixed once created";
/// Most zone properties listed, read-only, in the zone inspector.
const MAXIMUM_PROPERTY_ROWS: usize = 8;
/// Shown with the anchor editing rows: ids cannot be changed in this
/// version, and how to get the effect of a rename instead.
const ANCHOR_IDS_FIXED: &str = "Anchor ids are fixed in this version";
const ANCHOR_RENAME_HINT: [&str; 2] =
    ["To rename: create a new anchor", "and retarget its portals"];
/// The badge legend rows of Anchor mode; each carries its badge's colour.
const ANCHOR_LEGEND: &str = "Light badge: anchor";
const PORTAL_LEGEND: &str = "Dark badge: portal object";
/// The widest a toolbar button gets, whatever its label measures.
const TOOLBAR_BUTTON_MAXIMUM: u32 = 96;
/// Most anchors listed for one selected tile.
const MAXIMUM_TILE_ANCHORS: usize = anchors::MAXIMUM_TILE_ANCHORS;
/// Most anchors in Anchor mode's list, like the zone list.
const MAXIMUM_LISTED_ANCHORS: usize = tilewright_editor_ui::inspector::MAX_CHOICES;
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
/// Press and drag cells of the scripted stroke-preview capture, across
/// grass and the sand the demonstration paints with.
const DEMO_STROKE: [[i64; 2]; 2] = [[1, 2], [5, 2]];

/// Press and drag cells of the scripted zone stroke: down open meadow onto
/// the neutral sample's path row.
const DEMO_ZONE_STROKE: [[i64; 2]; 2] = [[5, 0], [5, 5]];
/// The scripted zone management: the zone whose display name it edits and
/// under which it creates a zone, the new name, the new zone's id and name,
/// and the open meadow tile it paints with the new zone.
const DEMO_RENAMED_ZONE: &str = "meadow";
const DEMO_ZONE_NAME: &str = "Open meadow";
const DEMO_NEW_ZONE: &str = "meadow.grove";
const DEMO_NEW_ZONE_NAME: &str = "Grove";
const DEMO_NEW_ZONE_TILE: [i64; 2] = [2, 4];
/// The scripted anchor editing: the new anchor's id, the tile it is created
/// on and the tile it moves to, and the sample's anchor a portal names.
const DEMO_NEW_ANCHOR: &str = "camp";
const DEMO_ANCHOR_TILE: [i64; 2] = [2, 3];
const DEMO_ANCHOR_MOVE: [i64; 2] = [3, 4];
const DEMO_NAMED_ANCHOR: &str = "arrival";
/// The scripted portal editing: the gate retargeted to another anchor, the
/// gate whose target is picked by click and then cleared, and the tile
/// picked.
const DEMO_RETARGETED_GATE: &str = "gate_east";
const DEMO_TARGET_ANCHOR: &str = "lookout";
const DEMO_PICKED_GATE: &str = "gate_west";
const DEMO_PORTAL_TILE: [i64; 2] = [4, 3];

/// The zones of the in-view tiles for one World revision and frame.
struct ZoneView {
    key: ([u8; 32], u64),
    tiles: Vec<([i64; 2], String)>,
}

/// The anchor and portal facts the sidebar and the badges show, for one
/// shown World: computed once per rebuild (the shown World changes only
/// then) instead of scanning the World on every redraw.
#[derive(Clone, Debug, Default)]
struct AnchorView {
    /// The source revision of the shown World these facts are for.
    key: [u8; 32],
    /// For each anchor of the view's plane that a portal names, the objects
    /// whose portals name it, described for one sidebar row each.
    named: std::collections::BTreeMap<String, Vec<String>>,
    /// The view's tiles that hold an anchor, and those that hold an object
    /// with a portal.
    anchor_tiles: std::collections::BTreeSet<[i64; 2]>,
    portal_tiles: std::collections::BTreeSet<[i64; 2]>,
}
impl AnchorView {
    fn of(
        world: &tilewright_source::World,
        catalog: &tilewright_editor_preview::PreviewCatalog,
    ) -> Self {
        let plane = catalog.view().0;
        let mut named = std::collections::BTreeMap::<String, Vec<String>>::new();
        for holder in anchors::portals(world) {
            if let anchors::Target::Anchor {
                plane: target,
                anchor,
            } = holder.target
                && target == plane
            {
                named
                    .entry(anchor.to_owned())
                    .or_default()
                    .push(anchors::describe_holder_short(&holder));
            }
        }
        Self {
            key: world.source_digest(),
            named,
            anchor_tiles: anchors::view_anchors(world, catalog)
                .into_iter()
                .map(|(_, at)| at)
                .collect(),
            portal_tiles: anchors::view_portal_tiles(world, catalog),
        }
    }
    /// How many portals name anchor `id` of the view's plane.
    fn count(&self, id: &str) -> usize {
        self.named.get(id).map_or(0, Vec::len)
    }
}

/// A sidebar row whose text names a zone id that must stay
/// distinguishable however narrow the sidebar is: fitted when the sidebar is
/// laid out (see `workspace_render::fit_id`).
enum IdText {
    /// The first `(prefix, suffix)` around `id` that fits; else the last
    /// one's prefix with the id elided in the middle.
    Id {
        id: String,
        variants: Vec<(String, String)>,
    },
    /// `Path: ` and the zone chain, its last zone kept whole.
    Path(Vec<String>),
}

/// Which granted edit a scripted selection needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Capability {
    Move,
    Field,
    Delete,
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
    /// How many times the World scene has been prepared. Pointer samples
    /// during a stroke never prepare it; presses, dispatched actions (a
    /// release among them) and view changes do.
    prepared_scenes: u64,
    /// Whether the last composed frame left out the stroke preview because
    /// World, preview and interface together exceeded the scene limits.
    preview_omitted: bool,
    /// The zone of every in-view tile, computed once per redraw for the
    /// legend and the overlay, keyed by the World source revision and the
    /// frame it was computed for.
    zone_view: Option<ZoneView>,
    /// The anchor and portal facts of the shown World, computed when it is
    /// rebuilt.
    anchor_view: Option<AnchorView>,
    /// The scene limits every composed frame must meet: the scene library's
    /// defaults (`SceneLimits::default()`), never queried from the renderer;
    /// tests lower them.
    frame_limits: tilewright_scene::SceneLimits,
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
    let actor = tilewright_editor_ui::Actor::new(ACTOR).map_err(|error| error.to_string())?;
    let mut session = Session::new(History::new(document), catalog, resources, actor);
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
        prepared_scenes: 1,
        preview_omitted: false,
        zone_view: None,
        anchor_view: None,
        frame_limits: tilewright_scene::SceneLimits::default(),
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
        // A chosen zone the shown World no longer defines (deleted, or its
        // creation undone) is no longer chosen.
        self.tools
            .revalidate_zones(self.session.world(), self.session.catalog());
        self.tools
            .revalidate_anchors(self.session.world(), self.session.catalog());
        self.anchor_view = Some(AnchorView::of(self.session.world(), self.session.catalog()));
        self.prepare_scene()
    }

    /// The one place the interactive World scene is prepared.
    fn prepare_scene(&mut self) -> Result<()> {
        self.prepared = tilewright_editor_preview::prepare(
            self.session.world(),
            self.session.catalog(),
            self.session.resources(),
            &self.view,
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        self.prepared_scenes += 1;
        Ok(())
    }

    fn reflow(&mut self) -> Result<()> {
        self.presented_generation = None;
        self.interrupt_stroke();
        self.prepare_scene()
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
                matches!(
                    self.tools.mode(),
                    ToolMode::Terrain | ToolMode::Zone | ToolMode::Anchor
                ),
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
        // In Zone mode without the paint grant or a chosen zone a press only
        // selects the tile, so its zone can be inspected.
        if self.tools.mode() == ToolMode::Zone
            && (!self.session.catalog().editing().zones.paint || self.tools.zone_choice().is_none())
        {
            return self.click(x, y);
        }
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
        let key = self.zone_view_key();
        if self.zone_view.as_ref().is_none_or(|view| view.key != key) {
            let tiles = zones::view_zones(self.session.world(), self.session.catalog())
                .into_iter()
                .map(|(at, zone)| (at, zone.to_owned()))
                .collect();
            self.zone_view = Some(ZoneView { key, tiles });
        }
        let mut ui = WorkspaceUi::new(self.ui.logical_size(), self.ui.size);
        self.toolbar(&mut ui);
        self.sidebar(&mut ui);
        self.review(&mut ui);
        for widget in &mut ui.widgets {
            if widget.editable.is_none() {
                let indent = if widget.thumbnail.is_some() || widget.swatch.is_some() {
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

    /// The toolbar: one button per mode and command, each as wide as its
    /// label needs, so all of them and the notice after them fit the
    /// 720-pixel minimum window width.
    fn toolbar(&mut self, ui: &mut WorkspaceUi) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        let buttons = [
            ("Select", Control::Select, true, mode == ToolMode::Select),
            ("Move", Control::Move, true, mode == ToolMode::Move),
            ("Terrain", Control::Terrain, true, mode == ToolMode::Terrain),
            (
                "Place",
                Control::Place,
                !tools::object_palette(self.session.catalog()).is_empty(),
                mode == ToolMode::Place,
            ),
            ("Zone", Control::Zone, true, mode == ToolMode::Zone),
            ("Anchor", Control::Anchor, true, mode == ToolMode::Anchor),
            (
                "Clear",
                Control::Clear,
                self.session.selection().is_some(),
                false,
            ),
            (
                "Undo",
                Control::Undo,
                ready && self.session.history().depths().0 > 0,
                false,
            ),
            (
                "Redo",
                Control::Redo,
                ready && self.session.history().depths().1 > 0,
                false,
            ),
            (
                "Delete",
                Control::Delete,
                ready && self.selection_deletable(),
                false,
            ),
        ];
        let mut x = 8;
        for (label, control, enabled, active) in buttons {
            // The label's shaped width, the 8 px the row text keeps free and
            // a little air.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let width = self
                .text
                .layout(label, crate::workspace_render::text_size(32), None)
                .map_or(TOOLBAR_BUTTON_MAXIMUM, |layout| {
                    (layout.size()[0].ceil() as u32 + 12).min(TOOLBAR_BUTTON_MAXIMUM)
                });
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
            x += width + 4;
        }
        if self.unsupported_runs > 0 {
            ui.widget(
                Rect {
                    x: x + 4,
                    y: 8,
                    width: ui.logical_size().0.saturating_sub(x + 14),
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

    fn inspector_rows(&self) -> (UiRows, Vec<(usize, IdText)>) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let mode = self.tools.mode();
        let mut rows = Vec::new();
        let mut fits = Vec::new();
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
                    &mut fits,
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
                self.cell_zone_rows(&mut rows, &mut fits, plane, layer, [x, y]);
                self.cell_anchor_rows(&mut rows, &mut fits, plane, layer, [x, y]);
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
        self.zone_toggle_row(&mut rows);
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
        if mode == ToolMode::Zone {
            self.zone_rows(&mut rows, &mut fits, ready);
        }
        if mode == ToolMode::Anchor {
            self.anchor_rows(&mut rows, &mut fits, ready);
        }
        (rows, fits)
    }

    /// Outside Zone mode, the zone overlay toggle sits between the
    /// selection and the tool's own rows.
    fn zone_toggle_row(&self, rows: &mut UiRows) {
        if self.tools.mode() == ToolMode::Zone {
            return;
        }
        let shown = self.tools.zones_visible();
        // Outside Anchor mode the badges show with the zones.
        let badges = self.tools.mode() != ToolMode::Anchor;
        rows.push((
            match (shown, badges) {
                (true, true) => "Hide zones and badges  O",
                (false, true) => "Show zones and badges  O",
                (true, false) => "Hide zones  O",
                (false, false) => "Show zones  O",
            }
            .into(),
            Some(Control::ZoneOverlay),
            true,
            shown,
        ));
    }

    /// The selected tile's zone: its id (and whether it is the plane's
    /// default), the parent chain and the display name. The id and chain
    /// rows are fitted later so long ids keep their distinguishing tail.
    fn cell_zone_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        plane: &str,
        layer: i32,
        [x, y]: [i64; 2],
    ) {
        let Some(plane) = self.session.world().planes().get(plane) else {
            return;
        };
        let Some(zone) = zones::tile_zone(plane, layer, x, y) else {
            rows.push(("Zone: none".into(), None, true, false));
            return;
        };
        let mut variants = Vec::new();
        if zone.from_default {
            variants.push(("Zone: ".into(), " (plane default)".into()));
        }
        variants.push(("Zone: ".into(), String::new()));
        fits.push((
            rows.len(),
            IdText::Id {
                id: zone.id.into(),
                variants,
            },
        ));
        rows.push((format!("Zone: {}", zone.id), None, true, false));
        fits.push((
            rows.len(),
            IdText::Path(
                zone.path
                    .iter()
                    .rev()
                    .map(|zone| (*zone).to_owned())
                    .collect(),
            ),
        ));
        // Outermost first, so the tile's own zone ends the line.
        let outward: Vec<&str> = zone.path.iter().rev().copied().collect();
        rows.push((format!("Path: {}", outward.join(" > ")), None, true, false));
        rows.push((
            format!(
                "Zone name: {}",
                zone.name
                    .map_or_else(|| "none".into(), stored_breaks_marked)
            ),
            None,
            true,
            false,
        ));
    }

    /// The anchors on the selected tile, sorted by id, each with how many
    /// portals in the world name it. Only the view's own plane and layer
    /// can be selected, so these are that plane's anchors on that layer.
    fn cell_anchor_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        plane: &str,
        layer: i32,
        at: [i64; 2],
    ) {
        let world = self.session.world();
        let Some(source) = world.planes().get(plane) else {
            return;
        };
        let here = anchors::tile_anchors(source, layer, at);
        if here.is_empty() {
            rows.push(("Anchors here: none".into(), None, true, false));
            return;
        }
        let facts = self.anchor_view();
        for id in here.iter().take(MAXIMUM_TILE_ANCHORS) {
            let named = facts.count(id);
            let suffix = format!(", {}", anchors::portal_count(named));
            id_row(
                rows,
                fits,
                id,
                vec![
                    ("Anchor here: ".into(), suffix),
                    ("Anchor: ".into(), format!(" ({named})")),
                    ("Anchor: ".into(), String::new()),
                ],
                None,
                true,
                false,
            );
        }
        if here.len() > MAXIMUM_TILE_ANCHORS {
            rows.push((
                format!(
                    "{} more anchors on this tile",
                    here.len() - MAXIMUM_TILE_ANCHORS
                ),
                None,
                true,
                false,
            ));
        }
    }

    /// Anchor mode: what the badges mean, the anchor editing rows when the
    /// catalog grants them, then every anchor of the view's plane, sorted
    /// by id, with its layer, position, whether it is in view and how many
    /// portals name it. At most 256 anchors are listed.
    fn anchor_rows(&self, rows: &mut UiRows, fits: &mut Vec<(usize, IdText)>, ready: bool) {
        let world = self.session.world();
        let catalog = self.session.catalog();
        let (plane, layer, _) = catalog.view();
        let edit = catalog.editing().anchors.edit;
        rows.push((ANCHOR_LEGEND.into(), None, true, false));
        rows.push((PORTAL_LEGEND.into(), None, true, false));
        let Some(source) = world.planes().get(plane) else {
            return;
        };
        let facts = self.anchor_view();
        let selected = match self.session.selection() {
            Some(SelectionKey::Cell {
                plane: p,
                layer: l,
                x,
                y,
                ..
            }) if catalog.tile_in_active_view(p, *l, *x, *y) => Some([*x, *y]),
            _ => None,
        };
        if edit {
            rows.push(match selected {
                Some([x, y]) => (
                    format!("New anchor at ({x}, {y})"),
                    Some(Control::AnchorCreate),
                    ready,
                    self.tools.anchor_draft().is_some(),
                ),
                None => (
                    "New anchor: select a tile first".into(),
                    Some(Control::AnchorCreate),
                    false,
                    false,
                ),
            });
        } else {
            rows.push(("Anchor editing not granted".into(), None, false, false));
        }
        let chosen = self
            .tools
            .anchor_choice()
            .and_then(|id| source.anchors.get_key_value(id));
        if let Some((id, anchor)) = chosen {
            self.chosen_anchor_rows(rows, fits, (id, *anchor), selected, ready);
        } else if !source.anchors.is_empty() {
            rows.push(("Choose an anchor below".into(), None, true, false));
        }
        if edit {
            rows.push((ANCHOR_IDS_FIXED.into(), None, true, false));
            for line in ANCHOR_RENAME_HINT {
                rows.push((line.into(), None, true, false));
            }
        }
        let total = source.anchors.len();
        rows.push((format!("Anchors: {total} in the plane"), None, true, false));
        if total > MAXIMUM_LISTED_ANCHORS {
            rows.push((
                format!("First {MAXIMUM_LISTED_ANCHORS} anchors shown"),
                None,
                true,
                false,
            ));
        }
        for (id, anchor) in source.anchors.iter().take(MAXIMUM_LISTED_ANCHORS) {
            let [x, y] = anchor.at;
            let in_view = anchor.layer == layer && catalog.tile_in_active_view(plane, layer, x, y);
            let named = facts.count(id);
            let seen = if in_view { "in view" } else { "not in view" };
            let portals = anchors::portal_count(named);
            let long = format!(" = layer {} ({x}, {y}), {seen}, {portals}", anchor.layer);
            fits.push((
                rows.len(),
                IdText::Id {
                    id: id.clone(),
                    variants: vec![
                        (String::new(), long.clone()),
                        (String::new(), format!(" ({x}, {y}), {seen}, {portals}")),
                        (String::new(), format!(" ({x}, {y}), {portals}")),
                        (String::new(), format!(", {portals}")),
                        (String::new(), String::new()),
                    ],
                },
            ));
            rows.push((
                format!("{id}{long}"),
                Some(Control::AnchorChoose(id.clone())),
                ready,
                self.tools.anchor_choice() == Some(id.as_str()),
            ));
        }
    }

    /// The chosen anchor: its fixed id, place and the portals that name it,
    /// then, with the grant, its move and delete rows; a disabled row says
    /// why.
    fn chosen_anchor_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        (id, anchor): (&str, tilewright_source::Anchor),
        selected: Option<[i64; 2]>,
        ready: bool,
    ) {
        let catalog = self.session.catalog();
        let (plane, _, _) = catalog.view();
        let [x, y] = anchor.at;
        let in_view = catalog.tile_in_active_view(plane, anchor.layer, x, y);
        id_row(
            rows,
            fits,
            id,
            vec![
                ("Anchor ".into(), " (id is fixed)".into()),
                ("Anchor ".into(), String::new()),
            ],
            None,
            true,
            false,
        );
        rows.push((
            format!(
                "Layer {} at ({x}, {y}), {}",
                anchor.layer,
                if in_view { "in view" } else { "not in view" }
            ),
            None,
            true,
            false,
        ));
        let facts = self.anchor_view();
        let holders = facts.named.get(id).map_or(&[][..], Vec::as_slice);
        if holders.is_empty() {
            rows.push(("Named by no portal".into(), None, true, false));
        } else {
            rows.push((
                format!("Named by {}:", anchors::portal_count(holders.len())),
                None,
                true,
                false,
            ));
            for holder in holders.iter().take(anchors::MAXIMUM_NAMED) {
                rows.push((holder.clone(), None, true, false));
            }
            if holders.len() > anchors::MAXIMUM_NAMED {
                rows.push((
                    format!("and {} more", holders.len() - anchors::MAXIMUM_NAMED),
                    None,
                    true,
                    false,
                ));
            }
        }
        if !catalog.editing().anchors.edit {
            return;
        }
        rows.push(match (in_view, selected) {
            (false, _) => (
                "Move: only an anchor in view".into(),
                Some(Control::AnchorMove),
                false,
                false,
            ),
            (true, Some(tile)) if tile != anchor.at => (
                format!("Move here: ({}, {})", tile[0], tile[1]),
                Some(Control::AnchorMove),
                ready,
                false,
            ),
            (true, _) => (
                "Move: select its new tile".into(),
                Some(Control::AnchorMove),
                false,
                false,
            ),
        });
        rows.push(if holders.is_empty() {
            (
                "Delete anchor (unreferenced)".into(),
                Some(Control::AnchorDelete),
                ready,
                false,
            )
        } else {
            (
                format!(
                    "Delete: {} {} it",
                    anchors::portal_count(holders.len()),
                    if holders.len() == 1 { "names" } else { "name" }
                ),
                Some(Control::AnchorDelete),
                false,
                false,
            )
        });
    }

    /// Whether an overlay that needs the World's draw order (zone tints or
    /// a stroke preview) is wanted now but cannot be drawn because the cell
    /// slots are unavailable. Badges are drawn above the World and need no
    /// slots, so Anchor mode alone never reports this.
    fn overlay_unavailable(&self) -> bool {
        (self.tools.zones_visible() || self.tools.stroke().is_some())
            && self.prepared.cell_slots().is_none()
    }

    /// The anchor facts of the shown World: those computed at the last
    /// rebuild while they are current, else computed now.
    fn anchor_view(&self) -> std::borrow::Cow<'_, AnchorView> {
        match &self.anchor_view {
            Some(view) if view.key == self.session.world().source_digest() => {
                std::borrow::Cow::Borrowed(view)
            }
            _ => std::borrow::Cow::Owned(AnchorView::of(
                self.session.world(),
                self.session.catalog(),
            )),
        }
    }

    fn zone_view_key(&self) -> ([u8; 32], u64) {
        (
            self.session.world().source_digest(),
            self.view.frame_revision,
        )
    }

    /// The zones of the in-view tiles: this redraw's computation when it is
    /// current, else computed now.
    fn view_zone_tiles(&self) -> std::borrow::Cow<'_, [([i64; 2], String)]> {
        match &self.zone_view {
            Some(view) if view.key == self.zone_view_key() => {
                std::borrow::Cow::Borrowed(&view.tiles)
            }
            _ => std::borrow::Cow::Owned(
                zones::view_zones(self.session.world(), self.session.catalog())
                    .into_iter()
                    .map(|(at, zone)| (at, zone.to_owned()))
                    .collect(),
            ),
        }
    }

    /// The tint of every zone the view's plane defines in the shown World
    /// (the pending one while a change is reviewed): a zone's colour depends
    /// only on the plane's zone definitions, never on a stroke, the tiles'
    /// zones or what is in view. A pending change that creates or deletes a
    /// zone can move other zones' tints, and the legend, the overlay and
    /// the swatches all follow it, then the committed World again after
    /// Accept, Cancel or Undo.
    fn zone_tints(&self) -> std::collections::BTreeMap<String, usize> {
        self.session
            .world()
            .planes()
            .get(self.session.catalog().view().0)
            .map(|plane| {
                zones::plane_tints(plane)
                    .into_iter()
                    .map(|(zone, tint)| (zone.to_owned(), tint))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Zone mode: the brush, then the zone legend and palette in one list.
    /// Each zone of the plane is a row, sorted by id, with its display name
    /// and how many tiles of the view it holds, and its overlay tint as a
    /// swatch. With the paint grant a row chooses the zone to paint; the
    /// last row paints the default zone (empties the cells). At most 256
    /// zones are listed.
    fn zone_rows(&self, rows: &mut UiRows, fits: &mut Vec<(usize, IdText)>, ready: bool) {
        let world = self.session.world();
        let catalog = self.session.catalog();
        let paint = catalog.editing().zones.paint;
        let manage = catalog.editing().zones.manage;
        if paint {
            self.brush_rows(rows);
        }
        let tiles = self.view_zone_tiles();
        let counts = zones::tile_counts(tiles.iter().map(|(_, zone)| zone.as_str()));
        let tints = self.zone_tints();
        let (plane, _, _) = catalog.view();
        let plane = world.planes().get(plane);
        let palette = tools::zone_palette(world, catalog);
        let total = plane.map_or(0, |plane| plane.zones.len());
        rows.push((
            format!("Zones: {} of {total} in view (tiles)", counts.len()),
            None,
            true,
            false,
        ));
        if total > palette.len() {
            rows.push((
                format!("First {} zones shown", palette.len()),
                None,
                true,
                false,
            ));
        }
        for zone in palette {
            let name = plane
                .and_then(|plane| zones::display_name(plane, zone))
                .map_or_else(|| "no display name".into(), stored_breaks_marked);
            let shared = tints
                .get(zone)
                .is_some_and(|tint| tints.values().filter(|other| *other == tint).count() > 1);
            let shared = if shared { ", shared tint" } else { "" };
            let (long, count) = match counts.get(zone) {
                Some(count) => (
                    format!(" = {name} ({count}){shared}"),
                    format!(" ({count})"),
                ),
                None => (
                    format!(" = {name}, not in view{shared}"),
                    ", not in view".into(),
                ),
            };
            fits.push((
                rows.len(),
                IdText::Id {
                    id: zone.into(),
                    variants: vec![
                        (String::new(), long.clone()),
                        (String::new(), count),
                        (String::new(), String::new()),
                    ],
                },
            ));
            let choice = ZoneChoice::Zone(zone.to_owned());
            let active = (paint || manage) && self.tools.zone_choice() == Some(&choice);
            rows.push((
                format!("{zone}{long}"),
                Some(Control::ZonePalette(choice)),
                ready && (paint || manage),
                active,
            ));
        }
        let default = plane.and_then(|plane| plane.default_zone.as_deref());
        if let Some(zone) = default {
            fits.push((
                rows.len(),
                IdText::Id {
                    id: zone.into(),
                    variants: vec![
                        ("Default zone (clear): ".into(), String::new()),
                        ("Default: ".into(), String::new()),
                    ],
                },
            ));
        }
        rows.push((
            default.map_or_else(
                || "No zone (clear cells)".into(),
                |zone| format!("Default zone (clear): {zone}"),
            ),
            Some(Control::ZonePalette(ZoneChoice::Default)),
            ready && paint,
            paint && self.tools.zone_choice() == Some(&ZoneChoice::Default),
        ));
        if !paint {
            rows.push(("Zone painting not granted".into(), None, false, false));
        }
        if manage {
            self.zone_manage_rows(rows, fits, ready);
        }
    }

    /// With `editing.zones.manage`: the chosen zone's inspector (its fixed
    /// id, display name, parent, use, properties) with its edits, then the
    /// rows that start a new zone. Rows naming a zone id are fitted later.
    fn zone_manage_rows(&self, rows: &mut UiRows, fits: &mut Vec<(usize, IdText)>, ready: bool) {
        let world = self.session.world();
        let catalog = self.session.catalog();
        let plane = world.planes().get(catalog.view().0);
        let chosen = match self.tools.zone_choice() {
            Some(ZoneChoice::Zone(zone)) => plane.and_then(|plane| {
                plane
                    .zones
                    .get_key_value(zone)
                    .map(|(id, definition)| (plane, id.as_str(), definition))
            }),
            _ => None,
        };
        let Some((plane, id, zone)) = chosen else {
            rows.push(("Choose a zone above to manage it".into(), None, true, false));
            rows.push((
                "New top-level zone".into(),
                Some(Control::ZoneCreate(None)),
                ready,
                false,
            ));
            rows.push((ZONE_IDS_FIXED.into(), None, true, false));
            return;
        };
        id_row(
            rows,
            fits,
            id,
            vec![
                ("Zone ".into(), " (id is fixed)".into()),
                ("Zone ".into(), String::new()),
            ],
            None,
            true,
            false,
        );
        let name = match (&zone.name, zones::display_name(plane, id)) {
            (Some(name), _) => format!("Name: {}", stored_breaks_marked(name)),
            (None, Some(inherited)) => {
                rows.push(("Name: none".into(), None, true, false));
                format!("Inherits: {}", stored_breaks_marked(inherited))
            }
            (None, None) => "Name: none".into(),
        };
        rows.push((name, None, true, false));
        rows.push((
            "Edit display name".into(),
            Some(Control::ZoneName),
            ready,
            self.tools.zone_draft_target() == Some((tools::ZoneDraft::Name, Some(id))),
        ));
        self.zone_parent_rows(rows, fits, zone, ready);
        let usage = self.zone_use_rows(rows, fits, plane, (id, zone));
        if usage.unused() {
            rows.push((
                "Delete zone (unused)".into(),
                Some(Control::ZoneDelete),
                ready,
                false,
            ));
        } else {
            rows.push((
                "Delete: not while in use".into(),
                Some(Control::ZoneDelete),
                false,
                false,
            ));
            for reason in usage.short_reasons() {
                rows.push((format!("In use: {reason}"), None, true, false));
            }
        }
        id_row(
            rows,
            fits,
            id,
            vec![("New zone under ".into(), String::new())],
            Some(Control::ZoneCreate(Some(id.to_owned()))),
            ready,
            false,
        );
        rows.push((
            "New top-level zone".into(),
            Some(Control::ZoneCreate(None)),
            ready,
            false,
        ));
        rows.push((ZONE_IDS_FIXED.into(), None, true, false));
    }

    /// The chosen zone's parent, and when listed its choices: no parent,
    /// then every zone but itself and its descendants.
    fn zone_parent_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        zone: &tilewright_source::Zone,
        ready: bool,
    ) {
        let open = self.tools.parent_choices_open();
        match zone.parent.as_deref() {
            Some(parent) => id_row(
                rows,
                fits,
                parent,
                vec![
                    ("Parent: ".into(), " (change)".into()),
                    ("Parent: ".into(), String::new()),
                ],
                Some(Control::ZoneParents),
                ready,
                open,
            ),
            None => rows.push((
                "Parent: none (change)".into(),
                Some(Control::ZoneParents),
                ready,
                open,
            )),
        }
        if let Some(choices) = self.tools.parent_choices() {
            rows.push((
                "No parent (top level)".into(),
                Some(Control::ZoneParent(None)),
                ready && zone.parent.is_some(),
                zone.parent.is_none(),
            ));
            for candidate in &choices.zones {
                let candidate = candidate.as_str();
                let current = zone.parent.as_deref() == Some(candidate);
                id_row(
                    rows,
                    fits,
                    candidate,
                    vec![("Under ".into(), String::new())],
                    Some(Control::ZoneParent(Some(candidate.to_owned()))),
                    ready && !current,
                    current,
                );
            }
            if choices.total > choices.zones.len() {
                rows.push((
                    format!(
                        "First {} of {} zones shown",
                        choices.zones.len(),
                        choices.total
                    ),
                    None,
                    true,
                    false,
                ));
            }
        }
    }

    /// What keeps World from deleting the chosen zone, one fact per row, so
    /// a disabled Delete has its reasons in view, then its properties
    /// (read-only).
    fn zone_use_rows<'w>(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        plane: &'w tilewright_source::Plane,
        (id, zone): (&str, &tilewright_source::Zone),
    ) -> zones::ZoneUse<'w> {
        let layer = self.session.catalog().view().1;
        let (mut in_view, mut explicit) = (0, 0);
        for ([x, y], tile) in self.view_zone_tiles().iter() {
            if tile == id {
                in_view += 1;
                if zones::cell_zone(plane, layer, *x, *y) == Some(id) {
                    explicit += 1;
                }
            }
        }
        // What keeps World from deleting the zone, one fact per row, so a
        // disabled Delete below has its reasons in view.
        let usage = zones::zone_use(plane, id);
        let tiles = |count: usize| {
            if count == 1 {
                "1 tile".to_owned()
            } else {
                format!("{count} tiles")
            }
        };
        rows.push((
            format!("Shown on {} in view", tiles(in_view)),
            None,
            true,
            false,
        ));
        if in_view > explicit {
            // Empty cells show the plane's default zone without holding it.
            rows.push((
                format!("{} of them as the plane default", in_view - explicit),
                None,
                true,
                false,
            ));
        }
        // Every chunk of every layer of the plane, not just the view.
        rows.push((
            if usage.tiles == 0 {
                "Held by no tile in the plane".to_owned()
            } else {
                format!("Held by {} in the plane", tiles(usage.tiles))
            },
            None,
            true,
            false,
        ));
        if usage.layers > 1 {
            rows.push((format!("on {} layers", usage.layers), None, true, false));
        }
        if usage.listed > 0 {
            rows.push((
                if usage.listed == 1 {
                    "Listed by 1 chunk palette, no tile".to_owned()
                } else {
                    format!("Listed by {} chunk palettes, no tile", usage.listed)
                },
                None,
                true,
                false,
            ));
        }
        match usage.children.as_slice() {
            [] => rows.push(("No child zones".into(), None, true, false)),
            [child] => id_row(
                rows,
                fits,
                child,
                vec![("Child zone: ".into(), String::new())],
                None,
                true,
                false,
            ),
            children => rows.push((format!("{} child zones", children.len()), None, true, false)),
        }
        if usage.default {
            rows.push(("It is the plane's default zone".into(), None, true, false));
        }
        let properties = zones::properties(zone);
        for (name, value) in properties.iter().take(MAXIMUM_PROPERTY_ROWS) {
            rows.push((
                format!("Property {name} = {value} (read-only)"),
                None,
                true,
                false,
            ));
        }
        if properties.len() > MAXIMUM_PROPERTY_ROWS {
            rows.push((
                format!(
                    "{} more properties not shown",
                    properties.len() - MAXIMUM_PROPERTY_ROWS
                ),
                None,
                true,
                false,
            ));
        }
        usage
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

    /// The selected object's portal: where it leads, and with the
    /// catalog's `portal` grant the ways to set or clear it: an anchor from
    /// a list, a tile picked in the view, a typed location, or none.
    fn portal_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        uid: &str,
        plane: &str,
    ) {
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let world = self.session.world();
        let granted = self.session.catalog().can_edit_portal(world, uid);
        let portal = world.object(uid).and_then(|object| object.portal.as_ref());
        let target = portal.map(|portal| anchors::resolve(plane, portal));
        self.portal_target_rows(rows, fits, target.as_ref(), granted);
        if !granted {
            return;
        }
        let step = self
            .tools
            .portal_step()
            .filter(|(object, _)| *object == uid)
            .map(|(_, step)| step);
        let listing = step == Some(PortalStep::Anchors);
        rows.push((
            if listing {
                "Target an anchor: hide the list".into()
            } else {
                "Target an anchor".into()
            },
            Some(Control::PortalAnchors),
            ready,
            listing,
        ));
        if listing {
            self.portal_anchor_rows(rows, fits, uid, target.as_ref(), ready);
        }
        let picking = step == Some(PortalStep::PickTile);
        rows.push((
            if picking {
                "Click a tile in the view (Esc ends)".into()
            } else {
                "Target a tile in view".into()
            },
            Some(Control::PortalPick),
            ready,
            picking,
        ));
        rows.push((
            "Type a location".into(),
            Some(Control::PortalLocation),
            ready,
            self.tools.portal_draft() == Some(uid),
        ));
        rows.push((
            if portal.is_some() {
                "Clear portal".into()
            } else {
                "Clear portal: it has none".into()
            },
            Some(Control::PortalClear),
            ready && portal.is_some(),
            false,
        ));
    }

    /// Where the selected object's portal leads: `Portal: none` (only when
    /// its portal may be edited), a location, or an anchor and its place.
    fn portal_target_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        target: Option<&anchors::Target<'_>>,
        granted: bool,
    ) {
        let world = self.session.world();
        match target {
            None if granted => rows.push(("Portal: none".into(), None, true, false)),
            None => {}
            Some(&anchors::Target::Location { plane, layer, at }) => rows.push((
                format!(
                    "Portal: to {}, layer {layer}, ({}, {})",
                    anchors::short(plane),
                    at[0],
                    at[1]
                ),
                None,
                true,
                false,
            )),
            Some(&anchors::Target::Anchor { plane, anchor }) => {
                id_row(
                    rows,
                    fits,
                    anchor,
                    // The next row names its plane, layer and tile.
                    vec![
                        ("Portal: to anchor ".into(), String::new()),
                        ("Portal: ".into(), String::new()),
                    ],
                    None,
                    true,
                    false,
                );
                let location = world
                    .planes()
                    .get(plane)
                    .and_then(|source| source.anchors.get(anchor))
                    .map_or_else(
                        || "Not defined in its plane".to_owned(),
                        |found| {
                            format!(
                                "at {}, layer {}, ({}, {})",
                                anchors::short(plane),
                                found.layer,
                                found.at[0],
                                found.at[1]
                            )
                        },
                    );
                rows.push((location, None, true, false));
            }
        }
    }

    /// The anchors the selected object's portal may target: a hint naming
    /// the object, the real total, then the anchors grouped by plane, the
    /// view's plane first, at most 256. The one the portal names is marked.
    fn portal_anchor_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
        uid: &str,
        current: Option<&anchors::Target<'_>>,
        ready: bool,
    ) {
        let world = self.session.world();
        id_row(
            rows,
            fits,
            uid,
            vec![
                ("Portal of ".into(), ": choose an anchor".into()),
                ("Portal of ".into(), String::new()),
            ],
            None,
            true,
            false,
        );
        let (listed, total) = portals::target_anchors(world, self.session.catalog());
        rows.push((
            match total {
                0 => "No anchor in the world".into(),
                1 => "1 anchor in the world".into(),
                _ => format!("{total} anchors in the world"),
            },
            None,
            true,
            false,
        ));
        if total > listed.len() {
            rows.push((
                format!("First {} anchors shown", listed.len()),
                None,
                true,
                false,
            ));
        }
        let mut shown_plane = None;
        for found in listed {
            if shown_plane != Some(found.plane) {
                shown_plane = Some(found.plane);
                id_row(
                    rows,
                    fits,
                    found.plane,
                    vec![("Plane ".into(), ":".into())],
                    None,
                    true,
                    false,
                );
            }
            let [x, y] = found.at;
            let named = current
                == Some(&anchors::Target::Anchor {
                    plane: found.plane,
                    anchor: found.anchor,
                });
            id_row(
                rows,
                fits,
                found.anchor,
                vec![
                    (
                        String::new(),
                        format!(" = layer {} ({x}, {y})", found.layer),
                    ),
                    (String::new(), format!(" ({x}, {y})")),
                    (String::new(), String::new()),
                ],
                Some(Control::PortalAnchor {
                    plane: found.plane.into(),
                    anchor: found.anchor.into(),
                }),
                ready && !named,
                named,
            );
        }
    }

    fn object_rows(
        &self,
        rows: &mut UiRows,
        fits: &mut Vec<(usize, IdText)>,
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
        self.portal_rows(rows, fits, uid, plane);
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
                    tilewright_source::FieldValue::String(text) => stored_breaks_marked(text),
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
            rows.extend(stored_breaks_row(row));
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
        let (mut rows, fits) = self.inspector_rows();
        // Rows naming a zone id are fitted so the id's distinguishing tail
        // survives a narrow sidebar: they then fit whole and are not cut
        // at the end again when laid out.
        for (index, text) in fits {
            let Some(row) = rows.get_mut(index) else {
                continue;
            };
            let (height, indent) = match row.1 {
                Some(Control::ZonePalette(_)) => (
                    ZONE_ROW_HEIGHT,
                    crate::workspace_render::thumbnail_indent(ZONE_ROW_HEIGHT),
                ),
                Some(Control::AnchorChoose(_)) => (ZONE_ROW_HEIGHT, 0),
                _ => (TEXT_ROW_HEIGHT, 0),
            };
            let available = width.saturating_sub(indent);
            row.0 = match text {
                IdText::Id { id, variants } => crate::workspace_render::fit_id(
                    &mut self.text,
                    &id,
                    &variants,
                    available,
                    height,
                ),
                IdText::Path(path) => {
                    crate::workspace_render::fit_path(&mut self.text, &path, available, height)
                }
            };
        }
        let mut y = 56;
        if self.tools.mode() == ToolMode::Place
            && !tools::object_palette(self.session.catalog()).is_empty()
        {
            y = self.filter_field(ui, y);
            y = self.placing_row(ui, y);
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
                self.draft_hint(supported, width),
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

    /// The hint under a text draft. A zone draft names the zone it edits,
    /// or the parent a new zone goes under, fitted so the id stays
    /// distinguishable.
    fn draft_hint(&mut self, supported: bool, width: u32) -> String {
        if !supported {
            return "Unsupported text; cannot review".into();
        }
        if let Some([x, y]) = self.tools.anchor_draft() {
            return format!("New anchor id at ({x}, {y}); Enter reviews");
        }
        if let Some(uid) = self.tools.portal_draft() {
            let variants = vec![
                ("Portal of ".into(), ": [plane] layer x y".into()),
                ("Portal of ".into(), String::new()),
            ];
            return crate::workspace_render::fit_id(&mut self.text, uid, &variants, width, 22);
        }
        let (zone, variants) = match self.tools.zone_draft_target() {
            None => return "Ctrl+A selects / Enter reviews".into(),
            Some((tools::ZoneDraft::Create, None)) => {
                return "New top-level zone: id, space, name".into();
            }
            Some((tools::ZoneDraft::Name, Some(zone))) => (
                zone.to_owned(),
                vec![
                    ("Name of ".into(), "; empty = none".into()),
                    ("Name of ".into(), String::new()),
                ],
            ),
            Some((_, zone)) => (
                zone.unwrap_or_default().to_owned(),
                vec![
                    ("New zone under ".into(), ": id, name".into()),
                    ("New under ".into(), String::new()),
                ],
            ),
        };
        crate::workspace_render::fit_id(&mut self.text, &zone, &variants, width, 22)
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
        let tints = if self.tools.mode() == ToolMode::Zone {
            self.zone_tints()
        } else {
            std::collections::BTreeMap::new()
        };
        let default_zone = self
            .session
            .world()
            .planes()
            .get(self.session.catalog().view().0)
            .and_then(|plane| plane.default_zone.clone());
        let swatch = |control: &Option<Control>| {
            let zone = match control {
                Some(Control::ZonePalette(ZoneChoice::Zone(zone))) => Some(zone),
                Some(Control::ZonePalette(ZoneChoice::Default)) => default_zone.as_ref(),
                _ => None,
            }?;
            tints.get(zone).map(|tint| zones::swatch_color(*tint))
        };
        let swatches: Vec<_> = rows
            .iter()
            .map(|row| match row.0.as_str() {
                ANCHOR_LEGEND => (
                    Some(crate::workspace_render::ANCHOR_BADGE),
                    Some(crate::workspace_render::PORTAL_BADGE),
                ),
                PORTAL_LEGEND => (
                    Some(crate::workspace_render::PORTAL_BADGE),
                    Some(crate::workspace_render::ANCHOR_BADGE),
                ),
                _ => (swatch(&row.1), None),
            })
            .collect();
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
            } else if matches!(
                row.1,
                Some(Control::ZonePalette(_) | Control::AnchorChoose(_))
            ) {
                ZONE_ROW_HEIGHT
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
        for (row, swatch) in rows.into_iter().zip(swatches).skip(self.sidebar_scroll) {
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
                (widget.swatch, widget.swatch_outline) = swatch;
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

    /// The pinned, unclickable row under the filter that always names the
    /// prototype a tile click places, in full and with its thumbnail, even
    /// when the filter hides its palette row. It never scrolls, and is
    /// dimmed while a pending change blocks placing. Returns the next free
    /// row.
    fn placing_row(&self, ui: &mut WorkspaceUi, y: u32) -> u32 {
        let catalog = self.session.catalog();
        let (text, thumbnail) = match self.tools.placement_choice() {
            Some(proto) => (
                format!("Placing: {proto}"),
                self.session.resources().prototype_texture(catalog, proto),
            ),
            None => ("Placing: choose a type below".into(), None),
        };
        let height = if thumbnail.is_some() {
            PALETTE_ROW_HEIGHT
        } else {
            TEXT_ROW_HEIGHT
        };
        let before = ui.widgets.len();
        ui.widget(
            Rect {
                x: ui.sidebar_x(),
                y,
                width: ui.sidebar_width(),
                height,
            },
            text,
            None,
            self.session.history().pending().is_none(),
            false,
        );
        if ui.widgets.len() > before
            && let Some(widget) = ui.widgets.last_mut()
        {
            widget.thumbnail = thumbnail;
        }
        y + height + ROW_GAP
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

    /// What an open portal step asks for, naming the object.
    fn portal_status(&self) -> Option<String> {
        let (uid, step) = self.tools.portal_step()?;
        (self.session.status().phase == SessionPhase::Ready).then(|| match step {
            PortalStep::PickTile => format!(
                "Click a tile in the view: the portal of {} leads there.",
                anchors::short(uid)
            ),
            PortalStep::Anchors => format!(
                "Choose an anchor in the sidebar for the portal of {}.",
                anchors::short(uid)
            ),
        })
    }

    #[allow(clippy::too_many_lines)]
    fn status_label(&self) -> String {
        if self.blocked.is_some() || self.session.status().phase == SessionPhase::Blocked {
            return "Editing blocked. Recovery details below.".into();
        }
        if let Some(stroke) = self.tools.stroke() {
            let changes = self
                .session
                .stroke_changes(stroke)
                .map_or(0, |cells| cells.len());
            let target = match stroke.target() {
                StrokeTarget::Terrain(proto) => format!("with {proto}"),
                StrokeTarget::Zone(Some(zone)) => format!("into zone {zone}"),
                StrokeTarget::Zone(None) => "back to the default zone".into(),
            };
            return format!(
                "Painting {} cells {target}, {changes} to change; release to review.{}{}",
                stroke.len(),
                if stroke.capped() {
                    " Brush limit reached."
                } else {
                    ""
                },
                if self.preview_omitted {
                    " Too many cells to preview."
                } else if self.prepared.cell_slots().is_none() {
                    " Preview unavailable for this view."
                } else {
                    ""
                }
            );
        }
        if self.notice.is_some() {
            return "Input not applied. Details below.".into();
        }
        if self.preview_omitted {
            // Badges show with the zones, and alone in Anchor mode.
            return if self.tools.zones_visible() {
                "Zone overlay and badges left out: too many cells for one frame."
            } else {
                "Anchor and portal badges left out: too many for one frame."
            }
            .into();
        }
        if self.overlay_unavailable() {
            return OVERLAY_UNAVAILABLE.into();
        }
        if self.session.status().phase == SessionPhase::Pending && !self.source_review().complete {
            return REVIEW_LIMIT_MESSAGE.into();
        }
        if let Some(status) = self.portal_status() {
            return status;
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
            SessionPhase::Ready if self.tools.mode() == ToolMode::Zone => {
                let editing = self.session.catalog().editing();
                if !editing.zones.paint && editing.zones.manage {
                    return "Showing zones. Choose a zone in the sidebar to manage it.".into();
                }
                if !editing.zones.paint {
                    return "Showing zones. This catalog does not grant zone painting.".into();
                }
                return match self.tools.zone_choice() {
                    None => "Showing zones. Choose a zone in the sidebar to paint.".into(),
                    Some(ZoneChoice::Zone(zone)) => format!("Ready. Drag to paint zone {zone}."),
                    Some(ZoneChoice::Default) => {
                        "Ready. Drag to return tiles to the default zone.".into()
                    }
                };
            }
            SessionPhase::Ready if self.tools.mode() == ToolMode::Anchor => {
                if !self.session.catalog().editing().anchors.edit {
                    return "Showing anchors. This catalog does not grant anchor editing.".into();
                }
                return match self.tools.anchor_choice() {
                    None => "Showing anchors. Select a tile for a new anchor, or choose one below."
                        .into(),
                    Some(anchor) => format!(
                        "Anchor {} chosen. Select a tile to move it there, or delete it.",
                        anchors::short(anchor)
                    ),
                };
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
        self.source_review().lines.clone()
    }

    fn source_review(&self) -> &Review {
        self.session.review()
    }

    fn control(&mut self, control: Control) -> Result<()> {
        self.presented_generation = None;
        self.notice = None;
        match control {
            Control::Select
            | Control::Move
            | Control::Terrain
            | Control::Zone
            | Control::Anchor => {
                self.tools.set_mode(match control {
                    Control::Move => ToolMode::Move,
                    Control::Terrain => ToolMode::Terrain,
                    Control::Zone => ToolMode::Zone,
                    Control::Anchor => ToolMode::Anchor,
                    _ => ToolMode::Select,
                });
                self.sidebar_scroll = 0;
                Ok(())
            }
            Control::Place => self.enter_place_mode(),
            Control::ZoneOverlay
            | Control::ZonePalette(_)
            | Control::ZoneName
            | Control::ZoneParents
            | Control::ZoneParent(_)
            | Control::ZoneCreate(_)
            | Control::ZoneDelete => self.zone_control(control),
            Control::ApplyDraft if self.tools.zone_draft().is_some() => self.apply_zone_draft(),
            Control::AnchorChoose(_)
            | Control::AnchorCreate
            | Control::AnchorMove
            | Control::AnchorDelete => self.anchor_control(control),
            Control::ApplyDraft if self.tools.anchor_draft().is_some() => self.apply_anchor_draft(),
            Control::PortalAnchors
            | Control::PortalAnchor { .. }
            | Control::PortalPick
            | Control::PortalLocation
            | Control::PortalClear => self.portal_control(control),
            Control::ApplyDraft if self.tools.portal_draft().is_some() => self.apply_portal_draft(),
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
                self.draft_control(control)
            }
        }
    }

    /// The coordinate and field drafts: begin one, or review the open one.
    fn draft_control(&mut self, control: Control) -> Result<()> {
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
                    return Err("missing font glyphs: review blocked; draft preserved".into());
                }
                let action = self
                    .tools
                    .build_action(&inspected, unavailable)
                    .map_err(|error| error.to_string())?;
                self.action(action)
            }
        }
    }

    /// The zone overlay toggle, the zone palette rows and the zone
    /// management rows. Edits leave as Session actions; the Session checks
    /// the grant, the revision and every refusal again.
    fn zone_control(&mut self, control: Control) -> Result<()> {
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let expected = self.session.history().document().revision();
        let plane = self.session.catalog().view().0.to_owned();
        let chosen = match self.tools.zone_choice() {
            Some(ZoneChoice::Zone(zone)) => Some(zone.clone()),
            _ => None,
        };
        match control {
            Control::ZoneOverlay => self.tools.toggle_zones().map(|_| ()),
            Control::ZonePalette(choice) => {
                self.tools
                    .choose_zone(self.session.world(), self.session.catalog(), choice)
            }
            Control::ZoneName => self.tools.begin_zone_name(
                self.session.world(),
                self.session.catalog(),
                expected,
                unavailable,
            ),
            Control::ZoneCreate(parent) => self.tools.begin_zone_create(
                self.session.world(),
                self.session.catalog(),
                parent,
                expected,
                unavailable,
            ),
            Control::ZoneParents => self
                .tools
                .toggle_parent_choices(self.session.world(), self.session.catalog())
                .map(|_| ()),
            Control::ZoneParent(parent) => {
                let zone = chosen.ok_or("choose a zone in the zone list first")?;
                // A choice closes the list, as a menu would.
                if self.tools.parent_choices_open() {
                    let _ = self
                        .tools
                        .toggle_parent_choices(self.session.world(), self.session.catalog());
                }
                return self.action(Action::SetZoneParent {
                    plane,
                    zone,
                    parent,
                    expected,
                });
            }
            Control::ZoneDelete => {
                let zone = chosen.ok_or("choose a zone in the zone list first")?;
                return self.action(Action::DeleteZone {
                    plane,
                    zone,
                    expected,
                });
            }
            _ => Ok(()),
        }
        .map_err(|error| error.to_string())
    }

    /// The anchor list and editing rows. Edits leave as Session actions;
    /// the Session checks the grant, the view, the revision and every
    /// refusal again.
    fn anchor_control(&mut self, control: Control) -> Result<()> {
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let expected = self.session.history().document().revision();
        let (world, catalog) = (self.session.world(), self.session.catalog());
        let action = match control {
            Control::AnchorChoose(anchor) => {
                return self
                    .tools
                    .choose_anchor(world, catalog, &anchor)
                    .map_err(|error| error.to_string());
            }
            Control::AnchorCreate => {
                return self
                    .tools
                    .begin_anchor_create(catalog, self.session.selection(), expected, unavailable)
                    .map_err(|error| error.to_string());
            }
            Control::AnchorMove => self.tools.move_anchor_action(
                world,
                catalog,
                self.session.selection(),
                expected,
                unavailable,
            ),
            Control::AnchorDelete => {
                self.tools
                    .delete_anchor_action(world, catalog, expected, unavailable)
            }
            _ => return Ok(()),
        }
        .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Review the open new-anchor draft: its glyphs must all be in the
    /// font, as for field drafts, then it becomes one Session action.
    fn apply_anchor_draft(&mut self) -> Result<()> {
        let snapshot = self.tools.draft_snapshot().ok_or("no draft")?;
        let layout = self
            .text
            .layout(&snapshot.display_text, 18.0, None)
            .map_err(|error| error.to_string())?;
        if !layout.missing_glyphs().is_empty() {
            return Err("missing font glyphs: review blocked; draft preserved".into());
        }
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let action = self
            .tools
            .anchor_action(self.session.world(), self.session.catalog(), unavailable)
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// The selected object's portal rows. Edits leave as Session actions;
    /// the Session checks the grant, the object's view, the revision and
    /// the target again.
    fn portal_control(&mut self, control: Control) -> Result<()> {
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let expected = self.session.history().document().revision();
        let (world, catalog) = (self.session.world(), self.session.catalog());
        let selection = self.session.selection();
        let action = match control {
            Control::PortalAnchors | Control::PortalPick => {
                let step = if control == Control::PortalAnchors {
                    PortalStep::Anchors
                } else {
                    PortalStep::PickTile
                };
                return self
                    .tools
                    .toggle_portal_step(world, catalog, selection, step, expected, unavailable)
                    .map(|_| ())
                    .map_err(|error| error.to_string());
            }
            Control::PortalLocation => {
                return self
                    .tools
                    .begin_portal_location(world, catalog, selection, expected, unavailable)
                    .map_err(|error| error.to_string());
            }
            Control::PortalAnchor { plane, anchor } => self.tools.portal_anchor_action(
                world,
                catalog,
                selection,
                (&plane, &anchor),
                unavailable,
            ),
            Control::PortalClear => {
                self.tools
                    .clear_portal_action(world, catalog, selection, expected, unavailable)
            }
            _ => return Ok(()),
        }
        .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// A click on the view while the portal pick step is open: the tile
    /// under it, through the exact cell mask, becomes the target. A miss
    /// keeps the step open.
    fn pick_portal_target(&mut self, x: u32, y: u32) -> Result<()> {
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
            .ok_or("no tile at this pixel: click a tile of the view, or press Esc")?;
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let action = self
            .tools
            .portal_pick_action(
                self.session.world(),
                self.session.catalog(),
                self.session.selection(),
                &cell,
                unavailable,
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Review the open typed portal location: its glyphs must all be in
    /// the font, as for field drafts, then it becomes one Session action.
    fn apply_portal_draft(&mut self) -> Result<()> {
        let snapshot = self.tools.draft_snapshot().ok_or("no draft")?;
        let layout = self
            .text
            .layout(&snapshot.display_text, 18.0, None)
            .map_err(|error| error.to_string())?;
        if !layout.missing_glyphs().is_empty() {
            return Err("missing font glyphs: review blocked; draft preserved".into());
        }
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let action = self
            .tools
            .portal_location_action(
                self.session.world(),
                self.session.catalog(),
                self.session.selection(),
                unavailable,
            )
            .map_err(|error| error.to_string())?;
        self.action(action)
    }

    /// Review the open zone draft: its glyphs must all be in the font, as
    /// for field drafts, then it becomes one Session action.
    fn apply_zone_draft(&mut self) -> Result<()> {
        let snapshot = self.tools.draft_snapshot().ok_or("no draft")?;
        let layout = self
            .text
            .layout(&snapshot.display_text, 18.0, None)
            .map_err(|error| error.to_string())?;
        if !layout.missing_glyphs().is_empty() {
            return Err("missing font glyphs: review blocked; draft preserved".into());
        }
        let unavailable = self.blocked.is_some() || !self.session.can_prepare();
        let action = self
            .tools
            .zone_action(self.session.world(), self.session.catalog(), unavailable)
            .map_err(|error| error.to_string())?;
        self.action(action)
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
            "toolkit":"0.2.0","renderer":"0.7.1","pending":self.session.history().pending().is_some()
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
            // The scripted move and deletion leave the objects whose portal
            // the portal step edits alone.
            let granted = granted
                && (capability == Capability::Field || !catalog.can_edit_portal(world, uid));
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

    /// Press, drag and capture a 3x3 brush stroke mid-gesture with its live
    /// preview, then cancel it with Esc as a native user would: nothing is
    /// prepared and the World is unchanged. The stroke crosses terrain that
    /// already holds the chosen value, which the preview leaves unmarked.
    fn stroke_preview_demo(&mut self) -> Result<()> {
        let frame = self.frame(true)?;
        let pixel = |cell: [i64; 2]| {
            self.prepared
                .cell_picks
                .iter()
                .find_map(|(id, key)| match key {
                    SelectionKey::Cell { x, y, .. } if [*x, *y] == cell => pixel_for(&frame, *id),
                    _ => None,
                })
                .ok_or("demonstration stroke cell is not in the cell mask")
        };
        let (start, end) = (pixel(DEMO_STROKE[0])?, pixel(DEMO_STROKE[1])?);
        let brush = self.tools.brush();
        self.tools.set_brush(tilewright_editor_ui::brush::Brush {
            shape: tilewright_editor_ui::brush::BrushShape::Square,
            radius: 1,
        });
        let result = self
            .begin_stroke(start.0, start.1)
            .and_then(|()| self.stroke_sample(end.0, end.1))
            .and_then(|()| self.capture("stroke-preview"));
        if self.tools.stroke().is_some() {
            native_key(self, Keycode::Escape)?;
        }
        self.tools.set_brush(brush);
        // The cancellation notice is for an interactive author; later
        // scripted captures keep their plain status.
        self.notice = None;
        result
    }

    /// Zone mode: capture the overlay and legend, then paint the last zone
    /// of the palette down a column with a one-cell brush through the
    /// ordinary press, drag and release, capturing the live preview
    /// mid-gesture and the pending review, accept it and capture the result.
    fn zone_demo(&mut self) -> Result<()> {
        self.control(Control::Zone)?;
        self.capture("zones")?;
        let zone = tools::zone_palette(self.session.world(), self.session.catalog())
            .last()
            .map(|zone| (*zone).to_owned())
            .ok_or("the plane defines no zone to paint")?;
        self.control(Control::ZonePalette(ZoneChoice::Zone(zone)))?;
        let frame = self.frame(true)?;
        let pixel = |cell: [i64; 2]| {
            self.prepared
                .cell_picks
                .iter()
                .find_map(|(id, key)| match key {
                    SelectionKey::Cell { x, y, .. } if [*x, *y] == cell => pixel_for(&frame, *id),
                    _ => None,
                })
                .ok_or("demonstration zone cell is not in the cell mask")
        };
        let (start, end) = (pixel(DEMO_ZONE_STROKE[0])?, pixel(DEMO_ZONE_STROKE[1])?);
        let brush = self.tools.brush();
        self.tools
            .set_brush(tilewright_editor_ui::brush::Brush::default());
        let result = self
            .begin_stroke(start.0, start.1)
            .and_then(|()| self.stroke_sample(end.0, end.1))
            .and_then(|()| self.capture("zone-stroke-preview"))
            .and_then(|()| self.finish_stroke());
        self.tools.set_brush(brush);
        result?;
        self.capture("zone-stroke")?;
        self.action(Action::Accept)?;
        self.capture("zone-painted")
    }

    /// Zone management: rename a zone's display name, create a child zone
    /// and paint one tile with it, try to delete it while that tile holds
    /// it (the refusal is captured), undo the paint and delete it. Every
    /// edit is reviewed and accepted through the ordinary controls.
    fn zone_manage_demo(&mut self) -> Result<()> {
        self.control(Control::Zone)?;
        self.action(Action::ClearSelection)?;
        self.control(Control::ZonePalette(ZoneChoice::Zone(
            DEMO_RENAMED_ZONE.into(),
        )))?;
        self.control(Control::ZoneName)?;
        self.tools.select_all().map_err(|error| error.to_string())?;
        self.tools
            .input(DEMO_ZONE_NAME)
            .map_err(|error| error.to_string())?;
        self.control(Control::ApplyDraft)?;
        self.capture("zone-rename")?;
        self.action(Action::Accept)?;
        self.control(Control::ZoneCreate(Some(DEMO_RENAMED_ZONE.into())))?;
        self.tools
            .input(&format!("{DEMO_NEW_ZONE} {DEMO_NEW_ZONE_NAME}"))
            .map_err(|error| error.to_string())?;
        self.control(Control::ApplyDraft)?;
        self.capture("zone-create")?;
        self.action(Action::Accept)?;
        // The new zone is in the palette: paint one tile with it.
        self.control(Control::ZonePalette(ZoneChoice::Zone(DEMO_NEW_ZONE.into())))?;
        let frame = self.frame(true)?;
        let [x, y] = DEMO_NEW_ZONE_TILE;
        let at = self
            .prepared
            .cell_picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Cell { x: cx, y: cy, .. } if (*cx, *cy) == (x, y) => {
                    pixel_for(&frame, *id)
                }
                _ => None,
            })
            .ok_or("demonstration zone tile is not in the cell mask")?;
        let brush = self.tools.brush();
        self.tools
            .set_brush(tilewright_editor_ui::brush::Brush::default());
        let result = self
            .begin_stroke(at.0, at.1)
            .and_then(|()| self.finish_stroke());
        self.tools.set_brush(brush);
        result?;
        self.action(Action::Accept)?;
        self.capture("zone-created")?;
        // A tile holds it now: the window shows the Delete row disabled,
        // with the reasons under it, so a press on it does nothing. The
        // capture shows that state; the Session's own refusal is tested.
        self.refresh_ui();
        if self
            .ui
            .widgets
            .iter()
            .any(|widget| widget.control == Some(Control::ZoneDelete) && widget.enabled)
        {
            return Err("the Delete row of a used zone is enabled".into());
        }
        self.capture("zone-delete-refused")?;
        self.action(Action::Undo)?;
        self.action(Action::Accept)?;
        self.control(Control::ZoneDelete)?;
        self.capture("zone-delete")?;
        self.action(Action::Accept)?;
        self.capture("zone-deleted")
    }

    /// Select tile `at` of the view through the exact cell mask, as a click
    /// in Anchor mode does.
    fn select_tile(&mut self, [x, y]: [i64; 2]) -> Result<()> {
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
            .ok_or("demonstration anchor tile is not in the cell mask")?;
        self.click(hit.0, hit.1)
    }

    /// Anchor mode: capture the badges and the list, create an anchor on a
    /// selected tile, move it to another, show that the anchor a portal
    /// names cannot be deleted (the window's disabled row and its reasons
    /// are captured), then delete the new one. Every edit is reviewed and
    /// accepted through the ordinary controls.
    fn anchor_demo(&mut self) -> Result<()> {
        self.control(Control::Anchor)?;
        self.action(Action::ClearSelection)?;
        self.capture("anchors")?;
        self.select_tile(DEMO_ANCHOR_TILE)?;
        self.control(Control::AnchorCreate)?;
        self.tools
            .input(DEMO_NEW_ANCHOR)
            .map_err(|error| error.to_string())?;
        self.control(Control::ApplyDraft)?;
        self.capture("anchor-create")?;
        self.action(Action::Accept)?;
        self.capture("anchor-created")?;
        self.control(Control::AnchorChoose(DEMO_NEW_ANCHOR.into()))?;
        self.select_tile(DEMO_ANCHOR_MOVE)?;
        self.control(Control::AnchorMove)?;
        self.capture("anchor-move")?;
        self.action(Action::Accept)?;
        // A portal names this one: the Delete row is disabled, with the
        // portals listed above it, so a press on it does nothing. The
        // capture shows that state; the Session's own refusal is tested.
        self.control(Control::AnchorChoose(DEMO_NAMED_ANCHOR.into()))?;
        self.refresh_ui();
        if self
            .ui
            .widgets
            .iter()
            .any(|widget| widget.control == Some(Control::AnchorDelete) && widget.enabled)
        {
            return Err("the Delete row of an anchor a portal names is enabled".into());
        }
        self.capture("anchor-delete-refused")?;
        self.control(Control::AnchorChoose(DEMO_NEW_ANCHOR.into()))?;
        self.control(Control::AnchorDelete)?;
        self.capture("anchor-delete")?;
        self.action(Action::Accept)?;
        self.capture("anchor-deleted")
    }

    /// Select placed object `uid` through the exact object mask, in Select
    /// mode.
    fn select_object(&mut self, uid: &str) -> Result<()> {
        let frame = self.frame(false)?;
        let hit = self
            .prepared
            .picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Object { uid: found } if found == uid => pixel_for(&frame, *id),
                _ => None,
            })
            .ok_or("demonstration object is not in the object mask")?;
        self.tools.set_mode(ToolMode::Select);
        self.click(hit.0, hit.1)
    }

    /// In the open pick step, click tile `at` through the exact cell mask.
    fn pick_portal_tile(&mut self, [x, y]: [i64; 2]) -> Result<()> {
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
            .ok_or("demonstration portal tile is not in the cell mask")?;
        self.pick_portal_target(hit.0, hit.1)
    }

    /// Portals: select a gate, open its anchor list and retarget it to
    /// another anchor; select the other gate and pick a target tile by
    /// click, first its own tile (reviewed with a warning, then cancelled)
    /// and then another one; then clear that portal. Every edit is
    /// reviewed and accepted through the ordinary controls.
    fn portal_demo(&mut self) -> Result<()> {
        self.select_object(DEMO_RETARGETED_GATE)?;
        self.capture("portal")?;
        self.control(Control::PortalAnchors)?;
        self.capture("portal-anchors")?;
        self.control(Control::PortalAnchor {
            plane: self.session.catalog().view().0.into(),
            anchor: DEMO_TARGET_ANCHOR.into(),
        })?;
        self.capture("portal-retarget")?;
        self.action(Action::Accept)?;
        self.capture("portal-retargeted")?;
        self.select_object(DEMO_PICKED_GATE)?;
        let own = self
            .session
            .world()
            .object(DEMO_PICKED_GATE)
            .and_then(|object| object.at)
            .map(|[x, y]| [i64::from(x), i64::from(y)])
            .ok_or("the demonstration gate is not placed")?;
        self.control(Control::PortalPick)?;
        self.capture("portal-pick")?;
        self.pick_portal_tile(own)?;
        self.capture("portal-tile-warning")?;
        self.action(Action::Cancel)?;
        self.control(Control::PortalPick)?;
        self.pick_portal_tile(DEMO_PORTAL_TILE)?;
        self.capture("portal-tile")?;
        self.action(Action::Accept)?;
        self.capture("portal-tile-set")?;
        self.control(Control::PortalClear)?;
        self.capture("portal-clear")?;
        self.action(Action::Accept)?;
        self.capture("portal-cleared")
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
                self.stroke_preview_demo()?;
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
            10 if self.session.catalog().editing().zones.paint => self.zone_demo()?,
            11 if self.session.catalog().editing().zones.manage
                && self.session.catalog().editing().zones.paint =>
            {
                self.zone_manage_demo()?;
            }
            12 if self.session.catalog().editing().anchors.edit => self.anchor_demo()?,
            13 if grants_portals(self.session.catalog().editing()) => self.portal_demo()?,
            14 => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn demo_all(&mut self) -> Result<()> {
        for step in 0..15 {
            if self.demo_step(step)? {
                break;
            }
        }
        Ok(())
    }
}

/// Push a row naming zone `id` and record how to fit it: `variants` are
/// the `(prefix, suffix)` pairs around the id, widest first.
fn id_row(
    rows: &mut UiRows,
    fits: &mut Vec<(usize, IdText)>,
    id: &str,
    variants: Vec<(String, String)>,
    control: Option<Control>,
    enabled: bool,
    active: bool,
) {
    let (prefix, suffix) = variants[0].clone();
    fits.push((
        rows.len(),
        IdText::Id {
            id: id.into(),
            variants,
        },
    ));
    rows.push((format!("{prefix}{id}{suffix}"), control, enabled, active));
}

/// A stored text value on one line, its line breaks and controls as markers.
fn stored_breaks_marked(text: &str) -> String {
    tilewright_editor_ui::text_input::mark_stored_breaks(text).0
}

/// A row counting the markers in a stored text value, when it has any.
fn stored_breaks_row(
    row: &inspector::FieldRow<'_>,
) -> Option<(String, Option<Control>, bool, bool)> {
    let Some(tilewright_source::FieldValue::String(text)) = row.current else {
        return None;
    };
    tilewright_editor_ui::text_input::stored_break_hint(text)
        .map(|hint| (hint, None, row.editable, false))
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

/// Whether any `editing.objects` entry grants portal editing.
fn grants_portals(editing: &tilewright_editor_preview::EditingCapabilities) -> bool {
    editing.objects.values().any(|grant| grant.portal)
}

/// Labels are logged after successful presentation; this does not own native handles.
fn demo_phase_label(app: &Application, step: usize) -> Option<&'static str> {
    let editing = app.session.catalog().editing();
    if (step == 5 && !grants_fields(editing))
        || (step == 6 && editing.terrain.is_empty())
        || ((7..=9).contains(&step) && editing.objects.is_empty())
        || (step == 10 && !editing.zones.paint)
        || (step == 11 && !(editing.zones.paint && editing.zones.manage))
        || (step == 12 && !editing.anchors.edit)
        || (step == 13 && !grants_portals(editing))
    {
        None
    } else {
        [
            "before",
            "pending",
            "after",
            "undo",
            "redo",
            "field",
            "terrain",
            "delete",
            "place",
            "palette",
            "zones",
            "zone-manage",
            "anchors",
            "portals",
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
        _ => native_input_event(
            app,
            event,
            window.size(),
            window.size_in_pixels(),
            *minimized,
            extend_selection,
        )
        .unwrap_or(Ok(())),
    }
}

/// Pointer and key routing that needs no window handle, so tests can drive
/// it with real SDL event values: presses, stroke drags and releases, and
/// world keys. `None` for every other event.
fn native_input_event(
    app: &mut Application,
    event: &Event,
    logical: (u32, u32),
    physical: (u32, u32),
    minimized: bool,
    extend_selection: bool,
) -> Option<Result<()>> {
    Some(match event {
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            ..
        } if !minimized => native_pointer_event(app, event, logical, physical, extend_selection),
        Event::MouseMotion {
            mousestate, x, y, ..
        } if !minimized && app.tools.stroke().is_some() => {
            native_stroke_motion(app, [*x, *y], mousestate.left(), logical, physical)
        }
        Event::MouseButtonUp {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } if app.tools.stroke().is_some() => {
            native_stroke_release(app, [*x, *y], logical, physical)
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
                return Some(Err(
                    "accept waits for the current displayed pending diff".into()
                ));
            }
            native_key(app, *key)
        }
        _ => return None,
    })
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
        Hit::World(x, y) if app.tools.portal_picking() => app.pick_portal_target(x, y),
        Hit::World(x, y) => {
            app.tools.cancel();
            if matches!(app.tools.mode(), ToolMode::Terrain | ToolMode::Zone) {
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
    // Esc ends an open portal step, then clears a palette filter, before
    // it cancels a pending edit.
    if key == Keycode::Escape && app.tools.retire_portal() {
        app.presented_generation = None;
        return Ok(());
    }
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
        Keycode::N => app.control(Control::Zone),
        Keycode::K => app.control(Control::Anchor),
        Keycode::O => app.control(Control::ZoneOverlay),
        Keycode::Slash | Keycode::KpDivide if app.tools.mode() == ToolMode::Place => {
            app.control(Control::PaletteFilter)
        }
        Keycode::LeftBracket if brushing(app) => app.control(Control::BrushSize(-1)),
        Keycode::RightBracket if brushing(app) => app.control(Control::BrushSize(1)),
        Keycode::B if brushing(app) => app.control(Control::BrushShape),
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

/// Whether the brush keys apply: Terrain mode, or Zone mode with the paint
/// grant.
fn brushing(app: &Application) -> bool {
    match app.tools.mode() {
        ToolMode::Terrain => true,
        ToolMode::Zone => app.session.catalog().editing().zones.paint,
        _ => false,
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

/// The cells a running stroke's release would change, from the same
/// Session computation that prepares the release, when the stroke belongs
/// to the current frame. `None` without a stroke, or when the release would
/// be refused.
fn stroke_changes(app: &Application) -> Option<Vec<[i64; 2]>> {
    let stroke = app.tools.stroke()?;
    if stroke.generation() != app.view.frame_revision
        || app.prepared.frame_revision != app.view.frame_revision
    {
        return None;
    }
    app.session.stroke_changes(stroke).ok()
}

/// The live preview of the running brush stroke, drawn on the cached
/// prepared cell scene of the stroke's own frame. A terrain stroke marks
/// the cells its release would change in white. A zone stroke previews its
/// result instead: those cells in the target zone's tint at
/// [`zones::PREVIEW_ALPHA`], twice the resting overlay's opacity (the
/// default-zone row shows the plane default's tint, or no tint without a
/// default), and [`zone_overlay`] leaves those cells out, so the preview
/// replaces their resting tint rather than stacking on it.
fn stroke_overlay(app: &Application) -> Vec<tilewright_scene::Quad> {
    let (Some(stroke), Some(changes)) = (app.tools.stroke(), stroke_changes(app)) else {
        return Vec::new();
    };
    if let StrokeTarget::Zone(zone) = stroke.target() {
        let default = app
            .session
            .history()
            .document()
            .world()
            .planes()
            .get(stroke.plane())
            .and_then(|plane| plane.default_zone.clone());
        let Some(shown) = zone.clone().or(default) else {
            return Vec::new();
        };
        let tints = app.zone_tints();
        let color = zones::preview_color(
            tints
                .get(&shown)
                .copied()
                .unwrap_or_else(|| zones::preferred_tint(&shown)),
        );
        let cells = changes.into_iter().map(|at| (at, color)).collect();
        return crate::workspace_render::zone_overlay(&app.prepared, &cells);
    }
    let cells = changes
        .into_iter()
        .map(|[x, y]| SelectionKey::Cell {
            plane: stroke.plane().into(),
            layer: stroke.layer(),
            grid: stroke.grid().into(),
            x,
            y,
        })
        .collect();
    crate::workspace_render::stroke_overlay(&app.prepared, &cells)
}

/// The zone overlay: every in-view tile that has a zone, tinted with its
/// zone's tint, over the cached prepared cell scene. Empty unless zones are
/// shown (always in Zone mode; after O elsewhere). Cells a running zone
/// stroke previews are left to [`stroke_overlay`].
fn zone_overlay(app: &Application) -> Vec<tilewright_scene::Quad> {
    if !app.tools.zones_visible() || app.prepared.frame_revision != app.view.frame_revision {
        return Vec::new();
    }
    let previewed: std::collections::BTreeSet<[i64; 2]> = app
        .tools
        .stroke()
        .filter(|stroke| matches!(stroke.target(), StrokeTarget::Zone(_)))
        .and_then(|_| stroke_changes(app))
        .unwrap_or_default()
        .into_iter()
        .collect();
    let tints = app.zone_tints();
    let cells = app
        .view_zone_tiles()
        .iter()
        .filter(|(at, _)| !previewed.contains(at))
        .map(|(at, zone)| {
            let tint = tints
                .get(zone)
                .copied()
                .unwrap_or_else(|| zones::preferred_tint(zone));
            (*at, zones::tint_color(tint))
        })
        .collect();
    crate::workspace_render::zone_overlay(&app.prepared, &cells)
}

/// The anchor and portal badges of the in-view tiles, over the cached
/// prepared cell scene: shown in Anchor mode, and wherever the zone overlay
/// shows. Anchors on other layers or out of view are listed, not drawn.
fn badge_overlay(app: &Application) -> Vec<tilewright_scene::Quad> {
    if !app.tools.badges_visible() || app.prepared.frame_revision != app.view.frame_revision {
        return Vec::new();
    }
    let facts = app.anchor_view();
    let shape = match app.session.world().profile().projection {
        tilewright_source::Projection::Isometric { .. } => {
            crate::workspace_render::CellShape::Diamond
        }
        tilewright_source::Projection::TopDown { .. } => crate::workspace_render::CellShape::Square,
    };
    crate::workspace_render::badge_overlay(
        &app.prepared,
        &facts.anchor_tiles,
        &facts.portal_tiles,
        shape,
    )
}

/// Compose the cached prepared World with `overlay` and the interface. If
/// the overlay would take the frame past the scene limits, compose without
/// it and report that it was left out; the release still reviews every
/// cell. Returns the presentation and whether the overlay was left out.
fn compose_with_preview(
    system: &mut tilewright_render_ui::TextSystem,
    ui: &WorkspaceUi,
    prepared: &PreparedPreview,
    overlay: &[tilewright_scene::Quad],
    limits: tilewright_scene::SceneLimits,
) -> std::result::Result<
    (crate::workspace_render::Presentation, bool),
    crate::workspace_render::CompositionError,
> {
    let compose = |system: &mut tilewright_render_ui::TextSystem, overlay| {
        crate::workspace_render::compose_with_overlay(
            system,
            ui,
            &prepared.scene,
            prepared.resources.clone(),
            overlay,
            limits,
        )
    };
    match compose(system, overlay) {
        Err(crate::workspace_render::CompositionError::Scene(
            tilewright_scene::Error::LimitExceeded,
        )) if !overlay.is_empty() => compose(system, &[]).map(|presentation| (presentation, true)),
        result => result.map(|presentation| (presentation, false)),
    }
}

fn workspace_presentation(app: &mut Application) -> Result<crate::workspace_render::Presentation> {
    // Zone tints and the stroke preview take the World's cell slots: each
    // copy draws right after its tile's terrain, and where a zone tint and
    // a preview mark share a slot they draw in the order passed here, so
    // the preview, passed last, draws over the tint. Badges take no cell
    // slot: they sit in their own slot after the last World quad, above
    // every tile and object, whatever order they are passed in.
    let mut overlay = zone_overlay(app);
    overlay.extend(badge_overlay(app));
    overlay.extend(stroke_overlay(app));
    let (mut presentation, mut omitted) = compose_with_preview(
        &mut app.text,
        &app.ui,
        &app.prepared,
        &overlay,
        app.frame_limits,
    )
    .map_err(|error| error.to_string())?;
    if app.unsupported_runs != presentation.unsupported_runs || app.preview_omitted != omitted {
        app.unsupported_runs = presentation.unsupported_runs;
        app.preview_omitted = omitted;
        app.refresh_ui();
        (presentation, omitted) = compose_with_preview(
            &mut app.text,
            &app.ui,
            &app.prepared,
            &overlay,
            app.frame_limits,
        )
        .map_err(|error| error.to_string())?;
        app.preview_omitted = omitted;
    }
    Ok(presentation)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "tests/prototype.rs"]
mod tests;
