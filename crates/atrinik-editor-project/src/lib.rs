// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use std::{collections::BTreeSet, fmt};

pub const MAX_PATH_BYTES: usize = 1_024;
pub const MAX_OPEN_DOCUMENTS: usize = 256;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn new(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref();
        if value.is_empty()
            || value.len() > MAX_PATH_BYTES
            || value.contains([':', '\\'])
            || value.chars().any(char::is_control)
            || value.starts_with('/')
            || value.starts_with("//")
            || value.split('/').any(|part| !portable_segment(part))
        {
            return Err(Error::InvalidPath);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn portable_segment(part: &str) -> bool {
    if part.is_empty() || matches!(part, "." | "..") || part.ends_with(['.', ' ']) {
        return false;
    }
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileKind {
    Regular,
    Directory,
    Symlink,
    Special,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileStamp {
    pub canonical: String,
    pub identity: u128,
    pub revision: [u8; 32],
    pub kind: FileKind,
}

pub trait PathProbe {
    fn inspect(&self, relative: &RelativePath) -> Result<FileStamp, Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectedWrite {
    relative: RelativePath,
    stamp: FileStamp,
}

impl InspectedWrite {
    #[must_use]
    pub fn relative(&self) -> &RelativePath {
        &self.relative
    }

    #[must_use]
    pub fn stamp(&self) -> &FileStamp {
        &self.stamp
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathPolicy {
    canonical_root: String,
    writable_prefixes: Vec<RelativePath>,
    denied_prefixes: Vec<RelativePath>,
}

impl PathPolicy {
    pub fn new(
        canonical_root: impl Into<String>,
        writable_prefixes: Vec<RelativePath>,
        denied_prefixes: Vec<RelativePath>,
    ) -> Result<Self, Error> {
        let canonical_root = canonical_root.into();
        if canonical_root.is_empty()
            || canonical_root.len() > MAX_PATH_BYTES
            || writable_prefixes.is_empty()
        {
            return Err(Error::InvalidPolicy);
        }
        Ok(Self {
            canonical_root,
            writable_prefixes,
            denied_prefixes,
        })
    }

    pub fn inspect<P: PathProbe>(
        &self,
        probe: &P,
        relative: RelativePath,
    ) -> Result<InspectedWrite, Error> {
        self.check_relative(&relative)?;
        let stamp = probe.inspect(&relative)?;
        self.check_stamp(&relative, &stamp)?;
        Ok(InspectedWrite { relative, stamp })
    }

    pub fn revalidate<P: PathProbe>(
        &self,
        probe: &P,
        inspected: &InspectedWrite,
    ) -> Result<FileStamp, Error> {
        self.check_relative(&inspected.relative)?;
        let current = probe.inspect(&inspected.relative)?;
        self.check_stamp(&inspected.relative, &current)?;
        if current != inspected.stamp {
            return Err(Error::ExternalChange);
        }
        Ok(current)
    }

    fn check_relative(&self, relative: &RelativePath) -> Result<(), Error> {
        let path = relative.as_str();
        if self
            .denied_prefixes
            .iter()
            .any(|prefix| within(path, prefix.as_str()))
            || !self
                .writable_prefixes
                .iter()
                .any(|prefix| within(path, prefix.as_str()))
        {
            return Err(Error::WriteDenied);
        }
        Ok(())
    }

    fn check_stamp(&self, relative: &RelativePath, stamp: &FileStamp) -> Result<(), Error> {
        if stamp.kind != FileKind::Regular {
            return Err(match stamp.kind {
                FileKind::Symlink => Error::SymlinkEscape,
                _ => Error::UnsupportedFile,
            });
        }
        let expected = format!(
            "{}/{}",
            self.canonical_root.trim_end_matches('/'),
            relative.as_str()
        );
        if stamp.canonical != expected {
            return Err(Error::CanonicalEscape);
        }
        Ok(())
    }
}

fn within(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectState {
    open: BTreeSet<RelativePath>,
    active: Option<RelativePath>,
    generation: u64,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            open: BTreeSet::new(),
            active: None,
            generation: 1,
        }
    }
}

impl ProjectState {
    pub fn open(&mut self, path: RelativePath) -> Result<(), Error> {
        if !self.open.contains(&path) && self.open.len() >= MAX_OPEN_DOCUMENTS {
            return Err(Error::LimitExceeded);
        }
        self.open.insert(path.clone());
        self.active = Some(path);
        self.bump()?;
        Ok(())
    }

    pub fn close(&mut self, path: &RelativePath) -> Result<(), Error> {
        if !self.open.remove(path) {
            return Err(Error::NotOpen);
        }
        if self.active.as_ref() == Some(path) {
            self.active = self.open.iter().next_back().cloned();
        }
        self.bump()?;
        Ok(())
    }

    #[must_use]
    pub fn active(&self) -> Option<&RelativePath> {
        self.active.as_ref()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    fn bump(&mut self) -> Result<(), Error> {
        self.generation = self.generation.checked_add(1).ok_or(Error::LimitExceeded)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidPath,
    InvalidPolicy,
    WriteDenied,
    CanonicalEscape,
    SymlinkEscape,
    UnsupportedFile,
    ExternalChange,
    LimitExceeded,
    NotOpen,
    Probe(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "editor project error: {self:?}")
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Error, FileKind, FileStamp, PathPolicy, PathProbe, ProjectState, RelativePath};

    struct Probe(FileStamp);
    impl PathProbe for Probe {
        fn inspect(&self, _: &RelativePath) -> Result<FileStamp, Error> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn rejects_path_attacks_and_denied_outputs() {
        for path in [
            "",
            "../map",
            "/map",
            "C:/map",
            "maps//x",
            "maps/./x",
            "maps\\x",
            "maps/file:stream",
            "maps/NUL.txt",
            "maps/com1",
            "maps/trailing.",
            "maps/trailing ",
            "maps/control\nname",
        ] {
            assert_eq!(RelativePath::new(path), Err(Error::InvalidPath));
        }
        let policy = PathPolicy::new(
            "/project",
            vec![RelativePath::new("maps").unwrap()],
            vec![RelativePath::new("maps/generated").unwrap()],
        )
        .unwrap();
        assert_eq!(
            policy.inspect(
                &Probe(FileStamp {
                    canonical: "/project/maps/generated/x".into(),
                    identity: 1,
                    revision: [1; 32],
                    kind: FileKind::Regular
                }),
                RelativePath::new("maps/generated/x").unwrap(),
            ),
            Err(Error::WriteDenied)
        );
    }

    #[test]
    fn revalidation_detects_identity_and_revision_changes() {
        let path = RelativePath::new("maps/a").unwrap();
        let first = FileStamp {
            canonical: "/project/maps/a".into(),
            identity: 7,
            revision: [1; 32],
            kind: FileKind::Regular,
        };
        let policy =
            PathPolicy::new("/project", vec![RelativePath::new("maps").unwrap()], vec![]).unwrap();
        let inspected = policy.inspect(&Probe(first.clone()), path).unwrap();
        let mut changed = first;
        changed.revision = [2; 32];
        assert_eq!(
            policy.revalidate(&Probe(changed), &inspected),
            Err(Error::ExternalChange)
        );
    }

    #[test]
    fn tab_state_is_bounded_and_deterministic() {
        let mut state = ProjectState::default();
        let a = RelativePath::new("maps/a").unwrap();
        let b = RelativePath::new("maps/b").unwrap();
        state.open(a.clone()).unwrap();
        state.open(b.clone()).unwrap();
        state.close(&b).unwrap();
        assert_eq!(state.active(), Some(&a));
        assert_eq!(state.generation(), 4);
    }
}
