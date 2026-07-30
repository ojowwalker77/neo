# Roadmap

The order here is chosen so that each stage produces something checkable before
the next one depends on it. The research hypothesis — whether movement can be
recovered as a small function of `t` — begins at stage 5, becomes an explicit
temporal program at stage 6, and remains open beyond both.

Stages are marked done only when verified, not when written.

The gates are deliberately separate. Reproducing the observed samples
`I(x,y,t_k)` is the first. Behaving correctly at withheld or otherwise unseen
times is the second. Remaining meaningful when the program is edited is the
third. Passing the first does not imply the other two, and none of them can be
replaced by visual plausibility alone.

---

## 1 · Format and decoder — **done**

Define `.mathset`, then build a decoder that reads one and paints it, with no
knowledge of any source image.

The decoder came before the fitter deliberately. It is the fixed point
everything else is measured against: a fitter is only as trustworthy as the
thing that replays its output, so the replay has to be independently correct
first.

Verified by two independent implementations agreeing at 56–62 dB, and by the
1× / 8× check. See [verification.md](verification.md).

## 2 · Fitter — **done**

Image in, `.mathset` out.

Round-trip a photograph: fit, decode, compare against the original. This
produces the first real number for how well the representation works, and the
first honest sense of how many primitives a real image needs.

Approach is greedy placement — propose primitives where the reconstruction is
most wrong, keep the ones that improve it. This is good at *where something is
missing* and poor at *making one thing exactly right*, which is stage 3's job.

Watch: reconstruction fidelity, primitive count, and where the primitives
cluster. If they pile up along edges, `β` is not doing its job and the
primitive needs another look before stage 5.

## 3 · Gradient refinement — **done**

Greedy placement cannot refine. Once a primitive is down, its parameters are
fixed for good, and improving the image means adding more primitives on top —
which inflates the count without improving the description.

Refinement computes `∂L/∂param` analytically for each primitive and descends.

Done: 6.2× fewer primitives at equal fidelity, gradients checked against
finite differences. See [refining.md](refining.md).

This stage is also a prerequisite rather than an optimization: stage 6 fits
coefficients of functions, which is not something greedy proposal can do at all.

It did need compute shaders and atomics, as expected — a primitive spans
several tiles, so gradients from different tiles land on the same parameters.
WGSL has no float atomics, so accumulation uses a compare-exchange loop on the
bit pattern rather than a fixed-point scale that would have needed tuning.

## 4 · Parsimony — **done**

With placement and refinement both working, ask how few primitives can carry an
image at a given fidelity.

This is not about file size. A set with 3,000 primitives that each correspond
to something in the image is a description. A set with 500,000 sub-pixel ones
is pixels in disguise — it would reconstruct perfectly and would be worthless
for everything downstream, because nothing in it persists or moves coherently.

Low count is evidence against a sub-pixel raster in disguise. It does not prove
that an individual primitive corresponds to a physical point or semantic part.

Done: 2,381 primitives at 31.12 dB against 24,886 at 30.86 — one primitive per
97 pixels rather than per 9. See [parsimony.md](parsimony.md).

## 5 · Two frames — **in progress, and the real test**

Fit frame A. Then fit frame B *initialized from A's set*, and measure how much
had to change.

The hypothesis gains evidence if most primitives survive with structured
parameter changes and those changes agree with known motion truth. A stable row
order alone is weaker: B can reconstruct well while rows remain in the wrong
places or change appearance instead. If B's fit wanders into an unrelated set,
the representation has no executable temporal identity and video has collapsed
to repeated still-image fitting.

The first answer is an instructive negative: independent warm-started
primitives reconstruct frame B but do not track motion beyond a 2–3 px capture
radius. A plain resolution pyramid preserves the displacement-to-`σ` ratio and
does not fix it.

A coarse rigid search recovers 2–26 px translation to sub-pixel accuracy when
group membership is supplied, and ordinary refinement preserves that identity.
Spatially separate changed regions can now also be inferred directly from the
two source frames without truth and translated independently. With supplied
membership and pivot, a joint translation-and-rotation search recovers an exact
+12° wheel test as +11.988°, with 0.02 px median position error, and replays
the path as circular arcs. The real 13-frame wheel then keeps one ordered
2,285-row set across every frame and reconstructs the sequence at 39.73 dB
mean fidelity through an executable keyframed timeline. Automatic rotating
membership, touching objects with different motion, and scale remain unsolved,
so this stage is not done. See [motion.md](motion.md).

## 6 · Temporal curves — **in progress**

Given persistence, a primitive's parameters become functions of `t`.

At that point a clip is not a sequence of descriptions. It is one description
with time in it, and motion is a handful of coefficients rather than a
difference between frames.

The representation uses `R` shared spatial modes per parameter, so thousands
of primitives moving coherently are described by a few modes rather than a few
thousand independent curves. Fully sampled loop controls use a periodic
Fourier basis. Long clips fitted at sparse anchors use parameter-aware local
knots for those shared modes, avoiding a single global curve through arbitrary
footage.

Two results, in the order they matter.

The first is positive and unambiguous. On the 13-frame wheel the program is a
**near-lossless re-encoding** of the keyframe timeline: `H`=6, `R`=12, 39.73 dB
mean and 38.16 dB worst, matching the stored states at the reported precision,
with `3.62 × 10⁻⁸` relative coefficient RMS. Only 110 of 2,269,696 rendered
pixels differ, by at most 3/255 in one RGB channel. Truncating to rank 3 gives
3.2× fewer coefficients at 35.32 dB.

The 88 unanchored frames of the 120-frame clip are now scored. A cosine model
chosen on 44 validation frames reached 31.94 dB mean / 28.32 dB worst on the
then-untouched 44-frame test half. A local rank-24 knot program developed from
that experiment comes within 0.17 dB of full anchor interpolation with 21%
fewer values.

So the stage now proves full-source temporal reconstruction for one real clip,
not general recovery. The knot family needs the same one-shot validation/test
protocol on a new unrelated source, and edits to the recovered program still
need intervention tests. Extraction also lives in a local TypeScript
playground rather than in `mathset/`, so it is not yet a shipped engine stage.
See [temporal.md](temporal.md).

## 7 · The exhibit

A browser build, so the thing can be shown rather than described. `wgpu`
targets WebGPU from the same source as the native decoder, so the renderer does
not need rewriting.

What makes the point, in order of effect:

- a slider that composites primitives one at a time, so an image assembles out
  of a few dozen ellipses in front of the viewer
- the same set rendered far beyond its fit resolution, sharp
- for video, the primitive count held constant while time advances
- editable `ω` and `τ` controls that change rate, direction, and phase at the
  program level

Those controls demonstrate executability. They are not, by themselves,
validation of unseen-time behaviour or meaningful semantic editing.

---

## Open questions

**Hard-edged content.** The current primitive suits photographic material.
Synthetic content — flat regions with crisp boundaries — would be served far
better by a primitive with exact edges. The format's `primitive` field is
deliberately a tag rather than an assumption, so a second type can be added
without a migration.

**Occlusion boundaries.** A one-sided hard edge — the primitive multiplied by a
soft half-plane — would represent silhouettes properly. It is two more
parameters and a real jump in complexity, so it waits until the fitter can
demonstrate that edges are where primitives are being wasted.

**Band limiting.** Point sampling per fragment is fine for the primitives that
exist by hand. A fitter producing very small, very sharp ones would alias. See
[math.md](math.md#resolution-independence).

## On the larger claim

The motivation is the idea that an image is not fundamentally a grid of
samples, and that what is on a screen is closer in kind to what reaches an eye
than it is usually taken to be.

What this project can actually establish is narrower: that sampled images and
simple video can be approximated by deterministic programs; that those programs
can imply coherent structure at spatial and temporal coordinates not stored as
pixels; and, eventually, that controlled edits have predictable consequences.
Model-implied structure is not source-ground-truth detail, and frame
reproduction is not physical or semantic recovery. The larger claim is a
direction to test, not a conclusion, and it is more persuasive stated that way.
