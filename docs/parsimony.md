# Parsimony

Stage 4. Refinement moves the primitives that exist but cannot change how many
there are — it can neither add one where the image needs it nor drop one that
has become useless. This stage does both, and it is where the density falls.

```bash
cd mathset && cargo run --release -- refine ../assets/whiterabbit.jpg in.mathset out.mathset \
  --iters 900 --adapt --count 2500
```

## The result

| | primitives | round trip | pixels per primitive |
|---|---:|---:|---:|
| placed only | 24,886 | 30.86 dB | 9 |
| placed + refined | 4,000 | 30.99 dB | 58 |
| **placed + refined + adapted** | **2,381** | **31.12 dB** | **97** |

**10.5× fewer primitives than placement alone, for a slightly better image.**

Across the range, at 900 iterations:

| primitives | round trip |
|---:|---:|
| 953 | 28.39 dB |
| 1,429 | 29.47 dB |
| 1,905 | 30.42 dB |
| 2,381 | 31.12 dB |
| 3,810 | 33.23 dB |

Adaptation is worth about **+2.2 dB over refinement alone** at equal count:
3,810 adapted primitives reach 33.23 dB where 4,000 refined ones reach 30.99.

## What a primitive is worth

The decision to drop a primitive should not be a heuristic when the exact
answer is available. Removing primitive `k` changes the image by

```
with k     C_N  = U_k + T_k·( a_k·c_k + (1 - a_k)·C_{k-1} )
without    C'_N = U_k + T_k·C_{k-1}
difference dC   = T_k·a_k·( c_k - C_{k-1} )
```

so the loss it earns its place with is exactly

```
L' - L = Σ [ (C_N - dC - T)² - (C_N - T)² ]
       = Σ [ dot(dC, dC) - dot(dC, dL/dC_N) ]
```

Every term is already in hand at that point of the backward walk — `T_k`,
`a_k`, `c_k`, `C_{k-1}` and the residual are all needed for the gradient
anyway. The worth score costs one extra slot per primitive and no extra pass.

Reading it:

- **positive** — removing it would raise the loss; it pays for itself
- **around zero** — it contributes nothing
- **negative** — removing it would *lower* the loss; it is actively harmful

Primitives at or below zero are always retired, however many there are.

## Where to add

The norm of a primitive's accumulated positional gradient. A large value means
the pixels under it are pulling it in conflicting directions — one primitive
being asked to cover two things at once. It is replaced by a pair offset along
its own major axis by 0.55σ, each narrowed by 1.6×, both taking the original's
place in the sequence so the composite order is undisturbed.

## Both at once, gently

Two things had to be fixed before this worked at all, and both are worth
recording because the failure modes were instructive.

**Prune and split must be decided from one gradient snapshot.** Pruning first
and splitting after consults gradients belonging to primitives that no longer
exist — in practice the split step silently selected nothing and the set only
ever shrank. Splitting first would spend the budget on primitives about to be
dropped.

**The worth distribution is long-tailed, so a threshold relative to the mean
is far too aggressive.** A first attempt retiring everything below 0.35× the
average worth removed 68% of the set in a single cycle, and 900 iterations
later a 4,000-primitive set had collapsed to 11 and lost 2.65 dB. Retiring a
fixed *share* of the set per cycle — 5% by default — and splitting back up to
the target count is stable and monotone.

The cycle is 60 iterations, and adaptation stops before the final stretch so
the run ends on a settled set. Every structural change disturbs the optimiser
around it: Adam moments travel with a primitive when it survives, and start
fresh for both halves of a split, because the primitive they belonged to no
longer exists.

## Verification

The worth score is checked the same way the gradients are — predicted loss
increase against the loss actually measured after removing the primitive:

```bash
cd mathset && cargo run --release -- gradcheck ../assets/whiterabbit.jpg tiny.mathset
```

```
  prim        predicted           actual    rel err
     0        135.36787        135.36869    0.00001
     6         21.15132         21.15127    0.00000
    12          5.74835          5.74854    0.00003
    38          2.96972          2.96998    0.00009
    17          1.97952          1.97844    0.00055
    30          0.55132          0.55072    0.00107
```

Worst relative error **0.1%** across four orders of magnitude of worth. The
prune criterion is exact, not an approximation that happens to work.

## Known limitations

- **Adaptation is still improving when it stops.** Every figure here is a
  lower bound.
- **Splitting is the only way to add.** A primitive can divide, but nothing
  introduces a primitive into a region that has none — if placement missed
  something entirely, refinement cannot discover it. Running the greedy
  proposer against the residual between cycles would fix this and has not been
  tried.
- **The split rule is a heuristic**, unlike the prune rule. Offset and
  narrowing factors were chosen by analogy with splat densification, not
  derived.
- **One image.** Every number here is `whiterabbit.jpg` at 451×511. The ratios
  should hold for photographic content of similar complexity, but nothing has
  confirmed that.
