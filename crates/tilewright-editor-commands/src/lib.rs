// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! Editor commands are world toolkit transactions (`tilewright.transaction/1`).
//! [`preview`] carries one out against an open world in memory: the toolkit
//! edits the files losslessly, validates the result and returns the sealed
//! transaction and its exact inverse, writing nothing. The [`brush`] turns
//! strokes into terrain operations, and a [`session`] collects the edits of
//! an interaction and saves them through the toolkit when it ends, keeping
//! each save's inverse for undo.

pub mod brush;
pub mod session;

use std::{fmt, path::Path};
use tilewright_edit::{
    FileChange, Options, Plan, PublishError, PublishOptions, Published, Transaction,
};

use tilewright_editor_document::WorldDocument;
use tilewright_jsonc::write_json_string;
use tilewright_source::{Diagnostic, World};

/// `text` as a JSON string, quoted and escaped, for operations that carry
/// text.
#[must_use]
pub fn json_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    write_json_string(text, &mut quoted);
    quoted
}

/// Who the transactions the editor writes name as their actor.
pub const ACTOR: &str = "editor:tilewright-editor";

/// Longest transaction title, in characters (TRANSACTIONS.md section 1).
pub const MAXIMUM_TITLE: usize = 256;

/// A transaction document that carries out `operations`, each the text of
/// one operation, in order. Its title is `title`, cut to
/// [`MAXIMUM_TITLE`] characters; its actor is [`ACTOR`]; and its `custom`
/// data records `commands`, each the JSON text of an editor command and its
/// parameters, under [`brush::NAMESPACE`].
#[must_use]
pub fn transaction(title: &str, operations: &[String], commands: &[String]) -> String {
    let title: String = title.chars().take(MAXIMUM_TITLE).collect();
    let mut text = String::from("{\n  \"format\": \"tilewright.transaction/1\",\n  \"title\": ");
    write_json_string(&title, &mut text);
    text.push_str(",\n  \"actor\": ");
    write_json_string(ACTOR, &mut text);
    text.push_str(",\n  \"operations\": [\n");
    // Each operation one level in; their lines are structure only, since
    // strings hold no raw line breaks.
    let indented: Vec<String> = operations
        .iter()
        .map(|operation| {
            operation
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    text.push_str(&indented.join(",\n"));
    text.push_str("\n  ],\n  \"custom\": {");
    write_json_string(brush::NAMESPACE, &mut text);
    text.push_str(": {\"commands\": [");
    text.push_str(&commands.join(", "));
    text.push_str("]}}\n}\n");
    text
}

/// A transaction carried out in memory against the tree of an open world.
#[derive(Debug)]
pub struct Preview {
    plan: Plan,
}

impl Preview {
    /// Every file the transaction touched or consulted, by path.
    #[must_use]
    pub fn files(&self) -> &[FileChange] {
        self.plan.files()
    }

    /// The files the transaction changes, by path.
    pub fn changed(&self) -> impl Iterator<Item = &FileChange> + '_ {
        self.plan.files().iter().filter(|file| file.is_changed())
    }

    /// The validated world after the transaction.
    #[must_use]
    pub fn world(&self) -> &World {
        self.plan.world()
    }

    /// The transaction with the digests of every file it touched or
    /// consulted, before and after.
    #[must_use]
    pub fn sealed(&self) -> &str {
        self.plan.sealed()
    }

    /// The transaction that, carried out after this one, restores every file
    /// byte for byte.
    #[must_use]
    pub fn inverse(&self) -> &str {
        self.plan.inverse()
    }

    /// Checks that this save can be undone: the check every forward save
    /// passes before anything is written, from the session and from `paint
    /// --save` alike. The toolkit plans the save's inverse against the tree
    /// the save leaves, and the inverse's own inverse against the tree that
    /// leaves, with every check planning makes; nothing is read from disk.
    /// Undo and redo then publish one of the two, and are not checked again.
    ///
    /// # Errors
    /// [`UndoError::TooLarge`] when only limits of one transaction refuse
    /// the undo or redo, so a smaller edit would save, and
    /// [`UndoError::Refused`] for any other reason.
    pub fn check_undo(&self) -> Result<(), UndoError> {
        self.plan.check_undo().map_err(UndoError::from_problems)
    }

    /// Writes the inverse to a new file at `path`, atomically, through the
    /// toolkit: the recovery record to keep before publishing. Returns
    /// whether the new name is durable too.
    ///
    /// # Errors
    /// Fails if `path` exists or cannot be written.
    pub fn record_inverse(&self, path: &Path) -> std::io::Result<bool> {
        tilewright_edit::write_new(path, self.plan.inverse())
    }

    /// What publishing this transaction would not check, in words to show
    /// the user before they accept it: on macOS and Windows, and on Linux
    /// for a tree on a file system whose access control lists Linux does not
    /// show. `None` where publishing checks everything.
    ///
    /// # Errors
    /// The toolkit's diagnostics for a directory it could not read.
    pub fn unchecked_access(&self) -> Result<Option<String>, Vec<Diagnostic>> {
        self.plan.unchecked_access()
    }

    /// Publishes the transaction's files to the tree, through the toolkit.
    /// Where [`Preview::unchecked_access`] names what would not be checked,
    /// that is refused unless `unchecked_access` accepts it.
    ///
    /// # Errors
    /// Fails as the toolkit's publish does, saying whether the tree changed.
    pub fn publish(&self, unchecked_access: bool) -> Result<Published, PublishError> {
        self.plan.publish_with(&PublishOptions { unchecked_access })
    }
}

/// Carries out the transaction `text`, named `name` in diagnostics, on the
/// tree of `document` as it is now, without writing anything. Its
/// preconditions are its own: cell and value expectations, tests, and the
/// file digests of its `base`.
///
/// # Errors
/// Returns the transaction's problems, each at its operation, or the
/// diagnostics of the edited world.
pub fn preview(document: &WorldDocument, name: &str, text: &str) -> Result<Preview, Error> {
    let transaction = Transaction::parse(name, text).map_err(Error::Rejected)?;
    let plan = tilewright_edit::plan(document.root(), &transaction, &Options::default())
        .map_err(Error::Rejected)?;
    Ok(Preview { plan })
}

/// The codes of a limit of one transaction: the toolkit's budgets and
/// counts, and the bounds the JSONC parser holds a transaction's text to.
/// An edit refused only by these would save if it were smaller.
const LIMITS: [&str; 10] = [
    "tx.limit",
    "jsonc.array_limit",
    "jsonc.depth_limit",
    "jsonc.input_too_large",
    "jsonc.line_too_long",
    "jsonc.object_limit",
    "jsonc.string_too_long",
    "jsonc.too_many_comments",
    "jsonc.too_many_lines",
    "jsonc.value_limit",
];

/// Whether `code` is a limit of one transaction: one of the toolkit's
/// budgets or counts (`tx.limit`), or a bound the JSONC parser holds a
/// transaction's text to, such as `jsonc.too_many_lines`.
#[must_use]
pub fn is_limit(code: &str) -> bool {
    LIMITS.contains(&code)
}

/// Why a save could not be undone, found before anything is written.
#[derive(Debug)]
pub enum UndoError {
    /// Only limits of one transaction refuse its undo or redo, so a smaller
    /// edit would save: why, in the toolkit's words.
    TooLarge(String),
    /// The toolkit could not plan its undo or redo for another reason: its
    /// diagnostics.
    Refused(Vec<Diagnostic>),
}

impl UndoError {
    /// What the toolkit's refusal of an undo means: too large when every
    /// problem is a limit, refused otherwise.
    fn from_problems(problems: Vec<Diagnostic>) -> Self {
        if problems.iter().all(|problem| is_limit(problem.code)) {
            let reason = problems
                .first()
                .map_or_else(String::new, |problem| problem.message.clone());
            Self::TooLarge(reason)
        } else {
            Self::Refused(problems)
        }
    }
}

impl fmt::Display for UndoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(reason) => {
                write!(formatter, "it is too large to save as one step: {reason}")
            }
            Self::Refused(problems) => {
                write!(
                    formatter,
                    "its undo could not be planned: {} problem(s)",
                    problems.len()
                )?;
                if let Some(first) = problems.first() {
                    write!(formatter, "; first: {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for UndoError {}

#[derive(Debug)]
pub enum Error {
    /// The transaction is malformed, a precondition failed, or the edited
    /// world does not validate: the toolkit's diagnostics.
    Rejected(Vec<Diagnostic>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(diagnostics) => {
                write!(
                    formatter,
                    "the transaction was rejected: {} problem(s)",
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
    use super::{Error, UndoError, preview};
    use tilewright_editor_document::WorldDocument;
    use tilewright_editor_testkit::fixture_world;

    const PAINT: &str = r#"{
  "format": "tilewright.transaction/1",
  "title": "Paint sand into the pond and a new chunk",
  "actor": "test:commands",
  "operations": [
    {"op": "terrain.set", "plane": "meadow", "layer": 0, "grid": "ground",
     "cells": [{"at": [10, 10], "value": "sand", "expect": "water"},
               {"at": [32, 0], "value": "sand", "expect": null}]}
  ]
}
"#;

    #[test]
    fn previews_a_transaction_without_writing() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let preview = preview(&document, "paint.jsonc", PAINT).unwrap();
        let changed: Vec<(&str, bool)> = preview
            .changed()
            .map(|file| (file.path.as_str(), file.before.is_none()))
            .collect();
        assert_eq!(
            changed,
            [
                ("planes/meadow/layers/0/0_0.jsonc", false),
                ("planes/meadow/layers/0/1_0.jsonc", true),
            ]
        );
        let plane = &preview.world().planes()["meadow"];
        let pond = &plane.chunks[&(0, 0, 0)].ground[0];
        assert_eq!(pond.value(10 * 32 + 10), Some("sand"));
        assert!(plane.chunks.contains_key(&(0, 1, 0)));
        assert!(preview.sealed().contains("\"base\""));
        assert!(preview.inverse().contains("document.delete"));
        // Previewing wrote nothing: the tree still has its revision.
        let reopened = WorldDocument::open(document.root()).unwrap();
        assert_eq!(reopened.revision(), document.revision());
    }

    #[test]
    fn a_failed_expectation_is_a_conflict() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let stale = PAINT.replace(r#""expect": "water""#, r#""expect": "grass""#);
        let Err(Error::Rejected(diagnostics)) = preview(&document, "paint.jsonc", &stale) else {
            panic!("a stale expectation was accepted");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "tx.conflict"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn a_malformed_transaction_is_rejected() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let Err(Error::Rejected(diagnostics)) = preview(&document, "bad.jsonc", "{\"format\": 1}")
        else {
            panic!("a malformed transaction was accepted");
        };
        assert!(!diagnostics.is_empty());
    }

    fn problem(code: &'static str) -> tilewright_source::Diagnostic {
        tilewright_source::Diagnostic {
            path: "<transaction>".to_owned(),
            start: 0,
            end: 0,
            line: 0,
            column: 0,
            code,
            message: format!("{code}: the transaction could not be undone"),
        }
    }

    #[test]
    fn an_undo_refused_only_by_limits_is_too_large() {
        for codes in [&["tx.limit"][..], &["jsonc.too_many_lines", "tx.limit"]] {
            let problems = codes.iter().map(|code| problem(code)).collect();
            assert!(
                matches!(UndoError::from_problems(problems), UndoError::TooLarge(reason) if reason.starts_with(codes[0])),
                "{codes:?}"
            );
        }
        // Splitting would not help with anything else.
        for codes in [
            &["tx.conflict"][..],
            &["tx.limit", "tx.shape"],
            &["jsonc.expected_value"],
        ] {
            let problems = codes.iter().map(|code| problem(code)).collect();
            assert!(
                matches!(UndoError::from_problems(problems), UndoError::Refused(_)),
                "{codes:?}"
            );
        }
    }
}
