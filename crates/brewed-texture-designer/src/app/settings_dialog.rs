//! The settings dialog (Edit → Settings…): the settings are changed in a draft, a page at a
//! time, and taken only when Apply is pressed; Cancel forgets the draft. A setting the program
//! takes only when it starts offers, once applied, to start it again.

use std::path::PathBuf;

use iced::Task;

use super::{App, Dialog, Message, PendingAction};
use crate::settings::{Page, Setting, Settings};
use crate::shortcuts::{Chord, Command, Shortcuts};

/// The settings dialog, open.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDialog {
    /// The page shown.
    pub page: Page,
    /// The settings as they are being changed (those in use are [`App::settings`]).
    pub draft: Settings,
    /// A command waiting for the key to give it, and which of its keys that is (the one after
    /// the last: a key more).
    pub capturing: Option<(Command, usize)>,
    /// What giving the last key did to the command that had it.
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SettingsMessage {
    /// Open the dialog, on the page shown last.
    Open,
    /// Open the dialog on a page.
    OpenPage(Page),
    Page(Page),
    Set(Setting),
    /// Every setting of the page shown as it is by default.
    ResetPage,
    /// Wait for the key to give a command, in place of one of its keys or as a key more.
    Capture(Command, usize),
    /// Stop waiting for a key.
    StopCapture,
    /// Take one of the keys of a command away.
    ClearKey(Command, usize),
    /// Give a command its keys by default.
    ResetKeys(Command),
    /// Choose a folder for the Library (a dialog of the system), and the one chosen.
    AddLibraryFolder,
    LibraryFolderPicked(Option<PathBuf>),
    RemoveLibraryFolder(usize),
    Apply,
    /// Forget the draft, and close.
    Cancel,
    /// Start the program again, for the settings it takes only then (the offer after Apply).
    Restart,
    /// Not now: the offer goes.
    RestartLater,
}

impl App {
    pub(super) fn settings_message(&mut self, message: SettingsMessage) -> Task<Message> {
        match message {
            SettingsMessage::OpenPage(page) => {
                self.ui.settings_page = page;
                return self.settings_message(SettingsMessage::Open);
            }
            SettingsMessage::Open => {
                self.close_popups();
                let dialog = SettingsDialog { page: self.ui.settings_page, draft: self.settings.clone(), capturing: None, note: None };
                self.ui.dialog = Some(Dialog::Settings(Box::new(dialog)));
                return Task::none();
            }
            SettingsMessage::Restart => {
                self.ui.restart_offer = false;
                self.ui.restarting = true;
                self.ui.discarded.clear();
                return self.guarded(PendingAction::Quit);
            }
            SettingsMessage::RestartLater => {
                self.ui.restart_offer = false;
                return Task::none();
            }
            _ => {}
        }
        let Some(Dialog::Settings(dialog)) = &mut self.ui.dialog else { return Task::none() };
        match message {
            SettingsMessage::Open | SettingsMessage::OpenPage(_) | SettingsMessage::Restart | SettingsMessage::RestartLater => {}
            SettingsMessage::Page(page) => {
                dialog.page = page;
                dialog.capturing = None;
                dialog.note = None;
                self.ui.settings_page = page;
            }
            SettingsMessage::Set(setting) => dialog.draft.set(setting),
            SettingsMessage::ResetPage => {
                dialog.draft = dialog.draft.reset(dialog.page);
                dialog.capturing = None;
                dialog.note = None;
            }
            SettingsMessage::Capture(command, slot) => {
                dialog.capturing = Some((command, slot));
                dialog.note = None;
            }
            SettingsMessage::StopCapture => dialog.capturing = None,
            SettingsMessage::ClearKey(command, slot) => {
                let mut keys = dialog.draft.shortcuts.of(command);
                if slot < keys.len() {
                    keys.remove(slot);
                }
                dialog.draft.shortcuts.set(command, keys);
                dialog.capturing = None;
                dialog.note = None;
            }
            SettingsMessage::ResetKeys(command) => {
                let taken = dialog.draft.shortcuts.set(command, Shortcuts::defaults(command));
                dialog.note = taken_from(&dialog.draft.shortcuts, &taken);
                dialog.capturing = None;
            }
            SettingsMessage::AddLibraryFolder => {
                let folder = dialog.draft.library_folders.last().map(PathBuf::from);
                return Task::perform(
                    super::blocking(move || {
                        let mut picker = rfd::FileDialog::new().set_title("A folder of packages for the Library");
                        if let Some(folder) = folder {
                            picker = picker.set_directory(folder);
                        }
                        picker.pick_folder()
                    }),
                    |picked| Message::Settings(SettingsMessage::LibraryFolderPicked(picked.flatten())),
                );
            }
            SettingsMessage::LibraryFolderPicked(Some(folder)) => {
                let folder = folder.display().to_string();
                if !dialog.draft.library_folders.contains(&folder) {
                    dialog.draft.library_folders.push(folder);
                }
            }
            SettingsMessage::LibraryFolderPicked(None) => {}
            SettingsMessage::RemoveLibraryFolder(i) => {
                if i < dialog.draft.library_folders.len() {
                    dialog.draft.library_folders.remove(i);
                }
            }
            SettingsMessage::Apply => {
                dialog.capturing = None;
                let draft = dialog.draft.clone();
                let looked = draft.library_folders != self.settings.library_folders;
                self.set_settings(draft);
                if looked {
                    return self.look_at_library();
                }
            }
            SettingsMessage::Cancel => self.ui.dialog = None,
        }
        Task::none()
    }

    /// A key pressed while the settings dialog waits for one (see
    /// [`SettingsDialog::capturing`]): the command has it now, and the command that had it
    /// does not.
    pub(super) fn capture_key(&mut self, chord: Chord) {
        let Some(Dialog::Settings(dialog)) = &mut self.ui.dialog else { return };
        let Some((command, slot)) = dialog.capturing.take() else { return };
        let mut keys = dialog.draft.shortcuts.of(command);
        if keys.contains(&chord) {
            dialog.note = None;
            return;
        }
        match keys.get_mut(slot) {
            Some(key) => *key = chord,
            None => keys.push(chord),
        }
        let taken = dialog.draft.shortcuts.set(command, keys);
        dialog.note = taken_from(&dialog.draft.shortcuts, &taken);
    }

    /// Is the settings dialog waiting for a key?
    pub(super) fn capturing_key(&self) -> bool {
        matches!(&self.ui.dialog, Some(Dialog::Settings(dialog)) if dialog.capturing.is_some())
    }

    /// The settings are `new`: kept, and what they change changed.
    pub(crate) fn set_settings(&mut self, new: Settings) {
        let old = std::mem::replace(&mut self.settings, new.clone());
        if old == new {
            return;
        }
        if old.keep_unsaved && !new.keep_unsaved {
            // Not kept any more: what was kept goes.
            self.forget_all_snapshots();
        }
        if old.undo_steps != new.undo_steps {
            let steps = new.undo_steps as usize;
            self.doc.set_undo_steps(steps);
            for open in &mut self.shelf {
                open.doc.set_undo_steps(steps);
            }
        }
        if old.needs_restart(&new) {
            // Offered while they are not those the program started with.
            self.ui.restart_offer = new.needs_restart(&self.started_with());
        }
        if let Err(e) = self.settings_store.save(&new) {
            self.set_status(e, true);
        }
    }

    /// The settings the program started with: those it takes only when it starts are these.
    pub fn started_with(&self) -> Settings {
        self.opts.settings.settings()
    }

    /// The graphics API the environment gives the program (`WGPU_BACKEND`), which comes before
    /// the settings'; not the one the settings gave it.
    pub fn graphics_from_environment(&self) -> Option<String> {
        crate::settings::graphics_from_environment().filter(|_| !self.opts.graphics_from_settings)
    }

    /// The settings file, and what is wrong with it (the settings dialog says it).
    pub fn settings_file(&self) -> (Option<&std::path::Path>, &[String]) {
        (self.settings_store.path(), &self.settings_store.problems)
    }

    /// Quitting to start again (see [`SettingsMessage::Restart`]), the program starts again:
    /// with the packages open now that have files, the one being edited last (to be edited
    /// again), and the environment it was started with.
    pub(super) fn start_again(&mut self) {
        let mut files: Vec<PathBuf> = self.shelf.iter().filter_map(|p| p.doc.path().cloned()).collect();
        files.extend(self.doc.path().cloned());
        log::info!("starting again, with {} package(s)", files.len());
        #[cfg(test)]
        {
            self.ui.started_again = Some(files);
        }
        #[cfg(not(test))]
        {
            #[cfg(feature = "dev-tools")]
            if self.opts.dev.autopilot.is_some() {
                return;
            }
            let started = std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| {
                let mut command = std::process::Command::new(exe);
                command.args(&files);
                if self.opts.graphics_from_settings {
                    // The settings' choice now, not the one they made when this one started.
                    command.env_remove("WGPU_BACKEND");
                }
                command.spawn().map(drop).map_err(|e| e.to_string())
            });
            if let Err(e) = started {
                log::error!("the program could not start again: {e}");
            }
        }
    }
}

/// What giving a key took from other commands, said: which command had it, and what keys it
/// has now.
fn taken_from(shortcuts: &Shortcuts, taken: &[(Command, Chord)]) -> Option<String> {
    let said: Vec<String> = taken
        .iter()
        .map(|(other, chord)| {
            let left: Vec<String> = shortcuts.of(*other).iter().map(Chord::shown).collect();
            let left = if left.is_empty() { "no key now".to_string() } else { format!("{} now", left.join(", ")) };
            format!("{} was the key of \"{}\", which has {left}.", chord.shown(), other.label())
        })
        .collect();
    (!said.is_empty()).then(|| said.join(" "))
}
