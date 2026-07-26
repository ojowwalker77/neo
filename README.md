# neo

An engine that reads an image and writes down the math that draws it.

Not a copy of the image — a set of numbers and a rule for evaluating them, from
which the image can be reconstructed at any resolution, on any machine, without
the original ever being present.

## The idea

Everything on a screen is pixels. But pixels are a *sampling* of something, not
the thing itself. If you can recover the continuous description underneath —
the shapes, their extents, their colours — then the pixels become one possible
rendering of it rather than the substance of it.

The test for whether you have actually recovered a description, rather than
merely compressed a picture, is simple: **fit at one resolution, render at
another, and see whether real detail appears.** A compressed picture cannot do
this. It has nothing to draw on. A description can, because it was never made
of pixels in the first place.

A video is then a sequence of such descriptions — and the interesting claim is
that consecutive frames should share most of their primitives, with motion
appearing as those primitives' parameters changing smoothly over time. Movement
stops being a difference between two grids of numbers and becomes a small
function of `t`.

That is the direction. It is being built one verified step at a time.

## Status

| stage | state |
|---|---|
| `.mathset` format | defined |
| decoder — math in, image out | working, cross-verified |
| fitter — image in, math out | working, 30.9 dB on the test image |
| gradient refinement | not started |
| two-frame persistence | not started |
| temporal curves | not started |

The two-frame stage is the real test of the thesis. Everything before it is
groundwork.

## Quick start

Requires a recent Rust toolchain and a GPU with Metal, Vulkan, or DX12.

```bash
cd mathset
cargo run --release -- render sets/five.mathset out.png
```

Five primitives, forty-five numbers, one image. Then render the same file
eight times larger and zoom in:

```bash
cargo run --release -- render sets/five.mathset big.png --scale 8
```

Nothing was upscaled. The edges are clean at 4096×4096 because the primitives
were re-evaluated there, not stretched.

See [DEMO.md](DEMO.md) for what is worth looking at, and
[docs/](docs/) for the format and the math behind it.

## Documentation

- [docs/format.md](docs/format.md) — the `.mathset` file, normatively
- [docs/math.md](docs/math.md) — the primitive, compositing, colour, and the derivations
- [docs/fitting.md](docs/fitting.md) — how an image becomes a set, and what was measured
- [docs/verification.md](docs/verification.md) — how correctness is established
- [docs/roadmap.md](docs/roadmap.md) — the staged plan and what each stage proves

## Layout

```
mathset/
  src/format.rs        the spec in code — parsing and validation
  src/splat.wgsl       the decoder — evaluates the primitive, composites
  src/render.rs        GPU setup, offscreen target, readback
  sets/                hand-written .mathset files
  tools/reference.py   independent CPU implementation, for cross-checking
  tools/verify.sh      build and check everything
assets/                test images
docs/
```

## Verify

```bash
./mathset/tools/verify.sh
```

Renders every set twice — once on the GPU in Rust and WGSL, once on the CPU in
pure Python — and diffs the results. The two implementations share no code. If
they agree, the image is a consequence of the file rather than of either
renderer.
