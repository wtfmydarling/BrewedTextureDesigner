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
//! The engine is single threaded; [`crate::service`] runs it on a background thread.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use brewed_texture_core::eval::{Evaluation, VarSource};
use brewed_texture_core::graph::{Graph, Node, NodeId, input_socket_key};
use brewed_texture_core::nodes::{ImageKind, NodeKind, PIXEL_MATH_BUILTINS, image_kind};

use crate::gpu::{COLOR_FORMAT, GRAY_FORMAT, Gpu, GpuImage, Readback, format_of, work_format};
use crate::kernels::{self, DisplayU, ExecCtx, PixelVar};
use crate::simd::{self, ImageStats};

pub const THUMB_SIZE: u32 = 128;
/// Samples in a baked gradient.
pub const LUT_SIZE: usize = 1024;
/// The size the pictures of a node's options are drawn at before they are made into
/// thumbnails: fine enough for their look, small enough to draw them all at once.
pub const CHOICE_RES: u32 = 256;

/// What the engine knows about one node.
#[derive(Default)]
pub struct NodeState {
    /// Its image, while something reads it (see [`Keep`]). An output or a bypassed node has
    /// none of its own: it shows the image of its `producer`.
    image: Option<GpuImage>,
    /// For an output or a bypassed node, the node whose image arrives at it (through other
    /// outputs and bypassed nodes).
    producer: Option<NodeId>,
    /// Grayscale or color; `None` while the node makes no image.
    kind: Option<ImageKind>,
    /// How the node shows its image: as the format of an output, or as it is (0).
    shown_as: u32,
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

/// What a view shows of a node: the image itself, with its mips, which the 2D and 3D views
/// sample directly (no copy), and how the node shows it (`widen` and `as_output` of
/// [`crate::gpu::SHOW_WGSL`]).
#[derive(Clone, Debug)]
pub struct DisplayImage {
    pub node: NodeId,
    pub version: u64,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// The texture size the image stands for (a uniform color is one pixel, shown as big as
    /// any other image).
    pub size: [u32; 2],
    /// One channel: a grayscale image, which reads as its red.
    pub gray: bool,
    /// The format of the output it is shown as, 0 when it is shown as it is.
    pub format: u32,
}

enum BitmapEntry {
    Loading,
    Ready { image: GpuImage, generation: u64 },
    Failed(String),
}

/// A file's pixels, as they will be uploaded.
enum Pixels {
    /// One value per pixel: the file is grayscale, and so is its image.
    Gray(Vec<f32>),
    /// RGBA half floats.
    Color(Vec<u16>),
}

struct DecodedBitmap {
    path: String,
    result: Result<(u32, u32, Pixels), String>,
}

struct Lut {
    hash: u64,
    image: GpuImage,
}

/// Which images an engine keeps from one evaluation to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keep {
    /// Every node's: for a caller that reads whichever image it likes (tests, tools).
    #[default]
    All,
    /// Only those something reads: the node the 2D view shows, what arrives at the outputs
    /// the 3D view uses, the nodes asked about, and what the last change read from outside
    /// itself (the next change of the same kind reads it again). The others live while the
    /// evaluation that makes them needs them.
    Read,
}

/// What an evaluation found about an image node before computing anything.
struct Plan {
    node_kind: NodeKind,
    /// An output or a bypassed node: it shows the image of what arrives at it.
    alias: bool,
    /// Its key changed: it has a new version, and its image has to be computed.
    changed: bool,
    kind: ImageKind,
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
    /// What the last change read from outside itself (see [`Keep::Read`]).
    boundary: HashSet<NodeId>,
    /// Thumbnails the evaluations recorded, to read back (see [`Engine::render_thumbnails`]).
    thumbnails: Vec<(NodeId, u64, GpuImage, Readback)>,
    /// Pictures of the options of choices, by node kind and parameter (see
    /// [`Engine::render_choices`]).
    choices: HashMap<(&'static str, String), Arc<Vec<Vec<u8>>>>,
    last_resolution: [u32; 2],
    pub report: EvalReport,
}

impl Engine {
    pub fn new(gpu: Gpu) -> Engine {
        let (bitmap_tx, bitmap_rx) = channel();
        Engine {
            gpu,
            states: HashMap::new(),
            next_version: 1,
            bitmaps: HashMap::new(),
            bitmap_generation: 1,
            bitmap_tx,
            bitmap_rx,
            luts: HashMap::new(),
            keep: Keep::default(),
            preview: None,
            preview_sent: None,
            materials: Vec::new(),
            materials_sent: Vec::new(),
            asked: Vec::new(),
            given: Vec::new(),
            boundary: HashSet::new(),
            thumbnails: Vec::new(),
            choices: HashMap::new(),
            last_resolution: [0, 0],
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

    /// Are files still being decoded? (The graph must be evaluated again when they arrive.)
    pub fn bitmaps_loading(&self) -> bool {
        self.bitmaps.values().any(|b| matches!(b, BitmapEntry::Loading))
    }

    /// Drop every cached result (new document, device change ...).
    pub fn reset(&mut self) {
        self.states.clear();
        self.luts.clear();
        self.preview_sent = None;
        self.materials_sent.clear();
        self.given.clear();
        self.boundary.clear();
        self.thumbnails.clear();
        self.choices.clear();
        self.gpu.clear_pool();
    }

    /// GPU memory the engine's images take: the images kept (with the mips of those the views
    /// show), baked gradients, loaded files and the scratch images kept for reuse.
    pub fn memory(&self) -> u64 {
        let mut seen: Vec<wgpu::Texture> = Vec::new();
        let images = self
            .states
            .values()
            .filter_map(|s| s.image.as_ref())
            .chain(self.luts.values().map(|l| &l.image))
            .chain(self.bitmaps.values().filter_map(|b| match b {
                BitmapEntry::Ready { image, .. } => Some(image),
                _ => None,
            }));
        let mut total = 0;
        for image in images {
            if !seen.contains(&image.texture) {
                seen.push(image.texture.clone());
                total += image.bytes();
            }
        }
        total + self.gpu.pool_bytes()
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
            Some(BitmapEntry::Loading) => Ok(None),
            None => {
                self.bitmaps.insert(path.to_string(), BitmapEntry::Loading);
                let tx = self.bitmap_tx.clone();
                let path = path.to_string();
                std::thread::spawn(move || {
                    let result = decode_bitmap(&path);
                    let _ = tx.send(DecodedBitmap { path, result });
                });
                Ok(None)
            }
        }
    }

    /// Forget a cached file so it is decoded again.
    pub fn reload_bitmap(&mut self, path: &str) {
        self.bitmaps.remove(path);
    }

    /// Upload decoded files. Returns true if any arrived (evaluate again).
    pub fn receive_bitmaps(&mut self) -> bool {
        let mut any = false;
        while let Ok(done) = self.bitmap_rx.try_recv() {
            let entry = match done.result {
                Ok((w, h, pixels)) => {
                    let image = match pixels {
                        Pixels::Gray(values) => {
                            let image = self.gpu.create_image_as(w, h, GRAY_FORMAT, "bitmap");
                            self.gpu.upload_gray_f32(&image, &values);
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

    // ------------------------------------------------------------------ evaluation

    /// Bring every image up to date with `graph`. `eval` is the CPU evaluation of the
    /// same graph (values and effective parameters).
    pub fn evaluate(&mut self, graph: &Graph, eval: &Evaluation) -> EvalReport {
        self.receive_bitmaps();

        // Forget nodes that were deleted, and let go of their images.
        let alive: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id).collect();
        let gone: Vec<NodeId> = self.states.keys().filter(|id| !alive.contains(id)).copied().collect();
        for id in gone {
            self.release(id);
            self.states.remove(&id);
        }
        self.luts.retain(|id, _| alive.contains(id));
        self.boundary.retain(|id| alive.contains(id));
        for id in &eval.cyclic {
            self.release(*id);
            let state = self.states.entry(*id).or_default();
            state.kind = None;
            state.error = None;
        }

        let resolution = graph.resolution.map(|r| r.clamp(16, 8192));
        if resolution != self.last_resolution {
            // Scratch images of the old size would only waste GPU memory.
            self.gpu.clear_pool();
            self.last_resolution = resolution;
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

        // 2. What to compute, from the last node back: a node that changed, and a node whose
        //    image is needed and not there, by a node computed after it or by what reads it
        //    once this evaluation is done. An output or a bypassed node computes nothing, but
        //    a changed one reads what arrives at it for its thumbnail.
        let wanted: HashSet<NodeId> = match self.keep {
            Keep::All => order.iter().copied().collect(),
            Keep::Read => self.preview.iter().chain(&self.materials).chain(&self.asked).copied().collect(),
        };
        let mut needed = wanted.clone();
        let mut runs: HashSet<NodeId> = HashSet::new();
        for &id in order.iter().rev() {
            let plan = &plans[&id];
            let run = plan.changed || (!plan.alias && needed.contains(&id) && self.states.get(&id).is_none_or(|s| s.image.is_none()));
            if run || (plan.alias && needed.contains(&id)) {
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
            Keep::Read => wanted.iter().map(|id| self.producer(*id)).chain(self.boundary.iter().copied()).collect(),
        };

        // 3. Compute, in order. Each image goes back to the pool (for the next image of its
        //    size) once the last node that reads it here has read it, unless it stays.
        let mut readers: HashMap<NodeId, usize> = HashMap::new();
        for id in &runs {
            for read in plans[id].reads() {
                *readers.entry(read).or_default() += 1;
            }
        }
        let shown: HashSet<NodeId> = self.preview.iter().chain(&self.materials).map(|id| self.producer(*id)).collect();
        let mut encoder = self.gpu.encoder();
        let mut report = EvalReport { image_nodes: order.len(), ..EvalReport::default() };
        for &id in &order {
            if !runs.contains(&id) {
                continue;
            }
            let plan = &plans[&id];
            if !plan.alias {
                let Some(ne) = eval.node(id) else { continue };
                self.compute(id, plan, &ne.params, ne.error(), shown.contains(&id), &mut encoder);
                report.recomputed += 1;
            }
            if plan.changed {
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
        let stale: Vec<NodeId> =
            self.states.iter().filter(|(id, s)| s.image.is_some() && !kept.contains(id)).map(|(id, _)| *id).collect();
        for id in stale {
            self.release(id);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        self.report = report;
        report
    }

    /// The node whose image `node` shows: itself, or, for an output or a bypassed node, what
    /// arrives at it.
    fn producer(&self, node: NodeId) -> NodeId {
        self.states.get(&node).and_then(|s| s.producer).unwrap_or(node)
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

    /// The outputs the 3D view uses: what arrives at them is read, and stays.
    pub fn set_materials(&mut self, nodes: &[NodeId]) {
        self.materials = nodes.to_vec();
    }

    /// Nodes whose images are read after the next evaluation (their statistics, a pixel of
    /// them, their export): each is computed if it is not there, and stays until an
    /// evaluation that is not asked for it.
    pub fn ask(&mut self, nodes: Vec<NodeId>) {
        self.asked = nodes;
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
        // behind it, through outputs and bypassed nodes), and the version of that node.
        let mut inputs: Vec<Option<NodeId>> = Vec::with_capacity(d.inputs.len());
        let mut input_kinds: Vec<Option<ImageKind>> = Vec::with_capacity(d.inputs.len());
        for s in d.inputs {
            let from = graph.link_into(node.id, &input_socket_key(s.key)).map(|l| l.from);
            match from.and_then(|f| self.states.get(&f).filter(|st| st.kind.is_some()).map(|st| (f, st))) {
                Some((f, st)) => {
                    (1u8, f.0, st.version).hash(&mut hasher);
                    inputs.push(Some(st.producer.unwrap_or(f)));
                    input_kinds.push(st.kind);
                }
                None => {
                    0u8.hash(&mut hasher);
                    inputs.push(None);
                    input_kinds.push(None);
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

        // Gradient of a Gradient Map node.
        let mut lut: Option<GpuImage> = None;
        if node.kind == NodeKind::GRADIENT_MAP {
            let gradient = params.gradient("gradient");
            let mut gh = DefaultHasher::new();
            gradient.hash_into(&mut gh);
            let ghash = gh.finish();
            let current = self.luts.get(&node.id).filter(|l| l.hash == ghash).map(|l| l.image.clone());
            let image = current.unwrap_or_else(|| {
                let image = self.gpu.create_lut(&gradient.bake(LUT_SIZE));
                self.luts.insert(node.id, Lut { hash: ghash, image: image.clone() });
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

        // An output shows what arrives at it, and a bypassed node passes it on: neither makes an
        // image of its own.
        let shown_as = if node.kind == NodeKind::OUTPUT { params.u("format") } else { 0 };
        let alias = node.kind == NodeKind::OUTPUT || (node.bypass && !d.inputs.is_empty());
        let (kind, size) = if alias {
            (input_kinds.first().copied().flatten(), resolution)
        } else {
            // A grayscale or a color image, as big as the texture size; but a uniform color is
            // the same everywhere, and one pixel of it is all of it.
            let kind = match &bitmap {
                Some(file) => file.kind(),
                None => image_kind(node.kind, params, &input_kinds),
            };
            let size = if node.kind == NodeKind::UNIFORM_COLOR { [1, 1] } else { resolution };
            (kind, size).hash(&mut hasher);
            (Some(kind), size)
        };
        let key = hasher.finish();

        let version = self.next_version + 1;
        let state = self.states.entry(node.id).or_default();
        let changed = state.version == 0 || state.key != key;
        state.key = key;
        state.kind = kind;
        state.shown_as = shown_as;
        state.producer = if alias { inputs.first().copied().flatten() } else { None };
        if alias {
            state.error = None;
            state.warning = None;
        }
        if changed {
            state.version = version;
        }
        // An image that stayed from before and does not fit any more is let go of (an output or
        // a bypassed node has none of its own).
        let fits = !alias && state.image.as_ref().is_none_or(|img| img.size() == size && kind.is_some_and(|k| img.format() == format_of(k)));
        if changed {
            self.next_version = version;
        }
        if !fits {
            self.release(node.id);
        }
        let kind = kind.unwrap_or(ImageKind::Color);
        Some(Plan { node_kind: node.kind, alias, changed, kind, size, inputs, vars, bitmap, lut, error, warning })
    }

    /// Compute a node's image (see [`Engine::evaluate`]): into its own image when it has one
    /// that fits, else into one from the pool; with mips when a view shows it.
    fn compute(
        &mut self,
        id: NodeId,
        plan: &Plan,
        params: &brewed_texture_core::eval::Params,
        cpu_error: Option<&str>,
        shown: bool,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let [w, h] = plan.size;
        let format = format_of(plan.kind);
        let mipped = shown && w.max(h) > 1;
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
        let target = if mipped { image.level(0) } else { image.clone() };
        // Kernels write what every device can store to: a grayscale image is computed in 32
        // bits and stored in 16.
        let work = (work_format(format) != format).then(|| self.gpu.take_scratch(w, h, work_format(format)));
        let output = work.as_ref().unwrap_or(&target);

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
                        output,
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
            let _ = kernels::clear(&self.gpu, encoder, output);
        }
        if let Some(work) = work {
            if let Err(e) = self.gpu.encode_store(encoder, &work, &target) {
                error = error.or(Some(e));
            }
            self.gpu.recycle(work);
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
        let Some(state) = self.states.get(&id) else { return };
        let (version, shown_as) = (state.version, state.shown_as);
        let Some(image) = self.image(id).cloned() else { return };
        let params = DisplayU { mode: 0, checker: 8.0, format: shown_as, pad: 0 };
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
        let plain = d.has_image() && !d.is_pixel_expression() && ![NodeKind::BITMAP, NodeKind::GRADIENT_MAP].contains(&kind);
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

        let display = DisplayU { mode: 0, checker: 8.0, format: 0, pad: 0 };
        let input_kinds: Vec<Option<ImageKind>> = vec![None; inputs.len()];
        let mut encoder = self.gpu.encoder();
        let mut jobs = Vec::with_capacity(options);
        let mut drawn = Vec::new();
        for option in 0..options {
            params.values[choice].1 = brewed_texture_core::ParamValue::Enum(option as u32);
            // Drawn as the node draws itself, in the kind of image it makes with the option.
            let format = work_format(format_of(image_kind(kind, &params, &input_kinds)));
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
                let _ = kernels::clear(&self.gpu, &mut encoder, &picture);
            }
            let thumb = self.gpu.take_scratch(THUMB_SIZE, THUMB_SIZE, COLOR_FORMAT);
            let _ = self.gpu.run(&mut encoder, "thumbnail", bytemuck::bytes_of(&display), &[&picture], &thumb);
            let readback = self.gpu.encode_readback(&mut encoder, &thumb);
            jobs.push((thumb, readback));
            drawn.push(picture);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
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
    /// arrives at an output or a bypassed node), with the mips a view samples it with, and how
    /// the node shows it. `None` when there is no image.
    fn display(&mut self, node: NodeId) -> Option<DisplayImage> {
        let state = self.states.get(&node)?;
        let (version, format) = (state.version, state.shown_as);
        let producer = self.producer(node);
        self.ensure_mips(producer);
        let image = self.image(producer)?.clone();
        if !self.given.contains(&image.texture) {
            self.given.push(image.texture.clone());
        }
        let size = self.last_resolution;
        Some(DisplayImage { node, version, size, gray: image.is_gray(), format, texture: image.texture, view: image.view })
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
        self.gpu.queue.submit(Some(encoder.finish()));
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
    /// spread over it, and an output reads as its format shows its image.
    pub fn read_image(&self, node: NodeId) -> Result<(u32, u32, Vec<f32>), String> {
        let image = self.image(node).ok_or("The node has no image")?;
        let shown_as = self.states.get(&node).map_or(0, |s| s.shown_as);
        let size = self.last_resolution;
        if image.size() == size && shown_as == 0 {
            return Ok((size[0], size[1], self.gpu.read_image_blocking(image)?));
        }
        let full = self.gpu.take_scratch(size[0], size[1], COLOR_FORMAT);
        let mut encoder = self.gpu.encoder();
        let copied = self.gpu.encode_show(&mut encoder, image, &full, shown_as);
        self.gpu.queue.submit(Some(encoder.finish()));
        let pixels = copied.and_then(|()| self.gpu.read_image_blocking(&full));
        self.gpu.recycle(full);
        Ok((size[0], size[1], pixels?))
    }

    /// Histogram and statistics (blocking readback, multithreaded SIMD). An image smaller
    /// than the texture size has the same histogram as it spread over it.
    pub fn stats(&self, node: NodeId) -> Option<Arc<ImageStats>> {
        let image = self.image(node)?;
        let pixels = self.gpu.read_image_blocking(image).ok()?;
        Some(Arc::new(simd::image_stats(&pixels, image.width(), image.height())))
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
        self.gpu.queue.submit(Some(encoder.finish()));
        rb.map();
        self.wait_idle();
        match rb.poll() {
            Some(Ok(())) => {
                let px = rb.to_f32();
                let shown_as = self.states.get(&node).map_or(0, |s| s.shown_as);
                (px.len() >= 4).then(|| as_output([px[0], px[1], px[2], px[3]], shown_as))
            }
            _ => None,
        }
    }
}

/// What an output in `format` shows of a pixel (the same as `as_output` of common.wgsl).
fn as_output(c: [f32; 4], format: u32) -> [f32; 4] {
    match format {
        1 => [c[0], c[1], c[2], 1.0],
        2 => [c[0], c[1], 0.0, 1.0],
        3 => [c[0], c[0], c[0], 1.0],
        4 => [c[1], c[1], c[1], 1.0],
        5 => [c[2], c[2], c[2], 1.0],
        6 => [c[3], c[3], c[3], 1.0],
        _ => c,
    }
}

/// Decode an image file: a grayscale file (without alpha) into one value per pixel, any
/// other into RGBA half floats (SIMD conversion on all cores).
fn decode_bitmap(path: &str) -> Result<(u32, u32, Pixels), String> {
    let img = image::open(path).map_err(|e| format!("Cannot open '{path}': {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("Unsupported image size {w}x{h} (max 8192)"));
    }
    match img.color() {
        image::ColorType::L8 => {
            return Ok((w, h, Pixels::Gray(img.into_luma8().into_raw().into_iter().map(|v| f32::from(v) / 255.0).collect())));
        }
        image::ColorType::L16 => {
            return Ok((w, h, Pixels::Gray(img.into_luma16().into_raw().into_iter().map(|v| f32::from(v) / 65535.0).collect())));
        }
        _ => {}
    }
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    match img.color() {
        image::ColorType::La16 | image::ColorType::Rgb16 | image::ColorType::Rgba16 => {
            simd::u16_to_f32(&img.into_rgba16().into_raw(), &mut rgba);
        }
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => rgba = img.into_rgba32f().into_raw(),
        _ => simd::u8_to_f32(&img.into_rgba8().into_raw(), &mut rgba),
    }
    let mut half = vec![0u16; rgba.len()];
    simd::f32_to_f16(&rgba, &mut half);
    Ok((w, h, Pixels::Color(half)))
}

/// Convenience for tests and tools: evaluate `graph` from scratch and wait for the GPU.
pub fn evaluate_blocking(engine: &mut Engine, graph: &Graph) -> Evaluation {
    let eval = brewed_texture_core::eval::evaluate(graph);
    engine.evaluate(graph, &eval);
    // Files are decoded in the background; wait for them once.
    let start = Instant::now();
    while engine.bitmaps_loading() && start.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        if engine.receive_bitmaps() {
            engine.evaluate(graph, &eval);
        }
    }
    engine.wait_idle();
    eval
}
