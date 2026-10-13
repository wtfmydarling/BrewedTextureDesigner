//! 3D material preview: the graph's outputs (by their Usage) on a model, as the OpenPBR
//! material they make (its base, specular, transmission, subsurface, coat, fuzz, emission and
//! thin film, and the maps of older workflows: see [`super::shading`]), lit by the studio (a
//! key light, a fill light and a sky) or by an environment image (see [`crate::environment`]),
//! which can show behind the model too. Height displaces the vertices; the maps are the images
//! the outputs write, in their channels and at their bit depths, with their mips, sampled
//! directly on the GPU (one channel read as gray, with the engine's `widen`).
//!
//! Left drag orbits, right drag turns the light (or the environment), the wheel zooms.

use std::sync::Arc;

use brewed_texture_core::NodeKind;
use brewed_texture_core::nodes::{Usage, UsageGroup};
use brewed_texture_core::package::file_name;
use brewed_texture_engine::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::widget::tooltip::Position;
use iced::widget::{button, column, container, pick_list, row, rule, shader as shader_widget, text};
use iced::{Alignment, Element, Length, Rectangle, Vector, mouse, window};
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
use super::shading::{self, Material};

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
    /// How far apart black and white in the height map move the surface, in model units: white
    /// out and black in, half of it each; middle gray (128) leaves it where it is, as in
    /// Substance Designer.
    pub displacement: f32,
    /// Texture repetitions.
    pub tiling: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera::start(Shape::RoundedCube)
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
    /// The camera as the view starts, of the model `shape`: looking at the model's axis from a
    /// little above, from as far as shows the model whole in the middle of the view, filling
    /// 0.86 of a square one in its longer way across (a view of any other shape shows more
    /// around it: see [`half_height`]). From one place for every model, the cylinder and the material ball were
    /// cut off at the bottom of the view, the plane at its sides, and no model was in the middle.
    /// How high on its axis a model is looked at, and from how far, are measured from the model
    /// (a test holds them to it); on its axis, so that turned, the model does not swing.
    pub fn start(shape: Shape) -> Camera {
        let (height, distance) = match shape {
            Shape::RoundedCube => (-0.145, 3.886),
            Shape::Sphere => (0.0, 3.765),
            Shape::MatBall => (-0.132, 4.07),
            Shape::Cube => (-0.149, 3.91),
            Shape::RoundedCylinder => (-0.095, 3.786),
            Shape::Cylinder => (-0.133, 4.322),
            Shape::Torus => (-0.108, 3.674),
            Shape::Plane => (0.077, 4.634),
            Shape::Cloth => (0.038, 3.65),
        };
        Camera { target: [0.0, height, 0.0], yaw: 0.0, pitch: 0.25, distance, light_yaw: -0.7, light_pitch: 0.6, shape, displacement: 0.04, tiling: 1.0 }
    }

    /// The camera with the model `shape`: looking at it as the view starts to, where it looked
    /// at its model so (turned, it may be: neither moved off nor neared), and as it is where it
    /// did not — a view that was moved or neared is left where it was put.
    pub fn with_shape(self, shape: Shape) -> Camera {
        let (was, start) = (Camera::start(self.shape), Camera::start(shape));
        match (self.target, self.distance) == (was.target, was.distance) {
            true => Camera { shape, target: start.target, distance: start.distance, ..self },
            false => Camera { shape, ..self },
        }
    }

    /// Where the camera is.
    fn eye(&self) -> [f32; 3] {
        let d = spherical(self.yaw, self.pitch);
        [self.target[0] + d[0] * self.distance, self.target[1] + d[1] * self.distance, self.target[2] + d[2] * self.distance]
    }

    /// The camera after a move of the mouse.
    pub fn moved(self, m: Move) -> Camera {
        match m {
            // (Round and round too, never so far that a turn is lost in the digits.)
            Move::Orbit(d) => Camera { yaw: wrapped(self.yaw - d.x * 0.01), pitch: (self.pitch + d.y * 0.01).clamp(-1.5, 1.5), ..self },
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
    /// The Output of the graph whose panorama (its usage is Panorama) lights the model, as its
    /// node or the menu asked: none at first (a panorama being made is not what one wants to see
    /// a material in), and none again once the studio or an image is chosen.
    pub panorama: Option<brewed_texture_core::graph::NodeId>,
    /// Whether the studio's key light lights the model over an environment image too: the
    /// pictures of the templates' materials are made so (see `app::dev`), their softbox's
    /// highlights drawn out by an anisotropy, a fuzz's sheen along its edges.
    pub key: bool,
    /// An environment image a graph's view asked for as it was shown again (what its package
    /// says lit it), to be read once the message that showed it is handled.
    pub wanted: Option<String>,
}

/// How many environment images the menu offers again.
pub const RECENT: usize = 8;

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            file: None,
            recent: Vec::new(),
            shown: None,
            kept: None,
            reading: None,
            exposure: 0.0,
            // (Not behind the model unless asked: what one looks at is the material.)
            background: false,
            blur: 0.25,
            menu: false,
            panorama: None,
            key: false,
            wanted: None,
        }
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

/// How wide the 3D view is at least for its header to name its fields, and for them to be in the
/// header at all (narrower, they go on a line under it): its padding, icon and the gaps beside
/// the reading, then each tool and the gap after it.
pub fn full_3d() -> f32 {
    let label = |t: &str| crate::view::text_width(t, style::SMALL) + 4.0;
    16.0 + 14.0 + 12.0 + (132.0 + 4.0) + (22.0 + 4.0) + label("Height") + (70.0 + 4.0) + label("Repeat") + (60.0 + 4.0) + 22.0 + 1.0
}
pub const COMPACT_3D: f32 = 16.0 + 14.0 + 12.0 + (110.0 + 4.0) + (22.0 + 4.0) + (64.0 + 4.0) + (54.0 + 4.0) + 22.0 + 1.0;

/// The tools of the 3D view's header for a view `room` wide; and, in a narrow one, its fields on
/// a line of their own under it.
pub fn toolbar(app: &App, room: f32) -> (iced::widget::Row<'_, Message>, Option<iced::widget::Row<'_, Message>>) {
    let cam = app.ui.preview.view3d;
    let (named, under) = (room >= full_3d(), room < COMPACT_3D);
    // A list rather than a button each: the view next to it is the one that needs the room.
    // As wide as its longest shape where there is room: one line.
    let list = if named || under { 132.0 } else { 110.0 };
    let r = row![
        pick_list(Shape::ALL, Some(cam.shape), move |shape| Message::Preview(PreviewMessage::Camera(cam.with_shape(shape))))
            .text_size(style::SMALL)
            .padding([2, 6])
            .width(Length::Fixed(list))
            .style(style::pick_list),
        lighting_button(app),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    // (A drag a step of undo: they are the graph's.)
    let height = number(cam.displacement as f64, 0.0, 0.3)
        .limits(0.0, 1.0)
        .on_change(move |v| Message::Preview(PreviewMessage::Camera(Camera { displacement: v as f32, ..cam })))
        .on_release(Message::Seal);
    let repeat = number(cam.tiling as f64, 1.0, 8.0)
        .limits(0.25, 32.0)
        .on_change(move |v| Message::Preview(PreviewMessage::Camera(Camera { tiling: v as f32, ..cam })))
        .on_release(Message::Seal);
    let reset = crate::view::tip(
        button(icon(Icon::Reset, 14.0, style::TEXT_DIM))
            .padding(4)
            .style(style::flat_button)
            .on_press(Message::Preview(PreviewMessage::Camera(Camera::start(cam.shape)))),
        "Put the camera, the light and the height back where they started",
        Position::Bottom,
    );
    let label = |t: &'static str| text(t).size(style::SMALL).color(style::TEXT_DIM);
    // (Under the tools in a view too narrow even for the fields with their names: as they are
    // between, below.)
    let named = named || (under && room >= NAMED_FIELDS);
    if named {
        let fields = row![label("Height"), container(height).width(Length::Fixed(70.0)), label("Repeat"), container(repeat).width(Length::Fixed(60.0)),]
            .spacing(4)
            .align_y(Alignment::Center);
        if under {
            return (r.push(reset), Some(fields));
        }
        return (r.push(fields).push(reset), None);
    }
    // Between: the fields without their names, which their tips and their first letters give.
    let tip = |field: crate::widget::number::NumberField<'static, Message>, says: &'static str, width: f32| {
        crate::view::tip(container(field).width(Length::Fixed(width)), says, Position::Bottom)
    };
    let fields = row![
        tip(height.prefix("h "), "Height: how far apart the height map moves the model's surface: white out, black in, middle gray (128) not at all", 64.0),
        tip(repeat.prefix("× "), "Repeat: how many times the maps tile across the model", 54.0),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    if under {
        return (r.push(reset), Some(fields));
    }
    (r.push(fields).push(reset), None)
}

/// How wide the line of the fields with their names is (its padding, the names, the fields, the
/// gaps).
const NAMED_FIELDS: f32 = 16.0 + 40.0 + 4.0 + 70.0 + 4.0 + 42.0 + 4.0 + 60.0;

/// The button of the lighting's menu, and the menu while it is open.
fn lighting_button(app: &App) -> Element<'_, Message> {
    let l = &app.ui.preview.lighting;
    let lit = l.file.is_some() || app.lit_by_panorama();
    let face = button(icon(Icon::Sun, 14.0, if lit { style::TEXT } else { style::TEXT_DIM }))
        .padding(4)
        .style(style::toggle_button(l.menu))
        .on_press(Message::Preview(PreviewMessage::LightingMenu(!l.menu)));
    let name = if app.lit_by_panorama() { "the graph's panorama".to_string() } else { l.name() };
    let face = crate::view::tip(face, format!("Lit by {name}: the environment, its brightness, and what shows behind the model"), Position::Bottom);
    dropdown(face, lighting_menu(app), l.menu).on_dismiss(Message::Preview(PreviewMessage::LightingMenu(false))).into()
}

fn lighting_menu(app: &App) -> Element<'_, Message> {
    let l = &app.ui.preview.lighting;
    let cam = app.ui.preview.view3d;
    let send = |m: PreviewMessage| Message::Preview(m);
    let choice = |on: bool, label: String, hint: String, message: Message| -> Element<'_, Message> {
        crate::view::tip(
            // (A long name of a file shortened in its middle: the menu is as wide as it is.)
            button(crate::view::fitted(format!("{}  {label}", if on { "●" } else { "○" }), style::TEXT_SIZE, style::ui_font(), Some(style::TEXT)))
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
        // (A heading as the menus of choices have them: white and heavy.)
        vec![container(text("Environment").size(style::TEXT_SIZE).font(style::heavy()).color(style::TEXT_BRIGHT)).padding([4, 10]).into()];
    // The graph's own light first, where it has one.
    let panorama = app.lit_by_panorama();
    for id in app.panorama_outputs() {
        let Some(node) = app.graph().node(id) else { continue };
        items.push(choice(
            app.panorama_node() == Some(id),
            format!("The Graph's Panorama: {}", node.text("name")),
            "The image of the graph's Output whose usage is Panorama: the light all around, as the graph makes it, \
             brought up to date as the graph changes (its Output node turns it on and off too)"
                .into(),
            send(PreviewMessage::LightBy(Some(id))),
        ));
    }
    items.push(choice(
        l.file.is_none() && !panorama,
        "Studio".into(),
        "A key light and a fill light under a sky. They stay where they are in the scene while the camera goes round, \
         so each side of the model is seen lit as it is; right drag turns them, or Turn and Height below"
            .into(),
        send(PreviewMessage::Environment(None)),
    ));
    for f in &l.recent {
        let reading = if l.reading.as_deref() == Some(f.as_str()) { "  (reading…)" } else { "" };
        items.push(choice(
            l.file.as_deref() == Some(f.as_str()) && !panorama,
            format!("{}{reading}", file_name(f)),
            f.clone(),
            send(PreviewMessage::Environment(Some(f.clone()))),
        ));
    }
    items.push(
        button(
            row![icon(Icon::Folder, 13.0, style::TEXT_DIM), text("Load an Environment Image…").size(style::TEXT_SIZE)].spacing(8).align_y(Alignment::Center),
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
    let lit_by_image = l.file.is_some() || panorama;
    if !lit_by_image {
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
    if lit_by_image {
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
    // (It scrolls where the room it opens in is shorter than it: cut there, its fields under the
    // images were out of reach.)
    let items = crate::widget::scroll::scroll_box(column(items).spacing(1).width(Length::Fixed(270.0)));
    container(items).padding(4).style(style::popup).into()
}

/// The usage of an engine image, from its Output node.
fn usage_of(app: &App, img: &brewed_texture_engine::DisplayImage) -> Option<Usage> {
    let node = app.graph().node(img.node).filter(|n| n.kind == NodeKind::OUTPUT)?;
    Usage::of(node.enum_value("usage")).filter(|u| u.shown_in_3d())
}

/// What the model wears, said in a few words: its maps, by name while there are a few, or how
/// many there are of how many parts of the material.
pub fn map_names(app: &App) -> Vec<String> {
    let usages: Vec<Usage> = app.res.materials.iter().filter_map(|img| usage_of(app, img)).filter(|u| *u != Usage::Panorama).collect();
    if usages.len() <= 6 {
        return usages.iter().map(|u| u.name().to_string()).collect();
    }
    let parts = UsageGroup::ALL.iter().filter(|g| usages.iter().any(|u| u.group() == **g)).count();
    vec![format!("{} maps, {parts} parts of the material", usages.len())]
}

/// The maps the model wears (see [`Scene::maps`]).
fn maps_of(app: &App) -> Vec<shading::Map> {
    let found: Vec<(Usage, &brewed_texture_engine::DisplayImage)> = app.res.materials.iter().filter_map(|img| usage_of(app, img).map(|u| (u, img))).collect();
    shading::of(found.iter().map(|(u, img)| (*u, *img)))
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
    let mut camera = Camera { displacement: view.displacement, tiling: view.tiling, ..Camera::start(view.shape) };
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
        let half_height = camera.distance * half_height(aspect);
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
    let m = mul(&lens(aspect), &look_at(camera.eye(), camera.target, [0.0, 1.0, 0.0]));
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
/// the model where the view shows it there, its exposure, and its light where it is turned) or
/// as the view starts: from
/// its first camera (see [`picture_camera`]), in the studio's light as bright as it is made,
/// on the studio's background: what lists of packages show of one (see `app::home`), the same
/// whatever the view was left at. None when the model wears no map of the graph's (nothing of
/// the package on it), or the GPU could not draw it. The view's pipeline, made once, is kept
/// in `pipeline`.
pub fn picture(app: &App, pipeline: &mut Option<Pipeline>, gpu: &crate::gpu::GpuHandles, size: [u32; 2], lit: bool) -> Option<::image::RgbaImage> {
    let maps = maps_of(app);
    if maps.is_empty() {
        return None;
    }
    let mut camera = picture_camera(app.ui.preview.view3d, size[0] as f32 / size[1].max(1) as f32);
    let l = &app.ui.preview.lighting;
    let primitive = match lit {
        true => {
            (camera.light_yaw, camera.light_pitch) = (app.ui.preview.view3d.light_yaw, app.ui.preview.view3d.light_pitch);
            Primitive {
                camera,
                maps,
                environment: app.environment_shown(),
                exposure: l.exposure.clamp(-12.0, 12.0).exp2(),
                background: l.background.then(|| l.blur.clamp(0.0, 1.0)),
                key: l.key,
            }
        }
        false => Primitive { camera, maps, environment: None, exposure: 1.0, background: None, key: false },
    };
    render(&primitive, pipeline, &gpu.device, &gpu.queue, size)
}

/// `primitive` drawn on its own, `size` pixels wide and high, over the view's background.
fn render(primitive: &Primitive, pipeline: &mut Option<Pipeline>, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2]) -> Option<::image::RgbaImage> {
    let bounds = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(size[0] as f32, size[1] as f32));
    let mut drawn = render_in(primitive, pipeline, device, queue, size, bounds, 1.0)?;
    let height = size[1] as f32;
    for (_, y, px) in drawn.enumerate_pixels_mut() {
        // The view's background where the model is not: from 48 at the top to 18 at the
        // bottom, as the view is drawn on.
        let back = (48.0 - 30.0 * y as f32 / height.max(1.0)).round();
        let a = f32::from(px[3]) / 255.0;
        let over = |c: u8| (f32::from(c) + back * (1.0 - a)).round().min(255.0) as u8;
        *px = ::image::Rgba([over(px[0]), over(px[1]), over(px[2]), 255]);
    }
    Some(drawn)
}

/// `primitive` drawn as the framework has it drawn: as the view at `bounds` (in the program's
/// points) of a window `size` pixels wide and high, on a screen of the scale `scale`; over
/// nothing (what is not drawn is left clear).
fn render_in(
    primitive: &Primitive,
    pipeline: &mut Option<Pipeline>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: [u32; 2],
    bounds: Rectangle,
    scale: f32,
) -> Option<::image::RgbaImage> {
    use shader::Primitive as _;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let pipeline = pipeline.get_or_insert_with(|| <Pipeline as shader::Pipeline>::new(device, queue, format));
    let [width, height] = size;
    // (What the framework lets a view draw on: its place in whole pixels, within the window.)
    let window = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(width as f32, height as f32));
    let clip = (bounds * scale).intersection(&window).and_then(Rectangle::snap)?;
    primitive.prepare(pipeline, device, queue, &bounds, &Viewport::with_physical_size(iced::Size::new(width, height), scale));
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
    primitive.render(pipeline, &mut encoder, &target, &clip);
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
        for line in mapped.chunks(padded as usize) {
            rgba.extend_from_slice(&line[..row as usize]);
        }
    }
    buffer.unmap();
    ::image::RgbaImage::from_raw(width, height, rgba)
}

pub fn view(app: &App) -> Element<'_, Message> {
    let maps = maps_of(app);
    let missing = maps.is_empty();
    let l = &app.ui.preview.lighting;
    let scene = Scene {
        wheel: if app.settings.invert_zoom { -1.0 } else { 1.0 },
        camera: app.ui.preview.view3d,
        maps,
        realtime: app.ui.realtime,
        environment: app.environment_shown(),
        exposure: l.exposure.clamp(-12.0, 12.0).exp2(),
        background: l.background.then_some(l.blur.clamp(0.0, 1.0)),
        key: l.key,
    };
    let mut layers: Vec<Element<'_, Message>> = vec![shader_widget(scene).width(Length::Fill).height(Length::Fill).into()];
    // How fast it draws, in its corner. Digits in the monospaced font: they change every quarter
    // of a second.
    let rate = app.ui.frame_rate;
    let frames = crate::view::tip(
        text(format!("{:.0} FPS · {:.1} ms", rate.fps, rate.frame_ms))
            .size(style::SMALL)
            .font(style::mono())
            .color(style::TEXT_DIM)
            .wrapping(text::Wrapping::None),
        format!(
            "Frames the 3D view drew in the last second, and how long a frame took on average (the \
             longest {:.1} ms). View → Real-time Rendering draws it frame after frame; without it, it \
             draws only when something changes.",
            rate.worst_ms
        ),
        Position::Top,
    );
    // What to do to see the maps at the top, how fast it draws at the bottom: one under the
    // other, never over each other (in a short view they were); where the view is too short for
    // both, the rate goes out of it.
    let mut said = column![];
    if missing {
        said = said.push(
            container(text("Set the Usage of Output nodes (Base Color, Normal, Height …) to see them here").size(style::SMALL).color(style::TEXT_DIM))
                .padding(8),
        );
    }
    said = said.push(iced::widget::space().height(Length::Fill)).push(container(frames).padding([6, 8]).width(Length::Fill).align_right(Length::Fill));
    layers.push(container(said).width(Length::Fill).height(Length::Fill).clip(true).into());
    container(iced::widget::stack(layers))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI)).add_stop(0.0, style::gray(48)).add_stop(1.0, style::gray(18)),
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
    /// The maps of the material, each of the input of the shader it gives.
    maps: Vec<shading::Map>,
    /// Draw frame after frame, rather than only when something changes.
    realtime: bool,
    /// The environment image that lights the model; `None`, the studio.
    environment: Option<Arc<Hdri>>,
    /// What light is multiplied by: the exposure.
    exposure: f32,
    /// How blurred the environment image shows behind the model; `None`, it does not.
    background: Option<f32>,
    /// The studio's key light over the environment image too (see `Lighting::key`).
    key: bool,
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

    fn update(&self, state: &mut DragState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<shader::Action<Message>> {
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
                    let scale = 2.0 * self.camera.distance * half_height(bounds.width / bounds.height.max(1.0)) / bounds.height.max(1.0);
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
            environment: self.environment.clone(),
            exposure: self.exposure,
            background: self.background,
            key: self.key,
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
    maps: Vec<shading::Map>,
    environment: Option<Arc<Hdri>>,
    exposure: f32,
    background: Option<f32>,
    key: bool,
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

/// How wide the 3D view looks, in radians: across the smaller of its sides (see
/// [`half_height`]).
const FOV: f32 = 0.62;

/// Half of what a view `aspect` wide for its height shows up and down, at a distance of 1. The
/// view looks [`FOV`] wide across the smaller of its sides: what is whole in a square view is
/// whole in a view of any shape (looking so wide up and down whatever its shape, a view
/// narrower than it is high cut the model off at its sides).
fn half_height(aspect: f32) -> f32 {
    (FOV / 2.0).tan() / aspect.clamp(0.01, 1.0)
}

/// What a view `aspect` wide for its height is seen through (see [`half_height`]).
fn lens(aspect: f32) -> Mat4 {
    perspective(2.0 * half_height(aspect).atan(), aspect, 0.05, 50.0)
}

/// Right handed perspective with depth in [0, 1].
fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    [[f / aspect, 0.0, 0.0, 0.0], [0.0, f, 0.0, 0.0], [0.0, 0.0, far / (near - far), -1.0], [0.0, 0.0, near * far / (near - far), 0.0]]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [[s[0], u[0], -f[0], 0.0], [s[1], u[1], -f[1], 0.0], [s[2], u[2], -f[2], 0.0], [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0]]
}

fn spherical(yaw: f32, pitch: f32) -> [f32; 3] {
    [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()]
}

// ---------------------------------------------------------------------------- meshes

struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    /// How many of its vertices a unit of the texture has across it (see `models::density`).
    density: f32,
}

impl Mesh {
    fn new(device: &wgpu::Device, shape: Shape) -> Mesh {
        let (v, i) = build_mesh(shape);
        let density = super::models::density(&v, &i);
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
        Mesh { vertices, indices, count: i.len() as u32, density }
    }
}

// ---------------------------------------------------------------------------- pipeline

const SHADER: &str = include_str!("view3d.wgsl");

/// How many samples each pixel of the view is drawn with: the edges of the model, and of what
/// its height makes of it, are smooth (drawn with one, they were steps).
const SAMPLES: u32 = 4;

/// What shows the view once it is drawn: the model is drawn with several samples a pixel into
/// a picture of its own, which is then put where the view is.
const SHOW: &str = r"
@group(0) @binding(0) var drawn: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Out;
    o.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    return textureSampleLevel(drawn, samp, i.uv, 0.0);
}
";

/// What the view is drawn into before it is shown, as large as the view: the model, several
/// samples a pixel, and their depths; what they come to, a sample a pixel, and that bound to be
/// shown.
struct Drawn {
    size: [u32; 2],
    samples: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
    shown: wgpu::BindGroup,
}

impl Drawn {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, size: [u32; 2]) -> Drawn {
        let texture = |label, format, sample_count, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let resolved = texture("brewed 3d drawn", format, 1, attachment | wgpu::TextureUsages::TEXTURE_BINDING);
        let shown = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("brewed 3d drawn"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolved) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Drawn {
            size,
            samples: texture("brewed 3d samples", format, SAMPLES, attachment),
            depth: texture("brewed 3d depth", wgpu::TextureFormat::Depth32Float, SAMPLES, attachment),
            resolved,
            shown,
        }
    }
}

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
    /// Where the shader reads each of its inputs (see `shading::place`).
    inputs: [u32; shading::MAX_INPUTS],
}

pub struct Pipeline {
    /// What draws the model: both sides of each triangle at once, where it is all there; where
    /// it is not (its opacity), its back faces first, then its front faces over them.
    pipeline: wgpu::RenderPipeline,
    back_faces: wgpu::RenderPipeline,
    front_faces: wgpu::RenderPipeline,
    /// What draws the environment image behind the model.
    background: wgpu::RenderPipeline,
    /// What puts the view, drawn, where it is shown (see [`Drawn`]).
    show: wgpu::RenderPipeline,
    show_layout: wgpu::BindGroupLayout,
    show_sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    environment_sampler: wgpu::Sampler,
    /// What makes the material ready, and what the shader reads of it besides its maps.
    material: Material,
    /// Whether the material has an opacity map (drawn in two passes, see `back_faces`).
    see_through: bool,
    /// Bound for the environment image when there is none.
    no_environment: wgpu::TextureView,
    /// The models shown so far, each made the first time it is.
    meshes: Vec<(Shape, Mesh)>,
    /// What makes environment images ready, once one is shown.
    filters: Option<environment::Filters>,
    /// The environment image shown, on the GPU.
    environment: Option<environment::Maps>,
    drawn: Option<Drawn>,
    bind_group: Option<wgpu::BindGroup>,
    shape: Shape,
    /// Where the view is in what it is shown in, in whole pixels: its corner, and its size.
    viewport: [u32; 4],
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
            source: wgpu::ShaderSource::Wgsl([brewed_texture_engine::gpu::SHOW_WGSL, "\n", &shading::wgsl_inputs(), SHADER].concat().into()),
        });
        let texture = |binding: u32, visibility: wgpu::ShaderStages, filterable: bool, view_dimension: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension, multisampled: false },
            count: None,
        };
        let sampler_entry = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let flat = wgpu::TextureViewDimension::D2;
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: both,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        // The slots of the maps, the extra maps' layers, the row of values: the vertices read
        // the height of them.
        entries.extend((1..=shading::SLOTS).map(|b| texture(b, both, true, flat)));
        entries.push(texture(12, both, true, wgpu::TextureViewDimension::D2Array));
        entries.push(texture(13, both, true, flat));
        entries.push(sampler_entry(14, both));
        entries.extend([texture(15, fragment, true, flat), texture(16, fragment, true, flat), sampler_entry(17, fragment), texture(18, fragment, true, flat)]);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("brewed 3d view"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("brewed 3d view"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        // What the shader writes is multiplied by how much of the surface is there.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let target = [Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })];
        let model = |cull_mode: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("brewed 3d view"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2, 4 => Float32x2],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &target, compilation_options: Default::default() }),
                primitive: wgpu::PrimitiveState { cull_mode, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                multiview: None,
                cache: None,
            })
        };
        let (pipeline, back_faces, front_faces) = (model(None), model(Some(wgpu::Face::Front)), model(Some(wgpu::Face::Back)));
        let target = [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        // Behind everything: it neither reads nor writes depth.
        let background = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brewed 3d background"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("behind_vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("behind_fs"), targets: &target, compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview: None,
            cache: None,
        });
        // What shows the view drawn: over what is there, as much as the model is there.
        let show_module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("brewed 3d shown"), source: wgpu::ShaderSource::Wgsl(SHOW.into()) });
        let show_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("brewed 3d shown"),
            entries: &[texture(0, fragment, true, flat), sampler_entry(1, fragment)],
        });
        let show = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brewed 3d shown"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("brewed 3d shown"),
                bind_group_layouts: &[&show_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState { module: &show_module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &show_module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // (A pixel of the view drawn for each pixel of the view shown: nothing to filter.)
        let show_sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("brewed 3d shown"), ..Default::default() });
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
        Pipeline {
            pipeline,
            back_faces,
            front_faces,
            background,
            show,
            show_layout,
            show_sampler,
            format,
            layout,
            uniforms,
            sampler,
            environment_sampler: environment::sampler(device),
            material: Material::new(device, queue),
            see_through: false,
            no_environment: solid(device, queue, [0, 0, 0, 255]),
            meshes: Vec::new(),
            filters: None,
            environment: None,
            drawn: None,
            bind_group: None,
            shape: Shape::Sphere,
            viewport: [0; 4],
            srgb: format.is_srgb(),
            behind: false,
        }
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(&self, pipeline: &mut Pipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &Viewport) {
        let scale = viewport.scale_factor();
        // The view in whole pixels of what it is shown in — the pixels the framework lets it
        // draw on, and puts what is beside it up to: each edge at the nearest pixel, whatever
        // the scale of the screen (ended a pixel before, a line of what is under the view
        // showed along its edge at some scales and sizes). What it is drawn into is as large.
        let physical = viewport.physical_size();
        let (left, top) = ((bounds.x * scale).round().max(0.0), (bounds.y * scale).round().max(0.0));
        let right = ((bounds.x + bounds.width) * scale).round().min(physical.width as f32);
        let bottom = ((bounds.y + bounds.height) * scale).round().min(physical.height as f32);
        let largest = device.limits().max_texture_dimension_2d;
        let size = [((right - left).max(1.0) as u32).min(largest), ((bottom - top).max(1.0) as u32).min(largest)];
        if pipeline.drawn.as_ref().is_none_or(|d| d.size != size) {
            pipeline.drawn = Some(Drawn::new(device, pipeline.format, &pipeline.show_layout, &pipeline.show_sampler, size));
        }
        pipeline.viewport = [left as u32, top as u32, size[0], size[1]];
        pipeline.shape = self.camera.shape;
        if !pipeline.meshes.iter().any(|(s, _)| *s == self.camera.shape) {
            pipeline.meshes.push((self.camera.shape, Mesh::new(device, self.camera.shape)));
        }
        let density = pipeline.meshes.iter().find(|(s, _)| *s == self.camera.shape).map_or(1.0, |(_, mesh)| mesh.density);
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
        // (As wide for its height as the pixels it is drawn into are: by its size in points,
        // whose edges fall between the pixels, a sphere was a pixel or two out of round at
        // some scales.)
        let aspect = (size[0] as f32 / size[1].max(1) as f32).max(0.01);
        let view_proj = mul(&lens(aspect), &look_at(eye, c.target, [0.0, 1.0, 0.0]));
        let light = light_direction(c);
        let placed = shading::place(&self.maps);
        pipeline.material.prepare(device, queue, &self.maps, &placed);
        pipeline.see_through = self.maps.iter().any(|m| m.input == shading::input("OPACITY"));
        // The rays behind the model: the camera's axes, reaching to the edges of the view.
        let forward = normalize(sub(c.target, eye));
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let half = half_height(aspect);
        // The level of the image behind the model: sharp, the one whose pixels are as large as
        // the view's; fully blurred, the one 32 pixels wide (11° a pixel).
        let background = match (env, self.background) {
            (Some(m), Some(blur)) => {
                let width = m.source.width() as f32;
                let sharp = (2.0 * half.atan() / (bounds.height * scale).max(1.0) * width / std::f32::consts::TAU).log2().max(0.0);
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
            params: [c.displacement, c.tiling, self.exposure, density],
            flags: [u32::from(self.key), u32::from(pipeline.srgb), 0, u32::from(env.is_some())],
            turn: [c.light_yaw.cos(), c.light_yaw.sin(), 0.0, 0.0],
            levels: [background.unwrap_or_default(), glossy_top as f32, 0.0, 0.0],
            right: [right[0] * half * aspect, right[1] * half * aspect, right[2] * half * aspect, 0.0],
            up: [up[0] * half, up[1] * half, up[2] * half, 0.0],
            forward: [forward[0], forward[1], forward[2], 0.0],
            sh,
            inputs: placed.table,
        };
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&u));
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: pipeline.uniforms.as_entire_binding() }];
        for slot in 0..shading::SLOTS {
            let view = placed.slots.get(slot as usize).map_or(&pipeline.material.nothing, |m| &m.view);
            entries.push(wgpu::BindGroupEntry { binding: 1 + slot, resource: wgpu::BindingResource::TextureView(view) });
        }
        entries.push(wgpu::BindGroupEntry { binding: 12, resource: wgpu::BindingResource::TextureView(&pipeline.material.extra_view) });
        entries.push(wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(&pipeline.material.values_view) });
        entries.push(wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::Sampler(&pipeline.sampler) });
        let (image, glossy) = env.map_or((&pipeline.no_environment, &pipeline.no_environment), |m| (&m.source_view, &m.specular_view));
        entries.push(wgpu::BindGroupEntry { binding: 15, resource: wgpu::BindingResource::TextureView(image) });
        entries.push(wgpu::BindGroupEntry { binding: 16, resource: wgpu::BindingResource::TextureView(glossy) });
        entries.push(wgpu::BindGroupEntry { binding: 17, resource: wgpu::BindingResource::Sampler(&pipeline.environment_sampler) });
        entries.push(wgpu::BindGroupEntry { binding: 18, resource: wgpu::BindingResource::TextureView(&pipeline.material.lut_view) });
        pipeline.bind_group =
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("brewed 3d view"), layout: &pipeline.layout, entries: &entries }));
    }

    fn render(&self, pipeline: &Pipeline, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, clip_bounds: &Rectangle<u32>) {
        let (Some(bind_group), Some(drawn)) = (&pipeline.bind_group, &pipeline.drawn) else { return };
        let Some((_, mesh)) = pipeline.meshes.iter().find(|(s, _)| *s == pipeline.shape) else { return };
        if clip_bounds.width == 0 || clip_bounds.height == 0 {
            return;
        }
        // Drawn, several samples a pixel, into a picture of its own (see `Drawn`)...
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brewed 3d view"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &drawn.samples,
                depth_slice: None,
                resolve_target: Some(&drawn.resolved),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &drawn.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if pipeline.behind {
            pass.set_pipeline(&pipeline.background);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let passes: &[&wgpu::RenderPipeline] = if pipeline.see_through { &[&pipeline.back_faces, &pipeline.front_faces] } else { &[&pipeline.pipeline] };
        for p in passes {
            pass.set_pipeline(p);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
        drop(pass);
        // ... then shown where the view is, over what is there.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("brewed 3d shown"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let [x, y, w, h] = pipeline.viewport;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, clip_bounds.width, clip_bounds.height);
        pass.set_pipeline(&pipeline.show);
        pass.set_bind_group(0, &drawn.shown, &[]);
        pass.draw(0..3, 0..1);
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

    /// Each layer of OpenPBR shows on the model, as the templates have them: the coat's sharp
    /// highlight over a rough metal, the fuzz's glow at the edges, the anisotropy's highlight
    /// drawn out one way, light under the surface into its shadow; and the glow of an
    /// emission, a thin film's colors, a surface that is partly not there, glass. Each against
    /// the same material with the layer off (its weight 0, or its default).
    #[test]
    fn the_layers_of_openpbr_show_on_the_model() {
        use brewed_texture_core::graph::Graph;
        use brewed_texture_core::templates::Template;
        use brewed_texture_core::types::ParamValue;
        let Ok(gpu) = brewed_texture_engine::Gpu::headless() else { return };
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let mut engine = brewed_texture_engine::Engine::new(gpu);
        let mut pipeline = None;
        const SIDE: u32 = 96;
        let mut draw = |g: &Graph| -> ::image::RgbaImage {
            let outputs: Vec<_> = g.nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            let _ = engine.prepare_materials(&[]);
            engine.set_materials(&outputs);
            brewed_texture_engine::engine::evaluate_blocking(&mut engine, g);
            let shown = engine.prepare_materials(&outputs).expect("the maps");
            let found: Vec<(Usage, &brewed_texture_engine::DisplayImage)> =
                shown.iter().filter_map(|img| Usage::of(g.node(img.node)?.enum_value("usage")).filter(|u| u.shown_in_3d()).map(|u| (u, img))).collect();
            let maps = shading::of(found.iter().map(|(u, img)| (*u, *img)));
            let camera = picture_camera(Camera { shape: Shape::Sphere, ..Camera::default() }, 1.0);
            let primitive = Primitive { camera, maps, environment: None, exposure: 1.0, background: None, key: false };
            render(&primitive, &mut pipeline, &device, &queue, [SIDE, SIDE]).expect("a picture")
        };
        // The template, with the Output named `name` fed `value` instead.
        let with = |template: Template, changes: &[(&str, [f32; 4])]| -> Graph {
            let mut g = template.graph([64, 64], brewed_texture_core::graph::Precision::Half);
            for (name, value) in changes {
                let out = g.nodes_of(NodeKind::OUTPUT).find(|n| n.text("name") == *name).map(|n| n.id).expect(name);
                let uniform = g.links.iter().find(|l| l.to == out).map(|l| l.from).expect("fed by a uniform color");
                // (A color, or the number of a map that holds numbers.)
                let node = g.node_mut(uniform).expect("the node");
                match node.kind == NodeKind::UNIFORM_VALUE {
                    true => node.param_mut("value").expect("its value").value = ParamValue::Float(value[0]),
                    false => node.param_mut("color").expect("its color").value = ParamValue::Color(*value),
                }
            }
            g
        };
        let gray = |v: f32| [v, v, v, 1.0];
        let lum = |p: &::image::Rgba<u8>| 0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]);
        // The pixels of the sphere (those that are not the background's gray), as (x, y, pixel).
        let sphere = |img: &::image::RgbaImage| -> Vec<(u32, u32, ::image::Rgba<u8>)> {
            let back = draw_background(SIDE);
            img.enumerate_pixels().filter(|(x, y, p)| p.0 != back.get_pixel(*x, *y).0).map(|(x, y, p)| (x, y, *p)).collect()
        };
        let mean = |pixels: &[(u32, u32, ::image::Rgba<u8>)], f: &dyn Fn(&::image::Rgba<u8>) -> f32| {
            pixels.iter().map(|(_, _, p)| f(p)).sum::<f32>() / pixels.len().max(1) as f32
        };
        let differ = |a: &::image::RgbaImage, b: &::image::RgbaImage| {
            a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|c| (f32::from(p[c]) - f32::from(q[c])).abs()).sum::<f32>()).sum::<f32>() / (SIDE * SIDE) as f32
        };

        // (The GPU finds nothing wrong with the shaders and what they are given.)
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let base = draw(&with(Template::Pbr, &[]));
        let mut popped = std::pin::pin!(device.pop_error_scope());
        let error = match std::future::Future::poll(popped.as_mut(), &mut std::task::Context::from_waker(std::task::Waker::noop())) {
            std::task::Poll::Ready(e) => e,
            std::task::Poll::Pending => None,
        };
        assert!(error.is_none(), "{error:?}");
        // With BREWED_3D_PICTURES=<folder>, each picture is kept there to be looked at.
        let keep = |name: &str, img: &::image::RgbaImage| {
            if let Ok(dir) = std::env::var("BREWED_3D_PICTURES") {
                img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("saved");
            }
        };
        keep("base", &base);
        assert!(sphere(&base).len() > (SIDE * SIDE / 4) as usize, "the sphere is drawn");

        // The coat: a mirror-like highlight over the rough red metal.
        let coat = draw(&with(Template::Coat, &[]));
        let bare = draw(&with(Template::Coat, &[("coat_weight", gray(0.0))]));
        keep("coat", &coat);
        keep("coat-off", &bare);
        // (What it reflects is white, of the light and the sky; what the metal reflects, red.)
        let white = |img: &::image::RgbaImage| mean(&sphere(img), &|p| f32::from(p[0].min(p[1]).min(p[2])));
        assert!(white(&coat) > white(&bare) + 5.0, "the coat's reflections: {} against {}", white(&coat), white(&bare));
        let peak = |img: &::image::RgbaImage| img.pixels().map(lum).fold(0.0f32, f32::max);

        // The fuzz: brighter at the edges of the sphere.
        let rim = |img: &::image::RgbaImage| {
            let c = SIDE as f32 / 2.0;
            let edge: Vec<_> = sphere(img)
                .into_iter()
                .filter(|(x, y, _)| {
                    let d = ((*x as f32 + 0.5 - c).powi(2) + (*y as f32 + 0.5 - c).powi(2)).sqrt();
                    d > c * PICTURE_FILL * 0.8
                })
                .collect();
            mean(&edge, &lum)
        };
        let fuzz = draw(&with(Template::Fuzz, &[]));
        let no_fuzz = draw(&with(Template::Fuzz, &[("fuzz_weight", gray(0.0))]));
        keep("fuzz", &fuzz);
        keep("fuzz-off", &no_fuzz);
        assert!(rim(&fuzz) > rim(&no_fuzz) + 8.0, "the fuzz at the edges: {} over {}", rim(&fuzz), rim(&no_fuzz));

        // The anisotropy: its highlight is drawn out up and down (the template's tangent runs up
        // the texture), as far as it is across: the bright pixels about the brightest (not those
        // of the fill light's highlight, far off), how much higher than wide they reach.
        let tall = |img: &::image::RgbaImage| {
            let top = peak(img) * 0.85;
            let (px, py, _) = img.enumerate_pixels().max_by(|a, b| lum(a.2).total_cmp(&lum(b.2))).expect("pixels");
            let near = |x: u32, y: u32| (x.abs_diff(px).pow(2) + y.abs_diff(py).pow(2)) as f32 <= (SIDE as f32 / 4.0).powi(2);
            let bright: Vec<(u32, u32)> = img.enumerate_pixels().filter(|(x, y, p)| lum(p) >= top && near(*x, *y)).map(|(x, y, _)| (x, y)).collect();
            let w = bright.iter().map(|b| b.0).max().unwrap_or(0) - bright.iter().map(|b| b.0).min().unwrap_or(0) + 1;
            let h = bright.iter().map(|b| b.1).max().unwrap_or(0) - bright.iter().map(|b| b.1).min().unwrap_or(0) + 1;
            h as f32 / w as f32
        };
        let brushed = draw(&with(Template::Anisotropy, &[]));
        let round = draw(&with(Template::Anisotropy, &[("specular_roughness_anisotropy", gray(0.0))]));
        keep("anisotropy", &brushed);
        keep("anisotropy-off", &round);
        assert!(tall(&brushed) > tall(&round) * 1.4, "drawn out up and down: {} against {}", tall(&brushed), tall(&round));

        // Under the surface: light reaches into the shadow, red the farthest.
        let shade = |img: &::image::RgbaImage| {
            let dark: Vec<_> = sphere(img).into_iter().filter(|(x, _, _)| *x > SIDE * 2 / 3).collect();
            (mean(&dark, &lum), mean(&dark, &|p| f32::from(p[0]) - f32::from(p[2])))
        };
        let wax = draw(&with(Template::Subsurface, &[]));
        let plaster = draw(&with(Template::Subsurface, &[("subsurface_weight", gray(0.0))]));
        keep("subsurface", &wax);
        keep("subsurface-off", &plaster);
        assert!(shade(&wax).1 > shade(&plaster).1 + 3.0, "redder in the shadow: {:?} against {:?}", shade(&wax), shade(&plaster));

        // The rest, in Complete: each changes the picture.
        let complete = draw(&with(Template::Complete, &[]));
        for (what, change) in [
            ("an emission", vec![("emission_luminance", gray(1000.0)), ("emission_color", [1.0, 0.2, 0.1, 1.0])]),
            ("a thin film", vec![("thin_film_weight", gray(1.0)), ("base_metalness", gray(1.0))]),
            ("an opacity", vec![("opacity", gray(0.3))]),
            ("glass", vec![("transmission_weight", gray(1.0))]),
            ("a coat", vec![("coat_weight", gray(1.0))]),
            ("metal", vec![("base_metalness", gray(1.0))]),
        ] {
            let changed = draw(&with(Template::Complete, &change));
            keep(&format!("complete-{}", what.replace(' ', "-")), &changed);
            assert!(differ(&changed, &complete) > 4.0, "{what}: {}", differ(&changed, &complete));
        }
        // The thin film colors the reflection: more colorful than the metal without it.
        let colorful = |img: &::image::RgbaImage| mean(&sphere(img), &|p| f32::from(p[0].max(p[1]).max(p[2])) - f32::from(p[0].min(p[1]).min(p[2])));
        let film = draw(&with(Template::Complete, &[("thin_film_weight", gray(1.0)), ("base_metalness", gray(1.0))]));
        let silver = draw(&with(Template::Complete, &[("base_metalness", gray(1.0))]));
        assert!(colorful(&film) > colorful(&silver) + 5.0, "{} against {}", colorful(&film), colorful(&silver));
    }

    /// Every usage does on the model what it says: an Output of each usage the 3D view shows
    /// changes the picture of the sphere, given what it needs to be seen (a coat for the coat's
    /// roughness, glass for its color …), values under 0 too where a usage has them. One whose
    /// place another usage takes (a diffuse color where there is a base color …) changes
    /// nothing, nor one the view does not show (a mask, the maps of older engines, none), nor a
    /// panorama, which is no map of the material (it lights the view when asked).
    #[test]
    fn every_usage_does_on_the_model_what_it_says() {
        use brewed_texture_core::graph::{Graph, Precision};
        use brewed_texture_core::nodes::DEPTH_16F;
        use brewed_texture_core::types::ParamValue;
        let Ok(gpu) = brewed_texture_engine::Gpu::headless() else { return };
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let mut engine = brewed_texture_engine::Engine::new(gpu);
        let mut pipeline = None;
        const SIDE: u32 = 96;
        /// What an Output is fed: a value all over, or a pattern (a bump needs one).
        #[derive(Clone, Copy, Debug)]
        enum Feed {
            Value([f32; 4]),
            Noise,
        }
        use Feed::{Noise, Value};
        // An Output of each usage, fed what it is given, written in floats.
        let graph = |maps: &[(Usage, Feed)]| -> Graph {
            let mut g = Graph { resolution: [64, 64], precision: Precision::Half, ..Graph::default() };
            for (i, (usage, feed)) in maps.iter().enumerate() {
                let y = i as f32 * 240.0;
                let from = match feed {
                    Value(color) => {
                        let id = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, y]);
                        g.node_mut(id).and_then(|n| n.param_mut("color")).expect("its color").value = ParamValue::Color(*color);
                        id
                    }
                    Noise => g.add_node(NodeKind::NOISE, [0.0, y]),
                };
                let out = g.add_node(NodeKind::OUTPUT, [340.0, y]);
                for (key, value) in
                    [("name", ParamValue::Text(usage.id().into())), ("usage", ParamValue::Enum(usage.index())), ("bit_depth", ParamValue::Enum(DEPTH_16F))]
                {
                    g.node_mut(out).and_then(|n| n.param_mut(key)).expect(key).value = value;
                }
                assert!(g.connect(from, "o:output", out, "i:input").is_ok());
            }
            g
        };
        let mut draw = |maps: &[(Usage, Feed)]| -> ::image::RgbaImage {
            let g = graph(maps);
            let outputs: Vec<_> = g.nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            let _ = engine.prepare_materials(&[]);
            engine.set_materials(&outputs);
            brewed_texture_engine::engine::evaluate_blocking(&mut engine, &g);
            // (None where there are no maps at all, as there were none before.)
            let shown = engine.prepare_materials(&outputs).unwrap_or_default();
            let found: Vec<(Usage, &brewed_texture_engine::DisplayImage)> =
                shown.iter().filter_map(|img| Usage::of(g.node(img.node)?.enum_value("usage")).filter(|u| u.shown_in_3d()).map(|u| (u, img))).collect();
            let maps = shading::of(found.iter().map(|(u, img)| (*u, *img)));
            let camera = picture_camera(Camera { shape: Shape::Sphere, ..Camera::default() }, 1.0);
            let primitive = Primitive { camera, maps, environment: None, exposure: 1.0, background: None, key: false };
            render(&primitive, &mut pipeline, &device, &queue, [SIDE, SIDE]).expect("a picture")
        };
        let differ = |a: &::image::RgbaImage, b: &::image::RgbaImage| {
            a.pixels().zip(b.pixels()).map(|(p, q)| (0..3).map(|c| (f32::from(p[c]) - f32::from(q[c])).abs()).sum::<f32>()).sum::<f32>() / (SIDE * SIDE) as f32
        };
        let gray = |v: f32| Value([v, v, v, 1.0]);
        let red = Value([0.9, 0.12, 0.08, 1.0]);
        let green = Value([0.1, 0.8, 0.2, 1.0]);
        // A normal leaning right and up; a tangent up the texture.
        let leaning = Value([0.8, 0.65, 0.85, 1.0]);
        let up = Value([0.5, 1.0, 0.5, 1.0]);
        use Usage as U;
        // Each usage: what it needs to be seen, and what it is given.
        let glass = (U::TransmissionWeight, gray(1.0));
        let under = (U::SubsurfaceWeight, gray(1.0));
        let coat = (U::CoatWeight, gray(1.0));
        // (A usage, the maps it needs to be seen, what it is given.)
        type Case = (Usage, Vec<(Usage, Feed)>, Feed);
        let cases: Vec<Case> = vec![
            (U::BaseWeight, vec![], gray(0.2)),
            (U::BaseColor, vec![], red),
            (U::Metallic, vec![], gray(1.0)),
            (U::BaseDiffuseRoughness, vec![], gray(1.0)),
            (U::SpecularWeight, vec![], gray(0.0)),
            (U::SpecularColor, vec![], Value([0.1, 0.3, 1.0, 1.0])),
            // (Rough: a highlight sharper than a pixel of so small a sphere holds is widened to it.)
            (U::Roughness, vec![], gray(0.9)),
            (U::SpecularRoughnessAnisotropy, vec![(U::Roughness, gray(0.4))], gray(1.0)),
            // (Not all the way anisotropic: a highlight drawn out to a hairline is little turned.)
            (U::AnisotropyAngle, vec![(U::Roughness, gray(0.4)), (U::SpecularRoughnessAnisotropy, gray(0.7))], gray(0.25)),
            (U::SpecularIor, vec![], gray(3.0)),
            (U::TransmissionWeight, vec![], gray(1.0)),
            (U::TransmissionColor, vec![glass], red),
            (U::TransmissionDepth, vec![glass, (U::TransmissionColor, red)], gray(0.3)),
            (U::TransmissionScatter, vec![glass], gray(0.9)),
            (U::TransmissionScatterAnisotropy, vec![glass, (U::TransmissionScatter, gray(0.9))], gray(0.8)),
            (U::TransmissionScatterAnisotropy, vec![glass, (U::TransmissionScatter, gray(0.9))], gray(-0.8)),
            (U::TransmissionDispersionScale, vec![glass, (U::TransmissionDispersionAbbeNumber, gray(2.0))], gray(1.0)),
            (U::TransmissionDispersionAbbeNumber, vec![glass, (U::TransmissionDispersionScale, gray(1.0))], gray(2.0)),
            (U::SubsurfaceWeight, vec![], gray(1.0)),
            (U::SubsurfaceColor, vec![under], red),
            (U::SubsurfaceRadius, vec![under], gray(0.1)),
            (U::SubsurfaceRadiusScale, vec![under], Value([0.1, 1.0, 0.1, 1.0])),
            (U::SubsurfaceScatterAnisotropy, vec![under], gray(0.8)),
            (U::SubsurfaceScatterAnisotropy, vec![under], gray(-0.8)),
            (U::CoatWeight, vec![], gray(1.0)),
            (U::CoatColor, vec![coat], red),
            (U::CoatRoughness, vec![coat], gray(0.5)),
            (U::CoatRoughnessAnisotropy, vec![coat, (U::CoatRoughness, gray(0.4))], gray(1.0)),
            (U::CoatIor, vec![coat], gray(3.0)),
            (U::CoatDarkening, vec![coat, (U::BaseColor, red)], gray(0.0)),
            (U::CoatNormal, vec![coat], leaning),
            (U::CoatTangent, vec![coat, (U::CoatRoughness, gray(0.4)), (U::CoatRoughnessAnisotropy, gray(0.7))], up),
            (U::FuzzWeight, vec![], gray(1.0)),
            (U::FuzzColor, vec![(U::FuzzWeight, gray(1.0))], red),
            (U::FuzzRoughness, vec![(U::FuzzWeight, gray(1.0))], gray(0.1)),
            (U::EmissionLuminance, vec![], gray(1000.0)),
            (U::Emissive, vec![], red),
            (U::ThinFilmWeight, vec![(U::Metallic, gray(1.0))], gray(1.0)),
            (U::ThinFilmThickness, vec![(U::Metallic, gray(1.0)), (U::ThinFilmWeight, gray(1.0))], gray(0.25)),
            (U::ThinFilmIor, vec![(U::Metallic, gray(1.0)), (U::ThinFilmWeight, gray(1.0))], gray(2.0)),
            (U::Opacity, vec![], gray(0.3)),
            (U::ThinWalled, vec![glass], gray(1.0)),
            (U::Normal, vec![], leaning),
            (U::Tangent, vec![(U::Roughness, gray(0.4)), (U::SpecularRoughnessAnisotropy, gray(0.7))], up),
            (U::AmbientOcclusion, vec![], gray(0.2)),
            (U::Height, vec![], gray(1.0)),
            (U::Displacement, vec![], gray(1.0)),
            (U::Bump, vec![], Noise),
            (U::DetailNormal, vec![], leaning),
            (U::Diffuse, vec![], red),
            (U::Specular, vec![], Value([0.9, 0.8, 0.2, 1.0])),
            (U::Glossiness, vec![], gray(0.1)),
            (U::SpecularLevel, vec![], gray(0.0)),
            (U::Mask, vec![], red),
            (U::Ambient, vec![], red),
            (U::Reflection, vec![], red),
            (U::Refraction, vec![], red),
            (U::Custom, vec![], red),
            (U::Panorama, vec![], red),
        ];
        for u in Usage::ALL {
            assert!(cases.iter().any(|(c, ..)| *c == u), "{u:?} has no case");
        }
        // Where another usage takes its place: nothing.
        let overridden: Vec<Case> = vec![
            (U::Diffuse, vec![(U::BaseColor, green)], red),
            (U::Glossiness, vec![(U::Roughness, gray(0.3))], gray(0.95)),
            (U::SpecularLevel, vec![(U::SpecularWeight, gray(1.0))], gray(0.0)),
            (U::Displacement, vec![(U::Height, gray(0.5))], gray(1.0)),
        ];
        let keep = |name: &str, img: &::image::RgbaImage| {
            if let Ok(dir) = std::env::var("BREWED_3D_PICTURES") {
                img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("saved");
            }
        };
        let mut seen = Vec::new();
        for (shows, list) in [(true, &cases), (false, &overridden)] {
            for (usage, needs, given) in list {
                let without = draw(needs);
                let mut maps = needs.clone();
                maps.push((*usage, *given));
                let with = draw(&maps);
                keep(&format!("usage-{}-{}", usage.id(), seen.len()), &with);
                let d = differ(&with, &without);
                seen.push(format!("{usage:?} {given:?}: {d:.2}"));
                if shows && usage.shown_in_3d() && *usage != U::Panorama {
                    assert!(d > 1.0, "{usage:?} given {given:?} changes the picture: {d}");
                } else {
                    assert_eq!(d, 0.0, "{usage:?} given {given:?} changes nothing");
                }
            }
        }
        eprintln!("{}", seen.join("\n"));
    }

    /// The view's background where there is no model, as `render` puts it under the picture.
    fn draw_background(side: u32) -> ::image::RgbaImage {
        ::image::RgbaImage::from_fn(side, side, |_, y| {
            let back = (48.0 - 30.0 * y as f32 / side as f32).round() as u8;
            ::image::Rgba([back, back, back, 255])
        })
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
        assert!(
            (wrapped(3.0 * std::f32::consts::PI) - std::f32::consts::PI).abs() < 1e-4
                || (wrapped(3.0 * std::f32::consts::PI) + std::f32::consts::PI).abs() < 1e-4
        );
        assert_eq!(wrapped(f32::NAN), 0.0);
    }

    /// The camera turns round the model round and round as well, looking where it would have
    /// looked: its angle stays within a turn, never so far that a turn is lost in the digits.
    #[test]
    fn the_camera_turns_round_and_round() {
        let (mut cam, mut turned) = (Camera::default(), 0.0f64);
        for _ in 0..100_000 {
            cam = cam.moved(Move::Orbit(Vector::new(-97.0, 0.0)));
            turned += 0.97;
        }
        assert!((-std::f32::consts::PI..=std::f32::consts::PI).contains(&cam.yaw), "{}", cam.yaw);
        let expected = (turned + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let off = (f64::from(cam.yaw) - expected).abs();
        assert!(off.min(std::f64::consts::TAU - off) < 1e-2, "{} where {expected} was turned to", cam.yaw);
    }

    /// The studio has one light, as the view says: one highlight on a glossy sphere (a second
    /// light opposite it made a second), and across a rounded edge of the cube, from the side
    /// the light is on to a side it is not, nothing darker than that side (between the sides
    /// the two lights lit ran a dark line). The edges of the model are smooth, drawn with
    /// several samples a pixel (they were steps), and so is the light on a rounded edge a pixel
    /// or two wide: no pixel far brighter than all those around it (it was a broken line of
    /// sparks). With BREWED_3D_PICTURES=<folder>, the pictures are kept there to be looked at.
    #[test]
    fn the_studio_has_one_light_and_the_edges_are_smooth() {
        use brewed_texture_core::graph::Graph;
        use brewed_texture_core::templates::Template;
        use brewed_texture_core::types::ParamValue;
        let Ok(gpu) = brewed_texture_engine::Gpu::headless() else { return };
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let mut engine = brewed_texture_engine::Engine::new(gpu);
        let mut pipeline = None;
        const SIZE: [u32; 2] = [450, 350];
        let mut draw = |g: &Graph, camera: Camera| -> ::image::RgbaImage {
            let outputs: Vec<_> = g.nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            let _ = engine.prepare_materials(&[]);
            engine.set_materials(&outputs);
            brewed_texture_engine::engine::evaluate_blocking(&mut engine, g);
            let shown = engine.prepare_materials(&outputs).expect("the maps");
            let found: Vec<(Usage, &brewed_texture_engine::DisplayImage)> =
                shown.iter().filter_map(|img| Usage::of(g.node(img.node)?.enum_value("usage")).filter(|u| u.shown_in_3d()).map(|u| (u, img))).collect();
            let maps = shading::of(found.iter().map(|(u, img)| (*u, *img)));
            let primitive = Primitive { camera, maps, environment: None, exposure: 1.0, background: None, key: false };
            render(&primitive, &mut pipeline, &device, &queue, SIZE).expect("a picture")
        };
        // The template of a plain material, its Output named `name` fed `value` instead.
        let with = |changes: &[(&str, f32)]| -> Graph {
            let mut g = Template::Pbr.graph([64, 64], brewed_texture_core::graph::Precision::Half);
            for (name, value) in changes {
                let out = g.nodes_of(NodeKind::OUTPUT).find(|n| n.text("name") == *name).map(|n| n.id).expect(name);
                let uniform = g.links.iter().find(|l| l.to == out).map(|l| l.from).expect("fed by a uniform color");
                let node = g.node_mut(uniform).expect("the node");
                match node.kind == NodeKind::UNIFORM_VALUE {
                    true => node.param_mut("value").expect("its value").value = ParamValue::Float(*value),
                    false => node.param_mut("color").expect("its color").value = ParamValue::Color([*value, *value, *value, 1.0]),
                }
            }
            g
        };
        let keep = |name: &str, img: &::image::RgbaImage| {
            if let Ok(dir) = std::env::var("BREWED_3D_PICTURES") {
                img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).expect("saved");
            }
        };
        let lum = |p: &::image::Rgba<u8>| 0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]);
        let turned = Camera { yaw: 0.6, pitch: 0.35, displacement: 0.0, ..Camera::default() };

        // One highlight: of a dark glossy sphere, what is as bright as a light on it is all about
        // its brightest point (the sky on its rim is not).
        let sphere = draw(&with(&[("base_color", 0.05), ("specular_roughness", 0.15)]), Camera { shape: Shape::Sphere, ..turned });
        keep("one-light-sphere", &sphere);
        let (px, py, peak) = sphere.enumerate_pixels().max_by(|a, b| lum(a.2).total_cmp(&lum(b.2))).map(|(x, y, p)| (x, y, lum(p))).expect("pixels");
        assert!(peak > 120.0, "a highlight: {peak}");
        let far: Vec<(u32, u32)> = sphere
            .enumerate_pixels()
            .filter(|(x, y, p)| lum(p) > 0.6 * peak && (x.abs_diff(px).pow(2) + y.abs_diff(py).pow(2)) > 70u32.pow(2))
            .map(|(x, y, _)| (x, y))
            .collect();
        assert!(far.is_empty(), "a second highlight, away from the one at {px},{py}: {:?}", far.first());

        // Across a rounded edge of the cube, from the lit side to the other: nothing darker than
        // the other side.
        let cube = draw(&with(&[("specular_roughness", 0.4)]), turned);
        keep("one-light-cube", &cube);
        let row: Vec<f32> = (SIZE[0] * 3 / 10..SIZE[0] * 3 / 4).map(|x| lum(cube.get_pixel(x, SIZE[1] / 2))).collect();
        let (lit, other) = (row[..10].iter().sum::<f32>() / 10.0, row[row.len() - 10..].iter().sum::<f32>() / 10.0);
        assert!(lit > other + 40.0, "a side the light is on, and one it is not: {lit}, {other}");
        let darkest = row.iter().copied().fold(f32::MAX, f32::min);
        assert!(darkest > other - 4.0, "a dark line along the edge: {darkest} under {other}\n{row:?}");

        // No sparks: no pixel of a glossy cube far brighter than all those around it.
        let glossy = draw(&with(&[("specular_roughness", 0.15)]), turned);
        keep("glossy-cube", &glossy);
        for (x, y, p) in glossy.enumerate_pixels().filter(|(x, y, _)| *x > 0 && *y > 0 && *x + 1 < SIZE[0] && *y + 1 < SIZE[1]) {
            let around = (0..9u32).filter(|k| *k != 4).map(|k| lum(glossy.get_pixel(x + k % 3 - 1, y + k / 3 - 1))).fold(0.0f32, f32::max);
            assert!(lum(p) < around + 60.0, "a spark at {x},{y}: {} among {around}", lum(p));
        }

        // Smooth edges: where the cube, white, ends on the dark behind it, pixels in between.
        let white = draw(&with(&[("base_color", 1.0), ("specular_roughness", 1.0)]), Camera { light_yaw: 0.6, light_pitch: 0.5, ..turned });
        keep("white-cube", &white);
        let mut between = 0;
        for y in SIZE[1] / 4..SIZE[1] * 3 / 4 {
            // (From the left, the first pixels that are not the background's.)
            let back = lum(white.get_pixel(0, y));
            let Some(edge) = (0..SIZE[0]).find(|x| lum(white.get_pixel(*x, y)) > back + 6.0) else { continue };
            let inside = lum(white.get_pixel(edge + 6, y));
            if (edge..edge + 3).any(|x| (back + 0.2 * (inside - back)..back + 0.8 * (inside - back)).contains(&lum(white.get_pixel(x, y)))) {
                between += 1;
            }
        }
        assert!(between > (SIZE[1] / 2) as usize / 3, "rows whose edge has a pixel in between: {between} of {}", SIZE[1] / 2);
    }

    /// The view is drawn the same on a screen of any scale (100 %, 125 %, 150 %, 175 %, 200 %,
    /// 300 %: a window moved from one screen to another of several is redrawn at the other's),
    /// wherever its edges fall between the pixels: on every pixel the framework gives it and on
    /// no other (ended a pixel short, a line of what is under the view showed along an edge),
    /// the model as large a part of it and as round, and drawn in the pixels of that screen
    /// (twice as many across at twice the scale, not a smaller picture stretched).
    #[test]
    fn the_view_is_drawn_at_any_scale_of_the_screen() {
        let Ok(gpu) = brewed_texture_engine::Gpu::headless() else { return };
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let mut pipeline = None;
        let of = |camera: Camera| Primitive { camera, maps: Vec::new(), environment: None, exposure: 1.0, background: None, key: false };
        // A plane so near that it is all the view shows; and a sphere, whole in it.
        let near = of(Camera { shape: Shape::Plane, yaw: 0.0, pitch: 0.0, distance: 1.0, displacement: 0.0, ..Camera::default() });
        let sphere = of(Camera { shape: Shape::Sphere, displacement: 0.0, ..Camera::default() });
        // Where a view is in a window of 640 by 480 points: its edges between the points, as
        // the panels' are once they are dragged.
        let views = [
            Rectangle::new(iced::Point::new(37.3, 21.7), iced::Size::new(301.4, 222.2)),
            Rectangle::new(iced::Point::new(0.0, 30.0), iced::Size::new(413.5, 317.25)),
        ];
        for bounds in views {
            let mut at_one: Option<[f32; 4]> = None;
            for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0, 3.0] {
                let size = [(640.0 * scale).round() as u32, (480.0 * scale).round() as u32];
                let window = Rectangle::new(iced::Point::ORIGIN, iced::Size::new(size[0] as f32, size[1] as f32));
                let given = (bounds * scale).intersection(&window).and_then(Rectangle::snap).expect("the view's pixels");
                let within = |x: u32, y: u32| x >= given.x && y >= given.y && x < given.x + given.width && y < given.y + given.height;

                let filled = render_in(&near, &mut pipeline, &device, &queue, size, bounds, scale).expect("a picture");
                for (x, y, px) in filled.enumerate_pixels() {
                    match within(x, y) {
                        true => assert_eq!(px[3], 255, "at {scale}: the view's pixel {x},{y} is not drawn (the view has {given:?})"),
                        false => assert_eq!(px.0, [0; 4], "at {scale}: {x},{y} is drawn on, outside the view ({given:?})"),
                    }
                }

                let round = render_in(&sphere, &mut pipeline, &device, &queue, size, bounds, scale).expect("a picture");
                let drawn = || round.enumerate_pixels().filter(|(_, _, px)| px[3] > 127);
                assert!(drawn().all(|(x, y, _)| within(x, y)), "at {scale}: the model is drawn outside the view");
                let (left, right) = (drawn().map(|p| p.0).min().expect("the model"), drawn().map(|p| p.0).max().expect("the model") + 1);
                let (top, bottom) = (drawn().map(|p| p.1).min().expect("the model"), drawn().map(|p| p.1).max().expect("the model") + 1);
                let (wide, high) = ((right - left) as f32, (bottom - top) as f32);
                assert!((wide - high).abs() <= 1.0, "at {scale}: the sphere is {wide} by {high} pixels");
                // In points of the view: where the model is, and how large.
                let shown = [(left - given.x) as f32 / scale, (top - given.y) as f32 / scale, wide / scale, high / scale];
                match at_one {
                    None => at_one = Some(shown),
                    Some(first) => {
                        for (now, then) in shown.iter().zip(first) {
                            // (Each edge, of the view and of the model, is at a whole pixel: of half
                            // a pixel's doubt at either scale.)
                            assert!((now - then).abs() <= 2.0, "at {scale}: the model is at {shown:?} of the view, and at {first:?} at 100 %");
                        }
                    }
                }
                // In the screen's pixels: its edge is a pixel or two of the screen wide at any
                // scale (a picture of fewer pixels stretched has an edge as many times wider).
                let y = (top + bottom) / 2;
                let edge = (given.x..left + (right - left) / 2).filter(|x| (16..240).contains(&round.get_pixel(*x, y)[3])).count();
                assert!(edge <= 2, "at {scale}: the model's edge is {edge} pixels wide");
            }
        }
    }

    /// How much of a square view a model fills as the view starts, in its longer way across.
    const START_FILL: f32 = 0.86;

    /// What [`Camera::start`] is to say of `shape`, measured from the model: how high on its
    /// axis it is looked at for it to be in the middle of the view, and from how far for it to
    /// fill [`START_FILL`] of a square one.
    fn measured_start(shape: Shape) -> (f32, f32) {
        let mut camera = Camera::start(shape);
        let (vertices, _) = build_mesh(shape);
        // (Perspective is not linear: a few steps bring it there.)
        for _ in 0..16 {
            let ([lo, hi], ..) = projected(&camera, 1.0, vertices.iter().map(|v| v.pos));
            let half = ((hi[0] - lo[0]) / 2.0).max((hi[1] - lo[1]) / 2.0).max(1e-3);
            camera.target[1] += (lo[1] + hi[1]) / 2.0 * camera.distance * half_height(1.0) / camera.pitch.cos();
            camera.distance *= half / START_FILL;
        }
        (camera.target[1], camera.distance)
    }

    /// As the view starts, every model is whole in it and in its middle, filling as much of a
    /// square view as the others do; whole in a view of any other shape too, wide or narrow;
    /// and looked at on its own axis (turned, it does not swing). The camera of a model picked
    /// in a view not moved yet is that model's; of one picked in a view moved off or neared,
    /// the one it was.
    #[test]
    fn every_model_is_whole_in_the_view_as_it_starts() {
        // (All that are not, said at once: with what each is to start at.)
        let mut off = Vec::new();
        for shape in Shape::ALL {
            let camera = Camera::start(shape);
            assert_eq!((camera.shape, camera.target[0], camera.target[2]), (shape, 0.0, 0.0), "{shape:?}: on its axis");
            let points: Vec<[f32; 3]> = build_mesh(shape).0.iter().map(|v| v.pos).collect();
            let ([lo, hi], _) = projected(&camera, 1.0, points.iter().copied());
            let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let half = ((hi[0] - lo[0]) / 2.0).max((hi[1] - lo[1]) / 2.0);
            let (height, distance) = measured_start(shape);
            if middle[0].abs() > 0.02 || middle[1].abs() > 0.01 || (half - START_FILL).abs() > 0.01 {
                off.push(format!("{shape:?}: its middle is at {middle:?} and it fills {half}; measured, it is to start at ({height:.3}, {distance:.3})"));
            }
            for aspect in [0.4, 0.75, 1.0, 1.6, 3.0] {
                let ([lo, hi], _) = projected(&camera, aspect, points.iter().copied());
                if lo[0] < -0.9 || hi[0] > 0.9 || lo[1] < -0.9 || hi[1] > 0.9 {
                    off.push(format!("{shape:?} in a view {aspect} wide for its height: from {lo:?} to {hi:?}"));
                }
            }
        }
        assert!(off.is_empty(), "{}", off.join("; "));
        // Another model picked: its own start, where the view was only turned.
        let turned = Camera { yaw: 1.0, pitch: 0.6, light_yaw: 2.0, ..Camera::start(Shape::RoundedCube) };
        let (picked, start) = (turned.with_shape(Shape::Cylinder), Camera::start(Shape::Cylinder));
        assert_eq!((picked.shape, picked.target, picked.distance), (Shape::Cylinder, start.target, start.distance));
        assert_eq!((picked.yaw, picked.pitch, picked.light_yaw), (1.0, 0.6, 2.0), "turned as it was");
        assert_ne!((start.target, start.distance), (turned.target, turned.distance), "the models start apart");
        // ... and the view as it was put, where it was moved off or neared.
        for moved in [turned.moved(Move::Zoom(0.7)), turned.moved(Move::Pan(Vector::new(0.3, 0.1)))] {
            let picked = moved.with_shape(Shape::Cylinder);
            assert_eq!(picked, Camera { shape: Shape::Cylinder, ..moved });
        }
        // The same model picked again changes nothing.
        assert_eq!(turned.with_shape(Shape::RoundedCube), turned);
    }
}
