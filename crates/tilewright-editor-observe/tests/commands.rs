// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The read-only commands on the committed fixtures, against golden
//! output, and their usage and exit codes.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, Outcome};
use tilewright_editor_testkit::fixture_world;

fn run(command: &str, arguments: &[&str]) -> Outcome {
    let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
    cli::run(command, &arguments)
}

fn root(name: &str) -> String {
    fixture_world(name).to_str().unwrap().to_owned()
}

/// The World revision of the fixture: CRLF checkouts change it, so goldens
/// hold a placeholder instead.
fn revision(name: &str) -> String {
    let document = tilewright_editor_document::WorldDocument::open(&fixture_world(name)).unwrap();
    tilewright_editor_document::revision_hex(&document.revision())
}

/// Compares `actual` with `tests/golden/<file>`, line endings aside.
/// `TILEWRIGHT_UPDATE_GOLDEN=1` rewrites the file instead.
fn golden(file: &str, fixture: &str, actual: &str) {
    let actual = actual.replace(&revision(fixture), "<revision>");
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("TILEWRIGHT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "golden {file} differs");
}

/// Runs `command` in both forms, checks each against its golden, that the
/// JSON parses strictly and that it exits with `code`.
fn check(command: &str, fixture: &str, extra: &[&str], name: &str, code: u8) {
    let root = root(fixture);
    let mut arguments = vec!["--json", root.as_str()];
    arguments.extend_from_slice(extra);
    let json = run(command, &arguments);
    assert_eq!(json.code, code, "{name} json: {json:?}");
    assert!(json.stderr.is_empty(), "{name}: {}", json.stderr);
    serde_json::from_str::<serde_json::Value>(&json.stdout).expect("strict JSON");
    golden(&format!("{name}.json"), fixture, &json.stdout);
    let text = run(command, &arguments[1..]);
    assert_eq!(text.code, code, "{name} text: {text:?}");
    golden(&format!("{name}.txt"), fixture, &text.stdout);
}

#[test]
fn overview_of_the_fixtures() {
    check("overview", "prototype-neutral", &[], "overview-neutral", 0);
    check("overview", "meadow", &[], "overview-meadow", 0);
}

#[test]
fn tiles_of_the_neutral_fixture() {
    // gate_east, whose portal names the anchor arrival.
    check(
        "tile",
        "prototype-neutral",
        &["main", "0", "7", "0"],
        "tile-gate",
        0,
    );
    // marker_1, with per-uid and per-prototype grants.
    check(
        "tile",
        "prototype-neutral",
        &["main", "0", "3", "2"],
        "tile-marker",
        0,
    );
    // The anchor arrival, which gate_east leads to.
    check(
        "tile",
        "prototype-neutral",
        &["main", "0", "7", "3"],
        "tile-anchor",
        0,
    );
    // Left of the view: gate_west's target, in chunk -1_0.
    check(
        "tile",
        "prototype-neutral",
        &["main", "0", "-2", "2"],
        "tile-outside",
        0,
    );
    // No chunk at all.
    check(
        "tile",
        "meadow",
        &["meadow", "3", "100", "-100"],
        "tile-no-chunk",
        0,
    );
}

#[test]
fn catalog_of_the_fixtures() {
    check("catalog", "prototype-neutral", &[], "catalog-neutral", 0);
    check(
        "catalog",
        "prototype-neutral",
        &["--match", "GRASS_EDGE_1"],
        "catalog-match",
        0,
    );
    check("catalog", "meadow", &[], "catalog-meadow", 0);
}

#[test]
fn lint_of_the_fixtures() {
    check("lint", "prototype-neutral", &[], "lint-neutral", 0);
    // No catalog: the editing entry point cannot open it.
    check("lint", "meadow", &[], "lint-meadow", cli::EXIT_FINDINGS);
}

#[test]
fn usage_errors_exit_2_and_print_nothing_on_stdout() {
    let root = root("prototype-neutral");
    for (command, arguments) in [
        ("overview", vec![]),
        ("overview", vec![root.as_str(), root.as_str()]),
        ("overview", vec!["--json", "--json", root.as_str()]),
        ("overview", vec!["--match", "a", root.as_str()]),
        ("lint", vec!["--verbose", root.as_str()]),
        ("tile", vec![root.as_str(), "main", "0", "1"]),
        ("tile", vec![root.as_str(), "main", "zero", "1", "2"]),
        ("tile", vec![root.as_str(), "main", "4294967296", "1", "2"]),
        (
            "tile",
            vec![root.as_str(), "main", "0", "1", "9223372036854775808"],
        ),
        ("catalog", vec![root.as_str(), "--match"]),
        (
            "catalog",
            vec!["--match", "a", "--match", "b", root.as_str()],
        ),
        ("catalog", vec!["--match", &"x".repeat(65), root.as_str()]),
        ("render", vec![root.as_str()]),
    ] {
        let outcome = run(command, &arguments);
        assert_eq!(outcome.code, cli::EXIT_USAGE, "{command} {arguments:?}");
        assert!(outcome.stdout.is_empty(), "{command} {arguments:?}");
        assert!(outcome.stderr.contains("usage") || outcome.stderr.contains("read-only commands"));
    }
    // Flags may come anywhere; the --match bound is in bytes.
    let root = root.as_str();
    assert_eq!(run("overview", &[root, "--json"]).code, 0);
    assert_eq!(run("catalog", &[root, "--match", &"x".repeat(64)]).code, 0);
}

#[test]
fn an_unknown_plane_is_refused_with_exit_4() {
    let root = root("prototype-neutral");
    let text = run("tile", &[&root, "nowhere", "0", "0", "0"]);
    assert_eq!(text.code, cli::EXIT_REFUSED);
    assert!(text.stdout.is_empty());
    assert!(text.stderr.contains("target.unknown"), "{}", text.stderr);
    let json = run("tile", &["--json", &root, "nowhere", "0", "0", "0"]);
    assert_eq!(json.code, cli::EXIT_REFUSED);
    let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
    assert_eq!(value["format"], "tilewright.editor-refusal/1");
    assert_eq!(value["code"], "target.unknown");
}

#[test]
fn a_world_that_does_not_load_exits_3_with_its_diagnostics() {
    // The profile directory has no world.jsonc.
    let root = fixture_world("meadow").join("profile");
    let root = root.to_str().unwrap();
    for command in cli::COMMANDS {
        let arguments: &[&str] = if command == "tile" {
            &["--json", root, "a", "0", "0", "0"]
        } else {
            &["--json", root]
        };
        let json = run(command, arguments);
        assert_eq!(json.code, cli::EXIT_NOT_LOADED, "{command}");
        assert!(json.stderr.contains("does not load"), "{}", json.stderr);
        let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
        assert_eq!(value["format"], "tilewright.editor-load-failure/1");
        let diagnostics = value["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics.iter().any(|item| item["path"] == "world.jsonc"),
            "{value}"
        );
        let text = run(command, &arguments[1..]);
        assert_eq!(text.code, cli::EXIT_NOT_LOADED);
        assert!(text.stdout.contains("path: world.jsonc"), "{}", text.stdout);
    }
}
