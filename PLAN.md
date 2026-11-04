# PLAN: compnode

A desktop, node-based image editor (Photoshop-like, but every operation is a node). Image-file nodes feed filter nodes, and the wired result is shown on a 2K canvas.

- **Platform:** desktop only
- **Canvas:** 2048 x 2048
- **Language:** Rust, edition 2024 (MSRV 1.85)
- **UI:** egui (eframe)
- **Image processing:** wgpu (GPU), with CPU reference implementations for tests only
- **Node graph engine:** the existing `compnode` crate (headless, zero dependencies)

> **Status: every phase is now written. Phase 0 was verified by you (formatted, tested, clippy-clean); everything added after that (the second round: egui shell, wgpu backend, Curves, minimap, focus, `image-io`) has NOT been compiled.** The sandbox used to prepare this has no Rust toolchain and no network, so no code has been built, tested, formatted or benchmarked. The tests in the new code were written alongside it and checked by reading, not by running. Expect compile errors and clippy lints on the first build, and some test expectations to need fixing. Run the "First run" steps in `README.md` and tick the verification items in Phase 0 below. Performance figures are estimates until measured.
>
> **Not done:** reuse of an input texture for single-consumer nodes, GPU timestamp queries, a free-form curve editor widget, recorded benchmark numbers.

---

## 1. Goals and non-goals

### Goals

- Blender-geometry-nodes-style UX in the node editor: scrub fields, box-select, wire dragging, link-cut, insert-on-wire, snapping, undo/redo.
- Smooth slider dragging: a parameter change re-runs only the affected node and its downstream nodes.
- Default widget set: float, int, text area, slider, button, toggle, dropdown, image preview, color picker, label.
- A themeable visual design system.
- A 2K canvas output that updates interactively.

### Non-goals (explicitly dropped)

- Web / WebGPU support.
- Tiling, region-of-interest evaluation, and mip selection for the canvas (2K fits in a single texture).
- GPU wire batching and instanced node rendering for the node editor (only needed for thousands of nodes; revisit if graphs grow that large).
- Writing a custom text field, menu system, or glyph atlas (egui provides these).

---

## 2. Architecture

```
compnode/                        workspace root (edition 2024)
├─ crates/compnode               graph model, DAG, grid, wires, viewport   (existing, headless, zero deps)
├─ crates/compnode-ui            widgets, interaction, undo, theme, DrawList (headless)
├─ crates/compnode-image              image nodes + evaluator + GPU backend
│   ├─ Evaluator                  topological walk, dirty propagation, per-node cache
│   ├─ TexturePool                texture reuse, memory budget, LRU eviction
│   ├─ gpu/                       wgpu context, shaders, pipelines
│   ├─ cpu/                       reference implementations (tests only)
│   └─ nodes/                     Image File, Blur, Levels, Blend, Resize, Output, ...
└─ crates/compnode-editor                eframe app: node graph + canvas viewer + inspector
```

### Key design decisions

1. **One draw-list boundary.** `compnode-ui` never draws. It emits a `DrawList` of primitives (rounded rect, line strip, triangle, text run, image quad, gradient quad). The egui example converts these into egui meshes.
2. **Widgets are headless state machines.** Each widget implements roughly `measure(width) -> height`, `event(&mut self, ev, ctx) -> Response`, and `draw(&self, &mut DrawList, rect, &Theme)`. They are drawn from primitives, so the look is consistent and not tied to egui widgets.
3. **Text entry uses an edit-overlay.** While a field or text area is being edited, the host (egui) shows a real `TextEdit`, so IME, clipboard, and selection are not reimplemented.
4. **Sockets carry the data model.** A socket schema (name, type, default `Value`, widget hint) lets unconnected inputs show a widget and connected inputs hide it, as in Blender. Socket types drive wire/pin colors and connection compatibility.
5. **Undo/redo via commands.** Every mutation is a `Command` with `apply` and `invert`. Drags and scrubs are grouped into one transaction.
6. **GPU image path on the shared eframe device.** Each node's result is an `RGBA16F` texture. The canvas is displayed through `egui_wgpu` with no CPU readback.
7. **Color pipeline.** Linear float RGBA16F, premultiplied alpha internally; convert to sRGB only on display.
8. **Reuse the existing topological order.** The graph already maintains a topological order and rejects cycles, and the evaluator uses it directly.

### Memory estimate (2048 x 2048)

| Format  | Per texture |
| ------- | ----------- |
| RGBA8   | ~16.8 MB    |
| RGBA16F | ~33.5 MB    |

Fifty cached `RGBA16F` node results would be about 1.7 GB, so the texture pool needs a memory budget, LRU eviction, and reuse of an input's texture when a node has a single consumer. These are arithmetic estimates, not measurements.

---

## 3. Phases

Legend: `[ ]` to do, `[x]` done.

### Phase 0: Foundation and edition 2024 (written, awaiting verification)

- [x] Rename project to `compnode` (crates `compnode`, `compnode-ui`, `compnode-image`, `compnode-editor`)
- [x] Convert the repo to a Cargo workspace (`crates/compnode`, `crates/compnode-ui`, `crates/compnode-image`, `crates/compnode-editor`)
- [x] Set `edition = "2024"`, `rust-version = "1.85"`, `resolver = "3"` through `[workspace.package]`; `[profile.release]` moved to the workspace root
- [x] Edition 2024 migration, reviewed by hand (`cargo fix --edition` could not be run, no toolchain). Findings:
  - [x] `impl Iterator` return types (`Graph::nodes`, `Graph::wires`, `Arena::iter`): lifetime capture rules only widen, no change needed
  - [x] tail-expression temporary scope and `if let` rescoping: only loosen borrows, no change needed
  - [x] `gen` reserved keyword: not used (the arena uses `generation`)
  - [x] no `unsafe`, `extern`, `static mut`, `env::set_var`, `macro_rules!` or `Box<[T]>` iteration in the code base
- [x] Add `#![forbid(unsafe_code)]` to all library crates
- [x] `geom`: `Vec2` helpers (`splat`, `length_sq`, `distance`, `min`, `max`, `lerp`, `Neg`, `AddAssign`, `SubAssign`)
- [x] `geom`: `Rect` utilities (`from_min_size`, `from_center_size`, `center`, `width`, `height`, `area`, `union`, `intersection`, `contains_rect`, `translate`)
- [x] `geom`: `Affine2` transform (identity, translation, scale, rotation, `then`, `inverse`, `transform_point/vector/rect`) and `Viewport::graph_to_screen` / `screen_to_graph`
- [x] New `color` module (a separate module rather than part of `geom`): linear-light `Color`, sRGB8 and hex conversion, premultiply helpers
- [x] Unit tests for all new code (`geom`, `color`, `view`)
- [x] Clippy pre-emptive fixes: `map_or(false/true, ..)` replaced with `is_some_and` / `is_none_or`
- [x] `bench` example takes node counts as arguments (used by the CI smoke run)
- [x] `rustfmt.toml` (style edition 2024) and `.gitignore`
- [x] CI workflow (`.github/workflows/ci.yml`): fmt, clippy `-D warnings`, test on Linux/Windows/macOS, MSRV 1.85 check, bench smoke run
- [x] `README.md` and `CHANGELOG.md` updated
- [x] **Verify on your machine:** `cargo fmt --all` (the original code was not rustfmt-formatted, so expect a one-time reformat; the CI fmt check fails until then)
- [x] **Verify:** `cargo test --workspace` passes (includes the old tests plus the new ones)
- [x] **Verify:** `cargo clippy --workspace --all-targets -- -D warnings` is clean (not run here; the pre-emptive fixes above are guesses at likely lints)
- [x] **Verify:** `cargo run --release -p compnode --example bench -- 1000` runs; then re-run the full bench and record the numbers (the README figures are from the old edition-2021 build)
- [x] **Verify:** `cargo check` passes on Rust 1.85.0 (the MSRV)
- [x] Commit the generated `Cargo.lock`
- **Exit criteria:** existing tests and `examples/bench` pass unchanged on edition 2024. _Not yet met: awaiting the verification items above._

### Phase 1: Socket schema and widgets (headless)

- [x] `Value` enum: Float, Int, Bool, Vec3, Color, String, Enum, Image handle
- [x] `SocketType` with colors and compatibility rules (for example Int to Float)
- [x] `SocketDef { name, ty, default, widget, min, max, step, soft_range }` and `NodeSchema`
- [x] Per-node storage for unconnected input values
- [x] Layout engine with variable row heights (text area and image preview are taller) and node resize
- [x] Update `layout.rs` so pin Y offsets follow actual row heights instead of a fixed `ROW_H`
- [x] Widget trait: `measure`, `event`, `draw`, and `Response { changed, begin_edit, end_edit }`
- [x] Default widgets (each with unit tests for event-to-value round trip and measured height):
  - [x] Float field (drag to scrub, Ctrl/Shift precision, click to type, arrow buttons)
  - [x] Int field (same behavior, integer stepping)
  - [x] Slider (factor fill bar)
  - [x] Toggle (checkbox and switch styles)
  - [x] Button (click and pressed state)
  - [x] Dropdown (enum, popup list, keyboard navigation)
  - [x] Text area (multiline, via edit overlay)
  - [x] Color picker (swatch plus popup with HSV square, hue bar, alpha, hex field)
  - [x] Image preview (host-provided texture id, aspect-fit, checkerboard for alpha)
  - [x] Label (static or computed text, ellipsis truncation)
- [x] Hide a widget when its socket is connected, and when the LOD is below Full
- **Exit criteria:** every widget has passing tests with no window or GPU _Written (tests included) but never run._

### Phase 2: Interaction layer (headless)

- [x] Backend-agnostic `InputEvent` (pointer, buttons, modifiers, scroll, key, text) and `EditorAction`
- [x] Interaction state machine: Idle, PanCanvas, DragNodes, BoxSelect, DragWire, CutLinks, EditWidget
- [x] Selection: click, Shift/Ctrl-click, box-select (grid query), select all, invert, active node
- [x] Multi-node dragging
- [x] Wire dragging from an output or an input pin:
  - [x] hover-highlight of compatible pins
  - [x] drop on empty space opens the add-node menu
  - [x] drag off an input to re-route
  - [x] cycle and type-mismatch feedback before drop
- [x] Snapping: grid snap, pin snap radius, alignment guides
- [x] Link-cut (Ctrl + right-drag across wires)
- [x] Insert a node onto a wire by dropping it over the wire
- [x] Reroute nodes, frames, collapse, and mute
- [x] Clipboard: copy, paste, duplicate (including internal links)
- [x] Add-node menu and search, fed by a node registry
- [x] `Command` (an enum with `apply` and `invert`, so merging is a plain method), undo/redo stack with transactions and merging (for example slider scrubbing)
- [x] Commands for: add/remove node, move, connect/disconnect, set value, resize, paste
- [x] Deterministic replay tests: scripted event sequences produce an expected graph state
- **Exit criteria:** a full editing session can be driven and verified in tests with no window _Written (scripted-session tests in `editor/interact.rs`) but never run._

### Phase 3: Visual design system

- [x] `Theme` struct: colors, radii, spacing, font sizes, line widths; Blender-dark default
- [x] Socket and wire colors per type; header colors per node category
- [x] Node states: normal, hovered, selected, active, muted, error
- [x] Wire styling: width by zoom, highlight when attached to a selected node, dimmed when muted
- [x] Background grid (minor/major lines, fading with zoom)
- [x] Widget style tokens: field fill, hover, pressed, focused outline
- [x] LOD rules: Full (widgets and labels), Compact (header and pins), Dot (flat rect)
- [x] `DrawList` primitives with a stable ordering (grid, wires, nodes, overlays, popups)
- [ ] Optional `serde` feature to load themes from RON or JSON (not done: needs a dependency)
- [x] Themes load from and save to a dependency-free `key = value` text format (`Theme::from_text` / `to_text`)
- **Exit criteria:** the entire look can be changed by editing a theme, with no code changes _Written but never run; themes load from a text file, not RON/JSON._

### Phase 4: egui editor shell

- [x] `crates/compnode-editor` on eframe, with three panels: node graph, canvas viewer, parameter inspector (`app.rs`; the inspector is minimal: errors, timing, flags)
- [x] `DrawList` to `egui::Mesh` converter (build one mesh for wires and one for node bodies; avoid per-shape calls) (`paint.rs`: one mesh per layer; text and images are separate shapes)
- [x] Mesh caching: rebuild only when the graph, theme, or zoom bucket changes; translate for panning (`Editor::draw_key` provides the cache key; the host side is not written) (rebuilds when `draw_key` or the exact zoom changes; panning clones and translates the cached shapes)
- [x] Map `egui::InputState` to `InputEvent`
- [x] Text-edit overlay using egui `TextEdit` while a field is being edited
- [x] File picker (`rfd`) for the Image File node, plus drag-and-drop of image files onto the graph
- [x] Save and load the project (graph and parameters): the headless `project::save` / `load` text format is written; the UI buttons are not (File menu; `.cnp` files)
- [x] Repaint only on demand (idle graph costs nothing) (`draw_key` only changes when something visible changes; the egui side is not written) (eframe repaints on input; the app requests repaints only while loading, evaluating or showing overlays)
- [x] Demo overlay: undo/redo buttons, theme switch, LOD readout, frame time (Edit/View menus and the status bar)
- **Exit criteria:** `cargo run -p compnode-editor` gives a usable editor where every widget works _Not met: only a scripted headless demo exists, the egui shell is not written._

### Phase 5: Image engine (CPU reference first)

- [x] `Image` socket type, plus Float, Color, Int, and Enum parameters
- [x] `Evaluator` using the graph's topological order
- [x] Dirty flags and propagation: changing a parameter marks only that node and its downstream nodes
- [x] Per-node result cache
- [x] CPU reference implementations of the starter nodes (used as test oracles):
  - [x] Image File
  - [x] Solid Color
  - [x] Invert
  - [x] Brightness / Contrast
  - [x] Blur
  - [x] Blend
  - [x] Output
- [x] Golden-image tests with a tolerance
- **Exit criteria:** a small graph evaluates correctly on the CPU, and dirty propagation is covered by tests _Written (evaluator and golden tests) but never run._

### Phase 6: wgpu GPU path

- [x] Share the eframe wgpu device and queue with `compnode-image` (`GpuContext::new(device, queue, budget)`)
- [x] Common `GpuNode` trait (inputs, parameters, encode pass) (not a trait: one `GpuContext::pass` helper plus a `match` on the node kind in `GpuEvaluator::run_node`)
- [x] One shader per starter node (fragment or compute) (fragment shaders in WGSL for every stock node)
- [x] `TexturePool` (used by `GpuContext`; the CPU result cache has its own LRU budget):
  - [x] reuse textures by size and format (generic `TexturePool<T>`, no wgpu types)
  - [x] memory budget with LRU eviction (soft budget: textures in use are never taken away)
  - [ ] reuse an input texture when the node has a single consumer (not done: a fragment pass cannot read and write the same texture, so it needs compute shaders with read-write storage textures, which `Rgba16Float` does not support everywhere)
- [x] Display result textures in egui through `egui_wgpu` with no CPU readback (display pass to `Rgba8Unorm`, registered with `register_native_texture`)
- [x] GPU vs CPU reference comparison tests with a tolerance (`gpu::compare_with_cpu`, run with `cargo run -p compnode-editor -- --selftest-gpu`; it needs a GPU so it is not part of `cargo test`)
- [x] Timing counters per node (GPU timestamps if available) (wall-clock per node; with the profile option on it waits for the GPU after each node; no timestamp queries)
- **Exit criteria:** a multi-node chain updates interactively on a 2K canvas; frame times are measured and recorded _Not met: no GPU backend._

### Phase 7: Canvas viewer and loading

- [x] Output canvas viewer: pan, zoom, fit, 1:1, checkerboard behind transparency (`CanvasView` holds the state and `Prim::Checker` the checkerboard; drawing the texture is the host's job)
- [x] Downscale source images to canvas resolution on load (keep the original path for full-resolution export): the Image File node downscales on load and keeps its path as a parameter
- [x] Async decode on worker threads; the UI thread never blocks (`AsyncLoader` with a pluggable `ImageLoader`; only PPM is built in, an `image`-crate loader is not written)
- [x] Async node thumbnails, generated only for visible nodes (`loader::thumbnail` and `Editor::visible_uids` exist; the scheduler that connects them does not) (`ThumbCache` plus `App::pump_thumbs` over `Editor::visible_uids`)
- [x] Image preview widget bound to node thumbnails (`Editor::set_preview` binds a host image to an output widget; only the Output node uses one) (Image File and Output nodes show a preview footer row)
- **Exit criteria:** loading a large photo does not freeze the UI _Not met: the async loader exists, but there is no UI and no `image`-crate decoder._

### Phase 8: More nodes and export

- [x] Levels / Curves (Levels is written; Curves is not) (Curves is a five-point monotone curve; there is no free-form curve editor widget)
- [x] Hue / Saturation
- [x] Sharpen
- [x] Threshold
- [x] Resize, Crop, Rotate
- [x] Blend modes (multiply, screen, overlay, and so on) with a mask input
- [x] Parameter-driven sockets (for example a Float node feeding a blur radius)
- [x] Export to PNG / JPEG / WebP (PNG and PPM are written with no dependency; JPEG and WebP need the `image` crate and return an error) (JPEG and lossless WebP through the `image-io` feature)
- **Exit criteria:** a realistic photo-editing graph can be built and exported _Partly: the nodes exist and PNG export works; no UI, no JPEG/WebP, no Curves._

### Phase 9: Polish and release

- [x] Keyboard shortcut map and view-all / view-selected (table in `docs/GUIDE.md`)
- [x] Minimap (optional) (`minimap.rs`, click or drag to move the view)
- [x] Keyboard-only editing of widgets and basic accessibility (widgets handle keys when focused via `set_focused`; the editor does not manage focus or Tab order yet) (Tab focus ring in `editor/focus.rs`; the accessibility tree is handed to egui's AccessKit support through `WidgetInfo`)
- [x] Documentation: architecture overview, widget authoring guide, node authoring guide, theme reference (`docs/GUIDE.md`)
- [x] Rewrite `README.md` to match what is actually implemented
- [ ] Benchmarks recorded with the machine they were run on (`cargo run --release -p compnode-editor -- --bench 1000` prints frame times; nothing has been run, so nothing is recorded)
- [x] Changelog and MSRV policy

---

## 4. Performance budgets (targets, to be measured)

| Area                                    | Target                       |
| --------------------------------------- | ---------------------------- |
| Node editor frame (hundreds of nodes)   | well under one 60 Hz frame   |
| Idle graph                              | no repaint, near-zero CPU    |
| Slider drag on a 2K canvas, short chain | interactive (no visible lag) |
| GPU texture memory                      | bounded by the pool budget   |
| File load                               | never blocks the UI thread   |

Record actual numbers in the repository after Phase 6, and update this table.

---

## 5. Risks

| Risk                                        | Mitigation                                                                                    |
| ------------------------------------------- | --------------------------------------------------------------------------------------------- |
| GPU memory use grows with graph size        | texture pool budget, LRU eviction, texture reuse                                              |
| GPU and CPU outputs differ slightly         | compare with an explicit tolerance; keep CPU nodes as oracles                                 |
| egui tessellation cost at large node counts | mesh caching, LOD, per-frame budget; if still needed, a wgpu canvas via `egui_wgpu` callbacks |
| Edition 2024 migration surprises            | migrate first (Phase 0) on the existing small codebase                                        |
| Text editing complexity                     | edit-overlay using egui `TextEdit`                                                            |
| Color and alpha mistakes                    | fix the pipeline early: linear RGBA16F, premultiplied, sRGB on display                        |

---

## 6. Open questions

- [x] Starter nodes: Image File, Solid Color, Invert, Brightness/Contrast, Levels, Hue/Saturation, Threshold, Blur, Sharpen, Blend, Resize, Crop, Rotate, Output, Float, Color
- [x] Export only at canvas resolution for now; the original path stays in the Image File node, so a full-resolution re-evaluation can be added later
- [x] Project file format: a line-based text format that references images by path (see `project.rs`); revisit if projects need embedded data
- [x] The canvas size is configurable (Canvas menu: 512, 1024, 2048)

