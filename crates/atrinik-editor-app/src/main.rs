// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

const TOOLKIT_COMPATIBILITY: &str = "content-toolkit-v1/0.1.0";
const RENDERER_COMPATIBILITY: &str = "scene-bundle-v1/0.1.0";

fn main() {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "version".to_owned());
    match command.as_str() {
        "version" | "--version" => println!(
            "atrinik-editor {} toolkit={} renderer={}",
            env!("CARGO_PKG_VERSION"),
            TOOLKIT_COMPATIBILITY,
            RENDERER_COMPATIBILITY
        ),
        "headless" => {
            let mut state = atrinik_editor_project::ProjectState::default();
            let path = atrinik_editor_project::RelativePath::new("maps/empty.map")
                .expect("constant path is valid");
            state.open(path).expect("empty project state is available");
            println!("headless generation={}", state.generation());
        }
        "window" => {
            let sdl = sdl3::init().unwrap_or_else(|error| fail(&error));
            let video = sdl.video().unwrap_or_else(|error| fail(&error));
            let window = video
                .window("Atrinik editor", 640, 480)
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
        _ => {
            eprintln!("usage: atrinik-editor [version|headless|window]");
            std::process::exit(2);
        }
    }
}

fn fail(error: &(impl std::fmt::Display + ?Sized)) -> ! {
    eprintln!("atrinik-editor: {error}");
    std::process::exit(1);
}
