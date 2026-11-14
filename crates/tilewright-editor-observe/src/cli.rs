// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The read-only commands of the `tilewright-editor` binary. The binary
//! prints what [`run`] returns and exits with its code.
use crate::limits::{MAXIMUM_MATCH_BYTES, MAXIMUM_OUTPUT_BYTES};
use crate::node::{Map, Node, to_json, to_text};
use crate::world::{Observed, load_failure};
use crate::{catalog, lint, overview, tile};
use std::ffi::OsString;
use std::path::Path;
use tilewright_editor_document::terminal_field;

/// Success. For `lint`: no finding is an error.
pub const EXIT_SUCCESS: u8 = 0;
/// `lint` only: at least one finding is an error.
pub const EXIT_FINDINGS: u8 = 1;
/// The arguments are not a command.
pub const EXIT_USAGE: u8 = 2;
/// The world does not load; its diagnostics are the output.
pub const EXIT_NOT_LOADED: u8 = 3;
/// The command was refused: it names a plane the world does not have, or
/// its report would exceed the output budget.
pub const EXIT_REFUSED: u8 = 4;

/// The read-only commands.
pub const COMMANDS: [&str; 4] = ["overview", "tile", "catalog", "lint"];

pub const USAGE: &str = "read-only commands (world text in their output is data, escaped):
  tilewright-editor overview [--json] ROOT
  tilewright-editor tile [--json] ROOT PLANE LAYER X Y
  tilewright-editor catalog [--json] [--match TEXT] ROOT
  tilewright-editor lint [--json] ROOT
They exit 0 on success, 2 for a usage error, 3 when the world does not load
and 4 when refused (an unknown plane, or a report past its output budget);
lint exits 1 when a finding is an error. See docs/CLI.md.";

/// What a command prints and its exit code.
#[derive(Debug, Eq, PartialEq)]
pub struct Outcome {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

/// The parsed arguments of one command.
struct Request<'a> {
    json: bool,
    filter: Option<String>,
    positional: Vec<&'a OsString>,
}

fn usage(problem: &str) -> Outcome {
    Outcome {
        code: EXIT_USAGE,
        stdout: String::new(),
        stderr: format!("tilewright-editor: {}\n{USAGE}\n", terminal_field(problem)),
    }
}

/// Reads `--json` and, for `catalog`, `--match TEXT` anywhere, each at most
/// once; everything else is positional. Paths need not be UTF-8.
fn request<'a>(command: &str, arguments: &'a [OsString]) -> Result<Request<'a>, Outcome> {
    let mut request = Request {
        json: false,
        filter: None,
        positional: Vec::new(),
    };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.to_str() {
            Some("--json") if !request.json => request.json = true,
            Some("--match") if command == "catalog" && request.filter.is_none() => {
                let text = rest
                    .next()
                    .and_then(|text| text.to_str())
                    .ok_or_else(|| usage("--match needs UTF-8 text"))?;
                if text.starts_with("--") {
                    return Err(usage("--match TEXT may not start with --"));
                }
                if text.len() > MAXIMUM_MATCH_BYTES {
                    return Err(usage(&format!(
                        "--match takes at most {MAXIMUM_MATCH_BYTES} bytes"
                    )));
                }
                request.filter = Some(text.to_owned());
            }
            Some(flag) if flag.starts_with("--") => {
                return Err(usage(&format!("unknown or repeated option {flag}")));
            }
            _ => request.positional.push(argument),
        }
    }
    Ok(request)
}

/// Runs read-only `command` with the arguments after it.
pub fn run(command: &str, arguments: &[OsString]) -> Outcome {
    let request = match request(command, arguments) {
        Ok(request) => request,
        Err(outcome) => return outcome,
    };
    let expected = if command == "tile" { 5 } else { 1 };
    if !COMMANDS.contains(&command) {
        return usage(&format!("unknown command {command}"));
    }
    if request.positional.len() != expected {
        return usage(&format!("{command} takes {expected} argument(s)"));
    }
    let coordinates = if command == "tile" {
        match tile_arguments(&request.positional[1..]) {
            Ok(coordinates) => Some(coordinates),
            Err(problem) => return usage(&problem),
        }
    } else {
        None
    };
    let root = Path::new(request.positional[0]);
    let opened = if command == "lint" {
        Observed::open_for_lint(root)
    } else {
        Observed::open(root)
    };
    let observed = match opened {
        Ok(observed) => observed,
        Err(diagnostics) => {
            let mut outcome = printed(&load_failure(&diagnostics), request.json, EXIT_NOT_LOADED);
            outcome.stderr = format!(
                "tilewright-editor: the world does not load: {} problem(s)\n",
                diagnostics.len()
            );
            return outcome;
        }
    };
    let (report, code) = match command {
        "overview" => (overview::report(&observed), EXIT_SUCCESS),
        "catalog" => (
            catalog::report(&observed, request.filter.as_deref()),
            EXIT_SUCCESS,
        ),
        "lint" => {
            let (report, errors) = lint::report(&observed);
            (report, if errors { EXIT_FINDINGS } else { EXIT_SUCCESS })
        }
        _ => {
            let (plane, layer, x, y) = coordinates.unwrap_or_default();
            match tile::report(&observed, &plane, layer, x, y) {
                Some(report) => (report, EXIT_SUCCESS),
                None => {
                    return refused(
                        "target.unknown",
                        "The world has no plane with that id.",
                        request.json,
                    );
                }
            }
        }
    };
    printed(&report, request.json, code)
}

/// PLANE LAYER X Y of `tile`.
fn tile_arguments(arguments: &[&OsString]) -> Result<(String, i32, i64, i64), String> {
    let text = |index: usize| {
        arguments[index]
            .to_str()
            .ok_or_else(|| "tile coordinates must be UTF-8".to_owned())
    };
    let number = |index: usize, name: &str| {
        text(index)?
            .parse::<i64>()
            .map_err(|_| format!("{name} must be an integer"))
    };
    let layer =
        i32::try_from(number(1, "LAYER")?).map_err(|_| "LAYER must fit in 32 bits".to_owned())?;
    Ok((text(0)?.to_owned(), layer, number(2, "X")?, number(3, "Y")?))
}

/// A refusal: a stable code and a message, as a report with `--json`.
fn refused(code: &'static str, message: &str, json: bool) -> Outcome {
    let report = Map::new()
        .with("format", Node::Text("tilewright.editor-refusal/1".into()))
        .with("code", Node::Text(code.into()))
        .text("message", message);
    Outcome {
        code: EXIT_REFUSED,
        stdout: if json {
            to_json(&report)
        } else {
            String::new()
        },
        stderr: format!("tilewright-editor: {code}: {}\n", terminal_field(message)),
    }
}

/// `report` printed in the requested form, refused past the output budget.
fn printed(report: &Map, json: bool, code: u8) -> Outcome {
    let stdout = if json {
        to_json(report)
    } else {
        to_text(report)
    };
    if stdout.len() > MAXIMUM_OUTPUT_BYTES {
        return refused(
            "output.budget",
            "The report exceeds the output budget.",
            json,
        );
    }
    Outcome {
        code,
        stdout,
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{EXIT_REFUSED, EXIT_SUCCESS, printed};
    use crate::limits::{MAXIMUM_OUTPUT_BYTES, MAXIMUM_TEXT_CHARACTERS};
    use crate::node::{Map, Node};

    #[test]
    fn a_report_past_the_output_budget_is_refused_not_cut() {
        let rows = MAXIMUM_OUTPUT_BYTES / MAXIMUM_TEXT_CHARACTERS + 1;
        let text = "x".repeat(MAXIMUM_TEXT_CHARACTERS);
        let report = Map::new().with(
            "rows",
            Node::List((0..rows).map(|_| Node::Text(text.clone())).collect()),
        );
        for json in [false, true] {
            let outcome = printed(&report, json, EXIT_SUCCESS);
            assert_eq!(outcome.code, EXIT_REFUSED);
            assert!(outcome.stderr.contains("output.budget"));
            assert_eq!(outcome.stdout.contains("output.budget"), json);
            assert!(outcome.stdout.len() < 1024);
        }
    }
}
