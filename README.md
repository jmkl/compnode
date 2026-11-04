# compnode

A node-based image editor in Rust: image-file nodes feed filter nodes, and the
wired result is shown on a 2K canvas. Desktop only.

**Status: every phase of `PLAN.md` is written.** Your formatting, test and clippy
run covered the first round (Phases 0 to 3 and 5). The second round (the egui
shell, the wgpu backend, Curves, minimap, keyboard focus, JPEG/WebP) was
prepared in a sandbox without a Rust toolchain and has **never been compiled**.
The calls most likely to need adjusting are the egui 0.37 and wgpu 30 ones. See
`PLAN.md` for exactly what is done and what is not.

## Workspace

| Crate | Purpose | State |
|---|---|---|
| `crates/compnode` | Headless graph engine: arena, spatial grid, DAG, wires, viewport, geometry, color | existing, extended (row heights, dry-run connect, HSV) |
| `crates/compnode-ui` | Values, schemas, widgets, interaction state machine, undo/redo, theme, draw list, project files | written, unverified, no dependencies |
| `crates/compnode-image` | Image nodes, evaluator with dirty propagation, CPU reference ops, loading, export, texture pool, thumbnails. Features: `image-io` (JPEG/WebP, decoding common formats), `gpu` (wgpu backend) | zero dependencies by default |
| `crates/compnode-editor` (`compnode-editor`) | The egui application: graph, canvas viewer, menus, CPU or GPU evaluation | second round, never compiled |

Requires Rust 1.85 or newer (edition 2024). MSRV policy: the MSRV is raised only
when needed and the change is listed in the changelog.

## First run

```sh
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p compnode-editor                 # the editor (GPU if available)
cargo run --release -p compnode-editor -- --no-gpu     # CPU evaluation, glow renderer
cargo run --release -p compnode-editor -- --selftest-gpu   # every GPU node against the CPU reference
cargo run --release -p compnode-editor -- --bench 1000     # scripted pan/zoom, prints frame times
cargo run --release -p compnode --example bench -- 1000
```

Notes:

* **wgpu versions.** `compnode-image` depends on `wgpu` 25 and the editor on
  `eframe` 0.37, which must use the same `wgpu`. Check with `cargo tree -i wgpu`:
  it must list one version. If it lists two, change the version in
  `crates/compnode-image/Cargo.toml` to the one `eframe` uses. The few wgpu calls
  that differ between releases are marked in `crates/compnode-image/src/gpu.rs`.
* **Linux** needs the GTK 3, xkbcommon and Wayland/X11 development packages for
  the file dialogs and the window (see `.github/workflows/ci.yml`).
* Without the GPU feature: `cargo run -p compnode-editor --no-default-features`.
* Commit the generated `Cargo.lock`.

See `docs/GUIDE.md` for the architecture and the widget, node, GPU and theme guides.

## How the pieces fit

```
InputEvent --> Editor (state machine, undo, selection) --> DrawList --> your renderer
                  |                                                       (egui, tests, ...)
              Document (graph + node values, stable uids) --Change log--> Evaluator --> ImageBuf
```

* `compnode` is the graph engine (arena, grid, DAG, wires, viewport).
* `compnode-ui` never draws: it consumes `InputEvent`s and produces a `DrawList`.
  Whole editing sessions are scripted in its tests.
* `compnode-image` evaluates the graph. Only dirty nodes and their downstream
  nodes are recomputed; results are cached under an optional memory budget.

## The engine (`compnode`)

- `arena`: generational ids (ABA-safe)
- `grid`: incremental spatial grid
- `graph`: nodes and wires, O(degree) edits, DAG enforcement with an incremental
  topological order, O(V+E) `load_edges` for file loading, dry-run `can_connect`,
  per-row node heights
- `wire`: Blender-style bezier wires, tolerance-driven flattening, hit testing,
  and a per-wire polyline cache that holds only visible wires
- `view`: pan/zoom, zoom about the cursor, level of detail
- `geom`, `color`: vectors, rectangles, affine transforms, linear-light color, HSV

### Earlier measurements (edition 2021, original crate, one sandbox)

These numbers predate this workspace move and have not been re-measured. They
cover CPU-side work only (culling, wire geometry, indexing), not drawing.

| graph | frame @ zoom 1.0 | frame @ zoom 0.05 (all visible) |
|---|---|---|
| 1k nodes / 1.4k wires | ~10 us | ~0.13 ms |
| 5k / 7.3k | ~10 us | ~0.93 ms |
| 20k / 29.7k | ~10 us | ~4.2 ms |

Re-run the bench on your machine and record the results.

## Rules of thumb

Bulk-load with `load_edges` (or `Document::load_links`), not repeated `connect`,
when node order is not dependency order.

