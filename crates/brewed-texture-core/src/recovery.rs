//! Work a crash would lose, kept to be recovered: a crash of the program, of the system, or
//! the power going.
//!
//! Each run of the program is a session: a folder of its own in the recovery folder, with a
//! file it holds locked while it runs (the system lets the lock go however the program stops,
//! a crash or a power cut included) and a snapshot of each package with unsaved changes (see
//! [`crate::project::save_snapshot`]), written anew as the package changes, whole or not at all
//! (see [`crate::files::replace`]). A program that quits as it should leaves nothing of its
//! session. A program that starts finds the sessions whose lock nobody holds (their programs
//! stopped without quitting) and offers what they kept. The packages' own files are never
//! written: a package is saved when its user saves it, and a snapshot of it goes then. The
//! files a package holds (embedded) never change and are named by their bytes: a session keeps
//! each apart, written once, and its snapshots name them.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};


use crate::files;
use crate::project::{FILE_EXTENSION, Loaded, Unsaved};

/// The file a session's program holds locked while it runs.
const LOCK: &str = "lock";
/// Made first when a session ends as it should: whatever is left of it then is not work to
/// recover (a snapshot that was being written as the program quit).
const ENDED: &str = "ended";
/// The extension of a file a package held, kept apart (named by the hash of its bytes).
const HELD: &str = "held";

/// Seconds since 1970, now.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The session of this run of the program.
pub struct Session {
    dir: PathBuf,
    /// Held locked while the program runs; closed, the session looks stopped.
    lock: Option<File>,
}

impl Session {
    /// A session of its own in `root`, which is made if it is not there.
    pub fn start(root: &Path) -> Result<Session, String> {
        std::fs::create_dir_all(root).map_err(|e| format!("Cannot make '{}': {e}", root.display()))?;
        // A folder of its own: made, not found (of sessions started at once, each makes one).
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let mut n = 0;
        let dir = loop {
            let dir = root.join(format!("{stamp}-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
                Err(e) => return Err(format!("Cannot make '{}': {e}", dir.display())),
            }
        };
        let lock = File::create(dir.join(LOCK)).map_err(|e| format!("Cannot make '{}': {e}", dir.join(LOCK).display()))?;
        match lock.try_lock() {
            // (A system that locks no files: the session is found as stopped while it runs,
            // by a program started meanwhile — which then keeps what it found.)
            Ok(()) | Err(TryLockError::Error(_)) => {}
            Err(TryLockError::WouldBlock) => return Err(format!("'{}' is held by another program", dir.display())),
        }
        Ok(Session { dir, lock: Some(lock) })
    }

    /// The session's folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the snapshot of the package open as `key` is written.
    pub fn file(&self, key: u64) -> PathBuf {
        self.dir.join(format!("{key}.{FILE_EXTENSION}"))
    }

    /// Where a file a package holds, of `hash` (see [`crate::files::embedded_hash`]), is kept
    /// apart.
    pub fn held(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.{HELD}"))
    }

    /// Take the files packages held that `orphan` kept apart as this session's (copies, when
    /// `copy`: the orphan still needs them), the hashes of those it has now.
    pub fn adopt_held(&self, orphan: &Orphan, copy: bool) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&orphan.dir) else { return Vec::new() };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|f| is_held(f))
            .filter_map(|f| {
                let hash = f.file_stem()?.to_str()?.to_string();
                let to = self.held(&hash);
                let taken = to.exists()
                    || if copy {
                        files::replace(&to, |beside| std::fs::copy(&f, beside).map(drop)).is_ok()
                    } else {
                        std::fs::rename(&f, &to).is_ok()
                    };
                taken.then_some(hash)
            })
            .collect()
    }

    /// The package open as `key` has nothing to recover any more: it was saved, or closed
    /// with its changes discarded.
    pub fn forget(&self, key: u64) {
        let _ = std::fs::remove_file(self.file(key));
    }

    /// Take the snapshot `file` of a stopped session (see [`Orphan`]) as the snapshot of the
    /// package now open as `key`, which is what it holds.
    pub fn adopt(&self, file: &Path, key: u64) -> Result<(), String> {
        std::fs::rename(file, self.file(key)).map_err(|e| format!("Cannot move '{}': {e}", file.display()))
    }

    /// The program quits as it should: nothing of the session is left.
    pub fn end(mut self) {
        let _ = File::create(self.dir.join(ENDED)).and_then(|f| f.sync_all());
        drop(self.lock.take());
        clear(&self.dir);
    }
}

/// A session whose program stopped without quitting, held by this program now (no other
/// program started meanwhile offers it too), and the snapshots it kept.
pub struct Orphan {
    dir: PathBuf,
    _lock: File,
    pub snapshots: Vec<Snapshot>,
}

/// A snapshot of a package with unsaved changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Where it is.
    pub file: PathBuf,
    /// What it says of its package.
    pub unsaved: Unsaved,
    /// When the package's file was written last, if it is there, in seconds since 1970 (after
    /// the snapshot, what the file holds may be newer).
    pub saved: Option<u64>,
}

impl Orphan {
    /// Its folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The package `snapshot` holds, with the files it held (kept apart; each is taken only if
    /// it is the bytes its name says).
    pub fn load(&self, snapshot: &Snapshot) -> Result<Loaded, String> {
        let mut loaded = crate::project::load(&snapshot.file)?;
        for resource in &mut loaded.package.resources {
            let Some(hash) = files::embedded_hash(&resource.path).filter(|_| resource.data.is_none()) else { continue };
            let Ok(bytes) = std::fs::read(self.dir.join(format!("{hash}.{HELD}"))) else { continue };
            if files::holds(&resource.path, &bytes) {
                let bytes = Arc::new(bytes);
                files::keep(&resource.path, &bytes);
                resource.data = Some(bytes);
            }
        }
        Ok(loaded)
    }

    /// Nothing of it is wanted any more (recovered, or discarded): it goes.
    pub fn discard(self) {
        let Orphan { dir, _lock, .. } = self;
        drop(_lock);
        clear(&dir);
    }
}

/// The sessions in `root` whose programs stopped without quitting, but `own` (see the module),
/// the oldest first. Those that kept nothing go.
pub fn orphans(root: &Path, own: Option<&Path>) -> Vec<Orphan> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found = Vec::new();
    for dir in entries.flatten().map(|e| e.path()) {
        if Some(dir.as_path()) == own || !is_session(&dir) {
            continue;
        }
        let Ok(lock) = File::options().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK)) else {
            continue;
        };
        match lock.try_lock() {
            Ok(()) | Err(TryLockError::Error(_)) => {}
            // Its program runs.
            Err(TryLockError::WouldBlock) => continue,
        }
        let mut snapshots = if dir.join(ENDED).exists() { Vec::new() } else { snapshots(&dir) };
        if snapshots.is_empty() {
            drop(lock);
            clear(&dir);
            continue;
        }
        snapshots.sort_by_key(|s| s.unsaved.time);
        found.push(Orphan { dir, _lock: lock, snapshots });
    }
    found.sort_by_key(|o| o.snapshots.first().map_or(0, |s| s.unsaved.time));
    found
}

/// Is `dir` the folder of a session: `<milliseconds>-<process>-<number>`?
fn is_session(dir: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let parts: Vec<&str> = name.split('-').collect();
    dir.is_dir() && parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
}

/// Is `file` a file a package held, kept apart: `<hash>.held`?
fn is_held(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == HELD)
        && file.file_stem().and_then(|s| s.to_str()).is_some_and(files::is_hash)
}

/// Is `file` a snapshot of a session: `<key>.brewtex`?
fn is_snapshot(file: &Path) -> bool {
    file.extension().is_some_and(|e| e == FILE_EXTENSION)
        && file.file_stem().and_then(|s| s.to_str()).is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
}

/// The snapshots in `dir` that can be read, with what they say of their packages (read
/// without holding a package in memory).
fn snapshots(dir: &Path) -> Vec<Snapshot> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|f| is_snapshot(f))
        .filter_map(|file| {
            let unsaved = crate::project::read_unsaved(&file)?;
            let saved = unsaved.path.as_ref().and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
            let saved = saved.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
            Some(Snapshot { file, unsaved, saved })
        })
        .collect()
}

/// Take away a session's folder: what a session makes there (its lock, its snapshots, the files
/// packages held, and the files they are written through), then the folder, if nothing else is
/// in it. The mark that it ended goes last, with the folder: a file still being written (it
/// was as the program quit) leaves the folder, marked, for the next program to take away.
fn clear(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for file in entries.flatten().map(|e| e.path()) {
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name == LOCK || is_snapshot(&file) || is_held(&file) || (name.starts_with('.') && name.ends_with(".tmp")) {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    let only_the_mark = std::fs::read_dir(dir).is_ok_and(|mut e| e.all(|f| f.is_ok_and(|f| f.file_name() == ENDED)));
    if only_the_mark {
        let _ = std::fs::remove_file(dir.join(ENDED));
    }
    let _ = std::fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("brewed-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn unsaved(title: &str, time: u64) -> Unsaved {
        Unsaved { path: Some(format!("C:/work/{title}")), title: title.into(), time }
    }

    fn snapshot(session: &Session, key: u64, u: &Unsaved) {
        let package = crate::package::Package::new("Wall", crate::graph::Graph::default());
        crate::project::save_snapshot(&session.file(key), &package, &Default::default(), package.first(), u).unwrap();
    }

    /// A session that stops without ending (a crash: its lock goes with it) is found, with
    /// its snapshots and what they say; while its program runs, or once it ended as it should,
    /// it is not. What a session did not make is never taken away.
    #[test]
    fn stopped_sessions_are_found_and_running_ones_are_not() {
        let root = root("found");
        let running = Session::start(&root).unwrap();
        snapshot(&running, 3, &unsaved("walls.brewtex", 100));
        let crashed = Session::start(&root).unwrap();
        snapshot(&crashed, 7, &unsaved("floor.brewtex", 200));
        snapshot(&crashed, 8, &unsaved("roof.brewtex", 150));
        let crashed_dir = crashed.dir().to_path_buf();
        // A folder of the user's own, and a session's folder with a file of the user's in it.
        std::fs::create_dir_all(root.join("mine")).unwrap();
        std::fs::write(root.join("mine").join("notes.txt"), b"keep").unwrap();
        drop(crashed);

        let me = Session::start(&root).unwrap();
        let found = orphans(&root, Some(me.dir()));
        assert_eq!(found.len(), 1, "the running one is not found");
        assert_eq!(found[0].dir(), crashed_dir);
        let titles: Vec<&str> = found[0].snapshots.iter().map(|s| s.unsaved.title.as_str()).collect();
        assert_eq!(titles, ["roof.brewtex", "floor.brewtex"], "the oldest first");
        // Held by this program now: a program started meanwhile does not find it too.
        assert!(orphans(&root, None).iter().all(|o| o.dir() != crashed_dir));
        // A snapshot is a package file as any other.
        let loaded = crate::project::load(&found[0].snapshots[0].file).unwrap();
        assert_eq!(loaded.unsaved, Some(unsaved("roof.brewtex", 150)));
        assert_eq!(loaded.package.graphs[0].name, "Wall");
        for orphan in found {
            orphan.discard();
        }
        assert!(!crashed_dir.exists());
        assert!(root.join("mine").join("notes.txt").exists());

        // Ended as it should, nothing of it is left, nor offered.
        let ended_dir = running.dir().to_path_buf();
        running.end();
        assert!(!ended_dir.exists());
        assert!(orphans(&root, Some(me.dir())).is_empty());
        me.end();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The files a package holds are kept apart, not in its snapshots (written once, whatever
    /// the package does), and recovered with it, as the bytes their names say.
    #[test]
    fn held_files_are_kept_apart_and_recovered() {
        use crate::package::{Resource, ResourceKind};
        let root = root("held");
        let session = Session::start(&root).unwrap();
        // Bytes nothing in this run holds yet.
        let bytes = Arc::new(format!("bricks of {} at {:?}", std::process::id(), SystemTime::now()).into_bytes());
        let path = files::embedded_path("bricks.png", &bytes);
        let hash = files::embedded_hash(&path).unwrap().to_string();
        let mut package = crate::package::Package::new("Wall", crate::graph::Graph::default());
        package.resources.push(Resource { path: path.clone(), kind: ResourceKind::Bitmap, data: Some(bytes.clone()), extras: Default::default() });
        files::write_whole(&session.held(&hash), &bytes).unwrap();
        // Another held file that is not what its name says is not taken.
        let forged = files::embedded_path("forged.png", b"what it was");
        package.resources.push(Resource::linked(forged.clone(), ResourceKind::Bitmap));
        files::write_whole(&session.held(files::embedded_hash(&forged).unwrap()), b"something else").unwrap();
        let snapshot = session.file(1);
        crate::project::save_snapshot(&snapshot, &package, &Default::default(), package.first(), &unsaved("w.brewtex", 1)).unwrap();
        let held: Vec<String> = crate::archive::Archive::new(File::open(&snapshot).unwrap()).unwrap().entries().iter().map(|e| e.name.clone()).collect();
        assert!(!held.iter().any(|n| n.starts_with("files/")), "not in the snapshot: {held:?}");
        drop(session);

        let me = Session::start(&root).unwrap();
        let orphan = orphans(&root, Some(me.dir())).pop().unwrap();
        let loaded = orphan.load(&orphan.snapshots[0]).unwrap();
        let held = |p: &str| loaded.package.resources.iter().find(|r| r.path == p).and_then(|r| r.data.clone());
        assert_eq!(held(&path).as_deref(), Some(&*bytes));
        assert_eq!(files::read(&path).unwrap(), bytes, "and read where it is named");
        assert_eq!(held(&forged), None);
        let mut adopted = me.adopt_held(&orphan, false);
        adopted.sort();
        assert!(adopted.contains(&hash) && me.held(&hash).exists());
        orphan.discard();
        me.end();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A snapshot taken over by a program that recovered it is the snapshot of the package
    /// it opened; one of a session that ended as it quit (written as it did) is not offered.
    #[test]
    fn snapshots_are_adopted_and_ended_sessions_offer_nothing() {
        let root = root("adopt");
        let crashed = Session::start(&root).unwrap();
        snapshot(&crashed, 1, &unsaved("a.brewtex", 5));
        drop(crashed);
        let me = Session::start(&root).unwrap();
        let orphan = orphans(&root, Some(me.dir())).pop().unwrap();
        me.adopt(&orphan.snapshots[0].file, 42).unwrap();
        assert!(me.file(42).exists());
        orphan.discard();
        me.forget(42);
        assert!(!me.file(42).exists());

        // Ending while a snapshot is still written (it lands after the session ended).
        let quitting = Session::start(&root).unwrap();
        let dir = quitting.dir().to_path_buf();
        let late = quitting.file(9);
        std::fs::write(dir.join(ENDED), b"").unwrap();
        drop(quitting);
        std::fs::write(&late, b"{}").unwrap();
        assert!(orphans(&root, Some(me.dir())).is_empty());
        assert!(!dir.exists());
        me.end();
        let _ = std::fs::remove_dir_all(&root);
    }
}
