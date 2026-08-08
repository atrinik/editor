// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use atrinik_editor_project::RelativePath;
use std::{collections::BTreeMap, fmt};

pub const MAX_PANEL_DIAGNOSTICS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Panel {
    Project,
    Catalog,
    Inspector,
    Diagnostics,
    History,
    Preview,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    pub document: RelativePath,
    pub semantic_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLink {
    pub document: RelativePath,
    pub start: usize,
    pub end: usize,
    pub code: String,
}

#[derive(Clone, Debug)]
pub struct UiState {
    panels: BTreeMap<Panel, bool>,
    selection: Option<Selection>,
    diagnostics: Vec<DiagnosticLink>,
}

impl Default for UiState {
    fn default() -> Self {
        let panels = [
            Panel::Project,
            Panel::Catalog,
            Panel::Inspector,
            Panel::Diagnostics,
            Panel::History,
            Panel::Preview,
        ]
        .into_iter()
        .map(|panel| (panel, true))
        .collect();
        Self {
            panels,
            selection: None,
            diagnostics: Vec::new(),
        }
    }
}

impl UiState {
    pub fn set_panel_visible(&mut self, panel: Panel, visible: bool) {
        self.panels.insert(panel, visible);
    }

    pub fn select(&mut self, selection: Selection) -> Result<(), Error> {
        if selection.semantic_id == 0 {
            return Err(Error::InvalidSelection);
        }
        self.selection = Some(selection);
        Ok(())
    }

    pub fn replace_diagnostics(&mut self, diagnostics: Vec<DiagnosticLink>) -> Result<(), Error> {
        if diagnostics.len() > MAX_PANEL_DIAGNOSTICS
            || diagnostics.iter().any(|diagnostic| {
                diagnostic.code.is_empty()
                    || diagnostic.code.len() > 128
                    || diagnostic.start > diagnostic.end
            })
        {
            return Err(Error::InvalidDiagnostics);
        }
        self.diagnostics = diagnostics;
        Ok(())
    }

    #[must_use]
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[DiagnosticLink] {
        &self.diagnostics
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidSelection,
    InvalidDiagnostics,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "editor UI model error: {self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Error, Selection, UiState};
    use atrinik_editor_project::RelativePath;

    #[test]
    fn semantic_selection_never_depends_on_pixels() {
        let mut ui = UiState::default();
        assert_eq!(
            ui.select(Selection {
                document: RelativePath::new("maps/a").unwrap(),
                semantic_id: 0
            }),
            Err(Error::InvalidSelection)
        );
        ui.select(Selection {
            document: RelativePath::new("maps/a").unwrap(),
            semantic_id: 42,
        })
        .unwrap();
        assert_eq!(ui.selection().unwrap().semantic_id, 42);
    }
}
