//! Work kept for recovery after a crash (see `brewed_texture_core::recovery`): as often as the
//! settings say, the packages whose unsaved changes changed since their last snapshot are
//! written as snapshots, in the background (the files a package holds apart from it, once each); a
//! package saved or closed loses its snapshot, and quitting as the program should leaves
//! nothing. Starting, the program offers what the sessions of
//! programs that stopped without quitting kept: each package with its unsaved changes, where it
//! was saved.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use brewed_texture_core::document::Document;
use brewed_texture_core::instance::GraphId;
use brewed_texture_core::package::Package;
use brewed_texture_core::project::{self, Unsaved, ViewState};
use brewed_texture_core::recovery::{self, Orphan, Session};
use iced::Task;

use super::{App, Dialog, Message, blocking};

/// What is kept for recovery, and what was found of programs that stopped.
#[derive(Default)]
pub struct Autosave {
    /// This run's session: none when nothing can be kept (no folder for settings, or it cannot
    /// be written).
    pub session: Option<Session>,
    /// The revision of each package whose snapshot is written, by its key in the Explorer.
    written: HashMap<u64, u64>,
    /// The files packages hold that are kept apart, by their hashes (see
    /// `brewed_texture_core::files::embedded_hash`).
    held: HashSet<String>,
    /// Snapshots are being written.
    busy: bool,
    /// Writing failed the last time (said once, until it works again).
    failing: bool,
    /// The sessions of programs that stopped without quitting, offered to be recovered.
    pub found: Vec<Orphan>,
}

/// What becomes of what programs that stopped kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// Open it: each package with its unsaved changes, where it was saved.
    Recover,
    /// Offer it again the next time the program starts.
    Later,
    /// It goes.
    Discard,
}

/// A snapshot to write.
pub(super) struct Job {
    key: u64,
    revision: u64,
    package: Package,
    views: HashMap<GraphId, ViewState>,
    open: GraphId,
    unsaved: Unsaved,
    file: PathBuf,
    /// The files its package holds that are not kept apart yet: their hashes, bytes, and where
    /// they go.
    held: Vec<(String, Arc<Vec<u8>>, PathBuf)>,
}

/// A snapshot written, or not: of the package open as which key, as it was at which revision,
/// and the hashes of the files it held that are kept apart now (else why it was not written).
pub type Written = (u64, u64, Result<Vec<String>, String>);

impl Job {
    /// Write it: the files its package holds first (a snapshot names them), then itself. The
    /// files `kept` are kept apart already (by the jobs before it).
    fn write(self, kept: &mut HashSet<String>) -> Written {
        let mut held = Vec::new();
        for (hash, bytes, file) in &self.held {
            if !kept.contains(hash) {
                if let Err(e) = brewed_texture_core::files::write_whole(file, bytes) {
                    return (self.key, self.revision, Err(e));
                }
                kept.insert(hash.clone());
            }
            held.push(hash.clone());
        }
        let written = project::save_snapshot(&self.file, &self.package, &self.views, self.open, &self.unsaved);
        (self.key, self.revision, written.map(|()| held))
    }
}

/// Write `jobs`, one after the other.
fn write_all(jobs: Vec<Job>) -> Vec<Written> {
    let mut kept = HashSet::new();
    jobs.into_iter().map(|job| job.write(&mut kept)).collect()
}

impl App {
    /// Keep work for recovery in `root`, and offer what programs that stopped kept there.
    pub(super) fn start_autosave(&mut self, root: &Path) {
        match Session::start(root) {
            Ok(session) => {
                self.autosave.found = recovery::orphans(root, Some(session.dir()));
                self.autosave.session = Some(session);
                if !self.autosave.found.is_empty() {
                    self.ui.dialog = Some(Dialog::Recover);
                }
            }
            Err(e) => self.set_status(format!("Unsaved work is not kept for recovery: {e}"), true),
        }
    }

    /// Write the snapshots of the packages whose unsaved changes changed since their last one,
    /// in the background.
    pub(super) fn autosave(&mut self) -> Task<Message> {
        let jobs = self.autosave_jobs();
        if jobs.is_empty() {
            return Task::none();
        }
        self.autosave.busy = true;
        Task::perform(blocking(move || write_all(jobs)), |done| Message::Autosaved(done.unwrap_or_default()))
    }

    /// [`App::autosave`], written before it returns.
    #[cfg(test)]
    pub(crate) fn autosave_now(&mut self) {
        let jobs = self.autosave_jobs();
        self.autosave.busy = !jobs.is_empty();
        let done = write_all(jobs);
        self.autosaved(done);
    }

    fn autosave_jobs(&mut self) -> Vec<Job> {
        self.forget_saved();
        let Some(session) = &self.autosave.session else { return Vec::new() };
        // (Not kept, as the settings may say: a tick on its way when they said so writes
        // nothing either.)
        if self.autosave.busy || !self.settings.keep_unsaved {
            return Vec::new();
        }
        self.packages()
            .into_iter()
            .filter(|(key, doc)| doc.is_dirty() && self.autosave.written.get(key) != Some(&doc.revision()))
            .map(|(key, doc)| Job {
                key,
                revision: doc.revision(),
                package: doc.package().clone(),
                views: self.views_of(key),
                open: doc.current(),
                unsaved: Unsaved { path: doc.key(), title: title(doc), time: recovery::now() },
                file: session.file(key),
                held: doc
                    .package()
                    .resources
                    .iter()
                    .filter_map(|r| {
                        let hash = brewed_texture_core::files::embedded_hash(&r.path)?;
                        let bytes = r.data.clone()?;
                        (!self.autosave.held.contains(hash)).then(|| (hash.to_string(), bytes, session.held(hash)))
                    })
                    .collect(),
            })
            .collect()
    }

    /// Snapshots were written, or could not be (see [`Written`]).
    pub(super) fn autosaved(&mut self, done: Vec<Written>) {
        self.autosave.busy = false;
        for (key, revision, written) in done {
            match written {
                Ok(held) => {
                    self.autosave.written.insert(key, revision);
                    self.autosave.held.extend(held);
                    self.autosave.failing = false;
                }
                Err(e) => {
                    if !std::mem::replace(&mut self.autosave.failing, true) {
                        self.set_status(format!("Unsaved work could not be kept for recovery: {e}"), true);
                    }
                }
            }
        }
        // Saved or closed meanwhile.
        self.forget_saved();
    }

    /// The snapshots of the packages that have nothing to recover any more (saved, or closed)
    /// go. Not while snapshots are being written: one of them could land after it went (it goes
    /// once they are written).
    pub(super) fn forget_saved(&mut self) {
        let Some(session) = &self.autosave.session else { return };
        if self.autosave.busy {
            return;
        }
        let unsaved: HashSet<u64> = self.packages().into_iter().filter(|(_, d)| d.is_dirty()).map(|(k, _)| k).collect();
        self.autosave.written.retain(|key, _| {
            let keep = unsaved.contains(key);
            if !keep {
                session.forget(*key);
            }
            keep
        });
    }

    /// What the user chose for what programs that stopped kept.
    pub(super) fn recovery(&mut self, choice: Recovery) {
        if self.ui.dialog == Some(Dialog::Recover) {
            self.ui.dialog = None;
        }
        let found = std::mem::take(&mut self.autosave.found);
        match choice {
            // Let go of: found again the next time.
            Recovery::Later => drop(found),
            Recovery::Discard => found.into_iter().for_each(Orphan::discard),
            Recovery::Recover => self.recover(found),
        }
    }

    /// Open what `found` kept: each package with its unsaved changes, where it was saved (in
    /// place of the same package open from its file, unchanged), its snapshot this session's.
    fn recover(&mut self, found: Vec<Orphan>) {
        let mut recovered = Vec::new();
        let mut failed = Vec::new();
        for orphan in found {
            let mut whole = true;
            for snapshot in &orphan.snapshots {
                let loaded = match orphan.load(snapshot) {
                    Ok(loaded) => loaded,
                    Err(e) => {
                        whole = false;
                        failed.push(format!("{}: {e}", snapshot.unsaved.title));
                        continue;
                    }
                };
                // Its file saved after the snapshot holds what is newer: the package opens
                // untitled, not to be saved over it unasked.
                let newer = snapshot.saved.is_some_and(|saved| saved > snapshot.unsaved.time);
                let path = snapshot.unsaved.path.as_ref().filter(|_| !newer).map(PathBuf::from);
                let same = path.as_ref().and_then(|p| self.open_key(&p.display().to_string())).filter(|k| self.package_doc(*k).is_some_and(|d| !d.is_dirty()));
                self.add_package(loaded.package, path, loaded.views, loaded.open);
                if let Some(same) = same.filter(|k| *k != self.doc_key) {
                    self.shelf.retain(|p| p.key != same);
                }
                self.doc.mark_unsaved();
                if let Some(session) = &self.autosave.session
                    && session.adopt(&snapshot.file, self.doc_key).is_ok()
                {
                    self.autosave.written.insert(self.doc_key, self.doc.revision());
                }
                recovered.push(snapshot.unsaved.title.clone());
            }
            // The files the recovered packages held are this session's too; what could not be
            // read is offered again next time (with its own).
            if let Some(session) = &self.autosave.session {
                let held = session.adopt_held(&orphan, !whole);
                self.autosave.held.extend(held);
            }
            if whole {
                orphan.discard();
            }
        }
        self.refresh();
        // What is recovered is what is edited now: off the Home screen, to it (left on the Home
        // screen, the work recovered waited behind its button).
        if !recovered.is_empty() {
            self.ui.home = None;
        }
        match (recovered.len(), failed.is_empty()) {
            (_, false) => self.set_status(format!("Could not recover {}", failed.join("; ")), true),
            (0, true) => {}
            (1, true) => self.set_status(format!("Recovered {}: save it to keep it", recovered[0]), false),
            (n, true) => self.set_status(format!("Recovered {n} packages: save them to keep them"), false),
        }
    }

    /// What programs that stopped without quitting kept, offered to be recovered.
    pub fn recoverable(&self) -> impl Iterator<Item = &recovery::Snapshot> {
        self.autosave.found.iter().flat_map(|o| &o.snapshots)
    }

    /// Unsaved work is not kept any more (the settings say so): what was kept of this
    /// session's packages goes.
    pub(super) fn forget_all_snapshots(&mut self) {
        let Some(session) = &self.autosave.session else { return };
        for key in std::mem::take(&mut self.autosave.written).into_keys() {
            session.forget(key);
        }
    }

    /// The program quits as it should: nothing is left to recover.
    pub(super) fn end_autosave(&mut self) {
        if let Some(session) = self.autosave.session.take() {
            session.end();
        }
        self.autosave.found.clear();
    }
}

/// What a package is called in the list of what can be recovered: its file's name, or the
/// name of the graph it has first.
fn title(doc: &Document) -> String {
    match doc.path() {
        Some(_) => doc.title(),
        None => doc.package().graphs.first().map_or_else(|| doc.title(), |g| g.name.clone()),
    }
}
