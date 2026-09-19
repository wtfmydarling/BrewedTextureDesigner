//! Draws the 2D view: the engine's mip-mapped image, sampled directly on the GPU with a
//! small render pipeline inside iced's render pass (channel isolation, checkerboard for
//! transparency, repeated copies for the tiling preview).

use iced::widget::shader::{self, Viewport};
use iced::{Rectangle, mouse};
use tessera_engine::wgpu;

use crate::app::Message;

const SHADER: &str = r#"
struct U {
    rect: vec4<f32>,      // image rectangle in the widget, normalized: x0, y0, x1, y1
    size: vec2<f32>,      // widget size in physical pixels
    checker: f32,         // checker cell size in physical pixels
    channel: u32,         // 0 RGBA, 1 RGB, 2 R, 3 G, 4 B, 5 A
    flags: u32,           // 1 tile x, 2 tile y, 4 sRGB target, 8 nearest
    pad0: u32, pad1: u32, pad2: u32,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var smooth_sampler: sampler;
@group(0) @binding(3) var nearest_sampler: sampler;

struct V {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) dim: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> V {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let dx = f32(i32(ii % 3u) - 1);
    let dy = f32(i32(ii / 3u) - 1);
    var out: V;
    let hidden = (dx != 0.0 && (u.flags & 1u) == 0u) || (dy != 0.0 && (u.flags & 2u) == 0u);
    let w = u.rect.z - u.rect.x;
    let h = u.rect.w - u.rect.y;
    let p = vec2<f32>(u.rect.x + (c.x + dx) * w, u.rect.y + (c.y + dy) * h);
    out.pos = vec4<f32>(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, select(1.0, 0.0, hidden));
    out.uv = c;
    out.dim = select(0.72, 1.0, dx == 0.0 && dy == 0.0);
    return out;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(in: V) -> @location(0) vec4<f32> {
    var c: vec4<f32>;
    if ((u.flags & 8u) != 0u) {
        c = textureSample(tex, nearest_sampler, in.uv);
    } else {
        c = textureSample(tex, smooth_sampler, in.uv);
    }
    let cell = vec2<i32>(floor(in.pos.xy / max(u.checker, 1.0)));
    let check = select(vec3<f32>(0.22), vec3<f32>(0.32), ((cell.x + cell.y) & 1) == 1);
    var rgb = c.rgb;
    switch u.channel {
        case 1u: { rgb = c.rgb; }
        case 2u: { rgb = vec3<f32>(c.r); }
        case 3u: { rgb = vec3<f32>(c.g); }
        case 4u: { rgb = vec3<f32>(c.b); }
        case 5u: { rgb = vec3<f32>(c.a); }
        default: { rgb = mix(check, c.rgb, clamp(c.a, 0.0, 1.0)); }
    }
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * in.dim;
    if ((u.flags & 4u) != 0u) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],
    size: [f32; 2],
    checker: f32,
    channel: u32,
    flags: u32,
    pad: [u32; 3],
}

/// What the 2D view shows (built by the view every frame).
pub struct Scene {
    pub image: Option<(wgpu::TextureView, [u32; 2])>,
    /// Image rectangle relative to the widget, in logical pixels.
    pub rect: Rectangle,
    pub channel: u32,
    pub tile: [bool; 2],
}

#[derive(Debug)]
pub struct Primitive {
    image: Option<wgpu::TextureView>,
    rect: Rectangle,
    channel: u32,
    tile: [bool; 2],
    nearest: bool,
}

impl shader::Program<Message> for Scene {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        // Zoomed in beyond 1:1, show crisp pixels.
        let nearest = self.image.as_ref().is_some_and(|(_, size)| self.rect.width > size[0] as f32 * 1.5);
        Primitive {
            image: self.image.as_ref().map(|(view, _)| view.clone()),
            rect: self.rect,
            channel: self.channel,
            tile: self.tile,
            nearest,
        }
    }
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    smooth: wgpu::Sampler,
    nearest: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    srgb: bool,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // iced created the device: hand it to the engine.
        crate::gpu::publish(device, queue, format);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tessera 2d view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tessera 2d view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tessera 2d view"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tessera 2d view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            // iced renders custom primitives into its (possibly multisampled) target.
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tessera 2d view"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = |mag| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("tessera 2d view"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: mag,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        };
        Pipeline {
            pipeline,
            layout,
            uniforms,
            smooth: sampler(wgpu::FilterMode::Linear),
            nearest: sampler(wgpu::FilterMode::Nearest),
            bind_group: None,
            srgb: format.is_srgb(),
        }
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let Some(view) = &self.image else {
            pipeline.bind_group = None;
            return;
        };
        let scale = viewport.scale_factor();
        let (w, h) = (bounds.width.max(1.0), bounds.height.max(1.0));
        let r = self.rect;
        let mut flags = 0;
        if self.tile[0] {
            flags |= 1;
        }
        if self.tile[1] {
            flags |= 2;
        }
        if pipeline.srgb {
            flags |= 4;
        }
        if self.nearest {
            flags |= 8;
        }
        let u = Uniforms {
            rect: [r.x / w, r.y / h, (r.x + r.width) / w, (r.y + r.height) / h],
            size: [w * scale, h * scale],
            checker: 8.0 * scale,
            channel: self.channel,
            flags,
            pad: [0; 3],
        };
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&u));
        pipeline.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tessera 2d view"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: pipeline.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&pipeline.smooth) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pipeline.nearest) },
            ],
        }));
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        if let Some(bind_group) = &pipeline.bind_group
            && self.image.is_some()
        {
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..6, 0..9);
        }
        true
    }
}
