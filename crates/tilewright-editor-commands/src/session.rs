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
//!   inverse fits a transaction, and the session that carrying the inverse
//!   out stays within [`crate::MAXIMUM_INVERSE_WORK`].
//! - A save that cannot be made now keeps its edits pending, and no
//!   interaction begins until [`Session::flush`] saves them or
//!   [`Session::discard`] drops them. That covers a file that cannot be read
//!   or written, a world that does not load from disk for reasons of its
//!   own, and a save waiting for the user's consent
//!   ([`Error::NeedsConsent`]). When a publish fails partway and leaves files
//!   it could not restore, its inverse is kept in [`Session::recovery`].
//! - A publish that reports a problem after every file was published, such
//!   as a directory it could not sync, counts as saved, with the problem in
//!   [`Saved::warning`].
//!
//! Time is passed in by the caller, so the rules are deterministic under
//! test.

use crate::{
    Error as PreviewError, MAXIMUM_INVERSE_WORK, Preview,
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
    recovery: Option<String>,
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
    /// It is kept until [`Session::take_recovery`] takes it, or until the
    /// edits it came from are saved.
    #[must_use]
    pub fn recovery(&self) -> Option<&str> {
        self.recovery.as_deref()
    }

    pub const fn take_recovery(&mut self) -> Option<String> {
        self.recovery.take()
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
        let published = match self.publish(&text) {
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
        // These edits are on disk now, so the inverse of an earlier failed
        // publish of them no longer restores anything.
        self.recovery = None;
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
            match self.publish(&save.inverse) {
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
            match self.publish(&save.forward) {
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
    /// world it produced as the session's and rebuilds the preview.
    fn publish(&mut self, text: &str) -> Result<Published, Error> {
        let planned = match preview(&self.document, TRANSACTION_NAME, text) {
            Ok(planned) => planned,
            Err(PreviewError::Rejected(problems)) => return Err(self.classify(problems)),
        };
        let work = planned.inverse_work();
        if work > MAXIMUM_INVERSE_WORK {
            return Err(Error::TooLarge(format!(
                "undoing it could take {} MiB of work, past the {} MiB one undo may take",
                work >> 20,
                MAXIMUM_INVERSE_WORK >> 20
            )));
        }
        // A directory that cannot be read here is a problem of the disk, not
        // of the edits, so they wait like any save that was not published.
        let unchecked = planned.unchecked_access().map_err(|problems| {
            Error::Publish(PublishError {
                applied: false,
                durable: true,
                cleaned: true,
                problems,
            })
        })?;
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
                    self.recovery = Some(planned.inverse().to_owned());
                }
                return Err(Error::Publish(error));
            }
        };
        self.document = self.document.with_world(planned.world().clone());
        self.terrains.clear();
        Ok(Published { planned, warning })
    }

    /// Why the toolkit would not carry a transaction out, as what the session
    /// does about it. A problem with the transaction itself, such as a
    /// conflict, its shape or a limit, is the edits': they are rejected, and
    /// a limit is a save too large. A file that cannot be read is the
    /// disk's: the edits wait. A world that does not validate is the edits'
    /// when the world on disk loads without them, and the disk's when it
    /// does not.
    fn classify(&self, problems: Vec<Diagnostic>) -> Error {
        let disk = |problem: &Diagnostic| matches!(problem.code, "tx.io" | "source.io");
        let waiting = |problems| {
            Error::Publish(PublishError {
                applied: false,
                durable: true,
                cleaned: true,
                problems,
            })
        };
        if problems.iter().all(disk) {
            return waiting(problems);
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
        if own && let Some(limit) = reported.iter().find(|problem| problem.code == "tx.limit") {
            return Error::TooLarge(limit.message.clone());
        }
        if own || WorldDocument::open(self.document.root()).is_ok() {
            return Error::Rejected(PreviewError::Rejected(problems));
        }
        waiting(problems)
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
