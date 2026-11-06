// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Explicit read-only, digest-pinned application fonts. Toolkit confinement
//! rejects symlink/traversal/special-file inputs; renderer validates all bytes.
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tilewright_render_ui::{FontBlob, FontPack, TextLimits, TextSystem, unicode_fonts::FONTS};
use tilewright_source::SourceRoot;

pub fn load(path: &Path, world: &Path) -> Result<TextSystem, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("font pack: {error}"))?;
    let world = world
        .canonicalize()
        .map_err(|error| format!("source root: {error}"))?;
    if canonical.starts_with(&world) || world.starts_with(&canonical) {
        return Err("font pack and project source roots must be separate".into());
    }
    let root = SourceRoot::new(&canonical);
    let manifest = root
        .read("manifest.json", 1024 * 1024)
        .map_err(|error| error.to_string())?
        .ok_or("font pack manifest missing")?;
    let digest: [u8; 32] = Sha256::digest(&manifest).into();
    if digest != tilewright_render_ui::unicode_fonts::FONT_PACK_MANIFEST_SHA256 {
        return Err("font pack manifest digest mismatch".into());
    }
    let mut fonts = Vec::with_capacity(FONTS.len());
    for font in FONTS {
        let bytes = root
            .read(
                font.path,
                u64::try_from(font.bytes).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("font pack {}: {error}", font.path))?
            .ok_or_else(|| format!("font pack file missing: {}", font.path))?;
        if bytes.len() != font.bytes {
            return Err(format!("font pack byte length mismatch: {}", font.path));
        }
        fonts.push(FontBlob {
            family: font.family.into(),
            bytes: Arc::from(bytes),
            sha256: font.sha256,
        });
    }
    let limits = TextLimits {
        maximum_raster_pixels: (tilewright_render_resources::DEFAULT_MAXIMUM_RESOURCE_BYTES - 12)
            / 4,
        ..TextLimits::default()
    };
    TextSystem::new(
        FontPack::new(fonts, limits).map_err(|error| error.to_string())?,
        limits,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn test_system() -> TextSystem {
    let path = std::env::var_os("TILEWRIGHT_UNICODE_FONT_ROOT")
        .expect("tests require explicit trusted font fixture TILEWRIGHT_UNICODE_FONT_ROOT");
    let world = test_world_root();
    assert!(
        world.is_dir(),
        "neutral World fixture must exist on this platform"
    );
    load(Path::new(&path), &world).expect("verified test font pack")
}

#[cfg(test)]
fn test_world_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tilewright-editor-testkit/fixtures/prototype-neutral")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_world_fixture_exists_and_is_separate_from_explicit_font_pack() {
        let world = test_world_root()
            .canonicalize()
            .expect("platform-independent fixture root");
        assert!(world.is_dir());
        let fonts =
            std::env::var_os("TILEWRIGHT_UNICODE_FONT_ROOT").expect("explicit font fixture");
        let fonts = Path::new(&fonts).canonicalize().unwrap();
        assert!(!world.starts_with(&fonts));
        assert!(!fonts.starts_with(&world));
    }
}
