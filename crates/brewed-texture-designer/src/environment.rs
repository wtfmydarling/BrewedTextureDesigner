//! What lights the model of the 3D view when the studio does not: a picture of all that is
//! around it, an HDR image of the whole sphere (equirectangular: across, all the way round;
//! down, from straight up to straight down), from a .hdr or .exr file.
//!
//! The file is read on a thread of its own, and made ready once. On the CPU, the light a
//! matte surface gets from each direction: the irradiance, as nine spherical harmonics per
//! channel. On the GPU, the image with its mips (what shows behind the model), and the light a
//! glossy surface reflects: the image blurred by the GGX lobe of six roughnesses, a mip level
//! each, importance sampled, each sample read from the mip whose pixels are as large as the
//! part of the sphere it stands for, so that a hundred samples leave no noise.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use brewed_texture_engine::wgpu;
use half::f16;
use wgpu::util::DeviceExt;

/// The widest an environment is kept: what shows behind the model, and what a mirror
/// reflects.
const MAX_WIDTH: usize = 2048;
/// The widest image the irradiance is measured on: it changes slowly with direction.
const SH_WIDTH: usize = 256;
/// The roughnesses glossy light is kept for, a mip level each: 0, 0.2, 0.4 … 1.
pub const LEVELS: u32 = 6;
/// Width of the glossy light at roughness 0 (a mirror), at most; each rougher level is half as
/// wide.
const SPECULAR_WIDTH: u32 = 1024;
/// Directions of the GGX lobe a pixel of the glossy light averages.
const SAMPLES: u32 = 128;
/// What both are kept in: filtered when read, and far past 1.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// An environment read from a file.
pub struct Hdri {
    /// Tells environments apart, even two read from one file.
    pub id: u64,
    /// The file, as it was chosen.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// Linear RGBA (alpha 1) in half floats, row by row from the top (straight up).
    pub pixels: Vec<f16>,
    /// The irradiance: nine terms of red, green and blue (see [`irradiance`]).
    pub sh: [[f32; 3]; 9],
}

impl std::fmt::Debug for Hdri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hdri({} {}×{})", self.path, self.width, self.height)
    }
}

/// Read an environment from a file: a .hdr or an .exr, or any picture the program reads (an
/// 8-bit one is taken as linear light up to 1).
pub fn load(path: &Path) -> Result<Hdri, String> {
    let named = |e: String| format!("{}: {e}", path.display());
    let mut reader = image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format()).map_err(|e| named(e.to_string()))?;
    // The largest usual environments, 16384 × 8192, take 1.5 GB decoded (more than the
    // decoder's own limit lets it take); they are made smaller then.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(2 << 30);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| named(e.to_string()))?;
    let (width, height) = (image.width() as usize, image.height() as usize);
    Ok(Hdri::new(path.display().to_string(), width, height, image.into_rgb32f().into_raw()))
}

impl Hdri {
    /// An environment from linear RGB, `width` × `height`, rows from the top.
    pub fn new(path: String, width: usize, height: usize, mut rgb: Vec<f32>) -> Hdri {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let (mut w, mut h) = (width.max(1), height.max(1));
        rgb.resize(w * h * 3, 0.0);
        // What light cannot be: not a number, below none, more than a half float holds.
        for v in &mut rgb {
            *v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 65000.0) };
        }
        while w > MAX_WIDTH {
            (rgb, w, h) = halve(&rgb, w, h);
        }
        let pixels = rgb.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).map(f16::from_f32).collect();
        let (mut small, mut sw, mut sh) = (rgb, w, h);
        while sw > SH_WIDTH {
            (small, sw, sh) = halve(&small, sw, sh);
        }
        Hdri { id: NEXT.fetch_add(1, Ordering::Relaxed), path, width: w as u32, height: h as u32, pixels, sh: project(&small, sw, sh) }
    }
}

/// Half as wide and as high (rounded up): the mean of each 2×2 pixels.
fn halve(rgb: &[f32], w: usize, h: usize) -> (Vec<f32>, usize, usize) {
    let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
    let mut out = vec![0.0; hw * hh * 3];
    for y in 0..hh {
        let (y0, y1) = (2 * y, (2 * y + 1).min(h - 1));
        for x in 0..hw {
            let (x0, x1) = (2 * x, (2 * x + 1).min(w - 1));
            for c in 0..3 {
                let at = |x: usize, y: usize| rgb[(y * w + x) * 3 + c];
                out[(y * hw + x) * 3 + c] = 0.25 * (at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1));
            }
        }
    }
    (out, hw, hh)
}

/// The direction a point of an equirectangular image looks in: across (`u`), all the way
/// round, its middle straight ahead (-z) and a quarter further to the right (+x); down (`v`),
/// from straight up (+y) to straight down. The shaders have the same.
pub fn direction(u: f32, v: f32) -> [f32; 3] {
    use std::f32::consts::{PI, TAU};
    let (phi, theta) = ((u - 0.5) * TAU, v * PI);
    [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()]
}

/// The constants of the nine spherical harmonics of the first three bands.
const CONSTANTS: [f32; 9] = [0.282_095, 0.488_603, 0.488_603, 0.488_603, 1.092_548, 1.092_548, 0.315_392, 1.092_548, 0.546_274];

/// The nine harmonics of a direction, without their constants.
fn terms([x, y, z]: [f32; 3]) -> [f32; 9] {
    [1.0, y, z, x, x * y, y * z, 3.0 * z * z - 1.0, x * z, x * x - y * y]
}

/// The irradiance of an environment as spherical harmonics (Ramamoorthi and Hanrahan, 2001):
/// the image projected on the nine harmonics, each pixel weighted by the part of the sphere it
/// covers, then seen through a cosine (each band scaled by its lobe), and the constants of the
/// harmonics multiplied in, so that [`irradiance`] only has to add.
fn project(rgb: &[f32], w: usize, h: usize) -> [[f32; 3]; 9] {
    use std::f32::consts::PI;
    let mut sums = [[0.0f64; 3]; 9];
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        // The solid angle of a pixel of this row.
        let area = (2.0 * PI / w as f32) * (PI / h as f32) * (v * PI).sin();
        for x in 0..w {
            let t = terms(direction((x as f32 + 0.5) / w as f32, v));
            let p = &rgb[(y * w + x) * 3..][..3];
            for (k, sum) in sums.iter_mut().enumerate() {
                let weight = t[k] * CONSTANTS[k] * area;
                for c in 0..3 {
                    sum[c] += f64::from(p[c] * weight);
                }
            }
        }
    }
    let lobe = |k: usize| match k {
        0 => PI,
        1..=3 => 2.0 * PI / 3.0,
        _ => PI / 4.0,
    };
    let mut out = [[0.0; 3]; 9];
    for k in 0..9 {
        for c in 0..3 {
            out[k][c] = sums[k][c] as f32 * lobe(k) * CONSTANTS[k];
        }
    }
    out
}

/// The light a surface facing `n` gets from the whole environment (its irradiance): what the
/// shader of the 3D view adds up the same way.
#[cfg(test)]
fn irradiance(sh: &[[f32; 3]; 9], n: [f32; 3]) -> [f32; 3] {
    let t = terms(n);
    let mut e = [0.0f32; 3];
    for (k, term) in sh.iter().enumerate() {
        for c in 0..3 {
            e[c] += term[c] * t[k];
        }
    }
    e.map(|v| v.max(0.0))
}

// ---------------------------------------------------------------------------- the GPU

/// An environment on the GPU, ready to light a model and to show behind it.
pub struct Maps {
    /// The [`Hdri`] it was made from.
    pub id: u64,
    /// The image, with its mips.
    pub source: wgpu::Texture,
    pub source_view: wgpu::TextureView,
    /// The glossy light: a mip level for each of the [`LEVELS`] roughnesses.
    pub specular: wgpu::Texture,
    pub specular_view: wgpu::TextureView,
    pub sh: [[f32; 3]; 9],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Filter {
    /// Of the level written.
    size: [f32; 2],
    roughness: f32,
    /// Of the image's largest level.
    source_width: f32,
    source_levels: f32,
    samples: u32,
    pad: [u32; 2],
}

/// How environments are read: round and round across, not past the poles, smoothly between
/// pixels and between mip levels.
pub fn sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("brewed environment"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// What makes [`Maps`]: the passes that halve the image a mip level at a time, and that blur it
/// by the GGX lobe of a roughness (see environment.wgsl).
pub struct Filters {
    layout: wgpu::BindGroupLayout,
    halve: wgpu::RenderPipeline,
    prefilter: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl Filters {
    pub fn new(device: &wgpu::Device) -> Filters {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brewed environment"),
            source: wgpu::ShaderSource::Wgsl(include_str!("environment.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brewed environment"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brewed environment"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("brewed environment"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("full"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(FORMAT.into())],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        Filters { halve: pipeline("halve"), prefilter: pipeline("prefilter"), layout, sampler: sampler(device) }
    }

    /// The environment on the GPU: the image uploaded and halved down to a pixel, then blurred
    /// for each roughness. The work is submitted at once.
    pub fn make(&self, device: &wgpu::Device, queue: &wgpu::Queue, hdri: &Hdri) -> Maps {
        self.make_with(device, queue, hdri, SPECULAR_WIDTH, SAMPLES)
    }

    /// [`Filters::make`] with the glossy light at most `width` wide, from `samples` samples.
    fn make_with(&self, device: &wgpu::Device, queue: &wgpu::Queue, hdri: &Hdri, width: u32, samples: u32) -> Maps {
        let (w, h) = (hdri.width.max(1), hdri.height.max(1));
        let levels = 32 - w.max(h).leading_zeros();
        let texture = |label: &str, size: [u32; 2], mips: u32, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
                view_formats: &[],
            })
        };
        let source = texture("brewed environment", [w, h], levels, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &source, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(&hdri.pixels),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let level = |texture: &wgpu::Texture, level: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() })
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("brewed environment") });
        for l in 1..levels {
            let size = [(w >> l).max(1) as f32, (h >> l).max(1) as f32];
            let filter = Filter { size, roughness: 0.0, source_width: 0.0, source_levels: 0.0, samples: 0, pad: [0; 2] };
            self.pass(device, &mut encoder, &self.halve, &level(&source, l - 1), &level(&source, l), filter);
        }
        // As wide as the image, in powers of two, and wide enough for the roughest level.
        let sw = width.min(w.next_power_of_two()).max(1 << (LEVELS - 1));
        let specular = texture("brewed environment glossy", [sw, sw / 2], LEVELS, wgpu::TextureUsages::COPY_SRC);
        let whole = source.create_view(&wgpu::TextureViewDescriptor::default());
        for l in 0..LEVELS {
            let filter = Filter {
                size: [(sw >> l).max(1) as f32, ((sw / 2) >> l).max(1) as f32],
                roughness: l as f32 / (LEVELS - 1) as f32,
                source_width: w as f32,
                source_levels: levels as f32,
                samples,
                pad: [0; 2],
            };
            self.pass(device, &mut encoder, &self.prefilter, &whole, &level(&specular, l), filter);
        }
        queue.submit([encoder.finish()]);
        let specular_view = specular.create_view(&wgpu::TextureViewDescriptor::default());
        Maps { id: hdri.id, source, source_view: whole, specular, specular_view, sh: hdri.sh }
    }

    fn pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
        filter: Filter,
    ) {
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("brewed environment"),
            contents: bytemuck::bytes_of(&filter),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brewed environment"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(input) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brewed environment"),
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
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// An environment `w` × `w/2` of what `light` says each direction gives.
    fn made_of(w: usize, light: impl Fn([f32; 3]) -> [f32; 3]) -> Hdri {
        let h = w / 2;
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                rgb.extend(light(direction((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32)));
            }
        }
        Hdri::new("made.hdr".into(), w, h, rgb)
    }

    fn close(a: [f32; 3], b: [f32; 3], within: f32) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() <= within)
    }

    #[test]
    fn directions_go_round_from_ahead_and_down_from_above() {
        let near = |a: [f32; 3], b: [f32; 3]| close(a, b, 1e-5);
        assert!(near(direction(0.5, 0.5), [0.0, 0.0, -1.0]));
        assert!(near(direction(0.75, 0.5), [1.0, 0.0, 0.0]));
        assert!(near(direction(0.0, 0.5), [0.0, 0.0, 1.0]));
        assert!(near(direction(0.3, 0.0), [0.0, 1.0, 0.0]));
        assert!(near(direction(0.3, 1.0), [0.0, -1.0, 0.0]));
    }

    #[test]
    fn irradiance_of_the_sky_and_of_even_light() {
        // Light of 1 from everywhere: π on every surface.
        let even = made_of(128, |_| [1.0, 0.5, 0.25]);
        for n in [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.6, 0.0, 0.8]] {
            assert!(close(irradiance(&even.sh, n), [PI, PI / 2.0, PI / 4.0], 0.01), "{n:?}: {:?}", irradiance(&even.sh, n));
        }
        // Light from the upper half only: π up, nothing down, half across (the harmonics are
        // exact for it).
        let sky = made_of(256, |d| if d[1] > 0.0 { [1.0; 3] } else { [0.0; 3] });
        let e = |n: [f32; 3]| irradiance(&sky.sh, n)[0];
        assert!((e([0.0, 1.0, 0.0]) - PI).abs() < 0.02, "{}", e([0.0, 1.0, 0.0]));
        assert!(e([0.0, -1.0, 0.0]).abs() < 0.02, "{}", e([0.0, -1.0, 0.0]));
        assert!((e([0.0, 0.0, 1.0]) - PI / 2.0).abs() < 0.02, "{}", e([0.0, 0.0, 1.0]));
        // A sun ahead: a surface facing it gets more than one facing away.
        let sun = made_of(256, |d| if d[2] < -0.95 { [50.0; 3] } else { [0.1; 3] });
        assert!(irradiance(&sun.sh, [0.0, 0.0, -1.0])[0] > 5.0 * irradiance(&sun.sh, [0.0, 0.0, 1.0])[0]);
    }

    #[test]
    fn a_large_environment_is_halved_and_keeps_its_light() {
        let big = made_of(MAX_WIDTH + 2, |d| [d[1].max(0.0) * 2.0, 1.0, 0.0]);
        assert_eq!((big.width, big.height), (1025, 513));
        assert_eq!(big.pixels.len(), 1025 * 513 * 4);
        let mean = big.pixels.as_chunks::<4>().0.iter().map(|p| p[1].to_f32()).sum::<f32>() / (1025.0 * 513.0);
        assert!((mean - 1.0).abs() < 1e-3, "{mean}");
        // What is no light, none; more than a half float holds, as much as it does.
        let odd = Hdri::new("odd.exr".into(), 2, 1, vec![f32::NAN, -3.0, f32::INFINITY, 1.0, 2.0, 1e9]);
        let v: Vec<f32> = odd.pixels.iter().map(|p| p.to_f32()).collect();
        assert_eq!(&v[..2], &[0.0, 0.0][..]);
        assert!(v[2] > 60000.0 && v[6] > 60000.0, "{v:?}");
        assert!(v.iter().all(|x| x.is_finite()));
        // A file with fewer pixels than it says: the rest is dark, never a crash.
        let short = Hdri::new("short.hdr".into(), 4, 2, vec![1.0; 5]);
        assert_eq!(short.pixels.len(), 4 * 2 * 4);
    }

    #[test]
    fn hdr_and_exr_files_are_read() {
        let folder = std::env::temp_dir().join(format!("brewed-environment-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (w, h) = (64u32, 32u32);
        let light = |x: u32, y: u32| [x as f32 / 8.0, y as f32 / 4.0, 3.5];
        let hdr = folder.join("sky.hdr");
        let pixels: Vec<image::Rgb<f32>> = (0..h).flat_map(|y| (0..w).map(move |x| image::Rgb(light(x, y)))).collect();
        image::codecs::hdr::HdrEncoder::new(std::fs::File::create(&hdr).unwrap()).encode(&pixels, w as usize, h as usize).unwrap();
        let exr = folder.join("sky.exr");
        image::Rgb32FImage::from_fn(w, h, |x, y| image::Rgb(light(x, y))).save(&exr).unwrap();
        for file in [&hdr, &exr] {
            let env = load(file).unwrap();
            assert_eq!((env.width, env.height), (w, h), "{}", file.display());
            let at = |x: u32, y: u32| {
                let p = &env.pixels[((y * w + x) * 4) as usize..][..3];
                [p[0].to_f32(), p[1].to_f32(), p[2].to_f32()]
            };
            // Radiance files keep a mantissa of 8 bits.
            assert!(close(at(40, 20), light(40, 20), 0.03), "{}: {:?}", file.display(), at(40, 20));
            assert_eq!(env.path, file.display().to_string());
        }
        let missing = load(&folder.join("missing.hdr")).unwrap_err();
        assert!(missing.contains("missing.hdr"), "{missing}");
        std::fs::write(folder.join("broken.hdr"), b"#?RADIANCE\nnot really").unwrap();
        assert!(load(&folder.join("broken.hdr")).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Each level of the glossy light, read back: rows of RGB.
    fn glossy(gpu: &brewed_texture_engine::gpu::Gpu, maps: &Maps) -> Vec<(u32, u32, Vec<[f32; 3]>)> {
        levels(gpu, &maps.specular)
    }

    /// Each mip level of a texture of half floats, read back: rows of RGB.
    fn levels(gpu: &brewed_texture_engine::gpu::Gpu, texture: &wgpu::Texture) -> Vec<(u32, u32, Vec<[f32; 3]>)> {
        let (device, queue) = (&gpu.device, &gpu.queue);
        let mut levels = Vec::new();
        for l in 0..texture.mip_level_count() {
            let (w, h) = ((texture.width() >> l).max(1), (texture.height() >> l).max(1));
            let row = (w * 8).div_ceil(256) * 256;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * h),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            queue.submit([encoder.finish()]);
            buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let bytes = buffer.slice(..).get_mapped_range();
            let halves: &[f16] = bytemuck::cast_slice(&bytes);
            let mut pixels = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let p = &halves[(y * row / 2 + x * 4) as usize..][..3];
                    pixels.push([p[0].to_f32(), p[1].to_f32(), p[2].to_f32()]);
                }
            }
            levels.push((w, h, pixels));
        }
        levels
    }

    #[test]
    fn glossy_light_blurs_the_environment_by_roughness() {
        let gpu = brewed_texture_engine::gpu::Gpu::headless().expect("a GPU");
        let filters = Filters::new(&gpu.device);
        // Even light stays as even at every roughness: the samples' weights add up to one.
        let even = made_of(128, |_| [0.5, 1.0, 2.0]);
        for (w, h, pixels) in glossy(&gpu, &filters.make_with(&gpu.device, &gpu.queue, &even, 64, 32)) {
            assert!(pixels.iter().all(|p| close(*p, [0.5, 1.0, 2.0], 0.02)), "{w}×{h}: {:?}", pixels.iter().find(|p| !close(**p, [0.5, 1.0, 2.0], 0.02)));
        }
        // A sun ahead: a mirror shows it as it is; rougher, it spreads, dimmer and wider, and
        // what faces away from it stays dark.
        let sun = made_of(256, |d| if d[2] < -0.99 { [20.0; 3] } else { [0.0; 3] });
        let levels = glossy(&gpu, &filters.make_with(&gpu.device, &gpu.queue, &sun, 64, 64));
        let (mut last_peak, mut last_lit) = (f32::INFINITY, 0);
        for (l, (w, h, pixels)) in levels.iter().enumerate() {
            assert!(pixels.iter().all(|p| p.iter().all(|c| c.is_finite() && *c >= 0.0)), "level {l}");
            // The smallest levels have no pixel ahead or behind to look at.
            if *w < 8 {
                continue;
            }
            let at = |x: u32, y: u32| pixels[(y * w + x) as usize][0];
            // Ahead is the middle of the image; behind, its left edge.
            let (cx, cy) = (w / 2, h / 2);
            let ahead = at(cx, cy).max(at(cx - 1, cy)).max(at(cx, cy - 1)).max(at(cx - 1, cy - 1));
            let behind = at(0, cy);
            let lit = pixels.iter().filter(|p| p[0] > 0.01).count() * 4096 / pixels.len();
            assert!(ahead < last_peak, "level {l}: {ahead} after {last_peak}");
            assert!(lit >= last_lit, "level {l}: {lit} after {last_lit}");
            assert!(behind < 0.05, "level {l}: {behind} behind the sun");
            (last_peak, last_lit) = (ahead, lit);
        }
        assert!(levels[0].2.iter().any(|p| p[0] > 10.0), "a mirror shows the sun");
    }

    /// The lowest and highest red along the middle row of a level.
    fn range_of_middle_row((w, h, pixels): &(u32, u32, Vec<[f32; 3]>)) -> (f32, f32) {
        let row = &pixels[(h / 2 * w) as usize..][..*w as usize];
        row.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])))
    }

    #[test]
    fn a_little_roughness_blurs_a_little_and_the_mips_keep_the_light() {
        let gpu = brewed_texture_engine::gpu::Gpu::headless().expect("a GPU");
        let filters = Filters::new(&gpu.device);
        // Stripes of 10° round the horizon, white and black; gray above and below.
        let stripes = made_of(512, |d| {
            let across = (d[0].atan2(-d[2]) / std::f32::consts::TAU + 0.5) * 36.0;
            match d[1].abs() < 0.5 {
                true if (across as i32) % 2 == 0 => [1.0; 3],
                true => [0.0; 3],
                false => [0.5; 3],
            }
        });
        let maps = filters.make_with(&gpu.device, &gpu.queue, &stripes, 256, 128);
        // Each mip of the image is the mean of the one above: the stripes, then their gray.
        let source = levels(&gpu, &maps.source);
        for (l, level) in source.iter().enumerate() {
            let row = &level.2[(level.1 / 2 * level.0) as usize..][..level.0 as usize];
            let mean = row.iter().map(|p| p[0]).sum::<f32>() / row.len() as f32;
            assert!((mean - 0.5).abs() < 0.03, "level {l}: {mean}");
        }
        assert_eq!(range_of_middle_row(&source[1]), (0.0, 1.0));
        // A mirror shows the stripes as they are, roughness 0.2 still shows them, and the
        // rougher levels less and less, down to their gray.
        let glossy = glossy(&gpu, &maps);
        let contrast: Vec<f32> = glossy
            .iter()
            .map(|level| {
                let (lo, hi) = range_of_middle_row(level);
                hi - lo
            })
            .collect();
        assert!(contrast[0] > 0.99, "{contrast:?}");
        assert!(contrast[1] > 0.4, "{contrast:?}");
        assert!(contrast.windows(2).all(|c| c[1] <= c[0] + 0.01), "{contrast:?}");
        assert!(contrast[5] < 0.05, "{contrast:?}");
    }
}
