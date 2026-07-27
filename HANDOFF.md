# Handoff

State of the project, the finding that matters, and what to do next.
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

## Where it stands

Stages 1–4 are done, verified, documented and pushed. See `README.md` sections
1–8, `docs/`, and `fits/LOG.md`.

| stage | state |
|---|---|
| `.mathset` format + decoder | done, two independent decoders agree at 56–62 dB |
| fitter (greedy placement) | done, 24,886 primitives → 30.86 dB |
| gradient refinement | done, 4,000 → 30.99 dB (6.2× fewer) |
| prune + split | done, 2,381 → 31.12 dB (10.5× fewer) |
| **two-frame persistence** | **in progress — spatially separate translation groups are recovered automatically** |
| temporal curves | not started |

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
automatically recoverable. Touching objects with different motion and
non-translational motion are not.

## What to do next

**1 · Split touching motions.** `track-change` already separates spatially
disconnected change components. A connected region containing two motions
still gets one transform. Estimate local correspondence inside it, split where
displacement disagrees, and merge adjacent pieces whose motion agrees. Keep
common motion as the discriminating signal; clustering one frame by colour and
position alone cannot be validated as object identity.

**2 · Generalize the group transform.** Once translation and membership are
recovered together, add rigid rotation, then affine motion, checking each
against `warp.py --rotate` and `--scale`. Do not jump straight to the most
flexible transform; it can hide bad membership the way colour refinement hid
bad motion.

**3 · Only then temporal curves.** Fitting parameters as functions of `t` is
meaningless until primitives reliably follow their content.

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
