# Verification

The project's central claim is that an image can be a consequence of a small
set of numbers. A renderer that produces a plausible-looking picture does not
establish that. Two unrelated renderers producing the *same* picture from the
same numbers does.

So correctness here means agreement between independent implementations, and
it is measured rather than asserted.

## The method

Two decoders exist:

| | |
|---|---|
| **GPU** | Rust, WGSL, `wgpu`. `f32`. Hardware alpha blending, hardware sRGB conversion. |
| **CPU** | Python, standard library only. `f64`. Software blending, software sRGB conversion. |

They share no code. The CPU implementation in
[`mathset/tools/reference.py`](../mathset/tools/reference.py) was written from
[format.md](format.md) alone — it has never read the shader. Different
language, different precision, different hardware, different blending path.

If both produce the same image, the image cannot be an artifact of either.

The CPU decoder is deliberately slow and literal. It is not a second product
decoder to keep in sync; it exists to be *obviously* correct by inspection, so
that it can be trusted as an oracle.

## Running it

```bash
./mathset/tools/verify.sh
```

Builds the decoder, then for every file in `mathset/sets/` renders it both ways
and diffs. Also checks that the same file rendered at 1× and 8× agrees.

A single set:

```bash
cd mathset && python3 tools/reference.py sets/five.mathset
```

## What the numbers mean

Healthy output looks like this:

```
independent CPU decode vs GPU decode, 512x512
  max channel difference : 2 / 255
  mean channel difference: 0.1615
  pixels differing by >1 : 30 of 262144 (0.011%)
  psnr                   : 56.05 dB
```

- **PSNR above 50 dB** — the conformance threshold from
  [format.md](format.md#conformance). Current sets land at 56–62 dB.
- **Max channel difference of 1–2** — `f32` versus `f64` rounding at blend
  boundaries, where a value sits near the midpoint between two 8-bit codes.
  This is the expected residual, and it is why the format specifies agreement
  within a tolerance rather than bit-exactness.
- **A handful of pixels differing by more than 1** — same cause.

Failure looks obviously different: PSNR in the 20s or 30s, max differences in
the tens, large contiguous regions wrong. A real divergence between the shader
and the spec is not subtle.

## What has been checked

### Falloff

A single primitive sampled radially, compared against the closed form
`exp(-½ r^{2β})`, across `β` from 0.5 to 12. Agreement to within **0.002 σ**,
the residual being the scan step of the probe rather than model error.

### Edge sharpness

The 90%→10% transition width measured from rendered output and compared with
the derivation in [math.md](math.md#the-shape-exponent):

| β | measured | closed form |
|---:|---:|---:|
| 0.5 | 4.3750 σ | 4.3944 σ |
| 1 | 1.6846 σ | 1.6869 σ |
| 2 | 0.7861 σ | 0.7874 σ |
| 4 | 0.3906 σ | 0.3872 σ |
| 12 | 0.1270 σ | 0.1285 σ |

Each primitive was measured in isolation. Measuring them side by side in one
image gives wrong answers, because a low-`β` primitive's envelope reaches far
enough to contaminate its neighbours.

### Covariance and rotation

Sampling a rotated, elongated primitive at equal σ-distances along its major
and minor axes returns **identical** values — anisotropic in canvas space,
radially symmetric in `(u, v)` space, which is what the transform is supposed
to achieve. Probing along the mirrored `−θ` axis returns exactly `0`, so the
sign convention is unambiguous rather than accidentally symmetric.

### Colour pipeline

A pixel far from any primitive's centre was computed by hand through the full
path — decode background and primitive colour to linear, composite, re-encode
— predicting `(15.4, 21.3, 41.2)` against a rendered `(15, 21, 42)`.

This one is worth doing by hand rather than trusting the cross-check, because a
transfer-function error applied consistently in both implementations would
cancel out and pass silently.

### Resolution independence

The same file rendered at 512×512 and 4096×4096. Box-downsampling the large
render back to 512×512 and comparing against the native 512×512 render gives
**60.9 dB**, max channel difference 1.

This checks that scale is a free parameter of the decoder and not a property
baked into the output — the two renders are the same description sampled at
different densities, not one derived from the other.

## What is not yet checked

- **Very small, very sharp primitives.** Point sampling per fragment is not
  band-limited; a primitive with `β ≫ 1` and `σ` near one pixel would alias.
  No such primitive exists in any current set. This needs revisiting once a
  fitter starts producing them.
- **Cross-vendor agreement.** Both decoders have only been run on one machine.
  The conformance threshold anticipates other GPUs, but nothing has confirmed
  it on one yet.
- **Extreme values.** Very large `β`, `σ` spanning the whole canvas, opacity of
  exactly 0 or 1 — accepted by the validator, not systematically exercised.
