// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use crate::{
    PreparedPreview, PreviewCatalog, PreviewDiagnostic, PreviewError, PreviewLimits,
    ReadyResources, SelectionKey, ViewSpec, failure, hash, loader::MemoryResources,
    validate_bounds,
};
use std::{collections::BTreeMap, sync::Arc};
use tilewright_render_world::{Anchor, HeightField, Stretch, TextureRef, WorldObject, WorldView};
use tilewright_scene::{Digest256, ResourceId, SemanticId};
use tilewright_source::{CHUNK_SIZE, World};

// Preserve source insertion order independently of sorted semantic-ID allocation.
type SceneObjects = BTreeMap<SelectionKey, (usize, WorldObject)>;

fn global(chunk: i64, local: u16) -> Result<i64, PreviewError> {
    chunk
        .checked_mul(i64::from(CHUNK_SIZE))
        .and_then(|v| v.checked_add(i64::from(local)))
        .ok_or_else(|| failure("chunk coordinate overflow"))
}
fn inside(bounds: [i64; 4], x: i64, y: i64) -> bool {
    x >= bounds[0] && y >= bounds[1] && x < bounds[0] + bounds[2] && y < bounds[1] + bounds[3]
}
fn rank(world: &World, name: &str) -> Result<u16, PreviewError> {
    world
        .profile()
        .render_layer_index(name)
        .and_then(|n| u16::try_from(n).ok())
        .ok_or_else(|| failure(format!("unsupported render layer: {name}")))
}
fn texture(
    catalog: &PreviewCatalog,
    cache: &ReadyResources,
    proto: &str,
    ground: bool,
) -> Result<(TextureRef, Anchor), PreviewError> {
    let (name, anchor) = catalog
        .prototypes
        .get(proto)
        .ok_or_else(|| failure(format!("unmapped rendered prototype: {proto}")))?;
    if ground && *anchor != Anchor::Ground {
        return Err(failure(format!(
            "terrain prototype {proto} requires ground anchor"
        )));
    }
    let texture = cache
        .textures
        .get(name)
        .copied()
        .ok_or_else(|| failure("missing ready resource"))?;
    Ok((texture, *anchor))
}
/// Where each cell of the cell scene falls in the World scene's draw order,
/// as the shared renderer orders it: the World objects and the cell
/// reference objects are compiled together into one scene that is only
/// read, never drawn, with every cell after the World objects of its own
/// order key. A cell's slot is then the `(depth, painter order)` of the last
/// drawn World quad before it, or `None` when no World quad comes before
/// it. Empty cells get a real slot too. Nothing is projected or ordered
/// here; the renderer's own compile does both. When the combined scene
/// exceeds twice the scene limits the map is empty and callers fall back.
fn cell_slots(
    world: &tilewright_scene::SceneBundle,
    world_objects: &[WorldObject],
    cell_view: &WorldView,
    limits: tilewright_scene::SceneLimits,
) -> BTreeMap<SemanticId, Option<(i16, u32)>> {
    let offset = world_objects
        .iter()
        .map(|object| object.semantic_id.0)
        .max()
        .unwrap_or(0);
    let mut objects = world_objects.to_vec();
    for cell in &cell_view.objects {
        let Some(id) = offset.checked_add(cell.semantic_id.0) else {
            return BTreeMap::new();
        };
        objects.push(WorldObject {
            semantic_id: SemanticId(id),
            ..cell.clone()
        });
    }
    let combined = WorldView {
        objects,
        ..cell_view.clone()
    };
    let doubled = tilewright_scene::SceneLimits {
        maximum_quads: limits.maximum_quads.saturating_mul(2),
        maximum_raster_samples: limits.maximum_raster_samples.saturating_mul(2),
        ..limits
    };
    let Ok(ordered) = tilewright_render_world::compile(&combined, doubled) else {
        return BTreeMap::new();
    };
    // A World quad is the same (identity, part) in both scenes.
    let in_world: BTreeMap<(SemanticId, u16), (i16, u32)> = world
        .quads()
        .iter()
        .map(|quad| {
            (
                (quad.semantic_id, quad.part),
                (quad.depth, quad.painter_order),
            )
        })
        .collect();
    // Scene quads are stored in draw order.
    let mut last = None;
    let mut slots = BTreeMap::new();
    for quad in ordered.quads() {
        if quad.semantic_id.0 > offset {
            slots
                .entry(SemanticId(quad.semantic_id.0 - offset))
                .or_insert(last);
        } else if let Some(slot) = in_world.get(&(quad.semantic_id, quad.part)) {
            last = Some(*slot);
        }
    }
    slots
}

fn identified(
    objects: SceneObjects,
) -> Result<(Vec<WorldObject>, BTreeMap<SemanticId, SelectionKey>), PreviewError> {
    let mut ordered = Vec::with_capacity(objects.len());
    let mut picks = BTreeMap::new();
    for (index, (key, (sequence, mut object))) in objects.into_iter().enumerate() {
        let id = SemanticId(
            u32::try_from(index)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or_else(|| failure("semantic identity overflow"))?,
        );
        object.semantic_id = id;
        picks.insert(id, key);
        ordered.push((sequence, object));
    }
    ordered.sort_by_key(|(sequence, _)| *sequence);
    Ok((
        ordered.into_iter().map(|(_, object)| object).collect(),
        picks,
    ))
}
fn insert(
    objects: &mut SceneObjects,
    key: SelectionKey,
    object: WorldObject,
    limits: PreviewLimits,
) -> Result<(), PreviewError> {
    let sequence = objects.len();
    if sequence >= limits.maximum_objects || objects.insert(key, (sequence, object)).is_some() {
        return Err(failure("duplicate semantic key or object budget exceeded"));
    }
    Ok(())
}

/// Pure adaptation. No source reads or rendering callbacks occur here.
pub fn prepare(
    world: &World,
    catalog: &PreviewCatalog,
    cache: &ReadyResources,
    view: &ViewSpec,
    limits: PreviewLimits,
) -> Result<PreparedPreview, PreviewError> {
    validate_bounds(view.bounds, limits)?;
    if view.frame_revision == 0
        || cache.catalog_revision != catalog.revision()
        || catalog.profile() != (&world.profile().id, &world.profile().version)
        || view.camera.projection != crate::catalog::projection(world)
    {
        return Err(failure("preview profile, cache or generation mismatch"));
    }
    if view.plane != catalog.plane || view.layer != catalog.layer {
        return Err(failure("view plane/layer outside catalog scope"));
    }
    let profile = world.profile();
    let plane = world
        .planes()
        .get(&view.plane)
        .ok_or_else(|| failure("unknown plane"))?;
    if plane.chunks.len() > limits.maximum_chunks {
        return Err(failure("plane chunk budget exceeded"));
    }
    if profile
        .ground_layers
        .iter()
        .enumerate()
        .any(|(n, name)| profile.render_layers.get(n) != Some(name))
    {
        return Err(failure(
            "ground layers must be the profile render-layer prefix",
        ));
    }
    let ground_ranks =
        u16::try_from(profile.ground_layers.len()).map_err(|_| failure("ground layer overflow"))?;
    let adapter = Adapter {
        world,
        catalog,
        cache,
        view,
        limits,
    };
    let heights = adapter.heights()?;
    let mut objects = BTreeMap::new();
    adapter.terrain(&mut objects)?;
    let inventory = adapter.objects(&mut objects)?;
    let (objects, picks) = identified(objects)?;
    let world_view_objects = objects.clone();
    let world_view = WorldView {
        revision: view.frame_revision,
        clock_millis: 0,
        camera: view.camera,
        clear_color: [0.04, 0.05, 0.07, 1.0],
        heights,
        stretch: Stretch::default(),
        ground_layer_ranks: ground_ranks,
        objects,
    };
    let scene = tilewright_render_world::compile(&world_view, limits.scene)
        .map_err(|e| failure(e.to_string()))?;
    let (cells, memory) = adapter.reference()?;
    let (objects, cell_picks) = identified(cells)?;
    let cell_view = WorldView {
        objects,
        ..world_view
    };
    let cell_scene = tilewright_render_world::compile(&cell_view, limits.scene)
        .map_err(|e| failure(e.to_string()))?;
    let cell_slots = cell_slots(&scene, &world_view_objects, &cell_view, limits.scene);
    let mut diagnostics=vec![PreviewDiagnostic{code:"preview.effects-neutral",message:"Preview v1 uses neutral effects and one-tile footprints; profile fields are retained without inferred rendering behavior.".into()}];
    if inventory > 0 {
        diagnostics.push(PreviewDiagnostic {
            code: "preview.inventory-retained",
            message: format!(
                "{inventory} placed objects retain inventory; inventory is not a placed sprite."
            ),
        });
    }
    Ok(PreparedPreview {
        scene,
        cell_scene,
        cell_slots,
        resources: Arc::new(memory),
        picks,
        cell_picks,
        source_revision: world.source_digest(),
        catalog_revision: catalog.revision(),
        frame_revision: view.frame_revision,
        diagnostics,
    })
}

struct Adapter<'a> {
    world: &'a World,
    catalog: &'a PreviewCatalog,
    cache: &'a ReadyResources,
    view: &'a ViewSpec,
    limits: PreviewLimits,
}
impl Adapter<'_> {
    fn heights(&self) -> Result<HeightField, PreviewError> {
        let view = self.view;
        let profile = self.world.profile();
        let plane = &self.world.planes()[&view.plane];
        let base = i64::from(view.layer)
            .checked_mul(i64::from(profile.layer_height.unwrap_or(0)))
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| failure("stacked layer height outside renderer range"))?;
        // Include neighbour heights so the shared compiler computes boundary corners
        // from the same field as interior cells, rather than flattening view edges.
        let origin = [
            view.bounds[0]
                .checked_sub(1)
                .ok_or_else(|| failure("height origin overflow"))?,
            view.bounds[1]
                .checked_sub(1)
                .ok_or_else(|| failure("height origin overflow"))?,
        ];
        let width = u32::try_from(
            view.bounds[2]
                .checked_add(2)
                .ok_or_else(|| failure("height width overflow"))?,
        )
        .map_err(|_| failure("height width overflow"))?;
        let height = u32::try_from(
            view.bounds[3]
                .checked_add(2)
                .ok_or_else(|| failure("height height overflow"))?,
        )
        .map_err(|_| failure("height height overflow"))?;
        let height_count = usize::try_from(u64::from(width) * u64::from(height))
            .map_err(|_| failure("height budget overflow"))?;
        if height_count > tilewright_render_world::MAXIMUM_HEIGHT_FIELD_CELLS {
            return Err(failure("height budget exceeded"));
        }
        let mut heights = Vec::with_capacity(height_count);
        for row in 0..height {
            for column in 0..width {
                let x = origin[0]
                    .checked_add(i64::from(column))
                    .ok_or_else(|| failure("height coordinate overflow"))?;
                let y = origin[1]
                    .checked_add(i64::from(row))
                    .ok_or_else(|| failure("height coordinate overflow"))?;
                let index = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32))
                    .map_err(|_| failure("height index overflow"))?;
                let h = plane
                    .chunks
                    .get(&(view.layer, x.div_euclid(32), y.div_euclid(32)))
                    .and_then(|chunk| chunk.heights.get(index))
                    .copied()
                    .unwrap_or(0);
                heights.push(Some(
                    base.checked_add(h)
                        .ok_or_else(|| failure("stacked terrain height overflow"))?,
                ));
            }
        }
        Ok(HeightField {
            origin,
            width,
            height,
            heights,
        })
    }
    fn objects(&self, objects: &mut SceneObjects) -> Result<usize, PreviewError> {
        let Self {
            world,
            catalog,
            cache,
            view,
            limits,
        } = *self;
        let plane = &world.planes()[&view.plane];
        let mut scanned = 0usize;
        let mut inventory = 0usize;
        for (&(layer, cx, cy), chunk) in &plane.chunks {
            if layer != view.layer {
                continue;
            }
            let left = global(cx, 0)?;
            let top = global(cy, 0)?;
            // Validate the whole chunk's checked coordinate interval, including negatives.
            global(cx, 31)?;
            global(cy, 31)?;
            if left > view.bounds[0] + view.bounds[2] - 1
                || top > view.bounds[1] + view.bounds[3] - 1
                || left.checked_add(31).is_none_or(|v| v < view.bounds[0])
                || top.checked_add(31).is_none_or(|v| v < view.bounds[1])
            {
                continue;
            }
            for object in &chunk.objects {
                scanned = scanned
                    .checked_add(1)
                    .filter(|n| *n <= limits.maximum_objects)
                    .ok_or_else(|| failure("placed object budget exceeded"))?;
                let at = object
                    .at
                    .ok_or_else(|| failure("top-level object missing position"))?;
                let x = global(cx, at[0])?;
                let y = global(cy, at[1])?;
                if !inside(view.bounds, x, y) {
                    continue;
                }
                let uid = object.uid.clone().ok_or_else(|| {
                    failure(format!(
                        "UID-less placed object {} at {x},{y} is unsupported",
                        object.proto
                    ))
                })?;
                let layer = object.layer.as_deref().ok_or_else(|| {
                    failure(format!("object {uid} missing render-layer assignment"))
                })?;
                let (texture, anchor) = texture(catalog, cache, &object.proto, false)?;
                let mut rendered =
                    WorldObject::new(SemanticId(1), [x, y], rank(world, layer)?, anchor, texture);
                rendered.sub_layer = object.sub_layer;
                rendered.z = object.z;
                if !object.inventory.is_empty() {
                    inventory += 1;
                }
                insert(objects, SelectionKey::Object { uid }, rendered, limits)?;
            }
        }
        Ok(inventory)
    }
    fn terrain(&self, objects: &mut SceneObjects) -> Result<(), PreviewError> {
        let Self {
            world,
            catalog,
            cache,
            view,
            limits,
        } = *self;
        let profile = world.profile();
        let plane = world
            .planes()
            .get(&view.plane)
            .ok_or_else(|| failure("unknown plane"))?;
        // Resolve each bounded chunk once using the released transition resolver,
        // including absent chunks where neighbouring terrain can derive edge tiles.
        let side = i64::from(CHUNK_SIZE);
        let first = [
            view.bounds[0].div_euclid(side),
            view.bounds[1].div_euclid(side),
        ];
        let last = [
            (view.bounds[0] + view.bounds[2] - 1).div_euclid(side),
            (view.bounds[1] + view.bounds[3] - 1).div_euclid(side),
        ];
        for cy in first[1]..=last[1] {
            for cx in first[0]..=last[0] {
                global(cx, 0)?;
                global(cy, 0)?;
                global(cx, 31)?;
                global(cy, 31)?;
                let chunk = plane.chunks.get(&(view.layer, cx, cy));
                let resolved = profile
                    .transitions
                    .as_ref()
                    .map(|t| t.resolve_chunk(plane, view.layer, cx, cy));
                for (ground, grid) in profile.ground_layers.iter().enumerate() {
                    let layer_rank = rank(world, grid)?;
                    for local_y in 0..CHUNK_SIZE {
                        for local_x in 0..CHUNK_SIZE {
                            let x = global(cx, local_x)?;
                            let y = global(cy, local_y)?;
                            if !inside(view.bounds, x, y) {
                                continue;
                            }
                            let index = usize::from(local_y) * usize::from(CHUNK_SIZE)
                                + usize::from(local_x);
                            let proto = match &resolved {
                                Some(r) => r.value(ground, index),
                                None => chunk
                                    .and_then(|c| c.ground.get(ground))
                                    .and_then(|g| g.value(index)),
                            };
                            if let Some(proto) = proto {
                                let (texture, anchor) = texture(catalog, cache, proto, true)?;
                                let rendered = WorldObject::new(
                                    SemanticId(1),
                                    [x, y],
                                    layer_rank,
                                    anchor,
                                    texture,
                                );
                                insert(
                                    objects,
                                    SelectionKey::Cell {
                                        plane: view.plane.clone(),
                                        layer: view.layer,
                                        grid: grid.clone(),
                                        x,
                                        y,
                                    },
                                    rendered,
                                    limits,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn reference(&self) -> Result<(SceneObjects, MemoryResources), PreviewError> {
        let Self {
            world,
            cache,
            view,
            limits,
            ..
        } = *self;
        let profile = world.profile();
        let mut cells = BTreeMap::new();
        let mut memory = cache.provider.0.clone();
        if let Some(grid) = &view.grid {
            if !profile.ground_layers.contains(grid) {
                return Err(failure("unknown cell-picking grid"));
            }
            let (width, height) = match view.camera.projection {
                tilewright_render_world::Projection::Isometric {
                    tile_width,
                    tile_height,
                } => (tile_width, tile_height),
                tilewright_render_world::Projection::TopDown { tile_size } => {
                    (tile_size, tile_size)
                }
            };
            let payload = u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|n| n.checked_mul(4))
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| {
                    n.checked_add(12)
                        .is_some_and(|v| v <= limits.maximum_resource_bytes)
                })
                .ok_or_else(|| failure("reference resource budget exceeded"))?;
            let mut bytes = Vec::with_capacity(payload + 12);
            bytes.extend_from_slice(b"ATX1");
            bytes.extend_from_slice(&width.to_le_bytes());
            bytes.extend_from_slice(&height.to_le_bytes()); // Authored reference art is an opaque square or diamond. Projection,
            // stretching and raster coverage still belong entirely to the renderer.
            for row in 0..height {
                for column in 0..width {
                    let opaque = match view.camera.projection {
                        tilewright_render_world::Projection::TopDown { .. } => true,
                        tilewright_render_world::Projection::Isometric { .. } => {
                            let dx = (i64::from(column) * 2 + 1 - i64::from(width)).unsigned_abs();
                            let dy = (i64::from(row) * 2 + 1 - i64::from(height)).unsigned_abs();
                            dx * u64::from(height) + dy * u64::from(width)
                                <= u64::from(width) * u64::from(height)
                        }
                    };
                    bytes.extend_from_slice(&[255, 255, 255, if opaque { 255 } else { 0 }]);
                }
            }
            let reference = TextureRef {
                id: ResourceId(u64::MAX),
                revision: 1,
                digest: Digest256(hash(&bytes)),
                width,
                height,
            };
            memory.insert(reference.id.0, (reference, bytes));
            for y in view.bounds[1]..view.bounds[1] + view.bounds[3] {
                for x in view.bounds[0]..view.bounds[0] + view.bounds[2] {
                    insert(
                        &mut cells,
                        SelectionKey::Cell {
                            plane: view.plane.clone(),
                            layer: view.layer,
                            grid: grid.clone(),
                            x,
                            y,
                        },
                        WorldObject::new(
                            SemanticId(1),
                            [x, y],
                            rank(world, grid)?,
                            Anchor::Ground,
                            reference,
                        ),
                        limits,
                    )?;
                }
            }
        }
        Ok((cells, MemoryResources(memory)))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn chunk_arithmetic_refuses_overflow_and_handles_negative_boundary() {
        assert_eq!(super::global(-1, 31).unwrap(), -1);
        assert_eq!(super::global(-1, 0).unwrap(), -32);
        assert!(super::global(i64::MAX, 0).is_err());
        assert!(super::global(i64::MIN, 31).is_err());
    }
}
