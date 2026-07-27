# Roadmap

The order here is chosen so that each stage produces something checkable before
the next one depends on it. The interesting question — whether motion is a
small function of `t` — sits at stage 5, and everything before it exists to
make that question answerable.

Stages are marked done only when verified, not when written.

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

Low count matters as evidence that the fit found real structure.

Done: 2,381 primitives at 31.12 dB against 24,886 at 30.86 — one primitive per
97 pixels rather than per 9. See [parsimony.md](parsimony.md).

## 5 · Two frames — **next, and the real test**

Fit frame A. Then fit frame B *initialized from A's set*, and measure how much
had to change.

The thesis holds if most primitives survive with small parameter changes — if
the difference between two frames is a modest, structured perturbation of the
same description. It fails if B's fit wanders off into an unrelated set of
primitives, which would mean the representation has no temporal identity and
video is just repeated still-image fitting.

This is cheap to reach and decides whether the rest is worth building. It gets
its own honest answer either way.

## 6 · Temporal curves

Given persistence, a primitive's parameters become functions of `t` — a low
order polynomial or a few spline knots each.

At that point a clip is not a sequence of descriptions. It is one description
with time in it, and motion is a handful of coefficients rather than a
difference between frames.

## 7 · The exhibit

A browser build, so the thing can be shown rather than described. `wgpu`
targets WebGPU from the same source as the native decoder, so the renderer does
not need rewriting.

What makes the point, in order of effect:

- a slider that composites primitives one at a time, so an image assembles out
  of a few dozen ellipses in front of the viewer
- the same set rendered far beyond its fit resolution, sharp
- for video, the primitive count held constant while time advances

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

What this project can actually establish is narrower: that images, and simple
video, are recoverable as compact deterministic descriptions, and that those
descriptions carry detail their source pixels never contained. That is a real,
demonstrable result. The larger claim is a direction it points in, not a thing
it proves, and it is more persuasive stated that way.
