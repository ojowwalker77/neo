# Demo

What works right now, what to run, and what is worth looking at.

This file grows as capabilities land. Each section is something you can
reproduce from a clean checkout.

```bash
cd mathset && cargo build --release
```

---

## 1 · An image made of forty-five numbers

**2026-07-26**

[`sets/five.mathset`](mathset/sets/five.mathset) is a text file containing five
rows of nine numbers. Open it — the whole thing fits on a screen. It is the
entire image.

```bash
cd mathset && cargo run --release -- render sets/five.mathset out.png
```

Each row is one soft ellipse:

```
x     y     sx    sy    theta       r     g     b     a
0.38  0.42  0.220 0.045 -0.5235988  0.95  0.55  0.15  0.85
```

Where it is, how long and how thin, how tilted, what colour, how opaque. That
row is the orange stroke.

Worth doing: change a number and re-render. Set `sy` to `0.15` and the stroke
fattens. Set `theta` to `0` and it lies flat. Move the last row to the top of
the list and the white dot vanishes behind the blue — because the list is a
paint order, not a set.

**The decoder never sees a source image.** There is nothing to copy from. The
picture is computed from those forty-five numbers and nothing else.

---

## 2 · The numbers have no resolution

**2026-07-26**

Same file. Render it eight times larger:

```bash
cd mathset && cargo run --release -- render sets/five.mathset big.png --scale 8
```

4096×4096. Zoom all the way into any edge — it is clean.

This is the part that matters. Nothing was upscaled or interpolated. There was
no 512×512 image to enlarge; there were five equations, and they were evaluated
at sixty-four times as many points. The detail at 4096 is not invented and not
smoothed, it was always implied by the numbers and simply not resolved before.

A compressed image cannot do this. It has nothing to draw on. This is the
difference between a picture *of* something and a description *of* something.

To confirm it is not an illusion, the 8× render can be shrunk back down and
compared against a native 512×512 render:

```
8x downsampled back and compared: 60.86 dB
```

The same description, sampled at two densities, agreeing.

---

## 3 · One number from soft to hard

**2026-07-26**

A Gaussian cannot make an edge — its falloff is smooth everywhere. Real images
are full of edges, so the primitive carries a shape exponent `β` that controls
how abruptly it ends.

```bash
cd mathset && cargo run --release -- render sets/beta.mathset out.png --scale 3
```

Five primitives, identical in position, size, and colour. Only `β` differs —
`0.5, 1, 2, 4, 12` from left to right. They run from a long-tailed blur through
an ordinary Gaussian to a nearly hard-edged disc: a **13× sharper** transition
across that range, for one extra number per primitive.

Note how far the leftmost one reaches — at `β = 0.5` the tails extend twelve
standard deviations and visibly wash into its neighbour. That is the cost of a
soft primitive, and the reason `β < 1` is rarely the right tool. Detail should
come from more primitives, not longer tails.

The point is not that hard edges are better. It is that the fitter will get to
choose, per primitive, so soft skin and a crisp sleeve cost exactly the same.

---

## 4 · The image is a consequence of the file

**2026-07-26**

A renderer producing a plausible picture proves very little. So there are two
decoders, sharing no code:

- Rust and WGSL, running `f32` on the GPU, with hardware blending
- Python standard library, running `f64` on the CPU, written from the written
  spec alone

```bash
./mathset/tools/verify.sh
```

```
── sets/five.mathset
    max channel difference : 2 / 255
    psnr                   : 56.05 dB

── sets/beta.mathset
    max channel difference : 1 / 255
    psnr                   : 57.66 dB
```

Different language, different precision, different hardware, different blending
path — same image, to within a rounding error of one or two parts in 255.

That agreement is the real claim. The picture is not an artifact of a
particular renderer or a particular machine. It is a consequence of the
numbers, and anyone with the file can recompute it.

---

## 5 · A photograph, written down as math

**2026-07-26**

The first four sections used sets written by hand. This one is read off a real
photograph.

```bash
cd mathset && cargo run --release -- fit ../assets/whiterabbit.jpg out.mathset --preview out.png
```

3.4 seconds. **24,886 primitives, 30.86 dB.**

That number is measured the hard way: the emitted file is reloaded from disk
and decoded by the ordinary decoder — the one that has never seen the
photograph — and *that* is compared against the source. Nothing the fitter
believes about its own canvas is taken on trust.

Open the output. It is the same format as the five-line file in section 1,
just longer. Every row is still nine or ten numbers describing one ellipse,
still in normalized coordinates, still with no pixel anywhere in it. Which
means:

```bash
cd mathset && cargo run --release -- render out.mathset big.png --scale 4
```

The photograph now renders at 1804×2044 — four times the resolution it was
fitted at. Not upscaled. The primitives were re-evaluated at the finer
sampling, and edges that were two pixels wide in the source are now smooth
curves, because in the description they were never pixels at all.

The kept fits and their numbers live in [fits/](fits/).

**What is honestly wrong with this result:** 24,886 primitives for 230,461
pixels is one primitive per nine pixels. That is too dense to call a
description of the image. The cause is structural — greedy placement can never
adjust a primitive after placing it, so the only way to fix an error is to
stack another primitive over it, and the count inflates without the
description improving. See
[docs/fitting.md](docs/fitting.md#the-honest-limitation).

Section 6 will be gradient refinement, and the number to watch is not the dB —
it is the dB *per primitive*.

---

## Next

See [docs/roadmap.md](docs/roadmap.md) for what comes after, and why the
two-frame test is the one that decides whether any of this extends to video.
