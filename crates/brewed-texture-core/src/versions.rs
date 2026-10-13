//! Earlier versions of package files, kept as they are saved: a local history, as JetBrains'
//! IDEs keep one, in a folder of the program's own beside its settings, and never beside the
//! package (no `walls.brewtex1` next to it, nor a folder of backups in the project).
//!
//! Each package file has a folder of versions there, named by a hash of where the file is. A
//! version is a package file of its own, which says where its package was saved and when it was
//! kept (see [`crate::project::save_snapshot`]); the files the package holds (embedded) are kept
//! apart, once each, named by their bytes, as a recovery snapshot's are (see [`crate::recovery`]):
//! a package that holds a gigabyte of pictures and was saved fifty times takes a gigabyte, and
//! fifty times the rest. Each save is kept; and before a file is written over, the file as it
//! is, if the versions have not got it (it was saved elsewhere, or before versions were kept).
//! The newest versions of each package stay, as many as asked; a file kept apart that no version
//! names any more goes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::files;
use crate::instance::GraphId;
use crate::package::Package;
use crate::project::{self, FILE_EXTENSION, Loaded, Unsaved, ViewState};

/// The folder of the files packages held, kept apart.
const HELD_FOLDER: &str = "files";
/// The extension of a file a package held, kept apart (named by the hash of its bytes).
const HELD: &str = "held";
/// What the folder of a package file remembers of the file as the versions last saw it.
const SEEN: &str = "seen.json";
/// How long a file kept apart that no version names stays: another program may be keeping a
/// version that names it right now (it marks the file as wanted first, see [`touch`]).
const GRACE: Duration = Duration::from_secs(3600);

/// What a version is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The package as it was saved.
    Saved,
    /// The file as it was before it was written over, which the versions had not got: saved
    /// elsewhere, by another program, or before versions were kept.
    Found,
    /// Changes not saved, as they were when an earlier version took their place.
    Replaced,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Saved, Kind::Found, Kind::Replaced];

    /// What the name of a version calls it.
    fn id(self) -> &'static str {
        match self {
            Kind::Saved => "saved",
            Kind::Found => "found",
            Kind::Replaced => "replaced",
        }
    }
}

/// A version of a package file.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub file: PathBuf,
    pub kind: Kind,
    /// When it was kept, in milliseconds since 1970.
    pub time: u64,
}

/// What the folder of a package file remembers of the file: where it is, and its size and when
/// it was written as the versions last saw it (kept, or written by a save that was kept).
#[derive(Serialize, Deserialize)]
struct Seen {
    path: String,
    size: u64,
    modified: u64,
}

/// The versions of package files kept in a folder.
#[derive(Clone, Debug)]
pub struct Versions {
    root: PathBuf,
    grace: Duration,
}

impl Versions {
    pub fn new(root: impl Into<PathBuf>) -> Versions {
        Versions { root: root.into(), grace: GRACE }
    }

    /// The folder they are kept in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder of the versions of the package file at `path`.
    fn folder(&self, path: &Path) -> PathBuf {
        self.root.join(key(path))
    }

    /// Where a file a package held, of `hash` (see [`crate::files::embedded_hash`]), is kept.
    fn held(&self, hash: &str) -> PathBuf {
        self.root.join(HELD_FOLDER).join(format!("{hash}.{HELD}"))
    }

    /// The versions of the package file at `path`, the newest first.
    pub fn of(&self, path: &Path) -> Vec<Version> {
        let mut found = versions_in(&self.folder(path));
        found.sort_by(|a, b| b.1.cmp(&a.1));
        found.into_iter().map(|(version, _)| version).collect()
    }

    /// Keep `package`, saved as the file at `path` (or, for [`Kind::Replaced`], open from it),
    /// as a version of `kind`: the files it holds first, each once (one kept already is only
    /// marked as wanted). Not when it is what the newest version holds (see
    /// [`crate::project::fingerprint`]): saved again as it was, or with only its view moved.
    /// Then those beyond the newest `keep` go: all, when `keep` is 0, and none is kept.
    pub fn keep(&self, path: &Path, package: &Package, views: &HashMap<GraphId, ViewState>, open: GraphId, kind: Kind, keep: usize) -> Result<(), String> {
        if keep > 0 && self.newest_holds(path, package) {
            if kind == Kind::Saved {
                self.saw(path);
            }
        } else if keep > 0 {
            let folder = self.folder(path);
            made(&folder)?;
            made(&self.root.join(HELD_FOLDER))?;
            let mut done = HashSet::new();
            for resource in &package.resources {
                let (Some(hash), Some(bytes)) = (files::embedded_hash(&resource.path), &resource.data) else { continue };
                if done.insert(hash) && !touch(&self.held(hash)) {
                    files::write_whole(&self.held(hash), bytes)?;
                }
            }
            let unsaved = Unsaved { path: Some(path.display().to_string()), title: title(path), time: crate::recovery::now() };
            project::save_snapshot(&new_file(&folder, kind), package, views, open, &unsaved)?;
            if kind == Kind::Saved {
                self.saw(path);
            }
        }
        self.prune(path, keep);
        Ok(())
    }

    /// Before the file at `path` is written over: keep it as it is (see [`Kind::Found`]), if
    /// the versions have not got it (it is not the file as they last saw it). A file that cannot
    /// be read as a package is kept as it is, byte for byte.
    pub fn before_writing(&self, path: &Path, keep: usize) -> Result<(), String> {
        let Ok(meta) = std::fs::metadata(path) else { return Ok(()) };
        if keep == 0 || !meta.is_file() || self.seen(path) == Some(stamp(&meta)) {
            return Ok(());
        }
        match project::load(path) {
            Ok(loaded) => self.keep(path, &loaded.package, &loaded.views, loaded.open, Kind::Found, keep)?,
            Err(_) => {
                let folder = self.folder(path);
                made(&folder)?;
                files::replace(&new_file(&folder, Kind::Found), |beside| std::fs::copy(path, beside).map(drop))?;
                self.prune(path, keep);
            }
        }
        self.saw(path);
        Ok(())
    }

    /// Whether the newest version of `path` holds `package` as it is.
    fn newest_holds(&self, path: &Path, package: &Package) -> bool {
        let Some(newest) = self.of(path).into_iter().next() else { return false };
        match (project::fingerprint(package), project::fingerprint_file(&newest.file)) {
            (Ok(now), Ok(then)) => now == then,
            _ => false,
        }
    }

    /// The package `version` holds, with the files it held (each taken only if it is the bytes
    /// its name says).
    pub fn load(&self, version: &Version) -> Result<Loaded, String> {
        let mut loaded = project::load(&version.file)?;
        loaded.unsaved = None;
        crate::recovery::take_held(&mut loaded.package, |hash| self.held(hash));
        Ok(loaded)
    }

    /// Hold every package file's versions to the newest `keep`, and let go of what nothing needs
    /// any more: the files kept apart that no version names, the folders without versions, and
    /// what was being written by a program that stopped (for when the program starts).
    pub fn tidy(&self, keep: usize) {
        let Ok(folders) = std::fs::read_dir(&self.root) else { return };
        for folder in folders.flatten().map(|e| e.path()).filter(|f| is_key(f)) {
            let mut found = versions_in(&folder);
            found.sort_by(|a, b| b.1.cmp(&a.1));
            for (version, _) in found.iter().skip(keep) {
                let _ = std::fs::remove_file(&version.file);
            }
            self.let_go_of_leftovers(&folder);
            // None left.
            if found.len().min(keep) == 0 {
                let _ = std::fs::remove_file(folder.join(SEEN));
                let _ = std::fs::remove_dir(&folder);
            }
        }
        self.let_go_of_leftovers(&self.root.join(HELD_FOLDER));
        let named = self.named(None);
        self.let_go_of_held(|hash| !named.contains(hash));
    }

    /// Let go of the versions of `path` beyond the newest `keep` (and of its folder, when none
    /// is left), then of the files kept apart only they named.
    fn prune(&self, path: &Path, keep: usize) {
        let folder = self.folder(path);
        let mut found = versions_in(&folder);
        if found.len() <= keep {
            return;
        }
        found.sort_by(|a, b| b.1.cmp(&a.1));
        let gone = found.split_off(keep);
        // What the versions that go name, and no version of the package that stays: named by
        // another package's versions, maybe (all are read only then).
        let names = |versions: &[(Version, (u64, u32, String))]| -> HashSet<String> {
            versions
                .iter()
                .filter_map(|(v, _)| project::read_files(&v.file).ok())
                .flatten()
                .filter_map(|f| files::embedded_hash(&f).map(str::to_string))
                .collect()
        };
        let staying = names(&found);
        let mut alone: HashSet<String> = names(&gone).into_iter().filter(|h| !staying.contains(h)).collect();
        for (version, _) in &gone {
            let _ = std::fs::remove_file(&version.file);
        }
        if found.is_empty() {
            let _ = std::fs::remove_file(folder.join(SEEN));
            let _ = std::fs::remove_dir(&folder);
        }
        if !alone.is_empty() {
            let elsewhere = self.named(Some(&folder));
            alone.retain(|h| !elsewhere.contains(h));
            self.let_go_of_held(|hash| alone.contains(hash));
        }
    }

    /// The hashes of the files the versions name, of every package file but the one whose
    /// folder is `but`.
    fn named(&self, but: Option<&Path>) -> HashSet<String> {
        let Ok(folders) = std::fs::read_dir(&self.root) else { return HashSet::new() };
        let mut named = HashSet::new();
        for folder in folders.flatten().map(|e| e.path()).filter(|f| is_key(f) && Some(f.as_path()) != but) {
            for (version, _) in versions_in(&folder) {
                // (One that cannot be read names none: whatever it named is of no use without it.)
                if let Ok(files) = project::read_files(&version.file) {
                    named.extend(files.iter().filter_map(|f| files::embedded_hash(f)).map(str::to_string));
                }
            }
        }
        named
    }

    /// Let go of the files kept apart whose hashes `unwanted` says, but those wanted lately (see
    /// [`GRACE`]).
    fn let_go_of_held(&self, unwanted: impl Fn(&str) -> bool) {
        let Ok(entries) = std::fs::read_dir(self.root.join(HELD_FOLDER)) else { return };
        for file in entries.flatten().map(|e| e.path()) {
            let hash = file.file_stem().and_then(|s| s.to_str()).filter(|s| files::is_hash(s) && file.extension().is_some_and(|e| e == HELD));
            if hash.is_some_and(&unwanted) && self.old(&file) {
                let _ = std::fs::remove_file(&file);
            }
        }
    }

    /// Let go of the files a program that stopped was writing in `folder` (see
    /// [`crate::files::replace`]), long ago.
    fn let_go_of_leftovers(&self, folder: &Path) {
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        for file in entries.flatten().map(|e| e.path()) {
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name.starts_with('.') && name.ends_with(".tmp") && self.old(&file) {
                let _ = std::fs::remove_file(&file);
            }
        }
    }

    /// Whether the file at `path` was last written, or wanted, longer ago than the grace.
    fn old(&self, path: &Path) -> bool {
        let modified = std::fs::metadata(path).and_then(|m| m.modified());
        modified.is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() >= self.grace)
    }

    /// The size, and the time of writing, of the file at `path` as the versions last saw it.
    fn seen(&self, path: &Path) -> Option<(u64, u64)> {
        let bytes = std::fs::read(self.folder(path).join(SEEN)).ok()?;
        let seen: Seen = serde_json::from_slice(&bytes).ok()?;
        Some((seen.size, seen.modified))
    }

    /// Remember the file at `path` as it is now.
    fn saw(&self, path: &Path) {
        let Ok(meta) = std::fs::metadata(path) else { return };
        let (size, modified) = stamp(&meta);
        let seen = Seen { path: path.display().to_string(), size, modified };
        if let Ok(bytes) = serde_json::to_vec_pretty(&seen) {
            let _ = files::write_whole(&self.folder(path).join(SEEN), &bytes);
        }
    }
}

/// Make the folder `dir`, if it is not there.
fn made(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot make '{}': {e}", dir.display()))
}

/// The name of the folder of the versions of the package file at `path`: a hash of where it is,
/// told apart as Windows and macOS tell names of files apart (not by case).
fn key(path: &Path) -> String {
    let mut name = crate::package::clean_path(path).display().to_string();
    if cfg!(any(windows, target_os = "macos")) {
        name = name.to_lowercase();
    }
    files::sha256(name.as_bytes())[..16].to_string()
}

/// Is `folder` the folder of a package file's versions?
fn is_key(folder: &Path) -> bool {
    let name = folder.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    folder.is_dir() && name.len() == 16 && name.bytes().all(|c| c.is_ascii_hexdigit())
}

/// What a list calls the package file at `path`: its name.
fn title(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The size of a file, and when it was written (in nanoseconds since 1970).
fn stamp(meta: &std::fs::Metadata) -> (u64, u64) {
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
    (meta.len(), modified)
}

/// Mark the file at `path` as wanted now (see [`GRACE`]): false if it is not there to be.
fn touch(path: &Path) -> bool {
    std::fs::File::options().write(true).open(path).and_then(|f| f.set_modified(SystemTime::now())).is_ok()
}

/// A name for a new version of `kind` in `folder`: `<milliseconds>-<n>-<kind>.brewtex`, the
/// first `n` no version of that millisecond has (they are kept in its order).
fn new_file(folder: &Path, kind: Kind) -> PathBuf {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let name = |n: u32, kind: Kind| folder.join(format!("{ms}-{n}-{}.{FILE_EXTENSION}", kind.id()));
    let n = (0u32..).find(|n| Kind::ALL.iter().all(|k| !name(*n, *k).exists())).unwrap_or_default();
    name(n, kind)
}

/// The versions in `folder`, each with what orders them: when it was kept, its number among
/// those kept then, and its name.
fn versions_in(folder: &Path) -> Vec<(Version, (u64, u32, String))> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.path();
            if file.extension()? != FILE_EXTENSION {
                return None;
            }
            let name = file.file_stem()?.to_str()?.to_string();
            let mut parts = name.split('-');
            let time: u64 = parts.next()?.parse().ok()?;
            let n: u32 = parts.next()?.parse().ok()?;
            let kind = parts.next().and_then(|id| Kind::ALL.into_iter().find(|k| k.id() == id))?;
            if parts.next().is_some() {
                return None;
            }
            Some((Version { file, kind, time }, (time, n, name)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::package::{Resource, ResourceKind};

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("brewed-versions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A package of one graph called `name`, holding a file of `bytes`.
    fn package(name: &str, bytes: &[u8]) -> Package {
        let mut package = Package::new(name, crate::graph::Graph::default());
        let bytes = Arc::new(bytes.to_vec());
        let path = files::embedded_path("bricks.png", &bytes);
        package.resources.push(Resource { data: Some(bytes), ..Resource::linked(path, ResourceKind::Bitmap) });
        package
    }

    fn held_files(root: &Path) -> usize {
        std::fs::read_dir(root.join(HELD_FOLDER)).map_or(0, |e| e.count())
    }

    /// Each save is a version, the newest first, and loads back as it was, with the files it
    /// held, which are kept once however many versions hold them. Nothing is put beside the
    /// package.
    #[test]
    fn saves_are_kept_and_load_back() {
        let dir = root("saves");
        let versions = Versions::new(dir.join("History"));
        let file = dir.join("work").join("walls.brewtex");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let a = package("Wall", format!("bricks {}", std::process::id()).as_bytes());
        let mut b = a.clone();
        b.graphs[0].name = "Wall 2".into();
        for p in [&a, &b] {
            versions.before_writing(&file, 20).unwrap();
            project::save(&file, p, &HashMap::new(), p.first()).unwrap();
            versions.keep(&file, p, &HashMap::new(), p.first(), Kind::Saved, 20).unwrap();
        }
        let kept = versions.of(&file);
        assert_eq!(kept.iter().map(|v| v.kind).collect::<Vec<_>>(), [Kind::Saved, Kind::Saved], "nothing found: every save was kept");
        let newest = versions.load(&kept[0]).unwrap();
        assert_eq!(newest.package.graphs[0].name, "Wall 2");
        assert_eq!(newest.unsaved, None);
        assert_eq!(newest.package.resources[0].data, a.resources[0].data, "with the file it held");
        assert_eq!(versions.load(&kept[1]).unwrap().package.graphs[0].name, "Wall");
        assert_eq!(held_files(versions.root()), 1, "kept once");
        let beside: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(beside, [std::ffi::OsString::from("walls.brewtex")], "nothing beside the package");
        // Another path, another history.
        assert!(versions.of(&dir.join("work").join("other.brewtex")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A package saved again as it was, or with only its view moved, or with another picture
    /// (the 3D view turned before saving: the picture is of what it shows), is no new version; a
    /// node moved is.
    #[test]
    fn saves_of_the_same_package_are_one_version() {
        let dir = root("same");
        let versions = Versions::new(dir.join("History"));
        let file = dir.join("walls.brewtex");
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = package("Wall", b"same bricks");
        let open = p.first();
        let save = |p: &Package, views: &HashMap<GraphId, ViewState>| {
            versions.before_writing(&file, 20).unwrap();
            project::save(&file, p, views, open).unwrap();
            versions.keep(&file, p, views, open, Kind::Saved, 20).unwrap();
        };
        for _ in 0..5 {
            save(&p, &HashMap::new());
        }
        let moved = [(open, ViewState { graph_pan: Some([300.0, 20.0]), graph_zoom: Some(1.5), ..ViewState::default() })].into_iter().collect();
        save(&p, &moved);
        // Pictures of the package that differ, and another node shown in the 2D view.
        let shown = [(open, ViewState { preview: Some(crate::graph::NodeId(7)), ..ViewState::default() })].into_iter().collect();
        for picture in [&b"a picture of the model turned one way"[..], &b"and another, turned the other way"[..]] {
            versions.before_writing(&file, 20).unwrap();
            project::save_with_picture(&file, &p, &shown, open, Some(picture)).unwrap();
            versions.keep(&file, &p, &shown, open, Kind::Saved, 20).unwrap();
            assert_eq!(project::read_picture(&file).as_deref(), Some(picture), "the file has the picture it was saved with");
        }
        assert_eq!(versions.of(&file).len(), 1, "saved eight times as it was");
        let mut graph = crate::graph::Graph::default();
        graph.add_node(crate::NodeKind::BLUR, [10.0, 20.0]);
        p.graphs[0].graph = std::sync::Arc::new(graph);
        save(&p, &HashMap::new());
        assert_eq!(versions.of(&file).len(), 2, "a node added: a version");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file about to be written over that the versions have not got is kept first: one saved
    /// before versions were kept, or changed elsewhere since; once, not before every save. One
    /// that is not a package is kept as it is.
    #[test]
    fn files_written_over_are_kept_first() {
        let dir = root("found");
        let versions = Versions::new(dir.join("History"));
        let file = dir.join("walls.brewtex");
        std::fs::create_dir_all(&dir).unwrap();
        let old = package("Before", b"old bricks");
        project::save(&file, &old, &HashMap::new(), old.first()).unwrap();
        let new = package("After", b"new bricks");
        let save = |p: &Package| {
            versions.before_writing(&file, 20).unwrap();
            project::save(&file, p, &HashMap::new(), p.first()).unwrap();
            versions.keep(&file, p, &HashMap::new(), p.first(), Kind::Saved, 20).unwrap();
        };
        save(&new);
        let kinds = |v: &Versions| v.of(&file).iter().map(|v| v.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&versions), [Kind::Saved, Kind::Found]);
        assert_eq!(versions.load(&versions.of(&file)[1]).unwrap().package.graphs[0].name, "Before");
        save(&new);
        assert_eq!(kinds(&versions), [Kind::Saved, Kind::Found], "seen, and saved as it was: nothing kept again");
        // Written elsewhere meanwhile (another size).
        let elsewhere = package("Elsewhere, and longer", b"their bricks");
        project::save(&file, &elsewhere, &HashMap::new(), elsewhere.first()).unwrap();
        save(&new);
        assert_eq!(versions.load(&versions.of(&file)[1]).unwrap().package.graphs[0].name, "Elsewhere, and longer");
        // Not a package: kept byte for byte.
        std::fs::write(&file, b"not a package").unwrap();
        versions.before_writing(&file, 20).unwrap();
        let found = &versions.of(&file)[0];
        assert_eq!((found.kind, std::fs::read(&found.file).unwrap()), (Kind::Found, b"not a package".to_vec()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The newest versions stay, as many as asked; the files only those that went held go (once
    /// they were not wanted for a while), those others hold stay; none are kept when none are
    /// asked for.
    #[test]
    fn the_oldest_go_with_the_files_only_they_held() {
        let dir = root("prune");
        let mut versions = Versions::new(dir.join("History"));
        versions.grace = Duration::ZERO;
        let (walls, floor) = (dir.join("walls.brewtex"), dir.join("floor.brewtex"));
        let shared = package("Shared", b"shared bricks");
        let keep = |v: &Versions, path: &Path, p: &Package, n: usize| v.keep(path, p, &HashMap::new(), p.first(), Kind::Saved, n).unwrap();
        keep(&versions, &floor, &shared, 5);
        keep(&versions, &walls, &shared, 2);
        keep(&versions, &walls, &package("Own", b"its own bricks"), 2);
        assert_eq!(held_files(versions.root()), 2);
        keep(&versions, &walls, &package("Plain", b"plain bricks"), 2);
        let names: Vec<String> = versions.of(&walls).iter().map(|v| versions.load(v).unwrap().package.graphs[0].name.clone()).collect();
        assert_eq!(names, ["Plain", "Own"], "the newest two");
        assert_eq!(held_files(versions.root()), 3, "the shared file stays: the floor's version holds it");
        keep(&versions, &walls, &package("Last", b"last bricks"), 2);
        keep(&versions, &walls, &package("Final", b"final bricks"), 2);
        assert_eq!(held_files(versions.root()), 3, "shared, last and final");
        // None asked for: none kept, and the folder goes.
        keep(&versions, &walls, &package("None", b"no bricks"), 0);
        assert!(versions.of(&walls).is_empty());
        assert!(!versions.folder(&walls).exists());
        assert_eq!(versions.before_writing(&walls, 0), Ok(()));
        // Tidied as the program starts: held to what is asked now, the files no version holds
        // gone.
        versions.tidy(0);
        assert_eq!(held_files(versions.root()), 0);
        assert!(versions.of(&floor).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file no version names stays while it may be wanted: another program may be keeping a
    /// version that names it right now.
    #[test]
    fn files_wanted_lately_stay() {
        let dir = root("grace");
        let versions = Versions::new(dir.join("History"));
        let walls = dir.join("walls.brewtex");
        let p = package("Wall", b"bricks of now");
        versions.keep(&walls, &p, &HashMap::new(), p.first(), Kind::Saved, 1).unwrap();
        let q = package("Other", b"other bricks");
        versions.keep(&walls, &q, &HashMap::new(), q.first(), Kind::Saved, 1).unwrap();
        assert_eq!(held_files(versions.root()), 2, "not let go of yet");
        versions.tidy(1);
        assert_eq!(held_files(versions.root()), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
