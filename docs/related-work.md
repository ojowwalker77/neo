# Related work

Neo sits inside an existing line of work on implicit video functions and
explicit 2D-Gaussian image and video representations. Deformable 2D Gaussians
for video already exist. This notebook therefore does **not** claim novelty for
using Gaussian-like primitives, keeping them over time, or rendering them at
arbitrary spatial resolutions.

The comparison below is intentionally about representation and evidence rather
than benchmark rank. Neo is not currently a compression system, and its
temporal extractor is not yet part of the tracked Rust engine.

## Direct antecedents

**NeRV (NeurIPS 2021).** [NeRV: Neural Representations for
Videos](https://proceedings.neurips.cc/paper/2021/hash/b44182379bf9fae976e6ae5996e13cd8-Abstract.html)
represents a video as a neural network that takes a frame index and emits the
whole RGB frame. It is a clear antecedent for treating a video as one
time-indexed executable representation rather than only as stored frames.
NeRV's learned network is implicit and frame-producing; Neo's intended output
is an explicit text program whose spatial primitives and temporal functions can
be inspected directly.

**GaussianImage (ECCV 2024).** [GaussianImage: 1000 FPS Image Representation
and Compression by 2D Gaussian
Splatting](https://www.ecva.net/papers/eccv_2024/papers_ECCV/html/1421_ECCV_2024_paper.php)
fits still images with explicit 2D Gaussians carrying position, covariance, and
colour, then develops a fast rasterizer and codec. It establishes the important
still-image antecedent: fitting pixels with a compact set of 2D Gaussians is
not unique to Neo.

**GSVC (2025).** [GSVC: Efficient Video Representation and Compression Through
2D Gaussian Splatting](https://arxiv.org/abs/2501.12060) represents each frame
with 2D splats, initializes predicted frames from the preceding frame, stores
parameter differences, prunes low-contribution splats, adds splats for new or
large-motion content, and introduces keyframes at large scene changes. Its
primary target is rate-distortion and fast decoding. GSVC is especially relevant
to Neo's negative warm-start result: successful reconstruction after
frame-to-frame parameter refinement does not, by itself, establish physical
motion or semantic primitive identity.

**GaussianVideo (CVPR Workshops 2025).** [GaussianVideo: Efficient Video
Representation and Compression by Gaussian
Splatting](https://openaccess.thecvf.com/content/CVPR2025W/PBVS/html/Lee_GaussianVideo_Efficient_Video_Representation_and_Compression_by_Gaussian_Splatting_CVPRW_2025_paper.html)
uses deformable 2D Gaussians. A multi-plane spatiotemporal encoder and
lightweight decoder take a Gaussian and time step and predict time-conditioned
changes to colour, coordinates, and shape. This is direct prior work for
continuous, time-conditioned deformation of 2D Gaussian video primitives.

**D2GV (2025; revised 2026).** [D2GV: Deformable 2D Gaussian Splatting for
Video Representation in 400
FPS](https://arxiv.org/abs/2503.05600v1) represents each group of pictures from
a canonical set of 2D Gaussians deformed to its timestamps, and evaluates video
interpolation, inpainting, and denoising as well as representation efficiency.
The [current revision, D2GV-AR](https://arxiv.org/abs/2503.05600), extends that
line toward arbitrary-resolution rendering and progressive coding, with
temporal evolution represented by a neural ordinary differential equation.
D2GV therefore overlaps not only in primitive choice but also in explicit
deformation, interpretability goals, downstream editing, and evaluation between
frames.

## Neo's intended research difference

Neo's target is an **inverse visual compiler**: pixels in, an inspectable and
editable continuous visual program out. The intended differentiators are:

- a portable text representation whose program semantics — including
  compositing, parameter domains, and time controls — are part of the artifact,
  rather than treating Gaussian arrays or a learned deformation network alone
  as the output;
- closed-form temporal functions with visible coefficients and direct
  program-level controls such as `ω` and `τ`;
- inspectability and editability as first-class evaluation targets, including
  intervention tests with expected outcomes;
- independent decoding and numerical verification of emitted programs;
- publication of failures and negative results, including cases where frame
  fidelity hides incorrect primitive motion.

Those points describe the research programme, not established novelty. Prior
work already claims explicitness, interpretability, editability, arbitrary
resolution, or downstream-task value in different combinations. Establishing
a novel contribution would require a broader literature review and direct
comparative experiments.

## The shared evaluation problem

All of these approaches fit finite observations. Agreement at sampled frames
does not uniquely identify the continuous function between them. A
time-conditioned Gaussian can reproduce a frame while taking the wrong path,
and a persistent array index need not denote a persistent physical or semantic
part.

Neo therefore separates three questions:

1. Does the representation reproduce observed samples?
2. Does it behave correctly at withheld or unseen times?
3. Does it remain meaningful under controlled interventions?

The first is reconstruction. The second tests temporal generalisation. The
third tests whether the recovered program supports the kind of understanding
and editing that motivates compiling pixels into programs at all.
