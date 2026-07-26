# The `.mathset` file

A `.mathset` is a complete, self-contained description of an image. Decoding
one requires nothing else — no source image, no error metric, no dictionary,
no trained model.

Version 0. JSON, so it can be read and edited by hand. A binary encoding will
come later if it is ever needed; it is not needed yet.

## Shape

```json
{
  "mathset": 0,
  "canvas": [512, 512],
  "space": "srgb",
  "bg": [0.05, 0.055, 0.075],
  "primitive": "gauss2d",
  "splats": [
    [0.50, 0.50, 0.300, 0.300,  0.0000000, 0.15, 0.25, 0.55, 0.90],
    [0.38, 0.42, 0.220, 0.045, -0.5235988, 0.95, 0.55, 0.15, 0.85]
  ]
}
```

| field | meaning |
|---|---|
| `mathset` | format version. Currently `0`. A decoder must refuse a version it does not know. |
| `canvas` | `[width, height]` of the reference resolution. A **framing hint**, not a constraint — it fixes the aspect ratio and the unit of length, nothing more. |
| `space` | colour space of the stored colours. Currently `"srgb"` only. |
| `bg` | the canvas before any primitive is composited, as `[r, g, b]`. |
| `primitive` | which primitive the entries describe. Currently `"gauss2d"` only. |
| `splats` | the primitives, **in composite order**. |

## Coordinates

All positions and extents are normalized against the **long edge** of
`canvas`. For a 1200×260 canvas the coordinate space runs `x ∈ [0, 1]`,
`y ∈ [0, 0.2167]`. Pixels are square. `y` increases downward.

Nothing in the file refers to a pixel. This is deliberate and load-bearing: it
is what allows the same set to be rendered at any resolution, and it is the
property most worth protecting as the format grows.

## Primitive: `gauss2d`

Each entry is an array of **9 or 10 numbers**:

```
[ x, y, sx, sy, theta, r, g, b, a, beta? ]
```

| | | |
|---|---|---|
| `x`, `y` | centre | normalized coordinates |
| `sx`, `sy` | standard deviations along the primitive's own axes | normalized; must be `> 0` |
| `theta` | rotation of those axes | radians, counter-clockwise |
| `r`, `g`, `b` | colour | sRGB-encoded, nominally `0..1` |
| `a` | opacity at the centre | `0..1` |
| `beta` | shape exponent | optional, defaults to `1`; must be `> 0` |

`beta = 1` is an ordinary Gaussian. Larger values square off the edge; smaller
values lengthen the tails. See [math.md](math.md#the-shape-exponent) for what
the number does and what it costs.

A 9-number entry is exactly a 10-number entry with `beta = 1`. Decoders must
accept both.

## Order is part of the file

Alpha compositing does not commute. `splats` is a **sequence**, not a set —
the first entry is painted first, the last entry is painted last and appears on
top. Reordering the list is not a no-op; it produces a different image.

Two dimensions have no depth to sort by, so there is nothing to derive the
order from. It has to be stored, and it is.

## Rendering at a resolution

Given an output of `W × H` pixels:

```
unit_ref = max(canvas.w, canvas.h)
ext      = (canvas.w / unit_ref, canvas.h / unit_ref)
unit     = min(W / ext.x, H / ext.y)          # pixels per normalized unit
```

Pixel `(i, j)` samples the description at normalized position
`((i + 0.5) / unit, (j + 0.5) / unit)`.

When the output aspect matches `canvas`, `unit` is simply the output's long
edge, and doubling the output resolution doubles `unit`. When it does not
match, the set is rendered at the largest scale that fits, anchored at the top
left. Nothing is ever cropped.

## Validation

A conforming decoder rejects a file when:

- `mathset` is a version it does not implement
- `primitive` is a type it does not implement
- `space` is not `"srgb"`
- either `canvas` dimension is zero
- an entry has a length other than 9 or 10
- any number is not finite
- `sx` or `sy` is not positive
- `beta` is present and not positive

Everything else is accepted as written. In particular, primitives may lie
partly or wholly outside the canvas, may overlap freely, and may have opacity
of zero. None of these are errors; a fitter will produce all of them.

## Conformance

Two decoders conform if, given the same file and the same output resolution,
they agree to **at least 50 dB PSNR**.

Exact agreement is not required. Floating-point rounding differs across GPUs
and across precisions, so demanding bit-identical output would constrain the
implementation far more than it would buy. In practice, independent
implementations at f32 and f64 land above 55 dB, with no channel differing by
more than 2 of 255. See [verification.md](verification.md).
