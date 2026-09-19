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

use tessera_core::eval::{Evaluation, VarSource};
use tessera_core::graph::{Graph, Node, NodeId, input_socket_key};
use tessera_core::nodes::{NodeKind, PIXEL_MATH_BUILTINS};

use crate::gpu::{Gpu, GpuImage};
use crate::kernels::{self, DisplayU, ExecCtx, PixelVar};
use crate::simd::{self, ImageStats};

pub const THUMB_SIZE: u32 = 128;
/// Samples in a baked gradient.
pub const LUT_SIZE: usize = 1024;

/// What the engine knows about one node.
#[derive(Default)]
pub struct NodeState {
    pub image: Option<GpuImage>,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EvalReport {
    pub recomputed: usize,
    pub image_nodes: usize,
    pub cpu_ms: f32,
}

/// A small display image of a node (RGBA8, composited over a checkerboard).
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub node: NodeId,
    pub version: u64,
    pub size: u32,
    pub rgba: Vec<u8>,
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

struct DecodedBitmap {
    path: String,
    result: Result<(u32, u32, Vec<u16>), String>,
}

struct Lut {
    hash: u64,
    image: GpuImage,
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
    preview: Option<DisplayImage>,
    /// Display copies of the outputs used by the 3D view, by output node.
    materials: HashMap<NodeId, DisplayImage>,
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
            preview: None,
            materials: HashMap::new(),
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
                (*id, ImageReport { version: s.version, error: s.error.clone(), warning: s.warning.clone() })
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
        self.preview = None;
        self.materials.clear();
        self.gpu.clear_pool();
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
                    let image = self.gpu.create_image(w, h, "bitmap");
                    self.gpu.upload_rgba_f16(&image, &pixels);
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
        let start = Instant::now();
        self.receive_bitmaps();

        // Forget nodes that were deleted.
        let alive: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id).collect();
        self.states.retain(|id, _| alive.contains(id));
        self.luts.retain(|id, _| alive.contains(id));
        self.materials.retain(|id, _| alive.contains(id));
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
        report.cpu_ms = start.elapsed().as_secs_f32() * 1000.0;
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

        let key = hasher.finish();
        let needs_alloc =
            self.states.get(&node.id).is_none_or(|s| s.image.as_ref().is_none_or(|img| img.size() != resolution));
        if !needs_alloc && self.states.get(&node.id).is_some_and(|s| s.key == key) {
            return false;
        }

        let output = if needs_alloc {
            self.gpu.create_image(resolution[0], resolution[1], d.name)
        } else {
            self.states[&node.id].image.clone().expect("allocated")
        };

        if error.is_none() {
            let result = if node.bypass {
                kernels::passthrough(
                    &self.gpu,
                    encoder,
                    inputs.first().and_then(|i| i.as_ref()),
                    &output,
                    params.tiling(),
                )
                .map(|_| None)
            } else {
                match kernels::kernel_for(node.kind) {
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
                }
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
        let Ok(kernel) = self.gpu.kernel("thumbnail") else { return Vec::new() };
        let mut encoder = self.gpu.encoder();
        let mut jobs = Vec::new();
        let params = DisplayU { mode: 0, checker: 8.0, pad: [0; 2] };
        for (id, state) in &mut self.states {
            let Some(image) = &state.image else { continue };
            if state.thumb_version == state.version {
                continue;
            }
            let thumb = self.gpu.take_scratch(THUMB_SIZE, THUMB_SIZE);
            self.gpu.dispatch(
                &mut encoder,
                &kernel,
                bytemuck::bytes_of(&params),
                &[&image.view],
                &thumb.view,
                [THUMB_SIZE, THUMB_SIZE],
            );
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

    // ------------------------------------------------------------------ display images

    fn update_display(&mut self, node: NodeId, current: Option<DisplayImage>) -> Option<DisplayImage> {
        let state = self.states.get(&node)?;
        let source = state.image.clone()?;
        let version = state.version;
        if let Some(c) = &current
            && c.node == node
            && c.version == version
            && c.size == source.size()
        {
            return None;
        }
        let size = source.size();
        let target = match current.filter(|c| c.size == size) {
            Some(c) => GpuImage { texture: c.texture, view: c.view },
            None => self.gpu.create_image_with_mips(size[0], size[1], "display"),
        };
        let mut encoder = self.gpu.encoder();
        if let Err(e) = self.gpu.encode_copy_with_mips(&mut encoder, &source, &target) {
            log::error!("display copy: {e}");
            return None;
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        Some(DisplayImage { node, version, texture: target.texture, view: target.view, size })
    }

    /// Mip-mapped copy of `node` for the 2D view. Returns the image when it changed.
    pub fn prepare_preview(&mut self, node: Option<NodeId>) -> Option<Option<DisplayImage>> {
        let Some(node) = node.filter(|n| self.image(*n).is_some()) else {
            return self.preview.take().map(|_| None);
        };
        let current = self.preview.clone();
        let updated = self.update_display(node, current)?;
        self.preview = Some(updated.clone());
        Some(Some(updated))
    }

    /// Mip-mapped copies of the given output nodes (for the 3D view). Returns the full
    /// set when anything changed.
    pub fn prepare_materials(&mut self, nodes: &[NodeId]) -> Option<Vec<DisplayImage>> {
        let mut changed = false;
        let before = self.materials.len();
        self.materials.retain(|id, _| nodes.contains(id));
        changed |= self.materials.len() != before;
        for &id in nodes {
            let current = self.materials.get(&id).cloned();
            if let Some(updated) = self.update_display(id, current) {
                self.materials.insert(id, updated);
                changed = true;
            } else if self.image(id).is_none() && self.materials.remove(&id).is_some() {
                changed = true;
            }
        }
        changed.then(|| nodes.iter().filter_map(|id| self.materials.get(id).cloned()).collect())
    }

    // ------------------------------------------------------------------ readbacks

    /// RGBA pixels of a node's image (blocking).
    pub fn read_image(&self, node: NodeId) -> Result<(u32, u32, Vec<f32>), String> {
        let image = self.image(node).ok_or("The node has no image")?;
        let pixels = self.gpu.read_image_blocking(image)?;
        Ok((image.width(), image.height(), pixels))
    }

    /// Histogram and statistics (blocking readback, multithreaded SIMD).
    pub fn stats(&self, node: NodeId) -> Option<Arc<ImageStats>> {
        let (w, h, rgba) = self.read_image(node).ok()?;
        Some(Arc::new(simd::image_stats(&rgba, w, h)))
    }

    /// Value of one pixel (blocking).
    pub fn probe(&self, node: NodeId, pixel: [u32; 2]) -> Option<[f32; 4]> {
        let image = self.image(node)?;
        if pixel[0] >= image.width() || pixel[1] >= image.height() {
            return None;
        }
        let mut encoder = self.gpu.encoder();
        let rb = self.gpu.encode_readback_region(&mut encoder, image, pixel);
        self.gpu.queue.submit(Some(encoder.finish()));
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

/// Decode an image file into RGBA half floats (SIMD conversion on all cores).
fn decode_bitmap(path: &str) -> Result<(u32, u32, Vec<u16>), String> {
    let img = image::open(path).map_err(|e| format!("Cannot open '{path}': {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("Unsupported image size {w}x{h} (max 8192)"));
    }
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    match img.color() {
        image::ColorType::L16 | image::ColorType::La16 | image::ColorType::Rgb16 | image::ColorType::Rgba16 => {
            simd::u16_to_f32(&img.into_rgba16().into_raw(), &mut rgba);
        }
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => rgba = img.into_rgba32f().into_raw(),
        _ => simd::u8_to_f32(&img.into_rgba8().into_raw(), &mut rgba),
    }
    let mut half = vec![0u16; rgba.len()];
    simd::f32_to_f16(&rgba, &mut half);
    Ok((w, h, half))
}

/// Convenience for tests and tools: evaluate `graph` from scratch and wait for the GPU.
pub fn evaluate_blocking(engine: &mut Engine, graph: &Graph) -> Evaluation {
    let eval = tessera_core::eval::evaluate(graph);
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
