// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Coded refusals. Every [`SessionError`] carries a stable, machine-readable
//! [`RefusalCode`] beside the message a person reads. Messages may be
//! reworded; codes may not. `docs/ENGINE.md` lists every code.
use std::fmt;

/// Why the engine refused an action, as a stable dotted code: a group
/// (`grant`, `scope`, `target`, ...) and the reason within it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum RefusalCode {
    /// The session is blocked after an uncertain publication or failed
    /// reopen; only an explicit Reopen inspection is accepted.
    SessionBlocked,
    /// A change is pending review: Accept or Cancel it first.
    PendingExists,
    /// Accept found no pending change.
    PendingNone,
    /// The action names a source revision other than the open one.
    RevisionStale,
    /// The source on disk differs from the World the session opened.
    SourceChanged,
    /// The source cannot be read, is not UTF-8, does not parse, does not
    /// load, or lacks a document the action needs.
    SourceInvalid,
    /// The editor catalog or its resources no longer verify.
    CatalogRefused,
    /// A selection names a stale frame or an unknown semantic key.
    SelectionStale,
    /// The catalog does not grant this edit.
    GrantMissing,
    /// The target is outside the catalog's active view, or is not a placed
    /// object of it.
    ScopeOutsideView,
    /// The named object, plane, zone, anchor, grid or field does not exist.
    TargetUnknown,
    /// A new id is already used.
    TargetExists,
    /// The target is still referenced and cannot be deleted.
    TargetInUse,
    /// The target exists but cannot take this edit (for example an
    /// object carried in an inventory).
    TargetUnsupported,
    /// A zone parent would make a cycle.
    HierarchyCycle,
    /// A new id does not have the World identifier shape.
    InputIdentifier,
    /// The action's input is malformed (for example unordered cells).
    InputInvalid,
    /// The action's input exceeds a value, length or count budget.
    InputBudget,
    /// The action would change nothing.
    ChangeNone,
    /// The actor given to the session is not a valid transaction actor.
    ActorInvalid,
    /// World refused the prepared transaction; nothing is pending.
    WorldRefused,
    /// The pending change's review is incomplete, so Accept is refused.
    ReviewIncomplete,
    /// History has no room for another step.
    HistoryFull,
    /// Undo found no step to undo.
    HistoryNothingToUndo,
    /// Redo found no step to redo.
    HistoryNothingToRedo,
    /// The recovery store did not retain the inverse; nothing was published.
    RecoveryFailed,
    /// Publication failed; the status reports what was applied.
    PublishFailed,
    /// Publication was applied, but the reopened source differs from the
    /// reviewed result or does not reopen.
    PublishUnverified,
    /// Reopen inspection found differing source or an uncertain
    /// publication.
    ReopenBlocked,
    /// No unused uid could be drawn for a new object.
    UidUnavailable,
    /// An unexpected internal failure: a bug, not a property of the input.
    Internal,
}

impl RefusalCode {
    /// Every code, in documentation order.
    pub const ALL: [Self; 31] = [
        Self::SessionBlocked,
        Self::PendingExists,
        Self::PendingNone,
        Self::RevisionStale,
        Self::SourceChanged,
        Self::SourceInvalid,
        Self::CatalogRefused,
        Self::SelectionStale,
        Self::GrantMissing,
        Self::ScopeOutsideView,
        Self::TargetUnknown,
        Self::TargetExists,
        Self::TargetInUse,
        Self::TargetUnsupported,
        Self::HierarchyCycle,
        Self::InputIdentifier,
        Self::InputInvalid,
        Self::InputBudget,
        Self::ChangeNone,
        Self::ActorInvalid,
        Self::WorldRefused,
        Self::ReviewIncomplete,
        Self::HistoryFull,
        Self::HistoryNothingToUndo,
        Self::HistoryNothingToRedo,
        Self::RecoveryFailed,
        Self::PublishFailed,
        Self::PublishUnverified,
        Self::ReopenBlocked,
        Self::UidUnavailable,
        Self::Internal,
    ];

    /// The stable code text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionBlocked => "session.blocked",
            Self::PendingExists => "pending.exists",
            Self::PendingNone => "pending.none",
            Self::RevisionStale => "revision.stale",
            Self::SourceChanged => "source.changed",
            Self::SourceInvalid => "source.invalid",
            Self::CatalogRefused => "catalog.refused",
            Self::SelectionStale => "selection.stale",
            Self::GrantMissing => "grant.missing",
            Self::ScopeOutsideView => "scope.outside_view",
            Self::TargetUnknown => "target.unknown",
            Self::TargetExists => "target.exists",
            Self::TargetInUse => "target.in_use",
            Self::TargetUnsupported => "target.unsupported",
            Self::HierarchyCycle => "hierarchy.cycle",
            Self::InputIdentifier => "input.identifier",
            Self::InputInvalid => "input.invalid",
            Self::InputBudget => "input.budget",
            Self::ChangeNone => "change.none",
            Self::ActorInvalid => "actor.invalid",
            Self::WorldRefused => "world.refused",
            Self::ReviewIncomplete => "review.incomplete",
            Self::HistoryFull => "history.full",
            Self::HistoryNothingToUndo => "history.nothing_to_undo",
            Self::HistoryNothingToRedo => "history.nothing_to_redo",
            Self::RecoveryFailed => "recovery.failed",
            Self::PublishFailed => "publish.failed",
            Self::PublishUnverified => "publish.unverified",
            Self::ReopenBlocked => "reopen.blocked",
            Self::UidUnavailable => "uid.unavailable",
            Self::Internal => "internal.unexpected",
        }
    }

    /// The code a History error is refused with.
    #[must_use]
    pub fn of_history(error: &tilewright_editor_commands::Error) -> Self {
        use tilewright_editor_commands::Error;
        match error {
            Error::Rejected(_) => Self::WorldRefused,
            Error::Document(_) => Self::SourceInvalid,
            Error::StaleRevision => Self::RevisionStale,
            Error::HistoryFull => Self::HistoryFull,
            Error::NothingToUndo => Self::HistoryNothingToUndo,
            Error::NothingToRedo => Self::HistoryNothingToRedo,
            Error::NothingPrepared => Self::PendingNone,
            Error::NeedsReload => Self::SessionBlocked,
            Error::Recovery(_) => Self::RecoveryFailed,
            Error::Publish(_) => Self::PublishFailed,
            Error::PublishedStateChanged { .. } => Self::PublishUnverified,
        }
    }
}

impl fmt::Display for RefusalCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refusal: a stable code and the message shown to a person. `Display`
/// writes the message alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionError {
    code: RefusalCode,
    message: String,
}

impl SessionError {
    #[must_use]
    pub fn new(code: RefusalCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// A History error with its code and its own message.
    #[must_use]
    pub fn history(error: &tilewright_editor_commands::Error) -> Self {
        Self::new(RefusalCode::of_history(error), error.to_string())
    }

    #[must_use]
    pub const fn code(&self) -> RefusalCode {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SessionError {}

/// A refusal with `code` and `message`.
pub(crate) fn refuse(code: RefusalCode, message: &str) -> SessionError {
    SessionError::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::RefusalCode;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_dotted_and_documented_once() {
        let texts: BTreeSet<&str> = RefusalCode::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(texts.len(), RefusalCode::ALL.len());
        for text in &texts {
            let (group, reason) = text.split_once('.').expect("group.reason");
            assert!(!group.is_empty() && !reason.is_empty() && !reason.contains('.'));
            assert!(
                text.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'),
                "{text}"
            );
        }
        let documented = include_str!("../../../../docs/ENGINE.md");
        for text in &texts {
            assert_eq!(
                documented.matches(&format!("`{text}`")).count(),
                1,
                "{text} must be listed exactly once in docs/ENGINE.md"
            );
        }
        // Only one code is reserved for unexpected internal failures.
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.starts_with("internal."))
                .count(),
            1
        );
    }
}
