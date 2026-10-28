// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

mod paint;

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
        "window" => {
            let sdl = sdl3::init().unwrap_or_else(|error| fail(&error));
            let video = sdl.video().unwrap_or_else(|error| fail(&error));
            let window = video
                .window("Tilewright editor", 640, 480)
                .hidden()
                .resizable()
                .build()
                .unwrap_or_else(|error| fail(&error));
            let (width, height) = window.size_in_pixels();
            if width == 0 || height == 0 {
                fail("SDL returned an empty window");
            }
            drop(window);
            println!("window=created-and-destroyed pixels={width}x{height}");
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: tilewright-editor [version | open ROOT | paint ROOT OPTIONS X,Y... | window]\n\n{}",
        paint::USAGE
    );
    std::process::exit(2);
}

fn fail(error: &(impl std::fmt::Display + ?Sized)) -> ! {
    eprintln!(
        "tilewright-editor: {}",
        tilewright_editor_document::terminal_field(&error.to_string())
    );
    std::process::exit(1);
}
