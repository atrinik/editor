// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Application composition for the bounded provisioned Linux prototype.
use sdl3::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tilewright_editor_commands::History;
use tilewright_editor_document::{WorldDocument, terminal_field, terminal_safe};
use tilewright_editor_preview::{PreparedPreview, PreviewLimits, SelectionKey, ViewSpec, loader};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_ui::{Action, Session};
use tilewright_render_api::{
    BackendPreference, FrameOutput, RenderRequest, Renderer, TargetDescriptor, TargetKind,
};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_render_wgpu::WgpuRenderer;
use tilewright_scene::SemanticId;
use tilewright_source::SourceRoot;

const HELP: &str = "Click select | arrows move | Enter accept | Esc cancel | Z undo | Y redo | T terrain | F field | WASD pan | +/- zoom | R inspect reopen | Q quit";

type Result<T> = std::result::Result<T, String>;

struct Application {
    session: Session,
    recovery: DurableRecovery,
    evidence: Evidence,
    view: ViewSpec,
    prepared: PreparedPreview,
    terrain: bool,
    blocked: Option<String>,
}

/// This command is intentionally a deployment seam, not arbitrary recovery-path
/// selection. The trusted provision helper passes fresh separate retained Linux
/// directory descriptors, whose identity/separation receipt it retains.
pub fn run(arguments: &[String]) -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Err(
            "checked prototype editing requires Linux; other platforms remain read-only".into(),
        );
    }
    let world = descriptor(arguments, "--world-fd")?;
    let recovery = descriptor(arguments, "--recovery-fd")?;
    let evidence = descriptor(arguments, "--evidence-fd")?;
    if world == recovery || world == evidence || recovery == evidence {
        return Err("provisioned descriptor numbers must be distinct".into());
    }
    let reference = arguments.iter().any(|argument| argument == "--reference");
    let demo = arguments.iter().any(|argument| argument == "--demo-script");
    let document = WorldDocument::open_prototype(&world).map_err(|error| error.to_string())?;
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let view = catalog.default_view(document.world(), 960, 640, 1);
    let prepared = tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let session = Session::new(History::new(document), catalog, resources);
    let recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&recovery),
        RecoveryLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let mut app = Application {
        session,
        recovery,
        evidence: Evidence::new(SourceRoot::new(&evidence))?,
        view,
        prepared,
        terrain: false,
        blocked: None,
    };
    println!(
        "{}\n{}",
        HELP,
        terminal_field(app.session.catalog().presentation())
    );
    app.report();
    if reference {
        if !demo {
            return Err(
                "reference mode requires --demo-script; it is evidence, not native acceptance"
                    .into(),
            );
        }
        app.demo_all()?;
        return Ok(());
    }
    native(app, demo)
}

fn descriptor(arguments: &[String], name: &str) -> Result<PathBuf> {
    let index = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing {name}; use tools/provision-prototype.py"))?;
    let value: u32 = arguments
        .get(index + 1)
        .ok_or_else(|| format!("missing {name} value"))?
        .parse()
        .map_err(|_| format!("invalid {name}"))?;
    if value < 3 {
        return Err("provisioned descriptors must not be standard streams".into());
    }
    Ok(PathBuf::from(format!("/proc/self/fd/{value}")))
}

impl Application {
    fn rebuild(&mut self) -> Result<()> {
        let result = self.rebuild_inner();
        if let Err(reason) = &result {
            self.blocked = Some(reason.clone());
        }
        result
    }

    fn rebuild_inner(&mut self) -> Result<()> {
        self.session
            .invalidate_frame()
            .map_err(|error| error.to_string())?;
        self.view.frame_revision = self.session.generation();
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        self.prepared = tilewright_editor_preview::prepare(
            self.session.world(),
            self.session.catalog(),
            self.session.resources(),
            &self.view,
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn action(&mut self, action: Action) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        let result = self
            .session
            .dispatch(action, &mut self.recovery)
            .map_err(|error| error.to_string());
        if let Some(error) = self.recovery.last_error() {
            self.session.set_recovery_descriptor(error.to_string());
        } else if let Some(record) = self.recovery.records().last() {
            self.session.set_recovery_descriptor(format!(
                "{} sha256={}",
                record.name,
                tilewright_editor_document::revision_hex(&record.sha256)
            ));
        }
        // Failed commands may still invalidate pending state (for example an
        // empty Undo). Always rebuild from Session's actual current World.
        let refreshed = self.rebuild();
        self.report();
        match result {
            Err(original) => Err(original),
            Ok(()) => refreshed,
        }
    }

    fn report(&self) {
        for diagnostic in &self.prepared.diagnostics {
            println!(
                "diagnostic {}: {}",
                terminal_field(diagnostic.code),
                terminal_field(&diagnostic.message)
            );
        }
        println!(
            "state={} selection={:?} history={:?}",
            terminal_field(&self.session.status().to_string()),
            self.session.selection(),
            self.session.history().depths()
        );
        if let Some(pending) = self.session.history().pending() {
            let mut remaining = 128 << 10;
            for file in pending.changed() {
                let text = format!(
                    "--- {}\n{}\n+++ {}\n{}\n",
                    file.path,
                    file.before.as_deref().unwrap_or("<absent>"),
                    file.path,
                    file.after.as_deref().unwrap_or("<absent>")
                );
                if text.len() > remaining {
                    println!("diff display truncated; plan retained");
                    break;
                }
                print!("{}", terminal_safe(&text));
                remaining -= text.len();
            }
        }
    }

    fn click(&mut self, x: u32, y: u32) -> Result<()> {
        loader::revalidate(
            self.session.history().document().source(),
            self.session.catalog(),
            self.session.resources(),
            PreviewLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        let key = self
            .prepared
            .pick(self.view.frame_revision, x, y, self.terrain)
            .map_err(|error| error.to_string())?
            .ok_or("no selectable semantic hit at this pixel")?;
        self.action(Action::Select {
            generation: self.view.frame_revision,
            key,
        })
    }

    fn nudge(&mut self, delta: [i64; 2]) -> Result<()> {
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an editable object first".into());
        };
        let location = self
            .session
            .world()
            .object_location(&uid)
            .ok_or("selection no longer exists")?;
        let at = self
            .session
            .world()
            .object(&uid)
            .and_then(|object| object.at)
            .ok_or("selection is not placed")?;
        let x = location
            .chunk
            .1
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[0])))
            .and_then(|value| value.checked_add(delta[0]))
            .ok_or("coordinate overflow")?;
        let y = location
            .chunk
            .2
            .checked_mul(32)
            .and_then(|value| value.checked_add(i64::from(at[1])))
            .and_then(|value| value.checked_add(delta[1]))
            .ok_or("coordinate overflow")?;
        self.action(Action::Move {
            uid,
            x,
            y,
            expected: self.session.history().document().revision(),
        })
    }

    fn field(&mut self) -> Result<()> {
        let Some(SelectionKey::Object { uid }) = self.session.selection().cloned() else {
            return Err("select an object with a field capability first".into());
        };
        let field = self
            .session
            .catalog()
            .editing()
            .fields
            .iter()
            .find(|(object, _)| object == &uid)
            .map(|(_, field)| field.clone())
            .ok_or("selected object has no field capability")?;
        self.action(Action::SetField {
            uid,
            field,
            value: serde_json::Value::String("Edited offline".into()),
            expected: self.session.history().document().revision(),
        })
    }

    fn paint(&mut self) -> Result<()> {
        let Some(SelectionKey::Cell {
            plane,
            layer,
            grid,
            x,
            y,
        }) = self.session.selection().cloned()
        else {
            return Err("select a cell in terrain mode first".into());
        };
        let values = self
            .session
            .catalog()
            .editing()
            .terrain
            .get(&grid)
            .ok_or("grid has no paint capability")?;
        let proto = values
            .iter()
            .next_back()
            .cloned()
            .ok_or("empty paint palette")?;
        self.action(Action::Paint {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            expected: self.session.history().document().revision(),
        })
    }

    fn frame(&self, cells: bool) -> Result<FrameOutput> {
        let mut renderer = ReferenceRenderer::new(offscreen_target(
            self.view.camera.width,
            self.view.camera.height,
        ))
        .map_err(|error| error.to_string())?;
        renderer
            .render(RenderRequest {
                scene: if cells {
                    &self.prepared.cell_scene
                } else {
                    &self.prepared.scene
                },
                resources: self.prepared.resources.clone(),
            })
            .map_err(|error| error.to_string())
    }

    fn capture(&mut self, name: &str) -> Result<()> {
        let frame = self.frame(false)?;
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|error| error.to_string())?
                .write_image_data(&frame.rgba8)
                .map_err(|error| error.to_string())?;
        }
        self.evidence.retain(&format!("{name}.png"), &bytes)?;
        self.evidence.retain(&format!("{name}.json"), serde_json::to_string_pretty(&serde_json::json!({
            "format":"tilewright.prototype-frame/1", "path":"cpu-reference", "frame_generation":self.view.frame_revision,
            "source_revision":tilewright_editor_document::revision_hex(&self.session.world().source_digest()),
            "committed_revision":tilewright_editor_document::revision_hex(&self.session.history().document().revision()),
            "catalog_revision":tilewright_editor_document::revision_hex(&self.prepared.catalog_revision),
            "width":frame.width,"height":frame.height,"status":self.session.status().to_string(),
            "toolkit":"0.2.0","renderer":"0.5.1","pending":self.session.history().pending().is_some()
        })).map_err(|error| error.to_string())?.as_bytes())
    }

    fn select_visible_object(&mut self, for_field: bool) -> Result<()> {
        let frame = self.frame(false)?;
        let hit = self
            .prepared
            .picks
            .iter()
            .find_map(|(id, key)| {
                let SelectionKey::Object { uid } = key else {
                    return None;
                };
                let capabilities = self.session.catalog().editing();
                let eligible = if for_field {
                    capabilities.fields.iter().any(|(object, _)| object == uid)
                } else {
                    capabilities.movable_uids.contains(uid)
                };
                if !eligible {
                    return None;
                }
                pixel_for(&frame, *id)
            })
            .ok_or("no visible object with the requested edit capability in reference mask")?;
        self.terrain = false;
        self.click(hit.0, hit.1)
    }

    fn select_empty_cell(&mut self) -> Result<()> {
        let frame = self.frame(true)?;
        // The authored neutral sample explicitly leaves (6,4) empty. The
        // ordinary mask path must find and select that cell before painting.
        let hit = self
            .prepared
            .cell_picks
            .iter()
            .find_map(|(id, key)| match key {
                SelectionKey::Cell { x: 6, y: 4, .. } => pixel_for(&frame, *id),
                _ => None,
            })
            .ok_or("neutral empty cell is not selectable")?;
        self.terrain = true;
        self.click(hit.0, hit.1)
    }

    fn demo_step(&mut self, step: usize) -> Result<bool> {
        match step {
            0 => {
                self.capture("before")?;
                self.select_visible_object(false)?;
            }
            1 => {
                self.nudge([1, 0])?;
                self.capture("pending")?;
            }
            2 => {
                self.action(Action::Cancel)?;
                self.capture("cancel")?;
                self.nudge([1, 0])?;
                self.action(Action::Accept)?;
                self.capture("after")?;
            }
            3 => {
                self.action(Action::Undo)?;
                self.action(Action::Accept)?;
                self.capture("undo")?;
            }
            4 => {
                self.action(Action::Redo)?;
                self.action(Action::Accept)?;
                self.capture("redo")?;
            }
            5 if !self.session.catalog().editing().fields.is_empty() => {
                self.select_visible_object(true)?;
                self.field()?;
                self.action(Action::Accept)?;
                self.capture("field")?;
            }
            6 if !self.session.catalog().editing().terrain.is_empty() => {
                self.select_empty_cell()?;
                self.paint()?;
                self.action(Action::Accept)?;
                self.capture("terrain")?;
            }
            7 => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn demo_all(&mut self) -> Result<()> {
        for step in 0..8 {
            if self.demo_step(step)? {
                break;
            }
        }
        Ok(())
    }
}

fn pixel_for(frame: &FrameOutput, id: SemanticId) -> Option<(u32, u32)> {
    let index = u64::try_from(frame.semantic_ids.iter().position(|value| *value == id.0)?).ok()?;
    Some((
        u32::try_from(index % u64::from(frame.width)).ok()?,
        u32::try_from(index / u64::from(frame.width)).ok()?,
    ))
}

struct Evidence {
    root: SourceRoot,
    _lock: tilewright_source::SourceLock,
}
impl Evidence {
    fn new(root: SourceRoot) -> Result<Self> {
        let lock = root.lock().map_err(|error| error.to_string())?;
        Ok(Self { root, _lock: lock })
    }
    fn retain(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if self.root.exists(name).map_err(|error| error.to_string())? {
            return Err(format!("evidence output already exists: {name}"));
        }
        let directory = self
            .root
            .directory("", false)
            .map_err(|error| error.to_string())?
            .ok_or("evidence root missing")?;
        let access = directory
            .own_access()
            .map_err(|error| error.to_string())?
            .new_file();
        directory
            .write(name, Some(&access), bytes)
            .map_err(|error| error.to_string())?;
        if !directory.sync().map_err(|error| error.to_string())? {
            return Err("evidence directory sync unavailable".into());
        }
        Ok(())
    }
}

fn native(mut app: Application, demo: bool) -> Result<()> {
    let sdl = sdl3::init().map_err(|error| error.to_string())?;
    let video = sdl.video().map_err(|error| error.to_string())?;
    let mut window = video
        .window("Tilewright offline prototype", 960, 640)
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| error.to_string())?;
    let (width, height) = window.size_in_pixels();
    app.view.camera.width = width;
    app.view.camera.height = height;
    app.rebuild()?;
    let mut renderer = WgpuRenderer::new(TargetDescriptor {
        kind: TargetKind::Window,
        width,
        height,
        backend: BackendPreference::Automatic,
    })
    .map_err(|error| error.to_string())?;
    println!("native adapter={:?}", renderer.adapter());
    let mut events = sdl.event_pump().map_err(|error| error.to_string())?;
    let mut demo_step = 0;
    let mut next_demo = Instant::now() + Duration::from_millis(800);
    let mut dirty = true;
    let mut phase = None;
    let mut minimized = false;
    'running: loop {
        for event in events.poll_iter() {
            if matches!(
                event,
                Event::Quit { .. }
                    | Event::Window {
                        win_event: WindowEvent::CloseRequested,
                        ..
                    }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Q),
                        ..
                    }
            ) {
                break 'running;
            }
            let result = native_event(&mut app, &mut renderer, &window, &event, &mut minimized);
            dirty = true;
            if let Err(error) = result {
                eprintln!("{}", terminal_field(&error));
            }
        }
        if demo && !minimized && Instant::now() >= next_demo {
            if app.demo_step(demo_step)? {
                break 'running;
            }
            phase = if (demo_step == 5 && app.session.catalog().editing().fields.is_empty())
                || (demo_step == 6 && app.session.catalog().editing().terrain.is_empty())
            {
                None
            } else {
                [
                    "before", "pending", "after", "undo", "redo", "field", "terrain",
                ]
                .get(demo_step)
                .copied()
            };
            demo_step += 1;
            next_demo = Instant::now() + Duration::from_millis(2500);
            dirty = true;
        }
        if dirty && !minimized {
            if native_present(&mut app, &mut renderer, &mut window)?
                && let Some(name) = phase.take()
            {
                println!("native-phase={name}");
            }
            dirty = false;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    pollster::block_on(renderer.finish()).map_err(|error| error.to_string())?;
    println!("native window closed cleanly");
    Ok(())
}

fn native_event(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &sdl3::video::Window,
    event: &Event,
    minimized: &mut bool,
) -> Result<()> {
    match event {
        Event::Window {
            win_event: WindowEvent::Minimized,
            ..
        } => {
            *minimized = true;
            Ok(())
        }
        Event::Window {
            win_event:
                WindowEvent::Restored | WindowEvent::PixelSizeChanged(..) | WindowEvent::Resized(..),
            ..
        } => {
            let size = window.size_in_pixels();
            *minimized = size.0 == 0 || size.1 == 0;
            if *minimized {
                return Ok(());
            }
            renderer
                .resize(size.0, size.1)
                .map_err(|error| error.to_string())?;
            app.view.camera.width = size.0;
            app.view.camera.height = size.1;
            app.rebuild()
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } if !*minimized => {
            let (px, py) =
                crate::viewport::physical_pixel([*x, *y], window.size(), window.size_in_pixels())
                    .ok_or("pointer outside physical viewport")?;
            app.click(px, py)?;
            if app.terrain { app.paint() } else { Ok(()) }
        }
        Event::KeyDown {
            keycode: Some(key),
            repeat: false,
            ..
        } => native_key(app, *key),
        _ => Ok(()),
    }
}

fn native_key(app: &mut Application, key: Keycode) -> Result<()> {
    match key {
        Keycode::Return | Keycode::KpEnter => app.action(Action::Accept),
        Keycode::Escape => app.action(Action::Cancel),
        Keycode::Z => app.action(Action::Undo),
        Keycode::Y => app.action(Action::Redo),
        Keycode::R => app.action(Action::Reopen),
        Keycode::F => app.field(),
        Keycode::Left => app.nudge([-1, 0]),
        Keycode::Right => app.nudge([1, 0]),
        Keycode::Up => app.nudge([0, -1]),
        Keycode::Down => app.nudge([0, 1]),
        Keycode::T => {
            app.terrain = !app.terrain;
            println!("terrain mode={}", app.terrain);
            Ok(())
        }
        Keycode::W | Keycode::A | Keycode::S | Keycode::D => {
            let change = match key {
                Keycode::W => [0.0, -1.0],
                Keycode::A => [-1.0, 0.0],
                Keycode::S => [0.0, 1.0],
                _ => [1.0, 0.0],
            };
            app.view.camera.center[0] += change[0];
            app.view.camera.center[1] += change[1];
            app.rebuild()
        }
        Keycode::Plus | Keycode::Equals | Keycode::Minus => {
            let factor = if key == Keycode::Minus { 0.8 } else { 1.25 };
            app.view.camera.zoom = (app.view.camera.zoom * factor).clamp(0.25, 8.0);
            app.rebuild()
        }
        _ => Ok(()),
    }
}

fn native_present(
    app: &mut Application,
    renderer: &mut WgpuRenderer,
    window: &mut sdl3::video::Window,
) -> Result<bool> {
    let title = if let Some(reason) = &app.blocked {
        format!("Tilewright BLOCKED: {reason} | Q quit")
    } else {
        format!(
            "Tilewright {} | {} | {}",
            app.session.catalog().presentation(),
            app.session.status(),
            HELP
        )
    };
    window
        .set_title(&terminal_field(&title))
        .map_err(|error| error.to_string())?;
    match tilewright_render_sdl3::present_existing(
        renderer,
        window,
        RenderRequest {
            scene: &app.prepared.scene,
            resources: app.prepared.resources.clone(),
        },
    ) {
        Ok(proof) => {
            println!(
                "native presented {}x{} backend={} adapter={}",
                proof.width,
                proof.height,
                terminal_field(&proof.backend),
                terminal_field(&proof.adapter_name)
            );
            Ok(true)
        }
        Err(error) => {
            let reason = error.to_string();
            app.blocked = Some(reason.clone());
            window
                .set_title(&format!(
                    "Tilewright BLOCKED: {} | Q quit",
                    terminal_field(&reason)
                ))
                .map_err(|error| error.to_string())?;
            eprintln!("{}", terminal_field(&reason));
            Ok(false)
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "tests/prototype.rs"]
mod tests;
