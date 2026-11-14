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
    /// author delete, move and edit the named fields of objects of that
    /// prototype, but only top-level objects in the active view (see
    /// [`PreviewCatalog::in_active_view`]); `layer` is the render layer a
    /// placed object will use.
    pub objects: BTreeMap<String, ObjectGrant>,
    /// Optional `editing.zones`. Absent, nothing about zones is granted.
    pub zones: ZoneGrant,
    /// Optional `editing.anchors`. Absent, nothing about anchors is granted.
    pub anchors: AnchorGrant,
}

/// `editing.anchors`: a capability flag, never a list of anchor ids, so the
/// catalog keeps loading when anchors are added, moved or deleted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AnchorGrant {
    /// Edit the anchors of the active view's plane: create one on a tile of
    /// the active view, move one that is on a tile of the view to another,
    /// and delete any one of the plane that no portal names, wherever it
    /// is. Anchor ids are fixed once created.
    pub edit: bool,
}

/// `editing.zones`: capability flags, never a list of zone ids, so the
/// catalog keeps loading when zones are added, renamed or deleted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ZoneGrant {
    /// Paint the zone of tiles in the active view (the zone brush).
    pub paint: bool,
    /// Manage the zone definitions of the active view's plane: edit a
    /// zone's display name and parent, create a zone, and delete an unused
    /// one. Zone ids are fixed once created.
    pub manage: bool,
}

/// One `editing.objects` entry, keyed by its prototype.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectGrant {
    pub layer: String,
    pub fields: BTreeSet<String>,
    /// Optional `defaults`: field values written into a newly placed object
    /// of this prototype, as `(name, canonical JSON)` in catalog order. The
    /// names are profile object fields; World validates the values again
    /// when the placement is prepared.
    pub defaults: Vec<(String, String)>,
    /// Optional `portal`: set, change or clear the portal of a top-level
    /// object of this prototype in the active view. The target may be
    /// anywhere; World checks that its plane and anchor exist.
    pub portal: bool,
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
    /// The catalog's prototypes in id order: each with the id of the
    /// resource it is drawn with and its anchor, `ground` or `standing`.
    pub fn prototypes(&self) -> impl Iterator<Item = (&str, &str, &'static str)> {
        self.prototypes.iter().map(|(proto, (resource, anchor))| {
            (
                proto.as_str(),
                resource.as_str(),
                match anchor {
                    Anchor::Ground => "ground",
                    Anchor::Standing => "standing",
                },
            )
        })
    }
    /// How many resources the catalog declares.
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }
    /// Whether `uid` names a top-level object in this view's plane and
    /// spatial layer whose tile lies inside the view bounds. Per-prototype
    /// grants reach only such objects; per-uid grants keep their own reach.
    pub fn in_active_view(&self, world: &World, uid: &str) -> bool {
        let (Some(location), Some(object)) = (world.object_location(uid), world.object(uid)) else {
            return false;
        };
        let Some([x, y]) = object.at else {
            return false;
        };
        let Some(plane) = world.planes().get(&location.plane) else {
            return false;
        };
        let size = i64::from(plane.chunk_size);
        let global =
            |chunk: i64, local: u16| chunk.checked_mul(size)?.checked_add(i64::from(local));
        let (Some(x), Some(y)) = (global(location.chunk.1, x), global(location.chunk.2, y)) else {
            return false;
        };
        location.path.len() == 1
            && self.tile_in_active_view(&location.plane, location.chunk.0, x, y)
    }
    /// Whether plane tile `x, y` of spatial `layer` in `plane` is inside the
    /// active view: the catalog's plane and spatial layer, with the tile in
    /// the half-open view bounds. Placement uses the same scope as the
    /// per-prototype grants of [`Self::in_active_view`].
    pub fn tile_in_active_view(&self, plane: &str, layer: i32, x: i64, y: i64) -> bool {
        let [min_x, min_y, width, height] = self.bounds;
        plane == self.plane
            && layer == self.layer
            && x.checked_sub(min_x).is_some_and(|x| x >= 0 && x < width)
            && y.checked_sub(min_y).is_some_and(|y| y >= 0 && y < height)
    }
    /// The `editing.objects` grant for `uid`'s prototype, when the object
    /// is in the active view.
    fn prototype_grant(&self, world: &World, uid: &str) -> Option<&ObjectGrant> {
        let grant = self.editing.objects.get(&world.object(uid)?.proto)?;
        self.in_active_view(world, uid).then_some(grant)
    }
    /// Movement is granted per uid (`movable_uids`) or, in the active view,
    /// per prototype.
    pub fn can_move(&self, world: &World, uid: &str) -> bool {
        self.editing.movable_uids.contains(uid) || self.prototype_grant(world, uid).is_some()
    }
    /// A field edit is granted per uid (`fields`) or, in the active view,
    /// per prototype.
    pub fn can_edit_field(&self, world: &World, uid: &str, field: &str) -> bool {
        self.editing
            .fields
            .iter()
            .any(|(object, name)| object == uid && name == field)
            || self
                .prototype_grant(world, uid)
                .is_some_and(|grant| grant.fields.contains(field))
    }
    /// Deletion is granted only per prototype, in the active view.
    pub fn can_delete(&self, world: &World, uid: &str) -> bool {
        self.prototype_grant(world, uid).is_some()
    }
    /// Portal editing is granted only per prototype (`portal: true`), in
    /// the active view.
    pub fn can_edit_portal(&self, world: &World, uid: &str) -> bool {
        self.prototype_grant(world, uid)
            .is_some_and(|grant| grant.portal)
    }
    /// Every field name granted for `uid`, per uid then per prototype,
    /// without duplicates.
    pub fn editable_fields(&self, world: &World, uid: &str) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .editing
            .fields
            .iter()
            .filter(|(object, _)| object == uid)
            .map(|(_, field)| field.as_str())
            .collect();
        if let Some(grant) = self.prototype_grant(world, uid) {
            for field in &grant.fields {
                if !names.contains(&field.as_str()) {
                    names.push(field);
                }
            }
        }
        names
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
    let editing = editing_specs(&document, member(root, "editing")?, world, &prototypes)?;
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
    document: &Document,
    e: &Value,
    world: &World,
    prototypes: &BTreeMap<String, (String, Anchor)>,
) -> Result<EditingCapabilities, PreviewError> {
    closed_optional(
        e,
        &["movable_uids", "terrain", "fields"],
        &["objects", "zones", "anchors"],
    )?;
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
        editing.objects = object_grants(document, objects, world, prototypes)?;
    }
    if let Some(zones) = e.get("zones") {
        closed(zones, &["paint", "manage"])?;
        let flag = |key| {
            member(zones, key)?
                .as_bool()
                .ok_or_else(|| failure("zone capability flags must be true or false"))
        };
        editing.zones = ZoneGrant {
            paint: flag("paint")?,
            manage: flag("manage")?,
        };
    }
    if let Some(anchors) = e.get("anchors") {
        closed(anchors, &["edit"])?;
        editing.anchors = AnchorGrant {
            edit: member(anchors, "edit")?
                .as_bool()
                .ok_or_else(|| failure("anchor capability flags must be true or false"))?,
        };
    }
    Ok(editing)
}

/// `editing.objects`: `[{proto, layer, fields?, defaults?, portal?}]`, one
/// entry per prototype.
fn object_grants(
    document: &Document,
    value: &Value,
    world: &World,
    prototypes: &BTreeMap<String, (String, Anchor)>,
) -> Result<BTreeMap<String, ObjectGrant>, PreviewError> {
    let mut grants = BTreeMap::new();
    for entry in array(value)? {
        closed_optional(
            entry,
            &["proto", "layer"],
            &["fields", "defaults", "portal"],
        )?;
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
        let defaults = match entry.get("defaults") {
            Some(values) => object_defaults(document, values, world)?,
            None => Vec::new(),
        };
        let portal = match entry.get("portal") {
            Some(flag) => flag
                .as_bool()
                .ok_or_else(|| failure("object capability portal must be true or false"))?,
            None => false,
        };
        if grants
            .insert(
                proto,
                ObjectGrant {
                    layer,
                    fields,
                    defaults,
                    portal,
                },
            )
            .is_some()
        {
            return Err(failure("duplicate object capability prototype"));
        }
    }
    Ok(grants)
}

/// Size limit of one entry's `defaults`: the canonical JSON bytes of its
/// values, names not counted, like a field edit's value.
const MAXIMUM_DEFAULTS_BYTES: usize = 65536;

/// `defaults`: `{field: value}` for profile object fields. Each value is
/// checked against its field's type here; the contents of structured
/// (`json`, `list`, `object`) values are left to World, which validates the
/// whole object again when a placement is prepared and refuses it there.
fn object_defaults(
    document: &Document,
    value: &Value,
    world: &World,
) -> Result<Vec<(String, String)>, PreviewError> {
    let members = value
        .as_object()
        .ok_or_else(|| failure("object defaults must be an object"))?;
    let mut defaults: Vec<(String, String)> = Vec::new();
    let mut bytes = 0usize;
    for member in members {
        let name = &member.key;
        let spec = world
            .profile()
            .object_fields
            .field(name)
            .ok_or_else(|| failure("object defaults name an unknown object field"))?;
        if defaults.iter().any(|(existing, _)| existing == name) {
            return Err(failure("duplicate object default field"));
        }
        default_value(&spec.field_type, &member.value, world)
            .map_err(|reason| failure(format!("object default {name}: {reason}")))?;
        let canonical = document.canonical(&member.value);
        bytes = bytes
            .checked_add(canonical.len())
            .filter(|total| *total <= MAXIMUM_DEFAULTS_BYTES)
            .ok_or_else(|| failure("object defaults exceed their byte budget"))?;
        defaults.push((name.clone(), canonical));
    }
    Ok(defaults)
}

/// The type check a placement relies on before World sees the object.
fn default_value(
    kind: &tilewright_source::FieldType,
    value: &Value,
    world: &World,
) -> Result<(), &'static str> {
    use tilewright_source::FieldType;
    let valid = match kind {
        FieldType::Boolean => value.as_bool().is_some(),
        FieldType::Integer { minimum, maximum } => value
            .as_i64()
            .is_some_and(|v| (*minimum..=*maximum).contains(&v)),
        FieldType::Number { minimum, maximum } => value
            .as_f64()
            .is_some_and(|v| v.is_finite() && *minimum <= v && v <= *maximum),
        FieldType::String {
            maximum_length,
            values,
        } => value.as_str().is_some_and(|text| {
            text.chars().count() <= *maximum_length && (values.is_empty() || values.contains(text))
        }),
        FieldType::Reference { target } => value.as_str().is_some_and(|id| {
            world
                .definitions()
                .contains_key(&(target.clone(), id.to_owned()))
        }),
        FieldType::List { .. } => value.as_array().is_some(),
        FieldType::Object { .. } => value.as_object().is_some(),
        FieldType::Json => true,
    };
    if valid {
        Ok(())
    } else {
        Err("value does not match the profile field type")
    }
}
