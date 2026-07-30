# Temporal curves

How finite video samples `I(x,y,t_k)` become one candidate continuous visual
program `I(x,y,t)`, what that is worth, and what it does not yet establish.

Read [motion.md](motion.md) first. This document assumes a sequence of
`.mathset` states that already share one ordered primitive identity — same
count, same rows, same order, only parameter values differing. Producing that
sequence is stage 5; turning it into a formula is stage 6.

---

## The problem

[motion.md](motion.md) ends with 13 stored states for a 13-frame GIF. That is a
persistent row-indexed description, but it is still a table. Nothing in it says
the wheel turns; the turning is implicit in the differences between adjacent
rows, and reading a value at a time between two stored states means
interpolating.

Stage 6 asks whether one useful smooth program can be recovered from those
states. A finite sample set never identifies a unique continuous function:
infinitely many curves can pass through the same values. Replacing the table
with a function is therefore a model choice to test, not proof that the
underlying continuous motion has been discovered.

Three evidence levels matter:

1. **Sample reproduction:** the program agrees with observed frames at `t_k`.
2. **Unseen-time behaviour:** it agrees with withheld frames at times excluded
   from fitting, then remains coherent where no source sample provides truth.
3. **Interventional meaning:** edits to time controls, temporal functions, or
   primitives have predictable, meaningful effects.

The measurements below establish the first level on the wheel, establish
full-source behaviour for one longer clip, and expose controls relevant to the
third. One clip does not establish cross-source generalisation, physical
motion, or semantic primitive identity from frame reconstruction alone.

## The program

Every primitive parameter becomes a function of `t`. The readable evaluator is:

![the typeset evaluator recovered for a 120-frame GIF](img/one-formula.png)

That typeset expression defines how to evaluate the model, but it is not the
whole payload. A self-contained recovered model also carries every spatial and
temporal coefficient, plus its canvas and timing metadata. The playground's
`Copy model` action appends those arrays and timing knots in a versioned
capsule; without them, the equation describes a family of visuals rather than
this particular clip.

In text, with `d` indexing the ten parameters of a primitive
(`x`, `y`, `σx`, `σy`, `θ`, `r`, `g`, `b`, `α`, `β`) and `i` indexing
primitives:

```
s(t)       = 2π(ωt + τ)

q̃_{i,d}(t) = μ_{i,d} + Σ_{r=1..R} w_{i,d,r} · f_{d,r}(t)

f_{d,r}(t) = A_{d,r,0} + Σ_{h=1..H} [ A_{d,r,h} cos(h·s(t))
                                    + B_{d,r,h} sin(h·s(t)) ]

q_{i,d}(t) = D_d⁻¹( q̃_{i,d}(t) )

I(x,y,t)   = Over_{i=1..N} Gauss2D( q_i(t) )
```

Two factorisations are stacked, and they do different jobs.

**Low rank across primitives.** For one parameter `d`, the `N` per-primitive
trajectories are not independent — when a wheel turns, thousands of primitives
move in a few coordinated ways. Each parameter gets its own set of `R` shared
modes, and each primitive carries `R` scalar weights `w_{i,d,r}` saying how
much of each mode it follows. `μ_{i,d}` is that primitive's mean value over the
clip. The modes come from an eigendecomposition of the frame-by-frame
covariance of the mean-removed trajectories.

**A temporal function for each mode.** Fully sampled periodic controls use a
real Fourier series in the phase `s(t)`, truncated at `H` harmonics. Long
sources fitted at sparse anchors can instead use local linear knots:

```
k(t)       = max { k : p_k ≤ t }
λ(t)       = (t − p_k) / (p_{k+1} − p_k)
f_{d,r}(t) = (1 − λ) C_{d,r,k} + λ C_{d,r,k+1}
```

The knot program is still one evaluable function of `t`; unlike a table of
full primitive states, it stores knot values only for a small set of shared
low-rank modes. Its local support avoids making every timestamp depend on one
global curve. At or beyond the final anchor phase it holds the final knot until
the phase wraps. `ω` and `τ` remain playback controls — rate and offset — not
fitted quantities.

`R` controls spatial rank for either temporal basis and is bounded by
`frames − 1`. Fourier programs also expose `H`, bounded by
`floor((frames − 1) / 2)`; knot programs use the recovered anchor phases
directly instead.

### Time is part of the program

Because time enters explicitly through `s(t) = 2π(ωt + τ)` or the knot phase
`u(t) = wrap(ωt + τ)`, playback controls operate on the recovered program
rather than on a rendered frame buffer:

- the magnitude of `ω` changes rate and its sign changes direction;
- `τ` shifts phase, choosing a different point on the loop as the origin;
- for this periodic basis, `ω = -1` traverses the recovered program backwards.

These controls are implemented by the local evaluator and survive in the
portable coefficient capsule. They demonstrate that the representation is
executable and directly controllable. They do **not** prove that a negative
`ω` reconstructs a physically valid reverse process, or that any newly sampled
time is correct relative to the source world.

## Fitting in the right domain

The fit is not performed on raw parameter values. Three of the ten parameters
are wrong to interpolate linearly, so each parameter is transformed before the
basis is fitted and inverted afterwards — the `D_d` and `D_d⁻¹` in the program:

| parameters | domain | why |
|---|---|---|
| `σx`, `σy`, `β` | `ln q` | strictly positive; a linear fit can cross zero and produce an invalid primitive |
| `r`, `g`, `b` | linear light (`sRGB⁻¹`) | averaging in sRGB darkens mixtures; light adds linearly |
| `θ` | unwrapped angle | orientation is circular, so a raw fit tears at the ±π seam |
| `x`, `y`, `α` | identity | already linear |

This is the same set of domain choices the keyframe sampler in
`mathset/src/timeline.rs` makes between two adjacent states, applied to a whole
trajectory at once instead of a single interval.

## What was measured

Source: the 13-frame `assets/wheel.gif`, through the 13 recovered persistent
states in [`fits/wheel-20260727-real/`](../fits/wheel-20260727-real/) —
2,285 primitives, one ordered identity, published in
[motion.md](motion.md) at 39.73 dB mean.

Every figure below is a decoder render of an evaluated program, scored against
the real GIF frame in sRGB. The decoder is the ordinary `mathset render`; it
never sees the source.

The wheel harness is `scripts/score-formula.mjs`; the long-source harness is
`scripts/validate-gif.mjs`. They import the TypeScript extraction and score
through the Rust/WGPU decoder. The playground and both scripts are untracked,
so these numbers cannot be regenerated from a clean checkout. Treat them as
measurements taken locally, not as a standing engine gate.

### The formula against the table it replaces

| model | coefficients | relative RMS | mean fidelity | worst frame |
|---|---:|---:|---:|---:|
| 13 stored keyframe states | 296,050 | — | 39.73 dB | 38.16 dB |
| formula, `H`=6 `R`=12 | 298,610 | **0.0000%** | **39.73 dB** | **38.16 dB** |
| formula, `H`=6 `R`=3 | 91,790 | 0.9033% | 35.32 dB | 34.40 dB |
| formula, `H`=6 `R`=1 | 45,830 | 1.3922% | 33.96 dB | 31.64 dB |

At full rank the program is a **near-lossless re-encoding**. Relative RMS in
coefficient space is `3.62 × 10⁻⁸`. Across the 13 decoder renders, 110 of
2,269,696 pixels differ from the stored keyframe renders — 0.0048% — and the
largest RGB-channel difference is 3/255. Both versions have the same 39.73 dB
mean and 38.16 dB worst-frame fidelity against the source at the reported
precision.

Read the first two rows together, though: 298,610 coefficients against 296,050
stored numbers. **At full rank this is not compression.** It is a change of
form — from a table that must be interpolated to a function that can be
evaluated — and the count goes very slightly up. Compression only appears when
`R` is truncated, and then it is paid for in fidelity: rank 3 is 3.2× smaller
and costs 4.4 dB.

`relative RMS` is measured in the transformed coefficient space described
above, over all 2,285 × 10 trajectories at the sampled phases. It is not a
pixel measurement, which is why the fidelity columns are reported beside it.

### Against frames it never saw

The table above scores the program at exactly the times it was fitted to. That
is the easy question. The real one is whether the curve is right *between*
samples.

Fitted on the 7 even-numbered states, scored against the 6 odd-numbered frames
that were withheld:

| harmonics | held-out relative RMS | train fidelity | unseen fidelity | gap |
|---|---:|---:|---:|---:|
| `H`=3 | 7.11% | 37.09 dB | 31.67 dB | **5.42 dB** |
| `H`=2 | 3.51% | 35.05 dB | 33.40 dB | 1.64 dB |
| `H`=1 | 3.91% | — | — | — |

This is an honest negative, and it is the clearest signal in the document.
`H`=3 fits its training frames better than `H`=2 and reconstructs the withheld
ones **worse** — 5.42 dB worse than what it saw, against 1.64 dB for the
smaller model. Larger `H` is buying agreement with the samples by bending
between them.

With 7 training frames, `H`=3 gives 7 Fourier terms for 7 constraints: the
series is exactly determined and has no slack anywhere. That it interpolates
its samples perfectly says nothing about the frames in between, and the
measurement confirms it does not.

So the correct statement of the current result is narrow:

- the program **reproduces every sampled state to decoder precision**, with
  the same source fidelity at the reported precision;
- the program **is not yet established as accurate at unsampled times**, and
  the evidence points the other way when harmonics are pushed;
- persistent row indices make the program evaluable, but do **not** establish
  that a row is one enduring physical point or semantic part.

### The 120-frame source

`assets/giphy.gif` is 120 frames at 600×500 over 3.6 s. It is a local test
source and is not committed to the repository. It is fitted at 32 motion-aware
anchors rather than every frame, and the recovered program is the one
photographed at the top of this document: 3,000 primitives, `H`=12, `R`=16.

![the 120-frame source beside its reconstruction](img/giphy-side-by-side.png)

It plays convincingly, and the settled poses match closely. The fast-motion
frames show visible streaking that the source does not have. Those 88
unanchored frames are now measured, rather than judged from playback.

The split is deterministic: order the unanchored frames by time, alternate 44
into validation and 44 into test, choose capacity using validation only, then
read test once. Every score below renders the evaluated state with the native
Rust/WGPU decoder at the model canvas and compares it to the real source.

| representation | coefficients | validation mean / worst | test mean / worst |
|---|---:|---:|---:|
| 32 full anchor states, parameter-aware interpolation | 960,000 | 32.80 / 28.99 dB | 32.80 / 28.47 dB |
| periodic Fourier, `H=12 R=16` | 514,000 | 31.25 / 24.67 dB | 31.25 / 25.81 dB |
| non-periodic cosine, `H=24 R=16` | 514,000 | **31.90 / 28.41 dB** | **31.94 / 28.32 dB** |
| non-periodic cosine, `H=24 R=24` | 756,000 | 32.52 / 29.17 dB | 32.49 / 29.07 dB |
| local linear knots, `R=24` | 757,712 | 32.64 / 28.97 dB | 32.63 / 28.43 dB |

The cosine `H=24 R=16` row is the clean model-selection result: it won on the
validation half, then improved the original Fourier model by 0.69 dB mean and
2.51 dB worst-frame fidelity on the untouched test half at the same
coefficient count.

The local-knot row is the best size/fidelity trade found afterwards. It uses
21% fewer values than the full anchor table and gives up 0.17 dB mean fidelity
on these frames. Because that model family was proposed after the test result
had been inspected, its score is development evidence, not a second untouched
claim. A new source is required to confirm it.

## Where the code is

The temporal extraction is **not part of the tracked `mathset` engine.**
`mathset/src/` contains the keyframed timeline sampler and parameter-aware
transitions, but no temporal curve-fitting code. The temporal search and
program evaluator are in the local playground, in TypeScript:

- `app/neo/model.ts` — basis extraction, rank truncation, evaluation, and the
  LaTeX rendering of the program;
- `app/neo/extraction.worker.ts` — runs it off the UI thread;
- `app/neo/capsule.ts` — the portable coefficient payload.

The repository's `.gitignore` excludes those playground files. The public
research record and Rust engine are tracked here; the temporal program
extraction described above is local. Anything that is meant to become an
engine guarantee has to be reimplemented against the Rust engine, and the
measurements in this document were produced by driving the TypeScript
extraction and the native decoder together.

## What would settle it

In rough order of how much each would change the picture:

1. **Run the same protocol on a new real clip.** Choose capacity on validation
   and open test once. The knot model must generalise beyond the source that
   motivated it.
2. **Fit the curve against pixels, not against recovered coefficients.** The
   present fit minimises error in coefficient space and inherits whatever the
   per-anchor spatial fits produced. A refinement pass with the loss on the
   rendered frame would let the curve trade one primitive's error against
   another's.
3. **Test interventions, not only playback.** Controlled edits need expected
   outcomes, ideally on synthetic scenes with known object and motion
   structure. A plausible edit is not evidence of semantic identity.
4. **Move the extraction into `mathset`.** While it lives in an untracked
   playground, none of the numbers above are reproducible from a checkout and
   none of them can be defended by `tools/verify.sh`.
