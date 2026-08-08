// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use atrinik_source::{Document, Limits, Revision, SourceId};
use std::{fmt, sync::Arc};

#[derive(Clone, Debug)]
pub struct DocumentView {
    document: Arc<Document>,
}

impl DocumentView {
    pub fn open(source_id: &str, bytes: Arc<[u8]>) -> Result<Self, Error> {
        let source_id = SourceId::new(source_id).map_err(Error::Toolkit)?;
        let document =
            Document::parse(source_id, bytes, Limits::default()).map_err(Error::Toolkit)?;
        Ok(Self {
            document: Arc::new(document),
        })
    }

    #[must_use]
    pub fn revision(&self) -> Revision {
        self.document.revision()
    }

    #[must_use]
    pub fn document(&self) -> &Arc<Document> {
        &self.document
    }

    #[must_use]
    pub fn diagnostics_len(&self) -> usize {
        self.document.diagnostics().values().len()
    }

    #[must_use]
    pub fn unchanged_bytes(&self) -> &[u8] {
        self.document.source_bytes()
    }
}

#[derive(Debug)]
pub enum Error {
    Toolkit(atrinik_source::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "document adapter error: {self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::DocumentView;
    use std::sync::Arc;

    #[test]
    fn unchanged_document_remains_byte_identical() {
        let bytes: Arc<[u8]> = Arc::from(&b"# retained\r\nname value\r\nunknown custom\r\n"[..]);
        let view = DocumentView::open("project:maps/example", bytes.clone()).unwrap();
        assert_eq!(view.unchanged_bytes(), bytes.as_ref());
    }
}
