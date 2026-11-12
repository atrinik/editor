// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::*;
#[test]
fn commit_in_drain_fences_return_in_either_event_order() {
    for composing in [false, true] {
        let mut input = NativeInput::new(1);
        input.begin_batch(true, None);
        assert!(!input.submit_allowed(composing));
        input.text_activity();
        assert!(!input.submit_allowed(false));
        input.return_released(false);
        assert!(!input.submit_allowed(false));
        input.begin_batch(false, None);
        assert!(input.submit_allowed(false));
    }
}
#[test]
fn focus_loss_requires_explicit_refocus_and_fresh_return_release() {
    let mut input = NativeInput::new(1);
    input.lose_focus();
    input.begin_batch(false, None);
    input.return_released(false);
    assert!(!input.submit_allowed(false));
    input.focused = true;
    assert!(input.submit_allowed(false));
}
#[test]
fn composition_capability_is_set_in_fresh_process() {
    const CHILD: &str = "TILEWRIGHT_TEST_IME_HINT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        configure_ime_ui().unwrap();
        assert_eq!(
            sdl3::hint::get(sdl3::hint::names::IME_IMPLEMENTED_UI).as_deref(),
            Some("composition")
        );
        return;
    }
    // Hints are SDL process-global. Test in a fresh subprocess, without
    // initializing SDL or changing another test's environment or UI state.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_input::tests::composition_capability_is_set_in_fresh_process",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("SDL_IME_IMPLEMENTED_UI", "candidates")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
