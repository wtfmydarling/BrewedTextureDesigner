//! The program's settings, which Edit → Settings… changes: kept in `settings.toml`, in the
//! settings folder (see [`crate::workspace::folder`]). People may edit the file too: the program
//! writes only the values that change, and keeps the rest of it as it is (comments, the order,
//! what this version does not know). What the file says that cannot be taken is said, and set as
//! it is by default; a file that cannot be read at all is not written over.

use std::path::{Path, PathBuf};

use brewed_texture_core::Precision;
use toml_edit::{DocumentMut, Item, Value};

use crate::shortcuts::{Command, Shortcuts};

/// What `settings.toml` says when the program writes it first: every setting, as it is by
/// default, and what it is.
const TEMPLATE: &str = r#"# The settings of Brewed Texture Designer. Edit → Settings… changes them, and so may you:
# the program keeps what you write here, and says what it cannot take.

[general]
# How many steps Undo can go back in each package (10 to 1000).
undo_steps = 200
# How many packages File → Open Recent offers (0 to 20).
recent_packages = 8
# Where the file dialogs open: "package" (the folder of the package being edited, else where
# a dialog was answered last) or "last" (where a dialog was answered last).
file_dialogs = "package"
# The mouse wheel zooms the other way, in the graph and in the 2D and 3D views.
invert_zoom = false

[packages]
# What becomes of a file chosen for a node or dropped on the window: "embed" (the package holds
# it: the package is one file to copy or send) or "link" (the package names it where it is).
new_files = "embed"
# The size of the images of a new graph, in pixels (16 to 8192), and what they are kept in:
# "unorm8" (8 bits), "float16" (16 bits float) or "float32" (32 bits float).
graph_size = 2048
graph_precision = "float16"

[recovery]
# Keep unsaved work, to offer it back after a crash, and how often, in minutes (1 to 60).
keep = true
every = 2

[performance]
# The video memory the images of nodes that no view shows may keep, in megabytes (0 to
# 65536): a change then computes only what follows it. 0 keeps only what is shown.
image_cache = 1024
# The graphics API: "auto", or "vulkan", "dx12" (Windows), "metal" (macOS); and whether the
# edges of lines and shapes are smoothed. Taken when the program starts; the environment
# variable WGPU_BACKEND, where there is one, comes first.
graphics_api = "auto"
antialiasing = true

# The keys of the commands, where they are not those by default: redo = ["Ctrl+Y"], say, or
# frame = [] for none. A command has one or two; Ctrl is ⌘ on macOS.
[shortcuts]
"#;

/// The largest settings file read: what is larger is not settings.
const MAX_FILE: u64 = 1 << 20;

/// The tables of the file and their keys (`[shortcuts]` takes the ids of the commands).
const TABLES: &[(&str, &[&str])] = &[
    ("general", &["undo_steps", "recent_packages", "file_dialogs", "invert_zoom"]),
    ("packages", &["new_files", "graph_size", "graph_precision"]),
    ("recovery", &["keep", "every"]),
    ("performance", &["image_cache", "graphics_api", "antialiasing"]),
    ("shortcuts", &[]),
];

/// The settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// How many steps Undo can go back in each package.
    pub undo_steps: u32,
    /// How many packages File → Open Recent offers.
    pub recent_packages: u32,
    /// Where the file dialogs open.
    pub file_dialogs: FileDialogs,
    /// The mouse wheel zooms the other way.
    pub invert_zoom: bool,
    /// What becomes of a file chosen for a node or dropped on the window.
    pub new_files: NewFiles,
    /// A new graph: the size of its images, and what they are kept in.
    pub graph_size: u32,
    pub graph_precision: Precision,
    /// Unsaved work is kept for recovery after a crash (see `app::autosave`), every so many
    /// minutes.
    pub keep_unsaved: bool,
    pub keep_every: u32,
    /// The video memory the images of nodes no view shows may keep, in megabytes.
    pub image_cache: u32,
    /// The graphics API, and whether edges are smoothed (taken when the program starts).
    pub graphics: Graphics,
    pub antialiasing: bool,
    /// The keys of the commands.
    pub shortcuts: Shortcuts,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            undo_steps: 200,
            recent_packages: 8,
            file_dialogs: FileDialogs::Package,
            invert_zoom: false,
            new_files: NewFiles::Embed,
            graph_size: 2048,
            graph_precision: Precision::Half,
            keep_unsaved: true,
            keep_every: 2,
            image_cache: 1024,
            graphics: Graphics::Auto,
            antialiasing: true,
            shortcuts: Shortcuts::default(),
        }
    }
}

/// A page of the settings: the settings of a table of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Packages,
    Recovery,
    Performance,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::General, Page::Packages, Page::Recovery, Page::Performance, Page::Shortcuts];

    pub fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Packages => "Packages",
            Page::Recovery => "Recovery",
            Page::Performance => "Performance",
            Page::Shortcuts => "Shortcuts",
        }
    }

    /// What the page is about, under its name.
    pub fn about(self) -> &'static str {
        match self {
            Page::General => "Undo, the packages offered again, the file dialogs, the mouse wheel.",
            Page::Packages => "The files taken into packages, and the graphs made in them.",
            Page::Recovery => "Unsaved work kept, to be offered back after a crash.",
            Page::Performance => "Video memory, and how the program draws.",
            Page::Shortcuts => "The keys of the commands. Click a key to give another; Esc always closes what is open.",
        }
    }
}

/// A setting, with a value (the settings dialog changes one at a time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    UndoSteps(u32),
    RecentPackages(u32),
    FileDialogs(FileDialogs),
    InvertZoom(bool),
    NewFiles(NewFiles),
    GraphSize(u32),
    GraphPrecision(Precision),
    KeepUnsaved(bool),
    KeepEvery(u32),
    ImageCache(u32),
    Graphics(Graphics),
    Antialiasing(bool),
}

impl Settings {
    /// Set one setting (held to its range).
    pub fn set(&mut self, setting: Setting) {
        match setting {
            Setting::UndoSteps(n) => self.undo_steps = n.clamp(10, 1000),
            Setting::RecentPackages(n) => self.recent_packages = n.min(20),
            Setting::FileDialogs(f) => self.file_dialogs = f,
            Setting::InvertZoom(on) => self.invert_zoom = on,
            Setting::NewFiles(f) => self.new_files = f,
            Setting::GraphSize(n) => self.graph_size = n.clamp(16, 8192),
            Setting::GraphPrecision(p) => self.graph_precision = p,
            Setting::KeepUnsaved(on) => self.keep_unsaved = on,
            Setting::KeepEvery(n) => self.keep_every = n.clamp(1, 60),
            Setting::ImageCache(n) => self.image_cache = n.min(65536),
            Setting::Graphics(g) => self.graphics = g,
            Setting::Antialiasing(on) => self.antialiasing = on,
        }
    }

    /// `self`, with the settings of `page` as `from` has them.
    fn take(&mut self, from: &Settings, page: Page) {
        match page {
            Page::General => {
                self.undo_steps = from.undo_steps;
                self.recent_packages = from.recent_packages;
                self.file_dialogs = from.file_dialogs;
                self.invert_zoom = from.invert_zoom;
            }
            Page::Packages => {
                self.new_files = from.new_files;
                self.graph_size = from.graph_size;
                self.graph_precision = from.graph_precision;
            }
            Page::Recovery => {
                self.keep_unsaved = from.keep_unsaved;
                self.keep_every = from.keep_every;
            }
            Page::Performance => {
                self.image_cache = from.image_cache;
                self.graphics = from.graphics;
                self.antialiasing = from.antialiasing;
            }
            Page::Shortcuts => self.shortcuts = from.shortcuts.clone(),
        }
    }

    /// `self`, with the settings of `page` as they are by default.
    pub fn reset(&self, page: Page) -> Settings {
        let mut reset = self.clone();
        reset.take(&Settings::default(), page);
        reset
    }

    /// Whether `self` and `other` differ on `page`.
    pub fn differ(&self, other: &Settings, page: Page) -> bool {
        let mut taken = self.clone();
        taken.take(other, page);
        taken != *self
    }

    /// Whether the program has to start again for `self` to be taken, running as `running`
    /// says (the settings it started with).
    pub fn needs_restart(&self, running: &Settings) -> bool {
        self.graphics != running.graphics || self.antialiasing != running.antialiasing
    }

    /// The image cache in bytes.
    pub fn image_cache_bytes(&self) -> u64 {
        u64::from(self.image_cache) << 20
    }
}

/// Where the file dialogs open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogs {
    /// In the folder of the package being edited (else where a dialog was answered last).
    Package,
    /// Where a dialog was answered last.
    Last,
}

/// What becomes of a file chosen for a node or dropped on the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewFiles {
    /// The package holds it: the package is one file to copy or send.
    Embed,
    /// The package names it where it is on disk.
    Link,
}

/// A graphics API, as `WGPU_BACKEND` names it. (Not OpenGL: wgpu shares one OpenGL context
/// between threads, and takes a thread that waits for it more than a second, as the window
/// does while the engine compiles a large shader, for a deadlock, and stops the program.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graphics {
    /// The one wgpu finds best.
    Auto,
    Vulkan,
    Dx12,
    Metal,
}

impl Graphics {
    const ALL: [Graphics; 4] = [Graphics::Auto, Graphics::Vulkan, Graphics::Dx12, Graphics::Metal];

    /// Those this system has.
    pub fn here() -> &'static [Graphics] {
        if cfg!(windows) {
            &[Graphics::Auto, Graphics::Vulkan, Graphics::Dx12]
        } else if cfg!(target_os = "macos") {
            &[Graphics::Auto, Graphics::Metal]
        } else {
            &[Graphics::Auto, Graphics::Vulkan]
        }
    }

    /// What the settings file and `WGPU_BACKEND` call it.
    pub fn id(self) -> &'static str {
        match self {
            Graphics::Auto => "auto",
            Graphics::Vulkan => "vulkan",
            Graphics::Dx12 => "dx12",
            Graphics::Metal => "metal",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Graphics::Auto => "Automatic",
            Graphics::Vulkan => "Vulkan",
            Graphics::Dx12 => "DirectX 12",
            Graphics::Metal => "Metal",
        }
    }
}

impl std::fmt::Display for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

const PRECISIONS: [(&str, Precision); 3] = [("unorm8", Precision::Bits8), ("float16", Precision::Half), ("float32", Precision::Full)];

/// The settings a document says, and what it says that cannot be taken (each set as it is by
/// default, or held to its range) or that this version does not know.
fn read(doc: &DocumentMut) -> (Settings, Vec<String>) {
    let d = Settings::default();
    let mut problems = Vec::new();
    for (name, item) in doc.iter() {
        match TABLES.iter().find(|(t, _)| *t == name) {
            None => problems.push(format!("{name} is no setting of this version")),
            Some((table, keys)) if !keys.is_empty() => {
                for (key, _) in item.as_table_like().into_iter().flat_map(|t| t.iter()).filter(|(k, _)| !keys.contains(k)) {
                    problems.push(format!("{table}.{key} is no setting of this version"));
                }
            }
            Some(_) => {}
        }
    }
    let mut get = |table: &str, key: &str| -> Option<Value> {
        let item = doc.get(table)?;
        let Some(table_like) = item.as_table_like() else {
            problems.push(format!("[{table}] is not a table"));
            return None;
        };
        let value = table_like.get(key)?;
        match value.as_value() {
            Some(v) => Some(v.clone()),
            None => {
                problems.push(format!("{table}.{key} is not a value"));
                None
            }
        }
    };
    let undo_steps = get("general", "undo_steps");
    let recent_packages = get("general", "recent_packages");
    let file_dialogs = get("general", "file_dialogs");
    let invert_zoom = get("general", "invert_zoom");
    let new_files = get("packages", "new_files");
    let size = get("packages", "graph_size");
    let precision = get("packages", "graph_precision");
    let keep = get("recovery", "keep");
    let every = get("recovery", "every");
    let image_cache = get("performance", "image_cache");
    let api = get("performance", "graphics_api");
    let antialiasing = get("performance", "antialiasing");
    let shortcuts = match doc.get("shortcuts") {
        None => None,
        Some(item) => match item.as_table_like() {
            Some(table) => Some(table),
            None => {
                problems.push("[shortcuts] is not a table".into());
                None
            }
        },
    };
    let (shortcuts, said) = Shortcuts::read(shortcuts);
    problems.extend(said);
    let mut said = |what: String| problems.push(what);
    let graphics: Vec<(&str, Graphics)> = Graphics::ALL.iter().map(|g| (g.id(), *g)).collect();
    let settings = Settings {
        undo_steps: number(undo_steps, "general.undo_steps", d.undo_steps, 10, 1000, &mut said),
        recent_packages: number(recent_packages, "general.recent_packages", d.recent_packages, 0, 20, &mut said),
        file_dialogs: named(
            file_dialogs,
            "general.file_dialogs",
            &[("package", FileDialogs::Package), ("last", FileDialogs::Last)],
            d.file_dialogs,
            &mut said,
        ),
        invert_zoom: boolean(invert_zoom, "general.invert_zoom", d.invert_zoom, &mut said),
        new_files: named(new_files, "packages.new_files", &[("embed", NewFiles::Embed), ("link", NewFiles::Link)], d.new_files, &mut said),
        graph_size: number(size, "packages.graph_size", d.graph_size, 16, 8192, &mut said),
        graph_precision: named(precision, "packages.graph_precision", &PRECISIONS, d.graph_precision, &mut said),
        keep_unsaved: boolean(keep, "recovery.keep", d.keep_unsaved, &mut said),
        keep_every: number(every, "recovery.every", d.keep_every, 1, 60, &mut said),
        image_cache: number(image_cache, "performance.image_cache", d.image_cache, 0, 65536, &mut said),
        graphics: named(api, "performance.graphics_api", &graphics, d.graphics, &mut said),
        antialiasing: boolean(antialiasing, "performance.antialiasing", d.antialiasing, &mut said),
        shortcuts,
    };
    (settings, problems)
}

/// A whole number the file says, held to `min..=max`; `default` for none, or for what is no
/// whole number.
fn number(value: Option<Value>, name: &str, default: u32, min: u32, max: u32, said: &mut impl FnMut(String)) -> u32 {
    let Some(value) = value else { return default };
    let n = match &value {
        Value::Integer(i) => *i.value(),
        Value::Float(f) if f.value().fract() == 0.0 && f.value().abs() < 1e15 => *f.value() as i64,
        other => {
            said(format!("{name} is {}, not a whole number", other.to_string().trim()));
            return default;
        }
    };
    let held = n.clamp(i64::from(min), i64::from(max));
    if held != n {
        said(format!("{name} is {n}: taken as {held}, from {min} to {max}"));
    }
    held as u32
}

/// True or false, as the file says; `default` for none, or for another value.
fn boolean(value: Option<Value>, name: &str, default: bool, said: &mut impl FnMut(String)) -> bool {
    let Some(value) = value else { return default };
    value.as_bool().unwrap_or_else(|| {
        said(format!("{name} is {}, not true or false", value.to_string().trim()));
        default
    })
}

/// One of `names` the file says; `default` for none, or for another.
fn named<T: Copy>(value: Option<Value>, name: &str, names: &[(&str, T)], default: T, said: &mut impl FnMut(String)) -> T {
    let Some(value) = value else { return default };
    match names.iter().find(|(n, _)| Some(*n) == value.as_str()) {
        Some((_, v)) => *v,
        None => {
            let known: Vec<String> = names.iter().map(|(n, _)| format!("\"{n}\"")).collect();
            said(format!("{name} is {}, not one of {}", value.to_string().trim(), known.join(", ")));
            default
        }
    }
}

/// Set `table.key` to `value` in `doc`: what was said around the value (a comment after it)
/// stays.
fn set(doc: &mut DocumentMut, table: &str, key: &str, value: Value) {
    let item = doc.entry(table).or_insert_with(toml_edit::table);
    if item.as_table_like().is_none() {
        *item = toml_edit::table();
    }
    let table = item.as_table_like_mut().expect("a table");
    match table.get_mut(key).and_then(Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

/// Take `table.key` out of `doc`.
fn unset(doc: &mut DocumentMut, table: &str, key: &str) {
    if let Some(table) = doc.get_mut(table).and_then(Item::as_table_like_mut) {
        table.remove(key);
    }
}

/// Write into `doc` what differs between `before` and `now`.
fn write(doc: &mut DocumentMut, before: &Settings, now: &Settings) {
    let whole = |n: u32| Value::from(i64::from(n));
    if now.undo_steps != before.undo_steps {
        set(doc, "general", "undo_steps", whole(now.undo_steps));
    }
    if now.recent_packages != before.recent_packages {
        set(doc, "general", "recent_packages", whole(now.recent_packages));
    }
    if now.file_dialogs != before.file_dialogs {
        let id = if now.file_dialogs == FileDialogs::Package { "package" } else { "last" };
        set(doc, "general", "file_dialogs", Value::from(id));
    }
    if now.invert_zoom != before.invert_zoom {
        set(doc, "general", "invert_zoom", Value::from(now.invert_zoom));
    }
    if now.new_files != before.new_files {
        set(doc, "packages", "new_files", Value::from(if now.new_files == NewFiles::Embed { "embed" } else { "link" }));
    }
    if now.graph_size != before.graph_size {
        set(doc, "packages", "graph_size", whole(now.graph_size));
    }
    if now.graph_precision != before.graph_precision {
        let id = PRECISIONS.iter().find(|(_, p)| *p == now.graph_precision).map_or("float16", |(id, _)| id);
        set(doc, "packages", "graph_precision", Value::from(id));
    }
    if now.keep_unsaved != before.keep_unsaved {
        set(doc, "recovery", "keep", Value::from(now.keep_unsaved));
    }
    if now.keep_every != before.keep_every {
        set(doc, "recovery", "every", whole(now.keep_every));
    }
    if now.image_cache != before.image_cache {
        set(doc, "performance", "image_cache", whole(now.image_cache));
    }
    if now.graphics != before.graphics {
        set(doc, "performance", "graphics_api", Value::from(now.graphics.id()));
    }
    if now.antialiasing != before.antialiasing {
        set(doc, "performance", "antialiasing", Value::from(now.antialiasing));
    }
    // A command's keys are said only where they are not those by default.
    for command in Command::ALL.iter().copied() {
        let chords = now.shortcuts.of(command);
        if chords == before.shortcuts.of(command) {
            continue;
        }
        if chords == Shortcuts::defaults(command) {
            unset(doc, "shortcuts", command.id());
        } else {
            let keys: toml_edit::Array = chords.iter().map(|c| c.to_string()).collect();
            set(doc, "shortcuts", command.id(), Value::Array(keys));
        }
    }
}

/// Where the settings are kept, and what the file says.
#[derive(Clone, Debug)]
pub struct Store {
    /// The file; none: settings are not kept (tests, the autopilot).
    path: Option<PathBuf>,
    /// What it says, to be written again with the values changed; none: it could not be read,
    /// and is not written over.
    doc: Option<DocumentMut>,
    /// The settings it says.
    said: Settings,
    /// What is wrong with it: said in the settings dialog.
    pub problems: Vec<String>,
}

/// Kept nowhere: the settings as they are by default.
impl Default for Store {
    fn default() -> Store {
        Store { path: None, doc: TEMPLATE.parse().ok(), said: Settings::default(), problems: Vec::new() }
    }
}

impl Store {
    /// The settings kept in the file at `path`; none: the settings as they are by default,
    /// kept nowhere.
    pub fn open(path: Option<PathBuf>) -> Store {
        let template = || TEMPLATE.parse::<DocumentMut>().ok();
        let Some(path) = path else { return Store::default() };
        let name = file_name(&path);
        let text = match std::fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Store::unread(path, format!("{name} cannot be read: {e}")),
            Ok(meta) if !meta.is_file() || meta.len() > MAX_FILE => {
                return Store::unread(path, format!("{name} is not a settings file"));
            }
            Ok(_) => match std::fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(e) => return Store::unread(path, format!("{name} cannot be read: {e}")),
            },
        };
        let Some(text) = text else {
            // Written when a setting first changes.
            return Store { path: Some(path), doc: template(), ..Store::default() };
        };
        match text.parse::<DocumentMut>() {
            Ok(doc) => {
                let (said, problems) = read(&doc);
                let problems = problems.into_iter().map(|p| format!("{name}: {p}")).collect();
                Store { path: Some(path), doc: Some(doc), said, problems }
            }
            Err(e) => Store::unread(path, format!("{name} is not TOML, and is not written over: {}", e.to_string().trim())),
        }
    }

    /// A file that cannot be read: the settings as they are by default, not written.
    fn unread(path: PathBuf, why: String) -> Store {
        Store { path: Some(path), doc: None, said: Settings::default(), problems: vec![why] }
    }

    /// The settings the file says (as they are by default where it says none, or what cannot
    /// be taken).
    pub fn settings(&self) -> Settings {
        self.said.clone()
    }

    /// The file the settings are kept in.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Keep `settings`: the values that changed written into the file, the rest of it as it is.
    pub fn save(&mut self, settings: &Settings) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        let Some(doc) = &mut self.doc else {
            return Err(format!("The settings are not saved: {} cannot be read (fix it, or take it away)", file_name(path)));
        };
        let mut changed = doc.clone();
        write(&mut changed, &self.said, settings);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        brewed_texture_core::files::write_whole(path, changed.to_string().as_bytes())?;
        *doc = changed;
        self.said = settings.clone();
        Ok(())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The graphics API `settings` choose, for iced and the engine to take (the environment says
/// it to them): unless the environment says one already. True if it is the settings' choice.
pub fn choose_graphics(settings: &Settings) -> bool {
    if settings.graphics == Graphics::Auto || graphics_from_environment().is_some() {
        return false;
    }
    // SAFETY: called by `main` before anything else runs: no other thread reads the
    // environment meanwhile.
    unsafe { std::env::set_var("WGPU_BACKEND", settings.graphics.id()) };
    true
}

/// The graphics API the environment says (`WGPU_BACKEND`), which comes before the settings'.
pub fn graphics_from_environment() -> Option<String> {
    std::env::var("WGPU_BACKEND").ok().filter(|v| !v.is_empty())
}

/// The graphics API the settings chose could not start (`why`): the next start lets wgpu
/// choose (else the program would not start again). What to tell the user.
pub fn graphics_failed(store: &mut Store, why: &str) -> String {
    let chosen = store.settings().graphics;
    let next = match store.save(&Settings { graphics: Graphics::Auto, ..store.settings() }) {
        Ok(()) => "The next time, it starts with the one it finds best.".to_string(),
        Err(e) => format!("{e}. There, graphics_api = \"auto\" under [performance] lets it start with the one it finds best."),
    };
    format!("{} could not start with {}, as its settings say: {why}.\n\n{next}", brewed_texture_core::APP_NAME, chosen.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::Chord;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("brewed-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// What the file says at first is every setting as it is by default, nothing wrong; every
    /// setting is on a page, and a page reset has it as it is by default.
    #[test]
    fn the_first_file_says_the_defaults() {
        let doc: DocumentMut = TEMPLATE.parse().unwrap();
        assert_eq!(read(&doc), (Settings::default(), Vec::new()));
        let mut changed = Settings {
            undo_steps: 50,
            recent_packages: 3,
            file_dialogs: FileDialogs::Last,
            invert_zoom: true,
            new_files: NewFiles::Link,
            graph_size: 512,
            graph_precision: Precision::Full,
            keep_unsaved: false,
            keep_every: 10,
            image_cache: 0,
            graphics: Graphics::Vulkan,
            antialiasing: false,
            shortcuts: Shortcuts::default(),
        };
        changed.shortcuts.set(Command::Frame, vec![Chord::parse("F2").unwrap()]);
        for page in Page::ALL {
            assert!(changed.differ(&Settings::default(), page), "{page:?}");
            changed = changed.reset(page);
            assert!(!changed.differ(&Settings::default(), page), "{page:?}");
        }
        assert_eq!(changed, Settings::default(), "every setting is on a page");
    }

    /// Settings are kept: the file is written when one changes (not before), with only what
    /// changed; what people wrote there stays, what cannot be taken or is not known is said
    /// (and set as it is by default), and a file that is no TOML is not written over.
    #[test]
    fn settings_are_kept_as_people_wrote_them() {
        let dir = scratch("kept");
        let path = dir.join("settings.toml");
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings::default());
        assert!(!path.exists(), "not written before a setting changes");
        let mut changed = Settings { undo_steps: 500, graphics: Graphics::Vulkan, ..settings };
        changed.shortcuts.set(Command::Redo, vec![Chord::parse("Ctrl+Shift+Z").unwrap()]);
        changed.shortcuts.set(Command::Frame, Vec::new());
        store.save(&changed).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for said in ["undo_steps = 500", "graphics_api = \"vulkan\"", "# The graphics API", "redo = [\"Ctrl+Shift+Z\"]", "frame = []"] {
            assert!(text.contains(said), "{said}: {text}");
        }
        assert_eq!(Store::open(Some(path.clone())).settings(), changed);
        // A command given its keys by default again is not said any more.
        let mut back = changed.clone();
        back.shortcuts.set(Command::Redo, Shortcuts::defaults(Command::Redo));
        store.save(&back).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.lines().any(|l| l.starts_with("redo")) && text.contains("frame = []"), "{text}");

        // Written by hand: a comment, a key the program does not know, a value it cannot take,
        // one out of its range, a key it cannot read, and one missing.
        std::fs::write(
            &path,
            "# mine\n[general]\nundo_steps = 9000 # many\nundo_step = 5\n\n[packages]\nnew_files = \"copy\"\n\n\
             [recovery]\nkeep = false\n\n[shortcuts]\nredo = \"Hyper+Y\"\n",
        )
        .unwrap();
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings { undo_steps: 1000, keep_unsaved: false, ..Settings::default() });
        assert_eq!(settings.shortcuts.of(Command::Redo), Shortcuts::defaults(Command::Redo), "nothing it can read: its own");
        let problems = &store.problems;
        assert_eq!(problems.len(), 4, "{problems:?}");
        for said in ["general.undo_steps is 9000", "general.undo_step is no setting", "packages.new_files is \"copy\"", "\"Hyper+Y\""] {
            assert!(problems.iter().any(|p| p.contains(said)), "{said}: {problems:?}");
        }
        store.save(&Settings { keep_every: 5, ..settings.clone() }).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# mine\n[general]\nundo_steps = 9000 # many\nundo_step = 5\n\n[packages]\nnew_files = \"copy\"\n\n\
             [recovery]\nkeep = false\nevery = 5\n\n[shortcuts]\nredo = \"Hyper+Y\"\n",
            "only what changed is written"
        );
        store.save(&Settings { undo_steps: 20, keep_every: 5, ..settings }).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("undo_steps = 20 # many"), "the comment after it stays");

        // Not TOML: said, and not written over.
        std::fs::write(&path, "[general\nundo_steps = ").unwrap();
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings::default());
        assert!(store.problems[0].contains("settings.toml is not TOML"), "{:?}", store.problems);
        assert!(store.save(&Settings { undo_steps: 20, ..settings }).unwrap_err().contains("not saved"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[general\nundo_steps = ");
        // A graphics API that could not start: the next start chooses one itself.
        std::fs::write(&path, "[performance]\ngraphics_api = \"vulkan\" # mine\n").unwrap();
        let mut store = Store::open(Some(path.clone()));
        let said = graphics_failed(&mut store, "no adapter");
        assert!(said.contains("could not start with Vulkan") && said.contains("no adapter") && said.contains("The next time"), "{said}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[performance]\ngraphics_api = \"auto\" # mine\n");
        // Kept nowhere: nothing is written.
        let mut nowhere = Store::open(None);
        assert!(nowhere.save(&Settings { undo_steps: 20, ..Settings::default() }).is_ok() && nowhere.path().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
