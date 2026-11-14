// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `lint`: the editor's own findings about a world that loads. World's
//! diagnostics come with a world that does not load (see
//! [`crate::world::load_failure`]); everything here is computed by the
//! editor's existing checks, with no analysis of its own. Reachability is
//! deliberately left out until the profile can say what blocks movement.
use crate::limits::MAXIMUM_FINDINGS;
use crate::node::{Map, Node};
use crate::world::{Catalog, Observed};
use std::collections::BTreeSet;
use tilewright_editor_preview::PreviewLimits;
use tilewright_editor_ui::{anchors, portals, zones};
use tilewright_source::World;

pub const FORMAT: &str = "tilewright.editor-lint/1";

/// How much a finding matters. Only errors change the exit code.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    /// The desktop editor cannot open or show the world as it is.
    Error,
    /// Probably a mistake, but the world loads and the editor opens it.
    Warning,
    /// Worth knowing; nothing is wrong.
    Info,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One finding: a stable code, a severity, where, and a message this
/// editor wrote. `detail` is text from elsewhere (a loader message), kept
/// apart from the message because it may quote world text.
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub location: Map,
    pub message: String,
    pub detail: Option<String>,
}

impl Finding {
    fn node(self) -> Node {
        Map::new()
            .with("code", Node::Text(self.code.into()))
            .with("severity", Node::Text(self.severity.as_str().into()))
            .with("location", self.location)
            .text("message", &self.message)
            .optional_text("detail", self.detail.as_deref())
            .into()
    }
}

/// Every finding about `observed`, in a fixed order: the catalog, the
/// prepared view, portals in World order, then anchors and zones by plane
/// and id.
pub fn findings(observed: &Observed) -> Vec<Finding> {
    let world = observed.world();
    let mut found = Vec::new();
    match &observed.catalog {
        Catalog::Absent => {}
        Catalog::Invalid(problem) => found.push(Finding {
            code: "catalog.invalid",
            severity: Severity::Error,
            location: Map::new().text("path", "preview.jsonc"),
            message:
                "The editor catalog does not load, so the desktop editor cannot open this world."
                    .into(),
            detail: Some(problem.clone()),
        }),
        Catalog::Loaded(loaded) => {
            let (catalog, resources) = &**loaded;
            let (plane, layer, _) = catalog.view();
            let view = || Map::new().text("plane", plane).with("layer", layer);
            // The view the desktop editor opens, prepared as it prepares it.
            let spec = catalog.default_view(world, 1280, 800, 1);
            match tilewright_editor_preview::prepare(
                world,
                catalog,
                resources,
                &spec,
                PreviewLimits::default(),
            ) {
                Ok(prepared) => {
                    for diagnostic in prepared.diagnostics {
                        found.push(Finding {
                            code: diagnostic.code,
                            severity: Severity::Info,
                            location: view(),
                            message: diagnostic.message,
                            detail: None,
                        });
                    }
                }
                Err(error) => found.push(Finding {
                    code: "preview.unprepared",
                    severity: Severity::Error,
                    location: view(),
                    message: "The catalog's view cannot be prepared for display, so the desktop editor cannot open this world.".into(),
                    detail: Some(error.to_string()),
                }),
            }
        }
    }
    portal_findings(observed, &mut found);
    let holders = anchors::portals(world);
    let named: BTreeSet<(&str, &str)> = holders
        .iter()
        .filter_map(|holder| match holder.target {
            anchors::Target::Anchor { plane, anchor } => Some((plane, anchor)),
            anchors::Target::Location { .. } => None,
        })
        .collect();
    for (plane_id, plane) in world.planes() {
        for (id, anchor) in &plane.anchors {
            if !named.contains(&(plane_id.as_str(), id.as_str())) {
                found.push(Finding {
                    code: "anchor.unreferenced",
                    severity: Severity::Info,
                    location: Map::new()
                        .text("plane", plane_id)
                        .text("anchor", id)
                        .with("layer", anchor.layer)
                        .with("at", anchor.at),
                    message: "No portal names this anchor; scripts or saves outside the world may still use its id.".into(),
                    detail: None,
                });
            }
        }
    }
    for (plane_id, plane) in world.planes() {
        for zone in zones::unused_zones(plane) {
            found.push(Finding {
                code: "zone.unused",
                severity: Severity::Info,
                location: Map::new().text("plane", plane_id).text("zone", zone),
                message: "No tile, chunk palette or child zone uses this zone and it is not the plane's default zone, so it could be deleted.".into(),
                detail: None,
            });
        }
    }
    found
}

/// The review's warnings about where each portal leads
/// ([`portals::check_target`]), for every portal in the world.
fn portal_findings(observed: &Observed, found: &mut Vec<Finding>) {
    let world = observed.world();
    let catalog = observed.catalog();
    let grid = catalog.map_or_else(
        || world.profile().ground_layers.first().map(String::as_str),
        |catalog| portals::pick_grid(world, catalog),
    );
    for holder in anchors::portals(world) {
        let check = portals::check_target(world, grid, None, &holder);
        let location = || holder_location(world, &holder);
        if check.own_tile {
            found.push(Finding {
                code: "portal.target_own_tile",
                severity: Severity::Warning,
                location: location(),
                message: "The portal leads to the tile its object stands on.".into(),
                detail: None,
            });
        }
        let Some(target) = check.location else {
            continue;
        };
        if target.missing_chunk {
            found.push(Finding {
                code: "portal.target_no_chunk",
                severity: Severity::Warning,
                location: location(),
                message: "The portal's location target is in a map chunk that does not exist; World does not check portal targets.".into(),
                detail: None,
            });
        }
        if let Some(grid) = target.missing_ground {
            found.push(Finding {
                code: "portal.target_no_ground",
                severity: Severity::Warning,
                location: location().text("grid", grid),
                message: "The portal's location target has no ground on the pick grid; World does not check portal targets.".into(),
                detail: None,
            });
        }
    }
}

/// Where a portal's object is: its chunk's path, plane, layer, tile, uid,
/// and whether it is carried in an inventory.
fn holder_location(world: &World, holder: &anchors::PortalHolder<'_>) -> Map {
    let path = world.planes().get(holder.plane).and_then(|plane| {
        let size = i64::from(plane.chunk_size.max(1));
        let [x, y] = holder.tile?;
        plane
            .chunks
            .get(&(holder.layer, x.div_euclid(size), y.div_euclid(size)))
            .map(|chunk| chunk.path.as_str())
    });
    Map::new()
        .optional_text("path", path)
        .text("plane", holder.plane)
        .with("layer", holder.layer)
        .with("tile", holder.tile.map_or(Node::Null, Node::from))
        .optional_text("uid", holder.uid)
        .with("carried", holder.depth > 0)
}

/// The lint report, and whether it has an error.
pub fn report(observed: &Observed) -> (Map, bool) {
    let found = findings(observed);
    let count = |severity: Severity| {
        found
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let errors = count(Severity::Error);
    let counts = Map::new()
        .count("error", errors)
        .count("warning", count(Severity::Warning))
        .count("info", count(Severity::Info));
    (
        observed
            .header(FORMAT)
            .with("catalog", observed.catalog_status(false))
            .with("counts", counts)
            .bounded("findings", found, MAXIMUM_FINDINGS, Finding::node),
        errors > 0,
    )
}
