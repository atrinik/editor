// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use crate::{PreviewError, PreviewLimits, ViewSpec, failure};
use std::collections::{BTreeMap, BTreeSet};
use tilewright_jsonc::{Document, Value};
use tilewright_render_world::{Anchor, Camera, Projection};
use tilewright_source::World;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditingCapabilities {
    pub movable_uids: BTreeSet<String>,
    pub terrain: BTreeMap<String, BTreeSet<String>>,
    pub fields: BTreeSet<(String, String)>,
    /// Optional `editing.objects`: one grant per prototype. Each lets the
    /// author delete, move and edit the listed fields of every object of
    /// that prototype; `layer` is the render layer a placed object uses.
    pub objects: BTreeMap<String, ObjectGrant>,
}

/// One `editing.objects` entry, keyed by its prototype.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectGrant {
    pub layer: String,
    pub fields: BTreeSet<String>,
}

impl EditingCapabilities {
    /// Movement is granted per uid (`movable_uids`) or per prototype.
    pub fn can_move(&self, uid: &str, proto: &str) -> bool {
        self.movable_uids.contains(uid) || self.objects.contains_key(proto)
    }
    /// A field edit is granted per uid (`fields`) or per prototype.
    pub fn can_edit_field(&self, uid: &str, proto: &str, field: &str) -> bool {
        self.fields
            .iter()
            .any(|(object, name)| object == uid && name == field)
            || self
                .objects
                .get(proto)
                .is_some_and(|grant| grant.fields.contains(field))
    }
    /// Deletion is granted only per prototype.
    pub fn can_delete(&self, proto: &str) -> bool {
        self.objects.contains_key(proto)
    }
    /// Every field granted for this object, per uid then per prototype,
    /// without duplicates.
    pub fn editable_fields<'a>(&'a self, uid: &str, proto: &str) -> Vec<&'a str> {
        let mut names: Vec<&str> = self
            .fields
            .iter()
            .filter(|(object, _)| object == uid)
            .map(|(_, field)| field.as_str())
            .collect();
        if let Some(grant) = self.objects.get(proto) {
            for field in &grant.fields {
                if !names.contains(&field.as_str()) {
                    names.push(field);
                }
            }
        }
        names
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResourceSpec {
    pub path: String,
    pub digest: [u8; 32],
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct PreviewCatalog {
    pub(crate) revision: [u8; 32],
    pub(crate) profile: (String, String),
    pub(crate) plane: String,
    pub(crate) layer: i32,
    pub(crate) bounds: [i64; 4],
    pub(crate) presentation: String,
    pub(crate) resources: BTreeMap<String, ResourceSpec>,
    pub(crate) prototypes: BTreeMap<String, (String, Anchor)>,
    pub(crate) editing: EditingCapabilities,
}

impl PreviewCatalog {
    pub const fn revision(&self) -> [u8; 32] {
        self.revision
    }
    pub fn profile(&self) -> (&str, &str) {
        (&self.profile.0, &self.profile.1)
    }
    pub fn view(&self) -> (&str, i32, [i64; 4]) {
        (&self.plane, self.layer, self.bounds)
    }
    pub fn presentation(&self) -> &str {
        &self.presentation
    }
    pub const fn editing(&self) -> &EditingCapabilities {
        &self.editing
    }
    #[allow(clippy::cast_precision_loss)]
    pub fn default_view(
        &self,
        world: &World,
        width: u32,
        height: u32,
        generation: u64,
    ) -> ViewSpec {
        ViewSpec {
            plane: self.plane.clone(),
            layer: self.layer,
            bounds: self.bounds,
            camera: Camera {
                projection: projection(world),
                center: [
                    self.bounds[0] as f64 + self.bounds[2] as f64 / 2.0,
                    self.bounds[1] as f64 + self.bounds[3] as f64 / 2.0,
                ],
                zoom: 1.0,
                width,
                height,
                sampling: tilewright_scene::Sampling::Nearest,
            },
            frame_revision: generation,
            grid: self
                .editing
                .terrain
                .keys()
                .next()
                .cloned()
                .or_else(|| world.profile().ground_layers.first().cloned()),
        }
    }
}

pub(crate) fn projection(world: &World) -> Projection {
    match world.profile().projection {
        tilewright_source::Projection::Isometric {
            tile_width,
            tile_height,
        } => Projection::Isometric {
            tile_width,
            tile_height,
        },
        tilewright_source::Projection::TopDown { tile_size } => Projection::TopDown { tile_size },
    }
}

fn closed(value: &Value, keys: &[&str]) -> Result<(), PreviewError> {
    let members = value
        .as_object()
        .ok_or_else(|| failure("expected object"))?;
    if members.len() != keys.len()
        || members
            .iter()
            .any(|member| !keys.contains(&member.key.as_str()))
    {
        return Err(failure("unknown or missing catalog key"));
    }
    Ok(())
}
/// Like [`closed`], but the `optional` keys may also appear.
fn closed_optional(value: &Value, keys: &[&str], optional: &[&str]) -> Result<(), PreviewError> {
    let members = value
        .as_object()
        .ok_or_else(|| failure("expected object"))?;
    let present = members
        .iter()
        .filter(|member| keys.contains(&member.key.as_str()))
        .count();
    if present != keys.len()
        || members.iter().any(|member| {
            !keys.contains(&member.key.as_str()) && !optional.contains(&member.key.as_str())
        })
    {
        return Err(failure("unknown or missing catalog key"));
    }
    Ok(())
}
fn member<'a>(value: &'a Value, key: &str) -> Result<&'a Value, PreviewError> {
    value
        .get(key)
        .ok_or_else(|| failure(format!("missing {key}")))
}
fn string(value: &Value) -> Result<String, PreviewError> {
    value
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 1024 && !v.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| failure("expected bounded nonempty string"))
}
fn identifier(value: &Value) -> Result<String, PreviewError> {
    let s = string(value)?;
    if !tilewright_source::is_identifier(&s) {
        return Err(failure("invalid identifier"));
    }
    Ok(s)
}
fn array(value: &Value) -> Result<&[Value], PreviewError> {
    value.as_array().ok_or_else(|| failure("expected array"))
}
fn integer(value: &Value) -> Result<i64, PreviewError> {
    value.as_i64().ok_or_else(|| failure("expected integer"))
}
fn dimension(value: &Value, limits: PreviewLimits) -> Result<u32, PreviewError> {
    u32::try_from(integer(value)?)
        .ok()
        .filter(|v| *v > 0 && *v <= limits.scene.maximum_dimension)
        .ok_or_else(|| failure("invalid resource dimensions"))
}
// The v1 catalog deliberately requires this exact lowercase portable suffix.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
pub(crate) fn portable_path(s: &str) -> bool {
    s.starts_with("preview-assets/")
        && s.ends_with(".atx1")
        && s.len() <= 512
        && !s.contains(['\\', ':', '\0'])
        && s.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(['.', ' '])
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
fn digest(value: &Value) -> Result<[u8; 32], PreviewError> {
    let s = string(value)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(failure("expected lowercase SHA-256"));
    }
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| failure("invalid digest"))?;
    }
    if out == [0; 32] {
        return Err(failure("zero resource digest"));
    }
    Ok(out)
}

pub(crate) fn decode(
    text: String,
    world: &World,
    limits: PreviewLimits,
) -> Result<PreviewCatalog, PreviewError> {
    let document = Document::parse(
        text,
        &tilewright_jsonc::Limits {
            max_input_bytes: limits.maximum_catalog_bytes,
            max_values: limits.maximum_catalog_values,
            max_array_items: limits.maximum_entries,
            ..tilewright_jsonc::Limits::default()
        },
    )
    .map_err(|e| failure(e.to_string()))?;
    let root = document.root();
    closed(
        root,
        &[
            "format",
            "profile",
            "view",
            "presentation",
            "resources",
            "prototypes",
            "editing",
        ],
    )?;
    if member(root, "format")?.as_str() != Some("tilewright.editor-preview/1") {
        return Err(failure("unsupported preview format"));
    }
    let p = member(root, "profile")?;
    closed(p, &["id", "version"])?;
    let profile = (
        identifier(member(p, "id")?)?,
        string(member(p, "version")?)?,
    );
    if profile.0 != world.profile().id || profile.1 != world.profile().version {
        return Err(failure("catalog profile mismatch"));
    }
    let v = member(root, "view")?;
    closed(v, &["plane", "layer", "bounds"])?;
    let plane = identifier(member(v, "plane")?)?;
    if !world.planes().contains_key(&plane) {
        return Err(failure("unknown view plane"));
    }
    let layer =
        i32::try_from(integer(member(v, "layer")?)?).map_err(|_| failure("layer overflow"))?;
    let values = array(member(v, "bounds")?)?;
    let bounds: [i64; 4] = values
        .iter()
        .map(integer)
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| failure("bounds needs four integers"))?;
    crate::validate_bounds(bounds, limits)?;
    let presentation = string(member(root, "presentation")?)?;
    if !["schematic", "authored"].contains(&presentation.as_str()) {
        return Err(failure("unknown presentation"));
    }
    let resources = resource_specs(member(root, "resources")?, limits)?;
    let prototypes = prototype_specs(member(root, "prototypes")?, world, &resources)?;
    let editing = editing_specs(member(root, "editing")?, world, &prototypes)?;
    Ok(PreviewCatalog {
        revision: crate::hash(document.source().as_bytes()),
        profile,
        plane,
        layer,
        bounds,
        presentation,
        resources,
        prototypes,
        editing,
    })
}

fn resource_specs(
    value: &Value,
    limits: PreviewLimits,
) -> Result<BTreeMap<String, ResourceSpec>, PreviewError> {
    let mut resources = BTreeMap::new();
    let mut paths = BTreeSet::new();
    for r in array(value)? {
        closed(
            r,
            &[
                "id", "path", "sha256", "width", "height", "license", "author", "notice",
            ],
        )?;
        let id = identifier(member(r, "id")?)?;
        let path = string(member(r, "path")?)?;
        if !portable_path(&path) || !paths.insert(path.to_ascii_lowercase()) {
            return Err(failure("unsafe or duplicate resource path"));
        }
        if member(r, "license")?.as_str() != Some("MIT") {
            return Err(failure("resource license must be MIT"));
        }
        string(member(r, "author")?)?;
        string(member(r, "notice")?)?;
        let spec = ResourceSpec {
            path,
            digest: digest(member(r, "sha256")?)?,
            width: dimension(member(r, "width")?, limits)?,
            height: dimension(member(r, "height")?, limits)?,
        };
        if resources.insert(id, spec).is_some() {
            return Err(failure("duplicate resource ID"));
        }
    }
    Ok(resources)
}

fn prototype_specs(
    value: &Value,
    world: &World,
    resources: &BTreeMap<String, ResourceSpec>,
) -> Result<BTreeMap<String, (String, Anchor)>, PreviewError> {
    let mut prototypes = BTreeMap::new();
    for p in array(value)? {
        closed(p, &["proto", "resource", "anchor"])?;
        let proto = identifier(member(p, "proto")?)?;
        let resource = identifier(member(p, "resource")?)?;
        let anchor = match member(p, "anchor")?.as_str() {
            Some("ground") => Anchor::Ground,
            Some("standing") => Anchor::Standing,
            _ => return Err(failure("unsupported anchor")),
        };
        if !world
            .definitions()
            .contains_key(&("proto".into(), proto.clone()))
            || !resources.contains_key(&resource)
        {
            return Err(failure("unresolved prototype/resource"));
        }
        if prototypes.insert(proto, (resource, anchor)).is_some() {
            return Err(failure("duplicate prototype"));
        }
    }
    Ok(prototypes)
}

fn editing_specs(
    e: &Value,
    world: &World,
    prototypes: &BTreeMap<String, (String, Anchor)>,
) -> Result<EditingCapabilities, PreviewError> {
    closed_optional(e, &["movable_uids", "terrain", "fields"], &["objects"])?;
    let mut editing = EditingCapabilities::default();
    for uid in array(member(e, "movable_uids")?)? {
        let uid = identifier(uid)?;
        if world
            .object_location(&uid)
            .is_none_or(|l| l.path.len() != 1)
            || !editing.movable_uids.insert(uid)
        {
            return Err(failure("invalid or duplicate movable UID"));
        }
    }
    for t in array(member(e, "terrain")?)? {
        closed(t, &["grid", "values"])?;
        let grid = identifier(member(t, "grid")?)?;
        if !world.profile().ground_layers.contains(&grid) {
            return Err(failure("unknown editable grid"));
        }
        let mut values = BTreeSet::new();
        for value in array(member(t, "values")?)? {
            let proto = identifier(value)?;
            if !prototypes.contains_key(&proto) || !values.insert(proto) {
                return Err(failure("invalid or duplicate terrain capability"));
            }
        }
        if values.is_empty() || editing.terrain.insert(grid, values).is_some() {
            return Err(failure("duplicate or empty terrain capability"));
        }
    }
    for f in array(member(e, "fields")?)? {
        closed(f, &["uid", "field"])?;
        let uid = identifier(member(f, "uid")?)?;
        let field = identifier(member(f, "field")?)?;
        if world.object(&uid).is_none()
            || world.profile().object_fields.field(&field).is_none()
            || !editing.fields.insert((uid, field))
        {
            return Err(failure("invalid or duplicate field capability"));
        }
    }
    if let Some(objects) = e.get("objects") {
        editing.objects = object_grants(objects, world, prototypes)?;
    }
    Ok(editing)
}

/// `editing.objects`: `[{proto, layer, fields?}]`, one entry per prototype.
fn object_grants(
    value: &Value,
    world: &World,
    prototypes: &BTreeMap<String, (String, Anchor)>,
) -> Result<BTreeMap<String, ObjectGrant>, PreviewError> {
    let mut grants = BTreeMap::new();
    for entry in array(value)? {
        closed_optional(entry, &["proto", "layer"], &["fields"])?;
        let proto = identifier(member(entry, "proto")?)?;
        if !prototypes.contains_key(&proto) {
            return Err(failure("object capability names an unmapped prototype"));
        }
        let layer = identifier(member(entry, "layer")?)?;
        if world.profile().render_layer_index(&layer).is_none() {
            return Err(failure("object capability names an unknown render layer"));
        }
        let mut fields = BTreeSet::new();
        if let Some(names) = entry.get("fields") {
            for name in array(names)? {
                let field = identifier(name)?;
                if world.profile().object_fields.field(&field).is_none() || !fields.insert(field) {
                    return Err(failure("invalid or duplicate object field capability"));
                }
            }
        }
        if grants
            .insert(proto, ObjectGrant { layer, fields })
            .is_some()
        {
            return Err(failure("duplicate object capability prototype"));
        }
    }
    Ok(grants)
}
