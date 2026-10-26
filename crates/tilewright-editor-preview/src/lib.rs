// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

pub mod ground;

use std::sync::Arc;
use tilewright_render_api::{Error as RenderError, FrameOutput, RenderRequest, Renderer};
use tilewright_render_resources::ResourceProvider;
use tilewright_scene::{SceneBundle, SceneLimits, Viewport};

pub const RENDERER_COMPATIBILITY: &str = "scene-bundle-v3/0.3.0";

pub fn empty_scene(
    width: u32,
    height: u32,
    revision: u64,
) -> Result<SceneBundle, tilewright_scene::Error> {
    SceneBundle::new(
        revision,
        0,
        Viewport {
            width,
            height,
            scale_milli: 1_000,
        },
        [0.04, 0.05, 0.07, 1.0],
        [],
        SceneLimits::default(),
    )
}

pub fn render_scene<R: Renderer>(
    renderer: &mut R,
    scene: &SceneBundle,
    resources: Arc<dyn ResourceProvider>,
) -> Result<FrameOutput, RenderError> {
    renderer.render(RenderRequest { scene, resources })
}

#[cfg(test)]
mod tests {
    use super::{empty_scene, render_scene};
    use tilewright_render_api::{BackendPreference, TargetDescriptor, TargetKind};
    use tilewright_render_testkit::{ReferenceRenderer, synthetic_provider};

    #[test]
    fn released_shared_renderer_draws_empty_viewport() {
        let scene = empty_scene(32, 24, 1).unwrap();
        let mut renderer = ReferenceRenderer::new(TargetDescriptor {
            kind: TargetKind::Offscreen,
            width: 32,
            height: 24,
            backend: BackendPreference::Automatic,
        })
        .unwrap();
        assert_eq!(
            super::RENDERER_COMPATIBILITY,
            format!(
                "scene-bundle-v{}/0.3.0",
                tilewright_scene::SCENE_BUNDLE_VERSION
            )
        );
        assert_eq!(scene.version(), tilewright_scene::SCENE_BUNDLE_VERSION);
        let frame = render_scene(&mut renderer, &scene, synthetic_provider().unwrap()).unwrap();
        assert_eq!((frame.width, frame.height), (32, 24));
        assert!(frame.semantic_ids.iter().all(|identity| *identity == 0));
        assert!(frame.coverage.iter().all(|coverage| *coverage == 0));
    }
}
