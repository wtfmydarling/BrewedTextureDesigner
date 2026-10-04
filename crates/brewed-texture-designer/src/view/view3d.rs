//! 3D material preview: the graph's outputs (by their Usage) on a model, lit with a
//! physically based model, by the studio (a key light, a fill light and a sky) or by an
//! environment image (see [`crate::environment`]), which can show behind the model too.
//! Height displaces the vertices; the maps are the images the outputs write, in their
//! channels and at their bit depths, with their mips, sampled directly on the GPU (one channel
//! read as gray, with the engine's `widen`).
//!
//! Left drag orbits, right drag turns the light (or the environment), the wheel zooms.

use std::sync::Arc;

use iced::widget::shader::{self, Viewport};
use iced::widget::tooltip::Position;
use iced::widget::{button, column, container, pick_list, row, rule, shader as shader_widget, text};
use iced::{Alignment, Element, Length, Rectangle, Vector, mouse, window};
use brewed_texture_core::NodeKind;
use brewed_texture_core::nodes::{USAGE_AO, USAGE_BASE_COLOR, USAGE_EMISSIVE, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL, USAGE_ROUGHNESS};
use brewed_texture_core::package::file_name;
use brewed_texture_engine::wgpu;
use wgpu::util::DeviceExt;

use crate::app::{App, Message, PreviewMessage};
use crate::environment::{self, Hdri};
use crate::icons::{Icon, icon};
use crate::style;
use crate::view::frames::FrameCounter;
use crate::widget::dropdown::dropdown;
use crate::widget::number::number;

pub use super::models::Shape;
use super::models::{Vertex, build_mesh, cross, dot, normalize, sub};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The point the camera looks at and turns around; moved by the middle button.
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Where the key light of the studio is, in the scene, whichever way the camera looks: how
    /// far round the vertical axis (which also turns an environment image), and how high
    /// above the horizon, from on it to straight overhead (never below: a light from under
    /// the floor is no studio's).
    pub light_yaw: f32,
    pub light_pitch: f32,
    pub shape: Shape,
    /// Displacement of white in the height map, in model units.
    pub displacement: f32,
    /// Texture repetitions.
    pub tiling: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            target: [0.0; 3],
            yaw: 0.0,
            pitch: 0.25,
            distance: 3.6,
            light_yaw: -0.7,
            light_pitch: 0.6,
            shape: Shape::RoundedCube,
            displacement: 0.04,
            tiling: 1.0,
        }
    }
}

/// What the mouse did to the 3D view, as a change: several moves can arrive in one frame,
/// and each is added to the camera as it is by then.
#[derive(Clone, Copy, Debug)]
pub enum Move {
    /// Turn around the target (left button), in pixels.
    Orbit(Vector),
    /// Turn the light (right button), in pixels.
    Light(Vector),
    /// Move the camera and its target sideways (middle button), in the units of the
    /// scene across the view.
    Pan(Vector),
    /// Get closer by this factor (the wheel).
    Zoom(f32),
}

impl Camera {
    /// Where the camera is.
    fn eye(&self) -> [f32; 3] {
        let d = spherical(self.yaw, self.pitch);
        [self.target[0] + d[0] * self.distance, self.target[1] + d[1] * self.distance, self.target[2] + d[2] * self.distance]
    }

    /// The camera after a move of the mouse.
    pub fn moved(self, m: Move) -> Camera {
        match m {
            Move::Orbit(d) => Camera { yaw: self.yaw - d.x * 0.01, pitch: (self.pitch + d.y * 0.01).clamp(-1.5, 1.5), ..self },
            Move::Light(d) => Camera {
                // Round and round, never so far that a turn of it is lost in the digits.
                light_yaw: wrapped(self.light_yaw + d.x * 0.01),
                light_pitch: (self.light_pitch - d.y * 0.01).clamp(0.0, LIGHT_TOP),
                ..self
            },
            Move::Pan(d) => {
                // Along the screen: its right and up, as the camera sees them.
                let forward = spherical(self.yaw, self.pitch).map(|v| -v);
                let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
                let up = cross(right, forward);
                let t = self.target;
                let target = [0, 1, 2].map(|i| t[i] - right[i] * d.x + up[i] * d.y);
                // Never so far that the model is lost: it is always somewhere in view.
                let limit = self.distance.max(4.0);
                Camera { target: target.map(|v| v.clamp(-limit, limit)), ..self }
            }
            Move::Zoom(f) => Camera { distance: (self.distance * f).clamp(1.6, 14.0), ..self },
        }
    }
}

/// How high the key light goes: all but straight overhead, where turning it would turn nothing.
const LIGHT_TOP: f32 = 1.5;

/// Which way the key light of the studio shines from, in the scene: where the camera's light
/// says, whichever way the camera looks (a light that went round with the camera lit every
/// face it was turned to).
fn light_direction(c: &Camera) -> [f32; 3] {
    spherical(c.light_yaw, c.light_pitch.clamp(0.0, LIGHT_TOP))
}

/// An angle in radians, from -π to π.
fn wrapped(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    if a.is_finite() { (a + PI).rem_euclid(TAU) - PI } else { 0.0 }
}

// ---------------------------------------------------------------------------- lighting

/// What lights the model: the studio or an environment image, how brightly, and what shows
/// behind the model. Kept from one session to the next (see `workspace`).
#[derive(Clone, Debug)]
pub struct Lighting {
    /// The environment image's file; `None`, the studio.
    pub file: Option<String>,
    /// The environment images used last, the latest first.
    pub recent: Vec<String>,
    /// The environment the view shows: the one read from `file`, or while that is read, the
    /// one before (`None`, the studio).
    pub shown: Option<Arc<Hdri>>,
    /// The environment read last, to come back to at once.
    pub kept: Option<Arc<Hdri>>,
    /// The file being read.
    pub reading: Option<String>,
    /// Brightness, in stops: each one doubles it.
    pub exposure: f32,
    /// Whether an environment image shows behind the model.
    pub background: bool,
    /// How blurred it shows there, from 0 (sharp) to 1 (as the roughest surface reflects it).
    pub blur: f32,
    /// Whether the menu of the lighting is open.
    pub menu: bool,
}

/// How many environment images the menu offers again.
pub const RECENT: usize = 8;

impl Default for Lighting {
    fn default() -> Self {
        Lighting { file: None, recent: Vec::new(), shown: None, kept: None, reading: None, exposure: 0.0, background: true, blur: 0.25, menu: false }
    }
}

impl Lighting {
    /// Put `file` first among the images used last.
    pub fn remember(&mut self, file: &str) {
        self.recent.retain(|f| f != file);
        self.recent.insert(0, file.to_string());
        self.recent.truncate(RECENT);
    }

    /// What lights the model: the image's file name, or the studio.
    pub fn name(&self) -> String {
        self.file.as_deref().map_or_else(|| "the studio".to_string(), file_name)
    }
}

pub fn toolbar(app: &App) -> Element<'_, Message> {
    let cam = app.ui.preview.view3d;
    // A list rather than a button each: the view next to it is the one that needs the room.
    let mut r = row![
        pick_list(Shape::ALL, Some(cam.shape), move |shape| Message::Preview(PreviewMessage::Camera(Camera { shape, ..cam })))
            .text_size(style::SMALL)
            .padding([2, 6])
            // As wide as its longest shape, however little room the view has: one line.
            .width(Length::Fixed(132.0))
            .style(style::pick_list),
        lighting_button(app),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    r = r.push(text("Height").size(style::SMALL).color(style::TEXT_DIM));
    r = r.push(
        container(
            number(cam.displacement as f64, 0.0, 0.3)
                .limits(0.0, 1.0)
                .on_change(move |v| Message::Preview(PreviewMessage::Camera(Camera { displacement: v as f32, ..cam }))),
        )
        .width(Length::Fixed(70.0)),
    );
    r = r.push(text("Repeat").size(style::SMALL).color(style::TEXT_DIM));
    r = r.push(
        container(
            number(cam.tiling as f64, 1.0, 8.0)
                .limits(0.25, 32.0)
                .on_change(move |v| Message::Preview(PreviewMessage::Camera(Camera { tiling: v as f32, ..cam }))),
        )
        .width(Length::Fixed(60.0)),
    );
    r = r.push(crate::view::tip(
        button(icon(Icon::Reset, 14.0, style::TEXT_DIM))
            .padding(4)
            .style(style::flat_button)
            .on_press(Message::Preview(PreviewMessage::Camera(Camera { shape: cam.shape, ..Camera::default() }))),
        "Put the camera, the light and the height back where they started",
        Position::Bottom,
    ));
    r.into()
}

/// The button of the lighting's menu, and the menu while it is open.
fn lighting_button(app: &App) -> Element<'_, Message> {
    let l = &app.ui.preview.lighting;
    let face = button(icon(Icon::Sun, 14.0, if l.file.is_some() { style::TEXT } else { style::TEXT_DIM }))
        .padding(4)
        .style(style::toggle_button(l.menu))
        .on_press(Message::Preview(PreviewMessage::LightingMenu(!l.menu)));
    let face = crate::view::tip(face, format!("Lit by {}: the environment, its brightness, and what shows behind the model", l.name()), Position::Bottom);
    dropdown(face, lighting_menu(app), l.menu).on_dismiss(Message::Preview(PreviewMessage::LightingMenu(false))).into()
}

fn lighting_menu(app: &App) -> Element<'_, Message> {
    let l = &app.ui.preview.lighting;
    let cam = app.ui.preview.view3d;
    let send = |m: PreviewMessage| Message::Preview(m);
    let choice = |on: bool, label: String, hint: String, message: Message| -> Element<'_, Message> {
        crate::view::tip(
            button(
                text(format!("{}  {label}", if on { "●" } else { "○" }))
                    .size(style::TEXT_SIZE)
                    .color(style::TEXT)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fill)
            .padding([4, 10])
            .style(style::row_button(on))
            .on_press(message),
            hint,
            Position::Right,
        )
    };
    let field = |label: &'static str, input: Element<'static, Message>| -> Element<'static, Message> {
        row![text(label).size(style::SMALL).color(style::TEXT_DIM).width(Length::Fixed(64.0)), input]
            .spacing(6)
            .padding([3, 10])
            .align_y(Alignment::Center)
            .into()
    };
    let mut items: Vec<Element<'_, Message>> =
        vec![container(text("Environment").size(style::SMALL).color(style::TEXT_DIM)).padding([4, 10]).into()];
    items.push(choice(
        l.file.is_none(),
        "Studio".into(),
        "A key light and a fill light under a sky. They stay where they are in the scene while the camera goes round, \
         so each side of the model is seen lit as it is; right drag turns them, or Turn and Height below"
            .into(),
        send(PreviewMessage::Environment(None)),
    ));
    for f in &l.recent {
        let reading = if l.reading.as_deref() == Some(f.as_str()) { "  (reading…)" } else { "" };
        items.push(choice(
            l.file.as_deref() == Some(f.as_str()),
            format!("{}{reading}", file_name(f)),
            f.clone(),
            send(PreviewMessage::Environment(Some(f.clone()))),
        ));
    }
    items.push(
        button(
            row![icon(Icon::Folder, 13.0, style::TEXT_DIM), text("Load an Environment Image…").size(style::TEXT_SIZE)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([4, 10])
        .style(style::row_button(false))
        .on_press(send(PreviewMessage::BrowseEnvironment))
        .into(),
    );
    items.push(
        container(rule::horizontal(1).style(|_| rule::Style { color: style::LINE, radius: 0.0.into(), fill_mode: rule::FillMode::Full, snap: true }))
            .padding([3, 6])
            .into(),
    );
    items.push(field(
        "Exposure",
        number(l.exposure as f64, -4.0, 4.0)
            .limits(-12.0, 12.0)
            .default_value(0.0)
            .suffix(" EV")
            .decimals(2)
            .on_change(move |v| send(PreviewMessage::Exposure(v as f32)))
            .into(),
    ));
    items.push(field(
        "Turn",
        number(f64::from(wrapped(cam.light_yaw).to_degrees()), -180.0, 180.0)
            .limits(-180.0, 180.0)
            .decimals(1)
            .suffix("°")
            .on_change(move |v| send(PreviewMessage::Camera(Camera { light_yaw: (v as f32).to_radians(), ..cam })))
            .into(),
    ));
    if l.file.is_none() {
        // How high the key light is above the horizon.
        items.push(field(
            "Height",
            number(f64::from(cam.light_pitch.to_degrees()), 0.0, f64::from(LIGHT_TOP.to_degrees()))
                .limits(0.0, f64::from(LIGHT_TOP.to_degrees()))
                .decimals(1)
                .suffix("°")
                .on_change(move |v| send(PreviewMessage::Camera(Camera { light_pitch: (v as f32).to_radians().clamp(0.0, LIGHT_TOP), ..cam })))
                .into(),
        ));
    }
    if l.file.is_some() {
        items.push(choice(
            l.background,
            "Show It Behind the Model".into(),
            "The environment image around the model, rather than the gray".into(),
            send(PreviewMessage::Background(!l.background)),
        ));
        items.push(field(
            "Blur",
            number(l.blur as f64, 0.0, 1.0).limits(0.0, 1.0).default_value(0.25).on_change(move |v| send(PreviewMessage::Blur(v as f32))).into(),
        ));
    }
    container(column(items).spacing(1).width(Length::Fixed(270.0))).padding(4).style(style::popup).into()
}

/// Material slots, in the order the shader binds them.
const USAGES: [u32; 7] = [USAGE_BASE_COLOR, USAGE_NORMAL, USAGE_HEIGHT, USAGE_ROUGHNESS, USAGE_METALLIC, USAGE_AO, USAGE_EMISSIVE];
const SLOT_NAMES: [&str; 7] = ["Base Color", "Normal", "Height", "Roughness", "Metallic", "AO", "Emissive"];

/// Material slot of an engine image, from its Output node's usage.
fn slot(app: &App, img: &brewed_texture_engine::DisplayImage) -> Option<usize> {
    let node = app.graph().node(img.node).filter(|n| n.kind == NodeKind::OUTPUT)?;
    USAGES.iter().position(|u| *u == node.enum_value("usage"))
}

/// Names of the maps shown on the model.
pub fn map_names(app: &App) -> Vec<&'static str> {
    let mut found = [false; 7];
    for img in &app.res.materials {
        if let Some(s) = slot(app, img) {
            found[s] = true;
        }
    }
    SLOT_NAMES.iter().zip(found).filter(|(_, f)| *f).map(|(n, _)| *n).collect()
}

/// The maps the model wears (see [`Scene::maps`]), and which of them are gray.
fn maps_of(app: &App) -> ([Option<wgpu::TextureView>; 7], u32) {
    let mut maps: [Option<wgpu::TextureView>; 7] = Default::default();
    let mut gray = 0u32;
    for img in &app.res.materials {
        if let Some(s) = slot(app, img) {
            maps[s] = Some(img.view.clone());
            if img.gray {
                gray |= 1 << s;
            }
        }
    }
    (maps, gray)
}

/// How much of a picture of a model the model fills, in its longer way across.
pub const PICTURE_FILL: f32 = 0.82;

/// The camera of a picture of a package (see [`picture`]), `aspect` wide for its height: from
/// where the view starts to look, whatever way the model was turned, moved or neared and the
/// light turned (a picture is not of where someone left the view: it would show a corner, or
/// nothing, of the model); the model, its relief and its repetitions are kept, they are how the
/// package is looked at. It looks at the middle of the model as the picture shows it, from as
/// far as makes the model fill [`PICTURE_FILL`] of it: as much room above it as below, on the
/// left as on the right.
pub fn picture_camera(view: Camera, aspect: f32) -> Camera {
    let mut camera = Camera { shape: view.shape, displacement: view.displacement, tiling: view.tiling, ..Camera::default() };
    let (vertices, _) = build_mesh(camera.shape);
    // (Perspective is not linear: a few steps bring it there.)
    for _ in 0..6 {
        let ([lo, hi], ..) = projected(&camera, aspect, vertices.iter().map(|v| v.pos));
        let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let half = ((hi[0] - lo[0]) / 2.0).max((hi[1] - lo[1]) / 2.0).max(1e-3);
        // The target moved across the view by as much of the world as the middle is off, at
        // the target's distance; the camera as far as makes the model the size it is to be.
        let eye = camera.eye();
        let forward = normalize(sub(camera.target, eye));
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let half_height = camera.distance * (FOV / 2.0).tan();
        let shift = [middle[0] * half_height * aspect, middle[1] * half_height];
        for (axis, (r, u)) in right.iter().zip(up).enumerate() {
            camera.target[axis] += r * shift[0] + u * shift[1];
        }
        camera.distance *= half / PICTURE_FILL;
    }
    camera
}

/// Where `points` are in the picture of `camera`, `aspect` wide for its height: the corners of
/// what they cover, from -1 to 1 across and up.
fn projected(camera: &Camera, aspect: f32, points: impl Iterator<Item = [f32; 3]>) -> ([[f32; 2]; 2], usize) {
    let m = mul(&perspective(FOV, aspect, 0.05, 50.0), &look_at(camera.eye(), camera.target, [0.0, 1.0, 0.0]));
    let (mut lo, mut hi, mut n) = ([f32::MAX; 2], [f32::MIN; 2], 0);
    for p in points {
        let p = [p[0], p[1], p[2], 1.0];
        let clip: [f32; 4] = std::array::from_fn(|r| (0..4).map(|c| m[c][r] * p[c]).sum());
        let at = [clip[0] / clip[3], clip[1] / clip[3]];
        lo = [lo[0].min(at[0]), lo[1].min(at[1])];
        hi = [hi[0].max(at[0]), hi[1].max(at[1])];
        n += 1;
    }
    ([lo, hi], n)
}

/// A picture of the model the 3D view shows, `size` pixels wide and high, framed as
/// [`picture_camera`] frames it; in the light of the view (`lit`: its environment image, behind
/// the model too, and its exposure) or as the view starts: from
/// its first camera (see [`picture_camera`]), in the studio's light as bright as it is made,
/// on the studio's background: what lists of packages show of one (see `app::home`), the same
/// whatever the view was left at. None when the model wears no map of the graph's (nothing of
/// the package on it), or the GPU could not draw it. The view's pipeline, made once, is kept
/// in `pipeline`.
pub fn picture(app: &App, pipeline: &mut Option<Pipeline>, gpu: &crate::gpu::GpuHandles, size: [u32; 2], lit: bool) -> Option<::image::RgbaImage> {
    use shader::Primitive as _;
    let (maps, gray) = maps_of(app);
    if maps.iter().all(Option::is_none) {
        return None;
    }
    let camera = picture_camera(app.ui.preview.view3d, size[0] as f32 / size[1].max(1) as f32);
    let l = &app.ui.preview.lighting;
    let primitive = match lit {
        true => Primitive {
            camera,
            maps,
            gray,
            environment: l.shown.clone(),
            exposure: l.exposure.clamp(-12.0, 12.0).exp2(),
            background: Some(l.blur.clamp(0.0, 1.0)),
        },
        false => Primitive { camera, maps, gray, environment: None, exposure: 1.0, background: None },
    };
    let (device, queue) = (&gpu.device, &gpu.queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let pipeline = pipeline.get_or_insert_with(|| <Pipeline as shader::Pipeline>::new(device, queue, format));
    let [width, height] = size;
    let bounds = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(width as f32, height as f32));
    primitive.prepare(pipeline, device, queue, &bounds, &Viewport::with_physical_size(iced::Size::new(width, height), 1.0));
    let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("brewed 3d picture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("brewed 3d picture") });
    // (Clear: the model is drawn over what is there.)
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("brewed 3d picture"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    }));
    primitive.render(pipeline, &mut encoder, &target, &Rectangle { x: 0, y: 0, width, height });
    let row = width * 4;
    let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("brewed 3d picture"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    let (sent, got) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |mapped| {
        let _ = sent.send(mapped);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    got.recv().ok()?.ok()?;
    let mut rgba = Vec::with_capacity((row * height) as usize);
    {
        let mapped = buffer.slice(..).get_mapped_range();
        for (y, line) in mapped.chunks(padded as usize).enumerate() {
            // The view's background where the model is not: from 48 at the top to 18 at the
            // bottom, as the view is drawn on.
            let back = (48.0 - 30.0 * y as f32 / (height as f32).max(1.0)).round() as u8;
            for px in line[..row as usize].chunks(4) {
                let a = f32::from(px[3]) / 255.0;
                let over = |c: u8| (f32::from(c) + f32::from(back) * (1.0 - a)).round().min(255.0) as u8;
                rgba.extend_from_slice(&[over(px[0]), over(px[1]), over(px[2]), 255]);
            }
        }
    }
    buffer.unmap();
    ::image::RgbaImage::from_raw(width, height, rgba)
}

pub fn view(app: &App) -> Element<'_, Message> {
    let (maps, gray) = maps_of(app);
    let missing = maps.iter().all(Option::is_none);
    let l = &app.ui.preview.lighting;
    let scene = Scene {
        wheel: if app.settings.invert_zoom { -1.0 } else { 1.0 },
        camera: app.ui.preview.view3d,
        maps,
        gray,
        realtime: app.ui.realtime,
        environment: l.shown.clone(),
        exposure: l.exposure.clamp(-12.0, 12.0).exp2(),
        background: l.background.then_some(l.blur.clamp(0.0, 1.0)),
    };
    let mut layers: Vec<Element<'_, Message>> = vec![shader_widget(scene).width(Length::Fill).height(Length::Fill).into()];
    if missing {
        layers.push(
            container(
                text("Set the Usage of Output nodes (Base Color, Normal, Height …) to see them here")
                    .size(style::SMALL)
                    .color(style::TEXT_DIM),
            )
            .padding(8)
            .into(),
        );
    }
    container(iced::widget::stack(layers))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                    .add_stop(0.0, style::gray(48))
                    .add_stop(1.0, style::gray(18)),
            ))),
            ..Default::default()
        })
        .into()
}

// ---------------------------------------------------------------------------- program

pub struct Scene {
    /// Which way the mouse wheel zooms: 1, the wheel pushed away zooms in; -1, the other way.
    wheel: f32,
    camera: Camera,
    /// Base color, normal, height, roughness, metallic, ambient occlusion, emissive.
    maps: [Option<wgpu::TextureView>; 7],
    /// The maps that are one channel (gray), as bits.
    gray: u32,
    /// Draw frame after frame, rather than only when something changes.
    realtime: bool,
    /// The environment image that lights the model; `None`, the studio.
    environment: Option<Arc<Hdri>>,
    /// What light is multiplied by: the exposure.
    exposure: f32,
    /// How blurred the environment image shows behind the model; `None`, it does not.
    background: Option<f32>,
}

#[derive(Default)]
pub struct DragState {
    orbit: Option<iced::Point>,
    light: Option<iced::Point>,
    pan: Option<iced::Point>,
    /// The frames the view drew, for the status bar.
    frames: FrameCounter,
}

impl shader::Program<Message> for Scene {
    type State = DragState;
    type Primitive = Primitive;

    fn update(
        &self,
        state: &mut DragState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Every move is sent as a change, never as the camera it makes: several moves can
        // arrive in one frame, and each has to add to the one before.
        let publish = |m: Move| Some(shader::Action::publish(Message::Preview(PreviewMessage::Move3d(m))).and_capture());
        match event {
            // Every frame the view draws is counted, and a few times a second the rate is said
            // (which draws the next frame too). Drawing in real time, each frame asks for the
            // next.
            iced::Event::Window(window::Event::RedrawRequested(now)) => match state.frames.frame(*now) {
                Some(rate) => Some(shader::Action::publish(Message::FrameRate(rate))),
                None if self.realtime => Some(shader::Action::request_redraw()),
                None => None,
            },
            iced::Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let p = cursor.position_over(bounds)?;
                match button {
                    mouse::Button::Left => state.orbit = Some(p),
                    mouse::Button::Right => state.light = Some(p),
                    mouse::Button::Middle => state.pan = Some(p),
                    _ => return None,
                }
                Some(shader::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                (state.orbit, state.light, state.pan) = (None, None, None);
                None
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let step = |last: &mut iced::Point| {
                    let d = *position - *last;
                    *last = *position;
                    d
                };
                if let Some(last) = &mut state.orbit {
                    return publish(Move::Orbit(step(last)));
                }
                if let Some(last) = &mut state.light {
                    return publish(Move::Light(step(last)));
                }
                if let Some(last) = &mut state.pan {
                    // The scene follows the mouse: a pixel of the view is this much of the scene
                    // at the distance of the target.
                    let scale = 2.0 * self.camera.distance * (FOV / 2.0).tan() / bounds.height.max(1.0);
                    return publish(Move::Pan(step(last) * scale));
                }
                None
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_over(bounds)?;
                let dy = self.wheel
                    * match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                        mouse::ScrollDelta::Pixels { y, .. } => *y,
                    };
                publish(Move::Zoom((-dy * 0.0015).exp()))
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &DragState, _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive {
            camera: self.camera,
            maps: self.maps.clone(),
            gray: self.gray,
            environment: self.environment.clone(),
            exposure: self.exposure,
            background: self.background,
        }
    }

    fn mouse_interaction(&self, state: &DragState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.orbit.is_some() || state.light.is_some() || state.pan.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[derive(Debug)]
pub struct Primitive {
    camera: Camera,
    maps: [Option<wgpu::TextureView>; 7],
    gray: u32,
    environment: Option<Arc<Hdri>>,
    exposure: f32,
    background: Option<f32>,
}

// ---------------------------------------------------------------------------- math

type Mat4 = [[f32; 4]; 4];

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = [[0.0; 4]; 4];
    for (c, col) in r.iter_mut().enumerate() {
        for (row, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

/// Vertical field of view of the 3D view, in radians.
const FOV: f32 = 0.62;

/// Right handed perspective with depth in [0, 1].
fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, far / (near - far), -1.0], [0.0, 0.0, near * far / (near - far), 0.0]]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

fn spherical(yaw: f32, pitch: f32) -> [f32; 3] {
    [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()]
}

// ---------------------------------------------------------------------------- meshes

struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

impl Mesh {
    fn new(device: &wgpu::Device, shape: Shape) -> Mesh {
        let (v, i) = build_mesh(shape);
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("brewed 3d mesh"),
            contents: bytemuck::cast_slice(&v),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("brewed 3d mesh"),
            contents: bytemuck::cast_slice(&i),
            usage: wgpu::BufferUsages::INDEX,
        });
        Mesh { vertices, indices, count: i.len() as u32 }
    }
}

// ---------------------------------------------------------------------------- pipeline

const SHADER: &str = include_str!("view3d.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: Mat4,
    eye: [f32; 4],
    light: [f32; 4],
    params: [f32; 4],
    flags: [u32; 4],
    turn: [f32; 4],
    levels: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    sh: [[f32; 4]; 9],
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    /// What draws the environment image behind the model.
    background: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    environment_sampler: wgpu::Sampler,
    defaults: [wgpu::TextureView; 7],
    /// Bound for the environment image when there is none.
    no_environment: wgpu::TextureView,
    /// The models shown so far, each made the first time it is.
    meshes: Vec<(Shape, Mesh)>,
    /// What makes environment images ready, once one is shown.
    filters: Option<environment::Filters>,
    /// The environment image shown, on the GPU.
    environment: Option<environment::Maps>,
    depth: Option<(wgpu::TextureView, [u32; 2])>,
    bind_group: Option<wgpu::BindGroup>,
    shape: Shape,
    viewport: [f32; 4],
    srgb: bool,
    /// Whether the environment image shows behind the model.
    behind: bool,
}

fn solid(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> wgpu::TextureView {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("brewed 3d default"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &rgba,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        crate::gpu::publish(device, queue);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brewed 3d view"),
            source: wgpu::ShaderSource::Wgsl([brewed_texture_engine::gpu::SHOW_WGSL, "\n", SHADER].concat().into()),
        });
        let texture = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        entries.extend((1..=7).map(texture));
        entries.push(sampler_entry(8));
        entries.extend([texture(9), texture(10), sampler_entry(11)]);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("brewed 3d view"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brewed 3d view"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let target = [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brewed 3d view"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                targets: &target,
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Behind everything: it neither reads nor writes depth.
        let background = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brewed 3d background"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("behind_vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("behind_fs"),
                targets: &target,
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brewed 3d view"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("brewed 3d view"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let defaults = [
            solid(device, queue, [184, 184, 184, 255]),
            solid(device, queue, [128, 128, 255, 255]),
            solid(device, queue, [0, 0, 0, 255]),
            solid(device, queue, [128, 128, 128, 255]),
            solid(device, queue, [0, 0, 0, 255]),
            solid(device, queue, [255, 255, 255, 255]),
            solid(device, queue, [0, 0, 0, 255]),
        ];
        Pipeline {
            pipeline,
            background,
            layout,
            uniforms,
            sampler,
            environment_sampler: environment::sampler(device),
            defaults,
            no_environment: solid(device, queue, [0, 0, 0, 255]),
            meshes: Vec::new(),
            filters: None,
            environment: None,
            depth: None,
            bind_group: None,
            shape: Shape::Sphere,
            viewport: [0.0; 4],
            srgb: format.is_srgb(),
            behind: false,
        }
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let scale = viewport.scale_factor();
        let physical = viewport.physical_size();
        let size = [physical.width.max(1), physical.height.max(1)];
        if pipeline.depth.as_ref().is_none_or(|(_, s)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("brewed 3d depth"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            pipeline.depth = Some((texture.create_view(&wgpu::TextureViewDescriptor::default()), size));
        }
        pipeline.viewport = [bounds.x * scale, bounds.y * scale, bounds.width * scale, bounds.height * scale];
        pipeline.shape = self.camera.shape;
        if !pipeline.meshes.iter().any(|(s, _)| *s == self.camera.shape) {
            pipeline.meshes.push((self.camera.shape, Mesh::new(device, self.camera.shape)));
        }
        // An environment image is made ready once, and let go when it is no longer shown.
        match &self.environment {
            Some(hdri) if pipeline.environment.as_ref().is_none_or(|m| m.id != hdri.id) => {
                let filters = pipeline.filters.get_or_insert_with(|| environment::Filters::new(device));
                pipeline.environment = Some(filters.make(device, queue, hdri));
            }
            Some(_) => {}
            None => pipeline.environment = None,
        }
        let env = pipeline.environment.as_ref();

        let c = &self.camera;
        let eye = c.eye();
        let aspect = (bounds.width / bounds.height.max(1.0)).max(0.01);
        let view_proj = mul(&perspective(FOV, aspect, 0.05, 50.0), &look_at(eye, c.target, [0.0, 1.0, 0.0]));
        let light = light_direction(c);
        let mut present = 0u32;
        for (i, m) in self.maps.iter().enumerate() {
            if m.is_some() {
                present |= 1 << i;
            }
        }
        // The rays behind the model: the camera's axes, reaching to the edges of the view.
        let forward = normalize(sub(c.target, eye));
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let half = (FOV / 2.0).tan();
        // The level of the image behind the model: sharp, the one whose pixels are as large as
        // the view's; fully blurred, the one 32 pixels wide (11° a pixel).
        let background = match (env, self.background) {
            (Some(m), Some(blur)) => {
                let width = m.source.width() as f32;
                let sharp = (FOV / (bounds.height * scale).max(1.0) * width / std::f32::consts::TAU).log2().max(0.0);
                let most = (width.log2() - 5.0).max(0.0);
                Some(sharp.max(blur * most))
            }
            _ => None,
        };
        pipeline.behind = background.is_some();
        let glossy_top = env.map_or(0, |m| m.specular.mip_level_count() - 1);
        let mut sh = [[0.0; 4]; 9];
        if let Some(m) = env {
            for (s, t) in sh.iter_mut().zip(m.sh) {
                *s = [t[0], t[1], t[2], 0.0];
            }
        }
        let u = Uniforms {
            view_proj,
            eye: [eye[0], eye[1], eye[2], 1.0],
            light: [light[0], light[1], light[2], 1.0],
            params: [c.displacement, c.tiling, self.exposure, 0.0],
            flags: [present, u32::from(pipeline.srgb), self.gray, u32::from(env.is_some())],
            turn: [c.light_yaw.cos(), c.light_yaw.sin(), 0.0, 0.0],
            levels: [background.unwrap_or_default(), glossy_top as f32, 0.0, 0.0],
            right: [right[0] * half * aspect, right[1] * half * aspect, right[2] * half * aspect, 0.0],
            up: [up[0] * half, up[1] * half, up[2] * half, 0.0],
            forward: [forward[0], forward[1], forward[2], 0.0],
            sh,
        };
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&u));
        let view = |i: usize| self.maps[i].as_ref().unwrap_or(&pipeline.defaults[i]);
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: pipeline.uniforms.as_entire_binding() }];
        for i in 0..7 {
            entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::TextureView(view(i)) });
        }
        entries.push(wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&pipeline.sampler) });
        let (image, glossy) = env.map_or((&pipeline.no_environment, &pipeline.no_environment), |m| (&m.source_view, &m.specular_view));
        entries.push(wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(image) });
        entries.push(wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::TextureView(glossy) });
        entries.push(wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::Sampler(&pipeline.environment_sampler) });
        pipeline.bind_group =
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("brewed 3d view"), layout: &pipeline.layout, entries: &entries }));
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let (Some(bind_group), Some((depth, _))) = (&pipeline.bind_group, &pipeline.depth) else { return };
        let Some((_, mesh)) = pipeline.meshes.iter().find(|(s, _)| *s == pipeline.shape) else { return };
        if clip_bounds.width == 0 || clip_bounds.height == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brewed 3d view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let [x, y, w, h] = pipeline.viewport;
        pass.set_viewport(x, y, w.max(1.0), h.max(1.0), 0.0, 1.0);
        pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, clip_bounds.width, clip_bounds.height);
        if pipeline.behind {
            pass.set_pipeline(&pipeline.background);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.count, 0, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of a package is of the model as the view starts, wherever the view was left:
    /// turned, moved off, neared, its light turned; the model, its relief and repetitions kept.
    /// Every model is in the middle of it, filling as much of it as the others, square or wide.
    #[test]
    fn pictures_of_packages_are_from_the_first_camera() {
        let left = Camera {
            target: [3.0, -2.0, 9.0],
            yaw: 2.7,
            pitch: -1.2,
            distance: 0.3,
            light_yaw: 2.0,
            light_pitch: -0.4,
            shape: Shape::Cloth,
            displacement: 0.12,
            tiling: 3.0,
        };
        let first = Camera { shape: Shape::Cloth, displacement: 0.12, tiling: 3.0, ..Camera::default() };
        assert_eq!(picture_camera(left, 1.0), picture_camera(first, 1.0), "wherever the view was left");
        let shown = picture_camera(left, 1.0);
        let start = Camera::default();
        assert_eq!((shown.yaw, shown.pitch, shown.light_yaw, shown.light_pitch), (start.yaw, start.pitch, start.light_yaw, start.light_pitch));
        assert_eq!((shown.shape, shown.displacement, shown.tiling), (Shape::Cloth, 0.12, 3.0));
        for shape in Shape::ALL {
            for aspect in [1.0, 1.6] {
                let camera = picture_camera(Camera { shape, ..Camera::default() }, aspect);
                let ([lo, hi], n) = projected(&camera, aspect, build_mesh(shape).0.iter().map(|v| v.pos));
                assert!(n > 0);
                let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                let half = ((hi[0] - lo[0]) / 2.0).max((hi[1] - lo[1]) / 2.0);
                assert!(middle[0].abs() < 0.01 && middle[1].abs() < 0.01, "{shape:?} at {aspect}: off the middle by {middle:?}");
                assert!((half - PICTURE_FILL).abs() < 0.01, "{shape:?} at {aspect}: fills {half}");
            }
        }
    }

    #[test]
    fn projection_maps_the_target_to_the_center() {
        let eye = [0.0, 0.0, 3.0];
        let m = mul(&perspective(0.8, 1.0, 0.1, 10.0), &look_at(eye, [0.0; 3], [0.0, 1.0, 0.0]));
        let p = [0.0f32, 0.0, 0.0, 1.0];
        let clip: Vec<f32> = (0..4).map(|r| (0..4).map(|c| m[c][r] * p[c]).sum()).collect();
        assert!(clip[0].abs() < 1e-5 && clip[1].abs() < 1e-5);
        let depth = clip[2] / clip[3];
        assert!(depth > 0.0 && depth < 1.0, "{depth}");
    }

    /// The studio's light stays where it is in the scene however the camera goes round, up or
    /// down: each face of a cube is lit as it is, not whichever one is looked at. Turning the
    /// light turns it, and it never goes under the horizon.
    #[test]
    fn the_light_stays_in_the_scene_when_the_camera_moves() {
        let start = Camera::default();
        let light = light_direction(&start);
        for m in [Move::Orbit(Vector::new(300.0, 0.0)), Move::Orbit(Vector::new(-120.0, 90.0)), Move::Pan(Vector::new(0.4, -0.2)), Move::Zoom(1.7)] {
            let moved = start.moved(m);
            assert_ne!((moved.yaw, moved.pitch, moved.target, moved.distance), (start.yaw, start.pitch, start.target, start.distance));
            assert_eq!(light_direction(&moved), light, "{m:?}");
        }
        let turned = start.moved(Move::Light(Vector::new(80.0, 0.0)));
        assert!(dot(light_direction(&turned), light) < 0.9, "turning the light turns it");
        let mut low = start;
        for _ in 0..100 {
            low = low.moved(Move::Light(Vector::new(0.0, 50.0)));
        }
        assert!(light_direction(&low)[1] >= 0.0, "never under the horizon");
        let mut high = start;
        for _ in 0..100 {
            high = high.moved(Move::Light(Vector::new(0.0, -50.0)));
        }
        assert!(light_direction(&high)[1] < 1.0, "never straight overhead, where turning would turn nothing");
    }

    #[test]
    fn the_light_turns_round_and_round() {
        let mut cam = Camera::default();
        for _ in 0..1000 {
            cam = cam.moved(Move::Light(Vector::new(97.0, 0.0)));
        }
        assert!((-std::f32::consts::PI..=std::f32::consts::PI).contains(&cam.light_yaw), "{}", cam.light_yaw);
        assert!((wrapped(3.0 * std::f32::consts::PI) - std::f32::consts::PI).abs() < 1e-4 || (wrapped(3.0 * std::f32::consts::PI) + std::f32::consts::PI).abs() < 1e-4);
        assert_eq!(wrapped(f32::NAN), 0.0);
    }
}
