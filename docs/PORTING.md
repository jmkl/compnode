# Porting the original compnode nodes

Reference for the nodes ported from the original (egui-snarl) compnode into
this workspace. Nothing here has been compiled yet (see `README.md`).

## Status

| Original node | New id | Where | Status |
|---|---|---|---|
| Image | `image.file` | stock | already existed |
| Output | `image.output` | stock | already existed |
| Route | `core.reroute` | `compnode-ui` | already existed |
| Invert | `image.invert` | stock | already existed |
| Hue / Saturation | `image.hue_sat` | stock | already existed |
| Blend | `image.blend` | stock | already existed; now 17 modes (see below) |
| Curves | `image.curves` | stock | five-point curve; **new Channel input** (RGB, Red, Green, Blue) |
| Exposure | `image.exposure` | `adjust.rs` | ported |
| Color Balance | `image.color_balance` | `adjust.rs` | ported |
| Gradient Map | `image.gradient_map` | `adjust.rs` | ported |
| Mask | `image.mask` | `adjust.rs` | ported |
| Raw Filter | `image.raw_filter` | `rawfilter.rs` | ported |
| Gradient | `image.gradient` | `generate.rs` | ported |
| Noise | `image.noise` | `generate.rs` | ported |
| Texts | | | not yet (phase 4) |
| Remove Background, Remove Background (v2), Face Restore | | | not yet (phase 6) |
| Group | | | **dropped on purpose**: layer with chains of Blend nodes into Output |

## Conventions that changed

* **Pixels.** The original did 8-bit math on sRGB-encoded, straight-alpha
  pixels. Here images are linear-light, premultiplied `f32`. Color Balance,
  Gradient Map, Mask, Raw Filter, Gradient and Noise encode to sRGB, apply
  the original formula and decode again (`cpu::map_srgb`), so they look like
  the original. The consequences: they clamp to `0..1` (no values above
  white), and a gradient or a gray noise level is an sRGB value. **Exposure**
  works in linear light, because EV is a gain, and keeps values above white.
* **Blend modes** (`Normal`, `Multiply`, `Screen`, `Overlay`, `Add`,
  `Difference`, `Darken`, `Lighten`, then the new `Color Dodge`, `Color Burn`,
  `Hard Light`, `Soft Light`, `Exclusion`, `Hue`, `Saturation`, `Color`,
  `Luminosity`) are applied to linear color, like the existing modes, so they
  do not reproduce the original's look exactly. New modes are appended, so
  saved indices of the first eight stay valid. The original's separate
  "Source Over" is `Normal`.
* **Units.** Percent sliders became fractions (`-100..100` is `-1..1`,
  `0..100 %` is `0..1`); angles stay in degrees; EV stays in stops. Select
  fields are dropdowns, hex colors are color sockets, yes/no selects are
  toggles.
* **Mask** takes its luminance from the sRGB-encoded mask, so a 50 % gray
  paints 50 %. (The Mask input of the Blend node still uses linear luminance;
  that is the existing behavior.)
* **Gradient** and **Noise** fill the canvas (the original Gradient had its
  own Width/Height). Gradient positions are `0..1` (the original's
  `-200..200 %` defaults correspond to `0` and `1`).
* **Missing inputs** behave like every stock node: a missing image is a
  transparent canvas; Mask without a mask passes the image through.

## GPU

The blend shader implements all 17 modes and the Curves shader the channel
option; both are covered by `--selftest-gpu`. The other new nodes have no
shader yet: `GpuEvaluator::cpu_bridge` downloads the inputs, runs the CPU
operation and uploads the result, so they work on the GPU backend at the cost
of a readback per node.

## Adding more

Follow `docs/GUIDE.md`, "Authoring a node": a CPU function with a golden test,
a schema in `nodes.rs`, then (optionally) a GPU pass and a case in
`compare_with_cpu`.
