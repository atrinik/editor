// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use atrinik_source::{Document, Revision};
use atrinik_transaction::Transaction;
use std::{fmt, sync::Arc};

pub const MAX_HISTORY: usize = 1_024;

#[derive(Clone, Debug)]
struct Entry {
    before: Arc<Document>,
    after: Arc<Document>,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    pub fn replace_value(
        &mut self,
        current: Arc<Document>,
        expected: Revision,
        record: usize,
        replacement: &[u8],
    ) -> Result<Arc<Document>, Error> {
        if current.revision() != expected {
            return Err(Error::StaleRevision);
        }
        if self.undo.len() >= MAX_HISTORY {
            return Err(Error::HistoryFull);
        }
        let mut transaction = Transaction::new(current.clone());
        transaction
            .replace_value(record, replacement)
            .map_err(Error::Toolkit)?;
        let after = Arc::new(transaction.preview().map_err(Error::Toolkit)?);
        self.undo.push(Entry {
            before: current,
            after: after.clone(),
        });
        self.redo.clear();
        Ok(after)
    }

    pub fn undo(&mut self, current: &Arc<Document>) -> Result<Arc<Document>, Error> {
        let entry = self.undo.pop().ok_or(Error::NothingToUndo)?;
        if entry.after.revision() != current.revision() {
            self.undo.push(entry);
            return Err(Error::StaleRevision);
        }
        let before = entry.before.clone();
        self.redo.push(entry);
        Ok(before)
    }

    pub fn redo(&mut self, current: &Arc<Document>) -> Result<Arc<Document>, Error> {
        let entry = self.redo.pop().ok_or(Error::NothingToRedo)?;
        if entry.before.revision() != current.revision() {
            self.redo.push(entry);
            return Err(Error::StaleRevision);
        }
        let after = entry.after.clone();
        self.undo.push(entry);
        Ok(after)
    }

    #[must_use]
    pub fn depths(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

#[derive(Debug)]
pub enum Error {
    StaleRevision,
    HistoryFull,
    NothingToUndo,
    NothingToRedo,
    Toolkit(atrinik_source::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "editor command error: {self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Error, History};
    use atrinik_source::{Document, Limits, SourceId};
    use std::sync::Arc;

    fn document() -> Arc<Document> {
        Arc::new(
            Document::parse(
                SourceId::new("test:history").unwrap(),
                Arc::<[u8]>::from(&b"name old\n"[..]),
                Limits::default(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn commands_are_preconditioned_and_reversible() {
        let original = document();
        let mut history = History::default();
        assert!(matches!(
            history.replace_value(
                original.clone(),
                atrinik_source::Document::parse(
                    SourceId::new("test:other").unwrap(),
                    Arc::<[u8]>::from(&b"name other\n"[..]),
                    Limits::default()
                )
                .unwrap()
                .revision(),
                0,
                b"new"
            ),
            Err(Error::StaleRevision)
        ));
        let changed = history
            .replace_value(original.clone(), original.revision(), 0, b"new")
            .unwrap();
        assert_eq!(changed.source_bytes(), b"name new\n");
        let undone = history.undo(&changed).unwrap();
        assert_eq!(undone.source_bytes(), b"name old\n");
        assert_eq!(history.redo(&undone).unwrap().source_bytes(), b"name new\n");
    }
}
