/* Offscreen decoder: a .mathset in, RGBA8 pixels out.
 *
 * Knows nothing about images, targets, fitting, or error. It only replays a
 * set. Every later stage of this project is judged against what this produces,
 * so it stays deliberately dumb.
 *
 * The splat pipeline is exposed rather than private because the fitter applies
 * accepted primitives with it too. That is deliberate: if the fitter had its
 * own copy of the compositing rule, its accept decisions would be scored
 * against a canvas the decoder does not produce, and every fit would be
 * subtly wrong in a way nothing would catch.
 */
use crate::format::{MathSet, Splat, envelope, srgb_to_linear};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Both the decoder's output and the fitter's working canvas. sRGB so the
/// hardware decodes on read and encodes on write, putting the blend in linear
/// light where it belongs.
pub const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct GpuSplat {
    pub geom: [f32; 4],  // x, y, sx, sy
    pub rot: [f32; 4],   // theta, alpha, beta, envelope
    pub color: [f32; 4], // r, g, b (sRGB), spare
}

impl GpuSplat {
    pub fn new(s: &Splat) -> Self {
        GpuSplat {
            geom: [s.x, s.y, s.sx, s.sy],
            rot: [s.theta, s.a, s.beta, envelope(s.beta)],
            color: [s.rgb[0], s.rgb[1], s.rgb[2], 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SplatParams {
    pub res: [f32; 2],
    pub unit: f32,
    pub pad: f32,
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        pollster::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("no gpu adapter: {e}"))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("mathset"),
                    required_limits: adapter.limits(),
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("no gpu device: {e}"))?;
            Ok(Gpu { device, queue })
        })
    }

    pub fn wait(&self) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| format!("gpu poll: {e}"))
    }
}

/// The compositing pass — the one place primitives become pixels.
pub struct SplatPass {
    pub pipeline: wgpu::RenderPipeline,
    pub bgl: wgpu::BindGroupLayout,
}

impl SplatPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gauss2d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("splat.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("splat"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("splat"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: CANVAS_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        SplatPass { pipeline, bgl }
    }

    pub fn bind(
        &self,
        device: &wgpu::Device,
        splats: &wgpu::Buffer,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: splats.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
            ],
        })
    }
}

pub fn canvas_texture(device: &wgpu::Device, w: u32, h: u32, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CANVAS_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Clear values are linear light, so a stored sRGB colour is decoded first.
pub fn clear_color(rgb: [f32; 3]) -> wgpu::Color {
    wgpu::Color {
        r: srgb_to_linear(rgb[0]) as f64,
        g: srgb_to_linear(rgb[1]) as f64,
        b: srgb_to_linear(rgb[2]) as f64,
        a: 1.0,
    }
}

/// Pixels per normalized unit, chosen so the set's whole extent fits the
/// output. For a matching aspect this is just the output's long edge, which
/// is why --scale is exactly "the same numbers, re-evaluated finer".
pub fn unit_for(canvas: [u32; 2], w: u32, h: u32) -> f32 {
    let long = canvas[0].max(canvas[1]) as f32;
    let ext_x = canvas[0] as f32 / long;
    let ext_y = canvas[1] as f32 / long;
    (w as f32 / ext_x).min(h as f32 / ext_y)
}

/// Pull a canvas texture back to CPU memory as tightly packed RGBA8.
pub fn read_canvas(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let row = w * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);

    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.wait()?;
    rx.recv().map_err(|e| format!("readback: {e}"))?.map_err(|e| format!("map: {e}"))?;

    let data = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?;
    let mut rgba = Vec::with_capacity((row * h) as usize);
    for y in 0..h {
        let s = (y * padded) as usize;
        rgba.extend_from_slice(&data[s..s + row as usize]);
    }
    drop(data);
    buf.unmap();
    Ok(rgba)
}

/// `limit` draws only the first N primitives — the set is an ordered sequence,
/// so a prefix of it is a valid set in its own right: the same image, earlier.
pub fn render_with(
    gpu: &Gpu,
    ms: &MathSet,
    w: u32,
    h: u32,
    limit: Option<usize>,
) -> Result<Vec<u8>, String> {
    let pass = SplatPass::new(&gpu.device);

    let n = limit.unwrap_or(usize::MAX).min(ms.splats.len());
    let gpu_splats: Vec<GpuSplat> =
        ms.iter_splats().take(n).map(|s| GpuSplat::new(&s)).collect();
    // a zero-length storage buffer is invalid; one inert entry keeps the empty
    // set renderable (it must produce the bare background, not an error)
    let upload = if gpu_splats.is_empty() {
        vec![GpuSplat { geom: [0.0, 0.0, 1.0, 1.0], rot: [0.0, 0.0, 1.0, 1.0], color: [0.0; 4] }]
    } else {
        gpu_splats.clone()
    };
    let splat_buf = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("splats"),
        contents: bytemuck::cast_slice(&upload),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let params = SplatParams {
        res: [w as f32, h as f32],
        unit: unit_for(ms.canvas, w, h),
        pad: 0.0,
    };
    let param_buf = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind = pass.bind(&gpu.device, &splat_buf, &param_buf);

    let tex = canvas_texture(&gpu.device, w, h, "canvas");
    let view = tex.create_view(&Default::default());

    let mut enc = gpu.device.create_command_encoder(&Default::default());
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color(ms.bg)),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if !gpu_splats.is_empty() {
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &bind, &[]);
            // one instance per primitive, in file order — the order IS the paint order
            rp.draw(0..4, 0..gpu_splats.len() as u32);
        }
    }
    gpu.queue.submit([enc.finish()]);

    read_canvas(gpu, &tex, w, h)
}
