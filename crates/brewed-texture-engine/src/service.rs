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
use brewed_texture_core::NodeKind;
use brewed_texture_core::graph::{Graph, NodeId};

use crate::engine::{ChoicePictures, DisplayImage, Engine, EvalReport, ImageReport, Keep, Memory, Thumbnail};
use crate::export;
use crate::gpu::Gpu;
use crate::simd::ImageStats;

/// How long the engine waits with nothing to do before it settles (see [`Engine::settle`]):
/// while changes keep coming (a slider dragged) what the last change read from outside itself
/// is read again, once they stop it only takes memory. (The scratch images go as soon as no
/// change is waiting, see [`Engine::keep_scratch`].)
const IDLE: Duration = Duration::from_millis(500);

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
    /// Pictures of what a node of a kind makes with each option of one of its choices.
    Choices {
        kind: NodeKind,
        key: String,
    },
    /// Read a file again (edited elsewhere): see [`crate::Engine::reload_file`].
    ReloadFile(String),
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
    /// The GPU finished what a change asked for: its images, the mips of those the views
    /// show and their thumbnails. `millis` is how long that took from the moment the engine
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
    /// GPU memory the engine's images take now (sent when it changes).
    Memory(Memory),
    Exported(Result<PathBuf, String>),
    GpuErrors(Vec<String>),
    /// The engine stopped, as it never should: why. Nothing is computed any more.
    Stopped(String),
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
        // Every kernel built once meanwhile, for what the device keeps of it (see
        // `gpu::warm_up`): the first of a kind of node is not waited for.
        let warm = device.clone();
        let _ = std::thread::Builder::new().name("brewed-texture-warm-up".into()).spawn(move || crate::gpu::warm_up(&warm));
        let thread = std::thread::Builder::new()
            .name("brewed-texture-engine".into())
            .spawn(move || {
                // The front end reads the views' images and asks for others: only those stay.
                let mut engine = Engine::new(Gpu::new(device, queue, name));
                engine.set_keep(Keep::Read);
                // Should it stop (it never should), the front end is told, rather than left
                // waiting for images that never come.
                let tell = emit.clone();
                if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(engine, rx, emit))) {
                    let why = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "an error".to_string());
                    tell(Event::Stopped(why));
                }
            })
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
    choices: Vec<(NodeKind, String)>,
    exports: Vec<ExportJob>,
    preview_changed: bool,
    materials_changed: bool,
}

fn run(mut engine: Engine, rx: Receiver<Request>, emit: Emit) {
    let mut current: Option<(u64, Arc<Graph>, Arc<Evaluation>)> = None;
    let mut preview: Option<NodeId> = None;
    let mut materials: Vec<NodeId> = Vec::new();
    // The GPU memory the front end was last told of (`None`: not yet, or it forgot).
    let mut memory: Option<Memory> = None;
    // A request that came while the last round was at work: the next round starts with it.
    let mut waiting: Option<Request> = None;
    loop {
        // Wait for work; poll while files are being decoded in the background.
        let first = if let Some(request) = waiting.take() {
            Some(request)
        } else if engine.files_loading() {
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv_timeout(IDLE) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => {
                    // Nothing changes: all that stays is what the views show.
                    engine.settle();
                    report_memory(&engine, &mut memory, &emit);
                    match rx.recv() {
                        Ok(r) => Some(r),
                        Err(_) => break,
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
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
                Request::Choices { kind, key } => {
                    if !pending.choices.iter().any(|(k, c)| *k == kind && *c == key) {
                        pending.choices.push((kind, key));
                    }
                }
                Request::ReloadFile(path) => {
                    engine.reload_file(&path);
                    pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
                }
                Request::Reset => {
                    engine.reset();
                    // The front end forgets everything it was told with it.
                    memory = None;
                    pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
                }
            }
        }
        if engine.receive_files() {
            pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
        }
        // What is read after this round: the views' nodes and the nodes asked about. Whatever
        // of them has no image is computed, even if nothing changed.
        engine.set_preview(preview);
        engine.set_materials(&materials);
        let asked: Vec<NodeId> =
            pending.stats.iter().copied().chain(pending.probe.map(|(node, _)| node)).chain(pending.exports.iter().map(|j| j.node)).collect();
        let read: Vec<NodeId> = preview.iter().chain(&materials).chain(&asked).copied().collect();
        engine.ask(asked);
        if engine.missing(&read) {
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
        // A view that moved on to an image that was there needed no evaluation: what it showed
        // before goes now.
        engine.sweep();
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
            // The scratch images the evaluation worked in serve the next change if one is
            // already coming (a slider being dragged), as many of them as are worth their
            // memory; after anything else (a node shown, a single change) they only take it.
            waiting = rx.try_recv().ok();
            engine.keep_scratch(waiting.is_some());
            // Now, not after whatever else this round does (the pictures of the options take
            // a while when they are drawn first).
            report_memory(&engine, &mut memory, &emit);
        }
        for (kind, key) in pending.choices {
            if let Some(pictures) = engine.render_choices(kind, &key) {
                (emit)(Event::Choices(pictures));
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
        for job in pending.exports {
            match engine.read_stored(job.node) {
                Ok(image) => {
                    let emit = emit.clone();
                    // Converting and encoding happen off the engine thread, from the image as
                    // the GPU stores it, a band at a time.
                    std::thread::spawn(move || {
                        let result =
                            export::write_stored(&job.path, &image, job.format, job.bit_depth).map(|_| job.path);
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
        // What this round did not use, the next is unlikely to; and with nothing coming, none
        // of it is.
        if waiting.is_none() {
            waiting = rx.try_recv().ok();
        }
        engine.keep_scratch(waiting.is_some());
        report_memory(&engine, &mut memory, &emit);
    }
}

/// Tell the front end how much GPU memory the engine's images take, when that is not what it
/// was last told.
fn report_memory(engine: &Engine, reported: &mut Option<Memory>, emit: &Emit) {
    let now = engine.memory_report();
    if *reported != Some(now) {
        *reported = Some(now);
        (emit)(Event::Memory(now));
    }
}
