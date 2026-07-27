# Fit log

Every fit worth keeping, with the settings that produced it and the number it
earned. The point is to be able to say later whether a change actually helped,
against the same source, rather than relying on memory.

**Round trip** is always measured the same way: reload the emitted file from
disk, decode it with the ordinary decoder, compare to the resized source in
sRGB. It measures the file, not the fitter's opinion of its own canvas.

`.mathset` files here are large. Keep milestone fits — a stage landing, a
result worth defending — and reproduce the rest from the command line, which
is recorded in full below.

---

## whiterabbit-20260726-greedy

First working fit. Stage 2, greedy placement, no refinement.

| | |
|---|---|
| source | `assets/whiterabbit.jpg`, 451×511 |
| primitives | 24,886 |
| round trip | **30.86 dB** |
| fit time | 3.4 s |
| file | 2.1 MB |
| density | one primitive per 9.3 pixels |

```bash
cargo run --release -- fit ../assets/whiterabbit.jpg out.mathset --preview out.png
```

Defaults at the time: `passes 220 · candidates 32 · pace 0.08 · sigma
0.11→0.0045 · alpha 0.35,1.0 · beta-max 6 · linear error weighting`.

Quality against budget, same settings:

| primitives | round trip |
|---:|---:|
| 5,000 | 26.82 dB |
| 10,000 | 28.21 dB |
| 20,000 | 30.05 dB |
| 24,886 | 30.86 dB |

Notes: the density is the weak point, not the fidelity — see
[docs/fitting.md](../docs/fitting.md#the-honest-limitation). Greedy placement
cannot refine a primitive once placed, so improving the image means stacking
more on top. Stage 3 should move this number a long way down at equal
fidelity, and that comparison is the reason this file is kept.

---

## whiterabbit-20260726-refined

Stage 3. The same fitter output, with 900 iterations of gradient refinement.
No primitive added or removed — only the ten numbers in each row changed.

| | |
|---|---|
| source | `assets/whiterabbit.jpg`, 451×511 |
| primitives | 4,000 |
| round trip | **30.99 dB** |
| refine time | 205 s (900 iterations) |
| file | 358 KB |
| density | one primitive per 58 pixels |

```bash
cargo run --release -- fit ../assets/whiterabbit.jpg m.mathset --budget 4000
cargo run --release -- refine ../assets/whiterabbit.jpg m.mathset out.mathset --iters 900
```

Defaults at the time: `lr_pos 2e-4 · lr_scale 6e-3 · lr_rot 8e-3 · lr_colour
8e-3 · lr_alpha 4e-3 · lr_beta 6e-3 · beta clamped to [0.8, 16]`, Adam,
extents and beta stepping in log space.

**Against the greedy baseline above: the same fidelity from 6.2× fewer
primitives** — 4,000 against 24,886, 30.99 dB against 30.86 dB. That
comparison is the reason the greedy fit was kept.

Gain at matched count, 300 iterations:

| primitives | placed only | refined |
|---:|---:|---:|
| 1,000 | 25.00 dB | 27.21 dB |
| 3,000 | 26.15 dB | 29.63 dB |
| 8,000 | 27.68 dB | 33.17 dB |

Notes: still improving at 900 iterations (30.02 dB at 3,000 primitives after
900, from 29.64 at 300), so these are lower bounds rather than converged
values. The count is fixed — refinement cannot add a primitive where one is
needed or drop a useless one, which is what stage 4 is for.
