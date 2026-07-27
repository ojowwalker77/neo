/* Stage 3: gradient refinement.
 *
 * Greedy placement can put a primitive down but never adjust it, so the only
 * way it can fix an error is to stack another primitive on top. That inflates
 * the count without improving the description. Refinement moves the
 * primitives that already exist.
 *
 * Every parameter is differentiable — position, extents, rotation, colour,
 * opacity and the shape exponent — and the derivatives are short closed forms
 * (see refine.wgsl). No autodiff framework is involved.
 *
 * Division of labour: the GPU computes gradients, the CPU holds the
 * parameters and takes the Adam step. The parameter block is a few hundred
 * kilobytes, so the round trip costs less than the kernel does, and it keeps
 * the optimiser itself in code that is easy to read and to check.
 */
use crate::format::{MathSet, envelope, linear_to_srgb, srgb_to_linear};
use crate::render::Gpu;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const STRIDE: usize = 10;  // x y sx sy theta r g b a beta
const GSTRIDE: usize = 11; // the ten gradients, plus what the primitive is worth
const TILE: u32 = 16;

pub struct Options {
    pub iters: u32,
    pub lr_pos: f32,
    pub lr_scale: f32,
    pub lr_rot: f32,
    pub lr_colour: f32,
    pub lr_alpha: f32,
    pub lr_beta: f32,
    pub beta_min: f32,
    pub beta_max: f32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            iters: 300,
            lr_pos: 2e-4,
            lr_scale: 6e-3, // log space
            lr_rot: 8e-3,
            lr_colour: 8e-3,
            lr_alpha: 4e-3,
            lr_beta: 6e-3, // log space
            beta_min: 0.8,
            beta_max: 16.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uni {
    res: [f32; 2],
    unit: f32,
    tile: u32,
    tiles_x: u32,
    n: u32,
    _p: [u32; 2],
    bg: [f32; 4],
}

/// Adam, one state pair per parameter. Plain enough to check by eye.
struct Adam {
    m: Vec<f32>,
    v: Vec<f32>,
    t: i32,
}

impl Adam {
    fn new(n: usize) -> Self {
        Adam { m: vec![0.0; n], v: vec![0.0; n], t: 0 }
    }
    fn step(&mut self, i: usize, g: f32, lr: f32) -> f32 {
        const B1: f32 = 0.9;
        const B2: f32 = 0.999;
        self.m[i] = B1 * self.m[i] + (1.0 - B1) * g;
        self.v[i] = B2 * self.v[i] + (1.0 - B2) * g * g;
        let mh = self.m[i] / (1.0 - B1.powi(self.t));
        let vh = self.v[i] / (1.0 - B2.powi(self.t));
        -lr * mh / (vh.sqrt() + 1e-8)
    }
}

/// Which primitives touch which tile, in primitive order. Built on the CPU
/// because it is trivial there and because building it in primitive order is
/// what keeps the composite order exact without a sort.
fn bin(par: &[f32], n: usize, unit: f32, tx: u32, ty: u32) -> (Vec<u32>, Vec<u32>) {
    let ntiles = (tx * ty) as usize;
    let mut counts = vec![0u32; ntiles + 1];
    let mut spans = Vec::with_capacity(n);

    for k in 0..n {
        let b = k * STRIDE;
        let (x, y, sx, sy, th) = (par[b], par[b + 1], par[b + 2], par[b + 3], par[b + 4]);
        let env = envelope(par[b + 9]);
        let (ct, st) = (th.cos(), th.sin());
        let hx = env * ((sx * ct).abs() + (sy * st).abs()) * unit;
        let hy = env * ((sx * st).abs() + (sy * ct).abs()) * unit;
        let x0 = (((x * unit - hx) / TILE as f32).floor().max(0.0) as u32).min(tx - 1);
        let x1 = (((x * unit + hx) / TILE as f32).floor().max(0.0) as u32).min(tx - 1);
        let y0 = (((y * unit - hy) / TILE as f32).floor().max(0.0) as u32).min(ty - 1);
        let y1 = (((y * unit + hy) / TILE as f32).floor().max(0.0) as u32).min(ty - 1);
        // a primitive entirely off canvas still clamps to an edge tile; the
        // per-pixel envelope test rejects it there, so it costs a test, not a
        // wrong result
        spans.push((x0, x1, y0, y1));
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                counts[(gy * tx + gx) as usize + 1] += 1;
            }
        }
    }
    for i in 0..ntiles {
        counts[i + 1] += counts[i];
    }
    let off = counts.clone();
    let mut cursor = counts;
    let mut idx = vec![0u32; off[ntiles] as usize];
    for (k, &(x0, x1, y0, y1)) in spans.iter().enumerate() {
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                let t = (gy * tx + gx) as usize;
                idx[cursor[t] as usize] = k as u32;
                cursor[t] += 1;
            }
        }
    }
    (off, idx)
}

pub struct Refiner {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    view_tgt: wgpu::TextureView,
    w: u32,
    h: u32,
    unit: f32,
    tiles: (u32, u32),
    bg: [f32; 4],
    n: usize,
    target: Vec<u8>,
    par: Vec<f32>,
    adam: Adam,
    last_grad: Vec<f32>,
    opt: Options,
}

impl Refiner {
    pub fn new(
        gpu: &Gpu,
        ms: &MathSet,
        target: &[u8],
        w: u32,
        h: u32,
        opt: Options,
    ) -> Result<Self, String> {
        let dev = &gpu.device;
        let tex = crate::render::canvas_texture(dev, w, h, "refine target");
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            target,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        // parameters live in linear light; the file stores sRGB
        let n = ms.splats.len();
        let mut par = vec![0f32; n * STRIDE];
        for (k, s) in ms.iter_splats().enumerate() {
            let b = k * STRIDE;
            par[b] = s.x;
            par[b + 1] = s.y;
            par[b + 2] = s.sx;
            par[b + 3] = s.sy;
            par[b + 4] = s.theta;
            par[b + 5] = srgb_to_linear(s.rgb[0]);
            par[b + 6] = srgb_to_linear(s.rgb[1]);
            par[b + 7] = srgb_to_linear(s.rgb[2]);
            par[b + 8] = s.a.clamp(0.002, 0.99);
            par[b + 9] = s.beta.clamp(opt.beta_min, opt.beta_max);
        }

        let shader = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("refine"),
            source: wgpu::ShaderSource::Wgsl(include_str!("refine.wgsl").into()),
        });
        let store = |b: u32, ro: bool| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: ro },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refine"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                store(1, true),
                store(2, false),
                store(3, true),
                store(4, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("backward"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("backward"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Refiner {
            pipeline,
            bgl,
            view_tgt: tex.create_view(&Default::default()),
            w,
            h,
            // Match the ordinary decoder exactly. At a reduced working size,
            // rounding the short edge can make max(w,h) differ slightly from
            // the scale implied by the reference canvas aspect ratio.
            unit: crate::render::unit_for(ms.canvas, w, h),
            tiles: (w.div_ceil(TILE), h.div_ceil(TILE)),
            bg: [
                srgb_to_linear(ms.bg[0]),
                srgb_to_linear(ms.bg[1]),
                srgb_to_linear(ms.bg[2]),
                1.0,
            ],
            n,
            target: target.to_vec(),
            par,
            adam: Adam::new(n * STRIDE),
            last_grad: vec![0.0; n * GSTRIDE],
            opt,
        })
    }

    /// One gradient evaluation and one Adam step.
    pub fn step(&mut self, gpu: &Gpu) -> Result<(), String> {
        let g = self.gradients(gpu)?;
        self.update(&g);
        Ok(())
    }

    /// Raw dL/dparam for every primitive, unscaled.
    pub fn gradients(&mut self, gpu: &Gpu) -> Result<Vec<f32>, String> {
        let dev = &gpu.device;
        let (off, idx) = bin(&self.par, self.n, self.unit, self.tiles.0, self.tiles.1);

        let b_par = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("par"),
            contents: bytemuck::cast_slice(&self.par),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let b_grad = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grad"),
            contents: bytemuck::cast_slice(&vec![0f32; self.n * GSTRIDE]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let b_off = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tile off"),
            contents: bytemuck::cast_slice(&off),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let b_idx = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tile idx"),
            contents: bytemuck::cast_slice(if idx.is_empty() { &[0u32][..] } else { &idx[..] }),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let uni = Uni {
            res: [self.w as f32, self.h as f32],
            unit: self.unit,
            tile: TILE,
            tiles_x: self.tiles.0,
            n: self.n as u32,
            _p: [0; 2],
            bg: self.bg,
        };
        let b_uni = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uni"),
            contents: bytemuck::bytes_of(&uni),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.view_tgt),
                },
                wgpu::BindGroupEntry { binding: 1, resource: b_par.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: b_grad.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: b_off.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: b_idx.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: b_uni.as_entire_binding() },
            ],
        });

        let bytes = (self.n * GSTRIDE * 4) as u64;
        let stage = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grad readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = dev.create_command_encoder(&Default::default());
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_pipeline(&self.pipeline);
            cp.set_bind_group(0, &bind, &[]);
            cp.dispatch_workgroups(self.tiles.0, self.tiles.1, 1);
        }
        enc.copy_buffer_to_buffer(&b_grad, 0, &stage, 0, bytes);
        gpu.queue.submit([enc.finish()]);

        let slice = stage.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.wait()?;
        rx.recv().map_err(|e| format!("grad: {e}"))?.map_err(|e| format!("map: {e}"))?;
        let g: Vec<f32> = {
            let d = slice.get_mapped_range().map_err(|e| format!("mapped: {e}"))?;
            bytemuck::cast_slice::<u8, f32>(&d).to_vec()
        };
        stage.unmap();
        self.last_grad = g.clone();
        Ok(g)
    }

    fn update(&mut self, g: &[f32]) {
        let o = &self.opt;
        let scale = 1.0 / (self.w as f32 * self.h as f32);
        self.adam.t += 1;
        for k in 0..self.n {
            let b = k * STRIDE;
            let gb = k * GSTRIDE;
            let gr = |i: usize| {
                let v = g[gb + i] * scale;
                if v.is_finite() { v } else { 0.0 }
            };

            self.par[b] += self.adam.step(b, gr(0), o.lr_pos);
            self.par[b + 1] += self.adam.step(b + 1, gr(1), o.lr_pos);

            // extents and beta step in log space: positivity is then free, and
            // a step means the same proportional change at every scale
            for (i, lr) in [(2usize, o.lr_scale), (3, o.lr_scale), (9, o.lr_beta)] {
                let cur = self.par[b + i];
                let d = self.adam.step(b + i, gr(i) * cur, lr);
                self.par[b + i] = (cur.ln() + d).exp();
            }

            self.par[b + 4] += self.adam.step(b + 4, gr(4), o.lr_rot);
            for i in 5..8 {
                self.par[b + i] =
                    (self.par[b + i] + self.adam.step(b + i, gr(i), o.lr_colour)).clamp(0.0, 1.0);
            }
            // opacity stays strictly below 1: the backward pass divides by the
            // transmittance behind a primitive, and alpha = 1 would erase it
            self.par[b + 8] =
                (self.par[b + 8] + self.adam.step(b + 8, gr(8), o.lr_alpha)).clamp(0.002, 0.99);
            self.par[b + 2] = self.par[b + 2].clamp(0.3 / self.unit, 0.4);
            self.par[b + 3] = self.par[b + 3].clamp(0.3 / self.unit, 0.4);
            self.par[b + 9] = self.par[b + 9].clamp(o.beta_min, o.beta_max);
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// Prune and split in one move, from a single gradient snapshot.
    ///
    /// They have to be decided together. Pruning first and then splitting
    /// would consult gradients belonging to primitives that no longer exist,
    /// and splitting first would waste the budget on primitives about to be
    /// dropped.
    ///
    /// `frac` is the share of the set retired per cycle, taken from the
    /// bottom by worth. Gentle and repeated beats aggressive and once: every
    /// structural change disturbs the optimiser around it, and the set needs
    /// iterations to settle before it can be judged again.
    pub fn adapt(&mut self, frac: f32, target: usize) -> (usize, usize) {
        if self.last_grad.len() < self.n * GSTRIDE || self.n < 8 {
            return (0, 0);
        }
        let fin = |v: f32| if v.is_finite() { v } else { 0.0 };
        let worth: Vec<f32> = (0..self.n).map(|k| fin(self.last_grad[k * GSTRIDE + 10])).collect();

        // Retire the bottom slice by worth, and anything at or below zero
        // however many that is — those are paying nothing or actively costing.
        let mut order: Vec<usize> = (0..self.n).collect();
        order.sort_by(|&a, &b| worth[a].partial_cmp(&worth[b]).unwrap_or(std::cmp::Ordering::Equal));
        let quota = ((self.n as f32 * frac) as usize).min(self.n / 2);
        let mut drop = vec![false; self.n];
        for (rank, &k) in order.iter().enumerate() {
            if rank < quota || worth[k] <= 0.0 {
                drop[k] = true;
            }
        }
        let survivors = self.n - drop.iter().filter(|d| **d).count();
        if survivors < 4 {
            return (0, 0);
        }

        // Spend the freed budget on the survivors being pulled hardest: a
        // large positional gradient means one primitive is being asked to
        // cover two things at once.
        let room = target.saturating_sub(survivors) / 2;
        let mut rank: Vec<(usize, f32)> = (0..self.n)
            .filter(|&k| !drop[k])
            .map(|k| {
                let gx = fin(self.last_grad[k * GSTRIDE]);
                let gy = fin(self.last_grad[k * GSTRIDE + 1]);
                (k, (gx * gx + gy * gy).sqrt())
            })
            .collect();
        rank.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let split: std::collections::HashSet<usize> =
            rank.iter().take(room).filter(|r| r.1 > 0.0).map(|r| r.0).collect();

        let mut par: Vec<f32> = Vec::with_capacity((survivors + split.len()) * STRIDE);
        let mut m: Vec<f32> = Vec::with_capacity(par.capacity());
        let mut v: Vec<f32> = Vec::with_capacity(par.capacity());
        for k in 0..self.n {
            if drop[k] {
                continue;
            }
            let src = &self.par[k * STRIDE..(k + 1) * STRIDE];
            if !split.contains(&k) {
                par.extend_from_slice(src);
                // Adam moments are per parameter, so they travel with the
                // primitive rather than with the slot
                m.extend_from_slice(&self.adam.m[k * STRIDE..(k + 1) * STRIDE]);
                v.extend_from_slice(&self.adam.v[k * STRIDE..(k + 1) * STRIDE]);
                continue;
            }
            // two halves, offset along the primitive's own major axis, each
            // narrower — both take the original's place in the sequence, so
            // the composite order is undisturbed
            let (ct, st) = (src[4].cos(), src[4].sin());
            let along_x = src[2] >= src[3];
            let major = if along_x { src[2] } else { src[3] };
            let dir = if along_x { (ct, st) } else { (-st, ct) };
            for sign in [-1.0f32, 1.0] {
                let mut c = src.to_vec();
                c[0] += sign * 0.55 * major * dir.0;
                c[1] += sign * 0.55 * major * dir.1;
                if along_x { c[2] /= 1.6 } else { c[3] /= 1.6 }
                par.extend_from_slice(&c);
                m.extend(std::iter::repeat_n(0.0, STRIDE)); // fresh state: this primitive is new
                v.extend(std::iter::repeat_n(0.0, STRIDE));
            }
        }

        let dropped = self.n - survivors;
        let added = par.len() / STRIDE - survivors;
        self.par = par;
        self.adam.m = m;
        self.adam.v = v;
        self.n = self.par.len() / STRIDE;
        self.last_grad = vec![0.0; self.n * GSTRIDE];
        (dropped, added)
    }

    /// What the kernel says each primitive is worth: the loss increase that
    /// removing it would cause.
    pub fn worth(&self) -> Vec<f32> {
        (0..self.n).map(|k| self.last_grad[k * GSTRIDE + 10]).collect()
    }

    /// Remove one primitive, for checking a worth score against reality.
    pub fn drop_one(&mut self, k: usize) {
        self.par.drain(k * STRIDE..(k + 1) * STRIDE);
        self.adam.m.drain(k * STRIDE..(k + 1) * STRIDE);
        self.adam.v.drain(k * STRIDE..(k + 1) * STRIDE);
        self.n -= 1;
    }

    pub fn params_mut(&mut self) -> &mut [f32] {
        &mut self.par
    }

    /// The loss the kernel differentiates — linear compositing over the
    /// background, summed squared error, no 8-bit quantisation. Written
    /// independently of the shader so the two can be compared.
    pub fn loss(&self) -> f64 {
        let mut total = 0f64;
        for py in 0..self.h {
            for px in 0..self.w {
                let p = [
                    (px as f32 + 0.5) / self.unit,
                    (py as f32 + 0.5) / self.unit,
                ];
                let mut c = [self.bg[0] as f64, self.bg[1] as f64, self.bg[2] as f64];
                for k in 0..self.n {
                    let b = k * STRIDE;
                    let (sx, sy, th) = (self.par[b + 2], self.par[b + 3], self.par[b + 4]);
                    let (ct, st) = (th.cos(), th.sin());
                    let d = [p[0] - self.par[b], p[1] - self.par[b + 1]];
                    let u = (ct * d[0] + st * d[1]) / sx;
                    let v = (-st * d[0] + ct * d[1]) / sy;
                    let beta = self.par[b + 9];
                    let env = envelope(beta);
                    if u.abs() > env || v.abs() > env {
                        continue;
                    }
                    let q = (u * u + v * v) as f64;
                    let a = (self.par[b + 8] as f64) * (-0.5 * q.powf(beta as f64)).exp();
                    for i in 0..3 {
                        c[i] = a * self.par[b + 5 + i] as f64 + (1.0 - a) * c[i];
                    }
                }
                let o = ((py * self.w + px) * 4) as usize;
                for i in 0..3 {
                    let t = srgb_to_linear(self.target[o + i] as f32 / 255.0) as f64;
                    total += (c[i] - t) * (c[i] - t);
                }
            }
        }
        total
    }

    /// The current parameters as a set the ordinary decoder can read.
    pub fn to_mathset(&self, template: &MathSet) -> MathSet {
        let splats = (0..self.n)
            .map(|k| {
                let b = k * STRIDE;
                vec![
                    self.par[b],
                    self.par[b + 1],
                    self.par[b + 2],
                    self.par[b + 3],
                    self.par[b + 4],
                    linear_to_srgb(self.par[b + 5]),
                    linear_to_srgb(self.par[b + 6]),
                    linear_to_srgb(self.par[b + 7]),
                    self.par[b + 8],
                    self.par[b + 9],
                ]
            })
            .collect();
        MathSet {
            mathset: template.mathset,
            canvas: template.canvas,
            space: template.space.clone(),
            bg: template.bg,
            primitive: template.primitive.clone(),
            splats,
        }
    }
}
