// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! An editing session: the changeset of one open world, saved as the user
//! works.
//!
//! Edits accumulate in memory, where the live preview shows them, and are
//! published to the source tree automatically when an interaction ends: a
//! pointer or key is released and no other interaction begins within the
//! session's coalescing window ([`Options::coalesce`], 2 seconds by
//! default). Everything from the first press to that point is one
//! transaction, one save and one undo step. A window of zero saves on every
//! release.
//!
//! Precisely, the edits pending since the last save are published:
//!
//! - by [`Session::tick`] once no interaction is running and the window has
//!   passed since the last one ended;
//! - by [`Session::begin_interaction`] when the window had already passed,
//!   so a late tick never merges two interactions it should have
//!   separated;
//! - by [`Session::end_interaction`] when the window is zero, or the
//!   pending edits have grown past [`MAXIMUM_PENDING_CELLS`];
//! - by [`Session::flush`], on demand.
//!
//! Undo publishes the last save's inverse, which the toolkit returned when
//! it carried the save out, and redo publishes the forward transaction
//! again; each is one step. Undo with edits still pending drops them
//! instead, writing nothing, and redo publishes them. Time is passed in by
//! the caller, so the rules are deterministic under test.

use crate::{
    Error as PreviewError, Preview,
    brush::{self, Brush, Stroke},
    preview, transaction,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    time::{Duration, Instant},
};
use tilewright_edit::PublishError;
use tilewright_editor_document::{
    Error as DocumentError, WorldDocument,
    terrain::{self, ChunkKey, PlaneTerrain},
};

/// How long after an interaction ends its edits wait for the next one to
/// join them.
pub const DEFAULT_COALESCE: Duration = Duration::from_secs(2);

/// Pending terrain cells past which an interaction's end saves at once,
/// whatever the window: a transaction holds at most 1,000,000.
pub const MAXIMUM_PENDING_CELLS: usize = 500_000;

/// Longest coalescing window; a longer one counts as this.
pub const MAXIMUM_COALESCE: Duration = Duration::from_mins(1);

/// Most undo steps a session keeps; the oldest goes first.
pub const MAXIMUM_HISTORY: usize = 1_024;

/// How a session saves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    /// How long the edits of an ended interaction wait for the next one
    /// to join them in the same save and undo step, at most
    /// [`MAXIMUM_COALESCE`]. Zero saves on every release.
    pub coalesce: Duration,
    /// Whether the user has accepted what publishing on this platform does
    /// not check, [`crate::UNCHECKED_ACCESS`]. Without it saving is refused
    /// there; Linux checks everything and ignores it.
    pub unchecked_access: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            coalesce: DEFAULT_COALESCE,
            unchecked_access: false,
        }
    }
}

/// One edit made in an interaction: a transaction operation, with the
/// editor command that made it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub title: String,
    /// The text of one transaction operation.
    pub operation: String,
    /// The JSON text of the command and its parameters, recorded in the
    /// transaction's `custom` data.
    pub record: String,
    /// Terrain cells the operation sets.
    pub cells: usize,
}

/// A saved change: one undo step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Saved {
    pub title: String,
    /// The files it changed, by path.
    pub files: Vec<String>,
}

/// An undo or redo step.
#[derive(Clone, Debug)]
struct Step {
    title: String,
    /// The transaction that makes the change: the sealed one, with the
    /// digests of the files before and after, once it has been published.
    forward: String,
    /// The transaction that reverts it, once it has been published.
    inverse: Option<String>,
}

/// An editing session over one world.
#[derive(Debug)]
pub struct Session {
    document: WorldDocument,
    options: Options,
    /// The terrain of each plane the session has painted or shown, over
    /// the document's world.
    terrains: BTreeMap<String, PlaneTerrain>,
    stroke: Option<Stroke>,
    active: bool,
    pending: Vec<Command>,
    /// When the pending edits are saved if no interaction begins first.
    deadline: Option<Instant>,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl Session {
    /// Opens the world at `root`.
    ///
    /// # Errors
    /// Fails when the world does not load.
    pub fn open(root: &Path, options: Options) -> Result<Self, Error> {
        Ok(Self::new(
            WorldDocument::open(root).map_err(Error::Document)?,
            options,
        ))
    }

    #[must_use]
    pub fn new(document: WorldDocument, options: Options) -> Self {
        Self {
            document,
            options,
            terrains: BTreeMap::new(),
            stroke: None,
            active: false,
            pending: Vec::new(),
            deadline: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The world as last saved or opened.
    #[must_use]
    pub fn document(&self) -> &WorldDocument {
        &self.document
    }

    #[must_use]
    pub const fn options(&self) -> Options {
        self.options
    }

    /// The terrain of `plane` with the session's unsaved edits, for the
    /// live preview.
    ///
    /// # Errors
    /// Fails if the world has no such plane.
    pub fn terrain(&mut self, plane: &str) -> Result<&PlaneTerrain, Error> {
        self.plane_terrain(plane).map(|terrain| &*terrain)
    }

    fn plane_terrain(&mut self, plane: &str) -> Result<&mut PlaneTerrain, Error> {
        if !self.terrains.contains_key(plane) {
            let terrain =
                PlaneTerrain::new(self.document.world().clone(), plane).map_err(Error::Terrain)?;
            self.terrains.insert(plane.to_owned(), terrain);
        }
        self.terrains
            .get_mut(plane)
            .ok_or_else(|| Error::Terrain(terrain::Error::UnknownPlane(plane.to_owned())))
    }

    /// Whether edits are waiting to be saved, or a stroke is running.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.pending.is_empty()
            || self
                .stroke
                .as_ref()
                .is_some_and(|stroke| !stroke.is_empty())
    }

    /// Whether an interaction is running.
    #[must_use]
    pub const fn is_interacting(&self) -> bool {
        self.active
    }

    /// When the pending edits will be saved if no interaction begins first.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// The titles of the undo steps, oldest first, and of the redo steps,
    /// next first.
    #[must_use]
    pub fn history(&self) -> (Vec<&str>, Vec<&str>) {
        let undo = self.undo.iter().map(|step| step.title.as_str()).collect();
        let redo = self
            .redo
            .iter()
            .rev()
            .map(|step| step.title.as_str())
            .collect();
        (undo, redo)
    }

    /// The transaction the pending edits would be saved as, if any.
    #[must_use]
    pub fn pending_transaction(&self) -> Option<String> {
        compose(&self.pending)
    }

    /// Starts an interaction at `now`: a pointer or key press. If the
    /// coalescing window of the last one has passed, its edits are saved
    /// first, as their own step.
    ///
    /// # Errors
    /// Fails if an interaction is already running, or saving fails; then the
    /// interaction does not start.
    pub fn begin_interaction(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        let saved = if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.flush()?
        } else {
            None
        };
        self.active = true;
        self.deadline = None;
        Ok(saved)
    }

    /// Adds `command` to the pending edits. Its operation must already be
    /// reflected in the session's terrain, as a stroke's is.
    ///
    /// # Errors
    /// Fails outside an interaction.
    pub fn record(&mut self, command: Command) -> Result<(), Error> {
        if !self.active {
            return Err(Error::Idle);
        }
        self.pending.push(command);
        self.redo.clear();
        Ok(())
    }

    /// Ends the interaction at `now`: the release. Its edits are saved at
    /// once when the window is zero or they have grown past
    /// [`MAXIMUM_PENDING_CELLS`]; otherwise when the window has passed.
    ///
    /// # Errors
    /// Fails outside an interaction, while a stroke is running, or when
    /// saving fails; the interaction has ended then, and its edits stay
    /// pending.
    pub fn end_interaction(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        if !self.active {
            return Err(Error::Idle);
        }
        if self.stroke.is_some() {
            return Err(Error::Busy);
        }
        self.active = false;
        if self.pending.is_empty() {
            return Ok(None);
        }
        let cells: usize = self.pending.iter().map(|command| command.cells).sum();
        if self.options.coalesce.is_zero() || cells > MAXIMUM_PENDING_CELLS {
            return self.flush();
        }
        let window = self.options.coalesce.min(MAXIMUM_COALESCE);
        match now.checked_add(window) {
            Some(deadline) => {
                self.deadline = Some(deadline);
                Ok(None)
            }
            None => self.flush(),
        }
    }

    /// Saves the pending edits if the window has passed at `now`.
    ///
    /// # Errors
    /// Fails when saving fails; the edits stay pending and are tried again
    /// when the next interaction ends.
    pub fn tick(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        if self.active || self.deadline.is_none_or(|deadline| now < deadline) {
            return Ok(None);
        }
        self.flush()
    }

    /// Starts a brush stroke at `now` on `layer` of `plane`, painting
    /// `terrain`; it begins an interaction.
    ///
    /// # Errors
    /// Fails as [`Session::begin_interaction`] and [`Stroke::begin`] do.
    pub fn begin_stroke(
        &mut self,
        now: Instant,
        plane: &str,
        layer: i32,
        terrain: &str,
        brush: Brush,
    ) -> Result<Option<Saved>, Error> {
        // Check the stroke before anything is saved.
        Stroke::begin(self.plane_terrain(plane)?, layer, terrain, brush).map_err(Error::Brush)?;
        let saved = self.begin_interaction(now)?;
        let stroke = Stroke::begin(self.plane_terrain(plane)?, layer, terrain, brush)
            .map_err(Error::Brush)?;
        self.stroke = Some(stroke);
        Ok(saved)
    }

    /// Stamps the running stroke at plane tile `point`. Returns the chunks
    /// whose preview may have changed.
    ///
    /// # Errors
    /// Fails without a running stroke, or as [`Stroke::stamp`] does.
    pub fn stamp(&mut self, point: [i64; 2]) -> Result<BTreeSet<ChunkKey>, Error> {
        let mut stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        let result = match self.plane_terrain(stroke.plane()) {
            Ok(terrain) => stroke.stamp(terrain, point).map_err(Error::Brush),
            Err(error) => Err(error),
        };
        self.stroke = Some(stroke);
        result
    }

    /// Ends the running stroke and its interaction at `now`.
    ///
    /// # Errors
    /// Fails without a running stroke, or as [`Session::end_interaction`]
    /// does.
    pub fn end_stroke(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        if let Some(operation) = stroke.operation().map_err(Error::Brush)? {
            self.record(Command {
                title: stroke.title(),
                operation,
                record: stroke.record(),
                cells: stroke.len(),
            })?;
        }
        self.end_interaction(now)
    }

    /// Cancels the running stroke: its tiles go back, and its interaction
    /// ends without an edit. Returns the chunks whose preview changed.
    ///
    /// # Errors
    /// Fails without a running stroke.
    pub fn cancel_stroke(&mut self, now: Instant) -> Result<BTreeSet<ChunkKey>, Error> {
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        let terrain = self.plane_terrain(stroke.plane())?;
        let dirty = stroke.chunks().flat_map(terrain::neighbourhood).collect();
        stroke.revert(terrain).map_err(Error::Brush)?;
        self.end_interaction(now)?;
        Ok(dirty)
    }

    /// Saves the pending edits now, as one transaction and one undo step.
    ///
    /// # Errors
    /// Fails while an interaction runs, or when the toolkit rejects or
    /// cannot publish the transaction; the edits stay pending, and the tree
    /// is as the error says.
    pub fn flush(&mut self) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        self.deadline = None;
        let Some(text) = compose(&self.pending) else {
            return Ok(None);
        };
        let title = title(&self.pending);
        let planned = self.publish(&text)?;
        self.pending.clear();
        Ok(Some(self.push_undo(title, &planned)))
    }

    /// Drops the pending edits without saving them, and rebuilds the
    /// preview from the world as saved.
    ///
    /// # Errors
    /// Fails while an interaction runs.
    pub fn discard(&mut self) -> Result<(), Error> {
        if self.active {
            return Err(Error::Busy);
        }
        self.pending.clear();
        self.deadline = None;
        self.terrains.clear();
        Ok(())
    }

    /// Undoes the last step: drops the pending edits, or publishes the last
    /// save's inverse. Returns what was undone.
    ///
    /// # Errors
    /// Fails while an interaction runs, or when the inverse cannot be
    /// published, such as after the files changed outside the editor; the
    /// step stays in the history then.
    pub fn undo(&mut self) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        if let Some(forward) = compose(&self.pending) {
            let title = title(&self.pending);
            self.discard()?;
            self.redo.push(Step {
                title: title.clone(),
                forward,
                inverse: None,
            });
            return Ok(Some(Saved {
                title,
                files: Vec::new(),
            }));
        }
        let Some(step) = self.undo.pop() else {
            return Ok(None);
        };
        let Some(inverse) = step.inverse.as_deref() else {
            self.undo.push(step);
            return Err(Error::Unsaved);
        };
        let planned = match self.publish(inverse) {
            Ok(planned) => planned,
            Err(error) => {
                self.undo.push(step);
                return Err(error);
            }
        };
        let saved = Saved {
            title: step.title.clone(),
            files: changed(&planned),
        };
        self.redo.push(Step {
            inverse: None,
            ..step
        });
        Ok(Some(saved))
    }

    /// Redoes the last undone step by publishing its forward transaction.
    /// Returns what was redone.
    ///
    /// # Errors
    /// Fails while an interaction runs or edits are pending, or when the
    /// transaction cannot be published; the step stays undone then.
    pub fn redo(&mut self) -> Result<Option<Saved>, Error> {
        if self.active || !self.pending.is_empty() {
            return Err(Error::Busy);
        }
        let Some(step) = self.redo.pop() else {
            return Ok(None);
        };
        match self.publish(&step.forward) {
            Ok(planned) => {
                let redo = std::mem::take(&mut self.redo);
                let saved = self.push_undo(step.title, &planned);
                self.redo = redo;
                Ok(Some(saved))
            }
            Err(error) => {
                self.redo.push(step);
                Err(error)
            }
        }
    }

    /// Carries out and publishes `text` against the tree, then takes the
    /// world it produced as the session's and rebuilds the preview.
    fn publish(&mut self, text: &str) -> Result<Preview, Error> {
        let planned = preview(&self.document, "editor.jsonc", text).map_err(Error::Rejected)?;
        planned
            .publish(self.options.unchecked_access)
            .map_err(Error::Publish)?;
        self.document = self.document.with_world(planned.world().clone());
        self.terrains.clear();
        Ok(planned)
    }

    fn push_undo(&mut self, title: String, planned: &Preview) -> Saved {
        let saved = Saved {
            title: title.clone(),
            files: changed(planned),
        };
        if self.undo.len() >= MAXIMUM_HISTORY {
            self.undo.remove(0);
        }
        self.undo.push(Step {
            title,
            forward: planned.sealed().to_owned(),
            inverse: Some(planned.inverse().to_owned()),
        });
        self.redo.clear();
        saved
    }
}

/// The files a carried-out transaction changes.
fn changed(planned: &Preview) -> Vec<String> {
    planned.changed().map(|file| file.path.clone()).collect()
}

/// The transaction of `commands`, in order, or `None` without any.
fn compose(commands: &[Command]) -> Option<String> {
    if commands.is_empty() {
        return None;
    }
    let operations: Vec<String> = commands
        .iter()
        .map(|command| command.operation.clone())
        .collect();
    let records: Vec<String> = commands
        .iter()
        .map(|command| command.record.clone())
        .collect();
    Some(transaction(&title(commands), &operations, &records))
}

/// One title for a group of commands: the first's, with a count of the
/// rest.
fn title(commands: &[Command]) -> String {
    match commands {
        [] => String::new(),
        [only] => only.title.clone(),
        [first, rest @ ..] => {
            let plural = if rest.len() == 1 { "" } else { "s" };
            format!("{} and {} more edit{plural}", first.title, rest.len())
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Document(DocumentError),
    Terrain(terrain::Error),
    Brush(brush::Error),
    /// The toolkit rejected the transaction: its diagnostics.
    Rejected(PreviewError),
    /// Publishing failed; the error says whether the tree changed.
    Publish(PublishError),
    /// An interaction or stroke is running.
    Busy,
    /// No interaction is running.
    Idle,
    NoStroke,
    /// The step was never saved, so it has no inverse to publish.
    Unsaved,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(formatter),
            Self::Terrain(error) => error.fmt(formatter),
            Self::Brush(error) => error.fmt(formatter),
            Self::Rejected(error) => error.fmt(formatter),
            Self::Publish(error) => write!(formatter, "saving failed: {error}"),
            Self::Busy => write!(formatter, "an interaction is running"),
            Self::Idle => write!(formatter, "no interaction is running"),
            Self::NoStroke => write!(formatter, "no brush stroke is running"),
            Self::Unsaved => write!(formatter, "the step was never saved"),
        }
    }
}

impl std::error::Error for Error {}
