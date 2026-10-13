//! Earlier versions of the packages, kept as they are saved (see
//! `brewed_texture_core::versions`): beside the settings, never beside the packages. File →
//! Package History… lists those of the package being edited; one is restored in its place, not
//! saved yet (its unsaved changes are kept as a version first: nothing is lost), or opened as a
//! copy beside it.

use std::path::PathBuf;
use std::sync::Arc;

use brewed_texture_core::project::Loaded;
use brewed_texture_core::versions::{Kind, Version, Versions};
use iced::Task;

use super::{App, Dialog, Message};

/// The history dialog, open: the versions of the package open as `key`, saved as `path`.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionsDialog {
    pub key: u64,
    pub path: PathBuf,
    pub list: Vec<Version>,
}

#[derive(Clone, Debug)]
pub enum VersionsMessage {
    Open,
    /// Put the version kept as this file in place of the package, not saved yet.
    Restore(PathBuf),
    /// Open the version kept as this file beside the package, untitled.
    Copy(PathBuf),
    /// A version was read: to be restored in place of the package open as `key` (saved as
    /// `path`), as it was when it was asked for (`revision`), or opened beside it; when it was
    /// kept.
    Read {
        key: u64,
        path: PathBuf,
        restore: bool,
        revision: Option<u64>,
        time: u64,
        result: Result<Arc<Loaded>, String>,
    },
}

impl App {
    /// Where the versions are kept, and how many of each package (see the settings); none when
    /// they are not kept anywhere (tests, a system without a folder for settings).
    pub(crate) fn versions_kept(&self) -> Option<(Versions, usize)> {
        Some((self.versions.clone()?, self.settings.keep_versions as usize))
    }

    pub(crate) fn versions_message(&mut self, message: VersionsMessage) -> Task<Message> {
        match message {
            VersionsMessage::Open => {
                self.close_popups();
                let (Some(versions), Some(path)) = (&self.versions, self.doc.path()) else {
                    self.set_status("Earlier versions are kept of packages saved as files: save it first", true);
                    return Task::none();
                };
                let list = versions.of(path);
                self.ui.dialog = Some(Dialog::Versions(VersionsDialog { key: self.doc_key, path: path.clone(), list }));
                Task::none()
            }
            VersionsMessage::Restore(ref file) | VersionsMessage::Copy(ref file) => {
                let restore = matches!(message, VersionsMessage::Restore(_));
                let Some(Dialog::Versions(dialog)) = &self.ui.dialog else { return Task::none() };
                let Some(version) = dialog.list.iter().find(|v| v.file == *file).cloned() else { return Task::none() };
                let (key, path) = (dialog.key, dialog.path.clone());
                let Some((versions, keep)) = self.versions_kept() else { return Task::none() };
                self.ui.dialog = None;
                // The unsaved changes the version takes the place of: kept as a version too, once
                // it is read (keeping it may let the oldest go), even where none are kept.
                let replaced = self.package_doc(key).filter(|d| restore && d.is_dirty()).map(|d| (d.package().clone(), self.views_of(key), d.current()));
                // (Changed while the version is read, the package is not replaced: what changed
                // was not kept.)
                let revision = self.package_doc(key).map(|d| d.revision());
                // Read as a package being opened is: the file is not opened again meanwhile, and
                // what waits for files to be read waits for it.
                self.ui.opening.insert(super::packages::clean(&path));
                self.set_status("Reading the version…", false);
                let (to, time) = (path.clone(), version.time);
                self.in_background(
                    move || -> Result<Arc<Loaded>, String> {
                        let loaded = versions.load(&version)?;
                        if let Some((package, views, open)) = &replaced {
                            versions
                                .keep(&to, package, views, *open, Kind::Replaced, keep.max(1))
                                .map_err(|e| format!("Nothing was restored: the unsaved changes could not be kept first ({e})"))?;
                        }
                        Ok(Arc::new(loaded))
                    },
                    move |done| {
                        let result = done.unwrap_or_else(|| Err("The version was not read: what read it stopped".into()));
                        Message::Versions(VersionsMessage::Read { key, path: path.clone(), restore, revision, time, result })
                    },
                )
            }
            VersionsMessage::Read { key, path, restore, revision, time, result } => {
                self.ui.opening.remove(&super::packages::clean(&path));
                if restore && revision.is_some() && self.package_doc(key).map(|d| d.revision()) != revision {
                    self.set_status("Nothing was restored: the package changed while the version was read. Restore it again to put it in its place.", true);
                    return Task::none();
                }
                let loaded = match result.map(Arc::try_unwrap) {
                    Ok(Ok(loaded)) => loaded,
                    Ok(Err(_)) => return Task::none(),
                    Err(e) => {
                        self.set_status(e, true);
                        return Task::none();
                    }
                };
                // Within a sentence: "today, 14:32:05".
                let said = crate::clock::said(time);
                let mut letters = said.chars();
                let when: String = letters.next().map(|first| first.to_lowercase().chain(letters).collect()).unwrap_or_default();
                if restore {
                    // In place of the package, where it is open still; else open again, as
                    // its file.
                    match self.package_doc(key) {
                        Some(_) => {
                            self.activate(key, None);
                            self.replace_package(loaded.package, Some(path), loaded.views, loaded.open);
                        }
                        None => self.add_package(loaded.package, Some(path), loaded.views, loaded.open),
                    }
                    self.set_status(format!("Restored the version of {when}: save it to keep it"), false);
                } else {
                    self.add_package(loaded.package, None, loaded.views, loaded.open);
                    self.set_status(format!("Opened a copy of the version of {when}"), false);
                }
                self.doc.mark_unsaved();
                self.refresh();
                Task::none()
            }
        }
    }
}
