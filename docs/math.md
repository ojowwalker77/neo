# The math

Everything the decoder does, and why it does it that way.

## The primitive

A primitive is an anisotropic, rotatable, radially-falling blob. In its own
frame — coordinates `(u, v)` measured in standard deviations from its centre —
its value is

```
G(u, v) = exp( -½ · (u² + v²)^β )
```

with `β = 1` giving an ordinary Gaussian.

To evaluate it at a canvas position `p`, first move into that frame. With
centre `μ`, rotation `θ`, and extents `σx`, `σy`:

```
d = p − μ

u = (  d.x·cos θ + d.y·sin θ ) / σx
v = ( −d.x·sin θ + d.y·cos θ ) / σy
```

That is `R(−θ)` applied to the offset, then divided componentwise by the
extents. It undoes the primitive's own rotation and scaling, so a shape that is
an ellipse in canvas space becomes a circle in `(u, v)` space.

### The covariance view

For `β = 1` this is the usual multivariate Gaussian written differently. With

```
Σ = R(θ) · diag(σx², σy²) · R(θ)ᵀ
```

it follows that `u² + v² = dᵀ Σ⁻¹ d`, so

```
G(p) = exp( -½ · dᵀ Σ⁻¹ d )
```

The two forms are the same object. The file stores `(σx, σy, θ)` rather than
the three unique entries of `Σ` for two reasons: every stored number has an
independent meaning and a natural scale, and positivity of the extents is
enforced by a bound on a single number rather than by a positive-definiteness
constraint on a matrix. Both matter once a fitter is moving these values around.

### The shape exponent

`β` is one number per primitive, and it controls how abruptly the falloff
happens. It is the only parameter that can produce something a Gaussian
cannot: an edge.

The width of the transition from 90% to 10% of peak follows from inverting
`G`. Solving `exp(-½ r^{2β}) = t` gives `r = (−2 ln t)^{1/(2β)}`, so

```
width(β) = 4.60517^{1/2β} − 0.21072^{1/2β}
```

| β | transition width | relative to a Gaussian |
|---:|---:|---:|
| 0.5 | 4.394 σ | 0.4× — softer |
| 1 | 1.687 σ | 1× — Gaussian |
| 2 | 0.787 σ | 2.1× sharper |
| 4 | 0.387 σ | 4.4× sharper |
| 8 | 0.193 σ | 8.7× sharper |
| 20 | 0.077 σ | 21.9× sharper |

This matters because real images are full of step edges — an object against a
background, a sleeve against a wall — and a pure Gaussian can only approximate
one by stacking many primitives along it. That is expensive, and it is
expensive in the worst place: an object's silhouette is exactly where motion is
most visible, so an edge built from a crowd of overlapping blobs makes the
later temporal work harder, not just the still image worse.

One number avoids all of that, and stays smooth and differentiable, so a
fitter can discover the right sharpness per primitive rather than having it
chosen in advance.

**`β` below 1 is expensive.** The envelope below grows as `β` shrinks: at
`β = 0.5` a primitive covers roughly 600× the area of a `β = 4` one, and
overdraw scales with it. Long tails are also usually the wrong tool — more
primitives are the right one. Clamping `β ≥ 1` is a reasonable default.

### The envelope

A primitive has infinite support, so it has to be cut off somewhere. The cutoff
is derived rather than tuned: draw the primitive only where it could still
change an 8-bit output value, which is where it exceeds half a code value,
`ε = 1/510`.

```
exp( -½ r^{2β} )  ≥  ε
      ½ r^{2β}    ≤  −ln ε
             r    ≤  (−2 ln ε)^{1/2β}
```

With `ε = 1/510`, `−2 ln ε ≈ 12.4688`:

| β | envelope |
|---:|---:|
| 0.5 | 12.469 σ |
| 1 | 3.531 σ |
| 2 | 1.879 σ |
| 4 | 1.371 σ |
| 8 | 1.171 σ |
| 20 | 1.065 σ |

The decoder draws each primitive as a quad of this half-extent in `(u, v)`
space, so a sharp primitive automatically gets a tight quad and a soft one gets
a wide one. Note the quad is a **box** in `(u, v)`, not a disc — the corners
extend to `√2 ×` the envelope, where the falloff is smaller still.

## Compositing

Each primitive's effective opacity at a point is its peak opacity scaled by the
falloff:

```
α = a · G(u, v)
```

and primitives are composited in file order with the standard over operator:

```
C₀ = background
Cₖ = αₖ · cₖ + (1 − αₖ) · Cₖ₋₁
```

This does not commute. Swapping two overlapping primitives changes the result,
which is why the file stores a sequence rather than a set, and why order is
part of the format rather than an implementation detail.

## Colour

Colours are **stored** sRGB-encoded and **composited** in linear light.

Storage is sRGB because the file should be legible: `0.5` in a text editor
should mean the mid-grey a person expects. Compositing is linear because that
is where blending is physically correct — averaging two sRGB numbers does not
give the colour you get by mixing those two lights.

The standard transfer functions:

```
srgb → linear:   c ≤ 0.04045   ?  c / 12.92
                               :  ((c + 0.055) / 1.055)^2.4

linear → srgb:   c ≤ 0.0031308 ?  12.92 · c
                               :  1.055 · c^(1/2.4) − 0.055
```

Applied on the way in, undone on the way out. The background colour goes
through the same path.

Getting this wrong is easy and quiet — the image merely looks a bit off, and
nothing errors. It is checked explicitly:
[verification.md](verification.md#colour-pipeline).

## Resolution independence

This is the property the project rests on, so it is worth stating exactly what
does and does not happen.

The file contains no pixels. Rendering picks a scale — `unit`, in pixels per
normalized unit — and evaluates `G` at each pixel's centre:

```
p = ( (i + 0.5) / unit , (j + 0.5) / unit )
```

Doubling the output resolution doubles `unit`. Every fragment then evaluates
the same closed-form expression at a finer set of positions. Nothing is
sampled from a stored grid, nothing is interpolated, and no filter kernel is
involved anywhere in the decoder.

The consequence: rendering a set at 8× produces detail that is genuinely
present in the description and was simply not resolved at the lower scale. This
is the difference between a description of a thing and a picture of it, and it
is the one behaviour a compressed image cannot imitate.

The current implementation is not fully band-limited — each fragment takes a
single point sample rather than integrating the primitive over the pixel's
area. At normal scales the primitives are large relative to a pixel and this is
invisible. It will need revisiting if very sharp, very small primitives
(`β ≫ 1` with `σ` near a pixel) turn out to be common once a fitter exists.

## Differentiability

Not used yet, but it constrains the design, so it is worth recording why the
primitive looks like this.

`G` is smooth in every stored parameter — position, extents, rotation, colour,
opacity, and `β`. That means a fitter can eventually compute `∂L/∂param`
analytically and refine a primitive by gradient descent, instead of only being
able to propose and accept it.

Two later stages depend on this and cannot be built without it:

- **refinement** — nudging a primitive to be exactly right, rather than
  re-rolling random candidates until one happens to fit
- **temporal curves** — fitting the coefficients of a primitive's parameters as
  functions of `t`, which is what turns a sequence of frames into motion rather
  than a stack of independent fits

The derivatives are short closed forms for this primitive. No automatic
differentiation framework is required.
