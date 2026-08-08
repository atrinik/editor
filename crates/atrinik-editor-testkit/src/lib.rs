// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use atrinik_editor_project::{Error, FileKind, FileStamp, PathProbe, RelativePath};
use std::{collections::BTreeMap, sync::RwLock};

#[derive(Default)]
pub struct MemoryPathProbe {
    files: RwLock<BTreeMap<RelativePath, FileStamp>>,
}

impl MemoryPathProbe {
    pub fn insert(&self, path: RelativePath, stamp: FileStamp) -> Result<(), Error> {
        self.files
            .write()
            .map_err(|_| Error::Probe("poisoned fake".into()))?
            .insert(path, stamp);
        Ok(())
    }

    pub fn regular(root: &str, relative: &RelativePath, identity: u128, revision: u8) -> FileStamp {
        FileStamp {
            canonical: format!("{}/{}", root.trim_end_matches('/'), relative.as_str()),
            identity,
            revision: [revision; 32],
            kind: FileKind::Regular,
        }
    }
}

impl PathProbe for MemoryPathProbe {
    fn inspect(&self, relative: &RelativePath) -> Result<FileStamp, Error> {
        self.files
            .read()
            .map_err(|_| Error::Probe("poisoned fake".into()))?
            .get(relative)
            .cloned()
            .ok_or_else(|| Error::Probe("missing fake path".into()))
    }
}
