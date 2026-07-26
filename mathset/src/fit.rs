/* Stage 2: an image in, a .mathset out.
 *
 * Greedy placement. Each pass proposes a batch of candidate primitives,
 * scores every one of them in parallel against the current reconstruction,
 * and keeps the best candidate per cell — but only where it strictly lowers
 * the error. Candidates that would make the image worse are simply dropped,
 * so the reconstruction improves monotonically and the primitive list is only
 * ever appended to.
 *
 * Two properties are worth stating because they are easy to lose:
 *
 *   The working canvas is the decoder's canvas — same format, same blend, and
 *   the apply step is the decoder's own shader. A primitive is scored against
 *   exactly the pixels the decoder will produce from the emitted file.
 *
 *   The emitted order is the applied order. Winners are appended in cell
 *   index order, which is the instance order they were blended in, which is
 *   the order the decoder will replay them in.
 *
 * What this stage cannot do is refine. Once a primitive is placed its
 * parameters are fixed, and improving the image means putting another one on
 * top. That is stage 3's job, and it is why the primitive count here is
 * higher than it needs to be.
 */
use crate::format::MathSet;
use crate::render::{self, Gpu, GpuSplat, SplatPass, SplatParams};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const EBLOCK: u32 = 8; // error map block, pixels
const MAX_CELLS: u32 = 128; // per axis
const WG: u32 = 64;

pub struct Options {
    pub max_side: u32,
    pub passes: u32,
    pub budget: usize,
    pub candidates: u32,
    pub pace: f32,
    pub sigma_start: f32,
    pub sigma_end: f32,
    pub alpha_lo: f32,
    pub alpha_hi: f32,
    pub beta_max: f32,
    pub perceptual: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            max_side: 1024,
            passes: 220,
            budget: 200_000, // a safety cap; the size schedule is the real control
            candidates: 32,
            // Low, and it matters more than anything else here. Every cell
            // placing at once means neighbouring primitives were each scored
            // against a canvas the others then changed, and the fit collapses.
            pace: 0.08,
            sigma_start: 0.11,
            sigma_end: 0.0045,
            alpha_lo: 0.35,
            alpha_hi: 1.0,
            beta_max: 6.0,
            // Off by default: see docs/fitting.md. Weighting linear error by
            // the sRGB slope assumes the residual is small, and early in a fit
            // it is not, so it measurably loses.
            perceptual: false,
        }
    }
}

pub struct Report {
    pub w: u32,
    pub h: u32,
    pub target: Vec<u8>, // the resized source, RGBA8, what the fit was aiming at
    pub passes_run: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FitParams {
    res: [f32; 2],
    cells: [u32; 2],
    unit: f32,
    m: u32,
    pass_idx: u32,
    sigma: f32,
    sigma_min: f32,
    sigma_max: f32,
    alpha_lo: f32,
    alpha_hi: f32,
    ebw: u32,
    ebh: u32,
    eblock: u32,
    perceptual: u32,
    pace: f32,
    beta_max: f32,
    jitter: [f32; 2],
    _pad: [f32; 4],
}

/// Deterministic per-pass jitter. Anything that varies between passes has to
/// be reproducible, or two runs of the same fit diverge for no stated reason.
fn hash01(n: u32) -> f32 {
    let s = n.wrapping_mul(747796405).wrapping_add(2891336453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277803737);
    ((w >> 22) ^ w) as f32 / 4294967296.0
}

pub fn fit(
    gpu: &Gpu,
    img: &image::DynamicImage,
    opt: &Options,
    progress: &mut dyn FnMut(u32, u32, usize),
) -> Result<(MathSet, Report), String> {
    // ── working resolution ────────────────────────────────────────────────
    let (iw, ih) = (img.width(), img.height());
    let long = iw.max(ih);
    let k = if long > opt.max_side { opt.max_side as f32 / long as f32 } else { 1.0 };
    let w = ((iw as f32 * k).round() as u32).max(8);
    let h = ((ih as f32 * k).round() as u32).max(8);
    let resized = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3).to_rgba8();
    let target: Vec<u8> = resized.as_raw().clone();

    // Background is the target's mean. Starting from the average colour means
    // the first primitives correct a real image rather than paint onto void.
    let mut acc = [0f64; 3];
    for px in target.chunks_exact(4) {
        for c in 0..3 {
            acc[c] += crate::format::srgb_to_linear(px[c] as f32 / 255.0) as f64;
        }
    }
    let n = (target.len() / 4) as f64;
    let bg = [
        crate::format::linear_to_srgb((acc[0] / n) as f32),
        crate::format::linear_to_srgb((acc[1] / n) as f32),
        crate::format::linear_to_srgb((acc[2] / n) as f32),
    ];

    let dev = &gpu.device;
    let unit = w.max(h) as f32;
    let ext = [w as f32 / unit, h as f32 / unit];

    // ── textures ──────────────────────────────────────────────────────────
    let tex_tgt = render::canvas_texture(dev, w, h, "target");
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex_tgt,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &target,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    let tex_cur = render::canvas_texture(dev, w, h, "current");
    let view_cur = tex_cur.create_view(&Default::default());
    let view_tgt = tex_tgt.create_view(&Default::default());
    {
        let mut enc = dev.create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("prime"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view_cur,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(render::clear_color(bg)),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        gpu.queue.submit([enc.finish()]);
    }

    // ── buffers ───────────────────────────────────────────────────────────
    let ebw = w.div_ceil(EBLOCK);
    let ebh = h.div_ceil(EBLOCK);
    let max_cells = (MAX_CELLS * MAX_CELLS) as u64;
    let splat_bytes = std::mem::size_of::<GpuSplat>() as u64;

    let buf_err = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("errmap"),
        size: (ebw * ebh) as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let buf_cand = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("candidates"),
        size: max_cells * opt.candidates as u64 * splat_bytes,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let buf_win = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("winners"),
        size: max_cells * splat_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let buf_stage = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("winners readback"),
        size: max_cells * splat_bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let buf_fp = dev.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fit params"),
        size: std::mem::size_of::<FitParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // ── compute pipelines ─────────────────────────────────────────────────
    let shader = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fit"),
        source: wgpu::ShaderSource::Wgsl(include_str!("fit.wgsl").into()),
    });
    let tex_entry = |b: u32| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let store_entry = |b: u32| wgpu::BindGroupLayoutEntry {
        binding: b,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fit"),
        entries: &[
            tex_entry(0),
            tex_entry(1),
            store_entry(2),
            store_entry(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            store_entry(5),
        ],
    });
    let bind = dev.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fit"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view_tgt) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view_cur) },
            wgpu::BindGroupEntry { binding: 2, resource: buf_err.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: buf_cand.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: buf_fp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: buf_win.as_entire_binding() },
        ],
    });
    let play = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let mk = |entry: &str| {
        dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&play),
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let pipe_err = mk("errmap_main");
    let pipe_score = mk("score_main");
    let pipe_reduce = mk("reduce_main");

    // ── apply pass: the decoder's own pipeline, fed from the winners ──────
    let splat = SplatPass::new(dev);
    let sp = SplatParams { res: [w as f32, h as f32], unit, pad: 0.0 };
    let buf_sp = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("splat params"),
        contents: bytemuck::bytes_of(&sp),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_apply = splat.bind(dev, &buf_win, &buf_sp);

    // ── the loop ──────────────────────────────────────────────────────────
    let mut splats: Vec<Vec<f32>> = Vec::new();
    let mut passes_run = 0;

    for p in 0..opt.passes {
        if splats.len() >= opt.budget {
            break;
        }
        passes_run = p + 1;

        // Coarse to fine: a geometric schedule, so early passes lay down the
        // broad structure and later ones only have detail left to correct.
        let t = if opt.passes > 1 { p as f32 / (opt.passes - 1) as f32 } else { 1.0 };
        let sigma = opt.sigma_start * (opt.sigma_end / opt.sigma_start).powf(t);

        // Cells track the primitive size. Fixed cells would let big early
        // primitives pile up inside one cell, and each candidate's score
        // assumes the canvas is unchanged by its neighbours in the same pass.
        let cells = [
            ((ext[0] / (2.2 * sigma)).round() as u32).clamp(2, MAX_CELLS),
            ((ext[1] / (2.2 * sigma)).round() as u32).clamp(2, MAX_CELLS),
        ];
        let ncells = cells[0] * cells[1];

        let fp = FitParams {
            res: [w as f32, h as f32],
            cells,
            unit,
            m: opt.candidates,
            pass_idx: p,
            sigma,
            sigma_min: 0.55 / unit, // never smaller than about half a pixel
            sigma_max: 0.30,
            alpha_lo: opt.alpha_lo,
            alpha_hi: opt.alpha_hi,
            ebw,
            ebh,
            eblock: EBLOCK,
            perceptual: opt.perceptual as u32,
            pace: opt.pace,
            beta_max: opt.beta_max,
            jitter: [hash01(p * 2 + 1) - 0.5, hash01(p * 2 + 2) - 0.5],
            _pad: [0.0; 4],
        };
        gpu.queue.write_buffer(&buf_fp, 0, bytemuck::bytes_of(&fp));

        let mut enc = dev.create_command_encoder(&Default::default());
        {
            let mut cp = enc.begin_compute_pass(&Default::default());
            cp.set_bind_group(0, &bind, &[]);
            cp.set_pipeline(&pipe_err);
            cp.dispatch_workgroups(ebw.div_ceil(8), ebh.div_ceil(8), 1);
            cp.set_pipeline(&pipe_score);
            cp.dispatch_workgroups((ncells * opt.candidates).div_ceil(WG), 1, 1);
            cp.set_pipeline(&pipe_reduce);
            cp.dispatch_workgroups(ncells.div_ceil(WG), 1, 1);
        }
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("apply"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view_cur,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&splat.pipeline);
            rp.set_bind_group(0, &bind_apply, &[]);
            // Rejected cells hold an alpha-zero entry, which blends to an
            // exact no-op — so every cell can be drawn without branching.
            rp.draw(0..4, 0..ncells);
        }
        let bytes = ncells as u64 * splat_bytes;
        enc.copy_buffer_to_buffer(&buf_win, 0, &buf_stage, 0, bytes);
        gpu.queue.submit([enc.finish()]);

        // Read the winners back in cell order — the same order they were just
        // blended in, which is the order the decoder must replay them in.
        let slice = buf_stage.slice(0..bytes);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.wait()?;
        rx.recv().map_err(|e| format!("winners: {e}"))?.map_err(|e| format!("map: {e}"))?;
        {
            let data = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?;
            let won: &[GpuSplat] = bytemuck::cast_slice(&data);
            for s in won {
                if s.rot[1] <= 0.0 {
                    continue; // rejected cell
                }
                splats.push(vec![
                    s.geom[0], s.geom[1], s.geom[2], s.geom[3], s.rot[0],
                    s.color[0], s.color[1], s.color[2], s.rot[1], s.rot[2],
                ]);
                if splats.len() >= opt.budget {
                    break;
                }
            }
        }
        buf_stage.unmap();

        progress(p + 1, opt.passes, splats.len());
    }

    let ms = MathSet {
        mathset: crate::format::VERSION,
        canvas: [w, h],
        space: "srgb".into(),
        bg,
        primitive: crate::format::GAUSS2D.into(),
        splats,
    };
    Ok((ms, Report { w, h, target, passes_run }))
}

/// Peak signal-to-noise ratio between two RGBA8 buffers, over RGB only.
pub fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mut se = 0f64;
    let mut n = 0f64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..3 {
            let d = pa[c] as f64 - pb[c] as f64;
            se += d * d;
            n += 1.0;
        }
    }
    if se == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0 * 255.0 / (se / n)).log10()
}
