# Refining

Stage 3. Greedy placement can put a primitive down but never adjust it, so the
only way it can correct an error is to stack another primitive on top. That
inflates the count without improving the description. Refinement moves the
primitives that already exist.

```bash
cd mathset && cargo run --release -- refine ../assets/whiterabbit.jpg in.mathset out.mathset --iters 900
```

No primitive is added or removed. Only the ten numbers in each row change.

## What it buys

Same source, same fit, 300 iterations of refinement:

| primitives | placed only | refined | gain |
|---:|---:|---:|---:|
| 1,000 | 25.00 dB | 27.21 dB | +2.21 |
| 3,000 | 26.15 dB | 29.63 dB | +3.48 |
| 8,000 | 27.68 dB | 33.17 dB | **+5.50** |

The gain grows with the count, because a denser set has more parameters to
move and more redundancy for placement to have wasted.

The result that matters is not the fidelity but the density:

| | primitives | round trip |
|---|---:|---:|
| greedy, converged | 24,886 | 30.86 dB |
| refined | **4,000** | **30.99 dB** |

**6.2× fewer primitives for the same image.** One primitive per 58 pixels
rather than per 9. The file drops from 2.1 MB to 358 KB, though that is a side
effect — the point is that the description is now describing something rather
than patching itself.

## The gradient

The loss is squared error in linear light against the target. What makes it
non-trivial is the compositing: a primitive's effect on the final image is
scaled by everything painted *over* it.

```
C_k = a_k·c_k + (1 - a_k)·C_{k-1}          forward, k = 1..N

dC_N/dC_k = Π_{j>k} (1 - a_j)  =  T_k      transmittance in front of k
```

`T_k` is only known once the later primitives have been seen, which forces a
backward walk. Two accumulators carry it, both updating without division:

```
T_{k-1} = T_k·(1 - a_k)                    start T_N = 1
U_{k-1} = U_k + T_k·a_k·c_k                start U_N = 0
C_{k-1} = (C_N - U_{k-1}) / T_{k-1}        the colour behind k
```

With `g = dL/dC_N · T_k`, every parameter follows by the chain rule:

```
dL/dc       = g·a                    dL/da_eff = dot(g, c - C_{k-1})
dL/dopacity = dL/da_eff · G          dL/dG     = dL/da_eff · opacity

G = exp(-0.5·q^β),  q = u² + v²
dG/dq = -0.5·β·q^(β-1)·G             dG/dβ = -0.5·q^β·ln(q)·G
dL/du = dL/dq·2u                     dL/dv = dL/dq·2v

du/dμx = -cos θ/σx   du/dμy = -sin θ/σx   du/dσx = -u/σx   du/dθ =  v·σy/σx
dv/dμx =  sin θ/σy   dv/dμy = -cos θ/σy   dv/dσy = -v/σy   dv/dθ = -u·σx/σy
```

Ten derivatives, all closed form. No automatic differentiation, no tensor
framework — the whole backward pass is about eighty lines of WGSL.

## How it runs

One workgroup per 16×16 tile, one thread per pixel. Each thread walks only the
primitives touching its tile, forward to composite and backward to emit
gradients.

The tile lists are built on the **CPU**, in primitive order. That is the cheap
way to keep the composite order exact — a GPU binning pass would need a sort to
recover the ordering that a simple CPU loop preserves for free, and at a few
thousand primitives the CPU cost is invisible.

The **GPU computes gradients, the CPU takes the step.** The parameter block is
a few hundred kilobytes, so the round trip costs less than the kernel, and it
keeps Adam in code that can be read and checked directly.

Two details that are not arbitrary:

- **Extents and β step in log space.** Positivity is then structural rather
  than clamped, and a step means the same proportional change whether the
  primitive is two pixels wide or two hundred.
- **Opacity is capped below 1.** The backward pass divides by the
  transmittance behind a primitive, and an opacity of exactly 1 would make
  everything behind it unrecoverable. Capping at 0.99 bounds that division at
  100.

## Verification

A wrong gradient is the quietest possible bug. The image still improves —
the other nine parameters compensate — so no fidelity number reveals it.

So every derivative is checked against a central finite difference of the same
forward model, implemented separately in `f64` on the CPU:

```bash
cd mathset && cargo run --release -- gradcheck ../assets/whiterabbit.jpg tiny.mathset
```

```
 param         analytic      finite diff    rel err
     x        -246.8766        -245.3877    0.00603
     y         746.7726         749.4386    0.00356
    sx       -1208.4818       -1210.3604    0.00155
    sy       -1315.1563       -1314.8782    0.00021
 theta          15.6117          15.8483    0.01493
     r          34.1350          34.0665    0.00201
     g           4.8500           4.7377    0.02316
     b         266.7764         266.8133    0.00014
 alpha         -91.9222         -91.9314    0.00010
  beta          88.3083          88.5451    0.00267
```

Worst relative error 2.3%, on the smallest-magnitude component — where the
finite difference is itself least accurate. The check perturbs one parameter
across *all* primitives at once, so a sign error in any single primitive
fails to cancel.

The command exits non-zero above 5% relative error, so it can be run as a gate
after any change to the backward pass.

## Convergence and cost

Refinement is still improving when it stops. At 3,000 primitives:

| iterations | round trip |
|---:|---:|
| 150 | 28.97 dB |
| 300 | 29.64 dB |
| 600 | 29.92 dB |
| 900 | 30.02 dB |

Roughly 0.23 s per iteration at 451×511, near enough independent of primitive
count — the cost is dominated by the per-pixel walk, not by how many
primitives exist. 900 iterations is about 3.5 minutes.

## Known limitations

- **The count is fixed.** Refinement cannot add a primitive where the image
  needs one, nor delete one that has become useless. Splitting and pruning —
  which is what stage 4 is about — would push the density much further.
- **No band limiting.** Refinement drives primitives smaller, so it moves the
  representation toward the regime where single-point-per-fragment sampling
  starts to alias. Nothing visible yet; worth watching.
- **Learning rates are hand-set** per parameter group, and only lightly
  swept. A schedule would probably beat the fixed values.
- **Refinement optimises the continuous model**, compositing in `f32`, while
  the decoder writes 8-bit. Every number quoted here is measured through the
  real decoder after reloading the file, so the gap is accounted for rather
  than assumed away.
