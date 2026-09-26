//! Incremental evaluation of a graph on the GPU.
//!
//! [`Engine::evaluate`] walks the graph in topological order and computes a key per image
//! node: a hash of its type, effective parameters (from the CPU [`Evaluation`]),
//! resolution and the versions of its inputs. Only nodes whose key changed are recomputed,
//! so dragging a slider re-runs just the affected downstream chain. All work of one
//! evaluation is recorded into a single command encoder and submitted once.
//!
//! An image lives while something reads it (see [`Keep`]): the views, the nodes asked about,
//! and what the last change read from outside itself stay; every other image goes back to a
//! pool as soon as the evaluation that made it is done with it, and a later image of its size
//! takes its texture. Computed again with the same key, a node keeps its version, so nothing
//! after it is recomputed for it.
//!
//! An output makes the image it writes: what arrives at it, in its channels and at its bit
//! depth. That is what the views show of it and what is exported, and all that stays of
//! the graph's work once nothing changes (see [`Engine::settle`]).
//!
//! The engine is single threaded; [`crate::service`] runs it on a background thread.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use brewed_texture_core::eval::{Evaluation, VarSource};
use brewed_texture_core::graph::{Graph, Node, NodeId, input_socket_key};
use brewed_texture_core::nodes::{ImageKind, NodeKind, PIXEL_MATH_BUILTINS, Role, image_kind};
use brewed_texture_core::types::DataType;

use crate::gpu::{COLOR_FORMAT, GRAY_FORMAT, GRAY_FULL_FORMAT, Gpu, GpuImage, Readback, StoredImage, filterable, format_of, output_format};
use brewed_texture_core::Precision;
use crate::kernels::{self, DisplayU, ExecCtx, PixelVar};
use crate::simd::{self, ImageStats};
use crate::vector::{self, Raster, Vector, VectorJob};

pub const THUMB_SIZE: u32 = 128;
/// Samples in a baked gradient or curve.
pub const LUT_SIZE: usize = 1024;
/// The size the pictures of a node's options are drawn at before they are made into
/// thumbnails: fine enough for their look, small enough to draw them all at once.
pub const CHOICE_RES: u32 = 256;

/// What the engine knows about one node.
#[derive(Default)]
pub struct NodeState {
    /// Its image, while something reads it (see [`Keep`]). A bypassed node has none of its
    /// own: it shows the image of its `producer`.
    image: Option<GpuImage>,
    /// For a bypassed node, the node whose image arrives at it (through other bypassed
    /// nodes).
    producer: Option<NodeId>,
    /// Grayscale or color; `None` while the node makes no image.
    kind: Option<ImageKind>,
    /// The size of its image (of the image it shows, for a bypassed node).
    size: [u32; 2],
    /// The format of its image (of the image it shows, for a bypassed node).
    format: Option<wgpu::TextureFormat>,
    /// Bumped whenever the image changes.
    pub version: u64,
    key: u64,
    /// Problems found while computing the image (in addition to the CPU evaluation's).
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// Per-node result sent to front ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageReport {
    pub version: u64,
    pub error: Option<String>,
    pub warning: Option<String>,
    /// Grayscale or color (none while the node has no image).
    pub kind: Option<ImageKind>,
}

/// GPU memory the engine's images take, by what they are for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// The images the 2D and 3D views show, with their mips.
    pub shown: u64,
    /// How many images that is.
    pub shown_images: usize,
    /// Images kept for the next change and those asked about (kept as [`Keep::Read`], they
    /// go once nothing changes for a moment); every other image, kept as [`Keep::All`].
    pub kept: u64,
    /// Scratch images the last change worked in, kept for the next one while changes keep
    /// coming.
    pub scratch: u64,
    /// Loaded files, and baked gradients and curves.
    pub files: u64,
}

impl Memory {
    pub fn total(&self) -> u64 {
        self.shown + self.kept + self.scratch + self.files
    }
}

/// What an evaluation did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EvalReport {
    /// Image nodes computed again (the others were up to date).
    pub recomputed: usize,
    pub image_nodes: usize,
}

/// A small display image of a node (RGBA8, composited over a checkerboard).
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub node: NodeId,
    pub version: u64,
    pub size: u32,
    pub rgba: Vec<u8>,
}

/// What a node of a kind makes with each option of one of its choices, everything else as
/// it is when the node is added, as thumbnails (RGBA8, [`THUMB_SIZE`] square, composited
/// over a checkerboard): a menu of the options can show how each one looks rather than only
/// its name.
#[derive(Clone, Debug)]
pub struct ChoicePictures {
    pub kind: NodeKind,
    pub key: String,
    pub size: u32,
    /// One picture per option, in the order of the options.
    pub pictures: Arc<Vec<Vec<u8>>>,
}

/// What a view shows of a node: its image itself, with its mips, which the 2D and 3D views
/// sample directly (no copy). An output's image is what the output writes.
#[derive(Clone, Debug)]
pub struct DisplayImage {
    pub node: NodeId,
    pub version: u64,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// The texture size the image stands for (a uniform color is one pixel, shown as big as
    /// any other image).
    pub size: [u32; 2],
    /// One channel, which is shown as gray (`widen` of [`crate::gpu::SHOW_WGSL`]).
    pub gray: bool,
}

enum BitmapEntry {
    /// Being decoded, by the decoding with this number (the file may have been asked for
    /// again since an earlier one started, which then counts no more).
    Loading(u64),
    Ready { image: GpuImage, generation: u64 },
    Failed(String),
}

/// A file's pixels, as they will be uploaded.
enum Pixels {
    /// One half float per pixel: the file is grayscale, of 8 bits, and so is its image.
    Gray(Vec<u16>),
    /// One float per pixel: a grayscale file of more than 8 bits (a 16-bit height map), kept
    /// in floats, all its steps.
    FineGray(Vec<f32>),
    /// RGBA half floats.
    Color(Vec<u16>),
}

struct DecodedBitmap {
    path: String,
    /// The number of the decoding (see [`BitmapEntry::Loading`]).
    ticket: u64,
    result: Result<(u32, u32, Pixels), String>,
}

/// The drawing of an SVG or a Text node (see [`crate::vector`]).
enum Drawing {
    /// Being drawn by the job with this number, of the picture with this key; with the node's
    /// last drawing, shown until then, and the picture asked for since, drawn next.
    Busy { ticket: u64, key: u64, last: Option<(GpuImage, u64)>, next: Option<VectorJob> },
    Ready { key: u64, image: GpuImage, generation: u64 },
    Failed { key: u64, error: String },
}

struct Drawn {
    node: NodeId,
    ticket: u64,
    key: u64,
    size: [u32; 2],
    result: Result<Raster, String>,
}

struct Lut {
    hash: u64,
    image: GpuImage,
}

/// What a node reads from a table baked on the CPU.
enum Table {
    Gradient(brewed_texture_core::Gradient),
    Curve(brewed_texture_core::Curve),
}

impl Table {
    fn hash_into(&self, h: &mut impl Hasher) {
        match self {
            Table::Gradient(g) => {
                0u8.hash(h);
                g.hash_into(h);
            }
            Table::Curve(c) => {
                1u8.hash(h);
                c.hash_into(h);
            }
        }
    }

    /// `n` entries, from 0 to 1 (both in): colors of the gradient; the curve's values, in red.
    fn bake(&self, n: usize) -> Vec<[f32; 4]> {
        match self {
            Table::Gradient(g) => g.bake(n),
            Table::Curve(c) => c.bake(n).into_iter().map(|v| [v, v, v, 1.0]).collect(),
        }
    }
}

/// Which images an engine keeps from one evaluation to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keep {
    /// Every node's: for a caller that reads whichever image it likes (tests, tools).
    #[default]
    All,
    /// Only those something reads: the node the 2D view shows, the outputs the 3D view uses,
    /// the nodes asked about, and what the last change read from outside itself (the next
    /// change of the same kind reads it again), until the engine settles. The others live
    /// while the evaluation that makes them needs them.
    Read,
}

/// What an evaluation found about an image node before computing anything.
struct Plan {
    node_kind: NodeKind,
    /// Its key changed: it has a new version, and its image has to be computed.
    changed: bool,
    /// The format of the image it computes; `None` when it computes none (a bypassed node, an
    /// output nothing arrives at).
    format: Option<wgpu::TextureFormat>,
    size: [u32; 2],
    /// The node whose image arrives at each input.
    inputs: Vec<Option<NodeId>>,
    /// Pixel Math's variables.
    vars: Vec<Var>,
    bitmap: Option<GpuImage>,
    lut: Option<GpuImage>,
    /// Problems found before computing anything.
    error: Option<String>,
    warning: Option<String>,
}

impl Plan {
    /// The nodes whose images it reads.
    fn reads(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.inputs.iter().flatten().copied().chain(self.vars.iter().filter_map(|v| match v {
            Var::Image(from) => Some(*from),
            _ => None,
        }))
    }
}

/// The image nodes of `order` (an order that has each after the nodes it reads) in the order
/// to compute them in: chain by chain, each node nothing reads (an output, say) right after
/// what it reads that is not computed yet. A chain is done with, and its images go back to
/// the pool, before the next one starts; in the order of the graph, all the outputs come
/// last, and every image before them waits for them.
fn by_chains(order: &[NodeId], plans: &HashMap<NodeId, Plan>) -> Vec<NodeId> {
    fn visit(id: NodeId, plans: &HashMap<NodeId, Plan>, done: &mut HashSet<NodeId>, out: &mut Vec<NodeId>) {
        let Some(plan) = plans.get(&id) else { return };
        if !done.insert(id) {
            return;
        }
        for read in plan.reads() {
            visit(read, plans, done, out);
        }
        out.push(id);
    }
    let read: HashSet<NodeId> = plans.values().flat_map(Plan::reads).collect();
    let mut done = HashSet::new();
    let mut out = Vec::with_capacity(order.len());
    for &id in order.iter().filter(|id| !read.contains(id)) {
        visit(id, plans, &mut done, &mut out);
    }
    out
}

/// Which images get mips when they are computed (see `Engine::evaluate`).
#[derive(Clone, Copy, PartialEq)]
enum Mips {
    /// Those a filter can read: a view shows the image, or a node filters it (what cannot be
    /// filtered is read through a copy that can, with mips of its own).
    Filtered,
    /// Whatever its format: a node reads it at every scale in the precision it has.
    Any,
}

/// A Pixel Math variable, as planned: the image of a node is read when it is computed.
enum Var {
    Builtin(&'static str),
    Image(NodeId),
    Constant(f32),
}

pub struct Engine {
    pub gpu: Gpu,
    states: HashMap<NodeId, NodeState>,
    next_version: u64,
    bitmaps: HashMap<String, BitmapEntry>,
    bitmap_generation: u64,
    bitmap_tx: Sender<DecodedBitmap>,
    bitmap_rx: Receiver<DecodedBitmap>,
    /// The drawings of the SVG and Text nodes, by node.
    drawings: HashMap<NodeId, Drawing>,
    /// How many times each file drawn from was read again (see [`Engine::reload_file`]).
    rereads: HashMap<String, u64>,
    drawn_tx: Sender<Drawn>,
    drawn_rx: Receiver<Drawn>,
    luts: HashMap<NodeId, Lut>,
    keep: Keep,
    /// What the 2D view shows, and what of it it was last given.
    preview: Option<NodeId>,
    preview_sent: Option<(NodeId, u64, wgpu::Texture)>,
    /// The outputs the 3D view uses, and what of them it was last given.
    materials: Vec<NodeId>,
    materials_sent: Vec<(NodeId, u64, wgpu::Texture)>,
    /// Nodes whose images are read after the next evaluation (see [`Engine::ask`]).
    asked: Vec<NodeId>,
    /// The textures of the images the views were given (see [`Engine::release`]).
    given: Vec<wgpu::Texture>,
    /// What the views show of images they cannot filter (floats of 32 bits), by the node that
    /// makes them: a copy in half floats with mips, and the version of the node it is of.
    shown_copies: HashMap<NodeId, (u64, GpuImage)>,
    /// What the last change read from outside itself (see [`Keep::Read`]).
    boundary: HashSet<NodeId>,
    /// Thumbnails the evaluations recorded, to read back (see [`Engine::render_thumbnails`]).
    thumbnails: Vec<(NodeId, u64, GpuImage, Readback)>,
    /// Pictures of the options of choices, by node kind and parameter (see
    /// [`Engine::render_choices`]).
    choices: HashMap<(&'static str, String), Arc<Vec<Vec<u8>>>>,
    last_resolution: [u32; 2],
    /// The precision of the last graph evaluated.
    last_precision: Precision,
    pub report: EvalReport,
}

impl Engine {
    pub fn new(gpu: Gpu) -> Engine {
        let (bitmap_tx, bitmap_rx) = channel();
        let (drawn_tx, drawn_rx) = channel();
        Engine {
            gpu,
            states: HashMap::new(),
            next_version: 1,
            bitmaps: HashMap::new(),
            bitmap_generation: 1,
            bitmap_tx,
            bitmap_rx,
            drawings: HashMap::new(),
            rereads: HashMap::new(),
            drawn_tx,
            drawn_rx,
            luts: HashMap::new(),
            keep: Keep::default(),
            preview: None,
            preview_sent: None,
            materials: Vec::new(),
            materials_sent: Vec::new(),
            asked: Vec::new(),
            given: Vec::new(),
            shown_copies: HashMap::new(),
            boundary: HashSet::new(),
            thumbnails: Vec::new(),
            choices: HashMap::new(),
            last_resolution: [0, 0],
            last_precision: Precision::default(),
            report: EvalReport::default(),
        }
    }

    pub fn state(&self, id: NodeId) -> Option<&NodeState> {
        self.states.get(&id)
    }

    /// What `id` shows now: its own image, or the image of what arrives at an output or a
    /// bypassed node. `None` while nothing reads it (see [`Keep`]).
    pub fn image(&self, id: NodeId) -> Option<&GpuImage> {
        let state = self.states.get(&id)?;
        match state.producer {
            Some(producer) => self.states.get(&producer)?.image.as_ref(),
            None => state.image.as_ref(),
        }
    }

    pub fn version(&self, id: NodeId) -> Option<u64> {
        self.states.get(&id).map(|s| s.version)
    }

    pub fn reports(&self) -> HashMap<NodeId, ImageReport> {
        self.states
            .iter()
            .map(|(id, s)| (*id, ImageReport { version: s.version, error: s.error.clone(), warning: s.warning.clone(), kind: s.kind }))
            .collect()
    }

    /// Are files still being decoded, or pictures drawn? (The graph must be evaluated again when
    /// they arrive.)
    pub fn files_loading(&self) -> bool {
        self.bitmaps.values().any(|b| matches!(b, BitmapEntry::Loading(_))) || self.drawings.values().any(|d| matches!(d, Drawing::Busy { .. }))
    }

    /// Drop every cached result (new document, device change ...).
    pub fn reset(&mut self) {
        self.states.clear();
        self.luts.clear();
        self.drawings.clear();
        self.preview_sent = None;
        self.materials_sent.clear();
        self.given.clear();
        self.shown_copies.clear();
        self.boundary.clear();
        self.thumbnails.clear();
        self.choices.clear();
        self.gpu.clear_pool();
    }

    /// GPU memory the engine's images take, in all (see [`Engine::memory_report`]).
    pub fn memory(&self) -> u64 {
        self.memory_report().total()
    }

    /// GPU memory the engine's images take, by what they are for.
    pub fn memory_report(&self) -> Memory {
        let shown = self.shown();
        let mut seen: Vec<wgpu::Texture> = Vec::new();
        let mut once = |image: &GpuImage| {
            if seen.contains(&image.texture) {
                return 0;
            }
            seen.push(image.texture.clone());
            image.bytes()
        };
        let mut memory = Memory { scratch: self.gpu.pool_bytes(), ..Memory::default() };
        for (id, state) in &self.states {
            let Some(image) = &state.image else { continue };
            let bytes = once(image);
            if shown.contains(id) {
                memory.shown += bytes;
                memory.shown_images += 1;
            } else {
                memory.kept += bytes;
            }
        }
        for (_, copy) in self.shown_copies.values() {
            memory.shown += once(copy);
        }
        let drawings = self.drawings.values().filter_map(|d| match d {
            Drawing::Ready { image, .. } | Drawing::Busy { last: Some((image, _)), .. } => Some(image),
            _ => None,
        });
        let files = self.luts.values().map(|l| &l.image).chain(drawings).chain(self.bitmaps.values().filter_map(|b| match b {
            BitmapEntry::Ready { image, .. } => Some(image),
            _ => None,
        }));
        for image in files {
            memory.files += once(image);
        }
        memory
    }

    /// Block until the GPU finished everything submitted so far.
    pub fn wait_idle(&self) {
        let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
    }

    // ------------------------------------------------------------------ bitmaps

    fn bitmap_for(&mut self, path: &str) -> Result<Option<(GpuImage, u64)>, String> {
        if path.trim().is_empty() {
            return Err("No file selected".into());
        }
        match self.bitmaps.get(path) {
            Some(BitmapEntry::Ready { image, generation }) => Ok(Some((image.clone(), *generation))),
            Some(BitmapEntry::Failed(e)) => Err(e.clone()),
            Some(BitmapEntry::Loading(_)) => Ok(None),
            None => {
                self.bitmap_generation += 1;
                let ticket = self.bitmap_generation;
                self.bitmaps.insert(path.to_string(), BitmapEntry::Loading(ticket));
                let tx = self.bitmap_tx.clone();
                let path = path.to_string();
                std::thread::spawn(move || {
                    let result = decode_bitmap(&path);
                    let _ = tx.send(DecodedBitmap { path, ticket, result });
                });
                Ok(None)
            }
        }
    }

    /// Read a file again (edited elsewhere): a picture is decoded again, and what is drawn from
    /// it (an SVG, a font) is drawn again, each drawing showing its last one meanwhile.
    pub fn reload_file(&mut self, path: &str) {
        self.bitmaps.remove(path);
        *self.rereads.entry(path.to_string()).or_default() += 1;
    }

    /// The drawing an SVG or Text node shows for `job` (see [`crate::vector`]): drawn in the
    /// background, on every core; meanwhile the node's last drawing, if it has one, so that a
    /// value dragged never leaves it empty. While one is being drawn, only the last picture
    /// asked for is drawn next. `(image, generation)`.
    fn drawing_for(&mut self, node: NodeId, job: VectorJob) -> Result<Option<(GpuImage, u64)>, String> {
        let key = job.key();
        match self.drawings.get_mut(&node) {
            Some(Drawing::Ready { key: k, image, generation }) if *k == key => return Ok(Some((image.clone(), *generation))),
            Some(Drawing::Failed { key: k, error }) if *k == key => return Err(error.clone()),
            Some(Drawing::Busy { key: k, last, next, .. }) => {
                // Being drawn: this one next, unless it is the one being drawn.
                *next = (*k != key).then_some(job);
                return Ok(last.clone());
            }
            _ => {}
        }
        let last = match self.drawings.get(&node) {
            Some(Drawing::Ready { image, generation, .. }) => Some((image.clone(), *generation)),
            _ => None,
        };
        self.draw(node, job, last.clone());
        Ok(last)
    }

    fn draw(&mut self, node: NodeId, job: VectorJob, last: Option<(GpuImage, u64)>) {
        self.bitmap_generation += 1;
        let ticket = self.bitmap_generation;
        let key = job.key();
        self.drawings.insert(node, Drawing::Busy { ticket, key, last, next: None });
        let tx = self.drawn_tx.clone();
        std::thread::spawn(move || {
            let result = vector::rasterize(&job);
            let _ = tx.send(Drawn { node, ticket, key, size: job.size, result });
        });
    }

    /// Upload decoded files and drawn pictures. Returns true if any arrived (evaluate again).
    pub fn receive_files(&mut self) -> bool {
        let mut any = self.receive_drawings();
        while let Ok(done) = self.bitmap_rx.try_recv() {
            // A file no node reads any more, or asked for again since: not uploaded.
            if !matches!(self.bitmaps.get(&done.path), Some(BitmapEntry::Loading(t)) if *t == done.ticket) {
                continue;
            }
            let entry = match done.result {
                Ok((w, h, pixels)) => {
                    let image = match pixels {
                        Pixels::Gray(half) => {
                            let image = self.gpu.create_image_as(w, h, GRAY_FORMAT, "bitmap");
                            self.gpu.upload_gray_f16(&image, &half);
                            image
                        }
                        Pixels::FineGray(values) => {
                            let image = self.gpu.create_image_as(w, h, GRAY_FULL_FORMAT, "bitmap");
                            self.gpu.upload_floats(&image, &values);
                            image
                        }
                        Pixels::Color(half) => {
                            let image = self.gpu.create_image(w, h, "bitmap");
                            self.gpu.upload_rgba_f16(&image, &half);
                            image
                        }
                    };
                    self.bitmap_generation += 1;
                    BitmapEntry::Ready { image, generation: self.bitmap_generation }
                }
                Err(e) => BitmapEntry::Failed(e),
            };
            self.bitmaps.insert(done.path, entry);
            any = true;
        }
        any
    }

    fn receive_drawings(&mut self) -> bool {
        let mut any = false;
        while let Ok(done) = self.drawn_rx.try_recv() {
            // A node gone, or a picture asked for again since.
            let Some(Drawing::Busy { ticket, next, last, .. }) = self.drawings.get_mut(&done.node) else { continue };
            if *ticket != done.ticket {
                continue;
            }
            let (next, last) = (next.take(), last.take());
            let [w, h] = done.size;
            let drawn = match done.result {
                Ok(Raster::Gray(values)) => {
                    let image = self.gpu.create_image_as(w, h, GRAY_FORMAT, "drawing");
                    self.gpu.upload_gray_f16(&image, &values);
                    Ok(image)
                }
                Ok(Raster::Color(values)) => {
                    let image = self.gpu.create_image(w, h, "drawing");
                    self.gpu.upload_rgba_f16(&image, &values);
                    Ok(image)
                }
                Err(e) => Err(e),
            };
            self.bitmap_generation += 1;
            let generation = self.bitmap_generation;
            match (drawn, next) {
                // Drawn, and another asked for since: that one now, this one shown meanwhile.
                (Ok(image), Some(next)) => self.draw(done.node, next, Some((image, generation))),
                (Err(_), Some(next)) => self.draw(done.node, next, last),
                (Ok(image), None) => {
                    self.drawings.insert(done.node, Drawing::Ready { key: done.key, image, generation });
                }
                (Err(error), None) => {
                    self.drawings.insert(done.node, Drawing::Failed { key: done.key, error });
                }
            }
            any = true;
        }
        any
    }

    // ------------------------------------------------------------------ evaluation

    /// Bring every image up to date with `graph`. `eval` is the CPU evaluation of the
    /// same graph (values and effective parameters).
    pub fn evaluate(&mut self, graph: &Graph, eval: &Evaluation) -> EvalReport {
        self.receive_files();
        self.gpu.start_use();

        // Forget nodes that were deleted, and let go of their images.
        let alive: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id).collect();
        let gone: Vec<NodeId> = self.states.keys().filter(|id| !alive.contains(id)).copied().collect();
        for id in gone {
            self.release(id);
            self.states.remove(&id);
        }
        self.luts.retain(|id, _| alive.contains(id));
        self.boundary.retain(|id| alive.contains(id));
        // Files no Bitmap node reads any more go, whatever was open before (a file read again
        // is decoded again).
        let files: HashSet<&str> = graph
            .nodes_of(NodeKind::BITMAP)
            .map(|n| eval.params(n.id).map_or(n.text("path"), |p| p.text("path")))
            .collect();
        self.bitmaps.retain(|path, _| files.contains(path.as_str()));
        // The drawings of nodes that draw none any more go too.
        self.drawings.retain(|id, _| graph.node(*id).is_some_and(|n| n.kind == NodeKind::SVG || n.kind == NodeKind::TEXT));
        for id in &eval.cyclic {
            self.release(*id);
            let state = self.states.entry(*id).or_default();
            state.kind = None;
            state.error = None;
        }

        let resolution = graph.resolution.map(|r| r.clamp(16, 8192));
        if resolution != self.last_resolution || graph.precision != self.last_precision {
            // Scratch images of the old size or format would only waste GPU memory.
            self.gpu.clear_pool();
            self.last_resolution = resolution;
            self.last_precision = graph.precision;
        }

        // 1. What every image node will be, before anything is computed: its key, from its
        //    parameters and the versions of what arrives at it, and the kind and size of its
        //    image. A node whose key changed gets its new version now, so the keys of the
        //    nodes after it see it.
        let mut plans: HashMap<NodeId, Plan> = HashMap::new();
        let mut order = Vec::new();
        for &id in &eval.order {
            let Some(node) = graph.node(id) else { continue };
            if !node.def().has_image() {
                continue;
            }
            if let Some(plan) = self.plan(graph, eval, node, resolution) {
                order.push(id);
                plans.insert(id, plan);
            }
        }
        let order = by_chains(&order, &plans);

        // 2. What to compute, from the last node back: a node that changed, and a node whose
        //    image is needed and not there, by a node computed after it or by what reads it
        //    once this evaluation is done. A bypassed node computes nothing, but a changed one
        //    reads what arrives at it for its thumbnail, and one that is needed needs it.
        let wanted: HashSet<NodeId> = match self.keep {
            Keep::All => order.iter().copied().collect(),
            Keep::Read => self.preview.iter().chain(&self.materials).chain(&self.asked).copied().collect(),
        };
        let mut needed = wanted.clone();
        let mut runs: HashSet<NodeId> = HashSet::new();
        for &id in order.iter().rev() {
            let plan = &plans[&id];
            let makes = plan.format.is_some();
            let missing = self.states.get(&id).is_none_or(|s| s.image.is_none());
            let run = plan.changed || (makes && needed.contains(&id) && missing);
            if run || (!makes && needed.contains(&id)) {
                needed.extend(plan.reads());
            }
            if run {
                runs.insert(id);
            }
        }

        // What stays once this evaluation is done: what is wanted, and what the changed part of
        // the graph read from outside itself, which the next change of the same kind (the
        // same slider dragged on) reads again.
        let changed: HashSet<NodeId> = order.iter().filter(|id| plans[*id].changed).copied().collect();
        if !changed.is_empty() {
            self.boundary = changed.iter().flat_map(|id| plans[id].reads()).filter(|r| !changed.contains(r)).collect();
        }
        let kept: HashSet<NodeId> = match self.keep {
            Keep::All => order.iter().copied().collect(),
            Keep::Read => self.kept(),
        };

        // 3. Compute, in order. Each image goes back to the pool (for the next image of its
        //    size) once the last node that reads it here has read it, unless it stays.
        let mut readers: HashMap<NodeId, usize> = HashMap::new();
        for id in &runs {
            for read in plans[id].reads() {
                *readers.entry(read).or_default() += 1;
            }
        }
        // With mips: what the views show and what a node reads at every scale through a filter,
        // where it can be filtered (see `compute`); what a node reads at every scale in the
        // precision it has, whatever it is. Such a node reads it as it is rather than a copy.
        let mut mipped: HashMap<NodeId, Mips> = self.shown().into_iter().map(|id| (id, Mips::Filtered)).collect();
        for id in &runs {
            let Some((at_every_scale, filtered)) = kernels::reads_at_every_scale(plans[id].node_kind) else { continue };
            for input in plans[id].inputs.get(at_every_scale).unwrap_or_default().iter().flatten() {
                let mips = mipped.entry(*input).or_insert(Mips::Filtered);
                if !filtered {
                    *mips = Mips::Any;
                }
            }
        }
        let mut encoder = self.gpu.encoder();
        let mut report = EvalReport { image_nodes: order.len(), ..EvalReport::default() };
        for &id in &order {
            if !runs.contains(&id) {
                continue;
            }
            let plan = &plans[&id];
            if let Some(format) = plan.format {
                let Some(ne) = eval.node(id) else { continue };
                self.compute(id, plan, format, &ne.params, ne.error(), mipped.get(&id).copied(), &mut encoder);
                report.recomputed += 1;
            }
            // Nodes laid out from graphs used as nodes are not on the canvas: no thumbnails.
            if plan.changed && !id.is_inner() {
                self.encode_thumbnail(id, &mut encoder);
            }
            for read in plan.reads() {
                let left = readers.entry(read).or_default();
                *left = left.saturating_sub(1);
                if *left == 0 && !kept.contains(&read) {
                    self.release(read);
                }
            }
            // A node nothing reads after it, and that does not stay, only needed its thumbnail.
            if readers.get(&id).is_none_or(|n| *n == 0) && !kept.contains(&id) {
                self.release(id);
            }
        }

        // 4. Let go of what stayed from before and does not stay any more.
        self.sweep();
        self.gpu.submit(encoder);
        self.report = report;
        report
    }

    /// The node whose image `node` shows: itself, or, for a bypassed node, what arrives at it.
    fn producer(&self, node: NodeId) -> NodeId {
        self.states.get(&node).and_then(|s| s.producer).unwrap_or(node)
    }

    /// The nodes whose images the 2D and 3D views show.
    fn shown(&self) -> HashSet<NodeId> {
        self.preview.iter().chain(&self.materials).map(|id| self.producer(*id)).collect()
    }

    /// Let go of a node's image: it goes back to the pool, for the next image of its size.
    /// Not an image a view was given: the view draws it until it is given another, which it
    /// may not be yet, and keeps it until then; no other image may take its texture meanwhile.
    fn release(&mut self, node: NodeId) {
        let Some(image) = self.states.get_mut(&node).and_then(|s| s.image.take()) else { return };
        match self.given.iter().position(|t| *t == image.texture) {
            Some(i) => {
                self.given.swap_remove(i);
            }
            None => self.gpu.recycle(image),
        }
    }

    /// Which images this engine keeps from one evaluation to the next (see [`Keep`]).
    pub fn set_keep(&mut self, keep: Keep) {
        self.keep = keep;
    }

    /// The node the 2D view shows: its image is read, and stays.
    pub fn set_preview(&mut self, node: Option<NodeId>) {
        self.preview = node;
    }

    /// The outputs the 3D view uses: their images are read, and stay.
    pub fn set_materials(&mut self, nodes: &[NodeId]) {
        self.materials = nodes.to_vec();
    }

    /// Nodes whose images are read after the next evaluation (their statistics, a pixel of
    /// them, their export): each is computed if it is not there, and stays until an
    /// evaluation that is not asked for it, or until the engine settles.
    pub fn ask(&mut self, nodes: Vec<NodeId>) {
        self.asked = nodes;
    }

    /// The nodes whose images stay, kept as [`Keep::Read`]: what the views show, what is asked
    /// about, and what the last change read from outside itself.
    fn kept(&self) -> HashSet<NodeId> {
        let read = self.preview.iter().chain(&self.materials).chain(&self.asked);
        read.map(|id| self.producer(*id)).chain(self.boundary.iter().copied()).collect()
    }

    /// Let go of the images that no longer stay (kept as [`Keep::Read`]): what a view stopped
    /// showing, what is not asked about any more. An evaluation does it itself; this is for
    /// a view that moved on to an image that is there, which needs no evaluation.
    pub fn sweep(&mut self) {
        let shown = self.shown();
        self.shown_copies.retain(|id, _| shown.contains(id));
        if self.keep == Keep::All {
            return;
        }
        let kept = self.kept();
        let stale: Vec<NodeId> =
            self.states.iter().filter(|(id, s)| s.image.is_some() && !kept.contains(id)).map(|(id, _)| *id).collect();
        for id in stale {
            self.release(id);
        }
    }

    /// Keep the scratch images the next change is likely to need when one is `coming` (a
    /// slider being dragged): those the last round used, the smallest first, as many as fit in
    /// the memory of six grayscale images of the texture size. A new image costs its
    /// allocation whatever its size and then some for each byte: small ones are worth keeping,
    /// and a color image of the texture size is made again, for the memory it would take in
    /// between. With no change coming, none of them is kept.
    pub fn keep_scratch(&self, coming: bool) {
        if coming {
            let [w, h] = self.last_resolution;
            let pixel = format_of(ImageKind::Gray, self.last_precision).block_copy_size(None).unwrap_or(4);
            self.gpu.trim_pool(6 * u64::from(w) * u64::from(h) * u64::from(pixel));
        } else {
            self.gpu.clear_pool();
        }
    }

    /// Nothing has changed for a while: let go of everything the next change would have read
    /// and that the views do not show (kept as [`Keep::Read`]), and of the scratch images. What
    /// stays is what the views show, whatever was changed before.
    pub fn settle(&mut self) {
        self.boundary.clear();
        self.asked.clear();
        self.sweep();
        self.gpu.clear_pool();
    }

    /// Whether an evaluation has to run for all of `nodes` to have images.
    pub fn missing(&self, nodes: &[NodeId]) -> bool {
        nodes.iter().any(|id| self.states.get(id).is_some_and(|s| s.kind.is_some()) && self.image(*id).is_none())
    }

    /// Plan one image node (see [`Engine::evaluate`]): what it reads, the kind and size of its
    /// image, and its key; a new version when the key changed.
    fn plan(&mut self, graph: &Graph, eval: &Evaluation, node: &Node, resolution: [u32; 2]) -> Option<Plan> {
        let d = node.def();
        let ne = eval.node(node.id)?;
        let params = &ne.params;

        let mut hasher = DefaultHasher::new();
        node.kind.id().hash(&mut hasher);
        resolution.hash(&mut hasher);
        node.bypass.hash(&mut hasher);
        params.hash_into(&mut hasher);

        // What arrives at the inputs: an image of the node it comes from (made by the node
        // behind it, through bypassed nodes), and the version of that node. A graph used as a
        // node is laid out in the graph before it comes here (see
        // `brewed_texture_core::package::flatten`): what arrives at it is what it shows.
        let fixed: Vec<String> = match d.role {
            Role::Instance => graph.input_sockets(node).into_iter().filter(|s| s.ty == DataType::Image).map(|s| s.key).collect(),
            _ => d.inputs.iter().map(|s| input_socket_key(s.key)).collect(),
        };
        let mut inputs: Vec<Option<NodeId>> = Vec::with_capacity(fixed.len());
        let mut input_kinds: Vec<Option<ImageKind>> = Vec::with_capacity(fixed.len());
        let mut input_sizes: Vec<Option<[u32; 2]>> = Vec::with_capacity(fixed.len());
        let mut input_formats: Vec<Option<wgpu::TextureFormat>> = Vec::with_capacity(fixed.len());
        for key in &fixed {
            let from = graph.link_into(node.id, key).map(|l| l.from);
            match from.and_then(|f| self.states.get(&f).filter(|st| st.kind.is_some()).map(|st| (f, st))) {
                Some((f, st)) => {
                    (1u8, f.0, st.version).hash(&mut hasher);
                    inputs.push(Some(st.producer.unwrap_or(f)));
                    input_kinds.push(st.kind);
                    input_sizes.push(Some(st.size));
                    input_formats.push(st.format);
                }
                None => {
                    0u8.hash(&mut hasher);
                    inputs.push(None);
                    input_kinds.push(None);
                    input_sizes.push(None);
                    input_formats.push(None);
                }
            }
        }

        let mut error: Option<String> = ne.error().map(str::to_string);
        let mut warning: Option<String> = None;

        // File of a Bitmap node.
        let mut bitmap: Option<GpuImage> = None;
        if node.kind == NodeKind::BITMAP && !node.bypass {
            match self.bitmap_for(params.text("path")) {
                Ok(Some((img, generation))) => {
                    generation.hash(&mut hasher);
                    bitmap = Some(img);
                }
                Ok(None) => {
                    warning = Some("Loading…".into());
                    u64::MAX.hash(&mut hasher);
                }
                Err(e) => {
                    e.hash(&mut hasher);
                    error = Some(e);
                }
            }
        }

        // Drawing of an SVG or a Text node, at the texture's size.
        if (node.kind == NodeKind::SVG || node.kind == NodeKind::TEXT) && !node.bypass {
            let mut job = vector_job(node.kind, params, resolution);
            job.reads = job.file().and_then(|f| self.rereads.get(f)).copied().unwrap_or(0);
            match self.drawing_for(node.id, job) {
                Ok(Some((img, generation))) => {
                    generation.hash(&mut hasher);
                    bitmap = Some(img);
                }
                Ok(None) => {
                    warning = Some("Drawing…".into());
                    u64::MAX.hash(&mut hasher);
                }
                Err(e) => {
                    e.hash(&mut hasher);
                    error = Some(e);
                }
            }
        }

        // The gradient of a Gradient Map node, the curve of a Curve node: baked into a table.
        let mut lut: Option<GpuImage> = None;
        let table = if node.kind == NodeKind::GRADIENT_MAP {
            Some(Table::Gradient(params.gradient("gradient")))
        } else if node.kind == NodeKind::CURVE {
            Some(Table::Curve(params.curve("curve")))
        } else {
            None
        };
        if let Some(table) = table {
            let mut th = DefaultHasher::new();
            table.hash_into(&mut th);
            let thash = th.finish();
            let current = self.luts.get(&node.id).filter(|l| l.hash == thash).map(|l| l.image.clone());
            let image = current.unwrap_or_else(|| {
                let image = self.gpu.create_lut(&table.bake(LUT_SIZE));
                self.luts.insert(node.id, Lut { hash: thash, image: image.clone() });
                image
            });
            lut = Some(image);
        }

        // Pixel Math variables.
        let mut vars = Vec::new();
        if d.is_pixel_expression() {
            for v in &ne.vars {
                let var = match &v.source {
                    VarSource::Builtin => {
                        let b = PIXEL_MATH_BUILTINS.iter().copied().find(|b| *b == v.name).unwrap_or("u");
                        (2u8, b).hash(&mut hasher);
                        Var::Builtin(b)
                    }
                    VarSource::Image(from) => match self.states.get(from).filter(|s| s.kind.is_some()) {
                        Some(st) => {
                            (1u8, from.0, st.version).hash(&mut hasher);
                            Var::Image(st.producer.unwrap_or(*from))
                        }
                        None => {
                            3u8.hash(&mut hasher);
                            Var::Constant(0.0)
                        }
                    },
                    _ => {
                        (0u8, (v.value as f32).to_bits()).hash(&mut hasher);
                        Var::Constant(v.value as f32)
                    }
                };
                vars.push(var);
            }
        }

        // A bypassed node passes on what arrives at it, and makes no image of its own; so does a
        // graph used as a node, and Grayscale where a grayscale image arrives (the value of it
        // is its one value in every mode but Alpha: the grayscale input of a graph used as a
        // node is one, see `brewed_texture_core::package::flatten`). An output makes the image
        // it writes: what arrives at it (nothing, when nothing does), in its channels and at its
        // bit depth, as big as that is. Any other node makes a grayscale or a color image as
        // big as the texture size; but a uniform color is the same everywhere, and one pixel of
        // it is all of it.
        const GRAYSCALE_OF_ALPHA: u32 = 5;
        let gray_already = node.kind == NodeKind::GRAYSCALE
            && input_kinds.first() == Some(&Some(ImageKind::Gray))
            && params.u("mode") != GRAYSCALE_OF_ALPHA;
        let alias = (node.bypass && !fixed.is_empty()) || d.role == Role::Instance || gray_already;
        let arriving = input_kinds.first().copied().flatten();
        let arriving_size = input_sizes.first().copied().flatten().unwrap_or(resolution);
        let arriving_format = input_formats.first().copied().flatten();
        let (kind, size, format) = if alias {
            (arriving, arriving_size, None)
        } else if node.kind == NodeKind::OUTPUT {
            let formats = |k| output_format(params.u("format"), params.u("bit_depth"), k, arriving_format);
            (arriving, arriving_size, arriving.map(formats))
        } else {
            let kind = match &bitmap {
                Some(file) if node.kind == NodeKind::BITMAP => file.kind(),
                _ => image_kind(node.kind, params, &input_kinds),
            };
            let uniform = node.kind == NodeKind::UNIFORM_COLOR || node.kind == NodeKind::IMAGE_INPUT;
            let size = if uniform { [1, 1] } else { resolution };
            let precision = Precision::of_node(params.u("precision"), graph.precision);
            (Some(kind), size, Some(format_of(kind, precision)))
        };
        (size, format).hash(&mut hasher);
        let key = hasher.finish();

        let version = self.next_version + 1;
        let state = self.states.entry(node.id).or_default();
        let changed = state.version == 0 || state.key != key;
        state.key = key;
        state.kind = kind;
        state.size = size;
        state.format = if alias { arriving_format } else { format };
        state.producer = if alias { inputs.first().copied().flatten() } else { None };
        if alias {
            state.error = None;
            state.warning = None;
        }
        if changed {
            state.version = version;
        }
        // An image that stayed from before and does not fit any more is let go of.
        let fits = state.image.as_ref().is_none_or(|img| img.size() == size && Some(img.format()) == format);
        if changed {
            self.next_version = version;
        }
        if !fits {
            self.release(node.id);
        }
        Some(Plan { node_kind: node.kind, changed, format, size, inputs, vars, bitmap, lut, error, warning })
    }

    /// Compute a node's image in `format` (see [`Engine::evaluate`]): into its own image when
    /// it has one that fits, else into one from the pool; with mips when `mips` says (a view
    /// shows it, or a node reads it at every scale).
    #[allow(clippy::too_many_arguments)]
    fn compute(
        &mut self,
        id: NodeId,
        plan: &Plan,
        format: wgpu::TextureFormat,
        params: &brewed_texture_core::eval::Params,
        cpu_error: Option<&str>,
        mips: Option<Mips>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let [w, h] = plan.size;
        // (What cannot be filtered is shown, and filtered, through a copy that can, with its
        // own mips.)
        let mipped = w.max(h) > 1
            && match mips {
                Some(Mips::Filtered) => filterable(format),
                Some(Mips::Any) => true,
                None => false,
            };
        let own = self.states.get(&id).and_then(|s| s.image.clone());
        let image = match own {
            Some(img) if (img.texture.mip_level_count() > 1) == mipped => img,
            other => {
                if other.is_some() {
                    self.release(id);
                }
                if mipped { self.gpu.take_scratch_with_mips(w, h, format) } else { self.gpu.take_scratch(w, h, format) }
            }
        };
        let output = if mipped { image.level(0) } else { image.clone() };

        let inputs: Vec<Option<GpuImage>> = plan.inputs.iter().map(|i| i.and_then(|i| self.image(i).cloned())).collect();
        let pixel_vars: Vec<PixelVar> = plan
            .vars
            .iter()
            .map(|v| match v {
                Var::Builtin(b) => PixelVar::Builtin(b),
                Var::Constant(c) => PixelVar::Constant(*c),
                Var::Image(from) => match self.image(*from) {
                    Some(img) => PixelVar::Image(img.clone()),
                    None => PixelVar::Constant(0.0),
                },
            })
            .collect();
        let mut error = plan.error.clone();
        let mut warning = plan.warning.clone();
        if error.is_none() {
            let result = match kernels::kernel_for(plan.node_kind) {
                Some(kernel) => {
                    let mut cx = ExecCtx {
                        kind: plan.node_kind,
                        gpu: &self.gpu,
                        encoder: &mut *encoder,
                        params,
                        inputs: &inputs,
                        output: &output,
                        bitmap: plan.bitmap.as_ref(),
                        lut: plan.lut.as_ref(),
                        pixel_vars: &pixel_vars,
                    };
                    kernel(&mut cx)
                }
                None => Err(format!("No GPU kernel for '{}'", plan.node_kind.def().name)),
            };
            match result {
                Ok(w) => {
                    if warning.is_none() {
                        warning = w;
                    }
                }
                Err(e) => error = Some(e),
            }
        }
        if let Some(e) = &error {
            log::debug!("node {}: {e}", id.0);
            // Leave a transparent image so downstream nodes still work.
            self.gpu.encode_clear(encoder, &output);
        }
        if mipped && let Err(e) = self.gpu.encode_mips(encoder, &image) {
            error = error.or(Some(e));
        }

        let state = self.states.entry(id).or_default();
        state.image = Some(image);
        // Errors of the CPU evaluation are already reported by it.
        state.error = error.filter(|e| cpu_error != Some(e.as_str()));
        state.warning = warning;
    }

    // ------------------------------------------------------------------ thumbnails

    /// Record the thumbnail of a node that changed, from its image as it is now (before the
    /// image goes back to the pool); [`Engine::render_thumbnails`] reads it back.
    fn encode_thumbnail(&mut self, id: NodeId, encoder: &mut wgpu::CommandEncoder) {
        let Some(version) = self.version(id) else { return };
        let Some(image) = self.image(id).cloned() else { return };
        let params = DisplayU { mode: 0, checker: 8.0, pad: [0; 2] };
        let thumb = self.gpu.take_scratch(THUMB_SIZE, THUMB_SIZE, COLOR_FORMAT);
        if self.gpu.run(encoder, "thumbnail", bytemuck::bytes_of(&params), &[&image], &thumb).is_err() {
            self.gpu.recycle(thumb);
            return;
        }
        let readback = self.gpu.encode_readback(encoder, &thumb);
        // A thumbnail not read back yet is out of date.
        if let Some(i) = self.thumbnails.iter().position(|(node, ..)| *node == id) {
            let (_, _, old, _) = self.thumbnails.remove(i);
            self.gpu.recycle(old);
        }
        self.thumbnails.push((id, version, thumb, readback));
    }

    /// The thumbnails of the nodes the evaluations since the last call changed. Blocks until
    /// they are read back.
    pub fn render_thumbnails(&mut self) -> Vec<Thumbnail> {
        let jobs = std::mem::take(&mut self.thumbnails);
        if jobs.is_empty() {
            return Vec::new();
        }
        for (_, _, _, rb) in &jobs {
            rb.map();
        }
        self.wait_idle();
        let mut out = Vec::with_capacity(jobs.len());
        for (node, version, thumb, rb) in jobs {
            if let Some(Ok(())) = rb.poll() {
                let rgba = rb.to_f32();
                let mut bytes = vec![0u8; rgba.len()];
                simd::quantize_u8(&rgba, &mut bytes);
                out.push(Thumbnail { node, version, size: THUMB_SIZE, rgba: bytes });
            }
            self.gpu.recycle(thumb);
        }
        out
    }

    // ------------------------------------------------------------------ pictures of options

    /// What a node of `kind` makes with each option of its choice `key`, everything else as
    /// it is when such a node is added and nothing at its inputs: the same for every such
    /// node in every graph, so they are drawn once and kept. Drawn at [`CHOICE_RES`] and made
    /// into thumbnails like a node's own. Blocks until they are read back.
    pub fn render_choices(&mut self, kind: NodeKind, key: &str) -> Option<ChoicePictures> {
        let d = kind.def();
        let options = d.param(key)?.enum_options().len();
        // Nodes that need more than parameters (a file, a baked gradient, variables per
        // pixel) have no choices worth a picture.
        let plain = d.has_image() && !d.is_pixel_expression() && ![NodeKind::BITMAP, NodeKind::GRADIENT_MAP, NodeKind::CURVE].contains(&kind);
        if options == 0 || !plain {
            return None;
        }
        let cache = (kind.id(), key.to_string());
        if let Some(pictures) = self.choices.get(&cache) {
            return Some(ChoicePictures { kind, key: key.to_string(), size: THUMB_SIZE, pictures: pictures.clone() });
        }
        let kernel = kernels::kernel_for(kind)?;
        // The node as it is added, alone in a graph.
        let mut graph = Graph { resolution: [CHOICE_RES, CHOICE_RES], ..Graph::default() };
        let id = graph.add_node(kind, [0.0, 0.0]);
        let eval = brewed_texture_core::eval::evaluate(&graph);
        let mut params = eval.node(id)?.params.clone();
        let choice = params.values.iter().position(|(k, _)| k == key)?;
        let inputs: Vec<Option<GpuImage>> = vec![None; d.inputs.len()];

        let display = DisplayU { mode: 0, checker: 8.0, pad: [0; 2] };
        let input_kinds: Vec<Option<ImageKind>> = vec![None; inputs.len()];
        let mut encoder = self.gpu.encoder();
        let mut jobs = Vec::with_capacity(options);
        let mut drawn = Vec::new();
        for option in 0..options {
            params.values[choice].1 = brewed_texture_core::ParamValue::Enum(option as u32);
            // Drawn as the node draws itself, in the kind of image it makes with the option.
            let format = format_of(image_kind(kind, &params, &input_kinds), Precision::Half);
            let picture = self.gpu.take_scratch(CHOICE_RES, CHOICE_RES, format);
            let mut cx = ExecCtx {
                kind,
                gpu: &self.gpu,
                encoder: &mut encoder,
                params: &params,
                inputs: &inputs,
                output: &picture,
                bitmap: None,
                lut: None,
                pixel_vars: &[],
            };
            if kernel(&mut cx).is_err() {
                self.gpu.encode_clear(&mut encoder, &picture);
            }
            let thumb = self.gpu.take_scratch(THUMB_SIZE, THUMB_SIZE, COLOR_FORMAT);
            let _ = self.gpu.run(&mut encoder, "thumbnail", bytemuck::bytes_of(&display), &[&picture], &thumb);
            let readback = self.gpu.encode_readback(&mut encoder, &thumb);
            jobs.push((thumb, readback));
            drawn.push(picture);
        }
        self.gpu.submit(encoder);
        for (_, rb) in &jobs {
            rb.map();
        }
        self.wait_idle();
        let mut pictures = Vec::with_capacity(options);
        for (thumb, rb) in jobs {
            let mut bytes = vec![0u8; (THUMB_SIZE * THUMB_SIZE * 4) as usize];
            if let Some(Ok(())) = rb.poll() {
                simd::quantize_u8(&rb.to_f32(), &mut bytes);
            }
            pictures.push(bytes);
            self.gpu.recycle(thumb);
        }
        for picture in drawn {
            self.gpu.recycle(picture);
        }
        let pictures = Arc::new(pictures);
        self.choices.insert(cache, pictures.clone());
        Some(ChoicePictures { kind, key: key.to_string(), size: THUMB_SIZE, pictures })
    }

    // ------------------------------------------------------------------ what the views show

    /// What the views show of `node`: the image of the node that makes it (itself, or what
    /// arrives at a bypassed node), with the mips a view samples it with. `None` when there
    /// is no image.
    fn display(&mut self, node: NodeId) -> Option<DisplayImage> {
        let version = self.version(node)?;
        let producer = self.producer(node);
        let image = if self.image(producer).is_some_and(|img| filterable(img.format())) {
            self.ensure_mips(producer);
            let image = self.image(producer)?.clone();
            if !self.given.contains(&image.texture) {
                self.given.push(image.texture.clone());
            }
            image
        } else {
            self.shown_copy(producer)?
        };
        let size = self.last_resolution;
        Some(DisplayImage { node, version, size, gray: image.is_gray(), texture: image.texture, view: image.view })
    }

    /// What a view shows of a node's image that it cannot filter (floats of 32 bits): a copy of
    /// it in half floats with mips, made again when the node's image changes. The copy is a
    /// texture of its own, not from the pool: the view keeps it for as long as it draws it.
    fn shown_copy(&mut self, id: NodeId) -> Option<GpuImage> {
        let version = self.version(id)?;
        if let Some((v, copy)) = self.shown_copies.get(&id)
            && *v == version
        {
            return Some(copy.clone());
        }
        let image = self.image(id)?.clone();
        let [w, h] = image.size();
        let copy = self.gpu.create_image_with_mips(w, h, format_of(image.kind(), Precision::Half), "shown");
        let mut encoder = self.gpu.encoder();
        if let Err(e) = self.gpu.encode_copy_with_mips(&mut encoder, &image, &copy, 0) {
            log::error!("shown copy: {e}");
        }
        self.gpu.submit(encoder);
        self.shown_copies.insert(id, (version, copy.clone()));
        Some(copy)
    }

    /// Give a node's image the mips a view samples it with: the same image, in a texture that
    /// has them.
    fn ensure_mips(&mut self, id: NodeId) {
        let Some(image) = self.states.get(&id).and_then(|s| s.image.clone()) else { return };
        let [w, h] = image.size();
        if image.texture.mip_level_count() > 1 || w.max(h) <= 1 {
            return;
        }
        let mipped = self.gpu.take_scratch_with_mips(w, h, image.format());
        let mut encoder = self.gpu.encoder();
        encoder.copy_texture_to_texture(
            image.texture.as_image_copy(),
            mipped.texture.as_image_copy(),
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        if let Err(e) = self.gpu.encode_mips(&mut encoder, &mipped) {
            log::error!("mips: {e}");
        }
        self.gpu.submit(encoder);
        if let Some(state) = self.states.get_mut(&id) {
            state.image = Some(mipped);
        }
        self.gpu.recycle(image);
    }

    /// What the 2D view shows of `node`. Returns it when it changed (`Some(None)` when there
    /// is nothing to show any more).
    pub fn prepare_preview(&mut self, node: Option<NodeId>) -> Option<Option<DisplayImage>> {
        self.preview = node;
        let shown = node.and_then(|n| self.display(n));
        let sent = shown.as_ref().map(|d| (d.node, d.version, d.texture.clone()));
        if sent == self.preview_sent {
            return None;
        }
        self.preview_sent = sent;
        Some(shown)
    }

    /// What the 3D view shows of the given outputs. Returns the full set when anything
    /// changed.
    pub fn prepare_materials(&mut self, nodes: &[NodeId]) -> Option<Vec<DisplayImage>> {
        self.materials = nodes.to_vec();
        let shown: Vec<DisplayImage> = nodes.iter().filter_map(|n| self.display(*n)).collect();
        let sent: Vec<(NodeId, u64, wgpu::Texture)> = shown.iter().map(|d| (d.node, d.version, d.texture.clone())).collect();
        if sent == self.materials_sent {
            return None;
        }
        self.materials_sent = sent;
        Some(shown)
    }

    // ------------------------------------------------------------------ readbacks

    /// RGBA pixels of what a node shows, at the texture size (blocking): a grayscale image
    /// reads as gray and opaque, an image smaller than the texture (a uniform color) is
    /// spread over it, and an output reads as it writes its image.
    pub fn read_image(&self, node: NodeId) -> Result<(u32, u32, Vec<f32>), String> {
        let stored = self.read_stored(node)?;
        Ok((stored.width, stored.height, stored.to_f32()))
    }

    /// The same pixels as they are stored (see [`StoredImage`]): what an export writes.
    pub fn read_stored(&self, node: NodeId) -> Result<StoredImage, String> {
        let image = self.image(node).ok_or("The node has no image")?;
        let size = self.last_resolution;
        if image.size() == size {
            return self.gpu.read_stored_blocking(image);
        }
        let full = self.gpu.take_scratch(size[0], size[1], COLOR_FORMAT);
        let mut encoder = self.gpu.encoder();
        let copied = self.gpu.encode_show(&mut encoder, image, &full, 0);
        self.gpu.submit(encoder);
        let pixels = copied.and_then(|()| self.gpu.read_stored_blocking(&full));
        self.gpu.recycle(full);
        pixels
    }

    /// Histogram and statistics (blocking readback, multithreaded SIMD). An image smaller
    /// than the texture size has the same histogram as it spread over it.
    pub fn stats(&self, node: NodeId) -> Option<Arc<ImageStats>> {
        let image = self.image(node)?;
        let stored = self.gpu.read_stored_blocking(image).ok()?;
        Some(Arc::new(simd::stored_stats(&stored)))
    }

    /// Value of one pixel of the texture (blocking).
    pub fn probe(&self, node: NodeId, pixel: [u32; 2]) -> Option<[f32; 4]> {
        let image = self.image(node)?;
        let size = self.last_resolution;
        if pixel[0] >= size[0] || pixel[1] >= size[1] {
            return None;
        }
        // Where the pixel falls in an image smaller than the texture.
        let at = [pixel[0] * image.width() / size[0], pixel[1] * image.height() / size[1]];
        let mut encoder = self.gpu.encoder();
        let rb = self.gpu.encode_readback_region(&mut encoder, image, at);
        self.gpu.submit(encoder);
        rb.map();
        self.wait_idle();
        match rb.poll() {
            Some(Ok(())) => {
                let px = rb.to_f32();
                (px.len() >= 4).then(|| [px[0], px[1], px[2], px[3]])
            }
            _ => None,
        }
    }
}

/// Decode an image file (on disk, or held by its package, see
/// [`brewed_texture_core::files`]): a grayscale file (without alpha) into one value per pixel,
/// any other into RGBA half floats, on all cores. Pictures of 8 and 16 bits go from their
/// channels straight to half floats, looked up (8K of RGBA takes 0.8 GB at most, not 1.9).
fn decode_bitmap(path: &str) -> Result<(u32, u32, Pixels), String> {
    let bytes = brewed_texture_core::files::read(path)?;
    let img = open_picture(&bytes, &brewed_texture_core::package::file_name(path))?;
    drop(bytes);
    let (w, h) = (img.width(), img.height());
    let pixels = (w as usize) * (h as usize);
    let channels = usize::from(img.color().channel_count());
    let color = |convert: &dyn Fn(&mut [u16])| {
        let mut half = vec![0u16; pixels * 4];
        convert(&mut half);
        Pixels::Color(half)
    };
    let decoded = match img {
        image::DynamicImage::ImageLuma8(gray) => {
            let mut half = vec![0u16; pixels];
            simd::u8_to_f16(gray.as_raw(), &mut half);
            Pixels::Gray(half)
        }
        image::DynamicImage::ImageLuma16(gray) => {
            Pixels::FineGray(gray.into_raw().into_iter().map(|v| f32::from(v) / 65535.0).collect())
        }
        image::DynamicImage::ImageLumaA8(_) | image::DynamicImage::ImageRgb8(_) | image::DynamicImage::ImageRgba8(_) => {
            color(&|half| simd::u8_to_rgba_f16(img.as_bytes(), channels, half))
        }
        image::DynamicImage::ImageLumaA16(_) | image::DynamicImage::ImageRgb16(_) | image::DynamicImage::ImageRgba16(_) => {
            let words: &[u16] = bytemuck::cast_slice(img.as_bytes());
            color(&|half| simd::u16_to_rgba_f16(words, channels, half))
        }
        // Floats (EXR, HDR), and whatever else: through floats.
        other => {
            let floats = other.into_rgba32f().into_raw();
            color(&|half| simd::f32_to_f16(&floats, half))
        }
    };
    Ok((w, h, decoded))
}

/// The picture in the bytes of a file named `name`: of the kind its extension says, else of the
/// kind its first bytes say. A picture larger than a texture can be is refused before it is
/// decoded, as is one of no pixels.
pub fn open_picture(bytes: &[u8], name: &str) -> Result<image::DynamicImage, String> {
    const SIDE: u32 = brewed_texture_core::graph::MAX_RESOLUTION;
    let format = image::ImageFormat::from_path(name)
        .or_else(|_| image::guess_format(bytes))
        .map_err(|e| format!("Cannot open '{name}': {e}"))?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(SIDE);
    limits.max_image_height = Some(SIDE);
    // The largest of them, of 32-bit floats, takes 1 GB decoded (more than the decoder's own
    // limit lets it take).
    limits.max_alloc = Some(u64::from(SIDE) * u64::from(SIDE) * 16 + (64 << 20));
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(l) if matches!(l.kind(), image::error::LimitErrorKind::DimensionError) => {
            format!("'{name}' is larger than {SIDE} × {SIDE} pixels")
        }
        e => format!("Cannot open '{name}': {e}"),
    })?;
    if picture.width() == 0 || picture.height() == 0 {
        return Err(format!("'{name}' has no pixels"));
    }
    Ok(picture)
}

/// What an SVG or a Text node draws, from its parameters, at the texture size `size`.
fn vector_job(kind: NodeKind, params: &brewed_texture_core::eval::Params, size: [u32; 2]) -> VectorJob {
    let what = if kind == NodeKind::SVG {
        Vector::Svg { path: params.text("path").to_string(), fit: params.u("fit"), color: params.u("output") == 0 }
    } else {
        // Sizes in pixels of a 1024 px texture, as every length of a node.
        let px = params.f("size").max(0.0) * size[0] as f32 / 1024.0;
        Vector::Text {
            text: params.text("text").to_string(),
            font: params.text("font").to_string(),
            size: px,
            align: params.u("align"),
            spacing: params.f("spacing"),
        }
    };
    let scale = if kind == NodeKind::SVG { params.f("scale").max(1e-3) } else { 1.0 };
    VectorJob { what, size, offset: params.v2("offset"), rotation: params.f("rotation"), scale, tiling: params.tiling(), reads: 0 }
}

/// Convenience for tests and tools: evaluate `graph` from scratch and wait for the GPU.
pub fn evaluate_blocking(engine: &mut Engine, graph: &Graph) -> Evaluation {
    let eval = brewed_texture_core::eval::evaluate(graph);
    engine.evaluate(graph, &eval);
    // Files are decoded and pictures drawn in the background; wait for them.
    let start = Instant::now();
    while engine.files_loading() && start.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        if engine.receive_files() {
            engine.evaluate(graph, &eval);
        }
    }
    engine.wait_idle();
    eval
}
