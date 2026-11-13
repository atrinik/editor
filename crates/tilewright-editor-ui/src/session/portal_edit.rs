// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Portal editing: set, change or clear the `portal` member of one placed
//! object, addressed by its uid, as one reviewed World transaction. The
//! editor checks only what it must to refuse clearly (the grant, the
//! object's view scope, that the target plane and anchor exist, and that
//! something changes); World validates the result when it is prepared, and
//! its refusal leaves nothing pending. World does not check that a
//! location target has a chunk or ground: the review warns about that.
use super::{PortalTarget, Session, SessionError, refuse};
use crate::anchors::{Target, resolve};
use serde_json::Value;

/// JSON text of `text` as a string.
fn string(text: &str) -> String {
    Value::String(text.into()).to_string()
}

/// The portal written as text in the format's member order and spacing,
/// with `plane` left out when it is the object's own plane `own`.
fn portal_text(own: &str, target: &PortalTarget) -> String {
    let plane = |plane: &str| {
        if plane == own {
            String::new()
        } else {
            format!("\"plane\": {}, ", string(plane))
        }
    };
    match target {
        PortalTarget::Anchor { plane: p, anchor } => {
            format!("{{{}\"anchor\": {}}}", plane(p), string(anchor))
        }
        PortalTarget::Location {
            plane: p,
            layer,
            at: [x, y],
        } => format!("{{{}\"layer\": {layer}, \"at\": [{x}, {y}]}}", plane(p)),
    }
}

/// Whether `target` is where `current` already leads.
fn same(current: &Target<'_>, target: &PortalTarget) -> bool {
    match (current, target) {
        (
            Target::Anchor { plane, anchor },
            PortalTarget::Anchor {
                plane: p,
                anchor: a,
            },
        ) => plane == p && anchor == a,
        (
            Target::Location { plane, layer, at },
            PortalTarget::Location {
                plane: p,
                layer: l,
                at: a,
            },
        ) => plane == p && layer == l && at == a,
        _ => false,
    }
}

impl Session {
    pub(super) fn edit_portal(
        &mut self,
        uid: &str,
        target: Option<&PortalTarget>,
        expected: [u8; 32],
    ) -> Result<(), SessionError> {
        let world = self.history.document().world();
        let object = world.object(uid).ok_or_else(|| refuse("unknown UID"))?;
        if !self
            .catalog
            .editing()
            .objects
            .get(&object.proto)
            .is_some_and(|grant| grant.portal)
        {
            return Err(refuse(
                "portal editing refused: the catalog does not grant portal editing for this object's prototype (editing.objects portal)",
            ));
        }
        let location = world
            .object_location(uid)
            .ok_or_else(|| refuse("unknown UID location"))?;
        if location.path.len() != 1 {
            return Err(refuse(
                "only placed objects can have their portal edited, not objects carried in an inventory",
            ));
        }
        // The same active-view scope as every per-prototype grant. The
        // target may be anywhere.
        if !self.catalog.can_edit_portal(world, uid) {
            return Err(refuse(
                "only placed objects in the active view can have their portal edited",
            ));
        }
        self.verify_revision(expected)?;
        let world = self.history.document().world();
        let own = location.plane.as_str();
        let current = object.portal.as_ref().map(|portal| resolve(own, portal));
        let committed = self.inspect_object_source(uid, |syntax, object| {
            Ok(object.get("portal").map(|value| syntax.canonical(value)))
        })?;
        let address = format!("{{\"uid\": {}}}", string(uid));
        let operations = match target {
            None => {
                let canonical = committed.ok_or_else(|| refuse("the object has no portal to clear"))?;
                vec![format!(
                    "{{\"op\": \"remove\", \"object\": {address}, \"pointer\": [\"portal\"], \"expect\": {canonical}}}"
                )]
            }
            Some(target) => {
                let (PortalTarget::Anchor { plane, .. } | PortalTarget::Location { plane, .. }) =
                    target;
                let source = world.planes().get(plane).ok_or_else(|| {
                    refuse("unknown target plane: the world has no plane with this id")
                })?;
                if let PortalTarget::Anchor { anchor, .. } = target
                    && !source.anchors.contains_key(anchor)
                {
                    return Err(refuse(
                        "unknown anchor: it is not in the target plane's anchors.jsonc",
                    ));
                }
                if current.as_ref().is_some_and(|current| same(current, target)) {
                    return Err(refuse("the portal already leads there"));
                }
                let value = portal_text(own, target);
                match committed {
                    Some(canonical) => vec![format!(
                        "{{\"op\": \"set\", \"object\": {address}, \"pointer\": [\"portal\"], \"expect\": {canonical}, \"value\": {value}}}"
                    )],
                    None => vec![
                        format!(
                            "{{\"op\": \"test\", \"object\": {address}, \"pointer\": [\"portal\"], \"absent\": true}}"
                        ),
                        format!(
                            "{{\"op\": \"set\", \"object\": {address}, \"pointer\": [\"portal\"], \"value\": {value}}}"
                        ),
                    ],
                }
            }
        };
        self.prepare_text(expected, &operations, "portal change refused")
    }
}
