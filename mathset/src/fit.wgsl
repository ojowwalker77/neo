// Greedy placement: propose primitives, keep the ones that lower the error.
//
// Three compute kernels per pass.
//
//   errmap  per-block error of current vs target. Drives the size boost and
//           the stopping rule.
//   score   one thread per candidate. Derives the candidate from an integer
//           hash, integrates over its own footprint, and solves for the
//           colour that minimises the error it would leave behind. Emits that
//           colour and the error delta it buys.
//   reduce  one thread per cell. Picks that cell's best candidate, and drops
//           it entirely if it does not strictly improve the image.
//
// Applying the winners is not here — it reuses splat.wgsl, the decoder's own
// shader. The fitter must score against exactly the canvas the decoder will
// produce, or every accept decision is made against a fiction.

struct FitParams {
    res:        vec2<f32>,
    cells:      vec2<u32>,
    unit:       f32,
    m:          u32,
    pass_idx:   u32,
    sigma:      f32,
    sigma_min:  f32,
    sigma_max:  f32,
    alpha_lo:   f32,
    alpha_hi:   f32,
    ebw:        u32,
    ebh:        u32,
    eblock:     u32,
    perceptual: u32,
    pace:       f32,
    beta_max:   f32,
    jitter:     vec2<f32>,
    _pad:       vec4<f32>,
};

struct Cand {
    geom:  vec4<f32>,   // x, y, sx, sy
    rot:   vec4<f32>,   // theta, alpha, beta, envelope
    color: vec4<f32>,   // r, g, b (sRGB), delta
};

@group(0) @binding(0) var tgt: texture_2d<f32>;
@group(0) @binding(1) var cur: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> errmap: array<f32>;
@group(0) @binding(3) var<storage, read_write> cands: array<Cand>;
@group(0) @binding(4) var<uniform> P: FitParams;
@group(0) @binding(5) var<storage, read_write> winners: array<Cand>;

const LUMA = vec3<f32>(0.2126, 0.7152, 0.0722);
const ENV_K = 12.468822;      // -2 ln(1/510); see docs/math.md

fn envelope(beta: f32) -> f32 { return pow(ENV_K, 1.0 / (2.0 * beta)); }

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return select(lo, hi, c > vec3(0.0031308));
}

// Sensitivity of the sRGB encoding at this target value, squared. Weighting
// the linear-space error by it makes the fit minimise error as seen rather
// than error in photons, which otherwise leaves shadows mushy.
fn pweight(t: vec3<f32>) -> vec3<f32> {
    if (P.perceptual == 0u) { return vec3(1.0); }
    let l = max(t, vec3(0.0031308));
    let d = min(vec3(12.92), 0.439583 * pow(l, vec3(-0.5833333)));
    return d * d;
}

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rnd(seed: ptr<function, u32>) -> f32 {
    *seed = pcg(*seed);
    return f32(*seed) * (1.0 / 4294967296.0);
}

// ── errmap ────────────────────────────────────────────────────────────────
@compute @workgroup_size(8, 8)
fn errmap_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= P.ebw || gid.y >= P.ebh) { return; }
    let res = vec2<u32>(P.res);
    var acc = 0.0;
    var n = 0.0;
    for (var j = 0u; j < P.eblock; j = j + 1u) {
        for (var i = 0u; i < P.eblock; i = i + 1u) {
            let p = vec2<u32>(gid.x * P.eblock + i, gid.y * P.eblock + j);
            if (p.x >= res.x || p.y >= res.y) { continue; }
            let c = vec2<i32>(p);
            let T = textureLoad(tgt, c, 0).rgb;
            let d = textureLoad(cur, c, 0).rgb - T;
            acc = acc + dot(pweight(T) * d * d, vec3(1.0));
            n = n + 3.0;
        }
    }
    errmap[gid.y * P.ebw + gid.x] = select(0.0, acc / n, n > 0.0);
}

// ── footprint integration ─────────────────────────────────────────────────
// Accumulates the three sums that yield both the optimal colour and the error
// delta it buys, in one traversal. See docs/math.md.
//
//   c*     = S1 / S2
//   delta  = c²·S2 - 2c·S1 + S3      exact for any c, including a clamped one
//
// `cap` bounds the samples per axis; anything larger is strided and each
// sample stands for the area it skipped. `offset` shifts the sample lattice,
// which is what makes a second pass over the same candidate statistically
// independent of the first.

const CAP_SCORE: i32 = 32;   // selection: cheap, run for every candidate
const CAP_CHECK: i32 = 96;   // verification: accurate, run once per cell

struct Integ {
    c:     vec3<f32>,
    delta: f32,
    ok:    u32,
};

fn integrate(cd: Cand, cap: i32, offset: i32) -> Integ {
    let pos = cd.geom.xy;
    let sx = cd.geom.z;
    let sy = cd.geom.w;
    let ct = cos(cd.rot.x);
    let st = sin(cd.rot.x);
    let alpha = cd.rot.y;
    let beta = cd.rot.z;
    let env = cd.rot.w;

    let res = vec2<i32>(P.res);
    let hx = env * (abs(sx * ct) + abs(sy * st)) * P.unit;
    let hy = env * (abs(sx * st) + abs(sy * ct)) * P.unit;
    let c0 = vec2<i32>(clamp(pos * P.unit - vec2(hx, hy), vec2(0.0), vec2<f32>(res - 1)));
    let c1 = vec2<i32>(clamp(pos * P.unit + vec2(hx, hy), vec2(0.0), vec2<f32>(res - 1)));

    let span = c1 - c0 + vec2<i32>(1, 1);
    let stride = max(1, (max(span.x, span.y) + cap - 1) / cap);
    let area = f32(stride * stride);
    let start = c0 + vec2<i32>(offset % stride, offset % stride);

    var S1 = vec3(0.0);
    var S2 = vec3(0.0);
    var S3 = vec3(0.0);

    for (var y = start.y; y <= c1.y; y = y + stride) {
        for (var x = start.x; x <= c1.x; x = x + stride) {
            let pn = (vec2(f32(x), f32(y)) + 0.5) / P.unit - pos;
            let uu = ( ct * pn.x + st * pn.y) / sx;
            let vv = (-st * pn.x + ct * pn.y) / sy;
            if (abs(uu) > env || abs(vv) > env) { continue; }

            let A = alpha * exp(-0.5 * pow(uu * uu + vv * vv, beta));
            let ic = vec2<i32>(x, y);
            let T = textureLoad(tgt, ic, 0).rgb;
            let C = textureLoad(cur, ic, 0).rgb;
            let D = C - T;
            let w = pweight(T) * area;

            S1 = S1 + w * A * (A * C - D);
            S2 = S2 + w * A * A;
            S3 = S3 + w * (A * A * C * C - 2.0 * A * C * D);
        }
    }

    var r: Integ;
    if (S2.x <= 0.0 || S2.y <= 0.0 || S2.z <= 0.0) {
        r.ok = 0u;
        r.delta = 1e30;
        r.c = vec3(0.0);
        return r;
    }
    r.ok = 1u;
    r.c = clamp(S1 / S2, vec3(0.0), vec3(1.0));
    r.delta = dot(r.c * r.c * S2 - 2.0 * r.c * S1 + S3, vec3(1.0));
    return r;
}

fn err_at(pn: vec2<f32>) -> f32 {
    let px = pn * P.unit / f32(P.eblock);
    let b = vec2<u32>(clamp(px, vec2(0.0), vec2(f32(P.ebw) - 1.0, f32(P.ebh) - 1.0)));
    return errmap[b.y * P.ebw + b.x];
}

// ── score ─────────────────────────────────────────────────────────────────
@compute @workgroup_size(64)
fn score_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let total = P.cells.x * P.cells.y * P.m;
    if (gid.x >= total) { return; }

    let cell = gid.x / P.m;
    let cx = cell % P.cells.x;
    let cy = cell / P.cells.x;
    var seed = pcg(gid.x ^ (P.pass_idx * 2654435761u) ^ 0x9E3779B9u);

    // ── propose ──
    // The grid is re-jittered every pass so no cell boundary is ever a
    // persistent seam in the result.
    let extent = P.res / P.unit;
    let csz = extent / vec2<f32>(P.cells);
    let pos = (vec2(f32(cx), f32(cy)) + vec2(rnd(&seed), rnd(&seed))) * csz + P.jitter * csz;

    let boost = 0.55 + 1.45 * sqrt(clamp(err_at(pos) * 6.0, 0.0, 1.0));
    var sx = clamp(P.sigma * (0.45 + 1.10 * rnd(&seed)) * boost, P.sigma_min, P.sigma_max);
    let aspect = 0.18 + 0.82 * rnd(&seed);
    var sy = max(sx * aspect, P.sigma_min * 0.5);

    // Orientation follows the target's own structure where there is any:
    // a stroke laid along an edge buys far more than one laid across it.
    var theta: f32;
    let ip = vec2<i32>(pos * P.unit);
    let res = vec2<i32>(P.res);
    let l = vec2<i32>(max(ip.x - 1, 0), ip.y);
    let r = vec2<i32>(min(ip.x + 1, res.x - 1), ip.y);
    let d = vec2<i32>(ip.x, max(ip.y - 1, 0));
    let u = vec2<i32>(ip.x, min(ip.y + 1, res.y - 1));
    let gx = dot(textureLoad(tgt, r, 0).rgb - textureLoad(tgt, l, 0).rgb, LUMA);
    let gy = dot(textureLoad(tgt, u, 0).rgb - textureLoad(tgt, d, 0).rgb, LUMA);
    if (length(vec2(gx, gy)) > 0.0025 && rnd(&seed) < 0.75) {
        theta = atan2(gy, gx) + 1.5707963;         // along the edge, not across
    } else {
        theta = rnd(&seed) * 6.2831853;
    }

    // beta = 1 is a plain Gaussian and suits most of an image; the rest of the
    // range is what edges need.
    var beta = 1.0;
    if (rnd(&seed) < 0.45) { beta = 1.0 + rnd(&seed) * (P.beta_max - 1.0); }
    let alpha = P.alpha_lo + rnd(&seed) * (P.alpha_hi - P.alpha_lo);
    let env = envelope(beta);

    var out: Cand;
    out.geom = vec4(pos, sx, sy);
    out.rot = vec4(theta, alpha, beta, env);

    let est = integrate(out, CAP_SCORE, 0);
    if (est.ok == 0u) {
        out.color = vec4(0.0, 0.0, 0.0, 1e30);      // nothing under it; reject
        cands[gid.x] = out;
        return;
    }
    out.color = vec4(linear_to_srgb(est.c), est.delta);
    cands[gid.x] = out;
}

// ── reduce ────────────────────────────────────────────────────────────────
@compute @workgroup_size(64)
fn reduce_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let ncells = P.cells.x * P.cells.y;
    if (gid.x >= ncells) { return; }

    var best = -1e-9;                    // must strictly improve, not merely tie
    var bi = -1;
    for (var m = 0u; m < P.m; m = m + 1u) {
        let d = cands[gid.x * P.m + m].color.w;
        if (d < best) { best = d; bi = i32(m); }
    }

    // Throttle: with every cell placing every pass, a fit spends its whole
    // budget in the first few coarse passes.
    var seed = pcg(gid.x ^ (P.pass_idx * 40503u) ^ 0xB5297A4Du);
    if (rnd(&seed) > P.pace) { bi = -1; }

    // Selecting the minimum of M noisy estimates is biased: the winner is
    // partly whichever candidate the subsampling happened to flatter. Re-score
    // it on a denser, offset lattice, which is independent of the estimate
    // that selected it, and drop it unless it still earns its place.
    var verified: Integ;
    if (bi >= 0) {
        verified = integrate(cands[gid.x * P.m + u32(bi)], CAP_CHECK, 1);
        if (verified.ok == 0u || verified.delta >= 0.0) { bi = -1; }
    }

    if (bi < 0) {
        // An inert entry: alpha 0 blends to an exact no-op, so the apply pass
        // can draw every cell without branching and still change nothing.
        var dead: Cand;
        dead.geom = vec4(0.0, 0.0, 1.0, 1.0);
        dead.rot = vec4(0.0, 0.0, 1.0, 1.0);
        dead.color = vec4(0.0);
        winners[gid.x] = dead;
        return;
    }

    var w = cands[gid.x * P.m + u32(bi)];
    w.color = vec4(linear_to_srgb(verified.c), 0.0);   // the verified colour, not the estimate
    winners[gid.x] = w;
}
