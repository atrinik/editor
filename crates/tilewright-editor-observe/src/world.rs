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
    /// Only after [`Observed::open_for_lint`]: the World diagnostic codes,
    /// sorted and without repeats, with which the editing entry point's
    /// load budget refused a world that loads with World's default bounds.
    /// `None` when the world fits the budget or was not checked.
    pub over_editor_budget: Option<Vec<&'static str>>,
}

fn diagnostics(error: tilewright_editor_document::Error) -> Vec<Diagnostic> {
    match error {
        tilewright_editor_document::Error::Invalid(diagnostics) => diagnostics,
    }
}

impl Observed {
    /// Opens the world at `root` with World's default bounds, then its
    /// catalog.
    ///
    /// # Errors
    /// World's diagnostics, when the world does not load.
    pub fn open(root: &Path) -> Result<Self, Vec<Diagnostic>> {
        let document = WorldDocument::open(root).map_err(diagnostics)?;
        Ok(Self::with_catalog(document, None))
    }

    /// Opens the world as [`Observed::open`] does, but first with the
    /// editing entry point's load budget
    /// ([`tilewright_editor_document::prototype_load_options`]). The two
    /// loads read the same files with the same parser and validation and
    /// differ only in their limits, which can refuse a world but never
    /// change one: when the budgeted load succeeds, its World is the one the
    /// default load would give. When it fails and the default load
    /// succeeds, the budget alone refused the world, and its diagnostic
    /// codes are kept in [`Observed::over_editor_budget`]. When both fail,
    /// the default load's diagnostics are the error, as for
    /// [`Observed::open`].
    ///
    /// # Errors
    /// World's diagnostics under default bounds, when the world does not
    /// load.
    pub fn open_for_lint(root: &Path) -> Result<Self, Vec<Diagnostic>> {
        match WorldDocument::open_prototype(root) {
            Ok(document) => Ok(Self::with_catalog(document, None)),
            Err(error) => {
                let mut codes: Vec<&'static str> = diagnostics(error)
                    .iter()
                    .map(|diagnostic| diagnostic.code)
                    .collect();
                codes.sort_unstable();
                codes.dedup();
                let document = WorldDocument::open(root).map_err(diagnostics)?;
                Ok(Self::with_catalog(document, Some(codes)))
            }
        }
    }

    fn with_catalog(
        document: WorldDocument,
        over_editor_budget: Option<Vec<&'static str>>,
    ) -> Self {
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
        Self {
            document,
            catalog,
            over_editor_budget,
        }
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

#[cfg(test)]
mod tests {
    use super::load_failure;
    use crate::limits::{MAXIMUM_OUTPUT_BYTES, MAXIMUM_TEXT_CHARACTERS};
    use crate::node::{to_json, to_text};
    use tilewright_source::Diagnostic;

    #[test]
    fn the_largest_load_failure_fits_the_budget() {
        // World's most diagnostics, each with the longest path and message
        // of characters that print as 12-byte escapes.
        let text: String = std::iter::repeat_n('\u{e0041}', MAXIMUM_TEXT_CHARACTERS + 1).collect();
        let diagnostics: Vec<Diagnostic> = (0..tilewright_editor_document::MAXIMUM_DIAGNOSTICS)
            .map(|_| Diagnostic {
                path: text.clone(),
                start: usize::MAX,
                end: usize::MAX,
                line: usize::MAX,
                column: usize::MAX,
                code: "source.a_long_diagnostic_code_name_of_world",
                message: text.clone(),
            })
            .collect();
        let report = load_failure(&diagnostics);
        let json = to_json(&report);
        assert!(json.len() <= MAXIMUM_OUTPUT_BYTES, "{}", json.len());
        assert!(to_text(&report).len() <= MAXIMUM_OUTPUT_BYTES);
        assert!(json.contains("\"diagnostics_more\": 128"));
    }
}
