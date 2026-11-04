# compnode guide

Parts of what follows (the egui app, the GPU backend, focus, minimap, Curves)
have **not been compiled yet** (see the status in `README.md`). Where the guide and the code disagree, trust the code
and fix the guide.

## Architecture

* `compnode` (graph engine): `Graph<N>` with generational ids, spatial grid,
  incremental topological order, wires and a `Viewport`.
* `compnode-ui` (headless editor): `Document` wraps a `Graph<NodeData>`. Nodes
  have a stable `uid` (arena ids change when a node is deleted and restored, uids
  do not), so commands and the clipboard refer to uids. `Editor` owns the
  document, viewport, selection, widgets and undo history. It consumes
  `InputEvent`s (screen space) and emits `EditorAction`s and a `DrawList`.
* `compnode-image`: `Evaluator` reads the document's change log
  (`Document::take_changes` -> `Evaluator::apply_changes`), marks the changed nodes
  and everything downstream dirty, and recomputes only those on `evaluate`.

### A frame in a host

```text
for ev in host_events { editor.handle_event(&translate(ev)); }
for action in editor.drain_actions() { /* open menu, show text field, ... */ }
changes = editor.doc.take_changes(); evaluator.apply_changes(&editor.doc, &changes);
let image = evaluator.evaluate(&editor.doc, output_uid);
editor.draw(&mut draw_list);               // graph space; apply editor.view
```

Use `editor.draw_key()` to decide whether the previous draw list can be reused.

## Interaction cheat sheet

| Input | Effect |
|---|---|
| Left drag on empty space | box select (Shift add, Ctrl subtract) |
| Left click / Shift-click node | select / toggle |
| Left drag node | move selection (Ctrl inverts grid snap; frames carry their contents) |
| Drag from a pin | wire; drop on a pin to connect, on empty space to open the add menu |
| Drag off a connected input | re-route (dropping on empty space removes the link) |
| Drop a lone node on a wire | insert it into the wire |
| Ctrl + right drag | cut links |
| Right click | add-node menu (`EditorAction::OpenAddMenu`) |
| Middle drag / Alt + left drag | pan; scroll zooms about the cursor |
| Drag a node's right edge | resize (a frame's bottom-right corner resizes both ways) |
| Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y | undo, redo |
| Delete / X | delete selection |
| Ctrl+C, Ctrl+V, Ctrl+D | copy, paste at cursor, duplicate |
| A, Ctrl+I | select all, invert selection |
| M, H | mute, collapse selection |
| F | frame the selection |
| R | insert a reroute on the hovered wire |
| Home, `.` | view all, view selection |

## Authoring a widget

Implement `compnode_ui::widget::Widget`:

* `measure(width, value) -> f32`: the row height the widget wants (at least
  `ROW_H`; the editor lays rows out from this).
* `event(ev, rect, value) -> Response`: positions are already in graph space.
  Edit `value` directly and report it: `changed`, `begin_edit`/`end_edit` around a
  gesture (the editor groups everything between them into one undo step),
  `clicked`, `text_edit` to ask the host for a text field (answered through
  `commit_text` / `cancel_edit`).
* `captures() -> bool`: return true while a drag or popup is active, so the editor
  keeps routing events to you.
* `draw(dl, rect, value, label, theme)`: push primitives; use `Layer::Popup` for
  popups. Take every color and size from the `Theme`.

Register it by adding a `WidgetKind` variant and an arm in `make_widget`.
Tests need no window: build a rect, send `InputEvent`s, assert on the value.

## Authoring a node

1. Describe it: `NodeSchema::new("image.my_node", "My Node", Category::Filter)`
   with `.input(SocketDef::...)` / `.output(...)`. Row `i` holds input `i` and
   output `i`; a row may not carry both an input widget and an output widget
   (`NodeSchema::validate` checks this). Outputs show no widget unless you ask for
   one (for example `ImagePreview`).
2. Implement it: a `NodeOp` with `schema()` and `eval(&mut EvalCtx)`. Read inputs
   with `ctx.image(i)`, `ctx.float(i)`, `ctx.int(i)`, `ctx.color(i)`,
   `ctx.enum_index(i)`, `ctx.string(i)` (connected values win over the node's own).
   Return `Data::Image` / `Data::Val` per output.
3. Register it: `Evaluator::register(Box::new(op))`, then
   `evaluator.register_schemas(&mut registry)`.

Images are linear-light RGBA `f32`, premultiplied (`ImageBuf`). Write a CPU
function first and give it a golden test; a GPU shader would be checked against it.

## Theme reference

A `Theme` is plain data: `Col::*` colors, `Num::*` sizes, one color per
`SocketType` and per `Category`. `Theme::to_text` / `Theme::from_text` use
`key = value` lines (`col.node_body = #303030`, `num.radius = 6`,
`socket.image = #f4a9a8`, `header.filter = #3b5d7a`); a file may override just a
few keys. Call `Editor::set_theme` to apply one.

## Project files

`project::save(&doc)` / `project::load(&text, registry)`: a line-based text format
(nodes, parameter values, links). Images are referenced by path; no pixels are
stored.

## The editor application

`cargo run --release -p compnode-editor`. Menus: File (new, open, save, export
PNG/JPEG/WebP, load/save theme), Edit (undo, redo), View (show all, fit, 1:1,
minimap, preview the selected node, grid snap, dark/light), Canvas (size,
backend, GPU profiling). Drop image files on the graph to create Image File
nodes, drop a `.cnp` file to open a project. Images load on worker threads; a
node shows "loading ..." until its image arrives, so the UI never blocks.

* `paint.rs` turns a `DrawList` into one egui mesh per layer (rounded rects,
  strokes, thick polylines and gradients are tessellated there); text and images
  stay separate shapes. The shapes are cached against `Editor::draw_key()` and the
  exact zoom, and translated while panning.
* `backend.rs` hides whether the CPU `Evaluator` or the `GpuEvaluator` is used.

## The GPU backend (`gpu` feature of `compnode-image`)

* `GpuContext` owns the device and queue (the app shares eframe's), a bind
  group layout used by every pass, pipelines compiled on first use, and a
  `TexturePool<GpuImage>`. `GpuContext::pass(name, wgsl_body, format, inputs, params, size)`
  runs one full-screen fragment pass: up to three input textures `t0..t2`, sixteen
  float parameters `u.p[0..4]` (as `vec4`s), the output size in `u.size`.
* Textures are `Rgba16Float`, linear light, premultiplied alpha, like the CPU
  reference. Display goes through one more pass to `Rgba8Unorm` (sRGB, premultiplied).
* Adding a node: write the WGSL body (see `INVERT`), add a `match` arm in
  `GpuEvaluator::run_node`, and add a case to `compare_with_cpu`. The CPU function
  in `cpu.rs` stays the oracle; `--selftest-gpu` reports the largest difference per
  case (tolerance 5e-3, half floats).
* Limits: blur radius is a loop in the shader (large radii are slow), and passes
  never reuse their input texture.

## Keyboard and accessibility

Tab and Shift+Tab move the focus ring over the widgets of the selected nodes (or
all nodes). Left/Right step numbers and sliders, Up/Down change a dropdown,
Enter and Space toggle, press or open, Escape clears the focus.
`Editor::accessibility()` lists nodes and widgets with role, label, value and
rectangle; the app hands it to egui's AccessKit support through `WidgetInfo`.

## Minimap

`minimap::Minimap` maps between graph and minimap coordinates, draws itself into
a screen-space `DrawList`, and `Editor::minimap_click` moves the view.
