//! The program's settings, which Edit → Settings… changes: kept in `settings.toml`, in the
//! settings folder (see [`crate::workspace::folder`]). People may edit the file too: the program
//! writes only the values that change, and keeps the rest of it as it is (comments, the order,
//! what this version does not know). What the file says that cannot be taken is said, and set as
//! it is by default; a file that cannot be read at all is not written over.

use std::path::{Path, PathBuf};

use brewed_texture_core::Precision;
use toml_edit::{DocumentMut, Item, Value};

/// What `settings.toml` says when the program writes it first: every setting, as it is by
/// default, and what it is.
const TEMPLATE: &str = r#"# The settings of Brewed Texture Designer. Edit → Settings… changes them, and so may you:
# the program keeps what you write here, and says what it cannot take.

[interface]
# How large the interface is drawn, over what the system says: 1 is as it says (0.5 to 3).
scale = 1.0

[files]
# What becomes of a file chosen for a node or dropped on the window: "embed" (the package holds
# it: the package is one file to copy or send) or "link" (the package names it where it is).
new = "embed"

[new_graphs]
# The size of the images of a new graph, in pixels (16 to 8192), and what they are kept in:
# "unorm8" (8 bits), "float16" (16 bits float) or "float32" (32 bits float).
size = 2048
precision = "float16"

[recovery]
# Keep unsaved work, to offer it back after a crash, and how often (in seconds, 5 to 3600).
keep = true
every = 30

[graphics]
# The graphics API: "auto", or "vulkan", "dx12" (Windows), "metal" (macOS). Taken when the
# program starts; the environment variable WGPU_BACKEND, where there is one, comes first.
api = "auto"
"#;

/// The largest settings file read: what is larger is not settings.
const MAX_FILE: u64 = 1 << 20;

/// The settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// How large the interface is drawn, over what the system says.
    pub scale: f32,
    /// What becomes of a file chosen for a node or dropped on the window.
    pub new_files: NewFiles,
    /// A new graph: the size of its images, and what they are kept in.
    pub graph_size: u32,
    pub graph_precision: Precision,
    /// Unsaved work is kept for recovery after a crash (see `app::autosave`), every so many
    /// seconds.
    pub keep_unsaved: bool,
    pub keep_every: u32,
    /// The graphics API (taken when the program starts).
    pub graphics: Graphics,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            scale: 1.0,
            new_files: NewFiles::Embed,
            graph_size: 2048,
            graph_precision: Precision::Half,
            keep_unsaved: true,
            keep_every: 30,
            graphics: Graphics::Auto,
        }
    }
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

/// The settings a document says, and what it says that cannot be taken (each set as it is by
/// default, or held to its range).
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
    let scale = get("interface", "scale");
    let new_files = get("files", "new");
    let size = get("new_graphs", "size");
    let precision = get("new_graphs", "precision");
    let keep = get("recovery", "keep");
    let every = get("recovery", "every");
    let api = get("graphics", "api");
    let mut said = |what: String| problems.push(what);
    let settings = Settings {
        scale: number(scale, "interface.scale", d.scale as f64, 0.5, 3.0, &mut said) as f32,
        new_files: named(new_files, "files.new", &[("embed", NewFiles::Embed), ("link", NewFiles::Link)], d.new_files, &mut said),
        graph_size: number(size, "new_graphs.size", d.graph_size as f64, 16.0, 8192.0, &mut said).round() as u32,
        graph_precision: named(
            precision,
            "new_graphs.precision",
            &[("unorm8", Precision::Bits8), ("float16", Precision::Half), ("float32", Precision::Full)],
            d.graph_precision,
            &mut said,
        ),
        keep_unsaved: match keep {
            None => d.keep_unsaved,
            Some(v) => v.as_bool().unwrap_or_else(|| {
                said(format!("recovery.keep is {}, not true or false", v.to_string().trim()));
                d.keep_unsaved
            }),
        },
        keep_every: number(every, "recovery.every", d.keep_every as f64, 5.0, 3600.0, &mut said).round() as u32,
        graphics: {
            let all = [Graphics::Auto, Graphics::Vulkan, Graphics::Dx12, Graphics::Metal];
            let names: Vec<(&str, Graphics)> = all.iter().map(|g| (g.id(), *g)).collect();
            named(api, "graphics.api", &names, d.graphics, &mut said)
        },
    };
    (settings, problems)
}

/// A number the file says, held to `min..=max`; `default` for none, or for what is no number.
fn number(value: Option<Value>, name: &str, default: f64, min: f64, max: f64, said: &mut impl FnMut(String)) -> f64 {
    let Some(value) = value else { return default };
    let n = match &value {
        Value::Float(f) => *f.value(),
        Value::Integer(i) => *i.value() as f64,
        other => {
            said(format!("{name} is {}, not a number", other.to_string().trim()));
            return default;
        }
    };
    if !n.is_finite() {
        said(format!("{name} is {n}, not a number"));
        return default;
    }
    let held = n.clamp(min, max);
    if held != n {
        said(format!("{name} is {n}: taken as {held}, from {min} to {max}"));
    }
    held
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

/// Write into `doc` what differs between `before` and `now`.
fn write(doc: &mut DocumentMut, before: &Settings, now: &Settings) {
    if now.scale != before.scale {
        // (As the number people would write: 1.25, not 1.2500000476837158.)
        set(doc, "interface", "scale", Value::from(format!("{}", now.scale).parse::<f64>().unwrap_or(1.0)));
    }
    if now.new_files != before.new_files {
        set(doc, "files", "new", Value::from(if now.new_files == NewFiles::Embed { "embed" } else { "link" }));
    }
    if now.graph_size != before.graph_size {
        set(doc, "new_graphs", "size", Value::from(i64::from(now.graph_size)));
    }
    if now.graph_precision != before.graph_precision {
        let id = match now.graph_precision {
            Precision::Bits8 => "unorm8",
            Precision::Half => "float16",
            Precision::Full => "float32",
        };
        set(doc, "new_graphs", "precision", Value::from(id));
    }
    if now.keep_unsaved != before.keep_unsaved {
        set(doc, "recovery", "keep", Value::from(now.keep_unsaved));
    }
    if now.keep_every != before.keep_every {
        set(doc, "recovery", "every", Value::from(i64::from(now.keep_every)));
    }
    if now.graphics != before.graphics {
        set(doc, "graphics", "api", Value::from(now.graphics.id()));
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
        self.said
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
        self.said = *settings;
        Ok(())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The graphics API `settings` choose, for iced and the engine to take (the environment says
/// it to them): unless the environment says one already. True if it is the settings' choice.
pub fn choose_graphics(settings: &Settings) -> bool {
    if settings.graphics == Graphics::Auto || std::env::var_os("WGPU_BACKEND").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    // SAFETY: called by `main` before anything else runs: no other thread reads the
    // environment meanwhile.
    unsafe { std::env::set_var("WGPU_BACKEND", settings.graphics.id()) };
    true
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("brewed-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// What the file says at first is every setting as it is by default, nothing wrong.
    #[test]
    fn the_first_file_says_the_defaults() {
        let doc: DocumentMut = TEMPLATE.parse().unwrap();
        assert_eq!(read(&doc), (Settings::default(), Vec::new()));
    }

    /// Settings are kept: the file is written when one changes (not before), with only what
    /// changed; what people wrote there stays, what cannot be taken is said and set as it is by
    /// default, and a file that is no TOML is not written over.
    #[test]
    fn settings_are_kept_as_people_wrote_them() {
        let dir = scratch("kept");
        let path = dir.join("settings.toml");
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings::default());
        assert!(!path.exists(), "not written before a setting changes");
        let changed = Settings { scale: 1.25, graphics: Graphics::Vulkan, ..settings };
        store.save(&changed).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("scale = 1.25") && text.contains("api = \"vulkan\"") && text.contains("# The graphics API"), "{text}");
        assert_eq!(Store::open(Some(path.clone())).settings(), changed);

        // Written by hand: a comment, a key the program does not know, a value it cannot take,
        // one out of its range, and one missing.
        std::fs::write(
            &path,
            "# mine\n[interface]\nscale = 9 # big\ntheme = \"dark\"\n\n[files]\nnew = \"copy\"\n\n[recovery]\nkeep = false\n",
        )
        .unwrap();
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings { scale: 3.0, keep_unsaved: false, ..Settings::default() });
        assert_eq!(store.problems.len(), 2, "{:?}", store.problems);
        assert!(store.problems.iter().any(|p| p.contains("interface.scale is 9")));
        assert!(store.problems.iter().any(|p| p.contains("files.new is \"copy\"")));
        store.save(&Settings { keep_every: 120, ..settings }).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# mine\n[interface]\nscale = 9 # big\ntheme = \"dark\"\n\n[files]\nnew = \"copy\"\n\n[recovery]\nkeep = false\nevery = 120\n",
            "only what changed is written"
        );
        store.save(&Settings { scale: 2.0, keep_every: 120, ..settings }).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("scale = 2.0 # big"), "the comment after it stays");

        // Not TOML: said, and not written over.
        std::fs::write(&path, "[interface\nscale = ").unwrap();
        let mut store = Store::open(Some(path.clone()));
        let settings = store.settings();
        assert_eq!(settings, Settings::default());
        assert!(store.problems[0].contains("settings.toml is not TOML"), "{:?}", store.problems);
        assert!(store.save(&Settings { scale: 2.0, ..settings }).unwrap_err().contains("not saved"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[interface\nscale = ");
        // A graphics API that could not start: the next start chooses one itself.
        std::fs::write(&path, "[graphics]\napi = \"vulkan\" # mine\n").unwrap();
        let mut store = Store::open(Some(path.clone()));
        let said = graphics_failed(&mut store, "no adapter");
        assert!(said.contains("could not start with Vulkan") && said.contains("no adapter") && said.contains("The next time"), "{said}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[graphics]\napi = \"auto\" # mine\n");
        // Kept nowhere: nothing is written.
        let mut nowhere = Store::open(None);
        assert!(nowhere.save(&Settings { scale: 2.0, ..Settings::default() }).is_ok() && nowhere.path().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
