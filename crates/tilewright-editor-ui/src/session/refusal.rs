// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Coded refusals. Every [`SessionError`] carries a stable, machine-readable
//! [`RefusalCode`] beside the message a person reads. Messages may be
//! reworded; codes may not. `docs/ENGINE.md` lists every code.
use std::fmt;

/// Declares [`RefusalCode`], [`RefusalCode::ALL`] and
/// [`RefusalCode::as_str`] from one list, so no code can be left out of
/// `ALL` (and so out of its uniqueness and documentation tests).
macro_rules! refusal_codes {
    ($($(#[doc = $doc:literal])* $variant:ident => $text:literal,)*) => {
        /// Why the engine refused an action, as a stable dotted code: a group
        /// (`grant`, `scope`, `target`, ...) and the reason within it.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[non_exhaustive]
        pub enum RefusalCode {
            $($(#[doc = $doc])* $variant,)*
        }

        impl RefusalCode {
            /// Every code, in documentation order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// The stable code text.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }
        }
    };
}

refusal_codes! {
    /// The session is blocked after an uncertain publication or failed
    /// reopen; only an explicit Reopen inspection is accepted.
    SessionBlocked => "session.blocked",
    /// A change is pending review: Accept or Cancel it first.
    PendingExists => "pending.exists",
    /// Accept found no pending change.
    PendingNone => "pending.none",
    /// The action names a source revision other than the open one.
    RevisionStale => "revision.stale",
    /// The source on disk differs from the World the session opened.
    SourceChanged => "source.changed",
    /// The source cannot be read, is not UTF-8, does not parse, does not
    /// load, or lacks a document the action needs.
    SourceInvalid => "source.invalid",
    /// The editor catalog or its resources no longer verify.
    CatalogRefused => "catalog.refused",
    /// A selection names a stale frame or an unknown semantic key.
    SelectionStale => "selection.stale",
    /// The catalog does not grant this edit.
    GrantMissing => "grant.missing",
    /// The target is outside the catalog's active view, or, for zone
    /// management and anchor deletion, outside the view's plane. Carried
    /// objects are [`RefusalCode::TargetUnsupported`].
    ScopeOutsideView => "scope.outside_view",
    /// The named object, plane, zone, anchor, grid or field does not exist.
    TargetUnknown => "target.unknown",
    /// A new id is already used.
    TargetExists => "target.exists",
    /// The target is still referenced and cannot be deleted.
    TargetInUse => "target.in_use",
    /// The target exists but cannot take this edit (for example an
    /// object carried in an inventory).
    TargetUnsupported => "target.unsupported",
    /// A zone parent would make a cycle.
    HierarchyCycle => "hierarchy.cycle",
    /// A new id does not have the World identifier shape.
    InputIdentifier => "input.identifier",
    /// The action's input is malformed (for example unordered cells).
    InputInvalid => "input.invalid",
    /// The action's input exceeds a value, length or count budget.
    InputBudget => "input.budget",
    /// The action would change nothing.
    ChangeNone => "change.none",
    /// The actor given to the session is not a valid transaction actor.
    ActorInvalid => "actor.invalid",
    /// World refused the prepared transaction; nothing is pending.
    WorldRefused => "world.refused",
    /// The pending change's review is incomplete, so Accept is refused.
    ReviewIncomplete => "review.incomplete",
    /// History has no room for another step.
    HistoryFull => "history.full",
    /// Undo found no step to undo.
    HistoryNothingToUndo => "history.nothing_to_undo",
    /// Redo found no step to redo.
    HistoryNothingToRedo => "history.nothing_to_redo",
    /// The recovery store did not retain the inverse; nothing was published.
    RecoveryFailed => "recovery.failed",
    /// Publication failed; the status reports what was applied.
    PublishFailed => "publish.failed",
    /// Publication was applied, but the reopened source differs from the
    /// reviewed result or does not reopen.
    PublishUnverified => "publish.unverified",
    /// Reopen inspection found differing source or an uncertain
    /// publication.
    ReopenBlocked => "reopen.blocked",
    /// No unused uid could be drawn for a new object.
    UidUnavailable => "uid.unavailable",
    /// An unexpected internal failure: a bug, not a property of the input.
    Internal => "internal.unexpected",
}

impl RefusalCode {
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
        // Every code row of the table names a code that exists.
        let listed: Vec<&str> = documented
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split_once('`'))
            .map(|(code, _)| code)
            .collect();
        assert_eq!(listed.len(), texts.len());
        for code in listed {
            assert!(
                texts.contains(code),
                "docs/ENGINE.md lists unknown code {code}"
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
