//! Incremental evaluation of a graph on the GPU.
//!
//! [`Engine::evaluate`] walks the graph in topological order and computes a key per image
//! node: a hash of its type, effective parameters (from the CPU [`Evaluation`]),
//! resolution and the versions of its inputs. Only nodes whose key changed are recomputed,
//! so dragging a slider re-runs just the affected downstream chain. All work of one
//! evaluation is recorded into a single command encoder and submitted once.
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

use crate::gpu::{COLOR_FORMAT, DISPLAY_FORMAT, GRAY_FORMAT, Gpu, GpuImage, format_of};
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
    pub image: Option<GpuImage>,
    /// The image is another node's: an output shows what arrives at it, and a bypassed node
    /// passes it on. Before the node writes anything, it makes an image of its own.
    aliased: bool,
    /// How the node shows its image: as the format of an output, or as it is (0).
    shown_as: u32,
    /// Bumped whenever the image changes.
    pub version: u64,
    key: u64,
    /// Problems found while computing the image (in addition to the CPU evaluation's).
    pub error: Option<String>,
    pub warning: Option<String>,
    thumb_version: u64,
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

/// What a node makes with each option of one of its choices, as thumbnails (RGBA8,
/// [`THUMB_SIZE`] square, composited over a checkerboard): a menu of the options can show
/// how each one looks rather than only its name.
#[derive(Clone, Debug)]
pub struct ChoicePictures {
    pub node: NodeId,
    pub key: String,
    /// The version of the node's image when the pictures were drawn.
    pub version: u64,
    pub size: u32,
    /// One picture per option, in the order of the options.
    pub pictures: Arc<Vec<Vec<u8>>>,
}

/// A mip-mapped copy of a node image for display (2D and 3D views sample it directly).
#[derive(Clone, Debug)]
pub struct DisplayImage {
    pub node: NodeId,
    pub version: u64,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: [u32; 2],
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

/// Pictures of options, with a hash of everything they depend on (see
/// [`Engine::render_choices`]).
struct Drawn {
    hash: u64,
    pictures: Arc<Vec<Vec<u8>>>,
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
    /// The copies the views show, one per node shown however many views show it.
    displays: HashMap<NodeId, DisplayImage>,
    /// What the 2D view shows, and the version of it it was last given.
    preview: Option<NodeId>,
    preview_sent: Option<(NodeId, u64)>,
    /// The outputs the 3D view uses, and the versions of them it was last given.
    materials: Vec<NodeId>,
    materials_sent: Vec<(NodeId, u64)>,
    /// The pictures of the options of a node's choice, as last drawn.
    choices: HashMap<(NodeId, String), Drawn>,
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
            displays: HashMap::new(),
            preview: None,
            preview_sent: None,
            materials: Vec::new(),
            materials_sent: Vec::new(),
            choices: HashMap::new(),
            last_resolution: [0, 0],
            report: EvalReport::default(),
        }
    }

    pub fn state(&self, id: NodeId) -> Option<&NodeState> {
        self.states.get(&id)
    }

    pub fn image(&self, id: NodeId) -> Option<&GpuImage> {
        self.states.get(&id).and_then(|s| s.image.as_ref())
    }

    pub fn version(&self, id: NodeId) -> Option<u64> {
        self.states.get(&id).map(|s| s.version)
    }

    pub fn reports(&self) -> HashMap<NodeId, ImageReport> {
        self.states
            .iter()
            .map(|(id, s)| {
                let kind = s.image.as_ref().map(GpuImage::kind);
                (*id, ImageReport { version: s.version, error: s.error.clone(), warning: s.warning.clone(), kind })
            })
            .collect()
    }

    /// Are files still being decoded? (The graph must be evaluated again when they arrive.)
    pub fn bitmaps_loading(&self) -> bool {
        self.bitmaps.values().any(|b| matches!(b, BitmapEntry::Loading))
    }

    fn bump(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    /// Drop every cached result (new document, device change ...).
    pub fn reset(&mut self) {
        self.states.clear();
        self.luts.clear();
        self.displays.clear();
        self.preview_sent = None;
        self.materials_sent.clear();
        self.choices.clear();
        self.gpu.clear_pool();
    }

    /// GPU memory the engine's images take: every node's image, the copies the views show,
    /// baked gradients, loaded files and the scratch images kept for reuse. An image two
    /// nodes share (an output and what arrives at it) is counted once.
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
        for display in self.displays.values() {
            if !seen.contains(&display.texture) {
                seen.push(display.texture.clone());
                total += crate::gpu::texture_bytes(&display.texture);
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

        // Forget nodes that were deleted.
        let alive: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id).collect();
        self.states.retain(|id, _| alive.contains(id));
        self.luts.retain(|id, _| alive.contains(id));
        self.displays.retain(|id, _| alive.contains(id));
        self.choices.retain(|(id, _), _| alive.contains(id));
        for id in &eval.cyclic {
            let state = self.states.entry(*id).or_default();
            state.image = None;
            state.error = None;
        }

        let resolution = graph.resolution.map(|r| r.clamp(16, 8192));
        if resolution != self.last_resolution {
            // Scratch images of the old size would only waste GPU memory.
            self.gpu.clear_pool();
            self.last_resolution = resolution;
        }
        let mut encoder = self.gpu.encoder();
        let mut report = EvalReport::default();
        for &id in &eval.order {
            let Some(node) = graph.node(id) else { continue };
            if !node.def().has_image() {
                continue;
            }
            report.image_nodes += 1;
            if self.evaluate_image_node(graph, eval, node, resolution, &mut encoder) {
                report.recomputed += 1;
            }
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        self.report = report;
        report
    }

    /// Returns true when the node was recomputed.
    fn evaluate_image_node(
        &mut self,
        graph: &Graph,
        eval: &Evaluation,
        node: &Node,
        resolution: [u32; 2],
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let d = node.def();
        let Some(ne) = eval.node(node.id) else { return false };
        let params = &ne.params;

        let mut hasher = DefaultHasher::new();
        node.kind.id().hash(&mut hasher);
        resolution.hash(&mut hasher);
        node.bypass.hash(&mut hasher);
        params.hash_into(&mut hasher);

        // Fixed inputs.
        let mut inputs: Vec<Option<GpuImage>> = Vec::with_capacity(d.inputs.len());
        for s in d.inputs {
            let src = graph
                .link_into(node.id, &input_socket_key(s.key))
                .and_then(|l| self.states.get(&l.from).map(|st| (l.from, st)));
            match src {
                Some((from, st)) if st.image.is_some() => {
                    (1u8, from.0, st.version).hash(&mut hasher);
                    inputs.push(st.image.clone());
                }
                _ => {
                    0u8.hash(&mut hasher);
                    inputs.push(None);
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
        let mut pixel_vars = Vec::new();
        if d.is_pixel_expression() {
            for v in &ne.vars {
                let var = match &v.source {
                    VarSource::Builtin => {
                        let b = PIXEL_MATH_BUILTINS.iter().copied().find(|b| *b == v.name).unwrap_or("u");
                        (2u8, b).hash(&mut hasher);
                        PixelVar::Builtin(b)
                    }
                    VarSource::Image(from) => match self.states.get(from).filter(|s| s.image.is_some()) {
                        Some(st) => {
                            (1u8, from.0, st.version).hash(&mut hasher);
                            PixelVar::Image(st.image.clone().expect("checked"))
                        }
                        None => {
                            3u8.hash(&mut hasher);
                            PixelVar::Constant(0.0)
                        }
                    },
                    _ => {
                        (0u8, (v.value as f32).to_bits()).hash(&mut hasher);
                        PixelVar::Constant(v.value as f32)
                    }
                };
                pixel_vars.push(var);
            }
        }

        // An output shows what arrives at it, and a bypassed node passes it on: neither makes an
        // image of its own.
        let shown_as = if node.kind == NodeKind::OUTPUT { params.u("format") } else { 0 };
        if node.kind == NodeKind::OUTPUT || (node.bypass && !d.inputs.is_empty()) {
            let key = hasher.finish();
            if self.states.get(&node.id).is_some_and(|s| s.aliased && s.key == key) {
                return false;
            }
            let version = self.bump();
            let state = self.states.entry(node.id).or_default();
            state.image = if ne.error().is_some() { None } else { inputs.first().cloned().flatten() };
            state.aliased = true;
            state.shown_as = shown_as;
            state.version = version;
            state.key = key;
            state.error = None;
            state.warning = None;
            // Nothing was computed.
            return false;
        }

        // What the node makes: a grayscale or a color image, as big as the texture size; but
        // a uniform color is the same everywhere, and one pixel of it is all of it.
        let input_kinds: Vec<Option<ImageKind>> = inputs.iter().map(|i| i.as_ref().map(GpuImage::kind)).collect();
        let kind = match &bitmap {
            Some(file) => file.kind(),
            None => image_kind(node.kind, params, &input_kinds),
        };
        let format = format_of(kind);
        let size = if node.kind == NodeKind::UNIFORM_COLOR { [1, 1] } else { resolution };
        (kind, size).hash(&mut hasher);
        let key = hasher.finish();
        let own = self
            .states
            .get(&node.id)
            .filter(|s| !s.aliased)
            .and_then(|s| s.image.clone())
            .filter(|img| img.size() == size && img.format() == format);
        if own.is_some() && self.states.get(&node.id).is_some_and(|s| s.key == key) {
            return false;
        }
        let output = own.unwrap_or_else(|| self.gpu.create_image_as(size[0], size[1], format, d.name));

        if error.is_none() {
            let result = match kernels::kernel_for(node.kind) {
                Some(kernel) => {
                    let mut cx = ExecCtx {
                        kind: node.kind,
                        gpu: &self.gpu,
                        encoder: &mut *encoder,
                        params,
                        inputs: &inputs,
                        output: &output,
                        bitmap: bitmap.as_ref(),
                        lut: lut.as_ref(),
                        pixel_vars: &pixel_vars,
                    };
                    kernel(&mut cx)
                }
                None => Err(format!("No GPU kernel for '{}'", d.name)),
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
            log::debug!("node {} ({}): {e}", node.id.0, d.name);
            // Leave a transparent image so downstream nodes still work.
            let _ = kernels::clear(&self.gpu, encoder, &output);
        }

        let version = self.bump();
        let state = self.states.entry(node.id).or_default();
        state.image = Some(output);
        state.aliased = false;
        state.shown_as = shown_as;
        state.version = version;
        state.key = key;
        // Errors of the CPU evaluation are already reported by it.
        state.error = error.filter(|e| ne.error() != Some(e.as_str()));
        state.warning = warning;
        true
    }

    // ------------------------------------------------------------------ thumbnails

    /// Thumbnails of every node whose image changed since its last thumbnail. Blocks
    /// until the GPU work and the readbacks are done.
    pub fn render_thumbnails(&mut self) -> Vec<Thumbnail> {
        let mut encoder = self.gpu.encoder();
        let mut jobs = Vec::new();
        for (id, state) in &mut self.states {
            let Some(image) = &state.image else { continue };
            if state.thumb_version == state.version {
                continue;
            }
            let params = DisplayU { mode: 0, checker: 8.0, format: state.shown_as, pad: 0 };
            let thumb = self.gpu.take_scratch(THUMB_SIZE, THUMB_SIZE, COLOR_FORMAT);
            if self.gpu.run(&mut encoder, "thumbnail", bytemuck::bytes_of(&params), &[image], &thumb).is_err() {
                self.gpu.recycle(thumb);
                continue;
            }
            let readback = self.gpu.encode_readback(&mut encoder, &thumb);
            state.thumb_version = state.version;
            jobs.push((*id, state.version, thumb, readback));
        }
        if jobs.is_empty() {
            return Vec::new();
        }
        self.gpu.queue.submit(Some(encoder.finish()));
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

    /// What `node` makes with each option of its choice `key`, everything else as it is now:
    /// its parameters, its inputs, its tiling. Drawn at [`CHOICE_RES`] and made into
    /// thumbnails like the node's own. The pictures are kept, and asking again before
    /// anything they depend on changed costs nothing. Blocks until they are read back.
    pub fn render_choices(&mut self, graph: &Graph, eval: &Evaluation, node: NodeId, key: &str) -> Option<ChoicePictures> {
        let n = graph.node(node)?;
        let d = n.def();
        let options = d.param(key)?.enum_options().len();
        // Nodes that need more than parameters and images (a file, a baked gradient,
        // variables per pixel) have no choices worth a picture. A bypassed node shows what
        // it will make once it is not: that is what its options are chosen for.
        let plain = d.has_image() && !d.is_pixel_expression() && ![NodeKind::BITMAP, NodeKind::GRADIENT_MAP].contains(&n.kind);
        if options == 0 || !plain {
            return None;
        }
        let kernel = kernels::kernel_for(n.kind)?;
        let mut params = eval.node(node)?.params.clone();
        let choice = params.values.iter().position(|(k, _)| k == key)?;

        let mut hasher = DefaultHasher::new();
        n.kind.id().hash(&mut hasher);
        let mut inputs = Vec::with_capacity(d.inputs.len());
        for s in d.inputs {
            let source = graph
                .link_into(node, &input_socket_key(s.key))
                .and_then(|l| self.states.get(&l.from).filter(|st| st.image.is_some()).map(|st| (l.from, st)));
            match source {
                Some((from, st)) => {
                    (1u8, from.0, st.version).hash(&mut hasher);
                    inputs.push(st.image.clone());
                }
                None => {
                    0u8.hash(&mut hasher);
                    inputs.push(None);
                }
            }
        }
        // Everything but the choice itself: picking another option changes nothing drawn.
        params.values[choice].1 = brewed_texture_core::ParamValue::Enum(0);
        params.hash_into(&mut hasher);
        let hash = hasher.finish();

        let version = self.version(node).unwrap_or(0);
        let cache = (node, key.to_string());
        if let Some(drawn) = self.choices.get(&cache)
            && drawn.hash == hash
        {
            return Some(ChoicePictures { node, key: key.to_string(), version, size: THUMB_SIZE, pictures: drawn.pictures.clone() });
        }

        let display = DisplayU { mode: 0, checker: 8.0, format: 0, pad: 0 };
        let input_kinds: Vec<Option<ImageKind>> = inputs.iter().map(|i| i.as_ref().map(GpuImage::kind)).collect();
        let mut encoder = self.gpu.encoder();
        let mut jobs = Vec::with_capacity(options);
        let mut drawn = Vec::new();
        for option in 0..options {
            params.values[choice].1 = brewed_texture_core::ParamValue::Enum(option as u32);
            // Drawn as the node draws itself, in the kind of image it makes with the option.
            let format = format_of(image_kind(n.kind, &params, &input_kinds));
            let picture = self.gpu.take_scratch(CHOICE_RES, CHOICE_RES, format);
            let mut cx = ExecCtx {
                kind: n.kind,
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
        self.choices.insert(cache, Drawn { hash, pictures: pictures.clone() });
        Some(ChoicePictures { node, key: key.to_string(), version, size: THUMB_SIZE, pictures })
    }

    // ------------------------------------------------------------------ what the views show

    /// Bring the copy the views show of `node` up to date: its image as the node shows it,
    /// at the texture size, 8 bits a channel, with mips. `None` when the node has no image.
    fn display(&mut self, node: NodeId) -> Option<DisplayImage> {
        let state = self.states.get(&node)?;
        let source = state.image.clone()?;
        let (version, shown_as) = (state.version, state.shown_as);
        let size = self.last_resolution;
        if let Some(d) = self.displays.get(&node)
            && d.version == version
            && d.size == size
        {
            return Some(d.clone());
        }
        let target = match self.displays.get(&node).filter(|d| d.size == size) {
            Some(d) => GpuImage { texture: d.texture.clone(), view: d.view.clone() },
            None => self.gpu.create_image_with_mips(size[0], size[1], DISPLAY_FORMAT, "display"),
        };
        let mut encoder = self.gpu.encoder();
        if let Err(e) = self.gpu.encode_copy_with_mips(&mut encoder, &source, &target, shown_as) {
            log::error!("display copy: {e}");
            return None;
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        let display = DisplayImage { node, version, texture: target.texture, view: target.view, size };
        self.displays.insert(node, display.clone());
        Some(display)
    }

    /// Forget the copies no view shows any more.
    fn prune_displays(&mut self) {
        let (preview, materials) = (self.preview, &self.materials);
        self.displays.retain(|id, _| preview == Some(*id) || materials.contains(id));
    }

    /// What the 2D view shows of `node`. Returns it when it changed (`Some(None)` when there
    /// is nothing to show any more).
    pub fn prepare_preview(&mut self, node: Option<NodeId>) -> Option<Option<DisplayImage>> {
        self.preview = node;
        let shown = node.and_then(|n| self.display(n));
        self.prune_displays();
        let sent = shown.as_ref().map(|d| (d.node, d.version));
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
        self.prune_displays();
        let sent: Vec<(NodeId, u64)> = shown.iter().map(|d| (d.node, d.version)).collect();
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
        let params = [0u32, 0, shown_as, 0];
        let copied = self.gpu.run(&mut encoder, "convert", bytemuck::bytes_of(&params), &[image], &full);
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
