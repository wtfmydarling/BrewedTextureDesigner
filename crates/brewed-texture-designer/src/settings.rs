//! The program's settings, which Edit → Settings… changes: kept in `settings.toml`, in the
//! settings folder (see [`crate::workspace::folder`]). The file says only the settings that are
//! not as they are by default: one it does not say follows the program's default, whatever it is
//! then. People may edit the file too: the program writes only the values that change, and keeps
//! the rest of it as it is (comments, the order, what this version does not know). What the file
//! says that cannot be taken is said, and set as it is by default; a file that cannot be read at
//! all is not written over.

use std::path::{Path, PathBuf};

use brewed_texture_core::Precision;
use brewed_texture_core::history::DEFAULT_STEPS;
use toml_edit::{DocumentMut, Item, Value};

use crate::shortcuts::{Command, Shortcuts};

/// What `settings.toml` says when the program writes it first: its tables, and what each can
/// say, with the defaults.
const TEMPLATE: &str = r#"# The settings of Brewed Texture Designer. Edit → Settings… changes them, and so may you: a
# setting written here is taken in place of its default; one that is not is as it is by
# default. What cannot be taken is said in Edit → Settings….

# undo_steps: how many steps Undo can go back in each package (10 to 1000; 256 by default).
# recent_packages: how many packages File → Open Recent offers (0 to 20; 8 by default).
# file_dialogs: where the file dialogs open: "auto" (saving a package or one of its files, in
# the package's folder, any other where the system opens it; the default), "package" (the
# folder of the package being edited) or "last" (where a dialog was answered last).
# invert_zoom: the mouse wheel zooms the other way, in the graph and the 2D and 3D views
# (false by default).
[general]

# new: what becomes of a file chosen for a node or dropped on the window: "embed" (the package
# holds it: the package is one file to copy or send; the default) or "link" (the package names
# it where it is).
[files]

# size: the size of the images of a new graph, in pixels (16 to 8192; 2048 by default).
# precision: what they are kept in: "unorm8" (8 bits), "float16" (16 bits float; the default)
# or "float32" (32 bits float).
[new_graphs]

# keep: keep unsaved work, to offer it back after a crash (true by default).
# every_minutes: how often (1 to 60; 2 by default).
[recovery]

# Taken when the program starts. api: the graphics API, "auto" (the one wgpu finds best; the
# default), "vulkan", "dx12" (Windows) or "metal" (macOS); the environment variable
# WGPU_BACKEND, where there is one, comes first. antialiasing: whether the edges of lines and
# shapes are smoothed (true by default).
[graphics]

# The keys of the commands, where they are not those by default: redo = ["Ctrl+Y"], say, or
# frame = [] for none. A command has one or two; Ctrl is ⌘ on macOS.
[shortcuts]
"#;

/// The largest settings file read: what is larger is not settings.
const MAX_FILE: u64 = 1 << 20;

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
    /// The graphics API, and whether edges are smoothed (taken when the program starts).
    pub graphics: Graphics,
    pub antialiasing: bool,
    /// The keys of the commands.
    pub shortcuts: Shortcuts,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            undo_steps: DEFAULT_STEPS as u32,
            recent_packages: crate::workspace::RECENT_OFFERED,
            file_dialogs: FileDialogs::Auto,
            invert_zoom: false,
            new_files: NewFiles::Embed,
            graph_size: 2048,
            graph_precision: Precision::Half,
            keep_unsaved: true,
            keep_every: 2,
            graphics: Graphics::Auto,
            antialiasing: true,
            shortcuts: Shortcuts::default(),
        }
    }
}

/// A page of the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Packages,
    Recovery,
    Graphics,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::General, Page::Packages, Page::Recovery, Page::Graphics, Page::Shortcuts];

    pub fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Packages => "Packages",
            Page::Recovery => "Recovery",
            Page::Graphics => "Graphics",
            Page::Shortcuts => "Shortcuts",
        }
    }

    /// What the page is about, under its name.
    pub fn about(self) -> &'static str {
        match self {
            Page::General => "Undo, the packages offered again, the file dialogs, the mouse wheel.",
            Page::Packages => "The files taken into packages, and the graphs made in them.",
            Page::Recovery => "Unsaved work kept, to be offered back after a crash.",
            Page::Graphics => "How the program draws: taken when it starts.",
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
    Graphics(Graphics),
    Antialiasing(bool),
}

impl Settings {
    /// Set one setting (held to its range).
    pub fn set(&mut self, setting: Setting) {
        match setting {
            Setting::UndoSteps(n) => self.undo_steps = n.clamp(10, 1000),
            Setting::RecentPackages(n) => self.recent_packages = n.min(crate::workspace::RECENT_KEPT as u32),
            Setting::FileDialogs(f) => self.file_dialogs = f,
            Setting::InvertZoom(on) => self.invert_zoom = on,
            Setting::NewFiles(f) => self.new_files = f,
            Setting::GraphSize(n) => self.graph_size = n.clamp(16, 8192),
            Setting::GraphPrecision(p) => self.graph_precision = p,
            Setting::KeepUnsaved(on) => self.keep_unsaved = on,
            Setting::KeepEvery(n) => self.keep_every = n.clamp(1, 60),
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
            Page::Graphics => {
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
}

/// Where the file dialogs open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileDialogs {
    /// Where each finds best: saving a package or one of its files, in the package's folder;
    /// looking for a file that is not there, where it was; any other, where the system opens it.
    Auto,
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

const FILE_DIALOGS: [(&str, FileDialogs); 3] = [("auto", FileDialogs::Auto), ("package", FileDialogs::Package), ("last", FileDialogs::Last)];
const NEW_FILES: [(&str, NewFiles); 2] = [("embed", NewFiles::Embed), ("link", NewFiles::Link)];
const PRECISIONS: [(&str, Precision); 3] = [("unorm8", Precision::Bits8), ("float16", Precision::Half), ("float32", Precision::Full)];

/// The name of `value` among `names`.
fn id_of<T: PartialEq>(names: &[(&'static str, T)], value: &T) -> &'static str {
    names.iter().find(|(_, v)| v == value).map_or("", |(id, _)| id)
}

/// The settings a document says, and what it says that cannot be taken (each set as it is by
/// default, or held to its range). What this version does not know is left as it is.
fn read(doc: &DocumentMut) -> (Settings, Vec<String>) {
    let d = Settings::default();
    let mut problems = Vec::new();
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
    let new_files = get("files", "new");
    let size = get("new_graphs", "size");
    let precision = get("new_graphs", "precision");
    let keep = get("recovery", "keep");
    let every = get("recovery", "every_minutes");
    let api = get("graphics", "api");
    let antialiasing = get("graphics", "antialiasing");
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
    let recent_kept = crate::workspace::RECENT_KEPT as u32;
    let settings = Settings {
        undo_steps: number(undo_steps, "general.undo_steps", d.undo_steps, 10, 1000, &mut said),
        recent_packages: number(recent_packages, "general.recent_packages", d.recent_packages, 0, recent_kept, &mut said),
        file_dialogs: named(file_dialogs, "general.file_dialogs", &FILE_DIALOGS, d.file_dialogs, &mut said),
        invert_zoom: boolean(invert_zoom, "general.invert_zoom", d.invert_zoom, &mut said),
        new_files: named(new_files, "files.new", &NEW_FILES, d.new_files, &mut said),
        graph_size: number(size, "new_graphs.size", d.graph_size, 16, 8192, &mut said),
        graph_precision: named(precision, "new_graphs.precision", &PRECISIONS, d.graph_precision, &mut said),
        keep_unsaved: boolean(keep, "recovery.keep", d.keep_unsaved, &mut said),
        keep_every: number(every, "recovery.every_minutes", d.keep_every, 1, 60, &mut said),
        graphics: named(api, "graphics.api", &graphics, d.graphics, &mut said),
        antialiasing: boolean(antialiasing, "graphics.antialiasing", d.antialiasing, &mut said),
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

/// Say `table.key` in `doc`: `value`, or, for none (it is as it is by default), nothing. What
/// was said around the value (a comment after it) stays.
fn say(doc: &mut DocumentMut, table: &str, key: &str, value: Option<Value>) {
    let Some(value) = value else {
        if let Some(table) = doc.get_mut(table).and_then(Item::as_table_like_mut) {
            table.remove(key);
        }
        return;
    };
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

/// Write into `doc` what differs between `before` and `now`: a value, where it is not as it is
/// by default; else nothing.
fn write(doc: &mut DocumentMut, before: &Settings, now: &Settings) {
    let d = Settings::default();
    let whole = |n: u32| Value::from(i64::from(n));
    let mut changed = |table: &str, key: &str, differs: bool, default: bool, value: Value| {
        if differs {
            say(doc, table, key, (!default).then_some(value));
        }
    };
    macro_rules! setting {
        ($field:ident, $table:literal, $key:literal, $value:expr) => {
            changed($table, $key, now.$field != before.$field, now.$field == d.$field, $value)
        };
    }
    setting!(undo_steps, "general", "undo_steps", whole(now.undo_steps));
    setting!(recent_packages, "general", "recent_packages", whole(now.recent_packages));
    setting!(file_dialogs, "general", "file_dialogs", Value::from(id_of(&FILE_DIALOGS, &now.file_dialogs)));
    setting!(invert_zoom, "general", "invert_zoom", Value::from(now.invert_zoom));
    setting!(new_files, "files", "new", Value::from(id_of(&NEW_FILES, &now.new_files)));
    setting!(graph_size, "new_graphs", "size", whole(now.graph_size));
    setting!(graph_precision, "new_graphs", "precision", Value::from(id_of(&PRECISIONS, &now.graph_precision)));
    setting!(keep_unsaved, "recovery", "keep", Value::from(now.keep_unsaved));
    setting!(keep_every, "recovery", "every_minutes", whole(now.keep_every));
    setting!(graphics, "graphics", "api", Value::from(now.graphics.id()));
    setting!(antialiasing, "graphics", "antialiasing", Value::from(now.antialiasing));
    for command in Command::ALL.iter().copied() {
        let chords = now.shortcuts.of(command);
        let keys: toml_edit::Array = chords.iter().map(|c| c.to_string()).collect();
        changed("shortcuts", command.id(), chords != before.shortcuts.of(command), chords == Shortcuts::defaults(command), Value::Array(keys));
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
        Err(e) => format!("{e}. There, api = \"auto\" under [graphics] lets it start with the one it finds best."),
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

    /// The defaults are what the program did before it had settings for them: 256 steps of
    /// undo, 8 recent packages, dialogs where each opened, the wheel as it turned, lines
    /// smoothed. What the file says at first is them, nothing wrong; every setting is on a
    /// page, and a page reset has it as it is by default.
    #[test]
    fn the_first_file_says_the_defaults() {
        let d = Settings::default();
        assert_eq!((d.undo_steps, d.recent_packages, d.file_dialogs, d.invert_zoom, d.antialiasing), (256, 8, FileDialogs::Auto, false, true));
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

    /// A file the first version of the settings wrote (every setting as it was by default then)
    /// is read as it says, nothing wrong: what is no setting any more, or is said another way
    /// now (unsaved work kept every so many seconds), is left alone, and the defaults of now
    /// are taken in its place.
    #[test]
    fn a_file_of_the_first_settings_is_read() {
        let first = "# The settings of Brewed Texture Designer.\n\n[interface]\nscale = 1.0\n\n[files]\nnew = \"embed\"\n\n\
                     [new_graphs]\nsize = 2048\nprecision = \"float16\"\n\n[recovery]\nkeep = true\nevery = 30\n\n[graphics]\napi = \"auto\"\n";
        let doc: DocumentMut = first.parse().unwrap();
        assert_eq!(read(&doc), (Settings::default(), Vec::new()));
        let doc: DocumentMut = first.replace("new = \"embed\"", "new = \"link\"").replace("api = \"auto\"", "api = \"vulkan\"").parse().unwrap();
        let (settings, problems) = read(&doc);
        assert_eq!((settings.new_files, settings.graphics, problems.len()), (NewFiles::Link, Graphics::Vulkan, 0), "chosen then, taken now");
    }

    /// Settings are kept: the file is written when one changes (not before), with only what is
    /// not as it is by default, and a setting set back is not said any more; what people wrote
    /// there stays, what cannot be taken is said (and set as it is by default), and a file that
    /// is no TOML is not written over.
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
        let said = |text: &str, key: &str| text.lines().any(|l| l.starts_with(key));
        for line in ["undo_steps = 500", "api = \"vulkan\"", "redo = [\"Ctrl+Shift+Z\"]", "frame = []"] {
            assert!(said(&text, line), "{line}: {text}");
        }
        for key in ["recent_packages", "file_dialogs", "new ", "size", "keep", "every_minutes", "antialiasing", "undo "] {
            assert!(!said(&text, key), "{key} is as it is by default, and not said: {text}");
        }
        assert!(text.contains("# undo_steps: how many steps"), "what can be said, said: {text}");
        assert_eq!(Store::open(Some(path.clone())).settings(), changed);
        // Set back as they are by default: not said any more.
        let mut back = changed.clone();
        back.shortcuts.set(Command::Redo, Shortcuts::defaults(Command::Redo));
        back.undo_steps = DEFAULT_STEPS as u32;
        store.save(&back).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!said(&text, "redo") && !said(&text, "undo_steps") && said(&text, "frame = []"), "{text}");

        // Written by hand: a comment, a key the program does not know, a value it cannot take,
        // one out of its range, a key it cannot read, and one missing.
        std::fs::write(
            &path,
            "# mine\n[general]\nundo_steps = 9000 # many\nundo_step = 5\n\n[files]\nnew = \"copy\"\n\n\
             [recovery]\nkeep = false\n\n[shortcuts]\nredo = \"Hyper+Y\"\n",
        )
        .unwrap();
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings { undo_steps: 1000, keep_unsaved: false, ..Settings::default() });
        assert_eq!(settings.shortcuts.of(Command::Redo), Shortcuts::defaults(Command::Redo), "nothing it can read: its own");
        let problems = &store.problems;
        assert_eq!(problems.len(), 3, "what it does not know, it leaves alone: {problems:?}");
        for said in ["general.undo_steps is 9000", "files.new is \"copy\"", "\"Hyper+Y\""] {
            assert!(problems.iter().any(|p| p.contains(said)), "{said}: {problems:?}");
        }
        store.save(&Settings { keep_every: 5, ..settings.clone() }).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# mine\n[general]\nundo_steps = 9000 # many\nundo_step = 5\n\n[files]\nnew = \"copy\"\n\n\
             [recovery]\nkeep = false\nevery_minutes = 5\n\n[shortcuts]\nredo = \"Hyper+Y\"\n",
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
        std::fs::write(&path, "[graphics]\napi = \"vulkan\" # mine\n").unwrap();
        let mut store = Store::open(Some(path.clone()));
        let said = graphics_failed(&mut store, "no adapter");
        assert!(said.contains("could not start with Vulkan") && said.contains("no adapter") && said.contains("The next time"), "{said}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[graphics]\n");
        assert_eq!(Store::open(Some(path.clone())).settings().graphics, Graphics::Auto);
        // Kept nowhere: nothing is written.
        let mut nowhere = Store::open(None);
        assert!(nowhere.save(&Settings { undo_steps: 20, ..Settings::default() }).is_ok() && nowhere.path().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
