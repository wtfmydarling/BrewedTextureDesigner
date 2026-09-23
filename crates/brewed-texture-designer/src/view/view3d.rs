//! 3D material preview: the graph's outputs (by their Usage) on a sphere, cube, plane or
//! cylinder, lit with a physically based model. Height displaces the vertices; the maps are
//! the images of the nodes the outputs show, with their mips, sampled directly on the GPU
//! and read as each output shows its image (the engine's `widen` and `as_output`).
//!
//! Left drag orbits, right drag moves the light, the wheel zooms.

use iced::widget::shader::{self, Viewport};
use iced::widget::{button, container, pick_list, row, shader as shader_widget, text};
use iced::{Alignment, Element, Length, Rectangle, Vector, mouse, window};
use brewed_texture_core::NodeKind;
use brewed_texture_core::nodes::{USAGE_AO, USAGE_BASE_COLOR, USAGE_EMISSIVE, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL, USAGE_ROUGHNESS};
use brewed_texture_engine::wgpu;
use wgpu::util::DeviceExt;

use crate::app::{App, Message, PreviewMessage};
use crate::style;
use crate::view::frames::FrameCounter;
use crate::widget::number::number;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A cube with rounded edges: flat faces to judge the texture on, and curves to
    /// catch the light. What a material is usually looked at on.
    RoundedCube,
    Sphere,
    Cube,
    Plane,
    Cylinder,
}

impl Shape {
    pub const ALL: [Shape; 5] = [Shape::RoundedCube, Shape::Sphere, Shape::Cube, Shape::Plane, Shape::Cylinder];

    fn label(self) -> &'static str {
        match self {
            Shape::RoundedCube => "Rounded Cube",
            Shape::Sphere => "Sphere",
            Shape::Cube => "Cube",
            Shape::Plane => "Plane",
            Shape::Cylinder => "Cylinder",
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The point the camera looks at and turns around; moved by the middle button.
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
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
                light_yaw: self.light_yaw + d.x * 0.01,
                light_pitch: (self.light_pitch - d.y * 0.01).clamp(-1.4, 1.5),
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

pub fn toolbar(app: &App) -> Element<'_, Message> {
    let cam = app.ui.preview.view3d;
    // A list rather than a button each: the view next to it is the one that needs the room.
    let mut r = row![
        pick_list(Shape::ALL, Some(cam.shape), move |shape| Message::Preview(PreviewMessage::Camera(Camera { shape, ..cam })))
            .text_size(style::SMALL)
            .padding([2, 6])
            .style(style::pick_list)
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
        button(crate::icons::icon(crate::icons::Icon::Reset, 14.0, style::TEXT_DIM))
            .padding(4)
            .style(style::flat_button)
            .on_press(Message::Preview(PreviewMessage::Camera(Camera { shape: cam.shape, ..Camera::default() }))),
        "Put the camera, the light and the height back where they started",
        iced::widget::tooltip::Position::Bottom,
    ));
    r.into()
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

pub fn view(app: &App) -> Element<'_, Message> {
    let mut maps: [Option<wgpu::TextureView>; 7] = Default::default();
    let (mut gray, mut formats) = (0u32, 0u32);
    for img in &app.res.materials {
        if let Some(s) = slot(app, img) {
            maps[s] = Some(img.view.clone());
            if img.gray {
                gray |= 1 << s;
            }
            formats |= (img.format & 7) << (s * 3);
        }
    }
    let missing = maps.iter().all(Option::is_none);
    let scene = Scene { camera: app.ui.preview.view3d, maps, gray, formats, realtime: app.ui.realtime };
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
    camera: Camera,
    /// Base color, normal, height, roughness, metallic, ambient occlusion, emissive.
    maps: [Option<wgpu::TextureView>; 7],
    /// The maps that are one channel (gray), as bits.
    gray: u32,
    /// The format each map's output shows it as, three bits a map.
    formats: u32,
    /// Draw frame after frame, rather than only when something changes.
    realtime: bool,
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
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                publish(Move::Zoom((-dy * 0.0015).exp()))
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &DragState, _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive { camera: self.camera, maps: self.maps.clone(), gray: self.gray, formats: self.formats }
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
    formats: u32,
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

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Right handed perspective with depth in [0, 1].
/// Vertical field of view of the 3D view, in radians.
const FOV: f32 = 0.62;

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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    tangent: [f32; 4],
    uv: [f32; 2],
}

struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

/// A grid of `n` x `n` quads from `f(u, v) -> (position, normal, tangent)`.
fn grid(n: u32, uv_scale: [f32; 2], f: impl Fn(f32, f32) -> ([f32; 3], [f32; 3], [f32; 3]), out: &mut (Vec<Vertex>, Vec<u32>)) {
    let base = out.0.len() as u32;
    for j in 0..=n {
        for i in 0..=n {
            let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
            let (pos, normal, tangent) = f(u, v);
            out.0.push(Vertex { pos, normal, tangent: [tangent[0], tangent[1], tangent[2], 1.0], uv: [u * uv_scale[0], v * uv_scale[1]] });
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = base + j * (n + 1) + i;
            let b = a + 1;
            let c = a + n + 1;
            let d = c + 1;
            out.1.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
}

/// The six faces of a cube as (normal, u axis, v axis); v runs downwards in the texture.
const CUBE_FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
    ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, -1.0, 0.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
    ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
];

fn build_mesh(shape: Shape) -> (Vec<Vertex>, Vec<u32>) {
    use std::f32::consts::{PI, TAU};
    let mut out = (Vec::new(), Vec::new());
    match shape {
        Shape::RoundedCube => {
            // The surface of a box of half-size `H - R` grown by a ball of radius `R`:
            // every point of a face is pushed out from the nearest point of that box, which
            // leaves the middle of a face flat and rounds the edges and corners.
            const H: f32 = 0.8;
            const R: f32 = 0.22;
            for (n, a, b) in CUBE_FACES {
                grid(
                    96,
                    [1.0, 1.0],
                    |u, v| {
                        let (s, t) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
                        let q = [
                            (n[0] + a[0] * s + b[0] * t) * H,
                            (n[1] + a[1] * s + b[1] * t) * H,
                            (n[2] + a[2] * s + b[2] * t) * H,
                        ];
                        let core = q.map(|c| c.clamp(-(H - R), H - R));
                        let out = [q[0] - core[0], q[1] - core[1], q[2] - core[2]];
                        let normal = normalize(out);
                        let p = [core[0] + normal[0] * R, core[1] + normal[1] * R, core[2] + normal[2] * R];
                        // The face's own u axis, along the surface: it turns with the edges.
                        let along = dot(a, normal);
                        let tangent = normalize([a[0] - normal[0] * along, a[1] - normal[1] * along, a[2] - normal[2] * along]);
                        (p, normal, tangent)
                    },
                    &mut out,
                );
            }
        }
        Shape::Sphere => grid(
            160,
            [2.0, 1.0],
            |u, v| {
                let (theta, phi) = (v * PI, u * TAU);
                let n = [theta.sin() * phi.sin(), theta.cos(), theta.sin() * phi.cos()];
                let t = [phi.cos(), 0.0, -phi.sin()];
                (n, n, t)
            },
            &mut out,
        ),
        Shape::Plane => grid(
            256,
            [1.0, 1.0],
            |u, v| ([u * 2.4 - 1.2, 1.2 - v * 2.4, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            &mut out,
        ),
        Shape::Cube => {
            for (n, a, b) in CUBE_FACES {
                grid(
                    64,
                    [1.0, 1.0],
                    |u, v| {
                        let (s, t) = ((u * 2.0 - 1.0) * 0.8, (v * 2.0 - 1.0) * 0.8);
                        let p = [n[0] * 0.8 + a[0] * s + b[0] * t, n[1] * 0.8 + a[1] * s + b[1] * t, n[2] * 0.8 + a[2] * s + b[2] * t];
                        (p, n, a)
                    },
                    &mut out,
                );
            }
        }
        Shape::Cylinder => {
            grid(
                160,
                [2.0, 1.0],
                |u, v| {
                    let phi = u * TAU;
                    let n = [phi.sin(), 0.0, phi.cos()];
                    ([n[0] * 0.8, 1.0 - v * 2.0, n[2] * 0.8], n, [phi.cos(), 0.0, -phi.sin()])
                },
                &mut out,
            );
            for (y, sign) in [(1.0f32, 1.0f32), (-1.0, -1.0)] {
                grid(
                    48,
                    [1.0, 1.0],
                    |u, v| {
                        let (r, phi) = (v * 0.8, u * TAU);
                        ([phi.sin() * r, y, phi.cos() * r], [0.0, sign, 0.0], [1.0, 0.0, 0.0])
                    },
                    &mut out,
                );
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------- pipeline

const SHADER: &str = r#"
struct U {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    params: vec4<f32>,   // displacement, texture repeat, _, _
    flags: vec4<u32>,    // present maps (bits), sRGB target, gray maps (bits), formats (3 bits a map)
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var base_map: texture_2d<f32>;
@group(0) @binding(2) var normal_map: texture_2d<f32>;
@group(0) @binding(3) var height_map: texture_2d<f32>;
@group(0) @binding(4) var rough_map: texture_2d<f32>;
@group(0) @binding(5) var metal_map: texture_2d<f32>;
@group(0) @binding(6) var ao_map: texture_2d<f32>;
@group(0) @binding(7) var emissive_map: texture_2d<f32>;
@group(0) @binding(8) var samp: sampler;

const PI: f32 = 3.14159265;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

fn has(bit: u32) -> bool {
    return (u.flags.x & (1u << bit)) != 0u;
}

// A map as its output shows it (a map of one channel is gray), from 0 to 1 as it is
// exported.
fn read_map(c: vec4<f32>, bit: u32) -> vec4<f32> {
    let gray = (u.flags.z & (1u << bit)) != 0u;
    let format = (u.flags.w >> (bit * 3u)) & 7u;
    return clamp(as_output(widen(c, gray), format), vec4<f32>(0.0), vec4<f32>(1.0));
}

@vertex
fn vs(v: VIn) -> VOut {
    let uv = v.uv * u.params.y;
    var p = v.pos;
    if (has(2u)) {
        let h = read_map(textureSampleLevel(height_map, samp, uv, 0.0), 2u).r;
        p = p + v.normal * h * u.params.x;
    }
    var o: VOut;
    o.clip = u.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.normal = v.normal;
    o.tangent = v.tangent;
    o.uv = uv;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn environment(dir: vec3<f32>) -> vec3<f32> {
    let sky = vec3<f32>(0.55, 0.62, 0.75);
    let ground = vec3<f32>(0.16, 0.14, 0.12);
    let t = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    return mix(ground, sky, t * t * (3.0 - 2.0 * t));
}

fn brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let ndl = max(dot(n, l), 0.0);
    let ndv = max(dot(n, v), 1e-4);
    let ndh = max(dot(n, h), 0.0);
    let vdh = max(dot(v, h), 0.0);
    let a = max(rough * rough, 0.002);
    let a2 = a * a;
    let d = a2 / (PI * pow(ndh * ndh * (a2 - 1.0) + 1.0, 2.0));
    let k = (rough + 1.0) * (rough + 1.0) / 8.0;
    let g = (ndl / (ndl * (1.0 - k) + k)) * (ndv / (ndv * (1.0 - k) + k));
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let f = f0 + (1.0 - f0) * pow(1.0 - vdh, 5.0);
    let spec = d * g * f / max(4.0 * ndl * ndv, 1e-4);
    let kd = (1.0 - f) * (1.0 - metal);
    return (kd * albedo / PI + spec) * ndl;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    var albedo = vec3<f32>(0.72);
    if (has(0u)) {
        albedo = srgb_to_linear(read_map(textureSample(base_map, samp, i.uv), 0u).rgb);
    }
    var n = normalize(i.normal);
    if (has(1u)) {
        let t = normalize(i.tangent.xyz - n * dot(n, i.tangent.xyz));
        let b = cross(n, t) * i.tangent.w;
        let m = read_map(textureSample(normal_map, samp, i.uv), 1u).rgb * 2.0 - 1.0;
        // OpenGL normal maps: +y points up in the texture, i.e. against v.
        n = normalize(t * m.x + b * m.y + n * m.z);
    }
    var rough = 0.5;
    if (has(3u)) { rough = max(read_map(textureSample(rough_map, samp, i.uv), 3u).r, 0.02); }
    var metal = 0.0;
    if (has(4u)) { metal = read_map(textureSample(metal_map, samp, i.uv), 4u).r; }
    var ao = 1.0;
    if (has(5u)) { ao = read_map(textureSample(ao_map, samp, i.uv), 5u).r; }
    var emissive = vec3<f32>(0.0);
    if (has(6u)) { emissive = srgb_to_linear(read_map(textureSample(emissive_map, samp, i.uv), 6u).rgb); }

    let v = normalize(u.eye.xyz - i.world);
    let key = normalize(u.light.xyz);
    let fill = normalize(vec3<f32>(-key.x, 0.3, -key.z));
    var color = brdf(n, v, key, albedo, rough, metal) * vec3<f32>(3.2, 3.05, 2.85);
    color = color + brdf(n, v, fill, albedo, rough, metal) * vec3<f32>(0.45, 0.5, 0.6);
    // Ambient: diffuse from the hemisphere, specular from the reflected direction.
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let ndv = max(dot(n, v), 1e-4);
    let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
    let r = reflect(-v, n);
    color = color + (environment(n) * albedo * (1.0 - metal) * 0.35 + environment(r) * fr * (1.0 - rough * 0.6) * 0.6) * ao;
    color = aces(color + emissive);
    if (u.flags.y == 0u) {
        color = linear_to_srgb(color);
    }
    return vec4<f32>(color, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: Mat4,
    eye: [f32; 4],
    light: [f32; 4],
    params: [f32; 4],
    flags: [u32; 4],
}

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    defaults: [wgpu::TextureView; 7],
    meshes: Vec<(Shape, Mesh)>,
    depth: Option<(wgpu::TextureView, [u32; 2])>,
    bind_group: Option<wgpu::BindGroup>,
    shape: Shape,
    viewport: [f32; 4],
    srgb: bool,
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
            source: wgpu::ShaderSource::Wgsl([brewed_texture_engine::gpu::SHOW_WGSL, SHADER].concat().into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        for b in 1..=7 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: b,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("brewed 3d view"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brewed 3d view"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
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
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
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
        let meshes = Shape::ALL
            .iter()
            .map(|shape| {
                let (v, i) = build_mesh(*shape);
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
                (*shape, Mesh { vertices, indices, count: i.len() as u32 })
            })
            .collect();
        Pipeline {
            pipeline,
            layout,
            uniforms,
            sampler,
            defaults,
            meshes,
            depth: None,
            bind_group: None,
            shape: Shape::Sphere,
            viewport: [0.0; 4],
            srgb: format.is_srgb(),
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

        let c = &self.camera;
        let eye = c.eye();
        let aspect = (bounds.width / bounds.height.max(1.0)).max(0.01);
        let view_proj = mul(&perspective(FOV, aspect, 0.05, 50.0), &look_at(eye, c.target, [0.0, 1.0, 0.0]));
        let light = spherical(c.light_yaw + c.yaw, c.light_pitch);
        let mut present = 0u32;
        for (i, m) in self.maps.iter().enumerate() {
            if m.is_some() {
                present |= 1 << i;
            }
        }
        let u = Uniforms {
            view_proj,
            eye: [eye[0], eye[1], eye[2], 1.0],
            light: [light[0], light[1], light[2], 1.0],
            params: [c.displacement, c.tiling, 0.0, 0.0],
            flags: [present, u32::from(pipeline.srgb), self.gray, self.formats],
        };
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&u));
        let view = |i: usize| self.maps[i].as_ref().unwrap_or(&pipeline.defaults[i]);
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: pipeline.uniforms.as_entire_binding() }];
        for i in 0..7 {
            entries.push(wgpu::BindGroupEntry { binding: 1 + i as u32, resource: wgpu::BindingResource::TextureView(view(i)) });
        }
        entries.push(wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&pipeline.sampler) });
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

    #[test]
    fn meshes_are_valid() {
        for shape in Shape::ALL {
            let (v, i) = build_mesh(shape);
            assert!(i.iter().all(|&x| (x as usize) < v.len()), "{shape:?}");
            assert_eq!(i.len() % 3, 0);
        }
    }
}
