// Decoder for primitive "gauss2d".
//
// One instanced quad per splat, sized to the 3-sigma envelope of its own
// Gaussian. The fragment stage evaluates the Gaussian analytically — there is
// no sampled footprint, no lookup table, no pixel grid anywhere in here. That
// is what lets the same file render at any resolution and gain real detail
// rather than interpolated detail.

struct Splat {
    geom:  vec4<f32>,   // x, y, sx, sy       — normalized against the long edge
    rot:   vec4<f32>,   // theta, alpha, beta, envelope
    color: vec4<f32>,   // r, g, b, -         — sRGB-encoded
};

struct Params {
    res:  vec2<f32>,    // output resolution in pixels
    unit: f32,          // pixels per normalized unit
    pad:  f32,
};

@group(0) @binding(0) var<storage, read> splats: array<Splat>;
@group(0) @binding(1) var<uniform> P: Params;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,   // position within the splat, in sigma units
    @location(1) color: vec3<f32>,   // linear light
    @location(2) alpha: f32,
    @location(3) @interpolate(flat) beta: f32,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(lo, hi, c > vec3(0.04045));
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let s = splats[ii];

    // unit quad, triangle-strip order
    var corners = array<vec2<f32>, 4>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0),
    );
    // the quad only needs to cover this primitive's own footprint — a sharp
    // one (high beta) wants a tighter box than a long-tailed one
    let local = corners[vi] * s.rot.w;

    // local sigma-space -> normalized canvas space
    let ct = cos(s.rot.x);
    let st = sin(s.rot.x);
    let scaled = local * s.geom.zw;
    let offset = vec2(scaled.x * ct - scaled.y * st, scaled.x * st + scaled.y * ct);
    let p = s.geom.xy + offset;

    // normalized -> pixels -> clip. y runs downward in canvas space.
    let px = p * P.unit;
    let ndc = vec2(px.x / P.res.x * 2.0 - 1.0, 1.0 - px.y / P.res.y * 2.0);

    var out: VsOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.local = local;
    out.color = srgb_to_linear(s.color.rgb);
    out.alpha = s.rot.y;
    out.beta = s.rot.z;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // radial in sigma-space, which is the anisotropic rotated primitive in
    // canvas space — the vertex transform already carried the covariance.
    // beta = 1 is exactly a Gaussian; larger beta squares off the edge.
    let r2 = dot(in.local, in.local);
    let g = exp(-0.5 * pow(r2, in.beta));
    return vec4(in.color, in.alpha * g);
}
