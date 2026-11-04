# Changelog

## Unreleased

### Second round: editor shell, GPU, remaining nodes (never compiled, see PLAN.md)
- `compnode-ui`: footer rows (widget-only rows, used for image previews),
  keyboard focus ring (Tab, arrows, Enter, Space) and an accessibility tree,
  minimap geometry, `Editor::replace_document`, draw margin matched to the
  `DrawKey` cell so cached draw lists can be translated while panning.
- `compnode-image`: Curves node (five-point monotone curve), `image-io` feature
  (JPEG and lossless WebP export, decoding through the `image` crate), `gpu`
  feature (wgpu backend: WGSL shaders for every stock node, `GpuEvaluator`,
  half-float upload/download, display pass, GPU-versus-CPU comparison),
  `ThumbCache`, `StoreLoader`.
- `crates/compnode-editor`: the egui application (graph panel with cached meshes,
  canvas viewer, menus, text-edit overlay, add-node menu, drag and drop,
  async loading, thumbnails, minimap, project and theme files, `--selftest-gpu`,
  `--bench`).
- CI installs the Linux libraries the editor needs and also tests the `image-io` feature.

### First round: Phases 1 to 9, headless parts
- `compnode`: per-row node heights (`Graph::set_row_heights`, `set_width`),
  `Graph::topo_order`, `reaches` and dry-run `can_connect`, `rgb_to_hsv` /
  `hsv_to_rgb`.
- `compnode-ui`: `Value` / `SocketType` with conversion rules, node schemas and a
  searchable registry, ten widgets (float, int, slider, toggle, button,
  dropdown, text area, color picker, image preview, label), `Document` with
  stable node uids, undo/redo commands with transactions and merging,
  selection, the interaction state machine (select, drag, box-select, wire
  drag with feedback, re-route, link-cut, insert-on-wire, snapping and
  guides, resize, frames, reroutes, mute, collapse, clipboard, shortcuts),
  themes (with a text format), draw list rendering with level of detail,
  project save/load, canvas viewer state.
- `compnode-image`: CPU image buffer and reference operations, 16 stock nodes,
  evaluator with dirty propagation and a memory-budgeted cache, PPM loader,
  PNG/PPM export, async loader, thumbnails, texture pool.
- Not done: egui shell, wgpu backend, Curves, JPEG/WebP, minimap, focus
  management for keyboard-only editing, benchmarks.

### Phase 0: foundation (not yet verified by a compiler, see PLAN.md)
- Renamed the project from `nodeforge` to `compnode`.
- Converted to a Cargo workspace: `compnode`, `compnode-ui`, `compnode-image`,
  `crates/compnode-editor` (`compnode-editor`).
- Moved to Rust edition 2024, `rust-version = "1.85"`, resolver 3, with
  shared settings in `[workspace.package]`.
- `#![forbid(unsafe_code)]` on all library crates.
- `geom`: added `Vec2` helpers (`splat`, `length_sq`, `distance`, `min`, `max`,
  `lerp`, `Neg`, `AddAssign`, `SubAssign`), `Rect` helpers (`from_min_size`,
  `from_center_size`, `center`, `width`, `height`, `area`, `union`,
  `intersection`, `contains_rect`, `translate`) and the `Affine2` transform.
- New `color` module: linear-light `Color` with sRGB8 and hex conversion and
  premultiply helpers.
- `Viewport::graph_to_screen` / `screen_to_graph` return `Affine2`.
- Replaced `map_or(false/true, ..)` with `is_some_and` / `is_none_or`
  (clippy `unnecessary_map_or`).
- `bench` example accepts node counts as arguments.
- Added `rustfmt.toml` (style edition 2024) and a GitHub Actions workflow
  (fmt, clippy, test on 3 OSes, MSRV check, bench smoke run).

