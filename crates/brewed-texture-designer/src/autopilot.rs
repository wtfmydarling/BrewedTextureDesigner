//! The autopilot: the app driving itself (`--autopilot [MINUTES] [--seed N]`), to find what
//! breaks before a person does. In the real window, on the GPU and the engine thread as they
//! are, it goes through scenes (the texture size up to 8192, dragging every input value and
//! parameters, showing every node, adding every kind of node, typed values far past the
//! sliders, undo and redo, saving and opening, broken files too, exporting and reading the
//! files back, pictures for Bitmap nodes from one pixel to 4096², the 3D view, the pictures of
//! the options, documents changed while the engine works, a graph of 207 nodes, input values
//! at their edges, odd names of outputs, the window resized, an export at 8192, packages whose
//! graphs are used as nodes, saved, opened, moved and found again), then does
//! random things until its time is up, and quits. Everything it does goes to a log with the
//! time, the engine's memory and how fast the window draws, and what went wrong is marked
//! FINDING: whatever breaks can be followed back, and the same seed does the same things.
//!
//! It stays inside the app: no file dialog, no clipboard, no eyedropper; its files are in a
//! folder of its own in the system's temporary folder.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use brewed_texture_core::graph::{input_socket_key, output_socket_key};
use brewed_texture_core::{DataType, Edit, NodeId, NodeKind, ParamDef, ParamKind, ParamValue, SocketRef, Value};

use crate::app::versions::VersionsMessage;
use crate::app::{App, Dialog, DragPayload, ExplorerMessage, GraphMessage, Message, PendingAction, PreviewMessage, SelectMode, SettingsMessage};
use crate::settings::{FileDialogs, Page, Setting, Settings};
use crate::view::view3d::{Camera, Move, Shape};

/// How long the autopilot flies, and on which seed.
#[derive(Clone, Copy, Debug)]
pub struct Flight {
    pub minutes: f32,
    pub seed: u64,
}

/// What the autopilot does on its turn.
pub enum Turn {
    /// A message, as the interface sends it.
    Send(Message),
    /// The window changed, as a person changes it by its frame.
    Window(WindowChange),
}

#[derive(Clone, Copy, Debug)]
pub enum WindowChange {
    Resize(iced::Size),
    Maximize(bool),
}

/// What the autopilot does next.
enum Step {
    /// A message, as the interface sends it.
    Do(Message),
    /// The window changed.
    Window(WindowChange),
    /// Wait until the engine has done all it was asked.
    Wait,
    /// Wait this long (the engine settles half a second after the last change).
    Pause(Duration),
    /// Write down where things are: the engine's memory.
    Note(String),
    /// Something to check about the app now: what is wrong, if anything.
    Check(Check),
    /// Take away a folder of files the flight is done with (written, read back and checked):
    /// they would only fill the disk.
    Tidy(PathBuf),
}

type Check = Box<dyn Fn(&App) -> Option<String>>;

type Batch = Box<dyn FnMut(&App, &mut Dice) -> Option<Vec<Step>>>;

/// A part of the flight: what to do next with the app as it is, `None` once it is over.
struct Scene {
    name: &'static str,
    next: Batch,
}

/// Numbers for the autopilot's choices: the same seed, the same flight.
pub struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn float(&mut self, lo: f32, hi: f32) -> f32 {
        lo + ((self.next() >> 40) as f32 / (1u64 << 24) as f32) * (hi - lo)
    }
}

pub struct Autopilot {
    flight: Flight,
    dice: Dice,
    started: Instant,
    log: std::io::BufWriter<std::fs::File>,
    scenes: VecDeque<Scene>,
    scene: Option<Scene>,
    steps: VecDeque<Step>,
    waiting_since: Option<Instant>,
    pause_until: Option<Instant>,
    done: bool,
    actions: u64,
    findings: Vec<String>,
    most_memory: u64,
    /// When the autopilot last had its turn, and the longest the interface kept it waiting
    /// between two turns in this scene (busy with what it was told, or drawing).
    last_turn: Option<Instant>,
    longest_wait: Duration,
    /// The device the UI and the engine share, for what its allocator holds (where it says).
    device: Option<brewed_texture_engine::wgpu::Device>,
}

/// A number of MB.
fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// The folder of the flight of `seed`: its log, the files it reads and writes, and the unsaved
/// work kept for recovery as it flies (in `recovery`).
pub fn folder(seed: u64) -> PathBuf {
    std::env::temp_dir().join(format!("brewed-autopilot-{seed}"))
}

impl Autopilot {
    pub fn new(flight: Flight) -> Result<Autopilot, String> {
        let folder = folder(flight.seed);
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let file = std::fs::File::create(folder.join("autopilot.log")).map_err(|e| e.to_string())?;
        make_files(&folder)?;
        // A panic, wherever it happens, is written down with where it happened.
        let panics = folder.join("panic.log");
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let trace = std::backtrace::Backtrace::force_capture();
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&panics) {
                let _ = writeln!(f, "{info}\n{trace}");
            }
            report(info);
        }));
        let mut pilot = Autopilot {
            flight,
            dice: Dice(flight.seed.max(1)),
            started: Instant::now(),
            log: std::io::BufWriter::new(file),
            scenes: scenes(&folder),
            scene: None,
            steps: VecDeque::new(),
            waiting_since: None,
            pause_until: None,
            done: false,
            actions: 0,
            findings: Vec::new(),
            most_memory: 0,
            last_turn: None,
            longest_wait: Duration::ZERO,
            device: None,
        };
        pilot.write(&format!(
            "autopilot, seed {}, {} minutes, files in {}",
            flight.seed,
            flight.minutes,
            folder.display()
        ));
        println!("autopilot: the log is {}", folder.join("autopilot.log").display());
        Ok(pilot)
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The device the UI and the engine share: its allocator's memory goes into the notes.
    pub fn watch(&mut self, device: brewed_texture_engine::wgpu::Device) {
        self.device = Some(device);
    }

    fn write(&mut self, line: &str) {
        let t = self.started.elapsed().as_secs_f64();
        let _ = writeln!(self.log, "[{t:9.3} s] {line}");
        let _ = self.log.flush();
    }

    /// Something that went wrong.
    pub fn finding(&mut self, what: String) {
        self.write(&format!("FINDING: {what}"));
        self.findings.push(what);
    }

    /// The status bar said something (errors are written down: some are what a scene
    /// wants, such as a broken file refused).
    pub fn status(&mut self, text: &str, error: bool) {
        if error {
            self.write(&format!("status error: {text}"));
        }
    }

    /// The engine finished a change: how long it took.
    pub fn finished(&mut self, millis: f32, recomputed: usize) {
        if millis > 2000.0 {
            self.finding(format!("a change took {millis:.0} ms ({recomputed} nodes)"));
        }
    }

    /// What to do on this tick (nothing while it waits).
    pub fn tick(&mut self, app: &App) -> Option<Turn> {
        if self.done {
            return None;
        }
        self.most_memory = self.most_memory.max(app.res.memory.total());
        let now = Instant::now();
        // Turns come every 16 ms while the interface is free: a longer wait is a moment it was
        // not, which a person would feel.
        if let Some(last) = self.last_turn.replace(now) {
            let wait = now - last;
            self.longest_wait = self.longest_wait.max(wait);
            if wait > Duration::from_millis(1000) {
                self.finding(format!("the interface did not answer for {} ms", wait.as_millis()));
            } else if wait > Duration::from_millis(250) {
                self.write(&format!("the interface was busy for {} ms", wait.as_millis()));
            }
        }
        if let Some(since) = self.waiting_since {
            if !app.engine_idle() || !app.files_idle() {
                if now - since > Duration::from_secs(90) {
                    self.finding("the engine did not finish what it was asked in 90 s".into());
                    self.waiting_since = None;
                }
                return None;
            }
            self.waiting_since = None;
            // What the engine says once it finished (its memory, the views' images) comes
            // right after: a moment for it.
            self.pause_until = Some(now + Duration::from_millis(50));
        }
        if let Some(until) = self.pause_until {
            if now < until {
                return None;
            }
            self.pause_until = None;
        }
        loop {
            let Some(step) = self.steps.pop_front() else {
                if !self.next_batch(app) {
                    self.done = true;
                    self.summary(app);
                    self.tidy();
                    return None;
                }
                continue;
            };
            match step {
                Step::Do(message) => {
                    self.actions += 1;
                    let text: String = format!("{message:?}").chars().take(160).collect();
                    self.write(&format!("do {text}"));
                    // One a tick: as fast as a person dragging.
                    return Some(Turn::Send(message));
                }
                Step::Window(change) => {
                    self.actions += 1;
                    self.write(&format!("window {change:?}"));
                    return Some(Turn::Window(change));
                }
                Step::Wait => {
                    self.waiting_since = Some(now);
                    return None;
                }
                Step::Pause(d) => {
                    self.pause_until = Some(now + d);
                    return None;
                }
                Step::Tidy(dir) => {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                Step::Note(what) => {
                    let m = app.res.memory;
                    let memory = format!(
                        "{:.1} MB (views {:.1}, kept {:.1}, scratch {:.1}, files {:.1})",
                        mb(m.total()),
                        mb(m.shown),
                        mb(m.kept),
                        mb(m.scratch),
                        mb(m.files)
                    );
                    let nodes = app.graph().nodes.len();
                    // How fast the window draws (the 3D view draws it frame after frame in real time).
                    let rate = app.ui.frame_rate;
                    let drawn = format!("{:.0} FPS (worst frame {:.0} ms)", rate.fps, rate.worst_ms);
                    // What the device's allocator holds, UI and engine together (D3D12 says).
                    let report = self.device.as_ref().and_then(|d| d.generate_allocator_report());
                    let allocator = report.map_or_else(String::new, |r| {
                        let (taken, held) = (mb(r.total_allocated_bytes), mb(r.total_reserved_bytes));
                        format!(", allocator {taken:.1} MB of {held:.1}")
                    });
                    self.write(&format!("NOTE {what}: {memory}, {nodes} nodes, {drawn}{allocator}"));
                }
                Step::Check(check) => {
                    if let Some(problem) = check(app) {
                        self.finding(problem);
                    }
                }
            }
        }
    }

    /// The next steps of the scene, or of the next scene; `false` once all are done.
    fn next_batch(&mut self, app: &App) -> bool {
        loop {
            if let Some(scene) = &mut self.scene {
                let over_time =
                    scene.name == "random" && self.started.elapsed().as_secs_f32() > self.flight.minutes * 60.0;
                if !over_time && let Some(steps) = (scene.next)(app, &mut self.dice) {
                    self.steps.extend(steps);
                    return true;
                }
                let name = scene.name;
                let longest = std::mem::take(&mut self.longest_wait).as_millis();
                self.write(&format!(
                    "SCENE {name} over; the interface kept the autopilot waiting {longest} ms at most"
                ));
            }
            match self.scenes.pop_front() {
                Some(scene) => {
                    self.write(&format!("SCENE {}", scene.name));
                    self.scene = Some(scene);
                }
                None => {
                    self.scene = None;
                    return false;
                }
            }
        }
    }

    /// The flight is over: of what it wrote, only its log stays, when it found nothing (the
    /// files it made and exported take a few hundred MB a flight, and every seed has a folder);
    /// what it found is looked into with the files it was found with.
    fn tidy(&self) {
        if !self.findings.is_empty() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(folder(self.flight.seed)) else { return };
        for path in entries.flatten().map(|e| e.path()) {
            if path.file_name().is_some_and(|n| n == "autopilot.log") {
                continue;
            }
            // (What the program still holds, its recovery session, it takes away as it quits.)
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
    }

    fn summary(&mut self, app: &App) {
        let lines = [
            format!("{} actions in {:.1} min", self.actions, self.started.elapsed().as_secs_f64() / 60.0),
            format!("the most memory the engine said it took: {:.0} MB", mb(self.most_memory)),
            format!("at the end: {:.0} MB, {} nodes", mb(app.res.memory.total()), app.graph().nodes.len()),
            format!("{} findings", self.findings.len()),
        ];
        for line in lines {
            self.write(&line);
            println!("autopilot: {line}");
        }
        for f in &self.findings {
            println!("autopilot: FINDING {f}");
        }
    }
}

// ---------------------------------------------------------------------------- the files

/// The files the scenes use: pictures for Bitmap nodes (good ones of odd sizes and depths,
/// and broken ones), broken packages, and the folder exports go to.
fn make_files(folder: &Path) -> Result<(), String> {
    let e = |e: image::ImageError| e.to_string();
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255])).save(folder.join("one_pixel.png")).map_err(e)?;
    image::RgbaImage::from_fn(17, 9, |x, y| image::Rgba([(x * 15) as u8, (y * 28) as u8, 128, 200]))
        .save(folder.join("odd.png"))
        .map_err(e)?;
    image::RgbaImage::from_fn(4096, 4096, |x, y| image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255]))
        .save(folder.join("big.png"))
        .map_err(e)?;
    image::ImageBuffer::<image::Luma<u16>, _>::from_fn(1024, 1024, |x, y| image::Luma([((x * 64) ^ (y * 64)) as u16]))
        .save(folder.join("gray16.png"))
        .map_err(e)?;
    image::Rgba32FImage::from_fn(256, 256, |x, y| image::Rgba([x as f32 / 64.0, y as f32 / 64.0, 0.5, 1.0]))
        .save(folder.join("hdr.exr"))
        .map_err(e)?;
    // An environment for the 3D view: a sky with a sun, over a darker ground.
    let (w, h) = (512, 256);
    let sky: Vec<image::Rgb<f32>> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let up = 1.0 - (y as f32 + 0.5) / h as f32 * 2.0;
                let sun = (x as f32 - 300.0).powi(2) + (y as f32 - 80.0).powi(2) < 16.0;
                image::Rgb(if sun { [900.0, 850.0, 700.0] } else if up > 0.0 { [0.5 + up, 0.7 + up, 1.0 + up] } else { [0.15, 0.12, 0.1] })
            })
        })
        .collect();
    let file = std::fs::File::create(folder.join("sky.hdr")).map_err(|e| e.to_string())?;
    image::codecs::hdr::HdrEncoder::new(file).encode(&sky, w, h).map_err(e)?;
    let write = |name: &str, bytes: &[u8]| std::fs::write(folder.join(name), bytes).map_err(|e| e.to_string());
    write("broken.png", b"\x89PNG\r\n\x1a\nnot really")?;
    write("empty.png", b"")?;
    write("text.png", b"just some text in a file called a picture")?;
    // Packages cut short, damaged (a byte of a graph changed: its CRC-32 says so), of another
    // program, and not archives at all.
    let (demo, _) = brewed_texture_core::demo::package();
    let whole = brewed_texture_core::project::to_bytes(&demo, &Default::default(), demo.first(), None)?;
    write("broken.brewtex", &whole[..whole.len() / 2])?;
    let mut damaged = whole.clone();
    let at = damaged.len() / 2;
    damaged[at] ^= 0x55;
    write("damaged.brewtex", &damaged)?;
    let mut zip = brewed_texture_core::archive::Writer::new(Vec::new());
    zip.add("hello.txt", b"world", brewed_texture_core::archive::Method::Deflated).map_err(|e| e.to_string())?;
    write("other.brewtex", &zip.finish().map_err(|e| e.to_string())?)?;
    write("empty.brewtex", b"")?;
    write("text.brewtex", b"{ \"format\": \"brewed-texture-package\", \"graphs\": [1, 2, ")?;
    std::fs::create_dir_all(folder.join("export")).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------- the scenes

/// The files in `folder`; else why they cannot be listed (a folder an export was to fill that is
/// not there is found wanting, not passed over).
fn files_in(folder: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?.flatten().map(|e| e.path()).collect())
}

fn edit(e: Edit) -> Step {
    Step::Do(Message::Edit(e))
}

fn preview(node: NodeId) -> Step {
    Step::Do(Message::Graph(GraphMessage::Preview(node)))
}

/// The output the demo shows first, or the first output.
fn home(app: &App) -> Option<NodeId> {
    let g = app.graph();
    g.nodes_of(NodeKind::OUTPUT)
        .find(|n| n.text("name") == "basecolor")
        .or_else(|| g.nodes_of(NodeKind::OUTPUT).next())
        .map(|n| n.id)
}

/// Back to the demo as it opens, settled: it takes what it took when it opened, whatever was
/// done before.
fn back_home(what: &'static str) -> Vec<Step> {
    vec![
        Step::Do(Message::Proceed(PendingAction::LoadDemo)),
        Step::Wait,
        Step::Pause(Duration::from_millis(1200)),
        Step::Note(format!("the demo again, settled, after {what}")),
        Step::Check(Box::new(move |app: &App| {
            let m = app.res.memory;
            // The five maps it writes with their mips: 64 MB at 2048.
            (m.kept + m.scratch > 0 || m.total() > 65 * 1024 * 1024).then(|| {
                format!(
                    "after {what}: {:.1} MB (views {:.1}, kept {:.1}, scratch {:.1}, files {:.1}), not 64",
                    mb(m.total()),
                    mb(m.shown),
                    mb(m.kept),
                    mb(m.scratch),
                    mb(m.files)
                )
            })
        })),
    ]
}

/// The settings dialog, as people go through it: every page, values changed and cancelled,
/// changed and applied (a history held to fewer steps at once), every page set back as it is
/// by default.
fn settings_scene() -> Vec<Step> {
    let say = |m: SettingsMessage| Step::Do(Message::Settings(m));
    let set = |s: Setting| say(SettingsMessage::Set(s));
    let mut steps = vec![say(SettingsMessage::Open)];
    steps.extend(Page::ALL.into_iter().map(|page| say(SettingsMessage::Page(page))));
    steps.extend([
        set(Setting::UndoSteps(20)),
        set(Setting::InvertZoom(true)),
        say(SettingsMessage::Cancel),
        Step::Check(Box::new(|app: &App| (app.settings != Settings::default()).then(|| "cancelled, and taken all the same".into()))),
        say(SettingsMessage::Open),
        set(Setting::UndoSteps(20)),
        set(Setting::RecentPackages(3)),
        set(Setting::InvertZoom(true)),
        set(Setting::FileDialogs(FileDialogs::Last)),
        say(SettingsMessage::Apply),
        say(SettingsMessage::Cancel),
    ]);
    // Twenty-five steps done: twenty can be undone.
    for i in 0..25 {
        steps.extend([edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [i as f32 * 20.0, 7777.0] }), Step::Do(Message::Seal)]);
    }
    steps.extend((0..25).map(|_| Step::Do(Message::Undo)));
    steps.extend([
        Step::Wait,
        Step::Check(Box::new(|app: &App| {
            let left = app.graph().nodes_of(NodeKind::BLUR).filter(|n| n.pos[1] == 7777.0).count();
            (left != 5).then(|| format!("undo went back {} steps of 25, not 20", 25 - left))
        })),
        say(SettingsMessage::Open),
    ]);
    steps.extend(Page::ALL.into_iter().flat_map(|page| [say(SettingsMessage::Page(page)), say(SettingsMessage::ResetPage)]));
    steps.extend([
        say(SettingsMessage::Apply),
        say(SettingsMessage::Cancel),
        Step::Check(Box::new(|app: &App| {
            (app.settings != Settings::default() || app.ui.dialog.is_some() || app.ui.restart_offer)
                .then(|| "every page set back, and the settings are not as by default".into())
        })),
    ]);
    steps.extend(back_home("the settings"));
    steps
}

/// A value for a parameter: in its slider's range, or, `far`, what can be typed past it (up
/// to its limits).
fn value_for(pd: &ParamDef, dice: &mut Dice, far: bool) -> Option<ParamValue> {
    let limit = |v: f32| pd.limits.map_or(v, |(lo, hi)| v.clamp(lo, hi));
    Some(match pd.kind {
        ParamKind::Float { min, max, .. } => {
            let v = if far {
                let span = (max - min).abs().max(1.0);
                [max + span * 10.0, min - span * 10.0, 1e6, -1e6, 0.0][dice.below(5)]
            } else {
                dice.float(min, max)
            };
            ParamValue::Float(limit(v))
        }
        ParamKind::Int { min, max, .. } => {
            let v = if far {
                [max as f32 * 10.0 + 10.0, min as f32 - 100.0, 100_000.0][dice.below(3)]
            } else {
                dice.float(min as f32, max as f32).round()
            };
            ParamValue::Int(limit(v) as i32)
        }
        ParamKind::Vec2 { min, max, .. } => {
            let mut v = || if far { [max * 10.0, min * 10.0, 1e5][dice.below(3)] } else { dice.float(min, max) };
            ParamValue::Vec2([limit(v()), limit(v())])
        }
        ParamKind::Color { .. } => {
            let mut c = || if far { dice.float(-10.0, 10.0) } else { dice.float(0.0, 1.0) };
            ParamValue::Color([c(), c(), c(), c()])
        }
        ParamKind::Enum { options, .. } => ParamValue::Enum(dice.below(options.len()) as u32),
        ParamKind::Bool { .. } => ParamValue::Bool(dice.chance(50)),
        _ => return None,
    })
}

/// A value on the way along a drag from one end of a range to the other and back (`t` from
/// 0 to 1).
fn swept(value: &Value, min: &Value, max: &Value, t: f32) -> Value {
    let t = 1.0 - (2.0 * t - 1.0).abs();
    match (value, min, max) {
        (Value::Float(_), Value::Float(lo), Value::Float(hi)) => Value::Float(lo + (hi - lo) * t),
        (Value::Int(_), Value::Int(lo), Value::Int(hi)) => Value::Int(lo + ((hi - lo) as f32 * t).round() as i32),
        (Value::Vec2(_), Value::Vec2(lo), Value::Vec2(hi)) => {
            Value::Vec2([lo[0] + (hi[0] - lo[0]) * t, lo[1] + (hi[1] - lo[1]) * t])
        }
        (Value::Color(_), _, _) => Value::Color([t, 1.0 - t, 0.5 * t, 1.0]),
        (Value::Bool(_), _, _) => Value::Bool(t > 0.5),
        (v, _, _) => *v,
    }
}

/// The image nodes of the graph.
fn image_nodes(app: &App) -> Vec<&brewed_texture_core::Node> {
    app.graph().nodes.iter().filter(|n| n.def().has_image()).collect()
}

/// The scenes, in the order they are flown.
fn scenes(folder: &Path) -> VecDeque<Scene> {
    let mut scenes = VecDeque::new();
    let once = |steps: Vec<Step>| -> Batch {
        let mut steps = Some(steps);
        Box::new(move |_: &App, _: &mut Dice| steps.take())
    };

    scenes.push_back(Scene {
        name: "opening",
        next: once(vec![Step::Wait, Step::Pause(Duration::from_millis(1500)), Step::Note("opened, settled".into())]),
    });

    let sizes = [256, 512, 1024, 2048, 4096, 8192, 16, 2048];
    let mut i = 0;
    scenes.push_back(Scene {
        name: "texture sizes",
        next: Box::new(move |_: &App, _: &mut Dice| {
            let s = *sizes.get(i)?;
            i += 1;
            let mut steps = vec![
                edit(Edit::SetResolution([s, s])),
                Step::Do(Message::Seal),
                Step::Wait,
                Step::Note(format!("{s}x{s}")),
            ];
            if i == sizes.len() {
                steps.extend(back_home("the texture sizes"));
            }
            Some(steps)
        }),
    });

    let mut v = 0;
    scenes.push_back(Scene {
        name: "dragging the input values",
        next: Box::new(move |app: &App, _: &mut Dice| {
            let var = app.graph().variables.get(v)?.clone();
            v += 1;
            let mut steps: Vec<Step> = (0..=120)
                .map(|k| {
                    let value = swept(&var.value, &var.min, &var.max, k as f32 / 120.0);
                    edit(Edit::SetVariable { name: var.name.clone(), value })
                })
                .collect();
            steps.extend([Step::Do(Message::Seal), Step::Wait, Step::Note(format!("dragged {}", var.name))]);
            if v == app.graph().variables.len() {
                steps.extend(back_home("dragging the input values"));
            }
            Some(steps)
        }),
    });

    let mut dragged = 0;
    scenes.push_back(Scene {
        name: "dragging parameters",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            if dragged == 12 {
                return None;
            }
            dragged += 1;
            let nodes = image_nodes(app);
            let node = *nodes.get(dice.below(nodes.len()))?;
            let params: Vec<&ParamDef> = node
                .def()
                .params
                .iter()
                .filter(|p| {
                    matches!(p.kind, ParamKind::Float { .. } | ParamKind::Int { .. }) && !node.is_exposed(p.key)
                })
                .collect();
            let mut steps = Vec::new();
            if let Some(pd) = params.get(dice.below(params.len())) {
                let (id, key) = (node.id, pd.key.to_string());
                steps.push(Step::Do(Message::Graph(GraphMessage::Select { node: id, mode: SelectMode::Replace })));
                for k in 0..=60 {
                    let t = 1.0 - (2.0 * k as f32 / 60.0 - 1.0).abs();
                    let value = match pd.kind {
                        ParamKind::Float { min, max, .. } => ParamValue::Float(min + (max - min) * t),
                        ParamKind::Int { min, max, .. } => {
                            ParamValue::Int(min + ((max - min) as f32 * t).round() as i32)
                        }
                        _ => continue,
                    };
                    steps.push(edit(Edit::SetParam { node: id, key: key.clone(), value }));
                }
                steps.extend([
                    Step::Do(Message::Seal),
                    Step::Wait,
                    Step::Note(format!("dragged {} {}", node.def().name, pd.key)),
                ]);
            }
            if dragged == 12 {
                steps.extend(back_home("dragging parameters"));
            }
            Some(steps)
        }),
    });

    let mut shown = 0;
    scenes.push_back(Scene {
        name: "showing every node",
        next: Box::new(move |app: &App, _: &mut Dice| {
            let nodes = image_nodes(app);
            if shown > nodes.len() {
                return None;
            }
            shown += 1;
            let Some(n) = nodes.get(shown - 1) else {
                let mut steps: Vec<Step> = home(app).map(preview).into_iter().collect();
                steps.extend(back_home("showing every node"));
                return Some(steps);
            };
            let (id, name) = (n.id, n.def().name);
            Some(vec![
                preview(id),
                Step::Wait,
                Step::Note(format!("showing {name}")),
                Step::Check(Box::new(move |app: &App| {
                    if let Some(e) = app.res.reports.get(&id).and_then(|r| r.error.clone()) {
                        return Some(format!("{name} of the demo: {e}"));
                    }
                    let showing = app.res.preview.as_ref().map(|p| p.node);
                    (showing != Some(id)).then(|| format!("the 2D view shows {showing:?}, not {name} {id:?}"))
                })),
            ])
        }),
    });

    let mut kind = 0;
    let mut added: Vec<NodeId> = Vec::new();
    scenes.push_back(Scene {
        name: "every kind of node",
        next: Box::new(move |app: &App, _: &mut Dice| {
            let g = app.graph();
            // What the last step added (the node with the highest id): connected, and shown.
            if kind > 0
                && let Some(new) = g.nodes.iter().map(|n| n.id).max_by_key(|id| id.0)
                && !added.contains(&new)
                && let Some(node) = g.node(new)
            {
                added.push(new);
                let source = g.nodes.iter().find(|n| {
                    n.kind != NodeKind::OUTPUT && n.def().has_image() && !n.def().outputs.is_empty() && n.id != new
                });
                let mut steps = Vec::new();
                if let Some(source) = source {
                    for s in node.def().inputs.iter().filter(|s| s.ty == DataType::Image) {
                        steps.push(edit(Edit::Connect {
                            a: SocketRef {
                                node: source.id,
                                key: output_socket_key(source.def().outputs[0].key),
                                output: true,
                            },
                            b: SocketRef { node: new, key: input_socket_key(s.key), output: false },
                        }));
                    }
                }
                steps.push(Step::Do(Message::Seal));
                if node.def().has_image() {
                    // (A Bitmap or an SVG has no file yet: it says so.)
                    let expected = node.kind == NodeKind::BITMAP || node.kind == NodeKind::SVG;
                    let name = node.def().name;
                    steps.extend([
                        preview(new),
                        Step::Wait,
                        Step::Note(format!("added {name}")),
                        Step::Check(Box::new(move |app: &App| {
                            let error = app.res.reports.get(&new).and_then(|r| r.error.clone());
                            error.filter(|_| !expected).map(|e| format!("a new {name}, connected: {e}"))
                        })),
                    ]);
                }
                return Some(steps);
            }
            let all = NodeKind::ALL;
            if kind < all.len() {
                let k = all[kind];
                kind += 1;
                return Some(vec![edit(Edit::AddNode { kind: k, pos: [200.0 * kind as f32, 1400.0] })]);
            }
            if kind == all.len() {
                kind += 1;
                let mut steps = vec![edit(Edit::RemoveNodes(added.clone())), Step::Do(Message::Seal), Step::Wait];
                steps.extend(back_home("adding every kind of node"));
                return Some(steps);
            }
            None
        }),
    });

    let mut far = 0;
    scenes.push_back(Scene {
        name: "values typed far past the sliders",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            if far == 40 {
                return None;
            }
            far += 1;
            let nodes: Vec<_> = image_nodes(app).into_iter().filter(|n| !n.def().params.is_empty()).collect();
            let node = *nodes.get(dice.below(nodes.len()))?;
            let params: Vec<&ParamDef> = node.def().params.iter().filter(|p| !node.is_exposed(p.key)).collect();
            let mut steps = Vec::new();
            if let Some(pd) = params.get(dice.below(params.len()))
                && let Some(value) = value_for(pd, dice, true)
            {
                let text = format!("{} {} = {value:?}", node.def().name, pd.key);
                steps.extend([
                    preview(node.id),
                    edit(Edit::SetParam { node: node.id, key: pd.key.into(), value }),
                    Step::Do(Message::Seal),
                    Step::Wait,
                    Step::Note(text),
                ]);
            }
            if far == 40 {
                steps.extend(back_home("values typed far past the sliders"));
            }
            Some(steps)
        }),
    });

    let mut storm = 0;
    let mut nodes_before = 0;
    scenes.push_back(Scene {
        name: "undo and redo",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            storm += 1;
            match storm {
                1 => {
                    // Forty changes of every kind, to undo.
                    let mut steps = Vec::new();
                    let g = app.graph();
                    for _ in 0..40 {
                        let n = &g.nodes[dice.below(g.nodes.len())];
                        match dice.below(3) {
                            0 => steps.push(edit(Edit::MoveNodes {
                                ids: vec![n.id],
                                delta: [dice.float(-50.0, 50.0), dice.float(-50.0, 50.0)],
                            })),
                            1 => steps.push(edit(Edit::AddNode {
                                kind: NodeKind::ALL[dice.below(NodeKind::ALL.len())],
                                pos: [dice.float(0.0, 1000.0), -900.0],
                            })),
                            _ => {
                                if let Some(pd) = n.def().params.first()
                                    && let Some(v) = value_for(pd, dice, false)
                                {
                                    steps.push(edit(Edit::SetParam { node: n.id, key: pd.key.into(), value: v }));
                                }
                            }
                        }
                        steps.push(Step::Do(Message::Seal));
                    }
                    steps.push(Step::Wait);
                    Some(steps)
                }
                2 => {
                    nodes_before = app.graph().nodes.len();
                    let mut steps: Vec<Step> = (0..150).map(|_| Step::Do(Message::Undo)).collect();
                    steps.extend([Step::Wait, Step::Note("undone".into())]);
                    Some(steps)
                }
                3 => {
                    let before = nodes_before;
                    let mut steps: Vec<Step> = (0..150).map(|_| Step::Do(Message::Redo)).collect();
                    steps.extend([
                        Step::Wait,
                        Step::Note("redone".into()),
                        Step::Check(Box::new(move |app: &App| {
                            let now = app.graph().nodes.len();
                            (now != before)
                                .then(|| format!("undone and redone, the graph has {now} nodes, not {before}"))
                        })),
                    ]);
                    steps.extend(back_home("undo and redo"));
                    Some(steps)
                }
                _ => None,
            }
        }),
    });

    let saved = folder.join("saved.brewtex");
    let files = folder.to_path_buf();
    let mut filed = 0;
    let mut counts = (0, 0);
    scenes.push_back(Scene {
        name: "saving and opening",
        next: Box::new(move |app: &App, _: &mut Dice| {
            filed += 1;
            let open = |name: &str| Step::Do(Message::Proceed(PendingAction::OpenPath(files.join(name))));
            match filed {
                1 => {
                    counts = (app.graph().nodes.len(), app.graph().links.len());
                    Some(vec![
                        Step::Do(Message::SavePicked(Some(saved.clone()))),
                        Step::Do(Message::Proceed(PendingAction::New)),
                        Step::Wait,
                        Step::Note("saved, then a new graph".into()),
                    ])
                }
                2 => {
                    let expected = counts;
                    Some(vec![
                        Step::Do(Message::Proceed(PendingAction::OpenPath(saved.clone()))),
                        Step::Wait,
                        Step::Note("opened again".into()),
                        Step::Check(Box::new(move |app: &App| {
                            let now = (app.graph().nodes.len(), app.graph().links.len());
                            (now != expected).then(|| {
                                format!("opened again, the graph has {now:?} nodes and links, not {expected:?}")
                            })
                        })),
                    ])
                }
                3 => {
                    let expected = counts;
                    Some(vec![
                        open("broken.brewtex"),
                        Step::Wait,
                        open("damaged.brewtex"),
                        Step::Wait,
                        open("empty.brewtex"),
                        Step::Wait,
                        open("other.brewtex"),
                        Step::Wait,
                        open("text.brewtex"),
                        Step::Wait,
                        open("big.png"),
                        Step::Wait,
                        open("does-not-exist.brewtex"),
                        Step::Wait,
                        open(""),
                        Step::Wait,
                        Step::Note("broken files refused".into()),
                        // Refused, they leave the graph that was open.
                        Step::Check(Box::new(move |app: &App| {
                            let now = (app.graph().nodes.len(), app.graph().links.len());
                            (now != expected)
                                .then(|| format!("broken files refused, the graph has {now:?}, not {expected:?}"))
                        })),
                    ])
                }
                // Saved again, changed, and the history of the package looked at.
                4 => Some(vec![
                    edit(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }),
                    Step::Do(Message::Save),
                    Step::Wait,
                    edit(Edit::AddNode { kind: NodeKind::LEVELS, pos: [40.0, 0.0] }),
                    Step::Do(Message::Versions(VersionsMessage::Open)),
                    Step::Check(Box::new(|app: &App| match &app.ui.dialog {
                        Some(Dialog::Versions(dialog)) if dialog.list.len() >= 2 => None,
                        other => Some(format!("the history of a package saved twice is {other:?}")),
                    })),
                ]),
                // The oldest version (the first save) restored in the package's place, not saved;
                // the change it replaced kept as a version.
                5 => {
                    let Some(Dialog::Versions(dialog)) = &app.ui.dialog else { return Some(Vec::new()) };
                    let (oldest, expected, file, versions) = (dialog.list.last().map(|v| v.file.clone()), counts, saved.clone(), dialog.list.len());
                    Some(vec![
                        Step::Do(Message::Versions(VersionsMessage::Restore(oldest.unwrap_or_default()))),
                        Step::Wait,
                        Step::Note("the first save restored".into()),
                        Step::Check(Box::new(move |app: &App| {
                            let now = (app.graph().nodes.len(), app.graph().links.len());
                            let kept = app.versions_kept().map_or(0, |(v, _)| v.of(&file).len());
                            if !app.doc.is_dirty() || app.doc.path() != Some(&file) || now != expected {
                                Some(format!("restored, the package is {now:?} (not {expected:?}), at {:?}, dirty {}", app.doc.path(), app.doc.is_dirty()))
                            } else {
                                (kept != versions + 1).then(|| format!("{kept} versions after a restore, not {}", versions + 1))
                            }
                        })),
                    ])
                }
                6 => Some(back_home("saving and opening")),
                _ => None,
            }
        }),
    });

    let export = folder.join("export");
    let mut exported = 0;
    scenes.push_back(Scene {
        name: "exporting",
        next: Box::new(move |app: &App, _: &mut Dice| {
            exported += 1;
            let outputs: Vec<NodeId> = app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            match exported {
                1..=3 => {
                    // Every format and bit depth somewhere among the outputs, over three rounds.
                    let mut steps = Vec::new();
                    for (i, id) in outputs.iter().enumerate() {
                        let format = ((i + exported * 2) % 7) as u32;
                        let depth = ((i + exported) % 3) as u32;
                        steps.push(edit(Edit::SetParam {
                            node: *id,
                            key: "format".into(),
                            value: ParamValue::Enum(format),
                        }));
                        steps.push(edit(Edit::SetParam {
                            node: *id,
                            key: "bit_depth".into(),
                            value: ParamValue::Enum(depth),
                        }));
                    }
                    let folder = export.join(exported.to_string());
                    let _ = std::fs::create_dir_all(&folder);
                    steps.extend([
                        Step::Do(Message::Seal),
                        Step::Wait,
                        Step::Do(Message::ExportFolderPicked(Some(folder.clone()))),
                        Step::Wait,
                    ]);
                    let wanted = outputs.len();
                    steps.push(Step::Check(Box::new(move |_: &App| {
                        let files = match files_in(&folder) {
                            Ok(files) => files,
                            Err(e) => return Some(e),
                        };
                        if files.len() != wanted {
                            return Some(format!(
                                "{} files exported to {}, not {wanted}",
                                files.len(),
                                folder.display()
                            ));
                        }
                        for f in &files {
                            match image::open(f) {
                                Ok(img) if img.width() == 2048 && img.height() == 2048 => {}
                                Ok(img) => {
                                    return Some(format!(
                                        "{} is {}x{}, not 2048x2048",
                                        f.display(),
                                        img.width(),
                                        img.height()
                                    ));
                                }
                                Err(e) => return Some(format!("{} cannot be read back: {e}", f.display())),
                            }
                        }
                        None
                    })));
                    steps.push(Step::Note("exported and read back".into()));
                    Some(steps)
                }
                // Into a folder that is not there (gone since it was picked).
                4 => Some(vec![
                    Step::Do(Message::ExportFolderPicked(Some(export.join("gone").join("deeper")))),
                    Step::Wait,
                    Step::Note("exported into a folder that is not there".into()),
                ]),
                // Through the export dialog: a preset of a game engine a round, the normal maps
                // one way and the other, 8 and 16 bits; the files it listed are those written.
                5..=8 => {
                    use crate::app::export_dialog::{ExportDialog, ExportMessage as X};
                    use brewed_texture_core::export_presets::Preset;
                    use brewed_texture_core::nodes::{DEPTH_8, DEPTH_16};
                    let folder = export.join(format!("preset-{exported}"));
                    let _ = std::fs::create_dir_all(&folder);
                    let dialog = ExportDialog {
                        preset: Preset::ALL[exported - 4],
                        directx: exported % 2 == 0,
                        depth: if exported % 2 == 0 { DEPTH_16 } else { DEPTH_8 },
                        folder: Some(folder.clone()),
                    };
                    let mut steps = vec![
                        Step::Do(Message::Export(X::Open)),
                        Step::Do(Message::Export(X::Preset(dialog.preset))),
                        Step::Do(Message::Export(X::DirectX(dialog.directx))),
                        Step::Do(Message::Export(X::Depth(dialog.depth))),
                        Step::Do(Message::Export(X::Folder(Some(folder.clone())))),
                    ];
                    let asked = dialog.clone();
                    steps.push(Step::Check(Box::new(move |app: &App| match &app.ui.dialog {
                        Some(Dialog::Export(open)) if *open == asked => None,
                        other => Some(format!("the export dialog is {other:?}, not {asked:?}")),
                    })));
                    steps.extend([Step::Do(Message::Export(X::Export)), Step::Wait]);
                    steps.push(Step::Check(Box::new(move |app: &App| {
                        if app.ui.dialog.is_some() {
                            return Some(format!("the export dialog stays open: {:?}", app.ui.dialog));
                        }
                        let planned: Vec<PathBuf> = app.export_plan(&dialog, &folder).into_iter().map(|(path, _)| path).collect();
                        let files = match files_in(&folder) {
                            Ok(files) => files,
                            Err(e) => return Some(e),
                        };
                        if files.len() != planned.len() {
                            return Some(format!("{} files exported with {:?}, {} planned", files.len(), dialog.preset, planned.len()));
                        }
                        for path in &planned {
                            if let Err(e) = image::open(path) {
                                return Some(format!("{} ({:?}) cannot be read back: {e}", path.display(), dialog.preset));
                            }
                        }
                        None
                    })));
                    steps.push(Step::Note(format!("exported for {:?}", Preset::ALL[exported - 4])));
                    Some(steps)
                }
                9 => Some(back_home("exporting")),
                _ => None,
            }
        }),
    });

    let pictures = [
        "one_pixel.png",
        "odd.png",
        "big.png",
        "gray16.png",
        "hdr.exr",
        "broken.png",
        "empty.png",
        "text.png",
        "missing.png",
    ];
    let pictures_folder = folder.to_path_buf();
    let mut picture = 0;
    scenes.push_back(Scene {
        name: "files for Bitmap nodes",
        next: Box::new(move |app: &App, _: &mut Dice| {
            let n = pictures.len();
            if picture < 2 * n {
                let name = pictures[picture / 2];
                picture += 1;
                if picture % 2 == 1 {
                    let path = pictures_folder.join(name).display().to_string();
                    return Some(vec![
                        edit(Edit::AddNodeWith {
                            kind: NodeKind::BITMAP,
                            pos: [0.0, 1600.0],
                            params: vec![("path".into(), ParamValue::Text(path))],
                        }),
                        Step::Do(Message::Seal),
                    ]);
                }
                // The Bitmap just added: shown, loaded again, and gone.
                let Some(bitmap) = app.graph().nodes_of(NodeKind::BITMAP).map(|n| n.id).max_by_key(|id| id.0) else {
                    return Some(vec![Step::Check(Box::new(move |_: &App| {
                        Some(format!("no Bitmap node for {name}"))
                    }))]);
                };
                let file = app.graph().node(bitmap).map(|n| n.text("path").to_string()).unwrap_or_default();
                return Some(vec![
                    preview(bitmap),
                    Step::Wait,
                    Step::Pause(Duration::from_millis(300)),
                    Step::Wait,
                    Step::Do(Message::ReloadFile(file)),
                    Step::Pause(Duration::from_millis(300)),
                    Step::Wait,
                    Step::Note(format!("a Bitmap of {name}")),
                    edit(Edit::RemoveNodes(vec![bitmap])),
                    Step::Do(Message::Seal),
                    Step::Wait,
                ]);
            }
            if picture == 2 * n {
                picture += 1;
                return Some(back_home("files for Bitmap nodes"));
            }
            None
        }),
    });

    let mut viewed = 0;
    let skies = folder.to_path_buf();
    scenes.push_back(Scene {
        name: "the 3D view",
        next: Box::new(move |app: &App, _: &mut Dice| {
            viewed += 1;
            let cam = app.ui.preview.view3d;
            let camera = |c: Camera| Step::Do(Message::Preview(PreviewMessage::Camera(c)));
            let turn = |m: Move| Step::Do(Message::Preview(PreviewMessage::Move3d(m)));
            let light = |m: PreviewMessage| Step::Do(Message::Preview(m));
            let shapes = Shape::ALL.len();
            match viewed {
                n if n == shapes + 2 => {
                    // Lit by environment images: a sky, a square picture, one that is gone and one
                    // that is broken; as bright and as blurred as can be, turned round and round,
                    // on every shape; then the studio again.
                    let file = |name: &str| skies.join(name).display().to_string();
                    let (sky, square) = (file("sky.hdr"), file("hdr.exr"));
                    let mut steps = vec![
                        light(PreviewMessage::LightingMenu(true)),
                        light(PreviewMessage::Environment(Some(sky.clone()))),
                        Step::Pause(Duration::from_millis(1500)),
                    ];
                    let read = sky.clone();
                    steps.push(Step::Check(Box::new(move |app: &App| {
                        let l = &app.ui.preview.lighting;
                        (l.shown.as_ref().map(|h| h.path.as_str()) != Some(read.as_str()))
                            .then(|| format!("the sky was not read in a second and a half: {:?}, reading {:?}", l.shown, l.reading))
                    })));
                    for (k, shape) in Shape::ALL.into_iter().enumerate() {
                        steps.push(camera(Camera { shape, ..cam }));
                        steps.push(light(PreviewMessage::Exposure([-12.0, -3.0, 0.0, 2.5, 12.0][k % 5])));
                        steps.push(light(PreviewMessage::Blur([0.0, 0.25, 1.0][k % 3])));
                        for _ in 0..20 {
                            steps.push(turn(Move::Light(iced::Vector::new(40.0, 7.0))));
                            steps.push(turn(Move::Orbit(iced::Vector::new(-9.0, 3.0))));
                        }
                    }
                    for name in ["hdr.exr", "gone.hdr", "broken.png", "sky.hdr"] {
                        steps.push(light(PreviewMessage::Environment(Some(file(name)))));
                        steps.push(Step::Pause(Duration::from_millis(700)));
                    }
                    steps.extend([
                        light(PreviewMessage::Environment(Some(square))),
                        light(PreviewMessage::Background(false)),
                        light(PreviewMessage::Environment(None)),
                        light(PreviewMessage::Environment(Some(sky))),
                        light(PreviewMessage::Environment(None)),
                        light(PreviewMessage::LightingMenu(false)),
                        light(PreviewMessage::Exposure(0.0)),
                        light(PreviewMessage::Background(true)),
                        light(PreviewMessage::Blur(0.25)),
                        camera(Camera::default()),
                        Step::Check(Box::new(|app: &App| {
                            let l = &app.ui.preview.lighting;
                            let gone = l.recent.iter().find(|f| f.ends_with("gone.hdr") || f.ends_with("broken.png"));
                            gone.map(|f| format!("{f} cannot be read, and is still offered"))
                        })),
                        Step::Note("the 3D view lit by environment images, good and broken".into()),
                    ]);
                    Some(steps)
                }
                n if n <= shapes => {
                    let shape = Shape::ALL[n - 1];
                    let mut steps = vec![camera(Camera { shape, ..cam })];
                    for k in 0..60 {
                        steps.push(turn(Move::Orbit(iced::Vector::new(k as f32, 2.0))));
                        if k % 10 == 0 {
                            steps.push(turn(Move::Light(iced::Vector::new(-15.0, 3.0))));
                            steps.push(turn(Move::Zoom(if k < 30 { 1.3 } else { 0.7 })));
                        }
                    }
                    steps.push(Step::Note(format!("the 3D view on a {shape:?}")));
                    Some(steps)
                }
                n if n == shapes + 1 => Some(vec![
                    camera(Camera { displacement: 1.0, tiling: 32.0, ..cam }),
                    Step::Pause(Duration::from_millis(500)),
                    camera(Camera { displacement: 0.0, tiling: 0.25, ..cam }),
                    Step::Pause(Duration::from_millis(500)),
                    turn(Move::Zoom(1e6)),
                    turn(Move::Zoom(1e-6)),
                    turn(Move::Pan(iced::Vector::new(1e6, -1e6))),
                    Step::Pause(Duration::from_millis(300)),
                    Step::Do(Message::ToggleRealtime),
                    Step::Pause(Duration::from_millis(1000)),
                    Step::Do(Message::ToggleRealtime),
                    camera(Camera::default()),
                    Step::Note("the 3D view, far heights and repeats".into()),
                ]),
                _ => None,
            }
        }),
    });

    let mut picked = 0;
    scenes.push_back(Scene {
        name: "the pictures of the options",
        next: Box::new(move |app: &App, _: &mut Dice| {
            let noise = app.graph().nodes_of(NodeKind::NOISE).next()?.id;
            let options = NodeKind::NOISE.def().param("type")?.enum_options().len();
            picked += 1;
            match picked {
                1 => Some(vec![preview(noise), Step::Do(Message::ChoiceMenu(Some((noise, "type")))), Step::Wait]),
                n if n <= options + 1 => {
                    Some(vec![Step::Do(Message::PickChoice(noise, "type", (n - 2) as u32)), Step::Wait])
                }
                n if n == options + 2 => {
                    let mut steps = vec![Step::Do(Message::ChoiceMenu(None)), Step::Note("every type of noise".into())];
                    steps.extend(back_home("the pictures of the options"));
                    Some(steps)
                }
                _ => None,
            }
        }),
    });

    scenes.extend(more_scenes(folder));
    scenes.push_back(Scene { name: "the settings", next: once(settings_scene()) });
    scenes.push_back(Scene { name: "random", next: Box::new(random_steps) });
    scenes.push_back(Scene { name: "the end", next: once(back_home("the random part")) });
    scenes
}

/// Scenes of what happens at once, of large graphs and of odd names.
fn more_scenes(folder: &Path) -> Vec<Scene> {
    let mut scenes = Vec::new();
    let seal = || Step::Do(Message::Seal);

    // Documents changed, sizes changed and exports asked for while the engine is at work on
    // the last change: nothing waits for anything.
    let race = folder.join("export").join("race");
    let _ = std::fs::create_dir_all(&race);
    let saved = folder.join("saved.brewtex");
    let mut raced = false;
    scenes.push(Scene {
        name: "changes that do not wait",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            if std::mem::replace(&mut raced, true) {
                return None;
            }
            let nodes = image_nodes(app);
            let shown = nodes.get(dice.below(nodes.len())).map(|n| n.id);
            let mut steps = vec![
                edit(Edit::SetResolution([8192, 8192])),
                seal(),
                Step::Do(Message::Proceed(PendingAction::LoadDemo)),
                edit(Edit::SetResolution([4096, 4096])),
                seal(),
            ];
            steps.extend(shown.map(preview));
            steps.extend([
                Step::Do(Message::Proceed(PendingAction::New)),
                Step::Do(Message::Proceed(PendingAction::OpenPath(saved.clone()))),
                Step::Do(Message::Proceed(PendingAction::LoadDemo)),
                edit(Edit::SetResolution([8192, 8192])),
                seal(),
                Step::Do(Message::ExportFolderPicked(Some(race.clone()))),
                Step::Do(Message::Proceed(PendingAction::LoadDemo)),
                Step::Do(Message::Undo),
                Step::Do(Message::Redo),
                edit(Edit::SetResolution([1024, 1024])),
                seal(),
                Step::Do(Message::ExportFolderPicked(Some(race.clone()))),
                Step::Do(Message::Proceed(PendingAction::New)),
                Step::Wait,
                Step::Pause(Duration::from_millis(500)),
                Step::Wait,
                Step::Note("after changes that did not wait".into()),
            ]);
            // Whatever was exported is whole: of the document it was asked of, and the export
            // asked for last of each file (at 1024) is the file.
            let race = race.clone();
            steps.push(Step::Check(Box::new(move |_: &App| {
                let files: Vec<PathBuf> = std::fs::read_dir(&race).ok()?.flatten().map(|e| e.path()).collect();
                files.iter().find_map(|f| match image::image_dimensions(f) {
                    Err(e) => Some(format!("{} was exported broken: {e}", f.display())),
                    Ok(size) if size != (1024, 1024) => Some(format!("{} is {size:?}, not of the last export", f.display())),
                    Ok(_) => image::open(f).err().map(|e| format!("{} was exported broken: {e}", f.display())),
                })
            })));
            steps.extend(back_home("changes that did not wait"));
            Some(steps)
        }),
    });

    // A Bitmap's file changed again and again, and loaded again, while it loads: what stays
    // is the last file, and no other.
    let files = folder.to_path_buf();
    let mut stormed = 0;
    scenes.push(Scene {
        name: "a Bitmap changed while it loads",
        next: Box::new(move |app: &App, _: &mut Dice| {
            stormed += 1;
            let file = |name: &str| files.join(name).display().to_string();
            let path = |name: &str| ParamValue::Text(file(name));
            match stormed {
                1 => Some(vec![
                    edit(Edit::AddNodeWith {
                        kind: NodeKind::BITMAP,
                        pos: [0.0, 1600.0],
                        params: vec![("path".into(), path("big.png"))],
                    }),
                    seal(),
                ]),
                2 => {
                    let bitmap = app.graph().nodes_of(NodeKind::BITMAP).map(|n| n.id).max_by_key(|id| id.0)?;
                    let mut steps = vec![preview(bitmap)];
                    for name in [
                        "gray16.png",
                        "big.png",
                        "hdr.exr",
                        "broken.png",
                        "big.png",
                        "missing.png",
                        "odd.png",
                        "big.png",
                    ] {
                        steps.push(edit(Edit::SetParam { node: bitmap, key: "path".into(), value: path(name) }));
                        steps.push(seal());
                        steps.push(Step::Do(Message::ReloadFile(file(name))));
                    }
                    steps.extend([
                        Step::Do(Message::ReloadFile(file("big.png"))),
                        Step::Do(Message::ReloadFile(file("big.png"))),
                        Step::Wait,
                        Step::Pause(Duration::from_millis(2500)),
                        Step::Wait,
                        Step::Note("the last file of a Bitmap changed while it loaded".into()),
                        Step::Check(Box::new(move |app: &App| {
                            if let Some(e) = app.res.reports.get(&bitmap).and_then(|r| r.error.clone()) {
                                return Some(format!("a Bitmap changed while it loaded: {e}"));
                            }
                            // 4096² in RGBA of half floats, and no other file (the gradients the
                            // demo bakes take a few KB).
                            let files = app.res.memory.files;
                            (files.abs_diff(128 << 20) > 1 << 20)
                                .then(|| format!("a Bitmap of big.png keeps {:.1} MB of files, not 128", mb(files)))
                        })),
                        edit(Edit::RemoveNodes(vec![bitmap])),
                        seal(),
                        Step::Wait,
                        Step::Pause(Duration::from_millis(300)),
                        Step::Check(Box::new(|app: &App| {
                            let files = app.res.memory.files;
                            (files > 1 << 20).then(|| format!("no Bitmap, and {:.1} MB of files", mb(files)))
                        })),
                    ]);
                    Some(steps)
                }
                _ => None,
            }
        }),
    });

    // Nine demos in one graph: 207 nodes to draw, compute and change.
    let mut grown = 0;
    scenes.push(Scene {
        name: "a large graph",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            grown += 1;
            let g = app.graph();
            match grown {
                1 => {
                    let ids: Vec<NodeId> = g.nodes.iter().map(|n| n.id).collect();
                    let mut steps: Vec<Step> = (1..=8)
                        .flat_map(|k| {
                            [edit(Edit::Duplicate { ids: ids.clone(), offset: [0.0, 900.0 * k as f32] }), seal()]
                        })
                        .collect();
                    steps.extend([
                        Step::Do(Message::FrameAll),
                        Step::Wait,
                        Step::Note("nine demos".into()),
                        Step::Pause(Duration::from_secs(2)),
                        Step::Note("nine demos, drawn for two seconds".into()),
                    ]);
                    Some(steps)
                }
                2 => {
                    let var = g.variables.iter().find(|v| matches!(v.value, Value::Float(_)))?.clone();
                    let mut steps: Vec<Step> = (0..=60)
                        .map(|k| {
                            edit(Edit::SetVariable {
                                name: var.name.clone(),
                                value: swept(&var.value, &var.min, &var.max, k as f32 / 60.0),
                            })
                        })
                        .collect();
                    steps.extend([seal(), Step::Wait, Step::Note(format!("nine demos, {} dragged", var.name))]);
                    for _ in 0..40 {
                        let at = iced::Point::new(dice.float(0.0, 900.0), dice.float(0.0, 700.0));
                        steps
                            .push(Step::Do(Message::Graph(GraphMessage::ZoomAt { at, factor: dice.float(0.8, 1.25) })));
                        steps.push(Step::Do(Message::Graph(GraphMessage::PanBy(iced::Vector::new(
                            dice.float(-60.0, 60.0),
                            dice.float(-60.0, 60.0),
                        )))));
                    }
                    Some(steps)
                }
                3 => {
                    let count = g.nodes.len();
                    Some(vec![
                        Step::Do(Message::SelectAll),
                        Step::Do(Message::Delete),
                        Step::Wait,
                        Step::Note("everything deleted".into()),
                        Step::Do(Message::Undo),
                        Step::Wait,
                        Step::Note("everything back".into()),
                        Step::Check(Box::new(move |app: &App| {
                            let now = app.graph().nodes.len();
                            (now != count).then(|| format!("deleted and undone, {now} nodes, not {count}"))
                        })),
                    ])
                }
                4 => Some(back_home("a large graph")),
                _ => None,
            }
        }),
    });

    // Input values taken to their edges: ranges the wrong way round, values far out of them,
    // names taken or empty.
    let mut edged = false;
    scenes.push(Scene {
        name: "input values at their edges",
        next: Box::new(move |app: &App, _: &mut Dice| {
            if std::mem::replace(&mut edged, true) {
                return None;
            }
            let g = app.graph();
            let mut steps = Vec::new();
            let names: Vec<String> = g.variables.iter().map(|v| v.name.clone()).collect();
            let long = "x".repeat(500);
            for v in &g.variables {
                let name = v.name.clone();
                let far = |x: f32| match v.value {
                    Value::Int(_) => Value::Int(x as i32),
                    Value::Vec2(_) => Value::Vec2([x, -x]),
                    Value::Color(_) => Value::Color([x, -x, x, 1.0]),
                    Value::Bool(_) => Value::Bool(x > 0.0),
                    Value::Float(_) => Value::Float(x),
                };
                if v.has_range() {
                    steps.push(edit(Edit::SetVariableRange { name: name.clone(), min: far(10.0), max: far(-10.0) }));
                    steps.push(edit(Edit::SetVariableRange { name: name.clone(), min: far(0.0), max: far(0.0) }));
                }
                for x in [1e30, -1e30, f32::NAN, f32::INFINITY, 0.0] {
                    steps.push(edit(Edit::SetVariable { name: name.clone(), value: far(x) }));
                    steps.push(edit(Edit::SetVariableDefault { name: name.clone(), value: far(x) }));
                }
                steps.push(edit(Edit::ResetVariable(name.clone())));
                steps.push(edit(Edit::SetVariableDescription { name: name.clone(), text: "ы".repeat(5000) }));
                for to in ["", "   ", "1abc", "with space", "имя", names[0].as_str(), long.as_str()] {
                    steps.push(edit(Edit::RenameVariable { from: name.clone(), to: to.to_string() }));
                }
                steps.push(seal());
            }
            if let Some(input) = g.nodes.iter().find(|n| n.variable.is_some()) {
                steps.push(edit(Edit::BindVariable { node: input.id, name: "a_new_one".into() }));
                steps.push(edit(Edit::BindVariable { node: input.id, name: String::new() }));
            }
            steps.push(edit(Edit::AddVariableNode { name: "not_there".into(), pos: [0.0, 0.0] }));
            steps.extend([seal(), Step::Wait, Step::Note("input values at their edges".into())]);
            steps.push(Step::Check(Box::new(|app: &App| {
                let g = app.graph();
                g.variables.iter().find_map(|v| {
                    let values = [v.value, v.default, v.min, v.max];
                    values
                        .iter()
                        .any(|x| !x.is_finite())
                        .then(|| format!("the input value {} holds {values:?}", v.name))
                })
            })));
            steps.extend(back_home("input values at their edges"));
            Some(steps)
        }),
    });

    // Outputs called what file systems keep for themselves, or what they cannot hold.
    let named = folder.join("export").join("names");
    let mut rounds = 0;
    scenes.push(Scene {
        name: "odd names of outputs",
        next: Box::new(move |app: &App, _: &mut Dice| {
            rounds += 1;
            let names: Vec<String> = match rounds {
                1 => vec!["aux".into(), "a/b\\c:d*e?f\"g<h>i|".into(), "..".into(), "🎨 цвет".into(), "я".repeat(300)],
                2 => vec!["con.map".into(), String::new(), "LPT1".into(), "basecolor".into(), "BASECOLOR".into()],
                3 => vec!["nul.height".into(), "b".repeat(1000), " . ".into(), "COM9".into(), "com9".into()],
                4 => return Some(back_home("odd names of outputs")),
                _ => return None,
            };
            let outputs: Vec<NodeId> = app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            let folder = named.join(rounds.to_string());
            let _ = std::fs::create_dir_all(&folder);
            let mut steps = Vec::new();
            for (id, name) in outputs.iter().zip(names.iter().chain(std::iter::repeat(&names[0]))) {
                steps.push(edit(Edit::SetParam {
                    node: *id,
                    key: "name".into(),
                    value: ParamValue::Text(name.clone()),
                }));
            }
            let wanted = outputs.len();
            steps.extend([
                seal(),
                Step::Wait,
                Step::Do(Message::ExportFolderPicked(Some(folder.clone()))),
                Step::Wait,
                Step::Note(format!("outputs called {names:?}")),
                Step::Check(Box::new(move |_: &App| {
                    let files = match files_in(&folder) {
                        Ok(files) => files,
                        Err(e) => return Some(e),
                    };
                    if files.len() != wanted {
                        let names: Vec<_> = files.iter().filter_map(|f| f.file_name()).collect();
                        return Some(format!("{} files of {wanted} exported: {names:?}", files.len()));
                    }
                    files
                        .iter()
                        .find_map(|f| image::open(f).err().map(|e| format!("{} cannot be read back: {e}", f.display())))
                })),
            ]);
            Some(steps)
        }),
    });

    // The window made as small as it goes, larger than the screen, filling it and back, and
    // sixty sizes in a row: its frames, and the views and what they draw, are made again.
    let mut resized = false;
    scenes.push(Scene {
        name: "the window resized",
        next: Box::new(move |_: &App, _: &mut Dice| {
            if std::mem::replace(&mut resized, true) {
                return None;
            }
            let resize = |w: f32, h: f32| Step::Window(WindowChange::Resize(iced::Size::new(w, h)));
            let mut steps = Vec::new();
            // (20000 x 12000: larger than the GPU can draw, as a window moved onto a display of
            // a larger scale grows: it is held to what can be drawn, and nothing goes wrong.)
            for (w, h) in [(1000.0, 640.0), (1920.0, 1080.0), (400.0, 300.0), (3840.0, 2160.0), (20000.0, 12000.0), (1680.0, 1000.0)] {
                steps.extend([
                    resize(w, h),
                    Step::Pause(Duration::from_millis(400)),
                    Step::Note(format!("the window asked to be {w}x{h}")),
                ]);
            }
            for k in 0..60 {
                steps.push(resize(1000.0 + 23.0 * k as f32, 640.0 + 11.0 * (k % 7) as f32));
            }
            steps.extend([
                Step::Window(WindowChange::Maximize(true)),
                Step::Pause(Duration::from_millis(700)),
                Step::Note("the window filling the screen".into()),
                Step::Window(WindowChange::Maximize(false)),
                Step::Pause(Duration::from_millis(300)),
                resize(1680.0, 1000.0),
                Step::Pause(Duration::from_millis(700)),
                Step::Note("the window at its size again".into()),
                Step::Check(Box::new(|app: &App| {
                    let size = app.ui.window_size;
                    let off = (size.width - 1680.0).abs() > 1.0 || (size.height - 1000.0).abs() > 1.0;
                    off.then(|| format!("the window is {size:?} after it was asked to be 1680x1000"))
                })),
            ]);
            Some(steps)
        }),
    });

    // The largest export there is: every output at 8192².
    let large = folder.join("export").join("8192");
    let mut sized = false;
    scenes.push(Scene {
        name: "exporting at 8192",
        next: Box::new(move |app: &App, _: &mut Dice| {
            if std::mem::replace(&mut sized, true) {
                return None;
            }
            let _ = std::fs::create_dir_all(&large);
            let outputs: Vec<NodeId> = app.graph().nodes_of(NodeKind::OUTPUT).map(|n| n.id).collect();
            let wanted = outputs.len();
            let folder = large.clone();
            // In floats as well as in bytes: the largest images read back are colors of floats
            // (1 GB at 8192²), more than a buffer of the GPU may be.
            use brewed_texture_core::nodes::{DEPTH_8, DEPTH_16, DEPTH_16F, DEPTH_32F};
            let depths = [DEPTH_32F, DEPTH_16F, DEPTH_16, DEPTH_8];
            let formats = outputs.iter().zip(depths.iter().cycle()).map(|(id, depth)| {
                edit(Edit::SetParam { node: *id, key: "bit_depth".into(), value: ParamValue::Enum(*depth) })
            });
            let mut steps: Vec<Step> = formats.collect();
            steps.extend([
                edit(Edit::SetResolution([8192, 8192])),
                seal(),
                Step::Wait,
                Step::Note("8192, before exporting".into()),
                Step::Do(Message::ExportFolderPicked(Some(large.clone()))),
                Step::Wait,
                Step::Note("exported at 8192".into()),
                Step::Check(Box::new(move |_: &App| {
                    let files = match files_in(&folder) {
                        Ok(files) => files,
                        Err(e) => return Some(e),
                    };
                    if files.len() != wanted {
                        return Some(format!("{} files exported at 8192, not {wanted}", files.len()));
                    }
                    files.iter().find_map(|f| match image::image_dimensions(f) {
                        Ok((8192, 8192)) => None,
                        Ok((w, h)) => Some(format!("{} is {w}x{h}, not 8192x8192", f.display())),
                        Err(e) => Some(format!("{} cannot be read back: {e}", f.display())),
                    })
                })),
                // Five maps of 8192²: 1.7 GB, gone once they are read back.
                Step::Tidy(large.clone()),
            ]);
            steps.extend(back_home("exporting at 8192"));
            Some(steps)
        }),
    });

    // Packages: nodes made a graph of their own, its values dragged, the graph edited on its
    // own; the demo's graph used as a node in another package; both saved, closed and opened
    // again, Open Reference; the demo's file moved, asked for, and found.
    let packages = folder.join("packages");
    let _ = std::fs::create_dir_all(packages.join("moved"));
    let mut stage = 0;
    /// A stage past the last: the scene is over.
    const OVER: u32 = 1000;
    scenes.push(Scene {
        name: "packages and graphs used as nodes",
        next: Box::new(move |app: &App, dice: &mut Dice| {
            stage += 1;
            let wall = packages.join("wall.brewtex");
            let moved = packages.join("moved").join("wall.brewtex");
            let uses = packages.join("uses.brewtex");
            let close_all = |app: &App| -> Vec<Step> {
                app.packages().into_iter().map(|(key, _)| Step::Do(Message::Proceed(PendingAction::Close(key)))).collect()
            };
            let instance = |app: &App| app.graph().nodes.iter().find(|n| n.instance.is_some()).map(|n| n.id);
            // What was to be there is not: said, and the scene is over.
            let lost = |what: &'static str| Some(vec![Step::Check(Box::new(move |_: &App| Some(what.to_string())))]);
            match stage {
                1 => {
                    let _ = std::fs::remove_file(&moved);
                    // A chain of the demo: the Levels, what arrives at it and where it goes.
                    let g = app.graph();
                    let Some(levels) = g.nodes_of(NodeKind::LEVELS).next().map(|n| n.id) else {
                        stage = OVER;
                        return lost("the demo has no Levels");
                    };
                    let before = g.link_into(levels, "i:input").map(|l| l.from);
                    let after = g.links.iter().find(|l| l.from == levels).map(|l| l.to);
                    let ids: Vec<NodeId> = [before, Some(levels), after].into_iter().flatten().collect();
                    Some(vec![
                        Step::Do(Message::SavePicked(Some(wall))),
                        Step::Do(Message::Graph(GraphMessage::SelectBox { ids, additive: false })),
                        Step::Do(Message::GraphFromSelection),
                        Step::Wait,
                        Step::Note("nodes made a graph of their own".into()),
                        Step::Check(Box::new(|app: &App| {
                            let Some(used) = app.graph().nodes.iter().find(|n| n.instance.is_some()) else {
                                return Some(format!("no graph made of the selection: {:?}", app.ui.status.as_ref().map(|s| &s.text)));
                            };
                            let errors = app.eval.node(used.id).map(|e| e.errors.clone()).unwrap_or_default();
                            (app.doc.package().graphs.len() != 2 || !errors.is_empty() || !app.res.inner.is_empty())
                                .then(|| format!("made a graph of the selection: {} graphs, {errors:?}, {:?}", app.doc.package().graphs.len(), app.res.inner))
                        })),
                    ])
                }
                2 => {
                    // Its values dragged, as the properties drag them.
                    let Some(id) = instance(app) else {
                        stage = OVER;
                        return lost("the graph made of the selection is not used in its place");
                    };
                    let node = app.graph().node(id)?;
                    let mut steps = Vec::new();
                    for pd in node.param_defs() {
                        for _ in 0..10 {
                            if let Some(value) = value_for(&pd, dice, false) {
                                steps.push(edit(Edit::SetParam { node: id, key: pd.key.to_string(), value }));
                            }
                        }
                        steps.push(Step::Do(Message::Seal));
                    }
                    steps.extend([preview(id), Step::Wait, Step::Note("its values dragged".into())]);
                    Some(steps)
                }
                3 => {
                    // The graph edited on its own, and back.
                    let key = app.doc_key;
                    let sub = app.doc.package().graphs.last()?.id;
                    let main = app.doc.package().first();
                    Some(vec![
                        Step::Do(Message::Explorer(ExplorerMessage::OpenGraph(key, sub))),
                        Step::Wait,
                        Step::Note("the new graph on its own".into()),
                        Step::Do(Message::Explorer(ExplorerMessage::OpenGraph(key, main))),
                        Step::Wait,
                    ])
                }
                4 => {
                    // The demo's graph used as a node in a new package, shown.
                    let key = app.doc_key;
                    let graph = app.doc.package().first();
                    let name = app.doc.entry().name.clone();
                    Some(vec![
                        Step::Do(Message::SavePicked(Some(wall))),
                        Step::Do(Message::New),
                        Step::Do(Message::Graph(GraphMessage::Drop { payload: DragPayload::Graph { key, graph, name }, graph_pos: [0.0, 0.0] })),
                        Step::Wait,
                    ])
                }
                5 => {
                    let Some(id) = instance(app) else {
                        stage = OVER;
                        return lost("the demo dropped on another package is not a node of it");
                    };
                    Some(vec![
                        preview(id),
                        Step::Wait,
                        Step::Do(Message::SavePicked(Some(uses))),
                        Step::Note("the demo, a node of another package".into()),
                        Step::Check(Box::new(move |app: &App| {
                            let errors = app.eval.node(id).map(|e| e.errors.clone()).unwrap_or_default();
                            (!errors.is_empty() || app.res.preview.is_none()).then(|| format!("the demo as a node: {errors:?}, shown: {}", app.res.preview.is_some()))
                        })),
                    ])
                }
                6 => {
                    let mut steps = close_all(app);
                    steps.extend([Step::Do(Message::OpenPicked(Some(uses))), Step::Wait]);
                    Some(steps)
                }
                7 => {
                    let Some(id) = instance(app) else {
                        stage = OVER;
                        return lost("opened again, the package has no node that uses the demo");
                    };
                    Some(vec![
                        Step::Check(Box::new(|app: &App| app.ui.dialog.as_ref().map(|d| format!("opened again, it asked {d:?}")))),
                        Step::Do(Message::OpenReference(id)),
                        Step::Wait,
                        Step::Note("Open Reference".into()),
                        Step::Check(Box::new(|app: &App| {
                            (app.packages().len() != 2).then(|| format!("Open Reference: {} packages, not 2", app.packages().len()))
                        })),
                    ])
                }
                8 => {
                    // The demo's file moved: asked for.
                    let mut steps = close_all(app);
                    let _ = std::fs::rename(&wall, &moved);
                    steps.extend([
                        Step::Do(Message::OpenPicked(Some(uses))),
                        Step::Wait,
                        Step::Check(Box::new(|app: &App| {
                            (!matches!(app.ui.dialog, Some(Dialog::Missing(_)))).then(|| "a moved package was not asked for".into())
                        })),
                    ]);
                    Some(steps)
                }
                9 => {
                    let Some(Dialog::Missing(missing)) = &app.ui.dialog else { return Some(Vec::new()) };
                    let item = missing.items.first()?.path.clone();
                    Some(vec![
                        Step::Do(Message::Explorer(ExplorerMessage::Located(missing.key, item, Some(moved)))),
                        Step::Wait,
                        Step::Note("found where it was moved".into()),
                        Step::Check(Box::new(|app: &App| {
                            let Some(id) = app.graph().nodes.iter().find(|n| n.instance.is_some()).map(|n| n.id) else {
                                return Some("found again, the package has no node that uses the demo".into());
                            };
                            let errors = app.eval.node(id).map(|e| e.errors.clone()).unwrap_or_default();
                            (app.ui.dialog.is_some() || !errors.is_empty()).then(|| format!("found again: {:?}, {errors:?}", app.ui.dialog))
                        })),
                        Step::Do(Message::Undo),
                        Step::Do(Message::Redo),
                        Step::Wait,
                    ])
                }
                10 => {
                    let mut steps = close_all(app);
                    let _ = std::fs::rename(&moved, &wall);
                    steps.extend(back_home("packages"));
                    Some(steps)
                }
                _ => None,
            }
        }),
    });

    // Quitting with changes not saved asks first; not quitting, the app goes on.
    let mut asked = 0;
    scenes.push(Scene {
        name: "quit, and not",
        next: Box::new(move |app: &App, _: &mut Dice| {
            asked += 1;
            match asked {
                1 => Some(vec![edit(Edit::SetResolution([1024, 1024])), seal(), Step::Wait]),
                // (Nothing to save, quitting would quit.)
                2 if app.doc.is_dirty() => Some(vec![
                    Step::Do(Message::Quit),
                    Step::Pause(Duration::from_millis(300)),
                    Step::Check(Box::new(|app: &App| {
                        app.ui.dialog.is_none().then(|| "quitting with changes asked nothing".into())
                    })),
                    Step::Do(Message::Dialog(None)),
                    Step::Pause(Duration::from_millis(300)),
                    Step::Check(Box::new(|app: &App| app.ui.dialog.is_some().then(|| "the dialog stayed".into()))),
                ]),
                2 => Some(vec![Step::Check(Box::new(|_: &App| Some("a changed size left nothing to save".into())))]),
                _ => None,
            }
        }),
    });
    scenes
}

/// A few random things, as a person trying the app out does them.
fn random_steps(app: &App, dice: &mut Dice) -> Option<Vec<Step>> {
    let g = app.graph();
    if g.nodes.is_empty() {
        return Some(vec![Step::Do(Message::Proceed(PendingAction::LoadDemo)), Step::Wait]);
    }
    let node = &g.nodes[dice.below(g.nodes.len())];
    let other = &g.nodes[dice.below(g.nodes.len())];
    let mut steps = Vec::new();
    let seal = || Step::Do(Message::Seal);
    match dice.below(40) {
        0..=9 => {
            // A parameter dragged a little, or set far.
            let params: Vec<&ParamDef> = node.def().params.iter().filter(|p| !node.is_exposed(p.key)).collect();
            if let Some(pd) = params.get(dice.below(params.len())) {
                let far = dice.chance(10);
                for _ in 0..if far { 1 } else { 1 + dice.below(20) } {
                    if let Some(v) = value_for(pd, dice, far) {
                        steps.push(edit(Edit::SetParam { node: node.id, key: pd.key.into(), value: v }));
                    }
                }
                steps.push(seal());
            }
        }
        10..=11 => {
            if let Some(var) = g.variables.get(dice.below(g.variables.len())) {
                for _ in 0..=dice.below(30) {
                    let value = swept(&var.value, &var.min, &var.max, dice.float(0.0, 1.0));
                    steps.push(edit(Edit::SetVariable { name: var.name.clone(), value }));
                }
                steps.push(seal());
            }
        }
        12..=14 => {
            // An output to an input.
            let inputs = node.def().inputs;
            if let (Some(o), Some(i)) = (other.def().outputs.first(), inputs.get(dice.below(inputs.len()))) {
                steps.push(edit(Edit::Connect {
                    a: SocketRef { node: other.id, key: output_socket_key(o.key), output: true },
                    b: SocketRef { node: node.id, key: input_socket_key(i.key), output: false },
                }));
                steps.push(seal());
            }
        }
        15 => {
            if let Some(l) = g.links.get(dice.below(g.links.len())) {
                steps.extend([edit(Edit::RemoveLink(l.clone())), seal()]);
            }
        }
        16 => {
            let kind = NodeKind::ALL[dice.below(NodeKind::ALL.len())];
            let pos = [dice.float(-500.0, 1500.0), dice.float(-500.0, 1500.0)];
            steps.push(if dice.chance(50) {
                edit(Edit::AddNode { kind, pos })
            } else {
                Step::Do(Message::Graph(GraphMessage::Drop { payload: DragPayload::Node(kind), graph_pos: pos }))
            });
            steps.push(seal());
        }
        17 => {
            if g.nodes.len() > 3 {
                steps.extend([edit(Edit::RemoveNodes(vec![node.id])), seal()]);
            }
        }
        18..=20 => steps.push(Step::Do(if dice.chance(50) { Message::Undo } else { Message::Redo })),
        21..=23 => {
            if node.def().has_image() {
                steps.push(preview(node.id));
            }
        }
        24 => {
            let s = [256, 512, 1024, 2048, 2048, 4096][dice.below(6)];
            steps.extend([edit(Edit::SetResolution([s, s])), seal()]);
        }
        25 => steps.push(Step::Do(Message::ToggleBypass(node.id))),
        26 => steps.extend([edit(Edit::Duplicate { ids: vec![node.id], offset: [40.0, 40.0] }), seal()]),
        27 => {
            // Through the selection, as the keyboard does it.
            steps.push(Step::Do(Message::Graph(GraphMessage::Select { node: node.id, mode: SelectMode::Replace })));
            steps.push(Step::Do(Message::Graph(GraphMessage::Select { node: other.id, mode: SelectMode::Add })));
            steps.push(Step::Do(match dice.below(4) {
                0 => Message::Duplicate,
                1 if g.nodes.len() > 4 => Message::Delete,
                2 => Message::AddComment,
                _ => Message::Frame,
            }));
        }
        28 => {
            let exposable: Vec<&ParamDef> = node.def().params.iter().filter(|p| p.can_expose()).collect();
            if let Some(pd) = exposable.get(dice.below(exposable.len())) {
                let exposed = !node.is_exposed(pd.key);
                steps.extend([edit(Edit::SetExposed { node: node.id, key: pd.key.into(), exposed }), seal()]);
            }
        }
        29 => {
            let message = match dice.below(6) {
                0 => PreviewMessage::Channel(dice.below(5) as u32),
                1 => PreviewMessage::ToggleTiling,
                2 => PreviewMessage::Fit,
                3 => PreviewMessage::OneToOne,
                4 => PreviewMessage::ZoomAt {
                    at: iced::Point::new(dice.float(0.0, 600.0), dice.float(0.0, 600.0)),
                    factor: dice.float(0.2, 5.0),
                },
                _ => PreviewMessage::PanBy(iced::Vector::new(dice.float(-900.0, 900.0), dice.float(-900.0, 900.0))),
            };
            steps.push(Step::Do(Message::Preview(message)));
        }
        30 => {
            if node.kind == NodeKind::LEVELS {
                steps.push(Step::Do(Message::AutoLevels(node.id)));
            }
        }
        31 => {
            if g.nodes.len() < 8 || dice.chance(10) {
                steps.push(Step::Do(Message::Proceed(PendingAction::LoadDemo)));
            }
        }
        33 => {
            // Two nodes made a graph of their own; now and then, back into the graph they came
            // from (undo), or another graph of the package edited for a while.
            steps.push(Step::Do(Message::Graph(GraphMessage::SelectBox { ids: vec![node.id, other.id], additive: false })));
            steps.push(Step::Do(Message::GraphFromSelection));
            if dice.chance(50) {
                steps.push(Step::Do(Message::Undo));
            }
        }
        34 => {
            let graphs = &app.doc.package().graphs;
            if let Some(entry) = graphs.get(dice.below(graphs.len())) {
                steps.push(Step::Do(Message::Explorer(ExplorerMessage::OpenGraph(app.doc_key, entry.id))));
            }
        }
        35 => {
            // What lights the 3D view: never the file dialog, the images used before.
            let l = &app.ui.preview.lighting;
            let message = match dice.below(5) {
                0 => PreviewMessage::Exposure(dice.float(-6.0, 6.0)),
                1 => PreviewMessage::Blur(dice.float(0.0, 1.0)),
                2 => PreviewMessage::Background(dice.chance(50)),
                3 => PreviewMessage::Environment(l.recent.get(dice.below(l.recent.len() + 1)).cloned()),
                _ => PreviewMessage::LightingMenu(dice.chance(50)),
            };
            steps.push(Step::Do(Message::Preview(message)));
        }
        32 => {
            let inputs = node.def().inputs;
            if let Some(i) = inputs.get(dice.below(inputs.len())) {
                steps.extend([edit(Edit::Disconnect { node: node.id, socket: input_socket_key(i.key) }), seal()]);
            }
        }
        _ => steps.push(Step::Wait),
    }
    if dice.chance(15) {
        steps.push(Step::Wait);
    }
    if dice.chance(3) {
        steps.push(Step::Note("random".into()));
    }
    Some(steps)
}
