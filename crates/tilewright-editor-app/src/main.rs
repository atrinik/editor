// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

mod headless;
mod paint;
mod window;
mod workbench;

fn main() {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "version".to_owned());
    match command.as_str() {
        "version" | "--version" => println!(
            "tilewright-editor {} toolkit={} renderer={}",
            env!("CARGO_PKG_VERSION"),
            tilewright_editor_document::TOOLKIT_COMPATIBILITY,
            tilewright_editor_preview::RENDERER_COMPATIBILITY
        ),
        "open" => {
            let Some(root) = std::env::args_os().nth(2) else {
                usage();
            };
            match tilewright_editor_document::WorldDocument::open(std::path::Path::new(&root)) {
                Ok(document) => print!(
                    "{}",
                    tilewright_editor_document::terminal_safe(&format!(
                        "revision {}\n{}",
                        tilewright_editor_document::revision_hex(&document.revision()),
                        document.summary()
                    ))
                ),
                Err(tilewright_editor_document::Error::Invalid(diagnostics)) => {
                    for diagnostic in &diagnostics {
                        eprintln!(
                            "{}",
                            tilewright_editor_document::terminal_field(&diagnostic.to_string())
                        );
                    }
                    fail(&format!(
                        "the world does not load: {} problem(s)",
                        diagnostics.len()
                    ));
                }
            }
        }
        "paint" => {
            let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(2).collect();
            // Without arguments, the help, as written.
            if arguments.is_empty() {
                usage();
            }
            if let Err(error) = paint::run(&arguments) {
                fail(&error);
            }
        }
        "edit" => {
            let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(2).collect();
            let (root, plane, accepted) = edit_arguments(&arguments).unwrap_or_else(|| usage());
            if let Err(error) = window::run(&root, plane.as_deref(), accepted) {
                fail(&error);
            }
        }
        "headless" => {
            let Some(root) = std::env::args_os().nth(2) else {
                usage();
            };
            match headless::run(std::path::Path::new(&root)) {
                Ok(report) => println!("{report}"),
                Err(error) => fail(&error),
            }
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: tilewright-editor [version | open ROOT | edit ROOT [--plane PLANE] [--unchecked-access] | headless ROOT | paint ROOT OPTIONS X,Y...]\n\n\
edit opens the editor window on the world at ROOT. Where saving cannot\n\
check everything (macOS, Windows, and Linux trees on file systems that\n\
hide access control lists), --unchecked-access accepts that up front;\n\
otherwise the window asks before it first saves there. headless runs the\n\
window's interface and view for a few frames without a window or GPU.\n\n{}",
        paint::USAGE
    );
    std::process::exit(2);
}

/// The root, plane and consent of `edit`'s arguments.
fn edit_arguments(
    arguments: &[std::ffi::OsString],
) -> Option<(std::path::PathBuf, Option<String>, bool)> {
    let (root, mut rest) = arguments.split_first()?;
    let (mut plane, mut accepted) = (None, false);
    while let Some((argument, after)) = rest.split_first() {
        match argument.to_str()? {
            "--unchecked-access" => {
                accepted = true;
                rest = after;
            }
            "--plane" => {
                let (value, after) = after.split_first()?;
                plane = Some(value.to_str()?.to_owned());
                rest = after;
            }
            _ => return None,
        }
    }
    Some((std::path::PathBuf::from(root), plane, accepted))
}

fn fail(error: &(impl std::fmt::Display + ?Sized)) -> ! {
    eprintln!(
        "tilewright-editor: {}",
        tilewright_editor_document::terminal_field(&error.to_string())
    );
    std::process::exit(1);
}
