//! Runs an [`Engine`] on its own thread so front ends never wait for the GPU.
//!
//! Requests are coalesced: while the GPU is busy, newer graphs replace older ones and only
//! the latest is evaluated. Every loop iteration waits for the GPU to finish, which keeps
//! the queue short and the latency low while a slider is dragged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use brewed_texture_core::eval::Evaluation;
use brewed_texture_core::graph::{Graph, NodeId};

use crate::engine::{ChoicePictures, DisplayImage, Engine, EvalReport, ImageReport, Thumbnail};
use crate::export;
use crate::gpu::Gpu;
use crate::simd::ImageStats;

/// One file to write.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportJob {
    pub node: NodeId,
    pub path: PathBuf,
    /// Output format (index into [`brewed_texture_core::nodes::OUTPUT_FORMATS`]).
    pub format: u32,
    /// See [`export::DEPTH_8`] and friends.
    pub bit_depth: u32,
}

pub enum Request {
    Evaluate {
        revision: u64,
        graph: Arc<Graph>,
        eval: Arc<Evaluation>,
    },
    /// Which node the 2D view shows.
    Preview(Option<NodeId>),
    /// Output nodes used by the 3D view.
    Materials(Vec<NodeId>),
    Stats(NodeId),
    Probe {
        node: NodeId,
        pixel: [u32; 2],
    },
    Export(Vec<ExportJob>),
    /// Pictures of what a node makes with each option of one of its choices.
    Choices {
        node: NodeId,
        key: String,
    },
    ReloadBitmap(String),
    /// Forget every cached image (e.g. after opening another document).
    Reset,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// The work of a change is on its way to the GPU: every node's version and problems.
    Evaluated {
        revision: u64,
        reports: HashMap<NodeId, ImageReport>,
    },
    /// The GPU finished what a change asked for: its images, their copies for the views
    /// and their thumbnails. `millis` is how long that took from the moment the engine
    /// started on the change.
    Finished {
        revision: u64,
        report: EvalReport,
        millis: f32,
    },
    Thumbnails(Vec<Thumbnail>),
    /// The 2D view image changed (or went away).
    Preview(Option<DisplayImage>),
    Materials(Vec<DisplayImage>),
    Stats {
        node: NodeId,
        version: u64,
        stats: Arc<ImageStats>,
    },
    Probe {
        node: NodeId,
        pixel: [u32; 2],
        rgba: Option<[f32; 4]>,
    },
    Choices(ChoicePictures),
    Exported(Result<PathBuf, String>),
    GpuErrors(Vec<String>),
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

pub struct EngineService {
    tx: Option<Sender<Request>>,
    thread: Option<JoinHandle<()>>,
    adapter: String,
}

impl EngineService {
    /// Start the engine thread on an existing device (the UI's, so textures can be shown
    /// without copies). Events are delivered through `emit` from the engine thread.
    pub fn spawn(device: wgpu::Device, queue: wgpu::Queue, adapter: String, emit: Emit) -> EngineService {
        let (tx, rx) = channel();
        let name = adapter.clone();
        let thread = std::thread::Builder::new()
            .name("brewed-texture-engine".into())
            .spawn(move || run(Engine::new(Gpu::new(device, queue, name)), rx, emit))
            .expect("spawn engine thread");
        EngineService { tx: Some(tx), thread: Some(thread), adapter }
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn send(&self, request: Request) {
        if let Some(tx) = &self.tx
            && tx.send(request).is_err()
        {
            log::error!("the engine thread has stopped");
        }
    }
}

impl Drop for EngineService {
    fn drop(&mut self) {
        // Closing the channel ends the loop.
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Default)]
struct Pending {
    evaluate: Option<(u64, Arc<Graph>, Arc<Evaluation>)>,
    stats: Vec<NodeId>,
    probe: Option<(NodeId, [u32; 2])>,
    choices: Option<(NodeId, String)>,
    exports: Vec<ExportJob>,
    preview_changed: bool,
    materials_changed: bool,
}

fn run(mut engine: Engine, rx: Receiver<Request>, emit: Emit) {
    let mut current: Option<(u64, Arc<Graph>, Arc<Evaluation>)> = None;
    let mut preview: Option<NodeId> = None;
    let mut materials: Vec<NodeId> = Vec::new();
    loop {
        // Wait for work; poll while files are being decoded in the background.
        let first = if engine.bitmaps_loading() {
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(r) => Some(r),
                Err(_) => break,
            }
        };
        let mut pending = Pending::default();
        for request in first.into_iter().chain(rx.try_iter()) {
            match request {
                Request::Evaluate { revision, graph, eval } => pending.evaluate = Some((revision, graph, eval)),
                Request::Preview(node) => {
                    preview = node;
                    pending.preview_changed = true;
                }
                Request::Materials(nodes) => {
                    materials = nodes;
                    pending.materials_changed = true;
                }
                Request::Stats(node) => {
                    if !pending.stats.contains(&node) {
                        pending.stats.push(node);
                    }
                }
                Request::Probe { node, pixel } => pending.probe = Some((node, pixel)),
                Request::Export(jobs) => pending.exports.extend(jobs),
                Request::Choices { node, key } => pending.choices = Some((node, key)),
                Request::ReloadBitmap(path) => {
                    engine.reload_bitmap(&path);
                    pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
                }
                Request::Reset => {
                    engine.reset();
                    pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
                }
            }
        }
        if engine.receive_bitmaps() {
            pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
        }

        let mut evaluated = None;
        if let Some((revision, graph, eval)) = pending.evaluate.take() {
            let started = Instant::now();
            let report = engine.evaluate(&graph, &eval);
            (emit)(Event::Evaluated { revision, reports: engine.reports() });
            current = Some((revision, graph, eval));
            evaluated = Some((revision, report, started));
        }
        let evaluated_now = evaluated.is_some();
        if (evaluated_now || pending.preview_changed)
            && let Some(change) = engine.prepare_preview(preview)
        {
            (emit)(Event::Preview(change));
        }
        if (evaluated_now || pending.materials_changed)
            && let Some(set) = engine.prepare_materials(&materials)
        {
            (emit)(Event::Materials(set));
        }
        if let Some((revision, report, started)) = evaluated {
            // Also waits for the GPU, which throttles the loop to the GPU's pace.
            let thumbs = engine.render_thumbnails();
            if !thumbs.is_empty() {
                (emit)(Event::Thumbnails(thumbs));
            }
            // Whatever the thumbnails did not wait for is done too before the clock stops.
            engine.wait_idle();
            let millis = started.elapsed().as_secs_f32() * 1000.0;
            (emit)(Event::Finished { revision, report, millis });
        }
        // After the evaluation, so the pictures show the node as it is now.
        if let Some((node, key)) = pending.choices
            && let Some((_, graph, eval)) = &current
            && let Some(pictures) = engine.render_choices(graph, eval, node, &key)
        {
            (emit)(Event::Choices(pictures));
        }
        for node in pending.stats {
            if let (Some(stats), Some(version)) = (engine.stats(node), engine.version(node)) {
                (emit)(Event::Stats { node, version, stats });
            }
        }
        if let Some((node, pixel)) = pending.probe {
            (emit)(Event::Probe { node, pixel, rgba: engine.probe(node, pixel) });
        }
        for job in pending.exports {
            match engine.read_image(job.node) {
                Ok((w, h, rgba)) => {
                    let emit = emit.clone();
                    // Quantizing and encoding happen off the engine thread.
                    std::thread::spawn(move || {
                        let result =
                            export::write_image(&job.path, &rgba, w, h, job.format, job.bit_depth).map(|_| job.path);
                        (emit)(Event::Exported(result));
                    });
                }
                Err(e) => (emit)(Event::Exported(Err(e))),
            }
        }
        let errors = engine.gpu.take_errors();
        if !errors.is_empty() {
            (emit)(Event::GpuErrors(errors));
        }
    }
}
