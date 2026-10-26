// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::RwLock,
};
use tilewright_editor_project::{Error, FileKind, FileStamp, PathProbe, RelativePath};

/// The root of a committed fixture world under this crate's `fixtures`
/// directory, such as `meadow`: a small top-down world whose source holds
/// only painted terrain, with transitions that derive shores, grass edges
/// and grass details.
#[must_use]
pub fn fixture_world(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// Every file of the `meadow` fixture world, by root-relative path, with
/// its text: for tests that need a copy of the world they can change.
pub const MEADOW_FILES: &[(&str, &str)] = &[
    (
        "defs/proto/clover.jsonc",
        include_str!("../fixtures/meadow/defs/proto/clover.jsonc"),
    ),
    (
        "defs/proto/daisies.jsonc",
        include_str!("../fixtures/meadow/defs/proto/daisies.jsonc"),
    ),
    (
        "defs/proto/grass.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass.jsonc"),
    ),
    (
        "defs/proto/grass_edge_e.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass_edge_e.jsonc"),
    ),
    (
        "defs/proto/grass_edge_n.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass_edge_n.jsonc"),
    ),
    (
        "defs/proto/grass_edge_s.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass_edge_s.jsonc"),
    ),
    (
        "defs/proto/grass_edge_w.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass_edge_w.jsonc"),
    ),
    (
        "defs/proto/grass_tall.jsonc",
        include_str!("../fixtures/meadow/defs/proto/grass_tall.jsonc"),
    ),
    (
        "defs/proto/sand.jsonc",
        include_str!("../fixtures/meadow/defs/proto/sand.jsonc"),
    ),
    (
        "defs/proto/shore_e.jsonc",
        include_str!("../fixtures/meadow/defs/proto/shore_e.jsonc"),
    ),
    (
        "defs/proto/shore_n.jsonc",
        include_str!("../fixtures/meadow/defs/proto/shore_n.jsonc"),
    ),
    (
        "defs/proto/shore_s.jsonc",
        include_str!("../fixtures/meadow/defs/proto/shore_s.jsonc"),
    ),
    (
        "defs/proto/shore_w.jsonc",
        include_str!("../fixtures/meadow/defs/proto/shore_w.jsonc"),
    ),
    (
        "defs/proto/water.jsonc",
        include_str!("../fixtures/meadow/defs/proto/water.jsonc"),
    ),
    (
        "planes/meadow/layers/0/0_0.jsonc",
        include_str!("../fixtures/meadow/planes/meadow/layers/0/0_0.jsonc"),
    ),
    (
        "planes/meadow/plane.jsonc",
        include_str!("../fixtures/meadow/planes/meadow/plane.jsonc"),
    ),
    (
        "profile/profile.jsonc",
        include_str!("../fixtures/meadow/profile/profile.jsonc"),
    ),
    (
        "profile/schema/empty.jsonc",
        include_str!("../fixtures/meadow/profile/schema/empty.jsonc"),
    ),
    (
        "profile/transitions.jsonc",
        include_str!("../fixtures/meadow/profile/transitions.jsonc"),
    ),
    (
        "world.jsonc",
        include_str!("../fixtures/meadow/world.jsonc"),
    ),
];

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
