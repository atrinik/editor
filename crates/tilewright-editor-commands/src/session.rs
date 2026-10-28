// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! An editing session: the changes made to one open world, saved as the
//! user works.
//!
//! Each interaction, from a pointer or key press to its release, is saved to
//! the source tree when it ends: its edits become one transaction, which
//! the toolkit carries out, validates and publishes. Undo steps coalesce:
//! an interaction that begins within the session's window
//! ([`Options::coalesce`], 2 seconds by default) of the last saved one
//! joins its undo step, so a burst of strokes undoes and redoes as one. A
//! window of zero makes each interaction its own step.
//!
//! - Undo publishes the inverses of a step's saves, the last first; redo
//!   publishes its transactions again, the first first. A step that fails
//!   partway is split: what was undone or redone moves to the other stack,
//!   the rest stays, and the error says why.
//! - A save whose own edits are the problem drops that interaction's edits
//!   alone and keeps them in [`Session::rejected`]: one whose tiles changed
//!   outside the editor, one that does not parse, or one whose edits leave
//!   the world invalid while the world on disk loads without them. The
//!   session reloads the world from disk, so the next edit expects what is
//!   there now.
//! - A save too large to save or to undo is refused the same way, before
//!   anything is written ([`Error::TooLarge`]): the toolkit checks that its
//!   inverse fits a transaction, and [`Preview::check_undo`] that it can be
//!   carried out. Undo and redo publish what those checks passed, and are
//!   not checked again.
//! - A world that does not validate with a save's edits, while it loads
//!   from disk, may have been read while another program was changing it:
//!   the save is planned once more, and published if it now plans, before
//!   its edits are blamed.
//! - A save that cannot be made now keeps its edits pending, and no
//!   interaction begins until [`Session::flush`] saves them or
//!   [`Session::discard`] drops them. That covers a file that cannot be read
//!   or written, a world that does not load from disk for reasons of its
//!   own, and a save waiting for the user's consent
//!   ([`Error::NeedsConsent`]). When a publish fails partway and leaves files
//!   it could not restore, its inverse is kept in [`Session::recovery`]
//!   until that same transaction is published, or the record is taken.
//! - A publish that reports a problem after every file was published, such
//!   as a directory it could not sync, counts as saved, with the problem in
//!   [`Saved::warning`].
//!
//! Time is passed in by the caller, so the rules are deterministic under
//! test.

use crate::{
    Error as PreviewError, Preview,
    brush::{self, Brush, StampError, Stroke},
    preview, transaction,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    time::{Duration, Instant},
};
use tilewright_edit::{PublishError, limits};
use tilewright_editor_document::{
    Error as DocumentError, WorldDocument,
    terrain::{self, ChunkKey, PlaneTerrain},
};
use tilewright_source::Diagnostic;

/// The name the session's transactions carry in the toolkit's diagnostics.
const TRANSACTION_NAME: &str = "editor.jsonc";

/// How soon after the last saved interaction another one joins its undo
/// step.
pub const DEFAULT_COALESCE: Duration = Duration::from_secs(2);

/// Longest coalescing window; a longer one counts as this.
pub const MAXIMUM_COALESCE: Duration = Duration::from_mins(1);

/// Undo steps a session keeps by default; the oldest goes first.
pub const DEFAULT_HISTORY: usize = 1_024;

/// How a session saves and keeps history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    /// How soon after the last saved interaction another one joins its undo
    /// step, at most [`MAXIMUM_COALESCE`]. Zero makes each interaction its
    /// own step.
    pub coalesce: Duration,
    /// Whether the user has accepted what saving does not check where the
    /// toolkit cannot check everything ([`crate::Preview::unchecked_access`]):
    /// on macOS and Windows, and on Linux for trees on file systems that
    /// hide their access control lists. Without it, saving there stops with
    /// [`Error::NeedsConsent`].
    pub unchecked_access: bool,
    /// Most undo steps kept, at least one.
    pub history: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            coalesce: DEFAULT_COALESCE,
            unchecked_access: false,
            history: DEFAULT_HISTORY,
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
}

/// A save, undo or redo that reached the tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Saved {
    /// The undo step's title.
    pub title: String,
    /// The files it changed, by path.
    pub files: Vec<String>,
    /// A problem the toolkit reported after every file was published.
    pub warning: Option<String>,
}

/// Edits a save could not keep, which the session dropped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rejected {
    pub title: String,
    /// The transaction they would have been.
    pub transaction: String,
    /// Why the toolkit rejected it.
    pub reason: String,
}

/// One published transaction of an undo step.
#[derive(Clone, Debug)]
struct Save {
    title: String,
    /// The sealed transaction, with the digests of its files before and
    /// after: redo checks them.
    forward: String,
    /// The transaction that reverts it.
    inverse: String,
}

/// An undo or redo step: the saves of the interactions it coalesced, in
/// the order they were made.
#[derive(Clone, Debug)]
struct Step {
    saves: Vec<Save>,
}

impl Step {
    fn title(&self) -> String {
        match self.saves.as_slice() {
            [] => String::new(),
            [only] => only.title.clone(),
            [first, rest @ ..] => {
                let plural = if rest.len() == 1 { "" } else { "s" };
                format!("{} and {} more edit{plural}", first.title, rest.len())
            }
        }
    }
}

/// A transaction the toolkit published.
struct Published {
    planned: Preview,
    warning: Option<String>,
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
    /// The running interaction's edits, or those of a save that could not
    /// be published.
    pending: Vec<Command>,
    /// Whether the running or pending interaction joins the last undo step.
    joining: bool,
    /// When the last saved interaction ended, while its step can grow.
    last_end: Option<Instant>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    rejected: Option<Rejected>,
    recovery: Option<Recovery>,
    /// Runs once between a failed plan and the check of the world on disk,
    /// as another program would.
    #[cfg(test)]
    interleave: Interleave,
}

/// The inverse of a transaction whose publish left files it could not
/// restore, and that transaction.
#[derive(Debug)]
struct Recovery {
    source: String,
    inverse: String,
}

#[cfg(test)]
#[derive(Default)]
struct Interleave(Option<Box<dyn FnOnce()>>);

#[cfg(test)]
impl fmt::Debug for Interleave {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Interleave")
    }
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
            joining: false,
            last_end: None,
            undo: Vec::new(),
            redo: Vec::new(),
            rejected: None,
            recovery: None,
            #[cfg(test)]
            interleave: Interleave::default(),
        }
    }

    /// The world as last saved, opened or reloaded.
    #[must_use]
    pub fn document(&self) -> &WorldDocument {
        &self.document
    }

    #[must_use]
    pub const fn options(&self) -> Options {
        self.options
    }

    /// Records whether the user accepts what saving here does not check,
    /// as when they answer [`Error::NeedsConsent`].
    pub const fn set_unchecked_access(&mut self, accepted: bool) {
        self.options.unchecked_access = accepted;
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

    /// Whether edits are not saved yet: a stroke is running, or a save
    /// could not be published.
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

    /// Whether a save could not be published and waits for
    /// [`Session::flush`] or [`Session::discard`].
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        !self.active && !self.pending.is_empty()
    }

    /// The titles of the undo steps, oldest first, and of the redo steps,
    /// next first.
    #[must_use]
    pub fn history(&self) -> (Vec<String>, Vec<String>) {
        let undo = self.undo.iter().map(Step::title).collect();
        let redo = self.redo.iter().rev().map(Step::title).collect();
        (undo, redo)
    }

    /// The transaction the pending edits would be saved as, if any.
    #[must_use]
    pub fn pending_transaction(&self) -> Option<String> {
        compose(&self.pending)
    }

    /// The edits the last rejected save dropped, until another save
    /// succeeds or [`Session::take_rejected`] takes them.
    #[must_use]
    pub const fn rejected(&self) -> Option<&Rejected> {
        self.rejected.as_ref()
    }

    pub const fn take_rejected(&mut self) -> Option<Rejected> {
        self.rejected.take()
    }

    /// The inverse of the last transaction whose publish failed partway and
    /// could not restore every file: applied to the tree, it restores them.
    /// It is kept until [`Session::take_recovery`] takes it, or until that
    /// same transaction is published, as by a retried undo or redo or a
    /// flush of the same edits. Saves of other edits leave it.
    #[must_use]
    pub fn recovery(&self) -> Option<&str> {
        self.recovery
            .as_ref()
            .map(|recovery| recovery.inverse.as_str())
    }

    pub fn take_recovery(&mut self) -> Option<String> {
        self.recovery.take().map(|recovery| recovery.inverse)
    }

    /// Starts an interaction at `now`: a pointer or key press.
    ///
    /// # Errors
    /// Fails if an interaction is running, or a save waits to be published
    /// or discarded.
    pub fn begin_interaction(&mut self, now: Instant) -> Result<(), Error> {
        if self.active {
            return Err(Error::Busy);
        }
        if !self.pending.is_empty() {
            return Err(Error::Waiting);
        }
        let window = self.options.coalesce.min(MAXIMUM_COALESCE);
        self.joining = !window.is_zero()
            && !self.undo.is_empty()
            && self
                .last_end
                .is_some_and(|end| now.saturating_duration_since(end) <= window);
        self.active = true;
        Ok(())
    }

    /// Adds `command` to the running interaction. Its operation must already
    /// be reflected in the session's terrain, as a stroke's is.
    ///
    /// # Errors
    /// Fails outside an interaction.
    pub fn record(&mut self, command: Command) -> Result<(), Error> {
        if !self.active {
            return Err(Error::Idle);
        }
        self.pending.push(command);
        Ok(())
    }

    /// Ends the interaction at `now`, the release, and saves its edits.
    ///
    /// # Errors
    /// Fails outside an interaction, while a stroke is running, or when the
    /// save fails, as [`Session::flush`] says.
    pub fn end_interaction(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        if !self.active {
            return Err(Error::Idle);
        }
        if self.stroke.is_some() {
            return Err(Error::Busy);
        }
        self.active = false;
        self.store(now)
    }

    /// Saves edits whose save could not be published before, at `now`.
    ///
    /// # Errors
    /// Fails while an interaction runs. A rejected save drops its edits
    /// and reloads the world; one that cannot be published keeps them.
    pub fn flush(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        self.store(now)
    }

    /// Drops edits whose save could not be published, and rebuilds the
    /// preview from the world as saved.
    ///
    /// # Errors
    /// Fails while an interaction runs.
    pub fn discard(&mut self) -> Result<(), Error> {
        if self.active {
            return Err(Error::Busy);
        }
        self.pending.clear();
        self.terrains.clear();
        Ok(())
    }

    /// Starts a brush stroke at `now` on `layer` of `plane`, painting
    /// `terrain`; it begins an interaction.
    ///
    /// # Errors
    /// Fails as [`Session::begin_interaction`] and [`Stroke::begin`] do;
    /// nothing has started then.
    pub fn begin_stroke(
        &mut self,
        now: Instant,
        plane: &str,
        layer: i32,
        terrain: &str,
        brush: Brush,
    ) -> Result<(), Error> {
        if self.active {
            return Err(Error::Busy);
        }
        if !self.pending.is_empty() {
            return Err(Error::Waiting);
        }
        let stroke = Stroke::begin(self.plane_terrain(plane)?, layer, terrain, brush)
            .map_err(Error::Brush)?;
        self.begin_interaction(now)?;
        self.stroke = Some(stroke);
        Ok(())
    }

    /// Stamps the running stroke at plane tile `point`. Returns the chunks
    /// whose preview may have changed.
    ///
    /// # Errors
    /// Fails without a running stroke, or as [`Stroke::stamp`] does; then
    /// [`Error::Stamp`] names the chunks the stamp changed before it failed.
    pub fn stamp(&mut self, point: [i64; 2]) -> Result<BTreeSet<ChunkKey>, Error> {
        let mut stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        let result = match self.plane_terrain(stroke.plane()) {
            Ok(terrain) => stroke.stamp(terrain, point).map_err(Error::Stamp),
            Err(error) => Err(error),
        };
        self.stroke = Some(stroke);
        result
    }

    /// Ends the running stroke and its interaction at `now`, and saves it.
    ///
    /// # Errors
    /// Fails without a running stroke, or as [`Session::end_interaction`]
    /// does. A stroke that cannot become an operation is cancelled.
    pub fn end_stroke(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        let command = stroke.operation().map(|operation| {
            operation.map(|operation| Command {
                title: stroke.title(),
                operation,
                record: stroke.record(),
            })
        });
        let recorded = match command {
            Ok(Some(command)) => self.record(command),
            Ok(None) => Ok(()),
            Err(error) => Err(Error::Brush(error)),
        };
        if let Err(error) = recorded {
            self.abandon(&stroke)?;
            return Err(error);
        }
        self.end_interaction(now)
    }

    /// Cancels the running stroke and its interaction: the stroke's tiles go
    /// back and nothing is saved. Returns the chunks whose preview changed.
    ///
    /// # Errors
    /// Fails without a running stroke.
    pub fn cancel_stroke(&mut self) -> Result<BTreeSet<ChunkKey>, Error> {
        let stroke = self.stroke.take().ok_or(Error::NoStroke)?;
        let dirty = stroke.chunks().flat_map(terrain::neighbourhood).collect();
        self.abandon(&stroke)?;
        Ok(dirty)
    }

    /// Puts a stroke's tiles back and ends its interaction without saving.
    fn abandon(&mut self, stroke: &Stroke) -> Result<(), Error> {
        self.active = false;
        self.pending.clear();
        let terrain = self.plane_terrain(stroke.plane())?;
        stroke.revert(terrain).map_err(Error::Brush)
    }

    /// Saves the pending edits as one transaction.
    fn store(&mut self, now: Instant) -> Result<Option<Saved>, Error> {
        let Some(text) = compose(&self.pending) else {
            return Ok(None);
        };
        let title = title(&self.pending);
        let published = match self.publish(&text, true) {
            Ok(published) => published,
            Err(error @ (Error::Rejected(_) | Error::TooLarge(_))) => {
                self.pending.clear();
                self.rejected = Some(Rejected {
                    title,
                    transaction: text,
                    reason: error.to_string(),
                });
                self.reload();
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        self.pending.clear();
        self.rejected = None;
        let save = Save {
            title,
            forward: published.planned.sealed().to_owned(),
            inverse: published.planned.inverse().to_owned(),
        };
        match self.undo.last_mut() {
            Some(step) if self.joining => step.saves.push(save),
            _ => self.push_undo(Step { saves: vec![save] }),
        }
        self.redo.clear();
        self.last_end = Some(now);
        let step = self.undo.last().map(Step::title).unwrap_or_default();
        Ok(Some(Saved {
            title: step,
            files: changed(&published.planned),
            warning: published.warning,
        }))
    }

    /// Undoes the last step by publishing its saves' inverses, the last
    /// first, or drops edits whose save could not be published. Returns
    /// what was undone.
    ///
    /// # Errors
    /// Fails while an interaction runs, or when an inverse cannot be
    /// published, such as after its files changed outside the editor; what
    /// was undone by then moves to redo, and the rest stays.
    pub fn undo(&mut self) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        if !self.pending.is_empty() {
            let title = title(&self.pending);
            self.discard()?;
            return Ok(Some(Saved {
                title,
                files: Vec::new(),
                warning: None,
            }));
        }
        let Some(mut step) = self.undo.pop() else {
            return Ok(None);
        };
        self.last_end = None;
        let title = step.title();
        let (mut files, mut warning, mut undone) = (BTreeSet::new(), None, Vec::new());
        while let Some(save) = step.saves.pop() {
            match self.publish(&save.inverse, false) {
                Ok(published) => {
                    files.extend(changed(&published.planned));
                    warning = warning.or(published.warning);
                    undone.push(save);
                }
                Err(error) => {
                    step.saves.push(save);
                    self.undo.push(step);
                    if !undone.is_empty() {
                        undone.reverse();
                        self.redo.push(Step { saves: undone });
                    }
                    if matches!(error, Error::Rejected(_)) {
                        self.reload();
                    }
                    return Err(error);
                }
            }
        }
        undone.reverse();
        self.redo.push(Step { saves: undone });
        Ok(Some(Saved {
            title,
            files: files.into_iter().collect(),
            warning,
        }))
    }

    /// Redoes the last undone step by publishing its transactions again,
    /// the first first. Returns what was redone.
    ///
    /// # Errors
    /// Fails while an interaction runs or a save waits, or when a
    /// transaction cannot be published; what was redone by then moves to
    /// undo, and the rest stays.
    pub fn redo(&mut self) -> Result<Option<Saved>, Error> {
        if self.active {
            return Err(Error::Busy);
        }
        if !self.pending.is_empty() {
            return Err(Error::Waiting);
        }
        let Some(mut step) = self.redo.pop() else {
            return Ok(None);
        };
        self.last_end = None;
        let title = step.title();
        let (mut files, mut warning, mut redone) = (BTreeSet::new(), None, Vec::new());
        step.saves.reverse();
        while let Some(save) = step.saves.pop() {
            match self.publish(&save.forward, false) {
                Ok(published) => {
                    files.extend(changed(&published.planned));
                    warning = warning.or(published.warning);
                    redone.push(Save {
                        title: save.title,
                        forward: published.planned.sealed().to_owned(),
                        inverse: published.planned.inverse().to_owned(),
                    });
                }
                Err(error) => {
                    step.saves.push(save);
                    step.saves.reverse();
                    self.redo.push(step);
                    if !redone.is_empty() {
                        self.push_undo(Step { saves: redone });
                    }
                    if matches!(error, Error::Rejected(_)) {
                        self.reload();
                    }
                    return Err(error);
                }
            }
        }
        self.push_undo(Step { saves: redone });
        Ok(Some(Saved {
            title,
            files: files.into_iter().collect(),
            warning,
        }))
    }

    /// Carries out and publishes `text` against the tree, then takes the
    /// world it produced as the session's and rebuilds the preview. A
    /// `forward` save, which is not an undo or redo, is first checked to be
    /// one that can be undone.
    fn publish(&mut self, text: &str, forward: bool) -> Result<Published, Error> {
        let planned = self.plan(text)?;
        if forward {
            planned.check_undo().map_err(Error::TooLarge)?;
        }
        // A directory that cannot be read here is a problem of the disk, not
        // of the edits, so they wait like any save that was not published.
        let unchecked = planned.unchecked_access().map_err(waiting)?;
        if !self.options.unchecked_access
            && let Some(unchecked) = unchecked
        {
            return Err(Error::NeedsConsent(unchecked));
        }
        let warning = match planned.publish(self.options.unchecked_access) {
            Ok(published) => (!published.durable).then(|| {
                "the saved files' names may not be durable until the system syncs them".to_owned()
            }),
            // Every file was published; what went wrong after is a warning.
            Err(error) if error.applied => Some(error.to_string()),
            Err(error) => {
                // Only a publish that could not put every file back leaves
                // anything for its inverse to restore.
                if error
                    .problems
                    .iter()
                    .any(|problem| problem.message.contains("restoring failed"))
                {
                    self.recovery = Some(Recovery {
                        source: text.to_owned(),
                        inverse: planned.inverse().to_owned(),
                    });
                }
                return Err(Error::Publish(error));
            }
        };
        // The transaction a recovery came from is published now, so its
        // files are what it makes them, and there is nothing to restore.
        if self
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.source == text)
        {
            self.recovery = None;
        }
        self.document = self.document.with_world(planned.world().clone());
        self.terrains.clear();
        Ok(Published { planned, warning })
    }

    /// Plans `text` against the tree. A world that does not validate with
    /// the edits while it loads from disk may have been read while another
    /// program was changing it, so the text is planned once more before
    /// the edits are blamed.
    fn plan(&mut self, text: &str) -> Result<Preview, Error> {
        let mut replanned = false;
        loop {
            let problems = match preview(&self.document, TRANSACTION_NAME, text) {
                Ok(planned) => return Ok(planned),
                Err(PreviewError::Rejected(problems)) => problems,
            };
            #[cfg(test)]
            if let Some(interleave) = self.interleave.0.take() {
                interleave();
            }
            let root = self.document.root();
            match verdict(&problems, || WorldDocument::open(root).is_ok()) {
                Verdict::Replan if !replanned => replanned = true,
                Verdict::Replan | Verdict::Reject => {
                    return Err(Error::Rejected(PreviewError::Rejected(problems)));
                }
                Verdict::TooLarge(reason) => return Err(Error::TooLarge(reason)),
                Verdict::Wait => return Err(waiting(problems)),
            }
        }
    }

    /// Reads the world from disk again, after a rejected save, so the next
    /// edit expects what is there now. A tree that no longer loads keeps
    /// the world as it was.
    fn reload(&mut self) {
        if let Ok(document) = WorldDocument::open(self.document.root()) {
            self.document = document;
        }
        self.terrains.clear();
    }

    fn push_undo(&mut self, step: Step) {
        if self.undo.len() >= self.options.history.max(1) {
            self.undo.remove(0);
        }
        self.undo.push(step);
    }
}

/// What the session does about a plan the toolkit refused.
#[derive(Debug, Eq, PartialEq)]
enum Verdict {
    /// The disk's problem: the edits wait.
    Wait,
    /// The edits' problem: they are dropped.
    Reject,
    /// The edits reach a limit: they are dropped as too large, and why.
    TooLarge(String),
    /// The world did not validate with the edits but loads from disk now:
    /// plan them once more.
    Replan,
}

/// Why the toolkit would not carry a transaction out, as what the session
/// does about it. A problem with the transaction itself, such as a
/// conflict or its shape, is the edits', and a limit it reaches makes it
/// too large. A file that cannot be read is the disk's. A world that does
/// not validate is the disk's while the world on disk does not load
/// (`loads_now`), and is planned again while it does.
fn verdict(problems: &[Diagnostic], loads_now: impl FnOnce() -> bool) -> Verdict {
    let disk = |problem: &Diagnostic| matches!(problem.code, "tx.io" | "source.io");
    if problems.iter().all(disk) {
        return Verdict::Wait;
    }
    // Past its cap the toolkit ends the problems with a note, a limit at
    // the transaction's own name, that there may be more: that is not a
    // limit the edits reach.
    let note = problems.len() >= limits::DIAGNOSTICS
        && problems
            .last()
            .is_some_and(|last| last.code == "tx.limit" && last.path == TRANSACTION_NAME);
    let reported = &problems[..problems.len() - usize::from(note)];
    let own = reported
        .iter()
        .any(|problem| problem.path == TRANSACTION_NAME && !disk(problem));
    if !own {
        return if loads_now() {
            Verdict::Replan
        } else {
            Verdict::Wait
        };
    }
    match reported.iter().find(|problem| problem.code == "tx.limit") {
        Some(limit) => Verdict::TooLarge(limit.message.clone()),
        None => Verdict::Reject,
    }
}

/// Edits that were not published for a reason of the disk's, which wait.
fn waiting(problems: Vec<Diagnostic>) -> Error {
    Error::Publish(PublishError {
        applied: false,
        durable: true,
        cleaned: true,
        problems,
    })
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
    /// A stamp failed partway; its tiles stay painted.
    Stamp(StampError),
    /// The toolkit rejected the transaction: its diagnostics.
    Rejected(PreviewError),
    /// Publishing failed before every file was published; the error says
    /// whether files were left to restore.
    Publish(PublishError),
    /// An interaction or stroke is running.
    Busy,
    /// No interaction is running.
    Idle,
    NoStroke,
    /// A save could not be published and waits for a flush or a discard.
    Waiting,
    /// The save is too large to save or to undo, and was refused before
    /// anything was written: why.
    TooLarge(String),
    /// Saving here would not check what the text says, and the user has not
    /// accepted that ([`Options::unchecked_access`]); nothing was written.
    NeedsConsent(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(formatter),
            Self::Terrain(error) => error.fmt(formatter),
            Self::Brush(error) => error.fmt(formatter),
            Self::Stamp(error) => error.fmt(formatter),
            Self::Rejected(error) => error.fmt(formatter),
            Self::Publish(error) => write!(formatter, "saving failed: {error}"),
            Self::Busy => write!(formatter, "an interaction is running"),
            Self::Idle => write!(formatter, "no interaction is running"),
            Self::NoStroke => write!(formatter, "no brush stroke is running"),
            Self::Waiting => write!(
                formatter,
                "unsaved edits wait to be saved again or discarded"
            ),
            Self::TooLarge(reason) => write!(
                formatter,
                "the edit is too large to save as one step: {reason}"
            ),
            Self::NeedsConsent(unchecked) => write!(
                formatter,
                "saving here does not check everything and waits for your consent: {unchecked}"
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Interleave, Options, Recovery, Session, TRANSACTION_NAME, Verdict, verdict};
    use crate::{
        brush::{Brush, Shape},
        json_string, transaction,
    };
    use std::time::Instant;
    use tilewright_edit::{Options as PlanOptions, PublishOptions, Transaction, limits};
    use tilewright_editor_testkit::MEADOW_FILES;
    use tilewright_source::Diagnostic;

    fn meadow_copy() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let operations: Vec<String> = MEADOW_FILES
            .iter()
            .map(|(path, text)| {
                format!(
                    "{{\"op\": \"document.create\", \"path\": {}, \"text\": {}}}",
                    json_string(path),
                    json_string(text)
                )
            })
            .collect();
        let text = transaction("Copy the meadow", &operations, &[]);
        let created = Transaction::parse("copy.jsonc", text).unwrap();
        tilewright_edit::plan(directory.path(), &created, &PlanOptions::default())
            .unwrap()
            .publish_with(&PublishOptions {
                unchecked_access: true,
            })
            .unwrap();
        directory
    }

    fn session(root: &std::path::Path) -> Session {
        Session::open(
            root,
            Options {
                unchecked_access: true,
                ..Options::default()
            },
        )
        .unwrap()
    }

    fn paint(session: &mut Session, now: Instant, terrain: &str, at: [i64; 2]) {
        let dot = Brush {
            shape: Shape::Square,
            radius: 0,
        };
        session
            .begin_stroke(now, "meadow", 0, terrain, dot)
            .unwrap();
        session.stamp(at).unwrap();
        session.end_stroke(now).unwrap().unwrap();
    }

    /// A file the loader refuses, in the meadow's plane; it goes when
    /// dropped.
    fn stray(root: &std::path::Path) -> tempfile::NamedTempFile {
        tempfile::Builder::new()
            .prefix("stray")
            .suffix(".txt")
            .tempfile_in(root.join("planes/meadow"))
            .unwrap()
    }

    fn problem(path: &str, code: &'static str) -> Diagnostic {
        Diagnostic {
            path: path.to_owned(),
            start: 0,
            end: 0,
            line: 1,
            column: 1,
            code,
            message: format!("{code} at {path}"),
        }
    }

    #[test]
    fn a_world_changed_while_a_save_was_planned_does_not_drop_it() {
        let root = meadow_copy();
        let mut session = session(root.path());
        // Another program is midway through a change that leaves the tree
        // briefly invalid, and finishes it before the session looks again.
        let midway = stray(root.path());
        session.interleave = Interleave(Some(Box::new(move || drop(midway))));
        paint(&mut session, Instant::now(), "sand", [1, 1]);
        assert!(session.interleave.0.is_none(), "the first plan failed");
        assert_eq!(session.history().0, ["Paint sand on 1 tile"]);
        assert!(session.rejected().is_none() && !session.is_waiting());
    }

    #[test]
    fn a_recovery_stays_until_its_own_transaction_is_published_or_taken() {
        let root = meadow_copy();
        let mut session = session(root.path());
        let now = Instant::now();
        paint(&mut session, now, "sand", [1, 1]);
        // As if undoing that save had failed partway and left files
        // unrestored.
        let undo = session.undo.last().unwrap().saves[0].inverse.clone();
        session.recovery = Some(Recovery {
            source: undo,
            inverse: "the inverse of the undo".to_owned(),
        });
        // Saving other edits leaves it.
        let later = now + std::time::Duration::from_secs(5);
        paint(&mut session, later, "water", [40, 1]);
        assert_eq!(session.recovery(), Some("the inverse of the undo"));
        // Undoing the other edits, another transaction, leaves it too; the
        // undo it came from, published at last, clears it.
        session.undo().unwrap().unwrap();
        assert_eq!(session.recovery(), Some("the inverse of the undo"));
        session.undo().unwrap().unwrap();
        assert_eq!(session.recovery(), None);
        // Taking it takes it.
        session.recovery = Some(Recovery {
            source: String::new(),
            inverse: "kept".to_owned(),
        });
        assert_eq!(session.take_recovery().as_deref(), Some("kept"));
        assert_eq!(session.recovery(), None);
    }

    #[test]
    fn the_note_that_there_may_be_more_problems_is_no_limit() {
        let mut conflicts: Vec<Diagnostic> = (0..limits::DIAGNOSTICS - 1)
            .map(|_| problem(TRANSACTION_NAME, "tx.conflict"))
            .collect();
        conflicts.push(problem(TRANSACTION_NAME, "tx.limit"));
        assert_eq!(verdict(&conflicts, || true), Verdict::Reject);
        // A limit among fewer problems is one the edits reach.
        let limit = [problem(TRANSACTION_NAME, "tx.limit")];
        assert_eq!(
            verdict(&limit, || true),
            Verdict::TooLarge("tx.limit at editor.jsonc".to_owned())
        );
        // So is one among those reported before the note.
        conflicts[0] = problem(TRANSACTION_NAME, "tx.limit");
        assert!(matches!(verdict(&conflicts, || true), Verdict::TooLarge(_)));
    }

    #[test]
    fn problems_elsewhere_are_planned_again_while_the_world_loads() {
        let elsewhere = [problem("planes/meadow/stray.txt", "source.unexpected_file")];
        assert_eq!(verdict(&elsewhere, || true), Verdict::Replan);
        assert_eq!(verdict(&elsewhere, || false), Verdict::Wait);
        let disk = [problem(TRANSACTION_NAME, "tx.io")];
        assert_eq!(verdict(&disk, || true), Verdict::Wait);
    }
}
