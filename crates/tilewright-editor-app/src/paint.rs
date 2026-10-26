// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `tilewright-editor paint`: one terrain brush stroke from the command
//! line, through the same brush and transactions as the editor.

use std::{ffi::OsString, fmt::Write as _, path::Path};
use tilewright_editor_commands::{
    brush::{Brush, MAXIMUM_RADIUS, Shape, Stroke},
    preview, transaction,
};
use tilewright_editor_document::{WorldDocument, terrain::PlaneTerrain};

pub const USAGE: &str = "paint ROOT --plane PLANE --terrain TERRAIN [--layer LAYER] [--brush square:R|circle:R] [--save INVERSE [--unchecked-access]] X,Y...

paint strokes the brush through the tiles X,Y in order, painting TERRAIN of
the profile's transitions on LAYER (default 0) of PLANE. Without --save it
prints the transaction and the files it would change, writing nothing. With
--save it writes the transaction's inverse to INVERSE, which must not exist,
and then publishes the change; applying INVERSE with `tilewright tx apply`
undoes it. Where publishing cannot check everything, on macOS and Windows
and on Linux for a tree on a file system that hides its access control
lists, saving also needs --unchecked-access, accepting what it says is not
checked.";

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
    let document =
        WorldDocument::open(Path::new(&request.root)).map_err(|error| error.to_string())?;
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
        println!("the stroke changes no tile");
        return Ok(());
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
    match &request.inverse {
        None => print!("{text}{summary}"),
        Some(path) => {
            let unchecked = planned.unchecked_access().map_err(|problems| {
                problems
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            if let Some(unchecked) = unchecked {
                if !request.unchecked_access {
                    return Err(format!(
                        "saving here does not check everything: {unchecked}; pass --unchecked-access to accept that"
                    ));
                }
                eprintln!("tilewright-editor: saving without these checks: {unchecked}");
            }
            let durable = planned
                .record_inverse(Path::new(path))
                .map_err(|error| format!("cannot write the inverse: {error}"))?;
            if !durable {
                eprintln!("tilewright-editor: the inverse's name may not be durable yet");
            }
            planned
                .publish(request.unchecked_access)
                .map_err(|error| format!("saving failed: {error}"))?;
            print!("{}\n{summary}", stroke.title());
        }
    }
    Ok(())
}
