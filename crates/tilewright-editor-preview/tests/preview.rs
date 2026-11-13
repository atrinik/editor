// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write, fs, path::Path};
use tilewright_editor_preview::{
    ObjectGrant, PreviewLimits, SelectionKey, loader, prepare, render_scene,
};
use tilewright_render_api::{BackendPreference, TargetDescriptor, TargetKind};
use tilewright_render_resources::ResourceRequest;
use tilewright_render_testkit::ReferenceRenderer;
use tilewright_scene::{Digest256, ResourceId};
use tilewright_source::{SourceRoot, World};

fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").unwrap();
    }
    output
}
fn atx(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut b = b"ATX1".to_vec();
    b.extend(width.to_le_bytes());
    b.extend(height.to_le_bytes());
    for _ in 0..width * height {
        b.extend(color);
    }
    b
}
fn fixture(iso: bool, negative: bool) -> (tempfile::TempDir, SourceRoot, World, Value) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    put(
        root,
        "world.jsonc",
        r#"{"format":"tilewright.world/1","profile":"profile/profile.jsonc","planes":["main"]}"#,
    );
    let projection = if iso {
        json!({"kind":"isometric","tile_width":32,"tile_height":16})
    } else {
        json!({"kind":"top_down","tile_size":32})
    };
    put(root,"profile/profile.jsonc",&json!({"format":"tilewright.profile/1","id":"preview.test","version":"1","projection":projection,"render_layers":["floor","item"],"ground_layers":["floor"],"definition_types":{"proto":{"schema":"schema/empty.jsonc"}},"object_fields":"schema/empty.jsonc","zone_properties":"schema/empty.jsonc","layer_height":8}).to_string());
    put(
        root,
        "profile/schema/empty.jsonc",
        r#"{"format":"tilewright.schema/1","fields":{}}"#,
    );
    for proto in ["grass", "marker"] {
        put(
            root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format":"tilewright.def/1","type":"proto","id":proto,"fields":{}}).to_string(),
        );
    }
    put(
        root,
        "planes/main/plane.jsonc",
        r#"{"format":"tilewright.plane/1","id":"main","kind":"shared","chunk_size":32}"#,
    );
    let (cx, x, bounds) = if negative {
        (-1, 31, [-1, 0, 2, 1])
    } else {
        (0, 0, [0, 0, 2, 1])
    };
    let chunk = json!({"format":"tilewright.chunk/1","plane":"main","layer":0,"chunk":[cx,0],"terrain":{"floor":{"palette":{"g":"grass"},"fill":"g"}},"objects":[{"uid":"marker_1","at":[x,0],"proto":"marker","layer":"item","sub_layer":2,"z":0}]});
    put(
        root,
        &format!("planes/main/layers/0/{cx}_0.jsonc"),
        &chunk.to_string(),
    );
    let mut resources = Vec::new();
    for (id, width, height, color) in [
        ("grass", 32, if iso { 16 } else { 32 }, [20, 180, 40, 255]),
        ("marker", 16, 32, [220, 30, 40, 255]),
    ] {
        let b = atx(width, height, color);
        let p = format!("preview-assets/{id}.atx1");
        fs::create_dir_all(root.join("preview-assets")).unwrap();
        fs::write(root.join(&p), &b).unwrap();
        resources.push(json!({"id":id,"path":p,"sha256":digest(&b),"width":width,"height":height,"license":"MIT","author":"Seraphina Studios","notice":"authored deterministic geometric fixture"}));
    }
    let catalog = json!({"format":"tilewright.editor-preview/1","profile":{"id":"preview.test","version":"1"},"view":{"plane":"main","layer":0,"bounds":bounds},"presentation":"schematic","resources":resources,"prototypes":[{"proto":"grass","resource":"grass","anchor":"ground"},{"proto":"marker","resource":"marker","anchor":"standing"}],"editing":{"movable_uids":["marker_1"],"terrain":[{"grid":"floor","values":["grass"]}],"fields":[]}});
    put(root, "preview.jsonc", &catalog.to_string());
    let source = SourceRoot::new(root);
    let world = tilewright_source::load(root).unwrap();
    (dir, source, world, catalog)
}

#[test]
fn reference_pixels_masks_and_empty_cell_selection_are_concrete() {
    let (_dir, root, world, _) = fixture(false, false);
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let view = catalog.default_view(&world, 128, 96, 7);
    let prepared = prepare(&world, &catalog, &cache, &view, limits).unwrap();
    let mut renderer = ReferenceRenderer::new(TargetDescriptor {
        kind: TargetKind::Offscreen,
        width: 128,
        height: 96,
        backend: BackendPreference::Automatic,
    })
    .unwrap();
    let frame = render_scene(&mut renderer, &prepared.scene, prepared.resources.clone()).unwrap();
    // Fixed pixels are independent expected art/position facts, not geometry
    // recomputed by the bridge. The standing marker covers the first tile.
    assert_eq!(
        &frame.rgba8[(32 * 128 + 40) * 4..(32 * 128 + 40) * 4 + 4],
        &[220, 30, 40, 255]
    );
    assert_eq!(
        prepared.pick(7, 40, 32, false).unwrap(),
        Some(SelectionKey::Object {
            uid: "marker_1".into()
        })
    );
    assert_eq!(
        prepared.pick(7, 80, 48, false).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 1,
            y: 0
        })
    );
    assert_eq!(
        prepared.pick(7, 80, 48, true).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 1,
            y: 0
        })
    );
    assert!(prepared.pick(6, 40, 32, false).is_err());
    assert!(prepared.pick(7, 128, 0, false).is_err());
}

#[test]
fn empty_terrain_has_a_reference_hit_with_no_main_hit() {
    let (dir, root, _, _) = fixture(false, false);
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let mut chunk: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    chunk["terrain"] = json!({});
    fs::write(path, chunk.to_string()).unwrap();
    let world = tilewright_source::load(dir.path()).unwrap();
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let p = prepare(
        &world,
        &catalog,
        &cache,
        &catalog.default_view(&world, 128, 96, 1),
        limits,
    )
    .unwrap();
    assert_eq!(p.pick(1, 80, 48, false).unwrap(), None);
    assert_eq!(
        p.pick(1, 80, 48, true).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 1,
            y: 0
        })
    );
}

#[test]
fn catalog_and_resource_edits_invalidate_without_world_revision_change() {
    let (dir, root, world, mut text) = fixture(false, false);
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let original = world.source_digest();
    text["editing"]["movable_uids"] = json!([]);
    put(dir.path(), "preview.jsonc", &text.to_string());
    assert_eq!(
        tilewright_source::load(dir.path()).unwrap().source_digest(),
        original
    );
    assert!(loader::revalidate(&root, &catalog, &cache, limits).is_err());
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    fs::write(
        dir.path().join("preview-assets/marker.atx1"),
        atx(16, 32, [1, 2, 3, 255]),
    )
    .unwrap();
    assert!(loader::revalidate(&root, &catalog, &cache, limits).is_err());
    assert!(loader::load(&root, &world, limits).is_err());
}

#[test]
fn catalog_rejects_unknown_duplicate_capabilities_paths_and_bad_resources() {
    let (dir, root, world, original) = fixture(false, false);
    let limits = PreviewLimits::default();
    for mutation in 0..9 {
        let mut c = original.clone();
        match mutation {
            0 => c["extra"] = json!(true),
            1 => c["resources"][0]["path"] = json!("preview-assets/../grass.atx1"),
            2 => c["resources"][0]["license"] = json!("GPL-2.0"),
            3 => c["resources"][0]["width"] = json!(1),
            4 => c["resources"][0]["sha256"] = json!("A".repeat(64)),
            5 => c["prototypes"][1] = c["prototypes"][0].clone(),
            6 => c["editing"]["movable_uids"] = json!(["marker_1", "marker_1"]),
            7 => c["view"]["bounds"] = json!([0, 0, 9_007_199_254_740_991_i64, 2]),
            _ => c["resources"][0]["author"] = json!(""),
        }
        put(dir.path(), "preview.jsonc", &c.to_string());
        assert!(
            loader::load(&root, &world, limits).is_err(),
            "case {mutation}"
        );
    }
    put(
        dir.path(),
        "preview.jsonc",
        &original
            .to_string()
            .replacen("\"format\":", "\"format\":\"duplicate\",\"format\":", 1),
    );
    assert!(loader::load(&root, &world, limits).is_err());
    put(dir.path(), "preview.jsonc", &original.to_string());
    let tight = PreviewLimits {
        maximum_catalog_bytes: 16,
        ..limits
    };
    assert!(loader::load(&root, &world, tight).is_err());
    fs::write(dir.path().join("preview-assets/grass.atx1"), b"ATX1").unwrap();
    assert!(loader::load(&root, &world, limits).is_err());
}

#[test]
fn identity_is_deterministic_across_negative_chunk_boundary_and_projection() {
    for iso in [false, true] {
        let (_dir, root, world, _) = fixture(iso, true);
        let limits = PreviewLimits::default();
        let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
        let view = catalog.default_view(&world, 128, 96, 3);
        let a = prepare(&world, &catalog, &cache, &view, limits).unwrap();
        let b = prepare(&world, &catalog, &cache, &view, limits).unwrap();
        assert_eq!(a.scene, b.scene);
        assert_eq!(a.picks, b.picks);
        assert!(
            a.picks
                .values()
                .any(|key| matches!(key,SelectionKey::Object{uid} if uid=="marker_1"))
        );
        assert!(
            a.cell_picks
                .values()
                .any(|key| matches!(key, SelectionKey::Cell { x: -1, .. }))
        );
        assert!(
            a.cell_picks
                .values()
                .any(|key| matches!(key, SelectionKey::Cell { x: 0, .. }))
        );
        assert!(a.picks.keys().all(|id| id.0 > 0));
        let mut invalid = view.clone();
        invalid.frame_revision = 0;
        assert!(prepare(&world, &catalog, &cache, &invalid, limits).is_err());
        invalid.frame_revision = 3;
        invalid.bounds = [i64::MAX, 0, 2, 1];
        assert!(prepare(&world, &catalog, &cache, &invalid, limits).is_err());
        let request = ResourceRequest {
            id: ResourceId(1),
            revision: 2,
            digest: Digest256([1; 32]),
            maximum_bytes: 4096,
        };
        assert!(a.resources.load(request).is_err());
    }
}

#[cfg(unix)]
#[test]
fn retained_root_refuses_resource_symlink() {
    let (dir, root, world, _) = fixture(false, false);
    let p = dir.path().join("preview-assets/grass.atx1");
    fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink("marker.atx1", p).unwrap();
    assert!(loader::load(&root, &world, PreviewLimits::default()).is_err());
}

#[test]
fn spatial_height_render_rank_sub_layer_and_z_reach_the_shared_compiler() {
    let (dir, root, _, mut catalog_text) = fixture(false, false);
    let old = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let mut chunk: Value = serde_json::from_slice(&fs::read(&old).unwrap()).unwrap();
    chunk["layer"] = json!(1);
    chunk["objects"][0]["z"] = json!(4);
    let mut behind = chunk["objects"][0].clone();
    behind["uid"] = json!("marker_2");
    behind["sub_layer"] = json!(-2);
    behind["z"] = json!(0);
    chunk["objects"].as_array_mut().unwrap().push(behind);
    fs::remove_file(old).unwrap();
    put(
        dir.path(),
        "planes/main/layers/1/0_0.jsonc",
        &chunk.to_string(),
    );
    catalog_text["view"]["layer"] = json!(1);
    put(dir.path(), "preview.jsonc", &catalog_text.to_string());
    let world = tilewright_source::load(dir.path()).unwrap();
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let p = prepare(
        &world,
        &catalog,
        &cache,
        &catalog.default_view(&world, 128, 96, 4),
        limits,
    )
    .unwrap();
    assert_eq!(
        p.pick(4, 40, 20, false).unwrap(),
        Some(SelectionKey::Object {
            uid: "marker_1".into()
        })
    );
    // Layer 1 sits eight pixels above layer 0 and z adds four pixels for
    // this standing object. The reference ground mask uses the same height.
    assert_eq!(
        p.pick(4, 80, 25, true).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 1,
            grid: "floor".into(),
            x: 1,
            y: 0
        })
    );
    assert_eq!(p.pick(4, 80, 60, true).unwrap(), None);
    let object_id = p
        .picks
        .iter()
        .find(|(_, key)| {
            **key
                == SelectionKey::Object {
                    uid: "marker_1".into(),
                }
        })
        .unwrap()
        .0;
    let object_quad = p
        .scene
        .quads()
        .iter()
        .find(|quad| quad.semantic_id == *object_id)
        .unwrap();
    assert_eq!(object_quad.positions[0][1].to_bits(), 20.0_f32.to_bits());
}

#[test]
fn unsupported_visible_objects_are_explicit_errors() {
    let (dir, root, _, catalog) = fixture(false, false);
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for missing in ["uid", "layer"] {
        let mut chunk = original.clone();
        chunk["objects"][0].as_object_mut().unwrap().remove(missing);
        fs::write(&path, chunk.to_string()).unwrap();
        let world = tilewright_source::load(dir.path()).unwrap();
        let mut c = catalog.clone();
        c["editing"]["movable_uids"] = json!([]);
        put(dir.path(), "preview.jsonc", &c.to_string());
        let limits = PreviewLimits::default();
        let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
        let error = prepare(
            &world,
            &catalog,
            &cache,
            &catalog.default_view(&world, 128, 96, 1),
            limits,
        )
        .err()
        .unwrap();
        assert!(error.0.contains(if missing == "uid" {
            "UID-less"
        } else {
            "missing render-layer"
        }));
    }
}

#[test]
fn isometric_reference_diamonds_pick_centers_and_leave_corners_empty() {
    let (_dir, root, world, _) = fixture(true, true);
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let p = prepare(
        &world,
        &catalog,
        &cache,
        &catalog.default_view(&world, 128, 96, 5),
        limits,
    )
    .unwrap();
    assert_eq!(
        p.pick(5, 52, 44, true).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: -1,
            y: 0
        })
    );
    assert_eq!(
        p.pick(5, 72, 52, true).unwrap(),
        Some(SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 0,
            y: 0
        })
    );
    assert_eq!(p.pick(5, 40, 36, true).unwrap(), None);
    assert_eq!(p.pick(5, 87, 44, true).unwrap(), None);
}

#[test]
fn coincident_object_source_order_survives_sorted_semantic_ids() {
    let (dir, root, _, mut catalog) = fixture(false, false);
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let mut chunk: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    chunk["objects"][0]["uid"] = json!("z_first");
    chunk["objects"][0]["layer"] = json!("floor");
    let mut last = chunk["objects"][0].clone();
    last["uid"] = json!("a_last");
    chunk["objects"].as_array_mut().unwrap().push(last);
    fs::write(path, chunk.to_string()).unwrap();
    catalog["editing"]["movable_uids"] = json!([]);
    put(dir.path(), "preview.jsonc", &catalog.to_string());
    let world = tilewright_source::load(dir.path()).unwrap();
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let p = prepare(
        &world,
        &catalog,
        &cache,
        &catalog.default_view(&world, 128, 96, 1),
        limits,
    )
    .unwrap();
    // Alphabetical identity allocation gives a_last the smaller ID, while
    // source ordering still lets the later coincident object cover z_first.
    assert_eq!(
        p.picks[&tilewright_scene::SemanticId(1)],
        SelectionKey::Object {
            uid: "a_last".into()
        }
    );
    assert_eq!(
        p.pick(1, 40, 32, false).unwrap(),
        Some(SelectionKey::Object {
            uid: "a_last".into()
        })
    );
}

#[test]
fn a_retained_gesture_mask_matches_every_single_pick_and_only_its_frame() {
    let (_dir, root, world, _) = fixture(true, true);
    let limits = PreviewLimits::default();
    let (catalog, cache) = loader::load(&root, &world, limits).unwrap();
    let p = prepare(
        &world,
        &catalog,
        &cache,
        &catalog.default_view(&world, 128, 96, 5),
        limits,
    )
    .unwrap();
    for terrain in [false, true] {
        let mask = p.mask(5, terrain).unwrap();
        assert_eq!(mask.generation(), 5);
        assert_eq!(mask.size(), [128, 96]);
        for (x, y) in [(52, 44), (72, 52), (40, 36), (87, 44), (0, 0), (127, 95)] {
            assert_eq!(
                mask.pick(x, y).unwrap(),
                p.pick(5, x, y, terrain).unwrap(),
                "{x},{y}"
            );
        }
        assert!(mask.pick(128, 0).is_err());
        assert!(mask.pick(0, 96).is_err());
    }
    assert!(p.mask(4, true).is_err());
    assert!(p.mask(0, true).is_err());
}

#[test]
fn optional_object_grants_parse_strictly_and_their_absence_changes_nothing() {
    let (dir, root, _, original) = fixture(false, false);
    put(
        dir.path(),
        "profile/schema/object.jsonc",
        r#"{"format":"tilewright.schema/1","fields":{"label":{"type":"string","max_length":8},"note":{"type":"string","max_length":8}}}"#,
    );
    let profile_path = dir.path().join("profile/profile.jsonc");
    let mut profile: Value = serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    profile["object_fields"] = json!("schema/object.jsonc");
    put(dir.path(), "profile/profile.jsonc", &profile.to_string());
    let world = tilewright_source::load(dir.path()).unwrap();
    let limits = PreviewLimits::default();

    // An existing producer's catalog, without the key, grants no deletion.
    let (absent, _) = loader::load(&root, &world, limits).unwrap();
    let editing = absent.editing();
    assert!(editing.objects.is_empty());
    assert!(!absent.can_delete(&world, "marker_1"));
    assert!(absent.can_move(&world, "marker_1"));
    assert!(!absent.can_move(&world, "unknown_uid"));
    assert!(!absent.can_edit_field(&world, "marker_1", "label"));
    assert!(absent.editable_fields(&world, "marker_1").is_empty());

    let mut granted = original.clone();
    granted["editing"]["objects"] = json!([{"proto": "marker", "layer": "item", "fields": ["label"]}, {"proto": "grass", "layer": "floor"}]);
    put(dir.path(), "preview.jsonc", &granted.to_string());
    let (catalog, _) = loader::load(&root, &world, limits).unwrap();
    let editing = catalog.editing();
    assert_eq!(editing.movable_uids, absent.editing().movable_uids);
    assert_eq!(editing.terrain, absent.editing().terrain);
    assert_eq!(editing.fields, absent.editing().fields);
    assert_eq!(
        editing.objects["marker"],
        ObjectGrant {
            layer: "item".into(),
            fields: ["label".to_owned()].into(),
            defaults: Vec::new(),
        }
    );
    assert!(editing.objects["grass"].fields.is_empty());
    assert!(catalog.in_active_view(&world, "marker_1"));
    assert!(catalog.can_delete(&world, "marker_1"));
    assert!(catalog.can_edit_field(&world, "marker_1", "label"));
    assert!(!catalog.can_edit_field(&world, "marker_1", "note"));
    assert_eq!(catalog.editable_fields(&world, "marker_1"), ["label"]);
    assert!(!catalog.can_delete(&world, "unknown_uid"));
    granted["editing"]["objects"] = json!([]);
    put(dir.path(), "preview.jsonc", &granted.to_string());
    assert!(
        loader::load(&root, &world, limits)
            .unwrap()
            .0
            .editing()
            .objects
            .is_empty()
    );

    for (case, objects) in [
        json!({}),
        json!([{"proto": "marker"}]),
        json!([{"layer": "item"}]),
        json!([{"proto": "marker", "layer": "item", "extra": true}]),
        json!([{"proto": "unmapped", "layer": "item"}]),
        json!([{"proto": "", "layer": "item"}]),
        json!([{"proto": "marker", "layer": "sky"}]),
        json!([{"proto": "marker", "layer": 1}]),
        json!([{"proto": "marker", "layer": "item", "fields": "label"}]),
        json!([{"proto": "marker", "layer": "item", "fields": ["unknown"]}]),
        json!([{"proto": "marker", "layer": "item", "fields": ["label", "label"]}]),
        json!([{"proto": "marker", "layer": "item", "fields": [1]}]),
        json!([{"proto": "marker", "layer": "item"}, {"proto": "marker", "layer": "floor"}]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut c = original.clone();
        c["editing"]["objects"] = objects;
        put(dir.path(), "preview.jsonc", &c.to_string());
        assert!(loader::load(&root, &world, limits).is_err(), "case {case}");
    }
    // Other unknown editing keys stay refused.
    let mut c = original.clone();
    c["editing"]["placement"] = json!([]);
    put(dir.path(), "preview.jsonc", &c.to_string());
    assert!(loader::load(&root, &world, limits).is_err());
}

/// Give the preview fixture's profile an object field schema: an optional
/// short `label`, an optional `count` from 0 to 9, a required `kind` of a
/// fixed set, and a structured `settings` object.
fn with_object_fields(dir: &Path, required_kind: bool) -> World {
    put(
        dir,
        "profile/schema/object.jsonc",
        &json!({"format": "tilewright.schema/1", "fields": {
            "label": {"type": "string", "max_length": 8},
            "count": {"type": "integer", "min": 0, "max": 9},
            "kind": {"type": "string", "values": ["plain", "fancy"], "required": required_kind},
            "settings": {"type": "object", "fields": {"active": {"type": "boolean", "required": true}}},
            "note": {"type": "json"}
        }})
        .to_string(),
    );
    let profile_path = dir.join("profile/profile.jsonc");
    let mut profile: Value = serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    profile["object_fields"] = json!("schema/object.jsonc");
    put(dir, "profile/profile.jsonc", &profile.to_string());
    tilewright_source::load(dir).unwrap()
}

#[test]
fn object_defaults_parse_strictly_and_their_absence_changes_nothing() {
    let (dir, root, _, original) = fixture(false, false);
    let world = with_object_fields(dir.path(), false);
    let limits = PreviewLimits::default();
    let load = |objects: Value| {
        let mut catalog = original.clone();
        catalog["editing"]["objects"] = objects;
        put(dir.path(), "preview.jsonc", &catalog.to_string());
        loader::load(&root, &world, limits)
    };

    // An entry written for deletion alone loads unchanged, with no defaults.
    let (catalog, _) =
        load(json!([{"proto": "marker", "layer": "item", "fields": ["label"]}])).unwrap();
    assert!(catalog.editing().objects["marker"].defaults.is_empty());
    let (catalog, _) = load(json!([{"proto": "marker", "layer": "item", "defaults": {}}])).unwrap();
    assert!(catalog.editing().objects["marker"].defaults.is_empty());

    // Defaults keep catalog order (written out of sorted order here) and
    // canonical JSON; `fields` keeps its meaning as editable field names.
    let mut text = original.clone();
    text["editing"]["objects"] =
        json!([{"proto": "marker", "layer": "item", "fields": ["label"], "defaults": "DEFAULTS"}]);
    let text = text.to_string().replace(
        r#""DEFAULTS""#,
        r#"{"settings": {"active": true}, "label": "New", "kind": "plain", "count": 3}"#,
    );
    put(dir.path(), "preview.jsonc", &text);
    let (catalog, _) = loader::load(&root, &world, limits).unwrap();
    let grant = &catalog.editing().objects["marker"];
    assert_eq!(grant.fields, ["label".to_owned()].into());
    assert_eq!(
        grant.defaults,
        [
            ("settings".to_owned(), r#"{"active":true}"#.to_owned()),
            ("label".to_owned(), r#""New""#.to_owned()),
            ("kind".to_owned(), r#""plain""#.to_owned()),
            ("count".to_owned(), "3".to_owned()),
        ]
    );

    for (case, defaults) in [
        json!([]),
        json!("label"),
        json!({"unknown": 1}),
        json!({"label": 1}),
        json!({"label": "far too long"}),
        json!({"count": 10}),
        json!({"count": -1}),
        json!({"count": 1.5}),
        json!({"kind": "other"}),
        json!({"settings": "on"}),
        // Object member names are refused only as unknown fields: this
        // profile declares none of them.
        json!({"at": [0, 0]}),
        json!({"uid": "o.fixed"}),
        json!({"proto": "grass"}),
        json!({"layer": "floor"}),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            load(json!([{"proto": "marker", "layer": "item", "defaults": defaults}])).is_err(),
            "case {case}"
        );
    }
    // A duplicate member is refused by the strict JSONC reader itself.
    let mut text = original.clone();
    text["editing"]["objects"] =
        json!([{"proto": "marker", "layer": "item", "defaults": {"label": "a"}}]);
    let text = text
        .to_string()
        .replace(r#"{"label":"a"}"#, r#"{"label":"a","label":"b"}"#);
    put(dir.path(), "preview.jsonc", &text);
    assert!(loader::load(&root, &world, limits).is_err());
}

/// The same prototype grants, one tile at a time around the view edges and
/// in another plane at in-bounds x/y.
#[test]
fn prototype_grants_stop_at_the_view_plane_and_its_upper_bounds() {
    let (dir, root, _, mut catalog) = fixture(false, false);
    put(
        dir.path(),
        "world.jsonc",
        r#"{"format":"tilewright.world/1","profile":"profile/profile.jsonc","planes":["main","other"]}"#,
    );
    put(
        dir.path(),
        "planes/other/plane.jsonc",
        r#"{"format":"tilewright.plane/1","id":"other","kind":"shared","chunk_size":32}"#,
    );
    let object = |uid: &str, x: i64, y: i64| json!({"uid": uid, "at": [x, y], "proto": "marker", "layer": "item"});
    put(
        dir.path(),
        "planes/other/layers/0/0_0.jsonc",
        &json!({"format": "tilewright.chunk/1", "plane": "other", "layer": 0, "chunk": [0, 0],
            "objects": [object("elsewhere", 1, 1)]})
        .to_string(),
    );
    put(
        dir.path(),
        "planes/main/layers/0/0_0.jsonc",
        &json!({"format": "tilewright.chunk/1", "plane": "main", "layer": 0, "chunk": [0, 0],
            "objects": [object("marker_1", 0, 0), object("last_x", 2, 1), object("last_y", 1, 2),
                object("past_x", 3, 1), object("past_y", 1, 3)]})
        .to_string(),
    );
    let world = with_object_fields(dir.path(), false);
    // View [0, 0] to [2, 2] inclusive: width and height 3.
    catalog["view"]["bounds"] = json!([0, 0, 3, 3]);
    catalog["editing"]["objects"] =
        json!([{"proto": "marker", "layer": "item", "fields": ["label"]}]);
    put(dir.path(), "preview.jsonc", &catalog.to_string());
    let (catalog, _) = loader::load(&root, &world, PreviewLimits::default()).unwrap();

    for object in ["marker_1", "last_x", "last_y"] {
        assert!(catalog.in_active_view(&world, object), "{object}");
        assert!(catalog.can_delete(&world, object), "{object}");
        assert!(catalog.can_edit_field(&world, object, "label"), "{object}");
    }
    // Another plane at in-bounds x/y, and one tile past the upper x and y
    // bounds: no delete, no field edit, no move.
    for object in ["elsewhere", "past_x", "past_y"] {
        assert!(!catalog.in_active_view(&world, object), "{object}");
        assert!(!catalog.can_delete(&world, object), "{object}");
        assert!(!catalog.can_edit_field(&world, object, "label"), "{object}");
        assert!(!catalog.can_move(&world, object), "{object}");
    }
    // Placement targets use the same scope.
    assert!(catalog.tile_in_active_view("main", 0, 2, 2));
    assert!(catalog.tile_in_active_view("main", 0, 0, 0));
    for (plane, layer, x, y) in [
        ("other", 0, 1, 1),
        ("main", 1, 1, 1),
        ("main", 0, 3, 2),
        ("main", 0, 2, 3),
        ("main", 0, -1, 0),
        ("main", 0, 0, -1),
    ] {
        assert!(
            !catalog.tile_in_active_view(plane, layer, x, y),
            "{plane} {layer} {x} {y}"
        );
    }
}

#[test]
fn object_defaults_values_share_one_64_kib_budget_per_entry() {
    let (dir, root, _, original) = fixture(false, false);
    let world = with_object_fields(dir.path(), false);
    let limits = PreviewLimits::default();
    // The budget counts the canonical bytes of the values, not the names.
    let load = |note: usize, label: &str| {
        let mut catalog = original.clone();
        catalog["editing"]["objects"] = json!([{"proto": "marker", "layer": "item",
            "defaults": {"label": label, "note": "n".repeat(note)}}]);
        put(dir.path(), "preview.jsonc", &catalog.to_string());
        loader::load(&root, &world, limits)
    };
    // `"ab"` is 4 bytes; the note string is its length plus 2 quotes.
    let fits = 65536 - 4 - 2;
    let (catalog, _) = load(fits, "ab").unwrap();
    let total: usize = catalog.editing().objects["marker"]
        .defaults
        .iter()
        .map(|(_, value)| value.len())
        .sum();
    assert_eq!(total, 65536);
    assert!(load(fits + 1, "ab").is_err());
    assert!(load(fits, "abc").is_err());
}
