//! Tools for developing the program, which users do not get (they are built with the
//! `dev-tools` feature, and for the tests): screenshots of the window in a state asked for
//! (`--screenshot`, `--scenario`, `--select`, `--preview`), and the autopilot, which drives the
//! app by itself (`--autopilot`, see [`crate::autopilot`]).

use std::path::PathBuf;
use std::time::Duration;

use iced::{Point, Subscription, Task, window};

use brewed_texture_core::graph::{Graph, NodeId, SocketRef};
use brewed_texture_core::{Edit, NodeKind, ParamValue};

use super::{AddMenu, App, ColorTarget, ContextMenu, ContextTarget, Dialog, MenuId, Message, PendingAction, VarField};
use crate::view;

/// What the tools are asked for on the command line.
#[derive(Clone, Debug, Default)]
pub struct DevOptions {
    /// Save a screenshot once everything is computed, then quit.
    pub screenshot: Option<PathBuf>,
    /// Save the picture of the model, as the package's picture is made (see
    /// `view3d::picture`), once everything is computed, then quit.
    pub picture: Option<PathBuf>,
    /// How large the picture is, wide and high (512 square by default).
    pub picture_size: Option<[u32; 2]>,
    /// The picture in the light of the 3D view (its environment image, behind the model where
    /// the view shows it there), not the studio's a package's picture has.
    pub picture_lit: bool,
    /// Export every output of the graph into this folder once everything is computed, as File
    /// → Export writes them, then quit (or take the screenshot or the picture).
    pub export: Option<PathBuf>,
    /// UI state to show in the screenshot.
    pub scenario: Option<String>,
    /// Select the first node of this type at startup.
    pub select: Option<String>,
    /// Show the first node of this type in the 2D view.
    pub preview: Option<String>,
    /// Drive the app by itself, to find what breaks (see [`crate::autopilot`]).
    pub autopilot: Option<crate::autopilot::Flight>,
}

/// What the tools keep.
#[derive(Default)]
pub struct Dev {
    screenshot_requested: bool,
    /// The frames of the window drawn since all a screenshot waits for arrived (none: it has
    /// not yet).
    drawn: Option<u32>,
    /// The export of `--export` was asked for.
    exported: bool,
    /// The autopilot, when the app drives itself.
    pub pilot: Option<crate::autopilot::Autopilot>,
}

impl Dev {
    pub fn new(options: &DevOptions) -> Dev {
        let pilot = options.autopilot.and_then(|flight| {
            let pilot = crate::autopilot::Autopilot::new(flight);
            pilot.map_err(|e| log::error!("the autopilot cannot start: {e}")).ok()
        });
        Dev { screenshot_requested: false, drawn: None, exported: false, pilot }
    }
}

impl App {
    /// The node asked to be selected and the one asked to be shown, at the start.
    pub(super) fn select_at_start(&mut self) {
        let graph = self.doc.graph();
        let select = self.opts.dev.select.as_deref().and_then(|k| find_kind(graph, k));
        let preview = self.opts.dev.preview.as_deref().and_then(|k| find_kind(graph, k));
        if let Some(id) = select {
            self.ui.selection.insert(id);
        }
        if let Some(id) = preview {
            self.ui.preview.node = Some(id);
        }
    }

    /// The state asked to be shown, at the start.
    pub(super) fn scenario_at_start(&mut self) -> Task<Message> {
        match self.opts.dev.scenario.clone() {
            Some(s) => self.apply_scenario(&s),
            None => Task::none(),
        }
    }

    /// Are the packages being saved written, those being opened read?
    pub fn files_idle(&self) -> bool {
        self.ui.saving.is_empty() && self.ui.opening.is_empty()
    }

    /// Has the engine done all it was asked, every export too?
    pub fn engine_idle(&self) -> bool {
        self.engine.is_some()
            && self.res.busy_since.is_none()
            && self.res.finished_revision >= self.eval_revision
            && self.res.exports_pending == 0
    }

    /// Ticks to see when a screenshot can be taken, and the autopilot's turns.
    pub(super) fn dev_subscriptions(&self, subs: &mut Vec<Subscription<Message>>) {
        let dev = &self.opts.dev;
        if ((dev.screenshot.is_some() || dev.picture.is_some()) && self.ui.status.is_none()) || dev.export.is_some() {
            subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        if self.dev.pilot.as_ref().is_some_and(|p| !p.is_done()) {
            subs.push(iced::time::every(Duration::from_millis(16)).map(|_| Message::Autopilot));
        }
        // The frames of the window, while a screenshot waits for one of what arrived last.
        if self.dev.drawn.is_some() && !self.dev.screenshot_requested {
            subs.push(window::frames().map(|_| Message::Drawn));
        }
    }

    /// A frame of the window is drawn (see `maybe_screenshot`).
    pub(super) fn drawn(&mut self) -> Task<Message> {
        if let Some(frames) = &mut self.dev.drawn {
            *frames += 1;
        }
        self.maybe_screenshot()
    }

    /// The autopilot's turn: what it does now, as the interface does it; once it has done
    /// all it does, the app quits.
    pub(super) fn fly(&mut self) -> Task<Message> {
        use crate::autopilot::{Turn, WindowChange};
        let Some(mut pilot) = self.dev.pilot.take() else { return Task::none() };
        let turn = pilot.tick(self);
        let done = pilot.is_done();
        self.dev.pilot = Some(pilot);
        if done {
            return self.hide_and_exit();
        }
        match (turn, self.window) {
            (Some(Turn::Send(message)), _) => self.update(message),
            (Some(Turn::Window(WindowChange::Resize(size))), Some(id)) => window::resize(id, size),
            (Some(Turn::Window(WindowChange::Maximize(on))), Some(id)) => window::maximize(id, on),
            _ => Task::none(),
        }
    }

    fn apply_scenario(&mut self, scenario: &str) -> Task<Message> {
        // Several at once: split first, before one of them (an example's id) takes the rest
        // as its own.
        if scenario.contains(';') {
            return Task::batch(scenario.split(';').map(|s| self.apply_scenario(s)).collect::<Vec<_>>());
        }
        let graph = self.doc.graph().clone();
        let first = |kind: NodeKind| graph.nodes_of(kind).next().map(|n| n.id);
        match scenario {
            "file-menu" => self.ui.menu = Some(MenuId::File),
            // The Home screen (the demo behind it), its Learn page, and About over it (which
            // names the GPU: the engine started on the Home screen).
            "home" => self.ui.home = Some(super::home::HomePage::Home),
            "learn" => self.ui.home = Some(super::home::HomePage::Learn),
            other if other.starts_with("example:") => {
                match brewed_texture_core::templates::Example::from_id(&other["example:".len()..]) {
                    Some(example) => return self.home_message(super::home::HomeMessage::Example(example)),
                    None => log::warn!("unknown example '{other}'"),
                }
            }
            "home-about" => {
                self.ui.home = Some(super::home::HomePage::Home);
                self.ui.dialog = Some(Dialog::About);
            }
            "add-menu" => {
                self.ui.add_menu = Some(AddMenu {
                    at: Point::new(700.0, 300.0),
                    graph_pos: [0.0, 0.0],
                    query: "bl".into(),
                    selected: 0,
                    scroll: 0.0,
                    pending: None,
                });
            }
            "context-menu" => {
                if let Some(id) = first(NodeKind::LEVELS) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.context = Some(ContextMenu { at: Point::new(640.0, 360.0), target: ContextTarget::Node(id) });
                }
            }
            "gradient" => {
                if let Some(id) = first(NodeKind::GRADIENT_MAP) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                    self.ui.gradient_stop.insert((id, "gradient".into()), 1);
                    self.ui.color_picker = Some(ColorTarget::GradientStop(id, "gradient".into()));
                }
            }
            "math" => {
                if let Some(id) = first(NodeKind::MATH) {
                    self.ui.selection = [id].into_iter().collect();
                }
            }
            "levels" => {
                if let Some(id) = first(NodeKind::LEVELS) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "inputs" => {
                self.ui.expanded_vars.insert("mortar".into());
                self.ui.expanded_vars.insert("tint".into());
                // (The Color Editor in its window, the tint's swatch shown open.)
                self.ui.color_picker = Some(ColorTarget::Variable("tint".into(), VarField::Value));
            }
            "transform" => {
                // A Transform 2D of the noise, rotated so its gizmo shows.
                if let Some(noise) = first(NodeKind::NOISE)
                    && let Ok(done) = self.doc.apply(Edit::AddNode { kind: NodeKind::TRANSFORM_2D, pos: [160.0, 520.0] })
                    && let Some(&id) = done.created.first()
                {
                    let _ = self.doc.apply(Edit::Connect {
                        a: SocketRef { node: noise, key: "o:output".into(), output: true },
                        b: SocketRef { node: id, key: "i:input".into(), output: false },
                    });
                    let _ = self.doc.apply(Edit::SetParam { node: id, key: "rotation".into(), value: ParamValue::Float(30.0) });
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            // A parameter of the first node of a kind (by its id or name), as the properties would
            // set it: param:fx_map.depth=2, param:noise.type=weave, param:uniform_color.color=1,0,0.
            other if other.starts_with("param:") => {
                let set = other["param:".len()..].split_once('=').and_then(|(target, value)| {
                    let (kind, key) = target.split_once('.')?;
                    let id = find_kind(&graph, kind)?;
                    Some((id, key.to_string(), param_value(graph.node(id)?.def().param(key)?, value)?))
                });
                match set {
                    Some((node, key, value)) => {
                        let _ = self.doc.apply(Edit::SetParam { node, key, value });
                    }
                    None => log::warn!("{other}: no such node, parameter or value"),
                }
            }
            // A node of the graph (as a template made it) chosen and shown: its gizmo, if it has one.
            other if other.starts_with("show:") => {
                if let Some(id) = find_kind(&graph, &other["show:".len()..]) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "tiling" => {
                // The wall repeated: no seams.
                if let Some(id) = first(NodeKind::OUTPUT) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
                self.ui.preview.show_tiling = true;
            }
            "choices" => {
                if let Some(id) = first(NodeKind::NOISE) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                    let _ = self.open_choice_menu(id, "type");
                }
            }
            // The menu of an Output's usage, under its headings.
            "usage" => {
                if let Some(id) = first(NodeKind::OUTPUT) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                    let _ = self.open_choice_menu(id, "usage");
                }
            }
            "dialog" => {
                let _ = self.doc.apply(Edit::SetResolution([1024, 1024]));
                self.ui.dialog = Some(Dialog::Unsaved(PendingAction::Quit));
            }
            // The other windows, as the menus open them.
            "settings" => return self.handle(Message::Settings(super::SettingsMessage::Open)),
            other if other.starts_with("settings:") => {
                let name = &other["settings:".len()..];
                match crate::settings::Page::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name)) {
                    Some(page) => return self.handle(Message::Settings(super::SettingsMessage::OpenPage(page))),
                    None => log::warn!("no page '{name}' in the settings"),
                }
            }
            // A graph of the package being edited, by its name.
            other if other.starts_with("graph:") => {
                let name = &other["graph:".len()..];
                match self.doc.package().graphs.iter().find(|g| g.name == name).map(|g| g.id) {
                    Some(id) => self.activate(self.doc_key, Some(id)),
                    None => log::warn!("no graph '{name}'"),
                }
            }
            // A picture for an image input of the graph: picture:NAME=FILE.
            other if other.starts_with("picture:") => {
                if let Some((name, file)) = other["picture:".len()..].split_once('=') {
                    self.apply(Edit::SetVariablePicture { name: name.into(), picture: file.into(), default: false });
                }
            }
            // The value of the uniform color that feeds the Output called NAME: uniform:NAME=V or
            // uniform:NAME=R,G,B.
            other if other.starts_with("uniform:") => {
                let said = other["uniform:".len()..].split_once('=').map(|(name, v)| (name.to_string(), v.split(',').filter_map(|c| c.trim().parse::<f32>().ok()).collect::<Vec<_>>()));
                let graph = self.graph();
                let output = said.as_ref().and_then(|(name, _)| graph.nodes_of(NodeKind::OUTPUT).find(|n| n.text("name") == name).map(|n| n.id));
                let fed = output.and_then(|o| graph.link_into(o, "i:input")).map(|l| l.from);
                match (fed, said) {
                    // (A number where the map is fed one: a Uniform Value.)
                    (Some(node), Some((_, v))) if !v.is_empty() && graph.node(node).is_some_and(|n| n.kind == NodeKind::UNIFORM_VALUE) => {
                        self.apply(Edit::SetParam { node, key: "value".into(), value: ParamValue::Float(v[0]) });
                    }
                    (Some(node), Some((_, v))) if !v.is_empty() => {
                        let color = if v.len() >= 3 { [v[0], v[1], v[2], 1.0] } else { [v[0], v[0], v[0], 1.0] };
                        self.apply(Edit::SetParam { node, key: "color".into(), value: ParamValue::Color(color) });
                    }
                    _ => log::warn!("no uniform value in '{other}'"),
                }
            }
            // The texture size of the graph, as square: size:N.
            other if other.starts_with("size:") => match other["size:".len()..].parse::<u32>() {
                Ok(side) => {
                    self.apply(Edit::SetResolution([side, side]));
                }
                Err(e) => log::warn!("size: {e}"),
            },
            // A graph made of a template, as the Home screen's row makes it.
            other if other.starts_with("template:") => {
                let id = other["template:".len()..].to_string();
                return self.home_message(super::home::HomeMessage::Screen(home_screen::Event::Start(id)));
            }
            "shortcuts" => self.ui.dialog = Some(Dialog::Shortcuts),
            "about" => self.ui.dialog = Some(Dialog::About),
            "notices" => self.ui.dialog = Some(Dialog::Notices(Default::default())),
            "new-graph" => return self.handle(Message::NewGraph(super::home::NewGraphMessage::Open(None))),
            "export" => return self.handle(Message::Export(super::export_dialog::ExportMessage::Open)),
            "versions" => return self.handle(Message::Versions(super::versions::VersionsMessage::Open)),
            "tile" => {
                if let Some(id) = first(NodeKind::TILE_GENERATOR) {
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            "lighting-menu" => self.ui.preview.lighting.menu = true,
            "tile-sampler" => {
                // Stones: discs of every size and turn, kept and colored where the noise says.
                if let Some(noise) = first(NodeKind::NOISE)
                    && let Ok(done) = self.doc.apply(Edit::AddNode { kind: NodeKind::TILE_SAMPLER, pos: [160.0, 520.0] })
                    && let Some(&id) = done.created.first()
                {
                    for socket in ["i:mask_map", "i:rotation_map", "i:color_map"] {
                        let _ = self.doc.apply(Edit::Connect {
                            a: SocketRef { node: noise, key: "o:output".into(), output: true },
                            b: SocketRef { node: id, key: socket.into(), output: false },
                        });
                    }
                    for (key, value) in [
                        ("pattern", ParamValue::Enum(8)),
                        ("x_amount", ParamValue::Int(12)),
                        ("y_amount", ParamValue::Int(12)),
                        ("scale", ParamValue::Float(1.3)),
                        ("random_scale", ParamValue::Float(0.5)),
                        ("random_position", ParamValue::Float(0.6)),
                        ("random_luminance", ParamValue::Float(0.4)),
                        ("mask_threshold", ParamValue::Float(0.35)),
                        ("color", ParamValue::Enum(1)),
                        ("blend_mode", ParamValue::Enum(2)),
                    ] {
                        let _ = self.doc.apply(Edit::SetParam { node: id, key: key.into(), value });
                    }
                    self.ui.selection = [id].into_iter().collect();
                    self.ui.preview.node = Some(id);
                }
            }
            // The model of the 3D view, by its name ("mat-ball").
            other if other.starts_with("shape:") => {
                let name = other["shape:".len()..].replace('-', " ");
                match view::view3d::Shape::ALL.into_iter().find(|s| s.to_string().eq_ignore_ascii_case(&name)) {
                    Some(shape) => {
                        let camera = self.ui.preview.view3d;
                        return self.handle(Message::Preview(super::PreviewMessage::Camera(camera.with_shape(shape))));
                    }
                    None => log::warn!("no shape '{name}'"),
                }
            }
            // The 3D view lit by an environment image, as bright and as blurred behind the model
            // as said.
            other if other.starts_with("environment:") => return self.choose_environment(Some(other["environment:".len()..].to_string())),
            // The light the pictures of the templates' materials are made in (see `preview_light`).
            // (light:preview-right: the same light from the right of the camera.)
            "light:preview" | "light:preview-right" => {
                let (yaw, pitch) = PREVIEW_KEY;
                let yaw = if scenario.ends_with("-right") { -yaw } else { yaw };
                let camera = &mut self.ui.preview.view3d;
                (camera.light_yaw, camera.light_pitch) = (yaw, pitch);
                let l = &mut self.ui.preview.lighting;
                (l.file, l.shown, l.key) = (None, Some(std::sync::Arc::new(preview_light(yaw, pitch))), true);
            }
            other if other.starts_with("exposure:") => self.ui.preview.lighting.exposure = other["exposure:".len()..].parse().unwrap_or(0.0),
            other if other.starts_with("blur:") => self.ui.preview.lighting.blur = other["blur:".len()..].parse().unwrap_or(0.25),
            other => log::warn!("unknown scenario '{other}'"),
        }
        Task::none()
    }

    pub(super) fn maybe_screenshot(&mut self) -> Task<Message> {
        let dev = &self.opts.dev;
        if (dev.screenshot.is_none() && dev.picture.is_none() && dev.export.is_none()) || self.dev.screenshot_requested {
            return Task::none();
        }
        let settled = self.res.evaluated_revision == self.eval_revision
            && self.started.elapsed() > Duration::from_millis(2500)
            && (self.ui.preview.node.is_none() || self.res.preview.is_some())
            // The model wears the maps in every screenshot: they take longest to arrive.
            && self.res.materials.len() >= self.material_nodes().len()
            // ...and is lit by its environment image.
            && self.ui.preview.lighting.reading.is_none()
            // An open menu of pictures shows all of them (one of names has none to wait for).
            && self.ui.choice_menu.as_ref().is_none_or(|m| {
                self.graph().node(m.node).is_some_and(|n| {
                    n.def().param(m.key).is_none_or(|pd| !pd.pictured) || self.res.choices.contains_key(&(n.kind.id(), m.key))
                })
            });
        // (A machine that draws without a GPU — a virtual one — is waited for as long as
        // BREWED_SCREENSHOT_WAIT says, in seconds.)
        let patience = Duration::from_secs(std::env::var("BREWED_SCREENSHOT_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(30));
        if !settled || self.started.elapsed() < Duration::from_secs(3) {
            if self.started.elapsed() > patience {
                log::error!("screenshot: gave up waiting for the engine");
                return iced::exit();
            }
            return Task::none();
        }
        // The outputs exported first, and written whole.
        if let Some(folder) = self.opts.dev.export.clone() {
            if !self.dev.exported {
                self.dev.exported = true;
                let _ = std::fs::create_dir_all(&folder);
                return self.handle(Message::ExportFolderPicked(Some(folder)));
            }
            if self.res.exports_pending > 0 {
                return Task::none();
            }
            if self.opts.dev.screenshot.is_none() && self.opts.dev.picture.is_none() {
                self.dev.screenshot_requested = true;
                println!("exported to {}", folder.display());
                return iced::exit();
            }
        }
        if self.opts.dev.picture.is_none() && !self.shown_for_a_screenshot() {
            // (A window that is not drawn — hidden, its screen locked or dark — is waited for
            // no longer than the engine is: the screenshot is then of what it showed last.)
            if self.started.elapsed() < patience + Duration::from_secs(10) {
                return Task::none();
            }
            log::warn!("screenshot: the window is not being drawn (hidden, or its screen locked?): this is what it showed last");
        }
        self.dev.screenshot_requested = true;
        if let Some(path) = self.opts.dev.picture.clone() {
            let picture = self.gpu.clone().and_then(|gpu| {
                let mut pipeline = self.picture_pipeline.take();
                let (size, lit) = (self.opts.dev.picture_size.unwrap_or([512, 512]), self.opts.dev.picture_lit);
                let picture = crate::view::view3d::picture(self, &mut pipeline, &gpu, size, lit);
                self.picture_pipeline = pipeline;
                picture
            });
            match picture.map(|p| save_picture(&p, &path)) {
                Some(Ok(())) => println!("picture saved to {}", path.display()),
                other => eprintln!("picture failed: {other:?}"),
            }
            return iced::exit();
        }
        let Some(id) = self.window else { return Task::none() };
        window::screenshot(id).map(Message::Screenshot)
    }

    /// Whether the window shows what a screenshot is of: a screenshot is of the frame drawn
    /// last, which is of before what arrived last (on a machine that draws a frame in seconds —
    /// one without a GPU — of long before: the nodes without their pictures, the model
    /// without its maps). Two frames are waited for since all had arrived: the first is being
    /// drawn as it is told of.
    fn shown_for_a_screenshot(&mut self) -> bool {
        match self.dev.drawn {
            Some(frames) => frames >= 2,
            None => {
                self.dev.drawn = Some(0);
                false
            }
        }
    }

    pub(super) fn save_screenshot(&mut self, shot: window::Screenshot) -> Task<Message> {
        if let Some(path) = &self.opts.dev.screenshot {
            let (w, h) = (shot.size.width, shot.size.height);
            match ::image::RgbaImage::from_raw(w, h, shot.rgba.to_vec()).map(|img| img.save(path)) {
                Some(Ok(())) => println!("screenshot saved to {}", path.display()),
                other => eprintln!("screenshot failed: {other:?}"),
            }
        }
        iced::exit()
    }
}

/// The first node of a type, by its id or its name ("levels", "tile-sampler").
/// Where the light of the pictures of the templates' materials comes from: the left of the
/// camera, a little above it and before the model (the 3D view's light turned: its yaw and its
/// pitch).
const PREVIEW_KEY: (f32, f32) = (-1.33, 0.37);

/// The light the pictures of the templates' materials are made in: a dark room and one softbox to
/// the left of the camera, a little above it and before the model, the studio's key light in it
/// (the 3D view adds it, see `Lighting::key`), so half of a sphere is lit and half is in
/// shadow, and a coat or a metal reflects the softbox and the dark around it (not a bright sky
/// all round, in which the edge of a coat glows as fuzz does).
fn preview_light(yaw: f32, pitch: f32) -> crate::environment::Hdri {
    use std::f32::consts::{PI, TAU};
    const W: usize = 1024;
    const H: usize = 512;
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let normalize = |a: [f32; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let smoothstep = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // The softbox: where the key light is (the camera looks along -z), facing the model, its
    // axes and how large it is (half, as far as it is).
    let key = [pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos()];
    let right = normalize([key[2], 0.0, -key[0]]);
    let up = [right[1] * key[2] - right[2] * key[1], right[2] * key[0] - right[0] * key[2], right[0] * key[1] - right[1] * key[0]];
    let half = [0.45, 0.6];
    // The 3D view turns its environment by the light's turn.
    let (s, c) = yaw.sin_cos();
    let mut rgb = Vec::with_capacity(W * H * 3);
    for y in 0..H {
        for x in 0..W {
            let (phi, theta) = (((x as f32 + 0.5) / W as f32 - 0.5) * TAU, (y as f32 + 0.5) / H as f32 * PI);
            let p = [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()];
            // The direction in the scene: the panorama's turned back.
            let d = [c * p[0] - s * p[2], p[1], c * p[2] + s * p[0]];
            // The room: dark, a little lighter above.
            let room = 0.01 + 0.012 * (d[1] * 0.5 + 0.5);
            let facing = dot(d, key);
            let mut light = 0.0;
            if facing > 0.05 {
                let q = [d[0] / facing - key[0], d[1] / facing - key[1], d[2] / facing - key[2]];
                let edge = (dot(q, right) / half[0]).abs().max((dot(q, up) / half[1]).abs());
                light = 3.0 * (1.0 - smoothstep(0.85, 1.0, edge));
            }
            rgb.extend([room * 0.9 + light, room * 0.95 + light * 0.97, room + light * 0.93]);
        }
    }
    crate::environment::Hdri::new("the pictures' light".into(), W, H, rgb)
}

/// `text` as a value of `param`: a number, numbers apart by commas (a vector, a color), an
/// option by its id or its place, true or false, or words.
fn param_value(param: &brewed_texture_core::nodes::ParamDef<'_>, text: &str) -> Option<ParamValue> {
    use brewed_texture_core::nodes::ParamKind;
    let numbers: Vec<f32> = text.split(',').map(|n| n.trim().parse::<f32>()).collect::<Result<_, _>>().unwrap_or_default();
    Some(match param.kind {
        ParamKind::Float { .. } => ParamValue::Float(*numbers.first()?),
        ParamKind::Int { .. } => ParamValue::Int(text.trim().parse().ok()?),
        ParamKind::Bool { .. } => ParamValue::Bool(text.trim().parse().ok()?),
        ParamKind::Vec2 { .. } => ParamValue::Vec2([*numbers.first()?, *numbers.get(1).or(numbers.first())?]),
        ParamKind::Color { .. } => {
            let c = |i: usize| numbers.get(i).or(numbers.first()).copied();
            ParamValue::Color([c(0)?, c(1)?, c(2)?, numbers.get(3).copied().unwrap_or(1.0)])
        }
        ParamKind::Enum { ids, .. } => ParamValue::Enum(ids.iter().position(|id| *id == text.trim()).map(|i| i as u32).or_else(|| text.trim().parse().ok())?),
        ParamKind::Text { .. } | ParamKind::Expression { .. } | ParamKind::File { .. } => ParamValue::Text(text.into()),
        _ => return None,
    })
}

fn find_kind(graph: &Graph, name: &str) -> Option<NodeId> {
    let name = name.to_lowercase().replace([' ', '-'], "_");
    graph
        .nodes
        .iter()
        .find(|n| n.kind.id() == name || n.def().name.to_lowercase().replace(' ', "_") == name)
        .map(|n| n.id)
}

/// Save a picture: as PNG, or as JPEG (of quality 90) where the name says .jpg.
pub(crate) fn save_picture(picture: &::image::RgbaImage, path: &std::path::Path) -> Result<(), String> {
    let jpeg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if !jpeg {
        return picture.save(path).map_err(|e| e.to_string());
    }
    let rgb = ::image::DynamicImage::ImageRgba8(picture.clone()).to_rgb8();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    ::image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 90).encode_image(&rgb).map_err(|e| e.to_string())
}
