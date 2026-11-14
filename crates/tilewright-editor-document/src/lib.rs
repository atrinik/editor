// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! The editor's view of a world source tree. The world toolkit reads and
//! validates the tree; this crate keeps the validated world and describes it
//! for panels and the command line. It parses and writes nothing itself.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tilewright_source::{Diagnostic, LoadOptions, Overlay, Projection, SourceRoot, World};

/// The world format and toolkit release the editor builds against.
pub const TOOLKIT_COMPATIBILITY: &str = "tilewright.world/1 tilewright-source/0.2.0";

/// Most diagnostics opening a world reports. Past one fewer, the toolkit
/// keeps the last place for a `source.budget` diagnostic about the rest.
pub const MAXIMUM_DIAGNOSTICS: usize = 256;

/// Explicit bounds for the offline prototype; the general document API retains
/// its existing World defaults. Every reload and transaction retains these.
pub fn prototype_load_options() -> LoadOptions {
    let mut options = load_options();
    options.maximum_files = 256;
    options.maximum_entries = 1_024;
    options.maximum_total_bytes = 8 << 20;
    options.limits.max_input_bytes = 1 << 20;
    options.limits.max_values = 65_536;
    options.limits.max_array_items = 65_536;
    options
}

/// How the editor loads worlds: the toolkit keeps one diagnostic fewer than
/// [`MAXIMUM_DIAGNOSTICS`] and adds its budget note after them.
pub fn load_options() -> LoadOptions {
    LoadOptions {
        maximum_diagnostics: MAXIMUM_DIAGNOSTICS - 1,
        ..LoadOptions::default()
    }
}

/// A world source tree, opened and validated.
#[derive(Clone, Debug)]
pub struct WorldDocument {
    root: PathBuf,
    source: Arc<SourceRoot>,
    world: Arc<World>,
    options: LoadOptions,
}

impl WorldDocument {
    /// Loads and validates the world at `root` with the toolkit's default
    /// bounds.
    ///
    /// # Errors
    /// Returns the toolkit's diagnostics, at most [`MAXIMUM_DIAGNOSTICS`],
    /// when the tree does not load.
    pub fn open(root: &Path) -> Result<Self, Error> {
        Self::open_with_options(root, load_options())
    }

    /// Opens with the bounded offline-prototype source budget.
    pub fn open_prototype(root: &Path) -> Result<Self, Error> {
        Self::open_with_options(root, prototype_load_options())
    }

    /// Opens with explicit toolkit limits, retained by every reload and edit.
    pub fn open_with_options(root: &Path, options: LoadOptions) -> Result<Self, Error> {
        let source = Arc::new(SourceRoot::new(root));
        let world = tilewright_source::load_edited_in(&source, &options, &Overlay::new())
            .map_err(Error::Invalid)?;
        Ok(Self {
            root: root.to_path_buf(),
            source,
            world: Arc::new(world),
            options,
        })
    }

    /// Reload the originally opened toolkit root, never re-resolving its display path.
    pub fn reload(&self) -> Result<Self, Error> {
        let world = tilewright_source::load_edited_in(&self.source, &self.options, &Overlay::new())
            .map_err(Error::Invalid)?;
        Ok(Self {
            root: self.root.clone(),
            source: self.source.clone(),
            world: Arc::new(world),
            options: self.options,
        })
    }

    /// Exact toolkit source limits retained when opening this document.
    #[must_use]
    pub const fn load_options(&self) -> LoadOptions {
        self.options
    }

    /// Filesystem authority retained from opening the document.
    #[must_use]
    pub fn source(&self) -> &SourceRoot {
        &self.source
    }

    /// The display path the world was opened from; never use it as write authority.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn world(&self) -> &Arc<World> {
        &self.world
    }

    /// SHA-256 over every file the load read, as the toolkit defines it: the
    /// tree's revision when it was opened.
    #[must_use]
    pub fn revision(&self) -> [u8; 32] {
        self.world.source_digest()
    }

    /// What the world holds, for display.
    #[must_use]
    pub fn summary(&self) -> Summary {
        let profile = self.world.profile();
        let transitions = profile.transitions.as_deref();
        Summary {
            profile: profile.id.clone(),
            profile_version: profile.version.clone(),
            projection: profile.projection,
            ground_layers: profile.ground_layers.clone(),
            terrain_layer: transitions
                .and_then(|transitions| profile.ground_layers.get(transitions.terrain_layer()))
                .cloned(),
            terrains: transitions.map_or_else(Vec::new, |transitions| {
                transitions
                    .terrains()
                    .iter()
                    .map(|terrain| terrain.id.clone())
                    .collect()
            }),
            planes: self
                .world
                .planes()
                .values()
                .map(|plane| {
                    let mut layers: Vec<i32> =
                        plane.chunks.keys().map(|(layer, _, _)| *layer).collect();
                    layers.dedup();
                    PlaneSummary {
                        id: plane.id.clone(),
                        layers,
                        chunks: plane.chunks.len(),
                    }
                })
                .collect(),
        }
    }
}

/// A short description of a world: its profile, terrains and planes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
    pub profile: String,
    pub profile_version: String,
    pub projection: Projection,
    pub ground_layers: Vec<String>,
    /// The ground layer that holds painted terrain, when the profile has
    /// terrain transitions.
    pub terrain_layer: Option<String>,
    /// Terrains from lowest to highest precedence; empty without
    /// transitions.
    pub terrains: Vec<String>,
    /// Planes by ID.
    pub planes: Vec<PlaneSummary>,
}

/// One plane of a [`Summary`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaneSummary {
    pub id: String,
    /// Layers that hold at least one chunk, ascending.
    pub layers: Vec<i32>,
    pub chunks: usize,
}

impl fmt::Display for Summary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let projection = match self.projection {
            Projection::Isometric {
                tile_width,
                tile_height,
            } => format!("isometric {tile_width}x{tile_height}"),
            Projection::TopDown { tile_size } => format!("top-down {tile_size}"),
        };
        // Each value from the world is one field: its line breaks and
        // formatting characters cannot make lines of their own.
        let list = |values: &[String]| {
            values
                .iter()
                .map(|value| terminal_field(value))
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(
            formatter,
            "profile {} {} ({projection})",
            terminal_field(&self.profile),
            terminal_field(&self.profile_version)
        )?;
        writeln!(formatter, "ground layers: {}", list(&self.ground_layers))?;
        match &self.terrain_layer {
            Some(layer) => writeln!(
                formatter,
                "terrains in {}: {}",
                terminal_field(layer),
                list(&self.terrains)
            )?,
            None => writeln!(formatter, "terrains: none")?,
        }
        for plane in &self.planes {
            let layers: Vec<String> = plane.layers.iter().map(i32::to_string).collect();
            writeln!(
                formatter,
                "plane {}: {} chunks on layers [{}]",
                terminal_field(&plane.id),
                plane.chunks,
                layers.join(", ")
            )?;
        }
        Ok(())
    }
}

/// Whether a character changes how a terminal shows text rather than
/// showing itself: a control character, such as those that move the cursor,
/// retitle the window or write the clipboard; a line or paragraph
/// separator; or a bidirectional formatting character, which can reorder
/// what is shown. [`terminal_field`] escapes exactly these characters;
/// other writers, such as JSON output, escape the same set.
#[must_use]
pub fn unprintable(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
                | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

fn escaped(text: &str, keep: fn(char) -> bool) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut safe, character| {
            if unprintable(character) && !keep(character) {
                safe.extend(character.escape_unicode());
            } else {
                safe.push(character);
            }
            safe
        })
}

/// One value from a world, such as an ID, a version or a diagnostic, made
/// safe to print as part of a line: every character that would change how
/// the terminal shows text, line breaks included, comes out as its escape.
#[must_use]
pub fn terminal_field(text: &str) -> String {
    escaped(text, |_| false)
}

/// Text of several lines made safe to print, as a backstop behind
/// [`terminal_field`]: everything [`terminal_field`] escapes except line
/// breaks.
#[must_use]
pub fn terminal_safe(text: &str) -> String {
    escaped(text, |character| character == '\n')
}

/// Lowercase hexadecimal of a revision.
#[must_use]
pub fn revision_hex(revision: &[u8; 32]) -> String {
    use fmt::Write as _;
    revision
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[derive(Debug)]
pub enum Error {
    /// The tree does not load: the toolkit's diagnostics, sorted by path and
    /// position.
    Invalid(Vec<Diagnostic>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(diagnostics) => {
                write!(
                    formatter,
                    "the world does not load: {} problem(s)",
                    diagnostics.len()
                )?;
                if let Some(first) = diagnostics.first() {
                    write!(formatter, "; first: {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{
        Error, MAXIMUM_DIAGNOSTICS, PlaneSummary, Summary, WorldDocument, load_options,
        revision_hex, terminal_field, terminal_safe,
    };
    use tilewright_editor_testkit::fixture_world;
    use tilewright_source::Projection;

    #[test]
    fn explicit_source_budget_is_enforced_and_retained_on_reload() {
        let root = fixture_world("prototype-neutral");
        let mut bounded = super::prototype_load_options();
        bounded.maximum_total_bytes = 1;
        assert!(matches!(
            WorldDocument::open_with_options(&root, bounded),
            Err(Error::Invalid(_))
        ));
        let document = WorldDocument::open_prototype(&root).unwrap();
        assert_eq!(document.load_options(), super::prototype_load_options());
        let reopened = document.reload().unwrap();
        assert_eq!(reopened.load_options(), document.load_options());
        assert_eq!(reopened.revision(), document.revision());
    }

    #[test]
    fn opens_a_world_and_describes_it() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let summary = document.summary();
        assert_eq!(summary.profile, "meadow-example");
        assert_eq!(summary.projection, Projection::TopDown { tile_size: 32 });
        assert_eq!(summary.ground_layers, ["ground", "detail", "edge"]);
        assert_eq!(summary.terrain_layer.as_deref(), Some("ground"));
        assert_eq!(summary.terrains, ["water", "sand", "grass"]);
        assert_eq!(
            summary.planes,
            [PlaneSummary {
                id: "meadow".to_owned(),
                layers: vec![0],
                chunks: 1,
            }]
        );
        assert!(summary.to_string().contains("plane meadow: 1 chunks"));
    }

    #[test]
    fn revision_is_the_source_digest_and_stable() {
        let first = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let second = WorldDocument::open(&fixture_world("meadow")).unwrap();
        assert_eq!(first.revision(), second.revision());
        assert_eq!(first.revision(), first.world().source_digest());
        assert_eq!(revision_hex(&first.revision()).len(), 64);
    }

    #[test]
    fn a_tree_that_is_not_a_world_reports_the_toolkit_diagnostics() {
        // The profile directory has no world.jsonc.
        let root = fixture_world("meadow").join("profile");
        let Err(Error::Invalid(diagnostics)) = WorldDocument::open(&root) else {
            panic!("a tree without a manifest opened");
        };
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path == "world.jsonc"),
            "{diagnostics:?}"
        );
        let message = Error::Invalid(diagnostics).to_string();
        assert!(message.starts_with("the world does not load"), "{message}");
    }

    #[test]
    fn world_text_cannot_forge_lines_or_reorder_them() {
        let summary = Summary {
            profile: "example".to_owned(),
            profile_version: "1.0.0\nterrains: forged\nplane evil \u{202e}gnp.exe".to_owned(),
            projection: Projection::TopDown { tile_size: 32 },
            ground_layers: vec!["floor".to_owned()],
            terrain_layer: None,
            terrains: Vec::new(),
            planes: Vec::new(),
        };
        let shown = terminal_safe(&summary.to_string());
        assert_eq!(shown.lines().count(), 3, "{shown}");
        assert!(shown.starts_with(
            "profile example 1.0.0\\u{a}terrains: forged\\u{a}plane evil \\u{202e}gnp.exe"
        ));
        assert_eq!(
            terminal_field("a\nb\u{2066}c\u{2029}"),
            "a\\u{a}b\\u{2066}c\\u{2029}"
        );
        assert_eq!(terminal_safe("a\nb\u{200f}"), "a\nb\\u{200f}");
    }

    #[test]
    fn a_world_with_many_problems_reports_at_most_the_maximum() {
        // Three hundred broken definitions over the fixture, in memory.
        let overlay: tilewright_source::Overlay = (0..300)
            .map(|index| {
                (
                    format!("defs/proto/broken{index}.jsonc"),
                    Some("{".to_owned()),
                )
            })
            .collect();
        let Err(diagnostics) =
            tilewright_source::load_edited(&fixture_world("meadow"), &load_options(), &overlay)
        else {
            panic!("a broken world loaded");
        };
        assert_eq!(diagnostics.len(), MAXIMUM_DIAGNOSTICS);
        let notes = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "source.budget")
            .count();
        assert_eq!(notes, 1);
    }

    #[test]
    fn control_characters_from_a_world_print_escaped() {
        // A profile version may hold any text the toolkit accepts.
        let summary = Summary {
            profile: "example".to_owned(),
            profile_version: "1.0.0\u{1b}]0;title\u{7}\u{1b}[2J".to_owned(),
            projection: Projection::TopDown { tile_size: 32 },
            ground_layers: vec!["floor".to_owned()],
            terrain_layer: None,
            terrains: Vec::new(),
            planes: Vec::new(),
        };
        let shown = terminal_safe(&summary.to_string());
        assert!(
            !shown
                .chars()
                .any(|character| character.is_control() && character != '\n')
        );
        assert!(
            shown.contains("1.0.0\\u{1b}]0;title\\u{7}\\u{1b}[2J"),
            "{shown}"
        );
        assert_eq!(shown.lines().count(), 3);
        assert_eq!(terminal_safe("a\tb\r\n"), "a\\u{9}b\\u{d}\n");
    }

    #[test]
    fn a_missing_root_is_an_error_not_a_panic() {
        let root = fixture_world("does-not-exist");
        assert!(matches!(
            WorldDocument::open(&root),
            Err(Error::Invalid(diagnostics)) if !diagnostics.is_empty()
        ));
    }
}
