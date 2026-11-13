// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Zone management: display name, parent, creation and deletion of the zone
//! definitions in a plane's `zones.jsonc`, each one reviewed World
//! transaction. Zone ids are fixed; nothing here changes one. The editor
//! checks only what it must to refuse clearly (the grant, identifier shape,
//! duplicates, cycles and use); World validates the result when it is
//! prepared and its refusal leaves nothing pending.
use super::{Action, Session, SessionError, SessionPhase, refuse};
use serde_json::{Value, json};
use tilewright_source::Plane;

/// Most characters of a zone's display name (FORMAT.md section 8). Checked
/// here so the refusal says so; World checks it too.
pub const MAXIMUM_NAME_CHARACTERS: usize = 256;

/// JSON text of `text` as a string.
fn string(text: &str) -> String {
    Value::String(text.into()).to_string()
}

impl Session {
    pub(super) fn manage_zones(&mut self, action: &Action) -> Result<(), SessionError> {
        if !self.catalog.editing().zones.manage {
            return Err(refuse(
                "zone management refused: the catalog does not grant editing.zones.manage",
            ));
        }
        let (Action::RenameZone {
            plane, expected, ..
        }
        | Action::SetZoneParent {
            plane, expected, ..
        }
        | Action::CreateZone {
            plane, expected, ..
        }
        | Action::DeleteZone {
            plane, expected, ..
        }) = action
        else {
            return Err(refuse("not a zone management action"));
        };
        let expected = *expected;
        if plane != self.catalog.view().0 {
            return Err(refuse(
                "zone management is limited to the active view's plane",
            ));
        }
        self.verify_revision(expected)?;
        let path = format!("planes/{plane}/zones.jsonc");
        let world = self.history.document().world();
        let source = world
            .planes()
            .get(plane)
            .ok_or_else(|| refuse("unknown plane"))?;
        if let Action::RenameZone {
            name: Some(name), ..
        }
        | Action::CreateZone {
            name: Some(name), ..
        } = action
            && name.chars().count() > MAXIMUM_NAME_CHARACTERS
        {
            return Err(refuse(
                "display name refused: a zone's display name has at most 256 characters",
            ));
        }
        let operations = match action {
            Action::RenameZone { zone, name, .. } => rename(
                source,
                &path,
                zone,
                name.as_deref().filter(|n| !n.is_empty()),
            )?,
            Action::SetZoneParent { zone, parent, .. } => {
                reparent(source, &path, zone, parent.as_deref())?
            }
            Action::CreateZone {
                zone, name, parent, ..
            } => {
                let name = name.as_deref().filter(|name| !name.is_empty());
                check_new(source, zone, parent.as_deref())?;
                self.create_operations(&path, zone, name, parent.as_deref())?
            }
            Action::DeleteZone { zone, .. } => delete(source, &path, zone)?,
            _ => return Err(refuse("not a zone management action")),
        };
        self.prepare_text(expected, &operations)
    }

    /// The operations that add the new zone: an `insert` at the end of the
    /// `zones` array guarded by an absent test, or `document.create` of
    /// `zones.jsonc` when the plane has none.
    fn create_operations(
        &self,
        path: &str,
        zone: &str,
        name: Option<&str>,
        parent: Option<&str>,
    ) -> Result<Vec<String>, SessionError> {
        // Written as text so the entry keeps this member order and a space
        // after each colon and comma, like the format's own examples.
        let mut entry = format!("{{\"id\": {}", string(zone));
        if let Some(parent) = parent {
            entry.push_str(", \"parent\": ");
            entry.push_str(&string(parent));
        }
        if let Some(name) = name {
            entry.push_str(", \"name\": ");
            entry.push_str(&string(name));
        }
        entry.push('}');
        let document = self.history.document();
        let limits = document.load_options().limits;
        let limit = u64::try_from(limits.max_input_bytes)
            .map_err(|_| refuse("source byte budget overflow"))?;
        let bytes = document
            .source()
            .read(path, limit)
            .map_err(|error| SessionError(error.to_string()))?;
        let Some(bytes) = bytes else {
            // World writes the new document in its canonical layout.
            let text = format!("{{\"format\": \"tilewright.zones/1\", \"zones\": [{entry}]}}");
            return Ok(vec![format!(
                "{{\"op\": \"document.create\", \"path\": {}, \"document\": {text}}}",
                string(path)
            )]);
        };
        let text = String::from_utf8(bytes).map_err(|_| refuse("zones.jsonc is not UTF-8"))?;
        let syntax = tilewright_jsonc::Document::parse(text, &limits)
            .map_err(|error| SessionError(error.to_string()))?;
        let count = syntax
            .root()
            .get("zones")
            .and_then(tilewright_jsonc::Value::as_array)
            .ok_or_else(|| refuse("zones.jsonc has no zones array to add to"))?
            .len();
        Ok(vec![
            json!({"op": "test", "path": path, "pointer": ["zones", {"id": zone}], "absent": true})
                .to_string(),
            format!(
                "{{\"op\": \"insert\", \"path\": {}, \"pointer\": [\"zones\", {count}], \"value\": {entry}}}",
                string(path)
            ),
        ])
    }

    /// Prepare one transaction of `operations`, each already JSON text.
    /// History seals the consulted files and checks the full World revision;
    /// a World refusal leaves nothing pending.
    fn prepare_text(
        &mut self,
        expected: [u8; 32],
        operations: &[String],
    ) -> Result<(), SessionError> {
        let text = format!(
            "{{\"format\":\"tilewright.transaction/1\",\"title\":\"Editor semantic action\",\"actor\":\"editor:prototype\",\"operations\":[{}]}}",
            operations.join(",")
        );
        self.history
            .prepare(expected, "editor:action", &text)
            .map_err(|error| SessionError(format!("zone change refused: {error}")))?;
        self.status.phase = SessionPhase::Pending;
        self.status.message = "Pending World diff; Accept publishes, Cancel writes nothing".into();
        self.invalidate_frame()
    }
}

fn known<'w>(plane: &'w Plane, zone: &str) -> Result<&'w tilewright_source::Zone, SessionError> {
    plane
        .zones
        .get(zone)
        .ok_or_else(|| refuse("unknown zone: it is not in this plane's zones.jsonc"))
}

/// `set` (or `test` absent and `set`) of the `name` member, or its `remove`;
/// each expects the committed value.
fn rename(
    plane: &Plane,
    path: &str,
    zone: &str,
    name: Option<&str>,
) -> Result<Vec<String>, SessionError> {
    let old = known(plane, zone)?.name.as_deref();
    member_change(
        path,
        zone,
        "name",
        old,
        name,
        "the zone already has this display name",
    )
}

fn reparent(
    plane: &Plane,
    path: &str,
    zone: &str,
    parent: Option<&str>,
) -> Result<Vec<String>, SessionError> {
    let old = known(plane, zone)?.parent.as_deref();
    if let Some(parent) = parent {
        check_parent(plane, parent)?;
        if crate::zones::is_within(plane, parent, zone) {
            return Err(refuse(
                "parent refused: a zone cannot be its own parent or the child of one of its descendants (that would make a cycle)",
            ));
        }
    }
    member_change(
        path,
        zone,
        "parent",
        old,
        parent,
        "the zone already has this parent",
    )
}

fn member_change(
    path: &str,
    zone: &str,
    member: &str,
    old: Option<&str>,
    new: Option<&str>,
    unchanged: &str,
) -> Result<Vec<String>, SessionError> {
    let pointer = json!(["zones", {"id": zone}, member]);
    Ok(match (old, new) {
        (None, None) => return Err(refuse(unchanged)),
        (Some(old), Some(new)) if old == new => return Err(refuse(unchanged)),
        (Some(old), Some(new)) => vec![
            json!({"op": "set", "path": path, "pointer": pointer, "expect": old, "value": new})
                .to_string(),
        ],
        (None, Some(new)) => vec![
            json!({"op": "test", "path": path, "pointer": pointer, "absent": true}).to_string(),
            json!({"op": "set", "path": path, "pointer": pointer, "value": new}).to_string(),
        ],
        (Some(old), None) => vec![
            json!({"op": "remove", "path": path, "pointer": pointer, "expect": old}).to_string(),
        ],
    })
}

fn check_parent(plane: &Plane, parent: &str) -> Result<(), SessionError> {
    if plane.zones.contains_key(parent) {
        Ok(())
    } else {
        Err(refuse(
            "unknown parent zone: it is not in this plane's zones.jsonc",
        ))
    }
}

fn check_new(plane: &Plane, zone: &str, parent: Option<&str>) -> Result<(), SessionError> {
    if !tilewright_source::is_identifier(zone) {
        return Err(refuse(
            "invalid zone id: use 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
        ));
    }
    if plane.zones.contains_key(zone) {
        return Err(refuse("zone id already used in this plane"));
    }
    parent.map_or(Ok(()), |parent| check_parent(plane, parent))
}

/// One `remove` of the zone's entry, expecting its exact canonical JSON, once
/// [`crate::zones::zone_use`] finds it unused.
fn delete(plane: &Plane, path: &str, zone: &str) -> Result<Vec<String>, SessionError> {
    let definition = known(plane, zone)?;
    let usage = crate::zones::zone_use(plane, zone);
    if !usage.unused() {
        return Err(SessionError(format!(
            "zone is in use and cannot be deleted: {}",
            usage.reasons().join("; ")
        )));
    }
    Ok(vec![format!(
        "{{\"op\": \"remove\", \"path\": {}, \"pointer\": [\"zones\", {{\"id\": {}}}], \"expect\": {}}}",
        string(path),
        string(zone),
        definition.canonical
    )])
}
