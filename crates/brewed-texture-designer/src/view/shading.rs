//! What the 3D view reads of a material: each parameter of OpenPBR, and the maps of the
//! workflows before it (specular and glossiness, a bump, a detail normal …), from the Output
//! of its usage, or where the graph has none, the value the standard gives it.
//!
//! A material can have fifty maps, and the GPU iced draws with lets a shader read sixteen
//! textures. Most maps are uniform values (an image of one pixel: Uniform Color), and those are
//! read into one row of a texture, a pixel each, by the GPU itself (see [`Material::prepare`]),
//! with the standard's values where no map is given; the maps that are images each take one
//! of [`SLOTS`] textures, and the rare material that has more of them than that has the rest
//! copied into the layers of one more texture, with their mips. What the 3D view shows is the
//! same either way.
//!
//! Also here: the table of how much light each lobe reflects (see [`lut`]), which the shader
//! weighs the light around the model by.

use brewed_texture_core::nodes::Usage;
use brewed_texture_engine::wgpu;
use half::f16;

/// A parameter the shader reads: its name there, the usages whose maps give it (the first of
/// them the graph has), and its value where none does, as a map holds it (a color in sRGB).
pub struct Input {
    pub name: &'static str,
    pub usages: &'static [Usage],
    pub default: [f32; 4],
}

const fn gray(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// Every parameter the shader reads, in the order it knows them by (`IN_…` there).
pub const INPUTS: &[Input] = &[
    Input { name: "BASE_WEIGHT", usages: &[Usage::BaseWeight], default: gray(1.0) },
    Input { name: "BASE_COLOR", usages: &[Usage::BaseColor], default: gray(0.8) },
    Input { name: "METALNESS", usages: &[Usage::Metallic], default: gray(0.0) },
    Input { name: "DIFFUSE_ROUGHNESS", usages: &[Usage::BaseDiffuseRoughness], default: gray(0.0) },
    Input { name: "SPECULAR_WEIGHT", usages: &[Usage::SpecularWeight], default: gray(1.0) },
    Input { name: "SPECULAR_COLOR", usages: &[Usage::SpecularColor], default: gray(1.0) },
    Input { name: "ROUGHNESS", usages: &[Usage::Roughness], default: gray(0.3) },
    Input { name: "ANISOTROPY", usages: &[Usage::SpecularRoughnessAnisotropy], default: gray(0.0) },
    Input { name: "ANISOTROPY_ANGLE", usages: &[Usage::AnisotropyAngle], default: gray(0.0) },
    Input { name: "SPECULAR_IOR", usages: &[Usage::SpecularIor], default: gray(1.5) },
    Input { name: "TRANSMISSION_WEIGHT", usages: &[Usage::TransmissionWeight], default: gray(0.0) },
    Input { name: "TRANSMISSION_COLOR", usages: &[Usage::TransmissionColor], default: gray(1.0) },
    Input { name: "TRANSMISSION_DEPTH", usages: &[Usage::TransmissionDepth], default: gray(0.0) },
    Input { name: "TRANSMISSION_SCATTER", usages: &[Usage::TransmissionScatter], default: gray(0.0) },
    Input { name: "TRANSMISSION_SCATTER_ANISOTROPY", usages: &[Usage::TransmissionScatterAnisotropy], default: gray(0.0) },
    Input { name: "DISPERSION_SCALE", usages: &[Usage::TransmissionDispersionScale], default: gray(0.0) },
    Input { name: "ABBE_NUMBER", usages: &[Usage::TransmissionDispersionAbbeNumber], default: gray(20.0) },
    Input { name: "SUBSURFACE_WEIGHT", usages: &[Usage::SubsurfaceWeight], default: gray(0.0) },
    Input { name: "SUBSURFACE_COLOR", usages: &[Usage::SubsurfaceColor], default: gray(0.8) },
    Input { name: "SUBSURFACE_RADIUS", usages: &[Usage::SubsurfaceRadius], default: gray(1.0) },
    Input { name: "SUBSURFACE_RADIUS_SCALE", usages: &[Usage::SubsurfaceRadiusScale], default: [1.0, 0.5, 0.25, 1.0] },
    Input { name: "SUBSURFACE_ANISOTROPY", usages: &[Usage::SubsurfaceScatterAnisotropy], default: gray(0.0) },
    Input { name: "COAT_WEIGHT", usages: &[Usage::CoatWeight], default: gray(0.0) },
    Input { name: "COAT_COLOR", usages: &[Usage::CoatColor], default: gray(1.0) },
    Input { name: "COAT_ROUGHNESS", usages: &[Usage::CoatRoughness], default: gray(0.0) },
    Input { name: "COAT_ANISOTROPY", usages: &[Usage::CoatRoughnessAnisotropy], default: gray(0.0) },
    Input { name: "COAT_IOR", usages: &[Usage::CoatIor], default: gray(1.6) },
    Input { name: "COAT_DARKENING", usages: &[Usage::CoatDarkening], default: gray(1.0) },
    Input { name: "COAT_NORMAL", usages: &[Usage::CoatNormal], default: [0.5, 0.5, 1.0, 1.0] },
    Input { name: "COAT_TANGENT", usages: &[Usage::CoatTangent], default: [1.0, 0.5, 0.0, 1.0] },
    Input { name: "FUZZ_WEIGHT", usages: &[Usage::FuzzWeight], default: gray(0.0) },
    Input { name: "FUZZ_COLOR", usages: &[Usage::FuzzColor], default: gray(1.0) },
    Input { name: "FUZZ_ROUGHNESS", usages: &[Usage::FuzzRoughness], default: gray(0.5) },
    Input { name: "EMISSION_LUMINANCE", usages: &[Usage::EmissionLuminance], default: gray(0.0) },
    Input { name: "EMISSION_COLOR", usages: &[Usage::Emissive], default: gray(1.0) },
    Input { name: "THIN_FILM_WEIGHT", usages: &[Usage::ThinFilmWeight], default: gray(0.0) },
    Input { name: "THIN_FILM_THICKNESS", usages: &[Usage::ThinFilmThickness], default: gray(0.5) },
    Input { name: "THIN_FILM_IOR", usages: &[Usage::ThinFilmIor], default: gray(1.4) },
    Input { name: "OPACITY", usages: &[Usage::Opacity], default: gray(1.0) },
    Input { name: "THIN_WALLED", usages: &[Usage::ThinWalled], default: gray(0.0) },
    Input { name: "NORMAL", usages: &[Usage::Normal], default: [0.5, 0.5, 1.0, 1.0] },
    Input { name: "TANGENT", usages: &[Usage::Tangent], default: [1.0, 0.5, 0.0, 1.0] },
    Input { name: "AO", usages: &[Usage::AmbientOcclusion], default: gray(1.0) },
    Input { name: "HEIGHT", usages: &[Usage::Height, Usage::Displacement], default: gray(0.0) },
    Input { name: "BUMP", usages: &[Usage::Bump], default: gray(0.0) },
    Input { name: "DETAIL_NORMAL", usages: &[Usage::DetailNormal], default: [0.5, 0.5, 1.0, 1.0] },
    Input { name: "DIFFUSE", usages: &[Usage::Diffuse], default: gray(0.8) },
    Input { name: "SPECULAR_F0", usages: &[Usage::Specular], default: gray(0.22) },
    Input { name: "GLOSSINESS", usages: &[Usage::Glossiness], default: gray(0.7) },
    Input { name: "SPECULAR_LEVEL", usages: &[Usage::SpecularLevel], default: gray(0.5) },
];

/// How many inputs the shader's table has room for.
pub const MAX_INPUTS: usize = 64;
/// How many maps that are images the shader reads each from a texture of its own; more are
/// read from the layers of one texture.
pub const SLOTS: u32 = 11;
/// Where an input's value is read from the row of uniform values (not a texture).
pub const CONSTANT: u32 = 31;

/// The input of the shader whose `name` it is.
pub fn input(name: &str) -> u32 {
    INPUTS.iter().position(|i| i.name == name).expect("an input of the shader") as u32
}

/// What the shader knows of its inputs: the index of each.
pub fn wgsl_inputs() -> String {
    let mut s = String::new();
    for (i, input) in INPUTS.iter().enumerate() {
        s.push_str(&format!("const IN_{}: u32 = {i}u;\n", input.name));
    }
    s
}

/// A map of the material, as the engine has it on the GPU: which input it gives, and its image.
#[derive(Clone, Debug)]
pub struct Map {
    pub input: u32,
    pub view: wgpu::TextureView,
    pub texture: wgpu::Texture,
    /// Which of the images the engine has written in the texture it is (it writes a node's
    /// next image in the same texture).
    pub version: u64,
    /// One channel, read as gray.
    pub gray: bool,
}

impl Map {
    /// Whether it is one value all over (an image of a pixel).
    fn uniform(&self) -> bool {
        self.texture.width() == 1 && self.texture.height() == 1
    }
}

/// The maps of a material, each input given by the map of the first of its usages that one of
/// `maps` (each of a usage) has.
pub fn of<'a>(maps: impl Iterator<Item = (Usage, &'a brewed_texture_engine::DisplayImage)> + Clone) -> Vec<Map> {
    INPUTS
        .iter()
        .enumerate()
        .filter_map(|(i, input)| {
            input.usages.iter().find_map(|u| {
                maps.clone().find(|(usage, _)| usage == u).map(|(_, img)| Map {
                    input: i as u32,
                    view: img.view.clone(),
                    texture: img.texture.clone(),
                    version: img.version,
                    gray: img.gray,
                })
            })
        })
        .collect()
}

/// Where the shader reads each input (a word each: the slot, or a layer of the extra texture
/// past them, or [`CONSTANT`]; whether it is gray; whether a map gives it), and which maps
/// take the slots and which the extra layers, in order.
pub struct Placed {
    pub table: [u32; MAX_INPUTS],
    pub slots: Vec<Map>,
    pub extra: Vec<Map>,
}

/// Where the shader is to read `maps`: those that are images in the slots, in the order of the
/// inputs, then in the layers of the extra texture; the uniform ones and the inputs no map
/// gives from the row of uniform values.
pub fn place(maps: &[Map]) -> Placed {
    let mut table = [CONSTANT; MAX_INPUTS];
    let (mut slots, mut extra) = (Vec::new(), Vec::new());
    for map in maps {
        let slot = if map.uniform() {
            CONSTANT
        } else if (slots.len() as u32) < SLOTS {
            slots.push(map.clone());
            slots.len() as u32 - 1
        } else {
            extra.push(map.clone());
            SLOTS + extra.len() as u32 - 1
        };
        table[map.input as usize] = slot | u32::from(map.gray) << 5 | 1 << 6;
    }
    Placed { table, slots, extra }
}

// ---------------------------------------------------------------------------- the light the lobes reflect

/// Side of [`lut`].
pub const LUT_SIDE: usize = 32;

/// How much of the light around it each lobe reflects toward the eye, for each angle the
/// surface is seen at (across: the cosine, from 0 to 1) and each roughness (down, 0 to 1):
/// red and green, the scale and the bias of F0 of GGX's lobe (with Schlick's Fresnel and
/// Smith's height-correlated shadowing: the second part of Karis's split sum); blue, all the
/// light the fuzz reflects (Charlie's sheen, Neubelt's visibility), which the layers under it
/// do not get; alpha, 1.
pub fn lut() -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(LUT_SIDE * LUT_SIDE);
    for j in 0..LUT_SIDE {
        let rough = (j as f32 + 0.5) / LUT_SIDE as f32;
        for i in 0..LUT_SIDE {
            let nv = (i as f32 + 0.5) / LUT_SIDE as f32;
            let (a, b) = ggx_dfg(nv, rough);
            out.push([a, b, sheen_albedo(nv, rough), 1.0]);
        }
    }
    out
}

/// The `i`th of `n` points of the Hammersley set: evenly spread over the unit square.
fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n as f32, (i.reverse_bits() as f64 / 4_294_967_296.0) as f32)
}

/// Smith's Λ of GGX for a direction at `cos` to the normal.
fn smith_lambda(cos: f32, alpha: f32) -> f32 {
    let cos2 = (cos * cos).max(1e-8);
    let tan2 = (1.0 - cos2) / cos2;
    (-1.0 + (1.0 + alpha * alpha * tan2).sqrt()) / 2.0
}

/// The scale and bias of F0 that GGX's lobe reflects of light all around (see [`lut`]).
fn ggx_dfg(nv: f32, rough: f32) -> (f32, f32) {
    use std::f32::consts::TAU;
    let alpha = (rough * rough).max(1e-4);
    let v = [(1.0 - nv * nv).max(0.0).sqrt(), 0.0, nv];
    const N: u32 = 512;
    let (mut a, mut b) = (0.0, 0.0);
    for k in 0..N {
        let (u1, u2) = hammersley(k, N);
        // A half vector of GGX's distribution.
        let phi = TAU * u1;
        let cos_t = ((1.0 - u2) / (1.0 + (alpha * alpha - 1.0) * u2)).sqrt();
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let h = [sin_t * phi.cos(), sin_t * phi.sin(), cos_t];
        let vh = v[0] * h[0] + v[2] * h[2];
        let nl = 2.0 * vh * h[2] - v[2];
        if nl > 0.0 && vh > 0.0 {
            let g = 1.0 / (1.0 + smith_lambda(nv, alpha) + smith_lambda(nl, alpha));
            let visible = g * vh / (h[2] * nv);
            let fc = (1.0 - vh).powi(5);
            a += (1.0 - fc) * visible;
            b += fc * visible;
        }
    }
    ((a / N as f32).min(1.0), (b / N as f32).min(1.0))
}

/// All the light the fuzz reflects of light all around (see [`lut`]): its lobe over every
/// direction, a 64 × 64 grid of them.
fn sheen_albedo(nv: f32, rough: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let alpha = rough.max(0.07);
    let v = [(1.0 - nv * nv).max(0.0).sqrt(), 0.0, nv];
    const N: usize = 64;
    let mut sum = 0.0;
    for j in 0..N {
        // Evenly over the cosine (the hemisphere's area), and round.
        let nl = (j as f32 + 0.5) / N as f32;
        let sin_l = (1.0 - nl * nl).sqrt();
        for k in 0..N {
            let phi = TAU * (k as f32 + 0.5) / N as f32;
            let l = [sin_l * phi.cos(), sin_l * phi.sin(), nl];
            let h = [v[0] + l[0], v[1] + l[1], v[2] + l[2]];
            let len = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt().max(1e-8);
            let nh = h[2] / len;
            let d = (2.0 + 1.0 / alpha) * (1.0 - nh * nh).max(0.0).powf(0.5 / alpha) / TAU;
            let vis = 1.0 / (4.0 * (nl + nv - nl * nv));
            sum += d * vis * nl;
        }
    }
    // Each point stands for 2π / N² of the hemisphere.
    (sum * 2.0 * PI / (N * N) as f32).min(1.0)
}

// ---------------------------------------------------------------------------- the GPU

const SHADER: &str = include_str!("shading.wgsl");
const VALUES_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const EXTRA_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// What makes a material ready for the shader, and what it reads: the row of uniform values,
/// the extra texture of the maps past the slots, the table of [`lut`], and a texture of
/// nothing for the slots no map takes.
pub struct Material {
    layout: wgpu::BindGroupLayout,
    gather_color: wgpu::RenderPipeline,
    gather_gray: wgpu::RenderPipeline,
    copy_color: wgpu::RenderPipeline,
    copy_gray: wgpu::RenderPipeline,
    halve: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    values: wgpu::Texture,
    pub values_view: wgpu::TextureView,
    extra: Option<wgpu::Texture>,
    pub extra_view: wgpu::TextureView,
    pub lut_view: wgpu::TextureView,
    pub nothing: wgpu::TextureView,
    /// The maps last made ready: which input, which image (its texture and version), gray.
    made: Option<Vec<(u32, wgpu::Texture, u64, bool)>>,
}

impl Material {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Material {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brewed 3d material"),
            source: wgpu::ShaderSource::Wgsl([brewed_texture_engine::gpu::SHOW_WGSL, "\n", SHADER].concat().into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brewed 3d material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brewed 3d material"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("brewed 3d material"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("full"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let texture = |label: &str, format: wgpu::TextureFormat, size: [u32; 2], usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let values = texture(
            "brewed 3d material values",
            VALUES_FORMAT,
            [MAX_INPUTS as u32, 1],
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        );
        let lut = texture("brewed 3d lut", wgpu::TextureFormat::Rgba16Float, [LUT_SIDE as u32; 2], wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let table: Vec<f16> = lut_table().iter().flatten().map(|v| f16::from_f32(*v)).collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &lut, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&table),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(LUT_SIDE as u32 * 8), rows_per_image: Some(LUT_SIDE as u32) },
            wgpu::Extent3d { width: LUT_SIDE as u32, height: LUT_SIDE as u32, depth_or_array_layers: 1 },
        );
        let nothing = texture("brewed 3d no map", wgpu::TextureFormat::Rgba8Unorm, [1, 1], wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &nothing, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[0, 0, 0, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let no_extra = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("brewed 3d no extra maps"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXTRA_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Material {
            gather_color: pipeline("gather_color", VALUES_FORMAT),
            gather_gray: pipeline("gather_gray", VALUES_FORMAT),
            copy_color: pipeline("copy_color", EXTRA_FORMAT),
            copy_gray: pipeline("copy_gray", EXTRA_FORMAT),
            halve: pipeline("halve", EXTRA_FORMAT),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("brewed 3d material"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            values_view: values.create_view(&wgpu::TextureViewDescriptor::default()),
            values,
            extra: None,
            extra_view: no_extra.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() }),
            lut_view: lut.create_view(&wgpu::TextureViewDescriptor::default()),
            nothing: nothing.create_view(&wgpu::TextureViewDescriptor::default()),
            made: None,
        }
    }

    /// Make `placed` ready, if it is not what was made last: the row of uniform values (the
    /// standard's values, and the uniform maps read into their pixels), and the extra maps
    /// copied into the layers of a texture of their own, with their mips. The work is
    /// submitted at once.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, maps: &[Map], placed: &Placed) {
        let key: Vec<(u32, wgpu::Texture, u64, bool)> = maps.iter().map(|m| (m.input, m.texture.clone(), m.version, m.gray)).collect();
        if self.made.as_ref() == Some(&key) {
            return;
        }
        // The standard's values, a pixel each; the uniform maps' over them, below.
        let mut row = vec![f16::ZERO; MAX_INPUTS * 4];
        for (i, input) in INPUTS.iter().enumerate() {
            for (c, v) in input.default.iter().enumerate() {
                row[i * 4 + c] = f16::from_f32(*v);
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.values, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&row),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(MAX_INPUTS as u32 * 8), rows_per_image: Some(1) },
            wgpu::Extent3d { width: MAX_INPUTS as u32, height: 1, depth_or_array_layers: 1 },
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("brewed 3d material") });
        let uniform: Vec<&Map> = maps.iter().filter(|m| m.uniform()).collect();
        if !uniform.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("brewed 3d material values"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.values_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            for map in uniform {
                let group = self.group(device, &map.view);
                pass.set_pipeline(if map.gray { &self.gather_gray } else { &self.gather_color });
                pass.set_bind_group(0, &group, &[]);
                pass.set_viewport(map.input as f32, 0.0, 1.0, 1.0, 0.0, 1.0);
                pass.draw(0..3, 0..1);
            }
        }
        self.extra = None;
        if !placed.extra.is_empty() {
            let side = placed.extra.iter().map(|m| m.texture.width().max(m.texture.height())).max().unwrap_or(1).clamp(1, 4096);
            let levels = 32 - side.leading_zeros();
            let layers = placed.extra.len() as u32;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("brewed 3d extra maps"),
                size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: layers },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: EXTRA_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let one = |layer: u32, level: u32| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            };
            for (layer, map) in placed.extra.iter().enumerate() {
                let layer = layer as u32;
                self.draw(device, &mut encoder, if map.gray { &self.copy_gray } else { &self.copy_color }, &map.view, &one(layer, 0));
                for level in 1..levels {
                    self.draw(device, &mut encoder, &self.halve, &one(layer, level - 1), &one(layer, level));
                }
            }
            self.extra_view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
            self.extra = Some(texture);
        }
        queue.submit([encoder.finish()]);
        self.made = Some(key);
    }

    fn group(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brewed 3d material"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// `input` drawn by `pipeline` over all of `output`.
    fn draw(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, input: &wgpu::TextureView, output: &wgpu::TextureView) {
        let group = self.group(device, input);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brewed 3d material extra"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// [`lut`], worked out once.
fn lut_table() -> &'static [[f32; 4]] {
    static TABLE: std::sync::OnceLock<Vec<[f32; 4]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(lut)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every usage the 3D view shows gives an input (or lights the model: a panorama), none
    /// two; the shader's table has room for them all.
    #[test]
    fn every_usage_shown_gives_an_input() {
        assert!(INPUTS.len() <= MAX_INPUTS);
        for usage in Usage::ALL.into_iter().filter(|u| u.shown_in_3d() && *u != Usage::Panorama) {
            let given: Vec<&str> = INPUTS.iter().filter(|i| i.usages.contains(&usage)).map(|i| i.name).collect();
            assert_eq!(given.len(), 1, "{usage:?} gives {given:?}");
        }
        for input in INPUTS {
            assert!(input.usages.iter().all(|u| u.shown_in_3d() && *u != Usage::Panorama), "{}", input.name);
        }
    }

    /// The light the lobes reflect is what it is known to be: a mirror seen head on reflects
    /// F0 (scale 1, bias 0); the roughest surface seen head on, 1 - ln 2 of the light (its
    /// lobe is then even every way: ∫ μ / (1 + μ) over the cosine); the bias grows toward
    /// grazing; the fuzz reflects most at grazing angles, never more than all.
    #[test]
    fn the_table_of_reflected_light_is_right() {
        let at = |nv: f32, rough: f32| {
            let t = lut();
            let i = ((nv * LUT_SIDE as f32) as usize).min(LUT_SIDE - 1);
            let j = ((rough * LUT_SIDE as f32) as usize).min(LUT_SIDE - 1);
            t[j * LUT_SIDE + i]
        };
        let mirror = at(0.99, 0.0);
        assert!((mirror[0] - 1.0).abs() < 0.05 && mirror[1] < 0.02, "{mirror:?}");
        let rough = at(0.99, 0.99);
        assert!((rough[0] + rough[1] - (1.0 - std::f32::consts::LN_2)).abs() < 0.03, "{rough:?}");
        assert!(at(0.05, 0.3)[1] > at(0.9, 0.3)[1], "more bias at grazing angles");
        for v in lut() {
            assert!(v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)), "{v:?}");
        }
        assert!(at(0.1, 0.4)[2] > at(0.95, 0.4)[2], "fuzz shines at grazing angles");
    }
}
