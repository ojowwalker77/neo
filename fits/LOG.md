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

---

## whiterabbit-20260726-adapted

Stage 4. Prune and split during refinement — the count is no longer fixed.

| | |
|---|---|
| source | `assets/whiterabbit.jpg`, 451×511 |
| primitives | 2,381 |
| round trip | **31.12 dB** |
| time | 207 s (900 iterations) |
| file | 211 KB |
| density | one primitive per 97 pixels |

```bash
cargo run --release -- fit ../assets/whiterabbit.jpg m.mathset --budget 2500
cargo run --release -- refine ../assets/whiterabbit.jpg m.mathset out.mathset \
  --iters 900 --adapt --count 2500
```

Defaults at the time: refinement as before, plus `--prune 0.05` (share of the
set retired per cycle, bottom by worth) and `--cycle 60`.

**Against the two baselines above:**

| | primitives | round trip | pixels per primitive |
|---|---:|---:|---:|
| placed only | 24,886 | 30.86 dB | 9 |
| placed + refined | 4,000 | 30.99 dB | 58 |
| placed + refined + adapted | **2,381** | **31.12 dB** | **97** |

10.5× fewer than placement alone, 1.7× fewer than refinement alone.

Fidelity against count, 900 iterations each:

| primitives | round trip |
|---:|---:|
| 953 | 28.39 dB |
| 1,429 | 29.47 dB |
| 1,905 | 30.42 dB |
| 2,381 | 31.12 dB |
| 3,810 | 33.23 dB |

Notes: still improving at 900 iterations. The worth score that drives pruning
was checked against measured loss change to 0.1% relative error. Splitting is
the only way to add a primitive, so anything placement missed entirely stays
missed — see [docs/parsimony.md](../docs/parsimony.md#known-limitations).

This is the set that stage 5 should start from: the question there is whether
these 2,381 primitives survive into the next frame.

---

## whiterabbit-20260727-motion

Stage 5. One ordered 2,381-primitive set at the original position, after a
recovered 25.95 px group translation, and after ordinary endpoint refinement.

| file | role |
|---|---|
| `whiterabbit-20260727-motion-a.mathset` | original positions |
| `whiterabbit-20260727-motion-b.mathset` | pure movement endpoint; 314 rows change only `x/y` |
| `whiterabbit-20260727-motion-b-refined.mathset` | second frame after 200 refinement iterations |

The A→B transition is evaluated per primitive:

```text
x_i(t) = x_i(A) + t · (x_i(B) - x_i(A))
y_i(t) = y_i(A) + t · (y_i(B) - y_i(A)),  0 ≤ t ≤ 1
```

```bash
cd mathset
cargo run --release -- transition \
  ../fits/whiterabbit-20260727-motion-a.mathset \
  ../fits/whiterabbit-20260727-motion-b.mathset \
  target/motion-half.mathset --t 0.5
cargo run --release -- render target/motion-half.mathset target/motion-half.png
```

`transition` accepts only endpoint pairs with the same metadata, row count,
ordering, and non-position parameters. It cannot silently turn movement into
an appearance cross-fade.

Measured against exact synthetic truth: all 295 true members moved, pre-refine
median position error was 0.40 px, and outside false positives were 0.9%.
After refinement the median error was 0.66 px, with all 295 still recovered
and 1.1% outside false positives.
