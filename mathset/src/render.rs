/* Offscreen decoder: a .mathset in, RGBA8 pixels out.
 *
 * Knows nothing about images, targets, fitting, or error. It only replays a
 * set. Every later stage of this project is judged against what this produces,
 * so it stays deliberately dumb.
 */
use crate::format::{MathSet, envelope, srgb_to_linear};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuSplat {
    geom: [f32; 4],  // x, y, sx, sy
    rot: [f32; 4],   // theta, alpha, beta, envelope
    color: [f32; 4], // r, g, b, -
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    res: [f32; 2],
    unit: f32,
    pad: f32,
}

pub struct Rendered {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub fn render(ms: &MathSet, w: u32, h: u32) -> Result<Rendered, String> {
    pollster::block_on(run(ms, w, h))
}

async fn run(ms: &MathSet, w: u32, h: u32) -> Result<Rendered, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
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
            // whatever this machine can actually do — --scale wants big targets
            required_limits: adapter.limits(),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("no gpu device: {e}"))?;

    // ── the set, uploaded verbatim ────────────────────────────────────────
    let gpu: Vec<GpuSplat> = ms
        .iter_splats()
        .map(|s| GpuSplat {
            geom: [s.x, s.y, s.sx, s.sy],
            rot: [s.theta, s.a, s.beta, envelope(s.beta)],
            color: [s.rgb[0], s.rgb[1], s.rgb[2], 0.0],
        })
        .collect();
    // a zero-length storage buffer is invalid; one inert splat keeps the empty
    // set renderable (it must produce the bare background, not an error)
    let upload = if gpu.is_empty() {
        vec![GpuSplat { geom: [0.0, 0.0, 1.0, 1.0], rot: [0.0, 0.0, 1.0, 1.0], color: [0.0; 4] }]
    } else {
        gpu.clone()
    };
    let splat_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("splats"),
        contents: bytemuck::cast_slice(&upload),
        usage: wgpu::BufferUsages::STORAGE,
    });

    // Pixels per normalized unit, chosen so the set's whole extent fits the
    // output. For a matching aspect this is just the output's long edge, which
    // is why --scale is exactly "the same numbers, re-evaluated finer".
    let ext_x = ms.canvas[0] as f32 / ms.unit();
    let ext_y = ms.canvas[1] as f32 / ms.unit();
    let unit = (w as f32 / ext_x).min(h as f32 / ext_y);
    let params = Params { res: [w as f32, h as f32], unit, pad: 0.0 };
    let param_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    // ── pipeline ──────────────────────────────────────────────────────────
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gauss2d"),
        source: wgpu::ShaderSource::Wgsl(include_str!("splat.wgsl").into()),
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: splat_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: param_buf.as_entire_binding() },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });

    // sRGB target: the hardware decodes on read and encodes on write, so the
    // blend below happens in linear light where it physically belongs.
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
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
                format,
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

    // ── target ────────────────────────────────────────────────────────────
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canvas"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());

    // clear values are linear light, so the sRGB bg from the file is decoded
    let bg = wgpu::Color {
        r: srgb_to_linear(ms.bg[0]) as f64,
        g: srgb_to_linear(ms.bg[1]) as f64,
        b: srgb_to_linear(ms.bg[2]) as f64,
        a: 1.0,
    };

    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(bg), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if !gpu.is_empty() {
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            // one instance per splat, in file order — the order IS the paint order
            pass.draw(0..4, 0..gpu.len() as u32);
        }
    }

    // ── readback ──────────────────────────────────────────────────────────
    let row = w * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);

    let slice = out_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("gpu poll: {e}"))?;
    rx.recv().map_err(|e| format!("readback: {e}"))?.map_err(|e| format!("map: {e}"))?;

    let data = slice.get_mapped_range().map_err(|e| format!("mapped range: {e}"))?;
    let mut rgba = Vec::with_capacity((row * h) as usize);
    for y in 0..h {
        let s = (y * padded) as usize;
        rgba.extend_from_slice(&data[s..s + row as usize]);
    }
    drop(data);
    out_buf.unmap();

    Ok(Rendered { w, h, rgba })
}
