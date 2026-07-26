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
