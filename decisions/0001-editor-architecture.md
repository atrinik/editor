# ADR 0001: editor authority and dependency direction

Status: accepted for M1; amended 2026-10-26 (below).

The editor is an offline authoring consumer. The world toolkit owns source
syntax, lossless documents, schemas, catalogs, diagnostics, compilers, semantic
diffs, and atomic file transactions. The Tilewright renderer owns scene
types, resources, device/GPU state, output targets, semantic masks, and
offscreen rendering. This repository owns project/tab state, explicit user
commands and history, document-to-scene adaptation, authoring panels/tools,
diagnostic presentation, preview composition, recovery UI, and packages.

The dependency direction is:

```text
world toolkit release   -> document adapter -> commands -> application
renderer release        -> preview adapter  -> application
project state -> commands/UI/test fakes
```

Production manifests pin immutable released source revisions. Coordinated local
overrides stay outside committed manifests. There is no game client, protocol,
server, game-specific, parser, writer, network service, or alternate renderer
dependency.

M1 uses the shared renderer's versioned scene/API/resource crates and its
deterministic reference test renderer to prove an empty viewport. GPU and SDL
handles stay below released renderer/application boundaries. The application
owns only SDL lifecycle; headless model tests do not initialize SDL.

## Amendment 1 (2026-10-26): the window is eframe, not SDL3

Status: accepted. It supersedes the SDL3 parts of this decision.

The editor's interface is egui, the owner's choice of UI library. Its window
is eframe: winit for the window and input, and egui-wgpu to draw egui with
its own wgpu device, on the same wgpu major version as the renderer. SDL3
leaves the editor.

- The renderer still draws everything of the world. The application asks it
  for an offscreen frame of the plane in view and shows the frame's pixels as
  an egui image; the pointer finds tiles in the frame's exact semantic mask.
  Renderer GPU handles stay private to the renderer, and egui's stay inside
  eframe; the editor holds neither.
- One exception to picking from the mask alone: at some zooms the renderer
  leaves one-pixel seams between tiles, where the mask holds no tile, and
  there the pointer picks the nearest tile within `PICK_REACH` (2) pixels.
  It is to go when the editor adopts the renderer's fix, which snaps both
  edges of each quad (tilewright-renderer #55); revisit it then. The
  half-texel bleed of the editor's diamond swatches, which makes odd-sized
  neighbours meet, is the editor's own art, not picking, and stays.
- egui-wgpu is a second GPU device in the process and the view costs one
  readback each time it is drawn again. The view is drawn only when it
  changes, which an editor's view does at interaction rates, not every
  frame.
- No unsafe code: winit's windows can be handed to wgpu safely, so the
  editor needs no handle bridge. Linux builds load Wayland, X11 and Vulkan
  libraries at run time, so they need no native development headers.
- The interface and its tests stay headless: egui runs without a window or
  GPU, `tilewright-editor headless` composes the interface and the view for
  validation, and view tests render with the renderer's CPU reference.
- The alternatives were painting egui's meshes as renderer quads, which the
  scene contract cannot clip or shade per vertex, and adding an egui painter
  to the renderer, which waits on a renderer change. Either can replace this
  later without changing the interface code.
