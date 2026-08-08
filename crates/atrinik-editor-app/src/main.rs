// Copyright 2026 The Atrinik Project
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

fn main() {
    let command = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "version".to_owned());
    match command.as_str() {
        "version" | "--version" => println!(
            "atrinik-editor {} toolkit={} renderer={}",
            env!("CARGO_PKG_VERSION"),
            atrinik_editor_document::TOOLKIT_COMPATIBILITY,
            atrinik_editor_preview::RENDERER_COMPATIBILITY
        ),
        "headless" => {
            let mut state = atrinik_editor_project::ProjectState::default();
            let path = atrinik_editor_project::RelativePath::new("maps/empty.map")
                .expect("constant path is valid");
            state
                .open(path.clone())
                .expect("empty project state is available");
            let document = atrinik_editor_document::DocumentView::open(
                "headless:empty",
                std::sync::Arc::from(&b"name empty\n"[..]),
            )
            .expect("synthetic document is valid");
            let history = atrinik_editor_commands::History::default();
            let mut ui = atrinik_editor_ui::UiState::default();
            ui.select(atrinik_editor_ui::Selection {
                document: path,
                semantic_id: 1,
            })
            .expect("synthetic selection is valid");
            let _scene =
                atrinik_editor_preview::empty_scene(1, 1, 1).expect("synthetic viewport is valid");
            println!(
                "headless generation={} diagnostics={} history={:?}",
                state.generation(),
                document.diagnostics_len(),
                history.depths()
            );
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
