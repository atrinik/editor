// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The binary's read-only commands print exactly what the observe crate
//! returns and exit with its code. Their outputs are tested there.
use std::ffi::OsString;
use std::process::Command;
use tilewright_editor_observe::cli;
use tilewright_editor_testkit::fixture_world;

#[test]
fn the_binary_runs_the_read_only_commands() {
    let root = fixture_world("prototype-neutral");
    for (command, extra) in [
        ("overview", &["--json"][..]),
        ("tile", &["main", "0", "7", "0"][..]),
        ("catalog", &["--match", "gate"][..]),
        ("lint", &["--json"][..]),
        ("tile", &["nowhere", "0", "0", "0"][..]),
        ("tile", &["main", "zero", "0", "0"][..]),
    ] {
        let mut arguments: Vec<OsString> = vec![root.clone().into_os_string()];
        arguments.extend(extra.iter().map(OsString::from));
        let expected = cli::run(command, &arguments);
        let output = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
            .arg(command)
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::from(expected.code)),
            "{command} {extra:?}"
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected.stdout);
        assert_eq!(String::from_utf8(output.stderr).unwrap(), expected.stderr);
    }
    let usage = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("unknown")
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(
        String::from_utf8(usage.stderr)
            .unwrap()
            .contains("overview [--json] ROOT")
    );
}
