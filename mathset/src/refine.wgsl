// Stage 3: analytic gradients for every parameter of every primitive.
//
// One workgroup per tile, one thread per pixel. Each thread walks only the
// primitives that touch its tile — the list is built on the CPU, in primitive
// order, which is what keeps the composite order exact.
//
// Two walks per pixel:
//   forward   1..N   composite to get the final colour
//   backward  N..1   carry transmittance T and the accumulated colour in
//                    front, U, and emit each primitive's gradient
//
// The backward direction is forced by the compositing: a primitive's gradient
// is scaled by the product of (1-alpha) for everything painted over it, and
// that product is only known once you have seen the later primitives.
//
// Gradients from different tiles land on the same primitive, so accumulation
// is atomic. WGSL has no float atomics, so this uses a compare-exchange loop
// on the bit pattern — exact, with no fixed-point scale to tune.

struct R {
    res:     vec2<f32>,
    unit:    f32,
    tile:    u32,
    tiles_x: u32,
    n:       u32,
    _p:      vec2<u32>,
    bg:      vec4<f32>,      // linear light
};

@group(0) @binding(0) var tgt: texture_2d<f32>;
@group(0) @binding(1) var<storage, read>       par: array<f32>;
@group(0) @binding(2) var<storage, read_write> grad: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read>       tile_off: array<u32>;
@group(0) @binding(4) var<storage, read>       tile_idx: array<u32>;
@group(0) @binding(5) var<uniform> P: R;

const STRIDE: u32 = 10u;     // x y sx sy theta r g b a beta
const ENV_K: f32 = 12.468822;
const T_FLOOR: f32 = 1e-4;   // below this a primitive is buried; its gradient is ~0

fn envelope(beta: f32) -> f32 { return pow(ENV_K, 1.0 / (2.0 * beta)); }

fn add_grad(slot: u32, v: f32) {
    if (v == 0.0) { return; }
    var old = atomicLoad(&grad[slot]);
    loop {
        let sum = bitcast<u32>(bitcast<f32>(old) + v);
        let r = atomicCompareExchangeWeak(&grad[slot], old, sum);
        if (r.exchanged) { break; }
        old = r.old_value;
    }
}

struct Ev { alpha: f32, g: f32, q: f32, u: f32, v: f32, hit: bool };

// Evaluate primitive k at normalized position p.
fn eval(k: u32, p: vec2<f32>) -> Ev {
    let b = k * STRIDE;
    let sx = par[b + 2u];
    let sy = par[b + 3u];
    let th = par[b + 4u];
    let beta = par[b + 9u];
    let ct = cos(th);
    let st = sin(th);
    let d = p - vec2(par[b], par[b + 1u]);

    var e: Ev;
    e.u = ( ct * d.x + st * d.y) / sx;
    e.v = (-st * d.x + ct * d.y) / sy;
    let env = envelope(beta);
    if (abs(e.u) > env || abs(e.v) > env) {
        e.hit = false;
        e.alpha = 0.0;
        e.g = 0.0;
        e.q = 0.0;
        return e;
    }
    e.hit = true;
    e.q = e.u * e.u + e.v * e.v;
    e.g = exp(-0.5 * pow(e.q, beta));
    e.alpha = par[b + 8u] * e.g;
    return e;
}

fn colour(k: u32) -> vec3<f32> {
    let b = k * STRIDE;
    return vec3(par[b + 5u], par[b + 6u], par[b + 7u]);
}

@compute @workgroup_size(16, 16)
fn backward(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res = vec2<u32>(P.res);
    if (gid.x >= res.x || gid.y >= res.y) { return; }

    let tx = gid.x / P.tile;
    let ty = gid.y / P.tile;
    let t0 = tile_off[ty * P.tiles_x + tx];
    let t1 = tile_off[ty * P.tiles_x + tx + 1u];
    if (t0 == t1) { return; }

    let p = (vec2(f32(gid.x), f32(gid.y)) + 0.5) / P.unit;

    // ── forward ──
    var C = P.bg.rgb;
    for (var i = t0; i < t1; i = i + 1u) {
        let e = eval(tile_idx[i], p);
        if (!e.hit) { continue; }
        C = e.alpha * colour(tile_idx[i]) + (1.0 - e.alpha) * C;
    }

    let T = textureLoad(tgt, vec2<i32>(gid.xy), 0).rgb;
    let dLdC = 2.0 * (C - T);          // per-pixel; the mean is taken on the CPU

    // ── backward ──
    var Tr = 1.0;                       // transmittance in front of the current primitive
    var U = vec3(0.0);                  // colour already accumulated in front of it
    for (var i = t1; i > t0; i = i - 1u) {
        let k = tile_idx[i - 1u];
        let e = eval(k, p);
        if (!e.hit) { continue; }
        let b = k * STRIDE;
        let c = colour(k);

        let Tn = Tr * (1.0 - e.alpha);          // transmittance behind
        let Un = U + Tr * e.alpha * c;
        let behind = (C - Un) / max(Tn, T_FLOOR);

        let g = dLdC * Tr;
        add_grad(b + 5u, g.x * e.alpha);
        add_grad(b + 6u, g.y * e.alpha);
        add_grad(b + 7u, g.z * e.alpha);

        let dL_dalpha = dot(g, c - behind);
        add_grad(b + 8u, dL_dalpha * e.g);      // stored opacity

        // through the falloff
        let beta = par[b + 9u];
        let dL_dG = dL_dalpha * par[b + 8u];
        if (e.q > 1e-12) {
            let qb = pow(e.q, beta);
            add_grad(b + 9u, dL_dG * e.g * (-0.5 * qb * log(e.q)));
            let dL_dq = dL_dG * e.g * (-0.5 * beta * pow(e.q, beta - 1.0));

            // through the primitive's own frame
            let sx = par[b + 2u];
            let sy = par[b + 3u];
            let ct = cos(par[b + 4u]);
            let st = sin(par[b + 4u]);
            let dL_du = dL_dq * 2.0 * e.u;
            let dL_dv = dL_dq * 2.0 * e.v;

            add_grad(b + 0u, dL_du * (-ct / sx) + dL_dv * ( st / sy));
            add_grad(b + 1u, dL_du * (-st / sx) + dL_dv * (-ct / sy));
            add_grad(b + 2u, dL_du * (-e.u / sx));
            add_grad(b + 3u, dL_dv * (-e.v / sy));
            add_grad(b + 4u, dL_du * (e.v * sy / sx) + dL_dv * (-e.u * sx / sy));
        }

        Tr = Tn;
        U = Un;
        if (Tr < T_FLOOR) { break; }            // everything further back is hidden
    }
}
