// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Opening a world for observation: World loads and validates it through
//! [`WorldDocument::open`], and the editor catalog, when the world has one,
//! loads through the preview loader. Nothing here holds a session, a
//! recovery store or any write authority.
use crate::limits::MAXIMUM_DIAGNOSTICS;
use crate::node::{Map, Node};
use std::path::Path;
use tilewright_editor_document::{WorldDocument, revision_hex};
use tilewright_editor_preview::{PreviewCatalog, PreviewLimits, ReadyResources, loader};
use tilewright_editor_ui::anchors::Target;
use tilewright_source::{Diagnostic, World};

/// The editor catalog of an observed world.
pub enum Catalog {
    /// The world has no `preview.jsonc`.
    Absent,
    /// `preview.jsonc` is there but does not load; the loader's message.
    Invalid(String),
    Loaded(Box<(PreviewCatalog, ReadyResources)>),
}

/// A world opened read-only, with its catalog.
pub struct Observed {
    pub document: WorldDocument,
    pub catalog: Catalog,
}

impl Observed {
    /// Opens the world at `root` with World's default bounds, then its
    /// catalog.
    ///
    /// # Errors
    /// World's diagnostics, when the world does not load.
    pub fn open(root: &Path) -> Result<Self, Vec<Diagnostic>> {
        let document = WorldDocument::open(root).map_err(|error| match error {
            tilewright_editor_document::Error::Invalid(diagnostics) => diagnostics,
        })?;
        let catalog = match document.source().exists("preview.jsonc") {
            Ok(false) => Catalog::Absent,
            Err(error) => Catalog::Invalid(error.to_string()),
            Ok(true) => match loader::load(
                document.source(),
                document.world(),
                PreviewLimits::default(),
            ) {
                Ok(loaded) => Catalog::Loaded(Box::new(loaded)),
                Err(error) => Catalog::Invalid(error.to_string()),
            },
        };
        Ok(Self { document, catalog })
    }

    pub fn world(&self) -> &World {
        self.document.world()
    }

    /// The loaded catalog, if any.
    pub fn catalog(&self) -> Option<&PreviewCatalog> {
        match &self.catalog {
            Catalog::Loaded(loaded) => Some(&loaded.0),
            _ => None,
        }
    }

    /// The members every report starts with: its format and the World
    /// revision it describes.
    pub fn header(&self, format: &'static str) -> Map {
        Map::new().with("format", Node::Text(format.into())).with(
            "revision",
            Node::Text(revision_hex(&self.document.revision())),
        )
    }

    /// The catalog's status, and with `view` its view.
    pub fn catalog_status(&self, view: bool) -> Map {
        match &self.catalog {
            Catalog::Absent => Map::new().with("status", Node::Text("absent".into())),
            Catalog::Invalid(problem) => Map::new()
                .with("status", Node::Text("invalid".into()))
                .text("problem", problem),
            Catalog::Loaded(loaded) => {
                let status = Map::new().with("status", Node::Text("loaded".into()));
                if view {
                    status
                        .with("view", view_node(&loaded.0))
                        .text("presentation", loaded.0.presentation())
                } else {
                    status
                }
            }
        }
    }
}

/// The catalog's active view: its plane, spatial layer and tile bounds
/// `[x, y, width, height]`.
pub fn view_node(catalog: &PreviewCatalog) -> Map {
    let (plane, layer, bounds) = catalog.view();
    Map::new()
        .text("plane", plane)
        .with("layer", layer)
        .with("bounds", Node::List(bounds.map(Node::Integer).to_vec()))
}

/// Where a portal leads: a location as written, or an anchor and, when its
/// plane defines it, where it is.
pub fn target_node(world: &World, target: &Target<'_>) -> Map {
    match *target {
        Target::Location { plane, layer, at } => Map::new()
            .with("kind", Node::Text("location".into()))
            .text("plane", plane)
            .with("layer", layer)
            .with("at", at),
        Target::Anchor { plane, anchor } => {
            let found = world
                .planes()
                .get(plane)
                .and_then(|source| source.anchors.get(anchor));
            Map::new()
                .with("kind", Node::Text("anchor".into()))
                .text("plane", plane)
                .text("anchor", anchor)
                .with(
                    "resolved",
                    found.map_or(Node::Null, |found| {
                        Map::new()
                            .with("layer", found.layer)
                            .with("at", found.at)
                            .into()
                    }),
                )
        }
    }
}

/// The report of a world that does not load: World's diagnostics.
pub fn load_failure(diagnostics: &[Diagnostic]) -> Map {
    Map::new()
        .with(
            "format",
            Node::Text("tilewright.editor-load-failure/1".into()),
        )
        .count("problems", diagnostics.len())
        .bounded(
            "diagnostics",
            diagnostics,
            MAXIMUM_DIAGNOSTICS,
            |diagnostic| {
                Map::new()
                    .text("path", &diagnostic.path)
                    .count("line", diagnostic.line)
                    .count("column", diagnostic.column)
                    .count("start", diagnostic.start)
                    .count("end", diagnostic.end)
                    .text("code", diagnostic.code)
                    .text("message", &diagnostic.message)
                    .into()
            },
        )
}
