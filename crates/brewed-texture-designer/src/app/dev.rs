//! Tools for developing the program, which users do not get (they are built with the
//! `dev-tools` feature, and for the tests): screenshots of the window in a state asked for
//! (`--screenshot`, `--scenario`, `--select`, `--preview`), and the autopilot, which drives the
//! app by itself (`--autopilot`, see [`crate::autopilot`]).

use std::path::PathBuf;
use std::time::Duration;

use iced::{Point, Subscription, Task, Vector, window};

use brewed_texture_core::graph::{Graph, NodeId, SocketRef};
use brewed_texture_core::{Edit, NodeKind, ParamValue};

use super::{AddMenu, App, ColorTarget, ContextMenu, ContextTarget, Dialog, MenuId, Message, PendingAction, VarField};
use crate::view;

/// What the tools are asked for on the command line.
#[derive(Clone, Debug, Default)]
pub struct DevOptions {
    /// Save a screenshot once everything is computed, then quit.
    pub screenshot: Option<PathBuf>,
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
    /// The autopilot, when the app drives itself.
    pub pilot: Option<crate::autopilot::Autopilot>,
}

impl Dev {
    pub fn new(options: &DevOptions) -> Dev {
        let pilot = options.autopilot.and_then(|flight| {
            let pilot = crate::autopilot::Autopilot::new(flight);
            pilot.map_err(|e| log::error!("the autopilot cannot start: {e}")).ok()
        });
        Dev { screenshot_requested: false, pilot }
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

    /// Has the engine done all it was asked, every export too?
    pub fn engine_idle(&self) -> bool {
        self.engine.is_some()
            && self.res.busy_since.is_none()
            && self.res.finished_revision >= self.eval_revision
            && self.res.exports_pending == 0
    }

    /// Ticks to see when a screenshot can be taken, and the autopilot's turns.
    pub(super) fn dev_subscriptions(&self, subs: &mut Vec<Subscription<Message>>) {
        if self.opts.dev.screenshot.is_some() && self.ui.status.is_none() {
            subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        if self.dev.pilot.as_ref().is_some_and(|p| !p.is_done()) {
            subs.push(iced::time::every(Duration::from_millis(16)).map(|_| Message::Autopilot));
        }
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
        let graph = self.doc.graph().clone();
        let first = |kind: NodeKind| graph.nodes_of(kind).next().map(|n| n.id);
        match scenario {
            "file-menu" => self.ui.menu = Some(MenuId::File),
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
                // The picker floats where it was opened from: the swatch of the tint value.
                self.ui.cursor = Point::new(1840.0, 330.0);
                self.ui.color_picker_at = self.ui.cursor + Vector::new(-120.0, 18.0);
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
            "dialog" => {
                let _ = self.doc.apply(Edit::SetResolution([1024, 1024]));
                self.ui.dialog = Some(Dialog::Unsaved(PendingAction::Quit));
            }
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
            // Several at once.
            other if other.contains(';') => return Task::batch(other.split(';').map(|s| self.apply_scenario(s)).collect::<Vec<_>>()),
            // The model of the 3D view, by its name ("mat-ball").
            other if other.starts_with("shape:") => {
                let name = other["shape:".len()..].replace('-', " ");
                match view::view3d::Shape::ALL.into_iter().find(|s| s.to_string().eq_ignore_ascii_case(&name)) {
                    Some(shape) => self.ui.preview.view3d.shape = shape,
                    None => log::warn!("no shape '{name}'"),
                }
            }
            // The 3D view lit by an environment image, as bright and as blurred behind the model
            // as said.
            other if other.starts_with("environment:") => return self.choose_environment(Some(other["environment:".len()..].to_string())),
            other if other.starts_with("exposure:") => self.ui.preview.lighting.exposure = other["exposure:".len()..].parse().unwrap_or(0.0),
            other if other.starts_with("blur:") => self.ui.preview.lighting.blur = other["blur:".len()..].parse().unwrap_or(0.25),
            other => log::warn!("unknown scenario '{other}'"),
        }
        Task::none()
    }

    pub(super) fn maybe_screenshot(&mut self) -> Task<Message> {
        if self.opts.dev.screenshot.is_none() || self.dev.screenshot_requested {
            return Task::none();
        }
        let settled = self.res.evaluated_revision == self.eval_revision
            && self.started.elapsed() > Duration::from_millis(2500)
            && (self.ui.preview.node.is_none() || self.res.preview.is_some())
            // The model wears the maps in every screenshot: they take longest to arrive.
            && self.res.materials.len() >= self.material_nodes().len()
            // ...and is lit by its environment image.
            && self.ui.preview.lighting.reading.is_none()
            // An open menu of pictures shows all of them.
            && self.ui.choice_menu.as_ref().is_none_or(|m| {
                self.graph().node(m.node).is_some_and(|n| self.res.choices.contains_key(&(n.kind.id(), m.key)))
            });
        if !settled || self.started.elapsed() < Duration::from_secs(3) {
            if self.started.elapsed() > Duration::from_secs(30) {
                log::error!("screenshot: gave up waiting for the engine");
                return iced::exit();
            }
            return Task::none();
        }
        let Some(id) = self.window else { return Task::none() };
        self.dev.screenshot_requested = true;
        window::screenshot(id).map(Message::Screenshot)
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
fn find_kind(graph: &Graph, name: &str) -> Option<NodeId> {
    let name = name.to_lowercase().replace([' ', '-'], "_");
    graph
        .nodes
        .iter()
        .find(|n| n.kind.id() == name || n.def().name.to_lowercase().replace(' ', "_") == name)
        .map(|n| n.id)
}
