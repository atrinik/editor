// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The read-only commands on generated worlds: hostile text, every bound,
//! lint findings, and a tree left exactly as it was.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, Outcome};
use tilewright_editor_observe::limits;
use tilewright_editor_testkit::fixture_world;

fn run(command: &str, arguments: &[&str]) -> Outcome {
    let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
    cli::run(command, &arguments)
}

/// Runs `command` with `--json` on `root` and returns the parsed document.
fn json_of(command: &str, root: &Path, extra: &[&str], code: u8) -> (Value, String) {
    let root = root.to_str().unwrap();
    let mut arguments = vec!["--json", root];
    arguments.extend_from_slice(extra);
    let outcome = run(command, &arguments);
    assert_eq!(outcome.code, code, "{command}: {}", outcome.stderr);
    let value = serde_json::from_str(&outcome.stdout).expect("strict JSON");
    (value, outcome.stdout)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A writable copy of the neutral fixture.
fn neutral_copy() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    (directory, root)
}

fn read_json(root: &Path, path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join(path)).unwrap()).unwrap()
}

fn store_json(root: &Path, path: &str, value: &Value) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// Applies `change` to the JSON document at `path`.
fn edit(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let mut value = read_json(root, path);
    change(&mut value);
    store_json(root, path, &value);
}

const MAIN_CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

/// Text that tries everything: quotes and backslashes, NUL, BEL, ANSI and
/// OSC sequences, DEL, C1 controls, line and paragraph separators,
/// bidirectional overrides and isolates, and a line break.
const HOSTILE: &str = "q\"b\\s\u{1}\u{7}\u{1b}[31mred\u{1b}]0;t\u{7}\u{7f}\u{85}\u{9b}\u{2028}\u{2029}\u{202e}\u{2066}\nfake: line";

fn assert_no_raw_controls(output: &str, newlines: bool) {
    for character in output.chars() {
        let allowed = newlines && character == '\n';
        assert!(
            allowed || !tilewright_editor_document::unprintable(character),
            "raw U+{:04X} in output",
            u32::from(character)
        );
    }
}

#[test]
fn hostile_world_text_is_escaped_in_both_forms_and_round_trips_in_json() {
    let (_directory, root) = neutral_copy();
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!(HOSTILE);
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        zones["zones"][0]["name"] = json!(HOSTILE);
    });
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"][0]["fields"]["label"] = json!(HOSTILE);
    });
    let (reference, _) = json_of("overview", &fixture_world("prototype-neutral"), &[], 0);
    for (command, extra) in [
        ("overview", &[][..]),
        ("tile", &["main", "0", "3", "2"][..]),
        ("catalog", &[][..]),
        ("lint", &[][..]),
    ] {
        let (value, printed) = json_of(command, &root, extra, 0);
        assert_no_raw_controls(&printed, true);
        // Lint findings are this editor's text; the others quote the world.
        assert_eq!(
            printed.contains(r"\u001b[31mred"),
            command != "lint",
            "{printed}"
        );
        let root_text = root.to_str().unwrap();
        let mut arguments = vec![root_text];
        arguments.extend_from_slice(extra);
        let text = run(command, &arguments);
        assert_eq!(text.code, 0);
        assert_no_raw_controls(&text.stdout, true);
        // The escaped line break cannot start a line of its own.
        assert!(
            !text
                .stdout
                .lines()
                .any(|line| line.trim_start().starts_with("fake:")),
            "{}",
            text.stdout
        );
        assert_eq!(
            text.stdout.contains(r"\u{1b}[31mred"),
            command != "lint",
            "{}",
            text.stdout
        );
        if command == "overview" {
            assert_eq!(value["planes"][0]["title"], HOSTILE);
            assert_eq!(
                value.as_object().unwrap().len(),
                reference.as_object().unwrap().len()
            );
            assert!(
                text.stdout.contains(
                    r#"title: q"b\s\u{1}\u{7}\u{1b}[31mred\u{1b}]0;t\u{7}\u{7f}\u{85}\u{9b}"#
                ),
                "{}",
                text.stdout
            );
        }
        if command == "tile" {
            let label = &value["objects"][0]["fields"][0];
            assert_eq!(label["name"], "label");
            assert_eq!(label["value"], HOSTILE);
            assert_eq!(value["zone"]["name"], HOSTILE);
            // Every value stays on its own line: the hostile text adds none.
            let plain = run(
                "tile",
                &[
                    fixture_world("prototype-neutral").to_str().unwrap(),
                    "main",
                    "0",
                    "3",
                    "2",
                ],
            );
            assert_eq!(text.stdout.lines().count(), plain.stdout.lines().count());
        }
    }
}

#[test]
fn hostile_text_in_diagnostics_is_escaped() {
    let (_directory, root) = neutral_copy();
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"][0]["uid"] = json!(HOSTILE);
    });
    let (value, printed) = json_of("overview", &root, &[], cli::EXIT_NOT_LOADED);
    assert_no_raw_controls(&printed, true);
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["message"].as_str().unwrap().contains(HOSTILE)),
        "{value}"
    );
    let text = run("lint", &[root.to_str().unwrap()]);
    assert_eq!(text.code, cli::EXIT_NOT_LOADED);
    assert_no_raw_controls(&text.stdout, true);
    assert!(
        !text
            .stdout
            .lines()
            .any(|line| line.trim_start().starts_with("fake:")),
        "{}",
        text.stdout
    );
}

/// The neutral fixture grown past every list bound.
fn crowded() -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = neutral_copy();
    let count = 300;
    // Twenty extra object fields, so one object has more than the bound.
    edit(&root, "profile/schema/object.jsonc", |schema| {
        for index in 0..20 {
            schema["fields"][format!("extra_{index}")] = json!({"type": "string"});
        }
    });
    let mut catalog = read_json(&root, "preview.jsonc");
    for index in 0..count {
        let proto = format!("crowd_{index:03}");
        store_json(
            &root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format": "tilewright.def/1", "type": "proto", "id": proto, "fields": {}}),
        );
        catalog["prototypes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"proto": proto, "resource": "grass", "anchor": "ground"}));
        catalog["editing"]["terrain"][0]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!(proto));
    }
    // Seventy objects on tile (0, 0), each with a portal to that tile.
    let mut objects = Vec::new();
    for index in 0..70 {
        let uid = format!("crowd_object_{index}");
        let mut fields = serde_json::Map::new();
        fields.insert("label".into(), json!(format!("Object {index}")));
        if index == 0 {
            for extra in 0..20 {
                fields.insert(format!("extra_{extra}"), json!("x".repeat(400)));
            }
        }
        objects.push(json!({
            "uid": uid, "at": [0, 0], "proto": "marker", "layer": "item",
            "fields": fields, "portal": {"layer": 0, "at": [0, 0]}
        }));
        catalog["editing"]["movable_uids"]
            .as_array_mut()
            .unwrap()
            .push(json!(uid));
    }
    store_json(&root, "preview.jsonc", &catalog);
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"].as_array_mut().unwrap().extend(objects);
    });
    // Ten anchors on tile (0, 0).
    edit(&root, "planes/main/anchors.jsonc", |anchors| {
        for index in 0..10 {
            anchors["anchors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": format!("crowd_anchor_{index}"), "layer": 0, "at": [0, 0]}));
        }
    });
    // Three hundred unused zones.
    edit(&root, "planes/main/zones.jsonc", |zones| {
        for index in 0..count {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": format!("crowd_zone_{index:03}")}));
        }
    });
    // Seventy more layers in plane main, and seventy more planes.
    let mut chunk = read_json(&root, "planes/main/layers/0/-1_0.jsonc");
    chunk["objects"] = json!([]);
    for layer in 1..=70 {
        chunk["layer"] = json!(layer);
        store_json(
            &root,
            &format!("planes/main/layers/{layer}/-1_0.jsonc"),
            &chunk,
        );
    }
    edit(&root, "world.jsonc", |world| {
        for index in 0..70 {
            let id = format!("z_plane_{index:02}");
            world["planes"].as_array_mut().unwrap().push(json!(id));
            store_json(
                &root,
                &format!("planes/{id}/plane.jsonc"),
                &json!({"format": "tilewright.plane/1", "id": id, "kind": "shared", "chunk_size": 32}),
            );
        }
    });
    (directory, root)
}

fn more(value: &Value, key: &str) -> u64 {
    value[format!("{key}_more")].as_u64().unwrap()
}

#[test]
fn every_list_is_bounded_and_counts_what_it_leaves_out() {
    let (_directory, root) = crowded();
    let (overview, printed) = json_of("overview", &root, &[], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(
        overview["planes"].as_array().unwrap().len(),
        limits::MAXIMUM_PLANES
    );
    assert_eq!(more(&overview, "planes"), 71 - 64);
    let main = &overview["planes"][0];
    assert_eq!(main["id"], "main");
    assert_eq!(more(main, "layers"), 71 - 64);
    let grants = &overview["catalog"]["grants"];
    assert_eq!(more(grants, "movable_uids"), 71 - 64);
    assert_eq!(more(&grants["terrain"][0], "values"), 302 - 64);

    let (tile, printed) = json_of("tile", &root, &["main", "0", "0", "0"], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(more(&tile, "objects"), 70 - 16);
    let first = &tile["objects"][0];
    assert_eq!(more(first, "fields"), 21 - 16);
    assert_eq!(first["fields"][1]["value_clipped"], true);
    assert_eq!(
        first["fields"][1]["value"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        limits::MAXIMUM_TEXT_CHARACTERS
    );
    assert_eq!(more(&tile, "anchors"), 10 - 8);
    assert_eq!(more(&tile, "arrivals"), 70 - 8);

    let (catalog, printed) = json_of("catalog", &root, &[], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(more(&catalog, "prototypes"), 323 - 256);
    assert_eq!(more(&catalog, "terrain_palette"), 302 - 256);
    assert_eq!(more(&catalog, "zone_palette"), 303 - 256);
    let (matched, _) = json_of("catalog", &root, &["--match", "CROWD_ZONE_29"], 0);
    // crowd_zone_290 to crowd_zone_299.
    assert_eq!(matched["zone_palette"].as_array().unwrap().len(), 10);
    assert_eq!(matched["prototypes"].as_array().unwrap().len(), 0);

    let (lint, printed) = json_of("lint", &root, &[], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(
        lint["findings"].as_array().unwrap().len(),
        limits::MAXIMUM_FINDINGS
    );
    // 70 own-tile portals, 300 unused zones, 11 unreferenced anchors and
    // the preview's own note.
    assert_eq!(lint["counts"]["warning"], 70);
    assert_eq!(lint["counts"]["info"], 300 + 11 + 1);
    assert_eq!(more(&lint, "findings"), 70 + 312 - 256);

    // The text forms are bounded the same way.
    for (command, extra) in [
        ("overview", &[][..]),
        ("tile", &["main", "0", "0", "0"][..]),
        ("catalog", &[][..]),
        ("lint", &[][..]),
    ] {
        let mut arguments = vec![root.to_str().unwrap()];
        arguments.extend_from_slice(extra);
        let text = run(command, &arguments);
        assert_eq!(text.code, 0);
        assert!(text.stdout.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    }
}

#[test]
fn lint_reports_portal_anchor_zone_and_catalog_findings() {
    let (_directory, root) = neutral_copy();
    edit(&root, MAIN_CHUNK, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        // gate_east to its own tile, gate_west to a tile without ground,
        // marker_2 to a chunk that does not exist.
        objects[2]["portal"] = json!({"layer": 0, "at": [7, 0]});
        objects[3]["portal"] = json!({"layer": 0, "at": [-2, 5]});
        objects[1]["portal"] = json!({"layer": 0, "at": [500, 500]});
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "spare", "name": "Spare"}));
    });
    let (lint, _) = json_of("lint", &root, &[], 0);
    let codes: Vec<(&str, &str)> = lint["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["code"].as_str().unwrap(),
                finding["severity"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            ("preview.effects-neutral", "info"),
            ("portal.target_no_chunk", "warning"),
            ("portal.target_own_tile", "warning"),
            ("portal.target_no_ground", "warning"),
            ("anchor.unreferenced", "info"),
            ("anchor.unreferenced", "info"),
            ("zone.unused", "info"),
        ]
    );
    let own = &lint["findings"][2]["location"];
    assert_eq!(own["uid"], "gate_east");
    assert_eq!(own["path"], "planes/main/layers/0/0_0.jsonc");
    assert_eq!(own["tile"], json!([7, 0]));
    assert_eq!(lint["findings"][3]["location"]["grid"], "floor");
    assert_eq!(lint["findings"][6]["location"]["zone"], "spare");

    // An object the catalog cannot draw stops the view from being
    // prepared: an error, so lint exits 1.
    store_json(
        &root,
        "defs/proto/orphan.jsonc",
        &json!({"format": "tilewright.def/1", "type": "proto", "id": "orphan", "fields": {}}),
    );
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"uid": "orphan_1", "at": [2, 2], "proto": "orphan", "layer": "item"}));
    });
    let (lint, _) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    assert_eq!(lint["findings"][0]["code"], "preview.unprepared");
    assert_eq!(lint["counts"]["error"], 1);

    // A catalog that does not load is an error too; the other commands
    // still answer, with the catalog's status.
    std::fs::write(root.join("preview.jsonc"), "{\"format\": \"other\"}").unwrap();
    let (lint, _) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    assert_eq!(lint["findings"][0]["code"], "catalog.invalid");
    assert_eq!(lint["catalog"]["status"], "invalid");
    let (tile, _) = json_of("tile", &root, &["main", "0", "3", "2"], 0);
    assert_eq!(tile["catalog"]["status"], "invalid");
    assert_eq!(tile["objects"][0]["allowed"], Value::Null);
}

/// Every entry under `root`: its kind, bytes, permissions and modification
/// time.
fn snapshot(
    root: &Path,
) -> BTreeMap<PathBuf, (bool, Vec<u8>, std::fs::Permissions, std::time::SystemTime)> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            let bytes = if metadata.is_dir() {
                pending.push(entry.path());
                Vec::new()
            } else {
                std::fs::read(entry.path()).unwrap()
            };
            entries.insert(
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                (
                    metadata.is_dir(),
                    bytes,
                    metadata.permissions(),
                    metadata.modified().unwrap(),
                ),
            );
        }
    }
    entries
}

#[test]
fn the_commands_leave_the_world_tree_exactly_as_it_was() {
    let (directory, root) = neutral_copy();
    // A sibling of the root, where a careless writer might leave a lock,
    // recovery or temporary file.
    let parent = snapshot(directory.path());
    let before = snapshot(&root);
    let path = root.to_str().unwrap();
    for (command, extra) in [
        ("overview", &[][..]),
        ("tile", &["main", "0", "3", "2"][..]),
        ("tile", &["main", "0", "-2", "2"][..]),
        ("catalog", &[][..]),
        ("catalog", &["--match", "edge"][..]),
        ("lint", &[][..]),
    ] {
        for json in [false, true] {
            let mut arguments = vec![path];
            if json {
                arguments.push("--json");
            }
            arguments.extend_from_slice(extra);
            assert_eq!(run(command, &arguments).code, 0, "{command}");
            assert_eq!(snapshot(&root), before, "{command} changed the world");
            assert_eq!(
                snapshot(directory.path()),
                parent,
                "{command} added an entry"
            );
        }
    }
}
