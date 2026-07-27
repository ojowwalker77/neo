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
| **two-frame persistence** | **in progress — first result is negative, see below** |
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

**Diagnosis: the capture radius is roughly one primitive's σ — about 2–3 px.**
Position gradients are local. A primitive with σ ≈ 3 px cannot feel that its
content moved 25 px away; the gradient at its own location points at whatever
is under it now. Recolouring in place is a nearer local minimum than
translating, so gradient descent takes it. This is the classic large-motion
problem in optical flow.

This is a property of the method, not a bug. The 2 px row confirms it: within
the capture radius, tracking works.

## What to do next

The fix is not one thing, and the order matters.

**1 · Coarse-to-fine, first.** Standard remedy for large motion. Build an
image pyramid; estimate motion at a scale where the displacement is small
relative to primitive size, then propagate down. Cheapest version: run the
warm-start refinement on downsampled frames first (say 1/8, 1/4, 1/2, full),
carrying positions between levels. Because coordinates are normalized, a set
fitted at one resolution is valid at another — this should be nearly free.
**Try this before anything clever.** It may resolve the whole problem.

**2 · Grouping, which is what the owner asked for.** Primitives have no notion
of belonging to anything, so each one solves its own local problem. If
primitives were grouped, a group could be translated *as a unit* by a coarse
search, moving each member far beyond what its own gradient could reach.
Grouping is not a nicety here — it is the mechanism that makes large motion
solvable, and it is also what would turn the output into "objects with
trajectories" rather than "blobs with drift."

Note grouping needs motion to be meaningful — things that move together belong
together. With one frame it is just clustering by colour and position, which
cannot be validated. Develop it against the synthetic harness where the answer
is known.

Sketch worth trying: after a coarse warm-start, cluster primitives by
(position, displacement) — displacement is the discriminating feature. Then fit
one rigid or affine motion per cluster, apply it to all members, and only then
let per-primitive refinement run. Iterate.

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

## Uncommitted work

Two tools, both working, neither committed yet:

- `mathset/tools/warp.py` — synthesises frame B with known motion, writes
  `a.png`, `b.png`, `truth.json`
- `mathset/tools/persist.py` — compares two same-order `.mathset` files
  against `truth.json` and reports displacement accuracy

Reproduce the negative result:

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

Nothing about the negative result is in `README.md` or `docs/` yet. It should
be written up either way — a clean negative is a result, and the roadmap
already commits to reporting it honestly.

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
