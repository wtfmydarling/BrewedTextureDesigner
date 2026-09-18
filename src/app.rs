//! The application: window layout, menus, shortcuts, documents.
//!
//! Layout (as requested):
//! ```text
//! +-----------+-----------------------------+------------------+
//! |           | toolbar (menus, resolution) |  Input Values    |
//! |  Node     +-----------------------------+   (resizable)    |
//! |  library  |        node graph           +------------------+
//! |           +-----------------------------+  Properties of   |
//! |           |          2D view            |  selected node   |
//! +-----------+-----------------------------+------------------+
//! ```

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText};
use serde::{Deserialize, Serialize};

use crate::engine::export::{self, ExportMessage};
use crate::engine::{Engine, TextureRegistry};
use crate::gpu::Gpu;
use crate::graph::{Clipboard, Graph, NodeId, NodeKind, ParamValue};
use crate::history::History;
use crate::ui::graph_view::GraphView;
use crate::ui::inputs_panel::InputsPanelState;
use crate::ui::library::LibraryState;
use crate::ui::preview::PreviewState;
use crate::ui::properties::PropsState;
use crate::ui::{self, ERROR};

mod autotest;

/// Number of failed steps of the last `--ui-test` run (read by `main` for the exit code).
pub static UI_TEST_FAILURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const RESOLUTIONS: [u32; 6] = [128, 256, 512, 1024, 2048, 4096];
const FILE_EXTENSION: &str = "ftx";

#[derive(Default, Clone)]
pub struct StartupOptions {
    pub open: Option<PathBuf>,
    /// Save a screenshot of the window after startup and quit (used for testing).
    pub screenshot: Option<PathBuf>,
    /// Select the first node of this kind at startup (e.g. "Math").
    pub select: Option<String>,
    /// Show the first node of this kind in the 2D view at startup.
    pub preview: Option<String>,
    /// Drive the UI with synthetic input and check the results, then quit.
    pub ui_test: bool,
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    app: String,
    version: u32,
    graph: Graph,
    #[serde(default)]
    preview: Option<NodeId>,
}

struct Status {
    text: String,
    error: bool,
    time: f64,
}

struct ScreenshotJob {
    path: PathBuf,
    requested: bool,
}

pub struct FerrotexApp {
    graph: Graph,
    engine: Engine,
    view: GraphView,
    preview: PreviewState,
    library: LibraryState,
    inputs: InputsPanelState,
    props: PropsState,
    history: History,
    clipboard: Option<Clipboard>,
    project_path: Option<PathBuf>,
    saved_snapshot: String,
    status: Option<Status>,
    export_tx: Sender<ExportMessage>,
    export_rx: Receiver<ExportMessage>,
    lock_square: bool,
    show_shortcuts: bool,
    show_about: bool,
    screenshot: Option<ScreenshotJob>,
    frame: u64,
    title: String,
    autopilot: Option<autotest::AutoPilot>,
}

fn snapshot(graph: &Graph) -> String {
    serde_json::to_string(graph).unwrap_or_default()
}

impl FerrotexApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: StartupOptions) -> Result<FerrotexApp, String> {
        let rs = cc
            .wgpu_render_state
            .as_ref()
            .ok_or("Ferrotex needs the wgpu renderer (no compatible GPU found)")?;
        let info = rs.adapter.get_info();
        log::info!("GPU: {} ({:?})", info.name, info.backend);
        let gpu = Gpu::new(rs.device.clone(), rs.queue.clone(), format!("{} · {:?}", info.name, info.backend));
        let registry = TextureRegistry { renderer: rs.renderer.clone(), device: rs.device.clone() };
        let engine = Engine::new(gpu, Some(registry));
        ui::apply_theme(&cc.egui_ctx);

        let (graph, preview_node) = crate::demo::brick_wall();
        let (export_tx, export_rx) = channel();
        let mut app = FerrotexApp {
            history: History::new(&graph),
            saved_snapshot: snapshot(&graph),
            graph,
            engine,
            view: GraphView::default(),
            preview: PreviewState::default(),
            library: LibraryState::default(),
            inputs: InputsPanelState::default(),
            props: PropsState::default(),
            clipboard: None,
            project_path: None,
            status: None,
            export_tx,
            export_rx,
            lock_square: true,
            show_shortcuts: false,
            show_about: false,
            screenshot: options.screenshot.map(|path| ScreenshotJob { path, requested: false }),
            frame: 0,
            title: String::new(),
            autopilot: options.ui_test.then(autotest::AutoPilot::new),
        };
        app.preview.node = Some(preview_node);
        let select_kind = options.select.as_deref().unwrap_or("Math");
        if let Some(n) = app.graph.nodes.iter().find(|n| format!("{:?}", n.kind).eq_ignore_ascii_case(select_kind)) {
            app.view.selected.insert(n.id);
        }
        if let Some(kind) = options.preview.as_deref() {
            if let Some(n) = app.graph.nodes.iter().find(|n| format!("{:?}", n.kind).eq_ignore_ascii_case(kind)) {
                app.preview.node = Some(n.id);
            }
        }
        if let Some(path) = options.open {
            app.open_path(path);
        }
        Ok(app)
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool, now: f64) {
        let text = text.into();
        if error {
            log::warn!("{text}");
        }
        self.status = Some(Status { text, error, time: now });
    }

    fn replace_graph(&mut self, graph: Graph, preview: Option<NodeId>) {
        self.graph = graph;
        self.engine.reset();
        self.history.reset(&self.graph);
        self.saved_snapshot = snapshot(&self.graph);
        self.view.selected.clear();
        self.view.selected_link = None;
        self.view.request_frame();
        self.preview.node = preview
            .filter(|p| self.graph.contains(*p))
            .or_else(|| self.graph.nodes.iter().find(|n| n.kind == NodeKind::Output).map(|n| n.id));
    }

    fn new_document(&mut self) {
        self.replace_graph(Graph::default(), None);
        self.project_path = None;
    }

    fn open_dialog(&mut self, now: f64) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open Ferrotex graph")
            .add_filter("Ferrotex graph", &[FILE_EXTENSION, "json"])
            .pick_file()
        {
            self.open_path(path);
            if let Some(s) = &mut self.status {
                s.time = now;
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<ProjectFile>(&text).map_err(|e| e.to_string()));
        match result {
            Ok(mut file) => {
                file.graph.normalize();
                self.replace_graph(file.graph, file.preview);
                self.status = Some(Status { text: format!("Opened {}", path.display()), error: false, time: 0.0 });
                self.project_path = Some(path);
            }
            Err(e) => {
                self.status = Some(Status { text: format!("Cannot open {}: {e}", path.display()), error: true, time: 0.0 });
            }
        }
    }

    fn save(&mut self, save_as: bool, now: f64) {
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .set_title("Save Ferrotex graph")
                .add_filter("Ferrotex graph", &[FILE_EXTENSION])
                .set_file_name(format!("untitled.{FILE_EXTENSION}"))
                .save_file(),
        };
        let Some(path) = path else { return };
        let file = ProjectFile { app: "ferrotex".into(), version: 1, graph: self.graph.clone(), preview: self.preview.node };
        let result = serde_json::to_string_pretty(&file)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
        match result {
            Ok(()) => {
                self.saved_snapshot = snapshot(&self.graph);
                self.set_status(format!("Saved {}", path.display()), false, now);
                self.project_path = Some(path);
            }
            Err(e) => self.set_status(format!("Cannot save: {e}"), true, now),
        }
    }

    fn export_all(&mut self, now: f64) {
        if !self.graph.nodes.iter().any(|n| n.kind == NodeKind::Output) {
            self.set_status("The graph has no Output nodes to export", true, now);
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("Export outputs to folder").pick_folder() else { return };
        let n = export::export_outputs(&self.engine, &self.graph, &dir, &self.export_tx);
        self.set_status(format!("Exporting {n} output(s) to {}…", dir.display()), false, now);
    }

    fn export_one(&mut self, node: NodeId, now: f64) {
        let name = self.graph.node(node).map(|n| export::sanitize_file_name(n.text("name"))).unwrap_or_default();
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export output")
            .add_filter("PNG image", &["png"])
            .set_file_name(format!("{name}.png"))
            .save_file()
        {
            if export::export_node(&self.engine, &self.graph, node, path, &self.export_tx) {
                self.set_status("Exporting…", false, now);
            }
        }
    }

    fn undo(&mut self) {
        if let Some(g) = self.history.undo() {
            self.graph = g;
            self.view.selected.retain(|id| self.graph.contains(*id));
        }
    }

    fn redo(&mut self) {
        if let Some(g) = self.history.redo() {
            self.graph = g;
            self.view.selected.retain(|id| self.graph.contains(*id));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, now: f64) {
        let typing = ctx.egui_wants_keyboard_input();
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save(true, now);
        }
        if pressed(Modifiers::COMMAND, Key::S) {
            self.save(false, now);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.open_dialog(now);
        }
        if pressed(Modifiers::COMMAND, Key::E) {
            self.export_all(now);
        }
        if typing {
            return; // let text fields keep their own undo
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.new_document();
        }
        if ctx.input(|i| i.key_pressed(Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, now: f64) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New                 Ctrl+N").clicked() {
                    self.new_document();
                    ui.close();
                }
                if ui.button("Open…               Ctrl+O").clicked() {
                    ui.close();
                    self.open_dialog(now);
                }
                if ui.button("Save                Ctrl+S").clicked() {
                    ui.close();
                    self.save(false, now);
                }
                if ui.button("Save As…      Ctrl+Shift+S").clicked() {
                    ui.close();
                    self.save(true, now);
                }
                ui.separator();
                if ui.button("Export Outputs…     Ctrl+E").clicked() {
                    ui.close();
                    self.export_all(now);
                }
                ui.separator();
                if ui.button("Load Demo Graph").clicked() {
                    let (g, p) = crate::demo::brick_wall();
                    self.replace_graph(g, Some(p));
                    self.project_path = None;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo          Ctrl+Z")).clicked() {
                    self.undo();
                    ui.close();
                }
                if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo          Ctrl+Y")).clicked() {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Select All    Ctrl+A").clicked() {
                    self.view.selected = self.graph.nodes.iter().map(|n| n.id).collect();
                    ui.close();
                }
                if ui.button("Delete Selection    Del").clicked() {
                    self.view.delete_selection(&mut self.graph);
                    ui.close();
                }
                if ui.button("Frame All           F").clicked() {
                    self.view.request_frame();
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Shortcuts           F1").clicked() {
                    self.show_shortcuts = true;
                    ui.close();
                }
                if ui.button("About Ferrotex").clicked() {
                    self.show_about = true;
                    ui.close();
                }
            });
            ui.separator();

            // Texture size of the whole graph.
            ui.label("Size");
            let mut res = self.graph.resolution;
            for axis in 0..2 {
                egui::ComboBox::from_id_salt(("res", axis))
                    .width(64.0)
                    .selected_text(res[axis].to_string())
                    .show_ui(ui, |ui| {
                        for r in RESOLUTIONS {
                            ui.selectable_value(&mut res[axis], r, r.to_string());
                        }
                    });
                if axis == 0 {
                    ui.label("×");
                }
            }
            if self.lock_square && res != self.graph.resolution {
                let changed = if res[0] != self.graph.resolution[0] { res[0] } else { res[1] };
                res = [changed, changed];
            }
            ui.toggle_value(&mut self.lock_square, "Square").on_hover_text("Keep width and height equal");
            if res != self.graph.resolution {
                self.graph.resolution = res;
                self.set_status(format!("Texture size set to {}×{}", res[0], res[1]), false, now);
            }
            ui.separator();
            let r = self.engine.report;
            ui.weak(format!("{} nodes · {} recomputed · {:.1} ms", self.graph.nodes.len(), r.recomputed, r.cpu_ms))
                .on_hover_text(format!(
                    "GPU: {}\nNodes are only recomputed when something upstream changed.",
                    self.engine.gpu.adapter_info
                ));

            if let Some(s) = &self.status {
                if now - s.time < 7.0 || s.error && now - s.time < 12.0 {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if s.error { ERROR } else { Color32::from_rgb(140, 200, 140) };
                        ui.label(RichText::new(&s.text).color(color));
                    });
                }
            }
        });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        egui::Window::new("Shortcuts").open(&mut self.show_shortcuts).resizable(false).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("shortcuts").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                for (k, v) in [
                    ("Drag from library", "Add a node"),
                    ("Space / double-click", "Search and add a node at the cursor"),
                    ("Drag from a socket", "Connect (drop on empty space to add + connect)"),
                    ("Drag a connected input", "Re-route or remove the link"),
                    ("Double-click a node", "Show it in the 2D view"),
                    ("Mouse wheel", "Zoom (graph and 2D view)"),
                    ("Middle / right drag, Alt+drag", "Pan"),
                    ("Left drag on empty space", "Box select (Shift/Ctrl adds)"),
                    ("Del / Backspace", "Delete selection"),
                    ("Ctrl+C / Ctrl+X / Ctrl+V", "Copy / cut / paste nodes"),
                    ("Ctrl+D", "Duplicate"),
                    ("Ctrl+A", "Select all"),
                    ("F", "Frame selection / all"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                    ("Ctrl+S / Ctrl+O / Ctrl+N", "Save / open / new"),
                    ("Ctrl+E", "Export all outputs"),
                    ("Shift while dragging gizmo", "Snap rotation / uniform scale"),
                ] {
                    ui.label(RichText::new(k).strong());
                    ui.label(v);
                    ui.end_row();
                }
            });
        });
        egui::Window::new("About Ferrotex").open(&mut self.show_about).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.heading("Ferrotex");
            ui.label("GPU accelerated procedural texture editor.");
            ui.label("Rust · wgpu compute shaders · egui · SIMD (wide) + rayon.");
            ui.label(format!("GPU: {}", self.engine.gpu.adapter_info));
            ui.label(format!("CPU threads: {}", rayon::current_num_threads()));
        });
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.screenshot else { return };
        if !job.requested && self.frame >= 45 && !self.engine.busy() {
            job.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let path = job.path.clone();
            match image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|img| img.save(&path)) {
                Some(Ok(())) => println!("screenshot saved to {}", path.display()),
                other => eprintln!("screenshot failed: {other:?}"),
            }
            self.screenshot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for FerrotexApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(pilot) = &mut self.autopilot {
            // The self-test owns the input: ignore the real mouse and keyboard.
            raw.events.retain(|e| {
                !matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::MouseMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::ModifiersChanged(_)
                        | egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::Zoom(_)
                )
            });
            raw.events.extend(pilot.take_events());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.frame += 1;
        self.shortcuts(&ctx, now);

        while let Ok(msg) = self.export_rx.try_recv() {
            match msg {
                ExportMessage::Saved(p) => self.set_status(format!("Exported {}", p.display()), false, now),
                ExportMessage::Failed(e) => self.set_status(e, true, now),
            }
        }

        // Left: node library.
        egui::Panel::left("library")
            .resizable(true)
            .default_size(200.0)
            .size_range(150.0..=380.0)
            .show(ui, |ui| {
                if let Some(item) = ui::library::show(ui, &mut self.library) {
                    let pos = self.view.center_graph_pos();
                    let id = ui::create_from_library(&mut self.graph, &item, pos);
                    self.view.selected.clear();
                    self.view.selected.insert(id);
                }
            });

        // Right: input values (top) and properties (bottom), split by a draggable edge.
        egui::Panel::right("right")
            .resizable(true)
            .default_size(350.0)
            .size_range(270.0..=640.0)
            .show(ui, |ui| {
                let total = ui.available_height();
                let max_top = (total - 140.0).max(100.0);
                egui::Panel::top("inputs")
                    .resizable(true)
                    .default_size((total * 0.36).clamp(100.0, max_top))
                    .size_range(90.0..=max_top)
                    .show(ui, |ui| {
                        ui::inputs_panel::show(ui, &mut self.graph, &mut self.inputs);
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    let out = ui::properties::show(
                        ui,
                        &mut self.graph,
                        &mut self.engine,
                        &self.view.selected,
                        &mut self.inputs,
                        &mut self.props,
                    );
                    if let Some(id) = out.export_node {
                        self.export_one(id, now);
                    }
                    if let Some(id) = out.preview {
                        self.preview.node = Some(id);
                    }
                    if let Some(id) = out.browse_bitmap {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose an image")
                            .add_filter("Images", &["png", "jpg", "jpeg", "tga", "bmp"])
                            .pick_file()
                        {
                            if let Some(p) = self.graph.node_mut(id).and_then(|n| n.param_mut("path")) {
                                p.value = ParamValue::Text(path.display().to_string());
                            }
                        }
                    }
                });
            });

        // Center: toolbar, graph and 2D view.
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            egui::Panel::top("toolbar")
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 4)))
                .show(ui, |ui| self.toolbar(ui, now));
            egui::Panel::bottom("preview")
                .resizable(true)
                .default_size(340.0)
                .size_range(120.0..=1200.0)
                .show(ui, |ui| {
                    ui::preview::show(ui, &mut self.graph, &mut self.engine, &mut self.preview);
                });
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                let out = self.view.show(ui, &mut self.graph, &self.engine, self.preview.node, &mut self.clipboard);
                if let Some(p) = out.preview {
                    self.preview.node = Some(p);
                }
                if let Some((text, error)) = out.message {
                    self.set_status(text, error, now);
                }
            });
        });
        self.windows(&ctx);

        // Evaluate the graph on the GPU and refresh the 2D view texture. This happens
        // after the UI so this frame already paints the new results.
        self.graph.prune_links();
        self.engine.evaluate(&self.graph);
        if let Some(node) = self.preview.node {
            let had = self.engine.preview_info(node).is_some();
            if self.engine.prepare_preview(node, self.preview.mode).is_some() && !had {
                ctx.request_repaint();
            }
        }
        if self.engine.report.recomputed > 0 || self.engine.busy() {
            ctx.request_repaint();
        }
        for e in self.engine.gpu.take_errors() {
            self.set_status(format!("GPU error: {e}"), true, now);
        }

        let interacting = ctx.input(|i| i.pointer.any_down());
        self.history.track(&self.graph, now, interacting);

        let name = self
            .project_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("untitled".to_string(), |n| n.to_string_lossy().to_string());
        let dirty = snapshot(&self.graph) != self.saved_snapshot;
        let title = format!("Ferrotex — {name}{}", if dirty { " *" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.handle_screenshot(&ctx);

        if let Some(mut pilot) = self.autopilot.take() {
            pilot.update(self, now);
            ctx.request_repaint();
            if pilot.finished {
                let failed = pilot.results.iter().filter(|r| r.1.is_err()).count();
                UI_TEST_FAILURES.store(failed, std::sync::atomic::Ordering::SeqCst);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.autopilot = Some(pilot);
            }
        }
    }
}
