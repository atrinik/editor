// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The only preview I/O seam. All reads use the caller's retained rooted authority.
use crate::{PreviewCatalog, PreviewError, PreviewLimits, failure, hash};
use std::{collections::BTreeMap, sync::Arc};
use tilewright_render_resources::{ResourceBlob, ResourceProvider, ResourceRequest, Rgba8Image};
use tilewright_render_world::TextureRef;
use tilewright_scene::{Digest256, ResourceId};
use tilewright_source::{SourceRoot, World};

#[derive(Clone)]
pub struct ReadyResources {
    pub(crate) catalog_revision: [u8; 32],
    pub(crate) textures: BTreeMap<String, TextureRef>,
    pub(crate) provider: Arc<MemoryResources>,
    pub(crate) vector: Vec<(String, [u8; 32])>,
}

impl ReadyResources {
    /// The ready texture `proto` is drawn with under `catalog`, for UI
    /// previews such as palette thumbnails: the identity the prepared scene
    /// uses, served by the same provider, so nothing is loaded or copied
    /// again. `None` for an unmapped prototype or a different catalog.
    pub fn prototype_texture(&self, catalog: &PreviewCatalog, proto: &str) -> Option<TextureRef> {
        if self.catalog_revision != catalog.revision() {
            return None;
        }
        let (name, _) = catalog.prototypes.get(proto)?;
        self.textures.get(name).copied()
    }
}

pub(crate) struct MemoryResources(pub(crate) BTreeMap<u64, (TextureRef, Vec<u8>)>);
impl ResourceProvider for MemoryResources {
    fn load(
        &self,
        request: ResourceRequest,
    ) -> Result<ResourceBlob, tilewright_render_resources::Error> {
        request.validate()?;
        let (texture, bytes) = self
            .0
            .get(&request.id.0)
            .ok_or(tilewright_render_resources::Error::NotFound)?;
        if texture.revision != request.revision || texture.digest != request.digest {
            return Err(tilewright_render_resources::Error::DigestMismatch);
        }
        if bytes.len() > request.maximum_bytes {
            return Err(tilewright_render_resources::Error::LimitExceeded);
        }
        ResourceBlob::for_request(request, bytes.clone())
    }
}

fn read(root: &SourceRoot, path: &str, maximum: usize) -> Result<Vec<u8>, PreviewError> {
    root.read(
        path,
        u64::try_from(maximum).map_err(|_| failure("read bound overflow"))?,
    )
    .map_err(|e| failure(e.to_string()))?
    .ok_or_else(|| failure(format!("missing preview file: {path}")))
}

pub fn load(
    root: &SourceRoot,
    world: &World,
    limits: PreviewLimits,
) -> Result<(PreviewCatalog, ReadyResources), PreviewError> {
    let bytes = read(root, "preview.jsonc", limits.maximum_catalog_bytes)?;
    let text = String::from_utf8(bytes).map_err(|_| failure("catalog is not UTF-8"))?;
    let catalog = crate::catalog::decode(text, world, limits)?;
    let mut textures = BTreeMap::new();
    let mut memory = BTreeMap::new();
    let mut vector = Vec::new();
    let mut total = 0usize;
    for (index, (name, spec)) in catalog.resources.iter().enumerate() {
        let bytes = read(root, &spec.path, limits.maximum_resource_bytes)?;
        total = total
            .checked_add(bytes.len())
            .filter(|v| *v <= limits.maximum_total_resource_bytes)
            .ok_or_else(|| failure("resource cache budget exceeded"))?;
        let texture = TextureRef {
            id: ResourceId(
                u64::try_from(index)
                    .map_err(|_| failure("resource identity overflow"))?
                    .checked_add(1)
                    .ok_or_else(|| failure("resource identity overflow"))?,
            ),
            revision: 1,
            digest: Digest256(spec.digest),
            width: spec.width,
            height: spec.height,
        };
        let request = ResourceRequest {
            id: texture.id,
            revision: 1,
            digest: texture.digest,
            maximum_bytes: limits.maximum_resource_bytes,
        };
        let blob = ResourceBlob::for_request(request, bytes.clone())
            .map_err(|e| failure(e.to_string()))?;
        let dimensions = Rgba8Image::envelope_dimensions(
            &blob,
            limits.scene.maximum_dimension,
            limits.scene.maximum_pixels,
        )
        .map_err(|e| failure(e.to_string()))?;
        if dimensions != (spec.width, spec.height) {
            return Err(failure("catalog/ATX1 dimension mismatch"));
        }
        vector.push((spec.path.clone(), hash(&bytes)));
        memory.insert(texture.id.0, (texture, bytes));
        textures.insert(name.clone(), texture);
    }
    let cache = ReadyResources {
        catalog_revision: catalog.revision(),
        textures,
        provider: Arc::new(MemoryResources(memory)),
        vector,
    };
    Ok((catalog, cache))
}

/// Recheck capability text and every cached file before pending, publication or picking.
/// A mismatch is actionable: explicitly reopen before using the new capabilities.
pub fn revalidate(
    root: &SourceRoot,
    catalog: &PreviewCatalog,
    cache: &ReadyResources,
    limits: PreviewLimits,
) -> Result<(), PreviewError> {
    if cache.catalog_revision != catalog.revision()
        || hash(&read(root, "preview.jsonc", limits.maximum_catalog_bytes)?) != catalog.revision()
    {
        return Err(failure("preview catalog changed; reopen required"));
    }
    for (path, digest) in &cache.vector {
        if hash(&read(root, path, limits.maximum_resource_bytes)?) != *digest {
            return Err(failure(format!(
                "preview resource changed: {path}; reopen required"
            )));
        }
    }
    Ok(())
}
