# Handoff

State of the native math engine, the local Neo playground, the findings that
matter, and what to do next.
Delete this file whenever it stops being current.

## What the project is actually for

**The goal is math for movement.** Given a video, output a compact set of
parameters and functions of `t` that reproduce the motion; given that set,
replay it.

The still-image work — stages 1 to 4, all of `README.md` — is **groundwork
only**. It exists so that a frame can be represented as a few thousand
primitives rather than a pixel grid, which is the prerequisite for asking
whether those primitives persist over time. Do not treat image fidelity or
file size as the deliverable. Compression is explicitly not the goal (we lose
to JPEG by 7×, measured; that is fine and not worth optimising).

The motivating intuition, from the owner: a few hundred characters of
Processing code can render a convincingly organic animated creature
(@yuruyurau's work), so movement is describable by math — the open question is
whether it can be *recovered* from footage rather than hand-authored forward.

The owner clarified the product goal more concretely after the engine work:

> Given an image or GIF, recover the equations and coefficient tables that
> build it. Given those equations and tables, reproduce the visual without
> retaining a copy of the source.

The wheel GIF is a control, not the product. The product is an inverse visual
compiler.

## Immediate handoff: the local Neo playground

There is now a working local playground at `http://localhost:3000/`. It has two
directions:

- **IMAGE / GIF → MATH**: upload a real source, watch the native spatial fit,
  then receive typeset equations plus their complete coefficient tables;
- **MATH → IMAGE / MOTION**: paste a complete copied Neo model and reproduce
  it independently.

The playground is deliberately black, sparse, and process-visible. Do not
replace real progress with fake loading animation. The user specifically wants
to see placement, tracking, refinement, fidelity, primitive count, and temporal
extraction while they run.

### Interface rules — the UI was overhauled on 2026-07-28

The previous acid-lime terminal look was replaced with a **monochrome
instrument**. The rules are load-bearing; keep them:

- **One neutral ramp, no accent hue.** The only colour anywhere on the screen
  comes from the source frames and the Gaussian reconstruction, so the chrome
  can never compete with the thing being judged. `rendered-html.test.mjs`
  asserts the old `#dfff47` / `#e8b33a` never come back.
- **Emphasis is inversion, not colour.** The primary action is a white block
  with black text; that is the only loud element. Failures are signalled by
  structure and position, never by red.
- **Two type roles, and the split means something.** Sans for chrome and
  prose, mono with tabular numerals for measured values, filenames, and
  literal source. Sentence case everywhere — no all-caps, no wide tracking.
  The display face is KaTeX's own Computer Modern, set at reading size: the
  interface has no typographic voice so the mathematics can have all of it.
- **The seam is the progress indicator.** The 1px rule between the viewer and
  the equations fills as the fit runs (`.seam`, driven by `--progress`). It
  replaces the old glowing meters. There is exactly one progress affordance.
- **Layout does not move.** Viewer left, equations right, in both directions.
  The direction switch changes which half is the input and how the split is
  weighted; it never reorders the furniture.

Status tokens on the wire are still upper-case (`EXTRACT`, `TEMPORAL BASIS`,
`EQUATION FIT`, `READY`, `ERROR`) because the logic branches on them.
`statusText()` maps them to human copy at the point of display — add new
tokens there rather than shouting at the user.

### Source-control boundary — read this before touching anything

The user explicitly asked to gitignore the playground. The entire local site is
ignored: `app/`, `scripts/`, `tests/`, `public/`, package/build configuration,
and `.openai/`.

Their absence from `git status` does **not** mean the playground work is absent.
It means it exists only in this workspace and is intentionally not part of the
tracked engine commit. Do not claim it was committed or pushed. Do not remove
those ignored files, and do not unignore or publish them unless the owner asks.

The playground originally left `README.md` unchanged. On 2026-07-28 the owner
explicitly asked to document the temporal-formula milestone, so `README.md`,
`docs/temporal.md`, `docs/roadmap.md`, and `fits/LOG.md` now carry that result
and its held-out limitation.

### Local architecture

```text
image / GIF
    ↓ browser decode worker
source RGBA frames
    ↓ local native service, one job at a time
persistent ordered .mathset states
    ↓ coefficient-domain transform
per-primitive parameter trajectories
    ↓ low-rank spatial modes + periodic or local time basis
μ / w / temporal coefficient tables
    ↓ Neo model capsule appended as LaTeX comments
self-contained copy / paste model
```

Relevant ignored files:

- `app/neo/NeoPlayground.tsx` — full interaction and progress UI;
- `app/neo/model.ts` — static and temporal equation programs;
- `app/neo/capsule.ts` — complete portable coefficient payload;
- `app/neo/renderer.ts` — WebGL2 Gauss2D decoder;
- `app/neo/gif.ts`, `gif.worker.ts`, `image.worker.ts` — off-main-thread input
  decoding;
- `app/neo/anchors.ts` — motion-aware GIF anchor selection;
- `app/neo/extraction.worker.ts` — temporal basis extraction off the UI thread;
- `scripts/fit-server.mjs` — local native fitting service on port 3011;
- `scripts/dev.mjs` — starts the fitter and Vinext app together;
- `tests/*.test.mjs` — equation, capsule, anchor, GIF, and rendered-app gates.

Run it with:

```bash
npm run dev
```

This starts the UI on `http://localhost:3000/` and the native fitter on
`http://127.0.0.1:3011/`. The app proxies `/api/fit` and
`/api/fit/health` to that local service. The service requires the release
binary at `mathset/target/release/mathset`.

### GIF → math: current implementation

The old 120-frame path fitted every frame sequentially and took about
15 minutes. It now performs native fits only at up to 32 motion-aware anchors:

- anchor density follows measured pixel change, with a small time floor so
  quiet portions remain covered;
- the original frame delays become exact sample phases in the temporal fit;
- only selected frames are encoded to intermediate PNGs;
- the first state uses greedy placement plus 60 refinement iterations at
  320 px;
- subsequent anchors use `track-change` at 192 px / 4 levels, then 30
  refinement iterations at 320 px;
- every emitted state is scored consistently at 384 px;
- if an anchor falls below `max(30 dB, first-frame score − 1.25 dB)`, a
  20-iteration quality guard runs automatically.

For the 120-frame `assets/giphy.gif` used during development, the selected
source indices were:

```text
0, 4, 8, 12, 16, 20, 23, 27, 30, 34, 37, 40, 44, 47, 51, 55,
58, 62, 66, 70, 74, 78, 82, 86, 90, 94, 98, 102, 106, 111, 115, 119
```

Temporal extraction fits in transformed coefficient space: positive
extents/β in log space, colour in linear-light space, and orientation on an
unwrapped angular path. A source whose every frame was fitted keeps the
periodic Fourier control. An undersampled source now uses 24 low-rank spatial
modes with one parameter-aware linear knot per anchor. This is still one
continuous function of time, but its temporal behaviour is local rather than
forcing one global periodic curve through arbitrary footage.

This is a mathematically legitimate sampling optimisation, not a hardcoded
wheel shortcut. The 120-frame source has now been scored at every original
timestamp through the native Rust/WGPU decoder. The 88 unanchored frames were
alternated across the timeline into 44 validation and 44 test frames. The
original Fourier `H=12, R=16` model scored 31.25 dB validation / 31.25 dB
test; a same-budget cosine `H=24, R=16` model selected on validation scored
31.94 dB on the then-untouched test set.

The parameter-aware 32-anchor interpolation baseline scores 32.80 dB on that
test half with 960,000 stored primitive parameters. The new rank-24 local-knot
program scores 32.63 dB with 757,712 coefficients: 21% fewer values for a
0.17 dB mean-fidelity cost. That knot family was designed after inspecting
the first experiment, so its number is exploratory, not a new untouched test.
Use a new unrelated clip for the next claim.

The smaller 13-frame wheel control has now been scored. At `H=6`, `R=12`, its
formula matches the stored timeline's 39.73 dB mean and 38.16 dB worst-frame
source fidelity. Relative coefficient RMS is `3.62 × 10⁻⁸`; 110 of 2,269,696
rendered pixels differ from the keyframe renders, by at most 3/255 in one RGB
channel. This is a near-lossless re-encoding, not compression: the formula uses
298,610 coefficients for a 296,050-number table.

It also establishes the unresolved boundary. Fit on 7 wheel states and scored
on the 6 withheld real frames, `H=3` reaches 37.09 dB on training frames and
31.67 dB unseen; `H=2` reaches 35.05 dB and 33.40 dB. More harmonics fit the
samples better and the gaps worse. The formula works at sampled times; its
accuracy between them is not yet established.

### Still image → math: current implementation

PNG, JPEG, and WebP uploads up to 40 MB use one real native spatial fit. A
still image becomes a genuinely static equation program:

```text
H = 0
R = 0
q̃_i,d = μ_i,d
I(x,y) = Over_i Gauss2D(q_i)
```

It does not wrap the image in a fake one-frame motion model. The source is
decoded off the main thread, the native service solves 3,000 ordered Gaussian
primitives, and the UI shows a static transport state. The smoke source
`assets/whiterabbit.jpg` produced 3,000 primitives at 28.35 dB on a 339×384
canvas. The output is approximate because it is the real Gaussian
reconstruction, not copied pixels.

### Copy / paste contract

Readable LaTeX is not sufficient to reproduce a visual: the recovered spatial
and temporal coefficient values are the actual model. `COPY MODEL` appends a
versioned `% NEO_MODEL_V1_BEGIN … END` capsule containing every Float32
coefficient array:

- `means` (`μ`);
- low-rank per-trajectory `weights` (`w`);
- temporal `modeCoefficients` (Fourier `A/B`, cosine `A`, or knot `C`);
- irregular `samplePhases` used by a knot program;
- canvas, background, timing, basis, rank, fidelity, and source metadata.

Plain header-only LaTeX is rejected as incomplete. A complete pasted capsule
reconstructs independently of the original upload. Capsule decoding validates
array lengths, finite values, rank, harmonics, primitive count, and a 48 MB
coefficient-byte ceiling.

The editor currently exposes only `H`, `R`, `ω`, and `τ`. It is not yet a
general LaTeX compiler. For an existing bound model, changing equation
structure without matching new coefficient tables is rejected rather than
silently generating unrelated output.

### Local verification

The latest local gates are:

```bash
npm run lint
npm test
```

`npm test` performs the production build and 24 tests. Those tests cover the
wheel timeline, equation recovery, static image programs, complete model
copy/paste including irregular knots, temporal validation/model selection,
motion-anchor selection, GIF decoding, server-rendered UI, and shipped control
data.

The native image endpoint was also exercised end-to-end with
`assets/whiterabbit.jpg`; it returned one state, phase 0, 3,000 primitives,
and a complete event.

### Sites / hosting boundary

The playground is **not published in Sites**. `.openai/hosting.json` contains
no `project_id`.

More importantly, publishing the current UI alone would be dishonest:
`scripts/fit-server.mjs` spawns the local Rust `mathset` binary, which a Sites
deployment cannot execute. A hosted version needs the native fitter packaged
behind a real remote job API (with streaming progress and cancellation), or
the engine ported to a supported hosted runtime. Do not deploy a UI whose
upload button cannot reach the real solver.

## Where it stands

Stages 1–4 are done, verified, documented and pushed. See `README.md` sections
1–8, `docs/`, and `fits/LOG.md`.

| stage | state |
|---|---|
| `.mathset` format + decoder | done, two independent decoders agree at 56–62 dB |
| fitter (greedy placement) | done, 24,886 primitives → 30.86 dB |
| gradient refinement | done, 4,000 → 30.99 dB (6.2× fewer) |
| prune + split | done, 2,381 → 31.12 dB (10.5× fewer) |
| **two-frame persistence** | **in progress — translation and rigid tests pass; real wheel persists through 13 keyframes** |
| temporal curves | full-source scored on one long clip; new-source generalisation is open |

Verification gates, both should stay green:

```bash
./mathset/tools/verify.sh                                            # decoder conformance
cd mathset && cargo run --release -- gradcheck ../assets/whiterabbit.jpg target/tiny.mathset
```

`gradcheck` checks all ten analytic derivatives against central finite
differences of an independently written CPU forward model, and checks the
prune worth score against measured loss change. It exits non-zero above 5%
relative error. Run it after any change to `refine.wgsl`.

## The finding that matters

**Warm-starting a set onto the next frame recovers the image without tracking
the motion.**

Test: fit frame A (2,381 primitives, 30.52 dB). Synthesise frame B by
translating a rectangular patch of frame A by a known amount. Warm-start the
same primitives onto frame B and refine. Compare positions.

Fidelity looks like a success — 26.05 dB at the start, **30.90 dB after 400
iterations**, matching frame A's own quality, same primitive count.

The displacements say otherwise:

| true shift | mean displacement inside patch | median position error |
|---:|---:|---:|
| 2.0 px | +1.25 px | 0.89 px |
| 5.1 px | +1.77 px | 3.72 px |
| 10.2 px | +0.88 px | 9.42 px |
| 20.4 px | +0.29 px | 20.16 px |
| 25.6 px | −0.36 px | 25.69 px |

At 25.6 px the median error *equals the full shift* — the primitives did not
move at all. Only 6.8% of them registered any displacement. They stayed put
and **changed colour** to match the new content.

Primitives outside the patch behaved correctly (median error 0.78 px), so the
machinery is not broken. It is doing exactly what it was asked to do.

**Diagnosis: the capture radius is on the order of the smaller primitive
extents — about 2–3 px.** Inside the patch, geometric-mean `σ` is 2.29 px at
the 10th percentile, 3.27 px at the 25th, and 5.13 px at the median. Position
gradients are local. A small primitive cannot feel that its content moved 25 px
away; the gradient at its own location points at whatever is under it now.
Recolouring in place is a nearer local minimum than translating, so gradient
descent takes it. This is the classic large-motion problem in optical flow.

This is a property of the method, not a bug. The 2 px row confirms it: within
the capture radius, tracking works.

## What changed after the negative

**The plain pyramid was tried, and it does not fix the problem.**

`refine --max-side N` now makes a 1/8 → 1/4 → 1/2 → full-resolution run
possible. Its 1/8 level reaches 51.18 dB while recovering only +0.04 px; the
full chain ends at 30.88 dB but recovers only +0.78 px of the 25.55 px shift,
with 25.16 px median error. Coordinates and extents are both normalized, so
downsampling multiplies displacement and `σ` by the same factor; their ratio
and the capture problem remain.

Inflating each primitive's support by 2×–16× while freezing appearance was also
tried. It creates scattered movement and 10–36% false positives outside the
patch, not coherent translation.

**A coarse group translation does cross the basin.**

`track-group` takes a rectangle that supplies group membership, then searches
for that group's rigid 2D translation against frame B at 128 px. It does not
read the truth shift. On the 25.55 px case it recovers +26.35 px in 0.1 s.
After 200 ordinary refinement iterations:

| measurement | result |
|---|---:|
| reconstruction | 30.82 dB |
| mean displacement | +25.90, +0.11 px |
| median error | 0.84 px |
| moved members recovered | 295 / 295 |
| outside false positives | 4 / 2,086 (0.2%) |

The search also recovered 2.0, 5.1, 10.2, and 25.6 px horizontal shifts plus a
20.4 × 10.2 px diagonal shift within 0.16–0.80 px, with all supplied members
moved and no outside movement before refinement.

`track-change` now removes the supplied rectangle. It joins changed pixels
through 8-connected coarse cells, then uses sub-pixel A↔B frame correspondence
to recover one translation per spatially separate component. The imperfect
primitive reconstruction no longer judges its own motion.

On the original case it selects 314 primitives — all 295 true members plus 19
boundary extras — and recovers +25.95 px without truth. After 200 refinement
iterations:

| measurement | result |
|---|---:|
| reconstruction | 30.81 dB |
| mean displacement | +25.83, −0.01 px |
| median error | 0.66 px |
| moved members recovered | 295 / 295 |
| outside false positives | 23 / 2,086 (1.1%) |

On a two-region synthetic test, it independently recovers +19.96, 0.00 px and
−14.35, −9.92 px, within 0.48 and 1.03 px of truth. All 166 true members move;
outside false positives are 32 / 2,215 (1.4%).

This proves a stronger boundary: spatially separate translation groups are
automatically recoverable. Touching objects with different motion are not;
rigid rotation is handled separately below with supplied membership.

## Rigid validation and the real wheel timeline

`track-rigid` now searches a known group's translation and rotation together.
It rotates primitive centres around the rectangle's centre and also adds the
group angle to each primitive's own `theta`. It writes a small
`.motion.json` descriptor with membership, pivot, translation, and angle so
the path between endpoint mathsets is explicit.

Exact test: fit frame 0 of `assets/wheel.gif`, then use `warp.py` to rotate the
wheel rectangle by a known +12°. The A set has 2,285 primitives and reconstructs
at 40.02 dB. At native 512 px search resolution:

| measurement | result |
|---|---:|
| true / recovered rotation | +12.000° / +11.988° |
| recovered translation | 0.00, 0.00 px |
| median / 90th percentile position error | 0.02 / 0.03 px |
| median primitive orientation error | 0.012° |
| outside primitives moved | 0 / 89 |

The saved transition evaluates arcs rather than endpoint chords:

```text
p_i(t) = c + R(tφ) · (p_i(A) - c) + t·d
θ_i(t) = θ_i(A) + tφ
```

Applying only that rigid model to the real GIF was rejected. It rendered an
almost stationary crisp wheel and did not reproduce the supplied motion.

The accepted real-GIF path keeps the same ordered 2,285 rows across all 13
frames. Frame 0 is fitted once. Each later state is warm-started from the
immediately preceding state and refined for 100 iterations at native 512×341
resolution with position/orientation learning rates at one quarter of their
defaults and no adaptation, so rows are never added, removed, or reordered.
Extent, colour, opacity, and edge softness remain free to carry the visible
radial smear, darkness, and local spoke changes.

| measurement | result |
|---|---:|
| primitive count in every state | 2,285 |
| mean decoded fidelity | 39.73 dB |
| worst decoded fidelity | 38.16 dB |
| median adjacent position step | 0.57–0.68 px |
| 90th percentile adjacent position step | 1.32–1.47 px |

`sample-timeline` evaluates between the 13 states. It uses linear position and
opacity, shortest-path orientation, geometric positive extents and `β`, and
linear-light colour. It emits another ordinary mathset; it does not blend
rendered pixels.

The durable files are:

- `fits/wheel-20260727-rigid-a.mathset`;
- `fits/wheel-20260727-rigid-b.mathset`;
- `fits/wheel-20260727-rigid.motion.json`.
- `fits/wheel-20260727-real/timeline.json`;
- `fits/wheel-20260727-real/frame-00.mathset` through `frame-12.mathset`.

README section 10 and `docs/motion.md` contain the measurements, visible
real animation, parameter transition rules, and reproduction commands.

## What to do next

**1 · Repeat the full-source gate on unrelated footage.** The playground now
scores every original timestamp through the Rust/WGPU decoder and keeps
validation separate from a final test split. Run the complete 32-anchor fit on
a new real clip, choose model capacity on validation only, then open the test
result once. Do not tune the model family after reading that result. The
current local-knot result is promising but was developed on the only long clip
in the workspace.

**2 · Make the fitter remotely deployable before publishing Sites.** Preserve
the truthful streamed stages and cancellation contract. The obvious boundary
is a job service around the native Rust binary; the hosted UI should upload the
source, receive NDJSON/SSE progress, and download only recovered math states.
Do not replace the solver with a fake browser reconstruction to make deployment
easy.

**3 · Attach rigid estimation to inferred groups.** Rotation is currently
proved with rectangle-supplied membership and pivot. `track-change` discovers
translation groups but still estimates only translation. Use the changed
component as a rigid candidate, check that one transform explains it, and
retain the descriptor only when it beats translation by a meaningful margin.
This is also the likely path to better large-motion anchors in arbitrary GIFs.

**4 · Split touching motions.** `track-change` already separates spatially
disconnected change components. A connected region containing two motions
still gets one transform. Estimate local correspondence inside it, split where
displacement disagrees, and merge adjacent pieces whose motion agrees. Keep
common motion as the discriminating signal; clustering one frame by colour and
position alone cannot be validated as object identity.

**5 · Scale.** Check affine scale against `warp.py --scale`; do not infer it
from the real wheel, where no exact geometric truth exists.

**6 · Decide the repository boundary with the owner.** The playground is
currently ignored by explicit request. If it becomes the product rather than a
local experiment, ask before moving it into tracked source, splitting it into
another repository, committing it, pushing it, or deploying it.

## Relevant context for whoever picks this up

- **Do not start on real footage.** A plausible-looking result and a correct
  one are indistinguishable by eye — that is exactly how the negative result
  above would have been missed. Use `tools/warp.py`, which synthesises a
  second frame with an exact, recorded displacement field.
- **The 2D case is harder than the 3D case, on purpose.** 3D/4D Gaussian
  splatting gets persistence for free because a real scene exists underneath
  and only the camera moves. In flat 2D there is no geometry to anchor
  identity. That is why this corner is under-explored, and it is also the
  interesting corner for the project's argument.
- **Human vision does this with strong priors** — rigidity, common fate,
  surface continuity. Our fitter has none. That is the real gap, and it points
  at grouping as the missing prior rather than at a better optimiser.
- **The image stack is not novel** (3D Gaussian Splatting 2023; GaussianImage /
  Image-GS 2024 for the 2D case; GES for the shape exponent β; 4D-GS for
  temporal). The movement question in flat 2D is much less covered. Do not
  claim novelty on the image side.

## Reproduction

Reproduce the negative:

```bash
cd mathset
python3 tools/warp.py ../assets/whiterabbit.jpg target/mo \
  --patch 0.35,0.25,0.30,0.30 --shift 0.05,0.0
cargo run --release -- fit target/mo/a.png target/mo/A0.mathset --budget 2500
cargo run --release -- refine target/mo/a.png target/mo/A0.mathset target/mo/A.mathset \
  --iters 600 --adapt --count 2500
cargo run --release -- refine target/mo/b.png target/mo/A.mathset target/mo/B.mathset \
  --iters 400
python3 tools/persist.py target/mo/A.mathset target/mo/B.mathset target/mo/truth.json
```

Then discover the changed region, recover its translation, and verify that
refinement keeps it:

```bash
cargo run --release -- track-change target/mo/a.png target/mo/b.png \
  target/mo/A.mathset target/mo/grouped.mathset \
  --threshold 2 --range 0.10 --levels 7 --max-side 128
cargo run --release -- refine target/mo/b.png target/mo/grouped.mathset \
  target/mo/B-grouped.mathset --iters 200
python3 tools/persist.py target/mo/A.mathset target/mo/B-grouped.mathset \
  target/mo/truth.json
```

Recover exact rigid rotation from the wheel frame:

```bash
cd mathset
magick ../assets/wheel.gif -coalesce target/wheel/frames/frame-%02d.png
python3 tools/warp.py target/wheel/frames/frame-00.png \
  target/wheel/synthetic-rotation \
  --size 512x341 --patch 0.16015625,0.01953125,0.62890625,0.62890625 \
  --shift 0,0 --rotate 12
cargo run --release -- fit target/wheel/synthetic-rotation/a.png \
  target/wheel/A0.mathset --budget 3000 --max-side 512
cargo run --release -- refine target/wheel/synthetic-rotation/a.png \
  target/wheel/A0.mathset target/wheel/A.mathset \
  --iters 600 --adapt --count 2400
cargo run --release -- track-rigid \
  target/wheel/synthetic-rotation/b.png target/wheel/A.mathset \
  target/wheel/B.mathset \
  --rect 0.16015625,0.01953125,0.62890625,0.62890625 \
  --motion target/wheel/wheel.motion.json \
  --range 0.02 --angle-range 18 --levels 9 --max-side 512
python3 tools/persist.py target/wheel/A.mathset target/wheel/B.mathset \
  target/wheel/synthetic-rotation/truth.json
```

The full result, including the negative, failed pyramid, positive group search,
scope boundary, and motion-field plots, is now in `README.md` section 9 and
`docs/motion.md`.

The durable endpoint files are:

- `fits/whiterabbit-20260727-motion-a.mathset` — original positions;
- `fits/whiterabbit-20260727-motion-b.mathset` — recovered second positions,
  with only `x/y` changed;
- `fits/whiterabbit-20260727-motion-b-refined.mathset` — refined frame B.

Use `mathset transition A.mathset B.mathset out.mathset --t F` to evaluate the
linear per-primitive position field for `0 ≤ F ≤ 1`. The command rejects
non-position changes, so it cannot disguise a cross-fade as motion.

## House rules observed so far

- Every number quoted is measured through the real decoder after reloading the
  emitted file from disk, never from an in-memory canvas or the fitter's own
  opinion.
- Negative results get documented, not buried — see the perceptual weighting
  in `docs/fitting.md` and the two adaptation failure modes in
  `docs/parsimony.md`.
- Milestone fits are archived in `fits/` with the exact command and settings in
  `fits/LOG.md`, so later changes can be judged against a baseline rather than
  against memory.
- `README.md` is the demonstration and the front page; each section ends in
  something you can *see*. Keep that property — a stage whose result is
  invisible needs a comparison built for it.
