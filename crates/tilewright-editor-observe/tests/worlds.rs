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
    assert_eq!(
        more(&overview, "planes"),
        71 - limits::MAXIMUM_PLANES as u64
    );
    let main = &overview["planes"][0];
    assert_eq!(main["id"], "main");
    assert_eq!(more(main, "layers"), 71 - limits::MAXIMUM_LAYERS as u64);
    let grants = &overview["catalog"]["grants"];
    assert_eq!(
        more(grants, "movable_uids"),
        71 - limits::MAXIMUM_GRANTS as u64
    );
    assert_eq!(
        more(&grants["terrain"][0], "values"),
        302 - limits::MAXIMUM_GRANTS as u64
    );

    let (tile, printed) = json_of("tile", &root, &["main", "0", "0", "0"], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(
        more(&tile, "objects"),
        70 - limits::MAXIMUM_TILE_OBJECTS as u64
    );
    let first = &tile["objects"][0];
    assert_eq!(more(first, "fields"), 21 - limits::MAXIMUM_FIELDS as u64);
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
    assert_eq!(
        more(&tile, "arrivals"),
        70 - limits::MAXIMUM_ARRIVALS as u64
    );

    let (catalog, printed) = json_of("catalog", &root, &[], 0);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(
        more(&catalog, "prototypes"),
        323 - limits::MAXIMUM_PROTOTYPES as u64
    );
    assert_eq!(
        more(&catalog, "terrain_palette"),
        302 - limits::MAXIMUM_TERRAIN_CHOICES as u64
    );
    assert_eq!(
        more(&catalog, "zone_palette"),
        303 - limits::MAXIMUM_ZONES as u64
    );
    let (matched, _) = json_of("catalog", &root, &["--match", "CROWD_ZONE_29"], 0);
    assert_eq!(
        run("catalog", &[root.to_str().unwrap(), "--match", "--json"]).code,
        cli::EXIT_USAGE
    );
    // crowd_zone_290 to crowd_zone_299.
    assert_eq!(matched["zone_palette"].as_array().unwrap().len(), 10);
    assert_eq!(matched["prototypes"].as_array().unwrap().len(), 0);

    let (lint, printed) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    assert!(printed.len() <= limits::MAXIMUM_OUTPUT_BYTES);
    assert_eq!(
        lint["findings"].as_array().unwrap().len(),
        limits::MAXIMUM_FINDINGS
    );
    // 70 own-tile portals, 300 unused zones, 11 unreferenced anchors and
    // the preview's own note.
    // The crowded world is over the editing budget too.
    assert_eq!(lint["counts"]["error"], 1);
    assert_eq!(lint["counts"]["warning"], 70);
    assert_eq!(lint["counts"]["info"], 300 + 11 + 1);
    assert_eq!(more(&lint, "findings"), 1 + 70 + 312 - 256);

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
        assert_eq!(text.code, u8::from(command == "lint"));
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

#[test]
fn a_per_uid_move_grant_outside_the_view_layer_is_not_reported_as_allowed() {
    let (_directory, root) = neutral_copy();
    let mut chunk = read_json(&root, "planes/main/layers/0/-1_0.jsonc");
    chunk["layer"] = json!(1);
    chunk["objects"] = json!([{"uid": "far_1", "at": [5, 5], "proto": "marker", "layer": "item"}]);
    store_json(&root, "planes/main/layers/1/-1_0.jsonc", &chunk);
    edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["movable_uids"]
            .as_array_mut()
            .unwrap()
            .push(json!("far_1"));
    });
    let (tile, _) = json_of("tile", &root, &["main", "1", "-27", "5"], 0);
    let object = &tile["objects"][0];
    assert_eq!(object["uid"], "far_1");
    assert_eq!(
        object["allowed"],
        json!({"move": false, "delete": false, "portal": false, "fields": [], "fields_more": 0})
    );
    // The same grant in the view's layer is a move.
    let (tile, _) = json_of("tile", &root, &["main", "0", "3", "2"], 0);
    assert_eq!(tile["objects"][0]["allowed"]["move"], true);
}

#[test]
fn objects_without_a_uid_and_carried_portals_are_reported() {
    let (_directory, root) = neutral_copy();
    edit(&root, "planes/main/layers/0/-1_0.jsonc", |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        // Outside the view, so the view still prepares.
        objects.push(json!({"at": [10, 10], "proto": "marker", "layer": "item"}));
        objects.push(json!({
            "uid": "bag_1", "at": [12, 12], "proto": "crate", "layer": "item",
            "inventory": [{"proto": "marker", "inventory": [
                {"uid": "inner_1", "proto": "marker", "portal": {"layer": 0, "at": [-20, 12]}}
            ]}]
        }));
    });
    edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["movable_uids"] = json!(["marker_1", "bag_1"]);
    });
    let (tile, _) = json_of("tile", &root, &["main", "0", "-22", "10"], 0);
    let object = &tile["objects"][0];
    assert_eq!(object["uid"], Value::Null);
    assert_eq!(
        object["allowed"],
        json!({"move": false, "delete": false, "portal": false, "fields": [], "fields_more": 0})
    );
    let (tile, _) = json_of("tile", &root, &["main", "0", "-20", "12"], 0);
    let bag = &tile["objects"][0];
    assert_eq!(bag["uid"], "bag_1");
    assert_eq!(bag["inventory"], 1);
    assert_eq!(bag["carried"], 2);
    // A per-uid grant, but on chunk -1_0 of the view's layer: it moves.
    assert_eq!(bag["allowed"]["move"], true);
    assert_eq!(tile["arrivals"].as_array().unwrap().len(), 1);
    assert_eq!(tile["arrivals"][0]["uid"], "inner_1");
    assert_eq!(tile["arrivals"][0]["carried"], true);
    assert_eq!(tile["arrivals"][0]["at"], json!([-20, 12]));
    let (lint, _) = json_of("lint", &root, &[], 0);
    let own: Vec<&Value> = lint["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == "portal.target_own_tile")
        .collect();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0]["location"]["uid"], "inner_1");
    assert_eq!(own[0]["location"]["carried"], true);
    assert_eq!(
        own[0]["location"]["path"],
        "planes/main/layers/0/-1_0.jsonc"
    );
}

/// An identifier of World's longest, 128 characters, ending in `tag`.
fn long_id(prefix: char, tag: usize) -> String {
    let tag = format!("{tag:04}");
    format!("{prefix}{}{tag}", "x".repeat(127 - tag.len()))
}

/// Planes past the plane bound, with the longest ids and widest titles,
/// each with layers past the layer bound.
fn widest_planes(root: &Path, text: &str) {
    let root = root.to_path_buf();
    // Planes with the longest ids and titles, each with every layer full.
    let mut chunk = read_json(&root, "planes/main/layers/0/-1_0.jsonc");
    chunk["objects"] = json!([]);
    edit(&root, "world.jsonc", |world| {
        for index in 0..=limits::MAXIMUM_PLANES {
            let id = long_id('q', index);
            world["planes"].as_array_mut().unwrap().push(json!(id));
            store_json(
                &root,
                &format!("planes/{id}/plane.jsonc"),
                &json!({"format": "tilewright.plane/1", "id": id, "title": text, "kind": "shared", "chunk_size": 32, "default_zone": long_id('z', 0)}),
            );
            store_json(
                &root,
                &format!("planes/{id}/zones.jsonc"),
                &json!({"format": "tilewright.zones/1", "zones": [{"id": long_id('z', 0), "name": text}]}),
            );
            chunk["plane"] = json!(id);
            for layer in 0..=limits::MAXIMUM_LAYERS {
                chunk["layer"] = json!(layer);
                store_json(
                    &root,
                    &format!("planes/{id}/layers/{layer}/-1_0.jsonc"),
                    &chunk,
                );
            }
        }
    });
}

/// The neutral fixture with every list of every report past its bound, the
/// longest ids World allows and free text that prints at its widest.
fn widest() -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = neutral_copy();
    // 256 tag characters: each prints as a 12-byte JSON escape, the most.
    let text: String = std::iter::repeat_n('\u{e0041}', limits::MAXIMUM_TEXT_CHARACTERS).collect();
    // Field names are at most 64 characters.
    let field_names: Vec<String> = (0..=limits::MAXIMUM_FIELDS)
        .map(|i| format!("f{}{i:04}", "x".repeat(59)))
        .collect();
    edit(&root, "profile/profile.jsonc", |profile| {
        let layers = profile["render_layers"].as_array_mut().unwrap();
        for index in 0..=limits::MAXIMUM_RENDER_LAYERS {
            layers.push(json!(long_id('r', index)));
        }
    });
    edit(&root, "profile/schema/object.jsonc", |schema| {
        for name in &field_names {
            schema["fields"][name] = json!({"type": "string"});
        }
    });
    let mut catalog = read_json(&root, "preview.jsonc");
    let protos: Vec<String> = (0..=limits::MAXIMUM_PROTOTYPES)
        .map(|i| long_id('p', i))
        .collect();
    let defaults: serde_json::Map<String, Value> = field_names
        .iter()
        .map(|name| (name.clone(), json!("x")))
        .collect();
    for proto in &protos {
        store_json(
            &root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format": "tilewright.def/1", "type": "proto", "id": proto, "fields": {}}),
        );
        catalog["prototypes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"proto": proto, "resource": "marker", "anchor": "standing"}));
        catalog["editing"]["terrain"][0]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!(proto));
        catalog["editing"]["objects"].as_array_mut().unwrap().push(
            json!({"proto": proto, "layer": "item", "fields": field_names, "defaults": defaults}),
        );
    }
    // Objects on tile (0, 0) with the longest uids and every field full.
    let fields: serde_json::Map<String, Value> = field_names
        .iter()
        .map(|name| (name.clone(), json!(text)))
        .collect();
    let mut objects = Vec::new();
    for index in 0..=limits::MAXIMUM_GRANTS {
        let uid = long_id('u', index);
        objects.push(json!({
            "uid": uid, "at": [0, 0], "proto": protos[0], "layer": "item", "fields": fields,
            "portal": {"plane": long_id('q', 0), "layer": 0, "at": [0, 0]}
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
    edit(&root, "planes/main/zones.jsonc", |zones| {
        for index in 0..=limits::MAXIMUM_FINDINGS {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": long_id('z', index), "name": text}));
        }
    });
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!(text);
    });
    widest_planes(&root, &text);
    (directory, root)
}

#[test]
fn reports_at_every_bound_with_the_longest_ids_and_text_fit_the_budget() {
    let (_directory, root) = widest();
    for (command, extra) in [
        ("overview", &[][..]),
        ("tile", &["main", "0", "0", "0"][..]),
        ("catalog", &[][..]),
        ("lint", &[][..]),
    ] {
        for json in [true, false] {
            let mut arguments = vec![root.to_str().unwrap()];
            if json {
                arguments.push("--json");
            }
            arguments.extend_from_slice(extra);
            let outcome = run(command, &arguments);
            assert_eq!(
                outcome.code,
                u8::from(command == "lint"),
                "{command}: {}",
                outcome.stderr
            );
            eprintln!("{command} json={json}: {} bytes", outcome.stdout.len());
            assert!(outcome.stdout.len() <= limits::MAXIMUM_OUTPUT_BYTES);
        }
    }
    // Every list really is full.
    let (tile, _) = json_of("tile", &root, &["main", "0", "0", "0"], 0);
    assert!(more(&tile, "objects") > 0 && more(&tile["objects"][0], "fields") > 0);
    let (catalog, _) = json_of("catalog", &root, &[], 0);
    assert!(more(&catalog, "prototypes") > 0 && more(&catalog, "zone_palette") > 0);
    let long = catalog["prototypes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["proto"].as_str().unwrap().len() == 128)
        .unwrap();
    assert!(more(long, "fields") > 0 && more(long, "defaults") > 0);
    let (overview, _) = json_of("overview", &root, &[], 0);
    assert!(more(&overview, "planes") > 0 && more(&overview["planes"][1], "layers") > 0);
    assert!(more(&overview["profile"], "render_layers") > 0);
    assert!(more(&overview["catalog"]["grants"], "objects") > 0);
    let (lint, _) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    assert!(more(&lint, "findings") > 0);
}

/// The neutral fixture with `count` more prototype definitions.
fn with_definitions(count: usize) -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = neutral_copy();
    for index in 0..count {
        let proto = format!("extra_{index:03}");
        store_json(
            &root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format": "tilewright.def/1", "type": "proto", "id": proto, "fields": {}}),
        );
    }
    (directory, root)
}

/// The World source files of a tree: every JSONC file but the catalog.
fn world_files(root: &Path) -> usize {
    snapshot(root)
        .keys()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonc")
                && path.as_path() != Path::new("preview.jsonc")
        })
        .count()
}

#[test]
fn lint_reports_a_world_over_the_editing_budget_and_everything_else() {
    // The review's repro: 300 more definitions than the budget's files.
    let (directory, root) = with_definitions(300);
    let (before, beside) = (snapshot(&root), snapshot(directory.path()));
    let (lint, printed) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    // Both loads only read.
    assert_eq!(snapshot(&root), before);
    assert_eq!(snapshot(directory.path()), beside);
    let codes: Vec<&str> = lint["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "world.over_editor_budget",
            "preview.effects-neutral",
            "anchor.unreferenced"
        ]
    );
    assert_eq!(lint["findings"][0]["severity"], "error");
    // As World reported them: the budget, and the files it then left unread.
    assert_eq!(
        lint["findings"][0]["location"]["codes"],
        json!(["source.budget", "source.missing_file"])
    );
    assert_eq!(lint["counts"]["error"], 1);
    let revision = |text: &str| {
        let document = tilewright_editor_document::WorldDocument::open(&root).unwrap();
        text.replace(
            &tilewright_editor_document::revision_hex(&document.revision()),
            "<revision>",
        )
    };
    let text = run("lint", &[root.to_str().unwrap()]);
    assert_eq!(text.code, cli::EXIT_FINDINGS);
    for (name, actual) in [
        ("lint-over-budget.json", &printed),
        ("lint-over-budget.txt", &text.stdout),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        let actual = revision(actual);
        if std::env::var_os("TILEWRIGHT_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &actual).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(actual, expected, "golden {name} differs");
    }
    // The other commands load with World's default bounds and describe it.
    let (overview, _) = json_of("overview", &root, &[], 0);
    assert_eq!(overview["definitions"], 323);
}

#[test]
fn a_world_just_inside_the_editing_budget_has_no_budget_finding() {
    let files = tilewright_editor_document::prototype_load_options().maximum_files;
    let base = world_files(&fixture_world("prototype-neutral"));
    let (_inside, root) = with_definitions(files - base);
    assert_eq!(world_files(&root), files);
    let (lint, _) = json_of("lint", &root, &[], 0);
    assert_eq!(lint["counts"]["error"], 0, "{lint}");
    let (_outside, root) = with_definitions(files - base + 1);
    let (lint, _) = json_of("lint", &root, &[], cli::EXIT_FINDINGS);
    assert_eq!(lint["findings"][0]["code"], "world.over_editor_budget");
}

#[test]
fn a_budgeted_load_that_succeeds_gives_the_world_the_default_load_gives() {
    for name in ["prototype-neutral", "meadow"] {
        let root = fixture_world(name);
        let budgeted = tilewright_editor_document::WorldDocument::open_prototype(&root).unwrap();
        let default = tilewright_editor_document::WorldDocument::open(&root).unwrap();
        assert!(budgeted.world() == default.world(), "{name}");
    }
}
