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
    /// The channels the file is made of (gray of one; red, green and blue of three; and alpha
    /// of four) of what arrives at the Outputs they name (see [`export::read_what_arrives`]),
    /// in place of `node`'s image: what an export preset writes. None: `node`'s image as it is.
    pub pack: Vec<export::Packed>,
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
    /// Another document: forget every image, and the graph, what the views show of it and
    /// what was asked of it. Nothing is computed until the new one is sent (right after);
    /// files asked for before are written first, of the document they were asked of.
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

/// A graph to compute: the revision of the front end it is, the graph and its evaluation.
type Current = (u64, Arc<Graph>, Arc<Evaluation>);

#[derive(Default)]
struct Pending {
    evaluate: Option<Current>,
    stats: Vec<NodeId>,
    probe: Option<(NodeId, [u32; 2])>,
    choices: Vec<(NodeKind, String)>,
    exports: Vec<ExportJob>,
    preview_changed: bool,
    materials_changed: bool,
}

fn run(mut engine: Engine, rx: Receiver<Request>, emit: Emit) {
    let mut current: Option<Current> = None;
    let mut preview: Option<NodeId> = None;
    let mut materials: Vec<NodeId> = Vec::new();
    // The GPU memory the front end was last told of (`None`: not yet, or it forgot).
    let mut memory: Option<Memory> = None;
    // A request that came while the last round was at work: the next round starts with it.
    let mut waiting: Option<Request> = None;
    // A reset that waited for files asked of the document it forgets to be written.
    let mut reset = false;
    // The files being written, by path (see `write`).
    let mut writing: HashMap<PathBuf, JoinHandle<()>> = HashMap::new();
    loop {
        if std::mem::take(&mut reset) {
            forget(&mut engine, &mut current, &mut preview, &mut materials, &mut memory);
        }
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
                    // Files asked of the document that goes are written of it first: the
                    // reset waits for the next round, and what came after it too.
                    if !pending.exports.is_empty() {
                        reset = true;
                        break;
                    }
                    forget(&mut engine, &mut current, &mut preview, &mut materials, &mut memory);
                    // What was asked of it goes with it (the pictures of choices are of kinds
                    // of nodes, of no document), and it is not computed again: the new one
                    // comes next.
                    pending = Pending { choices: std::mem::take(&mut pending.choices), ..Pending::default() };
                }
            }
        }
        if engine.receive_files() {
            pending.evaluate = pending.evaluate.take().or_else(|| current.clone());
        }
        // A preset writes what arrives at the Outputs, as the graph about to be computed makes
        // it: the nodes it comes from are read.
        if let Some((_, graph, _)) = pending.evaluate.as_ref().or(current.as_ref()) {
            for job in pending.exports.iter_mut().filter(|j| !j.pack.is_empty()) {
                export::read_what_arrives(job, graph);
            }
        }
        // What is read after this round: the views' nodes and the nodes asked about. Whatever
        // of them has no image is computed, even if nothing changed.
        engine.set_preview(preview);
        engine.set_materials(&materials);
        let asked: Vec<NodeId> =
            pending.stats.iter().copied().chain(pending.probe.map(|(node, _)| node)).chain(pending.exports.iter().flat_map(export::nodes_of)).collect();
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
            // Packed of several images: each read as it is stored, at its own size (a uniform
            // color's one pixel), the file made of them off the engine thread.
            if !job.pack.is_empty() {
                let mut images = std::collections::HashMap::new();
                let mut failed = None;
                for node in export::nodes_of(&job) {
                    if images.contains_key(&node) {
                        continue;
                    }
                    match engine.read_own(node) {
                        Ok(image) => {
                            images.insert(node, image);
                        }
                        Err(e) => failed = Some(e),
                    }
                }
                match failed {
                    Some(e) => (emit)(Event::Exported(Err(e))),
                    None => {
                        let (emit, size) = (emit.clone(), engine.resolution());
                        write(&mut writing, job.path.clone(), move || {
                            let result = export::write_packed(&job.path, &job.pack, &images, size, job.bit_depth).map(|_| job.path);
                            (emit)(Event::Exported(result));
                        });
                    }
                }
                continue;
            }
            match engine.read_stored(job.node) {
                Ok(image) => {
                    let emit = emit.clone();
                    // Converting and encoding happen off the engine thread, from the image as
                    // the GPU stores it, a band at a time.
                    write(&mut writing, job.path.clone(), move || {
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
    // The files being written are written whole before the thread ends: the program quits
    // once they are, and leaves none half written.
    for file in writing.into_values() {
        let _ = file.join();
    }
}

/// Write a file off the engine thread once the write of it before is done: of two exports of
/// a file at once, the one asked for last is the file, however long the one before takes.
/// Files of their own are written side by side.
fn write(writing: &mut HashMap<PathBuf, JoinHandle<()>>, path: PathBuf, job: impl FnOnce() + Send + 'static) {
    writing.retain(|_, file| !file.is_finished());
    let before = writing.remove(&path);
    let file = std::thread::spawn(move || {
        if let Some(before) = before {
            let _ = before.join();
        }
        job();
    });
    writing.insert(path, file);
}

/// Forget the document (see [`Request::Reset`]): every image of the engine, and the graph, what
/// the views show of it and the memory the front end was told of, which it forgot too.
fn forget(
    engine: &mut Engine,
    current: &mut Option<Current>,
    preview: &mut Option<NodeId>,
    materials: &mut Vec<NodeId>,
    memory: &mut Option<Memory>,
) {
    engine.reset();
    *current = None;
    *preview = None;
    materials.clear();
    *memory = None;
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
