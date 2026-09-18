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
use std::time::Duration;

use tessera_core::eval::Evaluation;
use tessera_core::graph::{Graph, NodeId};

use crate::engine::{DisplayImage, Engine, EvalReport, ImageReport, Thumbnail};
use crate::export;
use crate::gpu::Gpu;
use crate::simd::ImageStats;

/// One file to write.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportJob {
    pub node: NodeId,
    pub path: PathBuf,
    /// Output format (index into [`tessera_core::nodes::OUTPUT_FORMATS`]).
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
    SampleLine {
        node: NodeId,
        from: [f32; 2],
        to: [f32; 2],
        count: u32,
        tag: u64,
    },
    Export(Vec<ExportJob>),
    ReloadBitmap(String),
    /// Forget every cached image (e.g. after opening another document).
    Reset,
}

#[derive(Clone, Debug)]
pub enum Event {
    Evaluated {
        revision: u64,
        reports: HashMap<NodeId, ImageReport>,
        report: EvalReport,
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
    LineSamples {
        tag: u64,
        colors: Vec<[f32; 4]>,
    },
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
            .name("tessera-engine".into())
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

struct LineRequest {
    node: NodeId,
    from: [f32; 2],
    to: [f32; 2],
    count: u32,
    tag: u64,
}

#[derive(Default)]
struct Pending {
    evaluate: Option<(u64, Arc<Graph>, Arc<Evaluation>)>,
    stats: Vec<NodeId>,
    probe: Option<(NodeId, [u32; 2])>,
    lines: Vec<LineRequest>,
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
                Request::SampleLine { node, from, to, count, tag } => {
                    pending.lines.push(LineRequest { node, from, to, count, tag })
                }
                Request::Export(jobs) => pending.exports.extend(jobs),
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

        let mut evaluated = false;
        if let Some((revision, graph, eval)) = pending.evaluate.take() {
            let report = engine.evaluate(&graph, &eval);
            (emit)(Event::Evaluated { revision, reports: engine.reports(), report });
            current = Some((revision, graph, eval));
            evaluated = true;
        }
        if (evaluated || pending.preview_changed)
            && let Some(change) = engine.prepare_preview(preview)
        {
            (emit)(Event::Preview(change));
        }
        if (evaluated || pending.materials_changed)
            && let Some(set) = engine.prepare_materials(&materials)
        {
            (emit)(Event::Materials(set));
        }
        if evaluated {
            // Also waits for the GPU, which throttles the loop to the GPU's pace.
            let thumbs = engine.render_thumbnails();
            if !thumbs.is_empty() {
                (emit)(Event::Thumbnails(thumbs));
            }
        }
        for node in pending.stats {
            if let (Some(stats), Some(version)) = (engine.stats(node), engine.version(node)) {
                (emit)(Event::Stats { node, version, stats });
            }
        }
        if let Some((node, pixel)) = pending.probe {
            (emit)(Event::Probe { node, pixel, rgba: engine.probe(node, pixel) });
        }
        for l in pending.lines {
            let colors = engine.sample_line(l.node, l.from, l.to, l.count).unwrap_or_default();
            (emit)(Event::LineSamples { tag: l.tag, colors });
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
