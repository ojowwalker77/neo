# Fitting

Stage 2: an image in, a `.mathset` out. Greedy placement — propose primitives,
keep the ones that measurably improve the reconstruction, never revisit a
placed one.

```bash
cd mathset && cargo run --release -- fit ../assets/whiterabbit.jpg out.mathset --preview out.png
```

## The loop

Each pass:

1. **errmap** — per-block error of the current reconstruction against the
   target. Drives the size boost, so proposals get larger where the image is
   still badly wrong.
2. **score** — one GPU thread per candidate. Derives the candidate from an
   integer hash, integrates over its own footprint, and solves for the colour
   that minimises the error it would leave behind.
3. **reduce** — one thread per cell. Takes that cell's best candidate,
   re-scores it honestly (see below), and drops it unless it still improves
   the image.
4. **apply** — blends the survivors into the canvas.

Applying uses `splat.wgsl` — the decoder's own shader — onto a canvas in the
decoder's own format. A primitive is therefore scored against exactly the
pixels the decoder will produce from the emitted file. If the fitter had its
own copy of the compositing rule, every accept decision would be made against
a canvas that does not exist.

Winners are appended in cell-index order, which is the instance order they
were blended in, which is the order the decoder replays them in. Order is
preserved by construction rather than by care.

## Colour and score in closed form

For one candidate, over its footprint, with `A = a·G(u,v)` the effective
opacity at a pixel, `C` the current value, `T` the target, and `D = C − T`:

```
N − T = A(c − C) + D

delta = Σ w[(N−T)² − (C−T)²]        the D² term cancels
      = c²·S2 − 2c·S1 + S3

S1 = Σ w·A(A·C − D)
S2 = Σ w·A²
S3 = Σ w(A²C² − 2A·C·D)
```

Setting `d(delta)/dc = 0` gives the optimal colour directly:

```
c* = S1 / S2          and then      delta* = S3 − S1²/S2
```

One traversal of the footprint yields both. There is no search over colour,
and no candidate is ever evaluated at a colour that is not its best possible
one. The quadratic is exact for any `c`, so clamping `c*` into `[0,1]` costs
accuracy in the colour but never in the reported delta.

## Two findings that cost most of the quality

Both were found by measurement, and both were counterintuitive enough to be
worth recording.

### Selecting the best of N noisy estimates is biased

A candidate's footprint is subsampled — stepping across it rather than
visiting every pixel — so its delta is an *estimate*. Taking the minimum over
`M` such estimates does not select the best candidate. It preferentially
selects whichever candidate the sampling noise happened to flatter, and the
bias grows with `M`.

The symptom was unmistakable: raising the candidate count made the fit
dramatically *worse*.

| candidates | selection only | with verification |
|---:|---:|---:|
| 8 | 24.99 dB | 26.41 dB |
| 32 | 13.82 dB | 27.25 dB |
| 64 | 10.91 dB | 27.69 dB |

Early passes were accepting large primitives that actively damaged the image,
and every later pass spent itself repairing them.

The fix is cheap. Select with the cheap estimate, then re-integrate the winner
on a **denser, offset lattice** — statistically independent of the estimate
that selected it — and reject it unless the honest delta is still negative.
That runs once per cell rather than once per candidate, so it costs a fraction
of the scoring pass. With it, quality is monotone in candidate count, as it
should have been.

### Primitives placed in the same pass invalidate each other

Every candidate is scored against the canvas as it stands at the start of the
pass. If two accepted primitives overlap, neither one's predicted improvement
survives the other.

`pace` is the fraction of cells permitted to place in a pass, and it is by far
the most sensitive knob in the fitter:

| pace | round trip |
|---:|---:|
| 0.04 | 28.75 dB |
| **0.08** | **30.05 dB** |
| 0.16 | 29.67 dB |
| 0.40 | 29.02 dB |
| 1.00 | 12.46 dB |

Letting every cell place collapses the fit outright. Below the optimum the
loss is different — the size schedule runs out before the budget is spent.

## Measured parameters

All against `assets/whiterabbit.jpg` at 451×511, capped at 20,000 primitives,
varying one setting at a time from the defaults.

| candidates | | beta-max | |
|---:|---:|---:|---:|
| 4 | 28.01 dB | 1 (plain Gaussian) | 29.68 dB |
| 8 | 28.78 dB | 3 | 29.97 dB |
| 16 | 29.47 dB | **6** | **30.05 dB** |
| 32 | **30.05 dB** | 16 | 30.00 dB |
| 64 | 30.55 dB | | |

More candidates keeps helping and keeps costing; 32 is the point where the
curve flattens relative to the time.

The shape exponent earns its place, but only just — **+0.37 dB** over
Gaussians alone. That is expected at this stage: `β` is sampled at random and
never refined, so the fit can only stumble onto a good value rather than seek
one. This number is worth re-measuring once stage 3 exists, because gradient
refinement is precisely what would let `β` be *chosen*.

### The perceptual weight loses

Weighting the linear-space error by the squared slope of the sRGB transfer
function makes the fit minimise error *as seen* rather than error in photons.
It is well motivated and it measurably hurts:

| weighting | round trip |
|---|---:|
| linear (default) | 30.05 dB |
| perceptual | 29.57 dB |

The reason is that the weight comes from linearising sRGB about the target
value, which is only valid when the residual is small. Early in a fit the
residual is enormous, and the weight — which spans a range of roughly 800×
between shadows and highlights — sends the fit chasing dark regions on the
strength of an approximation that does not hold there.

Kept behind `--perceptual` rather than deleted. A version clamped to a
narrower dynamic range, or applied only once the fit is close, may still be
worth having.

## Where it lands

| primitives | round trip |
|---:|---:|
| 5,000 | 26.82 dB |
| 10,000 | 28.21 dB |
| 20,000 | 30.05 dB |
| 24,886 (converged) | 30.86 dB |

3.4 seconds for the converged fit.

## The honest limitation

24,886 primitives for 230,461 pixels is **one primitive per nine pixels**.
That is a poor description, and the roadmap already names the failure mode it
is approaching: a set dense enough to be pixels in disguise.

The cause is structural, not a tuning problem. Greedy placement cannot
*refine*. Once a primitive is down its parameters are frozen, so the only way
to improve the image is to put another primitive on top of it. Each new
primitive corrects the last one's error rather than describing the image, and
the count inflates without the description getting better.

Nothing in this stage's parameters fixes that. Stage 3 — computing
`∂L/∂param` and descending — is the fix, and the number to watch is not the
round trip but the round trip *per primitive*.

## Reproducing the numbers

```bash
cd mathset
cargo run --release -- fit ../assets/whiterabbit.jpg out.mathset --budget 20000 --candidates 64
```

The reported round trip is measured by reloading the emitted file from disk
and decoding it with the ordinary decoder — not from the fitter's own
in-memory canvas. The file is the deliverable, so the file is what gets
measured. Text rounding costs nothing detectable: the on-disk and in-memory
figures agree to two decimals.
