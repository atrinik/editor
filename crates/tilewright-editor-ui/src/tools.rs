// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Local input only; every edit leaves this module as a Session action.
use crate::brush::{Brush, Stroke, StrokeTarget};
use crate::text_input::{TextInput, TextInputSnapshot, TextRevision, TextSelection};
use crate::{
    Action, SelectionKey,
    inspector::{
        Inspector, MAX_CHOICES, MAX_INPUT_BYTES, SelectionSummary, SnapshotStamp, scalar_text,
    },
};
use serde_json::Value;
use std::fmt;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::{FieldType, World};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToolMode {
    #[default]
    Select,
    Move,
    Terrain,
    /// Click a tile to place the chosen prototype there.
    Place,
    /// Show every tile's zone; drag to paint the chosen zone when the
    /// catalog grants `editing.zones.paint`.
    Zone,
    /// Show anchor and portal badges and the plane's anchors; a click
    /// selects a tile. With `editing.anchors.edit`, create an anchor on the
    /// selected tile, move the chosen one there, or delete an unreferenced
    /// one.
    Anchor,
}

/// What the zone brush paints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZoneChoice {
    /// A zone of the active view's plane.
    Zone(String),
    /// Empty the cells, so they show the plane's default zone again.
    Default,
}
impl ZoneChoice {
    /// The `zone` cell value this choice writes; `None` empties the cell.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Zone(zone) => Some(zone),
            Self::Default => None,
        }
    }
}

/// The zones of the active view's plane, sorted by id: the zone palette.
/// It lists the plane's definitions whether or not painting is granted.
pub fn zone_palette<'w>(world: &'w World, catalog: &PreviewCatalog) -> Vec<&'w str> {
    let (plane, _, _) = catalog.view();
    world.planes().get(plane).map_or_else(Vec::new, |plane| {
        plane
            .zones
            .keys()
            .map(String::as_str)
            .take(MAX_CHOICES)
            .collect()
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainChoice {
    pub grid: String,
    pub prototype: String,
}

/// Only explicit capabilities appear; palette selection never implies placement.
pub fn terrain_palette(catalog: &PreviewCatalog) -> Vec<TerrainChoice> {
    catalog
        .editing()
        .terrain
        .iter()
        .flat_map(|(grid, prototypes)| {
            prototypes.iter().map(move |prototype| TerrainChoice {
                grid: grid.clone(),
                prototype: prototype.clone(),
            })
        })
        .take(MAX_CHOICES)
        .collect()
}

/// The prototypes `editing.objects` lets the author place, sorted by
/// prototype id. Only explicit grants appear.
pub fn object_palette(catalog: &PreviewCatalog) -> Vec<&str> {
    catalog
        .editing()
        .objects
        .keys()
        .map(String::as_str)
        .take(MAX_CHOICES)
        .collect()
}

/// Most UTF-8 bytes the Place palette filter holds.
pub const MAX_FILTER_BYTES: usize = 64;

/// Whether the palette filter `filter` keeps `prototype`: its id contains
/// the filter text, ignoring case. Both sides are folded one character at a
/// time with Unicode's full lowercase mapping (`char::to_lowercase`), with
/// no locale tailoring, no context (capital sigma always becomes `σ`, never
/// final `ς`) and no normalization, so precomposed and decomposed accents do
/// not match each other. An empty filter keeps everything.
pub fn palette_matches(prototype: &str, filter: &str) -> bool {
    let fold = |text: &str| {
        text.chars()
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    fold(prototype).contains(&fold(filter))
}

#[derive(Clone, Debug)]
enum DraftKind {
    Field(String),
    Move,
    /// The Place palette filter. Local view state only: it is bound to no
    /// selection or World revision and never becomes an action.
    Filter,
    /// The display name of `zone`, reviewed as [`Action::RenameZone`]
    /// against source revision `expected`.
    ZoneName {
        plane: String,
        zone: String,
        expected: [u8; 32],
    },
    /// A new zone under `parent`: its id, then optionally a space and its
    /// display name, reviewed as [`Action::CreateZone`].
    ZoneCreate {
        plane: String,
        parent: Option<String>,
        expected: [u8; 32],
    },
    /// The id of a new anchor on plane tile `at` of the active view,
    /// reviewed as [`Action::CreateAnchor`].
    AnchorCreate {
        plane: String,
        at: [i64; 2],
        expected: [u8; 32],
    },
    /// A portal target typed as `layer x y` (the object's own plane `own`)
    /// or `plane layer x y`, for object `uid`, reviewed as
    /// [`Action::SetPortal`].
    PortalLocation {
        uid: String,
        own: String,
        expected: [u8; 32],
    },
}

/// Which portal step is open for the selected object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalStep {
    /// The list of anchors the portal may target.
    Anchors,
    /// The next click on a tile of the view becomes the target.
    PickTile,
}

/// An open portal step: for which object and source revision.
#[derive(Clone, Debug)]
struct OpenPortalStep {
    uid: String,
    expected: [u8; 32],
    step: PortalStep,
}

/// Most bytes a typed portal location holds: a plane id and three numbers.
const MAX_PORTAL_LOCATION_BYTES: usize = 128 + 3 * 21;
/// The largest plane coordinate World accepts: 2^53 - 1.
const MAX_SAFE_COORDINATE: i64 = (1 << 53) - 1;

/// Most bytes a zone name draft holds: 256 characters of up to four bytes.
const MAX_ZONE_NAME_BYTES: usize = 4 * MAX_ZONE_NAME_CHARACTERS;
/// Most bytes a new-zone draft holds: an id, a space and a name.
const MAX_ZONE_CREATE_BYTES: usize = 128 + 1 + MAX_ZONE_NAME_BYTES;
/// Most characters of a zone display name (FORMAT.md section 8).
pub const MAX_ZONE_NAME_CHARACTERS: usize = 256;
/// Most bytes a new-anchor draft holds: a World identifier.
const MAX_ANCHOR_ID_BYTES: usize = 128;

/// Which zone draft is open, for the hint shown with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZoneDraft {
    Name,
    Create,
}
#[derive(Clone, Debug)]
struct Draft {
    /// The selection and revision a field or move draft was begun for;
    /// `None` for the palette filter.
    stamp: Option<SnapshotStamp>,
    kind: DraftKind,
    input: TextInput,
}

#[derive(Clone, Debug, Default)]
pub struct ToolState {
    mode: ToolMode,
    terrain: Option<TerrainChoice>,
    placement: Option<String>,
    /// The palette filter's committed text while it is not focused. While it
    /// is focused, the focused draft holds the text.
    filter: String,
    draft: Option<Draft>,
    next_draft: u64,
    brush: Brush,
    stroke: Option<Stroke>,
    zone: Option<ZoneChoice>,
    /// Whether the zone overlay shows outside Zone mode.
    show_zones: bool,
    /// The chosen zone's parent choices while they are listed, computed
    /// when the list opens and again when the definitions change.
    parent_choices: Option<ParentChoices>,
    /// The anchor chosen in Anchor mode's list.
    anchor: Option<String>,
    /// The open portal step of the selected object, if any.
    portal: Option<OpenPortalStep>,
}

/// The zones that may become the chosen zone's parent: the first
/// [`MAX_CHOICES`] in id order, and how many there are in all.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParentChoices {
    pub zones: Vec<String>,
    pub total: usize,
}
impl ParentChoices {
    fn of(world: &World, catalog: &PreviewCatalog, zone: &str) -> Self {
        let (plane, _, _) = catalog.view();
        let all = world
            .planes()
            .get(plane)
            .map_or_else(Vec::new, |plane| crate::zones::parent_choices(plane, zone));
        Self {
            total: all.len(),
            zones: all
                .into_iter()
                .take(MAX_CHOICES)
                .map(str::to_owned)
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolError(pub &'static str);
impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ToolError {}

impl ToolState {
    pub fn mode(&self) -> ToolMode {
        self.mode
    }
    /// Switching tools ends any draft. Leaving Place mode also clears the
    /// palette filter, so Place always opens with the whole palette.
    pub fn set_mode(&mut self, mode: ToolMode) {
        self.cancel();
        self.parent_choices = None;
        if mode != ToolMode::Place {
            self.filter.clear();
        }
        self.mode = mode;
    }
    pub fn brush(&self) -> Brush {
        self.brush
    }
    /// A running stroke keeps the brush it began with.
    pub fn set_brush(&mut self, brush: Brush) {
        self.brush = brush.resized(0);
    }
    pub fn zone_choice(&self) -> Option<&ZoneChoice> {
        self.zone.as_ref()
    }
    /// Choose what the zone brush paints: a zone of the view's plane, or
    /// the default zone. Painting itself still needs the catalog grant.
    pub fn choose_zone(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        choice: ZoneChoice,
    ) -> Result<(), ToolError> {
        if let ZoneChoice::Zone(zone) = &choice
            && !zone_palette(world, catalog).contains(&zone.as_str())
        {
            return Err(ToolError("The zone is not defined in this plane"));
        }
        if self.zone.as_ref() != Some(&choice) {
            // A zone draft and the parent list belong to the zone they were
            // opened for; another choice ends them.
            self.parent_choices = None;
            self.retire_zone_draft();
        }
        self.zone = Some(choice);
        Ok(())
    }
    /// Keep the zone state true to the shown World after it changed: a
    /// chosen zone that is no longer defined is no longer chosen (and its
    /// draft and parent list end), and an open parent list is computed
    /// again from the current definitions.
    pub fn revalidate_zones(&mut self, world: &World, catalog: &PreviewCatalog) {
        if let Some(ZoneChoice::Zone(zone)) = &self.zone
            && !zone_palette(world, catalog).contains(&zone.as_str())
        {
            self.zone = None;
            self.parent_choices = None;
            self.retire_zone_draft();
        }
        if self.parent_choices.is_some()
            && let Some(ZoneChoice::Zone(zone)) = &self.zone
        {
            self.parent_choices = Some(ParentChoices::of(world, catalog, zone));
        }
    }
    /// End an open zone-name or new-zone draft; other drafts stay.
    fn retire_zone_draft(&mut self) {
        if self.zone_draft().is_some() {
            self.draft = None;
        }
    }
    /// Whether the chosen zone's parent choices are listed.
    pub fn parent_choices_open(&self) -> bool {
        self.parent_choices.is_some()
    }
    /// The listed parent choices, when the list is open.
    pub fn parent_choices(&self) -> Option<&ParentChoices> {
        self.parent_choices.as_ref()
    }
    /// List or hide the chosen zone's parent choices. Needs zone management
    /// and a chosen zone; choosing another zone hides them again.
    pub fn toggle_parent_choices(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
    ) -> Result<bool, ToolError> {
        let zone = self.managed_zone(world, catalog)?;
        self.parent_choices = if self.parent_choices.is_some() {
            None
        } else {
            Some(ParentChoices::of(world, catalog, &zone))
        };
        Ok(self.parent_choices.is_some())
    }
    /// The chosen zone, when it is a zone of the active view's plane and
    /// zone management is granted.
    fn managed_zone(&self, world: &World, catalog: &PreviewCatalog) -> Result<String, ToolError> {
        if !catalog.editing().zones.manage {
            return Err(ToolError(
                "The catalog does not allow managing zones (editing.zones.manage)",
            ));
        }
        match &self.zone {
            Some(ZoneChoice::Zone(zone))
                if zone_palette(world, catalog).contains(&zone.as_str()) =>
            {
                Ok(zone.clone())
            }
            Some(ZoneChoice::Zone(_)) => Err(ToolError("The chosen zone is no longer defined")),
            _ => Err(ToolError("Choose a zone in the zone list first")),
        }
    }
    /// Begin editing the chosen zone's display name, starting from its
    /// current name (empty when it has none). Review sends an empty draft as
    /// no name.
    pub fn begin_zone_name(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let zone = self.managed_zone(world, catalog)?;
        let (plane, _, _) = catalog.view();
        let name = world
            .planes()
            .get(plane)
            .and_then(|plane| plane.zones.get(&zone))
            .and_then(|zone| zone.name.clone())
            .unwrap_or_default();
        let input = self.new_limited_input(name, MAX_ZONE_NAME_BYTES)?;
        self.retire_draft();
        self.draft = Some(Draft {
            stamp: None,
            kind: DraftKind::ZoneName {
                plane: plane.into(),
                zone,
                expected,
            },
            input,
        });
        Ok(())
    }
    /// Begin typing a new zone under `parent` (`None`: at the top level).
    /// `parent` must be a zone of the plane.
    pub fn begin_zone_create(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        parent: Option<String>,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        if !catalog.editing().zones.manage {
            return Err(ToolError(
                "The catalog does not allow managing zones (editing.zones.manage)",
            ));
        }
        if let Some(parent) = &parent
            && !zone_palette(world, catalog).contains(&parent.as_str())
        {
            return Err(ToolError("The parent zone is not defined in this plane"));
        }
        let (plane, _, _) = catalog.view();
        let input = self.new_limited_input(String::new(), MAX_ZONE_CREATE_BYTES)?;
        self.retire_draft();
        self.draft = Some(Draft {
            stamp: None,
            kind: DraftKind::ZoneCreate {
                plane: plane.into(),
                parent,
                expected,
            },
            input,
        });
        Ok(())
    }
    /// The zone an open zone draft is for: the zone a name draft renames,
    /// or the parent a new zone goes under (`None`: the top level).
    pub fn zone_draft_target(&self) -> Option<(ZoneDraft, Option<&str>)> {
        match self.draft.as_ref().map(|draft| &draft.kind) {
            Some(DraftKind::ZoneName { zone, .. }) => Some((ZoneDraft::Name, Some(zone))),
            Some(DraftKind::ZoneCreate { parent, .. }) => {
                Some((ZoneDraft::Create, parent.as_deref()))
            }
            _ => None,
        }
    }
    /// Which zone draft is open, if one is.
    pub fn zone_draft(&self) -> Option<ZoneDraft> {
        match self.draft.as_ref().map(|draft| &draft.kind) {
            Some(DraftKind::ZoneName { .. }) => Some(ZoneDraft::Name),
            Some(DraftKind::ZoneCreate { .. }) => Some(ZoneDraft::Create),
            _ => None,
        }
    }
    /// The reviewed action of the open zone draft. A name draft gives
    /// [`Action::RenameZone`] (empty: no name); a new-zone draft is the id,
    /// then optionally one space and the display name, and gives
    /// [`Action::CreateZone`]. Identifier shape, a used id and the name's
    /// length are checked here so the draft stays open to fix them; the
    /// session checks them again.
    pub fn zone_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let draft = self.draft.as_ref().ok_or(ToolError("No input draft"))?;
        if draft.input.is_composing() {
            return Err(ToolError("Finish or cancel composition before review"));
        }
        let text = draft.input.text();
        let checked_name = |name: &str| {
            if name.chars().count() > MAX_ZONE_NAME_CHARACTERS {
                Err(ToolError(
                    "A zone's display name has at most 256 characters",
                ))
            } else {
                Ok((!name.is_empty()).then(|| name.to_owned()))
            }
        };
        match &draft.kind {
            DraftKind::ZoneName {
                plane,
                zone,
                expected,
            } => Ok(Action::RenameZone {
                plane: plane.clone(),
                zone: zone.clone(),
                name: checked_name(text)?,
                expected: *expected,
            }),
            DraftKind::ZoneCreate {
                plane,
                parent,
                expected,
            } => {
                // The name part is trimmed, and only whitespace is no
                // name. (A name draft keeps its text exactly, like every
                // field draft.)
                let (id, name) = text.split_once(' ').unwrap_or((text, ""));
                let name = name.trim();
                if id.is_empty() {
                    return Err(ToolError("Type the new zone's id first"));
                }
                if !tilewright_source::is_identifier(id) {
                    return Err(ToolError(
                        "A zone id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
                    ));
                }
                if world
                    .planes()
                    .get(plane)
                    .is_some_and(|plane| plane.zones.contains_key(id))
                {
                    return Err(ToolError("This plane already has a zone with this id"));
                }
                if catalog.view().0 != plane {
                    return Err(ToolError("The view moved to another plane"));
                }
                Ok(Action::CreateZone {
                    plane: plane.clone(),
                    zone: id.to_owned(),
                    name: checked_name(name)?,
                    parent: parent.clone(),
                    expected: *expected,
                })
            }
            _ => Err(ToolError("No zone draft is open")),
        }
    }
    /// Whether the zone overlay is drawn: always in Zone mode, and in the
    /// other modes only after [`Self::toggle_zones`] turned it on.
    pub fn zones_visible(&self) -> bool {
        self.mode == ToolMode::Zone || self.show_zones
    }
    /// Show or hide the zone overlay outside Zone mode. In Zone mode it is
    /// always shown, so nothing changes there.
    pub fn toggle_zones(&mut self) -> Result<bool, ToolError> {
        if self.mode == ToolMode::Zone {
            return Err(ToolError("Zones always show in Zone mode"));
        }
        self.show_zones = !self.show_zones;
        Ok(self.show_zones)
    }
    /// Whether anchor and portal badges are drawn: always in Anchor mode,
    /// and in the other modes whenever the zone overlay shows (Zone mode,
    /// or after [`Self::toggle_zones`]).
    pub fn badges_visible(&self) -> bool {
        self.mode == ToolMode::Anchor || self.zones_visible()
    }
    pub fn anchor_choice(&self) -> Option<&str> {
        self.anchor.as_deref()
    }
    /// Choose an anchor of the active view's plane in the anchor list.
    /// Editing it still needs the catalog grant.
    pub fn choose_anchor(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        anchor: &str,
    ) -> Result<(), ToolError> {
        if !defined_anchor(world, catalog, anchor) {
            return Err(ToolError("The anchor is not defined in this plane"));
        }
        self.anchor = Some(anchor.to_owned());
        Ok(())
    }
    /// Keep the anchor state true to the shown World after it changed: a
    /// chosen anchor that is no longer defined is no longer chosen.
    pub fn revalidate_anchors(&mut self, world: &World, catalog: &PreviewCatalog) {
        if self
            .anchor
            .as_deref()
            .is_some_and(|anchor| !defined_anchor(world, catalog, anchor))
        {
            self.anchor = None;
        }
    }
    /// Begin typing the id of a new anchor on the selected tile, which must
    /// be a tile of the active view. Needs `editing.anchors.edit`.
    pub fn begin_anchor_create(
        &mut self,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let at = anchor_target(catalog, selection)?;
        let input = self.new_limited_input(String::new(), MAX_ANCHOR_ID_BYTES)?;
        self.retire_draft();
        self.draft = Some(Draft {
            stamp: None,
            kind: DraftKind::AnchorCreate {
                plane: catalog.view().0.into(),
                at,
                expected,
            },
            input,
        });
        Ok(())
    }
    /// The tile an open new-anchor draft is for.
    pub fn anchor_draft(&self) -> Option<[i64; 2]> {
        match self.draft.as_ref().map(|draft| &draft.kind) {
            Some(DraftKind::AnchorCreate { at, .. }) => Some(*at),
            _ => None,
        }
    }
    /// The reviewed action of the open new-anchor draft. The id's shape
    /// and whether the plane uses it already are checked here so the draft
    /// stays open to fix them; the session checks them again.
    pub fn anchor_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let draft = self.draft.as_ref().ok_or(ToolError("No input draft"))?;
        if draft.input.is_composing() {
            return Err(ToolError("Finish or cancel composition before review"));
        }
        let DraftKind::AnchorCreate {
            plane,
            at: [x, y],
            expected,
        } = &draft.kind
        else {
            return Err(ToolError("No new-anchor draft is open"));
        };
        let id = draft.input.text();
        if id.is_empty() {
            return Err(ToolError("Type the new anchor's id first"));
        }
        if !tilewright_source::is_identifier(id) {
            return Err(ToolError(
                "An anchor id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
            ));
        }
        if world
            .planes()
            .get(plane)
            .is_some_and(|source| source.anchors.contains_key(id))
        {
            return Err(ToolError("This plane already has an anchor with this id"));
        }
        if catalog.view().0 != plane {
            return Err(ToolError("The view moved to another plane"));
        }
        Ok(Action::CreateAnchor {
            plane: plane.clone(),
            anchor: id.to_owned(),
            x: *x,
            y: *y,
            expected: *expected,
        })
    }
    /// Move the chosen anchor to the selected tile. The anchor must be on a
    /// tile of the active view, and the selected tile another one.
    pub fn move_anchor_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let (anchor, current) = self.editable_anchor(world, catalog)?;
        let (plane, _, _) = catalog.view();
        if !catalog.tile_in_active_view(plane, current.layer, current.at[0], current.at[1]) {
            return Err(ToolError(
                "Only an anchor on a tile of the view can be moved",
            ));
        }
        let [x, y] = anchor_target(catalog, selection)?;
        if current.at == [x, y] {
            return Err(ToolError(
                "The anchor is already on the selected tile: select its new tile",
            ));
        }
        Ok(Action::MoveAnchor {
            plane: plane.into(),
            anchor,
            x,
            y,
            expected,
        })
    }
    /// Delete the chosen anchor. The session refuses it while a portal
    /// names it, with the objects listed.
    pub fn delete_anchor_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let (anchor, _) = self.editable_anchor(world, catalog)?;
        Ok(Action::DeleteAnchor {
            plane: catalog.view().0.into(),
            anchor,
            expected,
        })
    }
    /// The chosen anchor and its place, when anchor editing is granted.
    fn editable_anchor(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
    ) -> Result<(String, tilewright_source::Anchor), ToolError> {
        if !catalog.editing().anchors.edit {
            return Err(ToolError(
                "The catalog does not allow editing anchors (editing.anchors.edit)",
            ));
        }
        let anchor = self
            .anchor
            .as_ref()
            .ok_or(ToolError("Choose an anchor in the anchor list first"))?;
        let current = world
            .planes()
            .get(catalog.view().0)
            .and_then(|source| source.anchors.get(anchor))
            .ok_or(ToolError("The chosen anchor is no longer defined"))?;
        Ok((anchor.clone(), *current))
    }
    /// The open portal step and the object it is for.
    pub fn portal_step(&self) -> Option<(&str, PortalStep)> {
        self.portal
            .as_ref()
            .map(|open| (open.uid.as_str(), open.step))
    }
    /// Whether the next click on the view picks a portal target.
    pub fn portal_picking(&self) -> bool {
        matches!(self.portal_step(), Some((_, PortalStep::PickTile)))
    }
    /// End an open portal step; whether one was open.
    pub fn retire_portal(&mut self) -> bool {
        self.portal.take().is_some()
    }
    /// Open `step` for the selected object, or close it when it is the one
    /// already open. Opening ends any text draft, and a draft or another
    /// step opened later ends this one. Needs the object's `portal` grant.
    pub fn toggle_portal_step(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        step: PortalStep,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<bool, ToolError> {
        available(pending)?;
        let uid = portal_object(world, catalog, selection)?;
        if self
            .portal
            .as_ref()
            .is_some_and(|open| open.uid == uid && open.step == step)
        {
            self.portal = None;
            return Ok(false);
        }
        self.retire_draft();
        self.portal = Some(OpenPortalStep {
            uid: uid.to_owned(),
            expected,
            step,
        });
        Ok(true)
    }
    /// The open step's object, when it is still the selected object and
    /// still granted, and the step is `step`.
    fn open_portal(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        step: PortalStep,
    ) -> Result<&OpenPortalStep, ToolError> {
        let open = self
            .portal
            .as_ref()
            .filter(|open| open.step == step)
            .ok_or(match step {
                PortalStep::Anchors => ToolError("Open the anchor list of the portal first"),
                PortalStep::PickTile => ToolError("Choose Target a tile in view first"),
            })?;
        if portal_object(world, catalog, selection)? != open.uid {
            return Err(ToolError(
                "The portal step is for another object: select it again",
            ));
        }
        Ok(open)
    }
    /// Target anchor `anchor` of plane `plane` from the open anchor list.
    pub fn portal_anchor_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        (plane, anchor): (&str, &str),
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let open = self.open_portal(world, catalog, selection, PortalStep::Anchors)?;
        if !world
            .planes()
            .get(plane)
            .is_some_and(|source| source.anchors.contains_key(anchor))
        {
            return Err(ToolError("The anchor is no longer defined"));
        }
        Ok(Action::SetPortal {
            uid: open.uid.clone(),
            target: Some(crate::PortalTarget::Anchor {
                plane: plane.into(),
                anchor: anchor.into(),
            }),
            expected: open.expected,
        })
    }
    /// Target the picked tile `cell` of the view, on the view's spatial
    /// layer, from the open pick step.
    pub fn portal_pick_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let open = self.open_portal(world, catalog, selection, PortalStep::PickTile)?;
        let SelectionKey::Cell {
            plane, layer, x, y, ..
        } = cell
        else {
            return Err(ToolError("Click a tile of the view"));
        };
        if !catalog.tile_in_active_view(plane, *layer, *x, *y) {
            return Err(ToolError("Click a tile of the view"));
        }
        Ok(Action::SetPortal {
            uid: open.uid.clone(),
            target: Some(crate::PortalTarget::Location {
                plane: plane.clone(),
                layer: *layer,
                at: [*x, *y],
            }),
            expected: open.expected,
        })
    }
    /// Clear the selected object's portal.
    pub fn clear_portal_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let uid = portal_object(world, catalog, selection)?;
        if world
            .object(uid)
            .is_none_or(|object| object.portal.is_none())
        {
            return Err(ToolError("The object has no portal to clear"));
        }
        Ok(Action::SetPortal {
            uid: uid.to_owned(),
            target: None,
            expected,
        })
    }
    /// Begin typing a location target for the selected object's portal.
    pub fn begin_portal_location(
        &mut self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let uid = portal_object(world, catalog, selection)?.to_owned();
        let own = world
            .object_location(&uid)
            .ok_or(ToolError("The object is no longer in the world"))?
            .plane
            .clone();
        let input = self.new_limited_input(String::new(), MAX_PORTAL_LOCATION_BYTES)?;
        self.retire_draft();
        self.portal = None;
        self.draft = Some(Draft {
            stamp: None,
            kind: DraftKind::PortalLocation { uid, own, expected },
            input,
        });
        Ok(())
    }
    /// The object an open typed-location draft is for.
    pub fn portal_draft(&self) -> Option<&str> {
        match self.draft.as_ref().map(|draft| &draft.kind) {
            Some(DraftKind::PortalLocation { uid, .. }) => Some(uid),
            _ => None,
        }
    }
    /// The reviewed action of the open typed-location draft: `layer x y`
    /// on the object's own plane, or `plane layer x y`, separated by spaces
    /// or commas. The plane must exist, the layer must be a 32-bit integer
    /// and the position integers World accepts; an error keeps the draft
    /// open to fix it. The session checks the grant and the plane again.
    pub fn portal_location_action(
        &self,
        world: &World,
        catalog: &PreviewCatalog,
        selection: Option<&SelectionKey>,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let draft = self.draft.as_ref().ok_or(ToolError("No input draft"))?;
        if draft.input.is_composing() {
            return Err(ToolError("Finish or cancel composition before review"));
        }
        let DraftKind::PortalLocation { uid, own, expected } = &draft.kind else {
            return Err(ToolError("No portal location draft is open"));
        };
        if portal_object(world, catalog, selection)? != uid {
            return Err(ToolError(
                "The draft is for another object: select it again",
            ));
        }
        let words: Vec<&str> = draft
            .input
            .text()
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty())
            .collect();
        let (plane, numbers) = match words.as_slice() {
            [layer, x, y] => (own.as_str(), [*layer, *x, *y]),
            [plane, layer, x, y] => (*plane, [*layer, *x, *y]),
            [] => return Err(ToolError("Type layer x y, or plane layer x y")),
            _ => {
                return Err(ToolError(
                    "Type three or four values: layer x y, or plane layer x y",
                ));
            }
        };
        if !world.planes().contains_key(plane) {
            return Err(ToolError("The world has no plane with this id"));
        }
        let layer: i32 = numbers[0]
            .parse()
            .map_err(|_| ToolError("The layer must be a whole number (32-bit)"))?;
        let coordinate = |text: &str| {
            text.parse::<i64>()
                .ok()
                .filter(|value| (-MAX_SAFE_COORDINATE..=MAX_SAFE_COORDINATE).contains(value))
                .ok_or(ToolError(
                    "x and y must be whole numbers within plus or minus 9007199254740991",
                ))
        };
        let at = [coordinate(numbers[1])?, coordinate(numbers[2])?];
        // Leading where it already leads is refused here too, so the
        // draft stays open to fix it.
        let current = world
            .object(uid)
            .and_then(|object| Some(crate::anchors::resolve(own, object.portal.as_ref()?)));
        if current == Some(crate::anchors::Target::Location { plane, layer, at }) {
            return Err(ToolError("The portal already leads there"));
        }
        Ok(Action::SetPortal {
            uid: uid.clone(),
            target: Some(crate::PortalTarget::Location {
                plane: plane.into(),
                layer,
                at,
            }),
            expected: *expected,
        })
    }
    pub fn terrain_choice(&self) -> Option<&TerrainChoice> {
        self.terrain.as_ref()
    }
    pub fn choose_terrain(
        &mut self,
        catalog: &PreviewCatalog,
        grid: &str,
        prototype: &str,
    ) -> Result<(), ToolError> {
        if !terrain_palette(catalog)
            .iter()
            .any(|c| c.grid == grid && c.prototype == prototype)
        {
            return Err(ToolError("Terrain value is not in the explicit palette"));
        }
        self.terrain = Some(TerrainChoice {
            grid: grid.into(),
            prototype: prototype.into(),
        });
        Ok(())
    }
    pub fn placement_choice(&self) -> Option<&str> {
        self.placement.as_deref()
    }
    /// Choose the prototype Place mode puts down; it must be in the palette.
    pub fn choose_placement(
        &mut self,
        catalog: &PreviewCatalog,
        prototype: &str,
    ) -> Result<(), ToolError> {
        if !object_palette(catalog).contains(&prototype) {
            return Err(ToolError(
                "The catalog does not allow placing this prototype",
            ));
        }
        self.placement = Some(prototype.into());
        Ok(())
    }
    /// The palette filter's committed text (an active composition is not
    /// part of it until committed).
    pub fn palette_filter(&self) -> &str {
        match &self.draft {
            Some(Draft {
                kind: DraftKind::Filter,
                input,
                ..
            }) => input.text(),
            _ => &self.filter,
        }
    }
    pub fn filter_focused(&self) -> bool {
        matches!(
            self.draft,
            Some(Draft {
                kind: DraftKind::Filter,
                ..
            })
        )
    }
    /// The palette rows the filter keeps, in palette order. It narrows the
    /// view only: choosing and placing still check the whole palette.
    pub fn filtered_palette<'a>(&self, catalog: &'a PreviewCatalog) -> Vec<&'a str> {
        let filter = self.palette_filter();
        object_palette(catalog)
            .into_iter()
            .filter(|proto| palette_matches(proto, filter))
            .collect()
    }
    /// Give keyboard focus to the palette filter, keeping its text, with the
    /// caret at the end. Only in Place mode; a pending diff does not block
    /// it because the filter never touches the World. Any other draft is
    /// discarded, as when another field is begun.
    pub fn focus_filter(&mut self) -> Result<(), ToolError> {
        if self.mode != ToolMode::Place {
            return Err(ToolError("The palette filter is only in Place mode"));
        }
        if self.filter_focused() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.filter);
        let identity = self
            .next_draft
            .checked_add(1)
            .ok_or(ToolError("Draft identity exhausted"))?;
        let input = TextInput::with_limit(text.clone(), identity, MAX_FILTER_BYTES);
        let input = match input {
            Ok(input) => input,
            Err(error) => {
                self.filter = text;
                return Err(error);
            }
        };
        self.next_draft = identity;
        self.draft = Some(Draft {
            stamp: None,
            kind: DraftKind::Filter,
            input,
        });
        Ok(())
    }
    /// Release filter focus and keep its committed text (Enter, or a click
    /// anywhere else). An uncommitted composition is dropped.
    pub fn release_filter(&mut self) {
        if self.filter_focused() {
            self.retire_draft();
        }
    }
    /// Esc for the filter: clear its text and release its focus. Returns
    /// whether there was anything to clear; `false` means Esc keeps its
    /// next meaning.
    pub fn clear_filter(&mut self) -> bool {
        let focused = self.filter_focused();
        if focused {
            self.draft = None;
        }
        let had_text = !self.filter.is_empty();
        self.filter.clear();
        focused || had_text
    }
    /// One placement on the exact cell `cell` picked from the current
    /// frame. The session checks the grant, view and revision again.
    pub fn place_action(
        &self,
        catalog: &PreviewCatalog,
        cell: &SelectionKey,
        expected: [u8; 32],
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        if self.mode != ToolMode::Place {
            return Err(ToolError("Place tool is not active"));
        }
        let proto = self
            .placement
            .as_ref()
            .filter(|proto| object_palette(catalog).contains(&proto.as_str()))
            .ok_or(ToolError("Choose a prototype to place in the palette"))?;
        let SelectionKey::Cell {
            plane, layer, x, y, ..
        } = cell
        else {
            return Err(ToolError("Place requires a tile"));
        };
        Ok(Action::Place {
            plane: plane.clone(),
            layer: *layer,
            x: *x,
            y: *y,
            proto: proto.clone(),
            expected,
        })
    }
    pub fn begin_field(
        &mut self,
        inspector: &Inspector<'_>,
        field: &str,
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let row = inspector
            .rows
            .iter()
            .find(|row| row.name == field && row.editable)
            .ok_or(ToolError("Field is read-only or absent from inspector"))?;
        let stamp = inspector
            .stamp
            .clone()
            .ok_or(ToolError("No valid selection"))?;
        let text = row.current.map_or_else(String::new, scalar_text);
        if text.len() > MAX_INPUT_BYTES {
            return Err(ToolError("Input exceeds byte budget"));
        }
        let input = self.new_input(text)?;
        self.retire_draft();
        self.draft = Some(Draft {
            stamp: Some(stamp),
            kind: DraftKind::Field(field.into()),
            input,
        });
        Ok(())
    }
    pub fn begin_move(
        &mut self,
        inspector: &Inspector<'_>,
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        let Some(SelectionSummary::Object {
            at: Some([x, y]),
            movable: true,
            ..
        }) = inspector.selection
        else {
            return Err(ToolError("Selection has no movement capability"));
        };
        let stamp = inspector
            .stamp
            .clone()
            .ok_or(ToolError("No valid selection"))?;
        let input = self.new_input(format!("{x},{y}"))?;
        self.retire_draft();
        self.draft = Some(Draft {
            stamp: Some(stamp),
            kind: DraftKind::Move,
            input,
        });
        Ok(())
    }
    /// Replace selection with exact UTF-8 atomically; commit any active preedit.
    pub fn input(&mut self, text: &str) -> Result<(), ToolError> {
        self.input_mut()?.input(text)
    }
    pub fn backspace(&mut self) {
        if let Some(draft) = &mut self.draft {
            // Legacy callers need no error result. Composition blocks deletion;
            // native dispatch handles IME keys before calling this method.
            let _ = draft.input.backspace();
        }
    }
    /// Discard local input: the text draft, an open portal step and any
    /// running brush stroke. The palette filter only loses focus; its
    /// committed text stays.
    pub fn cancel(&mut self) {
        self.retire_draft();
        self.portal = None;
        self.stroke = None;
    }
    /// End the current draft. A focused filter keeps its committed text.
    /// A new draft or step also ends an open portal step.
    fn retire_draft(&mut self) {
        self.portal = None;
        if let Some(Draft {
            kind: DraftKind::Filter,
            input,
            ..
        }) = self.draft.take()
        {
            input.text().clone_into(&mut self.filter);
        }
    }
    pub fn draft_text(&self) -> Option<&str> {
        self.draft.as_ref().map(|draft| draft.input.text())
    }
    pub fn draft_snapshot(&self) -> Option<TextInputSnapshot> {
        self.draft.as_ref().map(|draft| draft.input.snapshot())
    }
    pub fn input_revision(&self) -> Option<TextRevision> {
        self.draft.as_ref().map(|draft| draft.input.revision())
    }
    pub fn is_composing(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.input.is_composing())
    }
    pub fn set_selection(
        &mut self,
        selection: TextSelection,
        presented: TextRevision,
    ) -> Result<(), ToolError> {
        self.input_mut()?.set_selection(selection, presented)
    }
    pub fn select_all(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.select_all()
    }
    pub fn delete(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.delete()
    }
    pub fn update_preedit(&mut self, text: &str, start: i32, length: i32) -> Result<(), ToolError> {
        self.input_mut()?.update_preedit(text, start, length)
    }
    pub fn cancel_preedit(&mut self) -> Result<(), ToolError> {
        self.input_mut()?.cancel_preedit()
    }

    fn input_mut(&mut self) -> Result<&mut TextInput, ToolError> {
        self.draft
            .as_mut()
            .map(|draft| &mut draft.input)
            .ok_or(ToolError("No input draft"))
    }

    fn new_limited_input(&mut self, text: String, limit: usize) -> Result<TextInput, ToolError> {
        let identity = self
            .next_draft
            .checked_add(1)
            .ok_or(ToolError("Draft identity exhausted"))?;
        let input = TextInput::with_limit(text, identity, limit)?;
        self.next_draft = identity;
        Ok(input)
    }

    fn new_input(&mut self, text: String) -> Result<TextInput, ToolError> {
        let identity = self
            .next_draft
            .checked_add(1)
            .ok_or(ToolError("Draft identity exhausted"))?;
        let input = TextInput::with_identity(text, identity)?;
        self.next_draft = identity;
        Ok(input)
    }
    pub fn build_action(
        &self,
        inspector: &Inspector<'_>,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let draft = self.draft.as_ref().ok_or(ToolError("No input draft"))?;
        let Some(stamp) = draft.stamp.as_ref() else {
            return Err(ToolError(if self.zone_draft().is_some() {
                "Zone drafts are reviewed as zone actions"
            } else if self.anchor_draft().is_some() {
                "Anchor drafts are reviewed as anchor actions"
            } else if self.portal_draft().is_some() {
                "Portal drafts are reviewed as portal actions"
            } else {
                "The palette filter is never reviewed"
            }));
        };
        if inspector.stamp.as_ref() != Some(stamp) {
            return Err(ToolError(
                "Draft selection, generation or source revision is stale",
            ));
        }
        if draft.input.is_composing() {
            return Err(ToolError("Finish or cancel composition before review"));
        }
        let SelectionKey::Object { uid } = &stamp.key else {
            return Err(ToolError("Draft requires an object"));
        };
        match &draft.kind {
            DraftKind::Field(field) => {
                let row = inspector
                    .rows
                    .iter()
                    .find(|row| row.name == field && row.editable)
                    .ok_or(ToolError("Field is no longer editable"))?;
                let value = parse_scalar(&row.spec.field_type, draft.input.text(), inspector)?;
                Ok(Action::SetField {
                    uid: uid.clone(),
                    field: field.clone(),
                    value,
                    expected: stamp.revision,
                })
            }
            DraftKind::Filter => Err(ToolError("The palette filter is never reviewed")),
            DraftKind::ZoneName { .. } | DraftKind::ZoneCreate { .. } => {
                Err(ToolError("Zone drafts are reviewed as zone actions"))
            }
            DraftKind::AnchorCreate { .. } => {
                Err(ToolError("Anchor drafts are reviewed as anchor actions"))
            }
            DraftKind::PortalLocation { .. } => {
                Err(ToolError("Portal drafts are reviewed as portal actions"))
            }
            DraftKind::Move => {
                if !matches!(
                    inspector.selection,
                    Some(SelectionSummary::Object { movable: true, .. })
                ) {
                    return Err(ToolError("Selection has no movement capability"));
                }
                let (x, y) = draft
                    .input
                    .text()
                    .split_once(',')
                    .ok_or(ToolError("Enter coordinates as x,y"))?;
                let x = x
                    .trim()
                    .parse()
                    .map_err(|_| ToolError("Invalid x coordinate"))?;
                let y = y
                    .trim()
                    .parse()
                    .map_err(|_| ToolError("Invalid y coordinate"))?;
                Ok(Action::Move {
                    uid: uid.clone(),
                    x,
                    y,
                    expected: stamp.revision,
                })
            }
        }
    }
    /// Delete the inspected object. The selection must be current and its
    /// prototype granted deletion; the session checks both again.
    pub fn remove_action(
        &self,
        inspector: &Inspector<'_>,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        let stamp = inspector
            .stamp
            .as_ref()
            .ok_or(ToolError("Select an object to delete"))?;
        match inspector.selection {
            Some(SelectionSummary::Object {
                uid,
                deletable: true,
                ..
            }) => Ok(Action::Remove {
                uid: uid.into(),
                expected: stamp.revision,
            }),
            Some(SelectionSummary::Object { .. }) => Err(ToolError(
                "The catalog does not allow deleting this object's type",
            )),
            _ => Err(ToolError("Select an object to delete")),
        }
    }
    pub fn paint_action(
        &self,
        inspector: &Inspector<'_>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<Action, ToolError> {
        available(pending)?;
        if self.mode != ToolMode::Terrain {
            return Err(ToolError("Terrain tool is not active"));
        }
        let stamp = inspector
            .stamp
            .as_ref()
            .filter(|s| &s.key == cell)
            .ok_or(ToolError("Paint requires the current inspected cell"))?;
        let choice = self
            .terrain
            .as_ref()
            .ok_or(ToolError("Choose an explicit terrain palette value"))?;
        let SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } = cell
        else {
            return Err(ToolError("Paint requires a cell"));
        };
        if grid != &choice.grid || !terrain_palette(inspector.catalog).contains(choice) {
            return Err(ToolError("Terrain choice is unavailable for this grid"));
        }
        Ok(Action::Paint {
            plane: plane.clone(),
            layer: *layer,
            grid: grid.clone(),
            x: *x,
            y: *y,
            proto: choice.prototype.clone(),
            expected: stamp.revision,
        })
    }
}

impl ToolState {
    pub fn stroke(&self) -> Option<&Stroke> {
        self.stroke.as_ref()
    }
    /// Begin a drag stroke at the inspected `cell` with the brush and the
    /// chosen terrain (Terrain mode) or zone (Zone mode). Terrain uses the
    /// same capability, palette and freshness checks as a single-cell paint;
    /// both stamp the press cell immediately.
    pub fn begin_stroke(
        &mut self,
        inspector: &Inspector<'_>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<(), ToolError> {
        if self.mode == ToolMode::Zone {
            return self.begin_zone_stroke(inspector, cell, pending);
        }
        let Action::Paint {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            expected,
        } = self.paint_action(inspector, cell, pending)?
        else {
            return Err(ToolError("Paint requires a cell"));
        };
        let generation = inspector
            .stamp
            .as_ref()
            .map(|stamp| stamp.generation)
            .ok_or(ToolError("Paint requires the current inspected cell"))?;
        let (_, _, bounds) = inspector.catalog.view();
        self.retire_draft();
        let mut stroke = Stroke::begin(
            plane,
            layer,
            grid,
            StrokeTarget::Terrain(proto),
            self.brush,
            expected,
            generation,
            bounds,
        );
        stroke.stamp([x, y]);
        self.stroke = Some(stroke);
        Ok(())
    }
    /// A zone stroke samples the ground cell mask like a terrain stroke; its
    /// release writes the `zone` grid instead.
    fn begin_zone_stroke(
        &mut self,
        inspector: &Inspector<'_>,
        cell: &SelectionKey,
        pending: bool,
    ) -> Result<(), ToolError> {
        available(pending)?;
        if !inspector.catalog.editing().zones.paint {
            return Err(ToolError(
                "The catalog does not allow painting zones (editing.zones.paint)",
            ));
        }
        let stamp = inspector
            .stamp
            .as_ref()
            .filter(|s| &s.key == cell)
            .ok_or(ToolError("Paint requires the current inspected cell"))?;
        let choice = self
            .zone
            .as_ref()
            .ok_or(ToolError("Choose a zone in the zone palette"))?;
        if let ZoneChoice::Zone(zone) = choice
            && !zone_palette(inspector.world, inspector.catalog).contains(&zone.as_str())
        {
            return Err(ToolError("The chosen zone is no longer defined"));
        }
        let SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } = cell
        else {
            return Err(ToolError("Paint requires a cell"));
        };
        let (_, _, bounds) = inspector.catalog.view();
        let mut stroke = Stroke::begin(
            plane.clone(),
            *layer,
            grid.clone(),
            StrokeTarget::Zone(choice.value().map(str::to_owned)),
            self.brush,
            stamp.revision,
            stamp.generation,
            bounds,
        );
        self.retire_draft();
        stroke.stamp([*x, *y]);
        self.stroke = Some(stroke);
        Ok(())
    }
    /// Add one pointer sample. `cell` must come from the exact cell mask of
    /// the stroke's own frame `generation`; a changed frame ends the stroke.
    pub fn extend_stroke(
        &mut self,
        cell: &SelectionKey,
        generation: u64,
    ) -> Result<usize, ToolError> {
        let stroke = self.stroke.as_mut().ok_or(ToolError("No brush stroke"))?;
        if stroke.generation() != generation {
            self.stroke = None;
            return Err(ToolError(
                "View changed during the brush stroke; nothing was prepared",
            ));
        }
        let SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        } = cell
        else {
            return Err(ToolError("Brush sample is not a terrain cell"));
        };
        if plane != stroke.plane() || *layer != stroke.layer() || grid != stroke.grid() {
            return Err(ToolError("Brush sample is outside the stroke's grid"));
        }
        Ok(stroke.stamp([*x, *y]))
    }
    /// End the gesture. Returns the single reviewed paint action for every
    /// covered cell, or `None` when no stroke was running.
    pub fn finish_stroke(
        &mut self,
        generation: u64,
        pending: bool,
    ) -> Result<Option<Action>, ToolError> {
        let Some(stroke) = self.stroke.take() else {
            return Ok(None);
        };
        if stroke.generation() != generation {
            return Err(ToolError(
                "View changed during the brush stroke; nothing was prepared",
            ));
        }
        available(pending)?;
        stroke.into_action().map(Some)
    }
    /// Abandon a running stroke without preparing anything.
    pub fn cancel_stroke(&mut self) -> bool {
        self.stroke.take().is_some()
    }
}

/// The selected object, when it is one whose portal the catalog lets the
/// author edit: a top-level object of the active view whose prototype's
/// grant has `portal: true`.
fn portal_object<'s>(
    world: &World,
    catalog: &PreviewCatalog,
    selection: Option<&'s SelectionKey>,
) -> Result<&'s str, ToolError> {
    let Some(SelectionKey::Object { uid }) = selection else {
        return Err(ToolError("Select an object first"));
    };
    if !catalog.can_edit_portal(world, uid) {
        return Err(ToolError(
            "The catalog does not allow editing this object's portal (editing.objects portal)",
        ));
    }
    Ok(uid)
}

/// Whether the active view's plane defines anchor `anchor`.
fn defined_anchor(world: &World, catalog: &PreviewCatalog, anchor: &str) -> bool {
    world
        .planes()
        .get(catalog.view().0)
        .is_some_and(|source| source.anchors.contains_key(anchor))
}

/// The selected tile, when anchor editing is granted and it is a tile of the
/// active view.
fn anchor_target(
    catalog: &PreviewCatalog,
    selection: Option<&SelectionKey>,
) -> Result<[i64; 2], ToolError> {
    if !catalog.editing().anchors.edit {
        return Err(ToolError(
            "The catalog does not allow editing anchors (editing.anchors.edit)",
        ));
    }
    match selection {
        Some(SelectionKey::Cell {
            plane, layer, x, y, ..
        }) if catalog.tile_in_active_view(plane, *layer, *x, *y) => Ok([*x, *y]),
        _ => Err(ToolError("Select a tile of the view first")),
    }
}

fn available(pending: bool) -> Result<(), ToolError> {
    if pending {
        Err(ToolError("Accept or cancel the pending edit first"))
    } else {
        Ok(())
    }
}

/// Authoritative enum/reference options, bounded like inspector input.
pub fn field_choices<'a>(inspector: &'a Inspector<'_>, field: &str) -> Vec<&'a str> {
    let Some(row) = inspector
        .rows
        .iter()
        .find(|r| r.name == field && r.editable)
    else {
        return Vec::new();
    };
    match &row.spec.field_type {
        FieldType::String { values, .. } => values
            .iter()
            .take(MAX_CHOICES)
            .map(String::as_str)
            .collect(),
        FieldType::Reference { target } => inspector
            .world
            .definitions()
            .keys()
            .filter(|(t, _)| t == target)
            .take(MAX_CHOICES)
            .map(|(_, id)| id.as_str())
            .collect(),
        FieldType::Boolean => vec!["true", "false"],
        _ => Vec::new(),
    }
}

fn parse_scalar(
    kind: &FieldType,
    text: &str,
    inspector: &Inspector<'_>,
) -> Result<Value, ToolError> {
    match kind {
        FieldType::Boolean => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(ToolError("Enter true or false")),
        },
        FieldType::Integer { minimum, maximum } => {
            let value = text
                .parse::<i64>()
                .map_err(|_| ToolError("Enter an integer"))?;
            if value < *minimum || value > *maximum {
                return Err(ToolError("Integer is outside schema range"));
            }
            Ok(Value::from(value))
        }
        FieldType::Number { minimum, maximum } => {
            let value = text
                .parse::<f64>()
                .map_err(|_| ToolError("Enter a finite number"))?;
            if !value.is_finite() || value < *minimum || value > *maximum {
                return Err(ToolError("Number is non-finite or outside schema range"));
            }
            Ok(Value::from(value))
        }
        FieldType::String {
            maximum_length,
            values,
        } => {
            if text.chars().count() > *maximum_length
                || (!values.is_empty() && !values.contains(text))
            {
                return Err(ToolError(
                    "String exceeds schema length or is not an allowed choice",
                ));
            }
            Ok(Value::String(text.into()))
        }
        FieldType::Reference { target } => {
            if !inspector
                .world
                .definitions()
                .contains_key(&(target.clone(), text.into()))
            {
                return Err(ToolError(
                    "Reference does not name an authoritative definition",
                ));
            }
            Ok(Value::String(text.into()))
        }
        FieldType::Json | FieldType::List { .. } | FieldType::Object { .. } => {
            Err(ToolError("Structured fields are read-only"))
        }
    }
}
