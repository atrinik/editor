// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Pure action routing. World History remains the only document/history owner.
use serde_json::{Value, json};
use std::fmt;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_preview::{
    PreviewCatalog, PreviewLimits, ReadyResources, SelectionKey, loader,
};
use tilewright_source::{FieldValue, World};

#[derive(Clone, Debug)]
pub enum Action {
    Select {
        generation: u64,
        key: SelectionKey,
    },
    Move {
        uid: String,
        x: i64,
        y: i64,
        expected: [u8; 32],
    },
    SetField {
        uid: String,
        field: String,
        value: Value,
        expected: [u8; 32],
    },
    Paint {
        plane: String,
        layer: i32,
        grid: String,
        x: i64,
        y: i64,
        proto: String,
        expected: [u8; 32],
    },
    Accept,
    Cancel,
    Undo,
    Redo,
    Reopen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPhase {
    Ready,
    Pending,
    Retaining,
    Publishing,
    Blocked,
}

#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub phase: SessionPhase,
    pub message: String,
    pub applied: Option<bool>,
    pub durable: Option<bool>,
    pub cleaned: Option<bool>,
    pub recovery_descriptor: Option<String>,
}
impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.phase, self.message)?;
        if let Some(applied) = self.applied {
            write!(
                f,
                " applied={applied} durable={:?} cleaned={:?}",
                self.durable, self.cleaned
            )?;
        }
        if let Some(record) = &self.recovery_descriptor {
            write!(f, " recovery={record}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SessionError(pub String);
impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SessionError {}
fn refuse(message: &str) -> SessionError {
    SessionError(message.into())
}

pub struct Session {
    history: History,
    catalog: PreviewCatalog,
    resources: ReadyResources,
    selection: Option<SelectionKey>,
    generation: u64,
    status: SessionStatus,
}
impl Session {
    pub fn new(history: History, catalog: PreviewCatalog, resources: ReadyResources) -> Self {
        Self {
            history,
            catalog,
            resources,
            selection: None,
            generation: 1,
            status: SessionStatus {
                phase: SessionPhase::Ready,
                message: "World workspace edits publish immediately after durable recovery".into(),
                applied: None,
                durable: None,
                cleaned: None,
                recovery_descriptor: None,
            },
        }
    }
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn catalog(&self) -> &PreviewCatalog {
        &self.catalog
    }
    pub fn resources(&self) -> &ReadyResources {
        &self.resources
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn selection(&self) -> Option<&SelectionKey> {
        self.selection.as_ref()
    }
    pub fn status(&self) -> &SessionStatus {
        &self.status
    }
    pub fn world(&self) -> &World {
        self.history
            .pending()
            .map_or(self.history.document().world(), |p| p.world())
    }
    /// The recovery service supplies its bounded descriptor; the UI owns no storage.
    pub fn set_recovery_descriptor(&mut self, descriptor: String) {
        self.status.recovery_descriptor = Some(if descriptor.len() <= 1024 {
            descriptor
        } else {
            descriptor.chars().take(1024).collect()
        });
    }
    /// Native camera/viewport changes invalidate earlier input frames.
    pub fn invalidate_frame(&mut self) -> Result<(), SessionError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| refuse("frame generation exhausted"))?;
        Ok(())
    }
    fn verify_catalog(&self) -> Result<(), SessionError> {
        loader::revalidate(
            self.history.document().source(),
            &self.catalog,
            &self.resources,
            PreviewLimits::default(),
        )
        .map_err(|e| SessionError(e.to_string()))
    }
    fn verify_revision(&self, expected: [u8; 32]) -> Result<(), SessionError> {
        if expected != self.history.document().revision() {
            return Err(refuse("stale source revision"));
        }
        let current = self
            .history
            .document()
            .reload()
            .map_err(|e| SessionError(e.to_string()))?;
        if current.revision() != expected {
            return Err(refuse(
                "external source change: inspect and reopen a fresh session",
            ));
        }
        Ok(())
    }
    fn in_bounds(&self, x: i64, y: i64) -> bool {
        let (_, _, [min_x, min_y, w, h]) = self.catalog.view();
        x.checked_sub(min_x).is_some_and(|v| v >= 0 && v < w)
            && y.checked_sub(min_y).is_some_and(|v| v >= 0 && v < h)
    }
    fn valid_key(&self, key: &SelectionKey) -> bool {
        match key {
            SelectionKey::Object { uid } => {
                let (plane, layer, _) = self.catalog.view();
                self.world().object_location(uid).is_some_and(|location| {
                    location.path.len() == 1 && location.plane == plane && location.chunk.0 == layer
                })
            }
            SelectionKey::Cell {
                plane,
                layer,
                grid,
                x,
                y,
            } => {
                let (p, l, _) = self.catalog.view();
                plane == p
                    && *layer == l
                    && self.world().profile().ground_layers.contains(grid)
                    && self.in_bounds(*x, *y)
            }
        }
    }
    fn prepare(&mut self, expected: [u8; 32], op: &Value) -> Result<(), SessionError> {
        self.prepare_operations(expected, std::slice::from_ref(op))
    }
    fn prepare_operations(
        &mut self,
        expected: [u8; 32],
        operations: &[Value],
    ) -> Result<(), SessionError> {
        self.verify_revision(expected)?;
        let text = json!({"format":"tilewright.transaction/1","title":"Editor semantic action","actor":"editor:prototype","operations":operations}).to_string();
        // History seals the complete consulted file base and enforces full World revision.
        self.history
            .prepare(expected, "editor:action", &text)
            .map_err(|e| SessionError(e.to_string()))?;
        self.status.phase = SessionPhase::Pending;
        self.status.message = "Pending World diff; Accept publishes, Cancel writes nothing".into();
        self.invalidate_frame()
    }
    pub fn dispatch(
        &mut self,
        action: Action,
        recovery: &mut impl RecoveryStore,
    ) -> Result<(), SessionError> {
        let result = self.dispatch_inner(action, recovery);
        if let Err(error) = &result {
            self.status.message = error.to_string();
            if self.status.phase != SessionPhase::Blocked {
                self.status.phase = if self.history.needs_reload() {
                    SessionPhase::Blocked
                } else if self.history.pending().is_some() {
                    SessionPhase::Pending
                } else {
                    SessionPhase::Ready
                };
            }
        }
        result
    }
    #[allow(clippy::too_many_lines)]
    fn dispatch_inner(
        &mut self,
        action: Action,
        recovery: &mut impl RecoveryStore,
    ) -> Result<(), SessionError> {
        if matches!(action, Action::Reopen) {
            return self.inspect_reopen();
        }
        if self.status.phase == SessionPhase::Blocked || self.history.needs_reload() {
            return Err(refuse(
                "session blocked: inspect recovery and reopen explicitly",
            ));
        }
        if !matches!(action, Action::Cancel) {
            self.verify_catalog()?;
        }
        match action {
            Action::Select { generation, key } => {
                self.verify_revision(self.history.document().revision())?;
                if generation != self.generation || !self.valid_key(&key) {
                    return Err(refuse("stale frame or unknown semantic selection"));
                }
                self.selection = Some(key);
                Ok(())
            }
            Action::Move {
                uid,
                x,
                y,
                expected,
            } => {
                if !self.catalog.editing().movable_uids.contains(&uid) || !self.in_bounds(x, y) {
                    return Err(refuse("movement capability or bounds refused"));
                }
                let world = self.history.document().world();
                let object = world.object(&uid).ok_or_else(|| refuse("unknown UID"))?;
                let location = world
                    .object_location(&uid)
                    .ok_or_else(|| refuse("unknown UID location"))?;
                let (plane, layer, _) = self.catalog.view();
                if location.path.len() != 1 || location.plane != plane || location.chunk.0 != layer
                {
                    return Err(refuse(
                        "only placed objects in the active spatial layer can move",
                    ));
                }
                if location.chunk.1 == x.div_euclid(32) && location.chunk.2 == y.div_euclid(32) {
                    let chunk = world
                        .planes()
                        .get(&location.plane)
                        .and_then(|plane| plane.chunks.get(&location.chunk))
                        .ok_or_else(|| refuse("missing authoritative object chunk"))?;
                    let old_at = object
                        .at
                        .ok_or_else(|| refuse("placed object has no coordinates"))?;
                    let at = [
                        u16::try_from(x.rem_euclid(32)).map_err(|_| refuse("local x overflow"))?,
                        u16::try_from(y.rem_euclid(32)).map_err(|_| refuse("local y overflow"))?,
                    ];
                    let index = location.path[0];
                    // World 0.2 forbids object-relative set of `at`. A document
                    // pointer plus UID/prototype/old-value tests provides the
                    // supported order-preserving transaction for this chunk.
                    let coordinate = if old_at == at {
                        // A no-op move must preserve even the original coordinate
                        // whitespace and must not create a recovery record.
                        json!({"op":"test","path":chunk.path,"pointer":["objects",index,"at"],"value":at})
                    } else {
                        json!({"op":"set","path":chunk.path,"pointer":["objects",index,"at"],"expect":old_at,"value":at})
                    };
                    self.prepare_operations(expected, &[
                        json!({"op":"test","object":{"uid":uid},"pointer":["proto"],"value":object.proto}),
                        json!({"op":"test","path":chunk.path,"pointer":["objects",index,"uid"],"value":uid}),
                        coordinate,
                    ])
                } else {
                    // Crossing chunks uses the released relocation operation;
                    // exporters may refuse order changes they cannot represent.
                    self.prepare(expected, &json!({"op":"object.move","uid":uid,"expect":{"proto":object.proto},"to":{"at":[x,y]}}))
                }
            }
            Action::SetField {
                uid,
                field,
                value,
                expected,
            } => {
                bounded_value(&value)?;
                if !self
                    .catalog
                    .editing()
                    .fields
                    .contains(&(uid.clone(), field.clone()))
                {
                    return Err(refuse("field capability or value budget refused"));
                }
                let world = self.history.document().world();
                if world.profile().object_fields.field(&field).is_none() {
                    return Err(refuse("field absent from authoritative schema"));
                }
                let object = world.object(&uid).ok_or_else(|| refuse("unknown UID"))?;
                let mut op = json!({"op":"set","object":{"uid":uid},"pointer":["fields",field],"value":value});
                if let Some((_, before)) = object.fields.iter().find(|(name, _)| name == &field) {
                    op["expect"] = field_json(before)?;
                }
                self.prepare(expected, &op)
            }
            Action::Paint {
                plane,
                layer,
                grid,
                x,
                y,
                proto,
                expected,
            } => {
                let (p, l, _) = self.catalog.view();
                if plane != p
                    || layer != l
                    || !self.in_bounds(x, y)
                    || !self
                        .catalog
                        .editing()
                        .terrain
                        .get(&grid)
                        .is_some_and(|values| values.contains(&proto))
                {
                    return Err(refuse("terrain capability or bounds refused"));
                }
                let world = self.history.document().world();
                let index = world
                    .profile()
                    .ground_layers
                    .iter()
                    .position(|g| g == &grid)
                    .ok_or_else(|| refuse("unknown ground grid"))?;
                let local = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32))
                    .map_err(|_| refuse("cell overflow"))?;
                let before = world
                    .planes()
                    .get(&plane)
                    .and_then(|p| p.chunks.get(&(layer, x.div_euclid(32), y.div_euclid(32))))
                    .and_then(|c| c.ground.get(index))
                    .and_then(|g| g.value(local));
                self.prepare(expected,&json!({"op":"terrain.set","plane":plane,"layer":layer,"grid":grid,"cells":[{"at":[x,y],"value":proto,"expect":before}]}))
            }
            Action::Accept => self.accept(recovery),
            Action::Cancel => {
                self.history.cancel();
                self.status.phase = SessionPhase::Ready;
                self.status.message = "Pending action cancelled; no publication".into();
                self.invalidate_frame()
            }
            Action::Undo | Action::Redo => {
                self.verify_revision(self.history.document().revision())?;
                let result = if matches!(action, Action::Undo) {
                    self.history.prepare_undo()
                } else {
                    self.history.prepare_redo()
                };
                result.map_err(|e| SessionError(e.to_string()))?;
                self.status.phase = SessionPhase::Pending;
                self.status.message = "History transaction pending; review and Accept".into();
                self.invalidate_frame()
            }
            Action::Reopen => unreachable!(),
        }
    }
    fn accept(&mut self, recovery: &mut impl RecoveryStore) -> Result<(), SessionError> {
        self.verify_revision(self.history.document().revision())?;
        let changed = self
            .history
            .pending()
            .is_some_and(|pending| pending.changed().next().is_some());
        self.status.applied = None;
        self.status.durable = None;
        self.status.cleaned = None;
        self.status.phase = SessionPhase::Retaining;
        // Existing History calls retain immediately before publish. The wrapper
        // advances the session phase only when retention has succeeded.
        let mut retaining = Retaining {
            store: recovery,
            phase: &mut self.status.phase,
        };
        let result = self
            .history
            .commit(&mut retaining, &tilewright_edit::PublishOptions::default());
        match result {
            Ok(published) => {
                self.status.phase = SessionPhase::Ready;
                self.status.applied = Some(changed);
                self.status.durable = Some(published.durable);
                self.status.cleaned = Some(true);
                self.status.message =
                    "Published World workspace and reloaded verified result".into();
                self.selection = None;
                self.invalidate_frame()
            }
            Err(error) => {
                self.status.message = error.to_string();
                match &error {
                    tilewright_editor_commands::Error::Publish(e) => {
                        self.status.applied = Some(e.applied);
                        self.status.durable = Some(e.durable);
                        self.status.cleaned = Some(e.cleaned);
                    }
                    tilewright_editor_commands::Error::PublishedStateChanged {
                        published, ..
                    } => {
                        self.status.applied = Some(true);
                        self.status.durable = Some(published.durable);
                        self.status.cleaned = Some(true);
                    }
                    _ => {}
                }
                self.status.phase = if self.history.needs_reload() {
                    SessionPhase::Blocked
                } else if self.history.pending().is_some() {
                    SessionPhase::Pending
                } else {
                    SessionPhase::Ready
                };
                Err(SessionError(error.to_string()))
            }
        }
    }
    fn inspect_reopen(&mut self) -> Result<(), SessionError> {
        let reopened = match self.history.document().reload() {
            Ok(document) => document,
            Err(error) => {
                self.status.phase = SessionPhase::Blocked;
                return Err(SessionError(format!(
                    "reopen inspection failed: {error}; preserve recovery and inspect source"
                )));
            }
        };
        if let Err(error) = self.verify_catalog() {
            self.status.phase = SessionPhase::Blocked;
            return Err(error);
        }
        self.invalidate_frame()?;
        if self.history.needs_reload() || reopened.revision() != self.history.document().revision()
        {
            self.status.phase = SessionPhase::Blocked;
            self.status.message="Reopen inspection complete; source differs or publication uncertain. Preserve recovery; open a fresh explicitly inspected session".into();
            return Err(refuse(&self.status.message));
        }
        self.selection = None;
        self.status.message = "Reopen verified committed source revision".into();
        Ok(())
    }
}
fn field_json(value: &FieldValue) -> Result<Value, SessionError> {
    match value {
        FieldValue::Null => Ok(Value::Null),
        FieldValue::Bool(v) => Ok(json!(v)),
        FieldValue::Integer(v) => Ok(json!(v)),
        FieldValue::Number(v) => Ok(json!(v)),
        FieldValue::String(v) => Ok(json!(v)),
        FieldValue::Json(v) if v.len() <= 65536 => {
            serde_json::from_str(v).map_err(|e| SessionError(e.to_string()))
        }
        FieldValue::Json(_) => Err(refuse("existing field exceeds semantic editing budget")),
    }
}

struct Retaining<'a, R> {
    store: &'a mut R,
    phase: &'a mut SessionPhase,
}
impl<R: RecoveryStore> RecoveryStore for Retaining<'_, R> {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.store.retain(inverse)?;
        *self.phase = SessionPhase::Publishing;
        Ok(())
    }
}

/// Bound arbitrary action values before serde allocates their escaped encoding.
fn bounded_value(value: &Value) -> Result<(), SessionError> {
    let mut pending = vec![(value, 0usize)];
    let mut bytes = 0usize;
    let mut count = 0usize;
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if depth > 8 || count > 4096 {
            return Err(refuse("field value nesting or entry budget exceeded"));
        }
        let size = match value {
            Value::String(text) => text.len().checked_mul(6).and_then(|v| v.checked_add(3)),
            Value::Array(values) => {
                if values.len() > 4096usize.saturating_sub(count + pending.len()) {
                    return Err(refuse("field value entry budget exceeded"));
                }
                pending.extend(values.iter().map(|v| (v, depth + 1)));
                Some(values.len() + 2)
            }
            Value::Object(values) => {
                if values.len() > 4096usize.saturating_sub(count + pending.len()) {
                    return Err(refuse("field value entry budget exceeded"));
                }
                let mut keys = Some(2usize);
                for (key, value) in values {
                    keys = keys.and_then(|n| {
                        key.len()
                            .checked_mul(6)
                            .and_then(|k| n.checked_add(k))
                            .and_then(|n| n.checked_add(4))
                    });
                    pending.push((value, depth + 1));
                }
                keys
            }
            _ => Some(32),
        };
        bytes = size
            .and_then(|size| bytes.checked_add(size))
            .filter(|size| *size <= 65536)
            .ok_or_else(|| refuse("field value encoded byte budget exceeded"))?;
    }
    Ok(())
}
