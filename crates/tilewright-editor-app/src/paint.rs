// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `tilewright-editor paint`: one terrain brush stroke from the command
//! line, through the same brush and transactions as the editor.

use std::{ffi::OsString, fmt::Write as _, path::Path};
use tilewright_editor_commands::{
    brush::{Brush, MAXIMUM_RADIUS, Shape, Stroke},
    preview, transaction,
};
use tilewright_editor_document::{
    WorldDocument, terminal_field, terminal_safe, terrain::PlaneTerrain,
};

pub const USAGE: &str = "paint ROOT --plane PLANE --terrain TERRAIN [--layer LAYER] [--brush square:R|circle:R] [--save INVERSE [--unchecked-access]] X,Y...

paint strokes the brush through the tiles X,Y in order, painting TERRAIN of
the profile's transitions on LAYER (default 0) of PLANE. Without --save it
prints the transaction and the files it would change, writing nothing. With
--save it checks the change as an editing session checks each save, refusing
one too large to undo, writes the transaction's inverse to INVERSE, which
must not exist, and then publishes the change; applying INVERSE with
`tilewright tx apply` undoes it. Where publishing cannot check everything,
on macOS and Windows and on Linux for a tree on a file system that hides its
access control lists, saving also needs --unchecked-access, accepting what
it says is not checked.";

struct Request {
    root: OsString,
    plane: String,
    terrain: String,
    layer: i32,
    brush: Brush,
    inverse: Option<OsString>,
    unchecked_access: bool,
    points: Vec<[i64; 2]>,
}

fn text(value: &OsString, what: &str) -> Result<String, String> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{what} is not UTF-8"))
}

fn brush(value: &str) -> Result<Brush, String> {
    let (shape, radius) = value
        .split_once(':')
        .ok_or_else(|| format!("brush {value:?} is not SHAPE:RADIUS"))?;
    let shape = match shape {
        "square" => Shape::Square,
        "circle" => Shape::Circle,
        _ => return Err(format!("brush shape {shape:?} is not square or circle")),
    };
    let radius = radius
        .parse::<u16>()
        .ok()
        .filter(|radius| *radius <= MAXIMUM_RADIUS)
        .ok_or_else(|| format!("brush radius {radius:?} is not 0 to {MAXIMUM_RADIUS}"))?;
    Ok(Brush { shape, radius })
}

fn point(value: &str) -> Result<[i64; 2], String> {
    let (x, y) = value
        .split_once(',')
        .ok_or_else(|| format!("tile {value:?} is not X,Y"))?;
    match (x.parse(), y.parse()) {
        (Ok(x), Ok(y)) => Ok([x, y]),
        _ => Err(format!("tile {value:?} is not two integers")),
    }
}

fn request(arguments: &[OsString]) -> Result<Request, String> {
    let (root, mut rest) = arguments.split_first().ok_or_else(|| USAGE.to_owned())?;
    let (mut plane, mut terrain, mut layer, mut inverse) = (None, None, 0, None);
    let mut unchecked_access = false;
    let mut shape = Brush {
        shape: Shape::Square,
        radius: 0,
    };
    let mut points = Vec::new();
    while let Some((argument, after)) = rest.split_first() {
        let argument = text(argument, "an argument")?;
        if !argument.starts_with("--") {
            points.push(point(&argument)?);
            rest = after;
            continue;
        }
        if argument == "--unchecked-access" {
            unchecked_access = true;
            rest = after;
            continue;
        }
        let (given, after) = after
            .split_first()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        match argument.as_str() {
            "--plane" => plane = Some(text(given, "the plane")?),
            "--terrain" => terrain = Some(text(given, "the terrain")?),
            "--layer" => {
                layer = text(given, "the layer")?
                    .parse()
                    .map_err(|_| "the layer is not a 32-bit integer".to_owned())?;
            }
            "--brush" => shape = brush(&text(given, "the brush")?)?,
            "--save" => inverse = Some(given.clone()),
            other => return Err(format!("unknown option {other}")),
        }
        rest = after;
    }
    if points.is_empty() {
        return Err("paint needs at least one tile X,Y".to_owned());
    }
    Ok(Request {
        root: root.clone(),
        plane: plane.ok_or("paint needs --plane")?,
        terrain: terrain.ok_or("paint needs --terrain")?,
        layer,
        brush: shape,
        inverse,
        unchecked_access,
        points,
    })
}

pub fn run(arguments: &[OsString]) -> Result<(), String> {
    let request = request(arguments)?;
    let report = execute(&request, &mut |notice| {
        eprintln!("tilewright-editor: {}", terminal_field(notice));
    })?;
    print!("{}", terminal_safe(&report));
    Ok(())
}

/// The inverse is a recovery record, so it is kept outside the world's
/// source tree: under `planes/` or `defs/` the loader would refuse it as a
/// stray file, and elsewhere in the tree it would stay there unnoticed. On
/// Unix the folder and the folders above it are also compared with the root
/// by identity, so another name for the root itself is caught: another
/// spelling on a file system that ignores case, or a bind mount of the root.
/// A bind mount of a folder inside the tree is not caught: an inverse
/// written through one under `planes/` or `defs/` makes publishing refuse
/// the stray file, and one elsewhere stays in the tree unnoticed
/// (`docs/MUTABLE_PATHS.md`). Windows and macOS resolve names to how the
/// disk stores them, so there the canonical names compare.
fn outside(root: &Path, inverse: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve the world's root: {error}"))?;
    let folder = match inverse.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let folder = folder
        .canonicalize()
        .map_err(|error| format!("cannot resolve the inverse's folder: {error}"))?;
    let inside = folder.starts_with(&root) || same_folder_above(&root, &folder)?;
    if inside {
        return Err("the inverse must be written outside the world's source tree".to_owned());
    }
    Ok(())
}

/// Whether `folder` or a folder above it is `root` itself, by identity.
#[cfg(unix)]
fn same_folder_above(root: &Path, folder: &Path) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt as _;
    let unreadable =
        |error: std::io::Error| format!("cannot inspect the inverse's folder: {error}");
    let target = root.metadata().map_err(unreadable)?;
    for ancestor in folder.ancestors() {
        let found = ancestor.metadata().map_err(unreadable)?;
        if found.dev() == target.dev() && found.ino() == target.ino() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Elsewhere canonical names already show the stored spelling.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn same_folder_above(_root: &Path, _folder: &Path) -> Result<bool, String> {
    Ok(false)
}

/// Carries out `request` and returns what to print; `notice` receives
/// warnings as they arise.
fn execute(request: &Request, notice: &mut dyn FnMut(&str)) -> Result<String, String> {
    let root = Path::new(&request.root);
    let document = WorldDocument::open(root).map_err(|error| error.to_string())?;
    let mut unsaved = PlaneTerrain::new(document.world().clone(), &request.plane)
        .map_err(|error| error.to_string())?;
    let mut stroke = Stroke::begin(&unsaved, request.layer, &request.terrain, request.brush)
        .map_err(|error| error.to_string())?;
    for point in &request.points {
        stroke
            .stamp(&mut unsaved, *point)
            .map_err(|error| error.to_string())?;
    }
    let Some(operation) = stroke.operation().map_err(|error| error.to_string())? else {
        return Ok("the stroke changes no tile\n".to_owned());
    };
    let text = transaction(&stroke.title(), &[operation], &[stroke.record()]);
    let planned = preview(&document, "paint.jsonc", &text).map_err(|error| error.to_string())?;
    let mut summary = String::new();
    for file in planned.changed() {
        let mark = match (&file.before, &file.after) {
            (None, _) => 'A',
            (_, None) => 'D',
            _ => 'M',
        };
        let _ = writeln!(summary, "{mark} {}", file.path);
    }
    let Some(path) = &request.inverse else {
        return Ok(format!("{text}{summary}"));
    };
    // The same check an editing session makes of each save, before it asks
    // to save without every check.
    planned
        .check_undo()
        .map_err(|reason| format!("the stroke is too large to save as one step: {reason}"))?;
    let unchecked = planned.unchecked_access().map_err(|problems| {
        problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    if let Some(unchecked) = unchecked {
        if !request.unchecked_access {
            return Err(format!(
                "saving here does not check everything: {unchecked}; pass --unchecked-access to accept that"
            ));
        }
        notice(&format!("saving without these checks: {unchecked}"));
    }
    // Checked just before the write, so the folder is what it names now.
    outside(root, Path::new(path))?;
    let durable = planned
        .record_inverse(Path::new(path))
        .map_err(|error| format!("cannot write the inverse: {error}"))?;
    if !durable {
        notice("the inverse's name may not be durable yet");
    }
    match planned.publish(request.unchecked_access) {
        Ok(_) => {}
        // Every file was published; what went wrong after is a warning.
        Err(error) if error.applied => notice(&error.to_string()),
        Err(error) => return Err(format!("saving failed: {error}")),
    }
    Ok(format!("{}\n{summary}", stroke.title()))
}

#[cfg(test)]
mod tests {
    use super::{Request, execute, request};
    use std::{ffi::OsString, path::Path};
    use tilewright_edit::{Options, PublishOptions, Transaction};
    use tilewright_editor_commands::{json_string, transaction};
    use tilewright_editor_document::WorldDocument;
    use tilewright_editor_testkit::{MEADOW_FILES, fixture_world};

    /// Copies are made in a temporary directory no one else changes, so the
    /// tests accept what the toolkit cannot check on some platforms.
    const ACCEPTED: PublishOptions = PublishOptions {
        unchecked_access: true,
    };

    fn arguments(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn apply(root: &Path, text: &str) {
        let parsed = Transaction::parse("apply.jsonc", text).unwrap();
        tilewright_edit::plan(root, &parsed, &Options::default())
            .unwrap()
            .publish_with(&ACCEPTED)
            .unwrap();
    }

    fn meadow_copy() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let operations: Vec<String> = MEADOW_FILES
            .iter()
            .map(|(path, text)| {
                format!(
                    "{{\"op\": \"document.create\", \"path\": {}, \"text\": {}}}",
                    json_string(path),
                    json_string(text)
                )
            })
            .collect();
        apply(
            directory.path(),
            &transaction("Copy the meadow", &operations, &[]),
        );
        directory
    }

    fn revision(root: &Path) -> [u8; 32] {
        WorldDocument::open(root).unwrap().revision()
    }

    fn painting(root: &Path, extra: &[&str]) -> Request {
        let mut words = vec![
            root.to_str().unwrap(),
            "--plane",
            "meadow",
            "--terrain",
            "sand",
            "--brush",
            "circle:1",
            "30,3",
            "33,3",
        ];
        words.extend_from_slice(extra);
        request(&arguments(&words)).unwrap()
    }

    #[test]
    fn arguments_are_checked() {
        for (words, problem) in [
            (&["root"][..], "at least one tile"),
            (&["root", "1,1"][..], "--plane"),
            (&["root", "--plane", "p", "1,1"][..], "--terrain"),
            (&["root", "--plane"][..], "needs a value"),
            (&["root", "--colour", "red", "1,1"][..], "unknown option"),
            (&["root", "--brush", "hex:1", "1,1"][..], "square or circle"),
            (&["root", "--brush", "circle:33", "1,1"][..], "0 to 32"),
            (&["root", "--layer", "deep", "1,1"][..], "32-bit"),
            (&["root", "one,two"][..], "two integers"),
        ] {
            let Err(error) = request(&arguments(words)) else {
                panic!("{words:?} was accepted");
            };
            assert!(error.contains(problem), "{words:?}: {error}");
        }
    }

    #[test]
    fn a_dry_run_prints_the_transaction_and_writes_nothing() {
        let root = fixture_world("meadow");
        let opened = revision(&root);
        let report = execute(&painting(&root, &[]), &mut |_| {}).unwrap();
        assert!(report.contains("\"op\": \"terrain.set\""));
        assert!(
            report.ends_with("A planes/meadow/layers/0/1_0.jsonc\n"),
            "{report}"
        );
        assert_eq!(revision(&root), opened);
    }

    #[test]
    fn saving_keeps_the_inverse_first_and_the_inverse_undoes_it() {
        let root = meadow_copy();
        let outside = tempfile::tempdir().unwrap();
        let inverse = outside.path().join("undo.jsonc");
        let opened = revision(root.path());
        let request = painting(
            root.path(),
            &["--save", inverse.to_str().unwrap(), "--unchecked-access"],
        );
        let report = execute(&request, &mut |_| {}).unwrap();
        assert!(report.starts_with("Paint sand on"), "{report}");
        let painted = revision(root.path());
        assert_ne!(painted, opened);
        // Another stroke saved to the same INVERSE is refused before
        // anything is written.
        let mut other = painting(
            root.path(),
            &["--save", inverse.to_str().unwrap(), "--unchecked-access"],
        );
        other.terrain = "water".to_owned();
        let again = execute(&other, &mut |_| {}).unwrap_err();
        assert!(again.contains("cannot write the inverse"), "{again}");
        assert_eq!(revision(root.path()), painted);
        // Read through the toolkit's confined reader, as editor code does.
        let bytes = tilewright_source::SourceRoot::new(outside.path())
            .read("undo.jsonc", 64 << 20)
            .unwrap()
            .unwrap();
        apply(root.path(), &String::from_utf8(bytes).unwrap());
        assert_eq!(revision(root.path()), opened);
    }

    #[test]
    fn saving_without_consent_writes_nothing_where_saving_cannot_check_everything() {
        let root = meadow_copy();
        let outside = tempfile::tempdir().unwrap();
        let inverse = outside.path().join("undo.jsonc");
        let opened = revision(root.path());
        let request = painting(root.path(), &["--save", inverse.to_str().unwrap()]);
        match execute(&request, &mut |_| {}) {
            // Linux, on a file system that shows its access control lists.
            Ok(report) => {
                assert!(tilewright_edit::UNCHECKED_ACCESS.is_none(), "{report}");
                assert!(inverse.exists());
            }
            Err(error) => {
                assert!(error.contains("--unchecked-access"), "{error}");
                assert!(!inverse.exists());
                assert_eq!(revision(root.path()), opened);
            }
        }
    }

    #[test]
    fn the_inverse_is_never_written_inside_the_world() {
        let root = meadow_copy();
        let opened = revision(root.path());
        for inside in ["undo.jsonc", "planes/meadow/undo.jsonc"] {
            let path = root.path().join(inside);
            let request = painting(
                root.path(),
                &["--save", path.to_str().unwrap(), "--unchecked-access"],
            );
            let error = execute(&request, &mut |_| {}).unwrap_err();
            assert!(error.contains("outside the world"), "{error}");
            assert!(!path.exists());
        }
        assert_eq!(revision(root.path()), opened);
    }
}
