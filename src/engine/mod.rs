//! Graph evaluation.
//!
//! Every frame [`Engine::evaluate`] walks the graph in topological order and computes an
//! evaluation key per node: a hash of its kind, effective parameters (after exposed
//! parameters were driven by value nodes), resolution and the versions of its inputs.
//! Only nodes whose key changed are recomputed, so dragging a slider re-runs just the
//! affected downstream chain. All image work of one evaluation is recorded into a single
//! command encoder and submitted once.

pub mod exec;
pub mod export;

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use eframe::egui::{self, TextureId};
use eframe::egui_wgpu;
use eframe::wgpu;

use crate::gpu::{Gpu, GpuImage, Readback};
use crate::graph::{
    Graph, Node, NodeId, NodeKind, PIXEL_MATH_BUILTINS, ParamValue, Value, param_socket_key,
    var_socket_key,
};
use crate::simd::ImageStats;
use exec::{DisplayU, EffParams, ExecCtx, PixelVar};

pub const THUMB_SIZE: u32 = 128;

/// Bridge to egui's wgpu renderer so GPU images can be shown in the UI.
pub struct TextureRegistry {
    pub renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    pub device: wgpu::Device,
}

impl TextureRegistry {
    fn sampler(nearest_mag: bool) -> wgpu::SamplerDescriptor<'static> {
        wgpu::SamplerDescriptor {
            label: Some("ferrotex display"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: if nearest_mag { wgpu::FilterMode::Nearest } else { wgpu::FilterMode::Linear },
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        }
    }

    pub fn register(&self, view: &wgpu::TextureView, nearest_mag: bool) -> TextureId {
        self.renderer
            .write()
            .register_native_texture_with_sampler_options(&self.device, view, Self::sampler(nearest_mag))
    }

    pub fn update(&self, id: TextureId, view: &wgpu::TextureView, nearest_mag: bool) {
        self.renderer.write().update_egui_texture_from_wgpu_texture_with_sampler_options(
            &self.device,
            view,
            Self::sampler(nearest_mag),
            id,
        );
    }

    pub fn free(&self, id: TextureId) {
        self.renderer.write().free_texture(&id);
    }
}

/// Where an expression variable got its value from (shown in the properties panel).
#[derive(Clone, Debug, PartialEq)]
pub enum VarSource {
    Link(NodeId),
    GraphInput(String),
    Local,
    Builtin,
    Image(NodeId),
}

#[derive(Clone, Debug)]
pub struct VarInfo {
    pub name: String,
    pub value: f64,
    pub source: VarSource,
}

struct Thumb {
    image: GpuImage,
    id: Option<TextureId>,
    version: u64,
}

#[derive(Default)]
pub struct NodeState {
    pub image: Option<GpuImage>,
    /// Bumped whenever the image or value changes.
    pub version: u64,
    key: u64,
    pub value: Option<Value>,
    pub error: Option<String>,
    pub warning: Option<String>,
    /// Effective values of exposed parameters that are driven by a link.
    pub driven: BTreeMap<String, ParamValue>,
    pub vars: Vec<VarInfo>,
    thumb: Option<Thumb>,
}

struct Preview {
    image: GpuImage,
    id: Option<TextureId>,
    node: NodeId,
    version: u64,
    mode: u32,
}

pub struct PreviewInfo {
    pub texture: Option<TextureId>,
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

struct StatsJob {
    node: NodeId,
    version: u64,
    readback: Readback,
}

struct ProbeJob {
    node: NodeId,
    pixel: [u32; 2],
    version: u64,
    readback: Readback,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EvalReport {
    pub recomputed: usize,
    pub image_nodes: usize,
    pub cpu_ms: f32,
}

pub struct Engine {
    pub gpu: Gpu,
    registry: Option<TextureRegistry>,
    states: HashMap<NodeId, NodeState>,
    next_version: u64,
    preview: Option<Preview>,
    bitmaps: HashMap<String, BitmapEntry>,
    bitmap_generation: u64,
    bitmap_tx: Sender<DecodedBitmap>,
    bitmap_rx: Receiver<DecodedBitmap>,
    stats_job: Option<StatsJob>,
    stats_tx: Sender<(NodeId, u64, ImageStats)>,
    stats_rx: Receiver<(NodeId, u64, ImageStats)>,
    stats_running: bool,
    stats: HashMap<NodeId, (u64, Arc<ImageStats>)>,
    probe_job: Option<ProbeJob>,
    probe: Option<(NodeId, [u32; 2], u64, [f32; 4])>,
    pub report: EvalReport,
}

impl Engine {
    pub fn new(gpu: Gpu, registry: Option<TextureRegistry>) -> Engine {
        let (bitmap_tx, bitmap_rx) = channel();
        let (stats_tx, stats_rx) = channel();
        Engine {
            gpu,
            registry,
            states: HashMap::new(),
            next_version: 1,
            preview: None,
            bitmaps: HashMap::new(),
            bitmap_generation: 1,
            bitmap_tx,
            bitmap_rx,
            stats_job: None,
            stats_tx,
            stats_rx,
            stats_running: false,
            stats: HashMap::new(),
            probe_job: None,
            probe: None,
            report: EvalReport::default(),
        }
    }

    pub fn state(&self, id: NodeId) -> Option<&NodeState> {
        self.states.get(&id)
    }

    pub fn image(&self, id: NodeId) -> Option<&GpuImage> {
        self.states.get(&id).and_then(|s| s.image.as_ref())
    }

    pub fn value(&self, id: NodeId) -> Option<Value> {
        self.states.get(&id).and_then(|s| s.value)
    }

    pub fn thumbnail(&self, id: NodeId) -> Option<TextureId> {
        self.states.get(&id).and_then(|s| s.thumb.as_ref()).and_then(|t| t.id)
    }

    /// Anything still in flight that needs more frames (bitmaps, readbacks)?
    pub fn busy(&self) -> bool {
        self.bitmaps.values().any(|b| matches!(b, BitmapEntry::Loading))
            || self.stats_job.is_some()
            || self.stats_running
            || self.probe_job.is_some()
    }

    fn bump(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    /// Drop every cached result (new document, device change ...).
    pub fn reset(&mut self) {
        let ids: Vec<NodeId> = self.states.keys().copied().collect();
        for id in ids {
            self.remove_state(id);
        }
        self.stats.clear();
        self.probe = None;
        self.gpu.clear_pool();
    }

    fn remove_state(&mut self, id: NodeId) {
        if let Some(state) = self.states.remove(&id) {
            if let (Some(reg), Some(thumb)) = (&self.registry, state.thumb) {
                if let Some(tid) = thumb.id {
                    reg.free(tid);
                }
            }
        }
        self.stats.remove(&id);
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

    /// Forget a cached bitmap so it is decoded again from disk.
    pub fn reload_bitmap(&mut self, path: &str) {
        self.bitmaps.remove(path);
    }

    fn receive_bitmaps(&mut self) {
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
        }
    }

    // ------------------------------------------------------------------ evaluation

    /// Bring every node result up to date with `graph`.
    pub fn evaluate(&mut self, graph: &Graph) {
        let start = Instant::now();
        self.receive_bitmaps();
        self.receive_stats();
        self.gpu.poll();
        self.poll_jobs();

        // Forget nodes that were deleted.
        let stale: Vec<NodeId> = self.states.keys().copied().filter(|id| !graph.contains(*id)).collect();
        for id in stale {
            self.remove_state(id);
        }

        let (order, cyclic) = graph.topo_order();
        for id in cyclic {
            let state = self.states.entry(id).or_default();
            state.error = Some("Node is part of a cycle".into());
            state.image = None;
            state.value = None;
        }

        let resolution = graph.resolution.map(|r| r.clamp(16, 8192));
        let mut encoder = self.gpu.encoder();
        let mut report = EvalReport::default();
        for id in order {
            let Some(node) = graph.node(id) else { continue };
            if node.def().has_image {
                report.image_nodes += 1;
                if self.evaluate_image_node(graph, node, resolution, &mut encoder) {
                    report.recomputed += 1;
                }
            } else {
                self.evaluate_value_node(graph, node);
            }
        }
        self.update_thumbnails(graph, &mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        report.cpu_ms = start.elapsed().as_secs_f32() * 1000.0;
        self.report = report;
    }

    /// Value of the link feeding `socket` of `node`, if connected to a value output.
    fn linked_value(&self, graph: &Graph, node: NodeId, socket: &str) -> Option<(NodeId, Option<Value>)> {
        let link = graph.link_into(node, socket)?;
        let src = self.states.get(&link.from);
        Some((link.from, src.and_then(|s| s.value)))
    }

    /// Stored parameters with exposed + connected ones replaced by driven values.
    fn effective_params(&self, graph: &Graph, node: &Node, driven: &mut BTreeMap<String, ParamValue>) -> EffParams {
        let d = node.def();
        let mut values = Vec::with_capacity(node.params.len());
        for slot in &node.params {
            let mut value = slot.value.clone();
            if slot.exposed {
                if let Some((_, Some(v))) = self.linked_value(graph, node.id, &param_socket_key(&slot.key)) {
                    let count = d.param(&slot.key).map_or(0, |p| p.enum_options().len());
                    if let Some(pv) = slot.value.from_driven(&v, count) {
                        driven.insert(slot.key.clone(), pv.clone());
                        value = pv;
                    }
                }
            }
            values.push((slot.key.clone(), value));
        }
        EffParams { values }
    }

    /// Scalar value for an expression variable that is not linked to an image.
    fn scalar_var(&self, graph: &Graph, node: &Node, name: &str) -> (f64, VarSource) {
        if let Some((from, value)) = self.linked_value(graph, node.id, &var_socket_key(name)) {
            if let Some(v) = value.and_then(|v| v.as_f64()) {
                return (v, VarSource::Link(from));
            }
        }
        if node.bool_value("auto_bind") {
            if let Some(input) = graph.input_by_name(name) {
                if let Some(v) = input.value.as_f64() {
                    return (v, VarSource::GraphInput(input.name.clone()));
                }
            }
        }
        (node.var_values.get(name).copied().unwrap_or(0.0) as f64, VarSource::Local)
    }

    fn evaluate_value_node(&mut self, graph: &Graph, node: &Node) {
        let mut driven = BTreeMap::new();
        let params = self.effective_params(graph, node, &mut driven);
        let mut error = None;
        let mut warning = None;
        let mut vars = Vec::new();
        let value = match node.kind {
            NodeKind::Input => match node.input_ref.and_then(|i| graph.input(i)) {
                Some(input) => Some(input.value),
                None => {
                    error = Some("The graph input this node reads was deleted".into());
                    None
                }
            },
            NodeKind::Math => {
                let compiled = crate::expr::compile_cached(node.text("expression"));
                match &*compiled {
                    Ok(program) => {
                        let mut args = Vec::with_capacity(program.vars.len());
                        for name in &program.vars {
                            let (v, source) = self.scalar_var(graph, node, name);
                            args.push(v);
                            vars.push(VarInfo { name: name.clone(), value: v, source });
                        }
                        let r = program.eval(&args);
                        if !r.is_finite() {
                            warning = Some(format!("Result is not a finite number ({r})"));
                        }
                        Some(Value::Float(r as f32))
                    }
                    Err(e) => {
                        error = Some(format!("Expression is incorrect: {}", e.message));
                        for name in node.expression_vars() {
                            let (v, source) = self.scalar_var(graph, node, &name);
                            vars.push(VarInfo { name, value: v, source });
                        }
                        None
                    }
                }
            }
            NodeKind::ComposeVec2 => Some(Value::Vec2([params.f("x"), params.f("y")])),
            NodeKind::ComposeColor => {
                Some(Value::Color([params.f("r"), params.f("g"), params.f("b"), params.f("a")]))
            }
            _ => None,
        };
        let changed = self.states.get(&node.id).is_none_or(|s| s.value != value);
        let version = if changed { self.bump() } else { self.states[&node.id].version };
        let state = self.states.entry(node.id).or_default();
        state.value = value;
        state.version = version;
        state.error = error;
        state.warning = warning;
        state.driven = driven;
        state.vars = vars;
    }

    /// Returns true when the node was recomputed.
    fn evaluate_image_node(
        &mut self,
        graph: &Graph,
        node: &Node,
        resolution: [u32; 2],
        encoder: &mut wgpu::CommandEncoder,
    ) -> bool {
        let d = node.def();
        let mut driven = BTreeMap::new();
        let params = self.effective_params(graph, node, &mut driven);

        let mut hasher = DefaultHasher::new();
        node.kind.hash(&mut hasher);
        resolution.hash(&mut hasher);
        for (k, v) in &params.values {
            k.hash(&mut hasher);
            v.hash_into(&mut hasher);
        }

        // Image inputs.
        let mut inputs: Vec<Option<GpuImage>> = Vec::with_capacity(d.image_inputs.len());
        for s in d.image_inputs {
            let key = crate::graph::image_socket_key(s.key);
            let src = graph.link_into(node.id, &key).and_then(|l| self.states.get(&l.from).map(|st| (l.from, st)));
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

        let mut error: Option<String> = None;
        let mut warning: Option<String> = None;

        // Bitmap source.
        let mut bitmap: Option<GpuImage> = None;
        if node.kind == NodeKind::Bitmap {
            match self.bitmap_for(node.text("path")) {
                Ok(Some((img, generation))) => {
                    generation.hash(&mut hasher);
                    bitmap = Some(img);
                }
                Ok(None) => {
                    warning = Some("Loading…".into());
                    u64::MAX.hash(&mut hasher);
                }
                Err(e) => {
                    error = Some(e.clone());
                    e.hash(&mut hasher);
                }
            }
        }

        // Pixel Math variables.
        let mut pixel_vars = Vec::new();
        let mut vars_info = Vec::new();
        if node.kind == NodeKind::PixelMath {
            if let Ok(program) = &*crate::expr::compile_cached(node.text("expression")) {
                for name in &program.vars {
                    if let Some(b) = PIXEL_MATH_BUILTINS.iter().find(|b| **b == name.as_str()) {
                        pixel_vars.push(PixelVar::Builtin(b));
                        vars_info.push(VarInfo { name: name.clone(), value: 0.0, source: VarSource::Builtin });
                        (2u8, name).hash(&mut hasher);
                        continue;
                    }
                    let link = graph.link_into(node.id, &var_socket_key(name));
                    let img = link.and_then(|l| self.states.get(&l.from).map(|s| (l.from, s)));
                    if let Some((from, st)) = img.filter(|(_, s)| s.image.is_some()) {
                        (1u8, from.0, st.version).hash(&mut hasher);
                        pixel_vars.push(PixelVar::Image(st.image.clone().expect("checked")));
                        vars_info.push(VarInfo { name: name.clone(), value: 0.0, source: VarSource::Image(from) });
                    } else {
                        let (v, source) = self.scalar_var(graph, node, name);
                        (0u8, (v as f32).to_bits()).hash(&mut hasher);
                        pixel_vars.push(PixelVar::Constant(v as f32));
                        vars_info.push(VarInfo { name: name.clone(), value: v, source });
                    }
                }
            }
        }

        let key = hasher.finish();
        let needs_alloc = self
            .states
            .get(&node.id)
            .is_none_or(|s| s.image.as_ref().is_none_or(|img| img.size() != resolution));
        let unchanged = !needs_alloc && self.states.get(&node.id).is_some_and(|s| s.key == key);
        if unchanged {
            let state = self.states.get_mut(&node.id).expect("exists");
            state.driven = driven;
            state.vars = vars_info;
            return false;
        }

        let output = if needs_alloc {
            self.gpu.create_image(resolution[0], resolution[1], d.name)
        } else {
            self.states[&node.id].image.clone().expect("allocated")
        };

        if error.is_none() {
            let mut cx = ExecCtx {
                gpu: &self.gpu,
                encoder,
                params: &params,
                inputs: &inputs,
                output: &output,
                bitmap: bitmap.as_ref(),
                pixel_vars: &pixel_vars,
            };
            match exec::execute(node.kind, &mut cx) {
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
            let _ = self.gpu.run(encoder, "uniform_color", bytemuck::bytes_of(&[0.0f32; 4]), &[], &output);
        }

        let version = self.bump();
        let state = self.states.entry(node.id).or_default();
        state.image = Some(output);
        state.version = version;
        state.key = key;
        state.error = error;
        state.warning = warning;
        state.driven = driven;
        state.vars = vars_info;
        state.value = None;
        true
    }

    fn update_thumbnails(&mut self, graph: &Graph, encoder: &mut wgpu::CommandEncoder) {
        if self.registry.is_none() {
            return;
        }
        let kernel = match self.gpu.kernel("thumbnail") {
            Ok(k) => k,
            Err(_) => return,
        };
        for node in &graph.nodes {
            let Some(state) = self.states.get_mut(&node.id) else { continue };
            let Some(image) = &state.image else { continue };
            if state.thumb.as_ref().is_some_and(|t| t.version == state.version) {
                continue;
            }
            let thumb = state.thumb.get_or_insert_with(|| Thumb {
                image: self.gpu.create_image(THUMB_SIZE, THUMB_SIZE, "thumbnail"),
                id: None,
                version: 0,
            });
            let u = DisplayU { mode: 0, checker: 8.0, pad: [0; 2] };
            self.gpu.dispatch(
                encoder,
                &kernel,
                bytemuck::bytes_of(&u),
                &[&image.view],
                &thumb.image.view,
                [THUMB_SIZE, THUMB_SIZE],
            );
            thumb.version = state.version;
            if thumb.id.is_none() {
                if let Some(reg) = &self.registry {
                    thumb.id = Some(reg.register(&thumb.image.view, false));
                }
            }
        }
    }

    // ------------------------------------------------------------------ 2D view

    /// Display texture of `node` if it is the one currently prepared for the 2D view.
    pub fn preview_info(&self, node: NodeId) -> Option<PreviewInfo> {
        let p = self.preview.as_ref()?;
        (p.node == node && p.version != 0).then(|| PreviewInfo { texture: p.id, size: p.image.size() })
    }

    /// Prepare the display image (with mipmaps) of `node` for the 2D view.
    /// `mode`: 0 RGBA, 1 RGB, 2 R, 3 G, 4 B, 5 A.
    pub fn prepare_preview(&mut self, node: NodeId, mode: u32) -> Option<PreviewInfo> {
        let state = self.states.get(&node)?;
        let source = state.image.clone()?;
        let version = state.version;
        let size = source.size();

        let recreate = self.preview.as_ref().is_none_or(|p| p.image.size() != size);
        if recreate {
            let image = self.gpu.create_image_with_mips(size[0], size[1], "preview");
            let id = match (&self.registry, self.preview.as_ref().and_then(|p| p.id)) {
                (Some(reg), Some(id)) => {
                    reg.update(id, &image.view, true);
                    Some(id)
                }
                (Some(reg), None) => Some(reg.register(&image.view, true)),
                _ => None,
            };
            self.preview = Some(Preview { image, id, node, version: 0, mode: u32::MAX });
        }
        let preview = self.preview.as_mut().expect("created above");
        if preview.node != node || preview.version != version || preview.mode != mode || recreate {
            let mut encoder = self.gpu.encoder();
            let u = DisplayU { mode, checker: 16.0, pad: [0; 2] };
            let mip0 = preview.image.mip_view(0);
            if let Ok(k) = self.gpu.kernel("preview") {
                self.gpu.dispatch(&mut encoder, &k, bytemuck::bytes_of(&u), &[&source.view], &mip0, size);
            }
            if let Ok(k) = self.gpu.kernel("mip") {
                let levels = preview.image.texture.mip_level_count();
                for level in 1..levels {
                    let src = preview.image.mip_view(level - 1);
                    let dst = preview.image.mip_view(level);
                    let w = (size[0] >> level).max(1);
                    let h = (size[1] >> level).max(1);
                    self.gpu.dispatch(&mut encoder, &k, bytemuck::bytes_of(&u), &[&src], &dst, [w, h]);
                }
            }
            self.gpu.queue.submit(Some(encoder.finish()));
            preview.node = node;
            preview.version = version;
            preview.mode = mode;
        }
        Some(PreviewInfo { texture: preview.id, size })
    }

    // ------------------------------------------------------------------ readbacks

    fn poll_jobs(&mut self) {
        if let Some(job) = &self.stats_job {
            match job.readback.poll() {
                Some(Ok(())) => {
                    let job = self.stats_job.take().expect("checked");
                    let half = job.readback.to_f16();
                    let (w, h) = (job.readback.width, job.readback.height);
                    let tx = self.stats_tx.clone();
                    self.stats_running = true;
                    rayon::spawn(move || {
                        let mut rgba = vec![0.0f32; half.len()];
                        crate::simd::f16_to_f32(&half, &mut rgba);
                        let stats = crate::simd::image_stats(&rgba, w, h);
                        let _ = tx.send((job.node, job.version, stats));
                    });
                }
                Some(Err(_)) => self.stats_job = None,
                None => {}
            }
        }
        if let Some(job) = &self.probe_job {
            match job.readback.poll() {
                Some(Ok(())) => {
                    let job = self.probe_job.take().expect("checked");
                    let px = job.readback.to_f32();
                    if px.len() >= 4 {
                        self.probe = Some((job.node, job.pixel, job.version, [px[0], px[1], px[2], px[3]]));
                    }
                }
                Some(Err(_)) => self.probe_job = None,
                None => {}
            }
        }
    }

    fn receive_stats(&mut self) {
        while let Ok((node, version, stats)) = self.stats_rx.try_recv() {
            self.stats_running = false;
            self.stats.insert(node, (version, Arc::new(stats)));
        }
    }

    /// Histogram and statistics of a node's image; computed in the background
    /// (GPU readback + multithreaded SIMD). Returns the latest available result and
    /// whether it matches the current image.
    pub fn stats(&mut self, node: NodeId) -> Option<(Arc<ImageStats>, bool)> {
        let version = self.states.get(&node)?.version;
        let cached = self.stats.get(&node).cloned();
        let fresh = cached.as_ref().is_some_and(|(v, _)| *v == version);
        if !fresh && self.stats_job.is_none() && !self.stats_running {
            if let Some(image) = self.image(node).cloned() {
                let mut encoder = self.gpu.encoder();
                let readback = self.gpu.encode_readback(&mut encoder, &image);
                self.gpu.queue.submit(Some(encoder.finish()));
                readback.map();
                self.stats_job = Some(StatsJob { node, version, readback });
            }
        }
        cached.map(|(_, s)| (s, fresh))
    }

    /// Value of one pixel of a node's image (asynchronous; returns the last result).
    pub fn probe(&mut self, node: NodeId, pixel: [u32; 2]) -> Option<[f32; 4]> {
        let version = self.states.get(&node)?.version;
        let current = self.probe.filter(|p| p.0 == node && p.1 == pixel && p.2 == version).map(|p| p.3);
        if current.is_none() && self.probe_job.is_none() {
            let image = self.image(node)?.clone();
            if pixel[0] < image.width() && pixel[1] < image.height() {
                let mut encoder = self.gpu.encoder();
                let readback = self.gpu.encode_readback_region(&mut encoder, &image, pixel);
                self.gpu.queue.submit(Some(encoder.finish()));
                readback.map();
                self.probe_job = Some(ProbeJob { node, pixel, version, readback });
            }
        }
        current.or_else(|| self.probe.filter(|p| p.0 == node).map(|p| p.3))
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
    let sixteen_bit = matches!(
        img.color(),
        image::ColorType::L16 | image::ColorType::La16 | image::ColorType::Rgb16 | image::ColorType::Rgba16
    );
    if sixteen_bit {
        let data = img.into_rgba16().into_raw();
        crate::simd::u16_to_f32(&data, &mut rgba);
    } else {
        let data = img.into_rgba8().into_raw();
        crate::simd::u8_to_f32(&data, &mut rgba);
    }
    let mut half = vec![0u16; rgba.len()];
    crate::simd::f32_to_f16(&rgba, &mut half);
    Ok((w, h, half))
}

/// Convenience for tests and tools: evaluate a graph and read one node back.
pub fn read_node(engine: &Engine, id: NodeId) -> Option<Vec<f32>> {
    engine.image(id).and_then(|img| engine.gpu.read_image_blocking(img).ok())
}


#[cfg(test)]
mod tests;
