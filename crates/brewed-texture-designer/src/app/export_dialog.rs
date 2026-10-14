//! The export dialog (File → Export…, Ctrl+E): what is written of the graph being edited (its
//! Outputs as they are, or the files a game engine expects of a material, see
//! [`brewed_texture_core::export_presets`]), in what, and where; the files listed before they
//! are written.

use std::path::{Path, PathBuf};

use brewed_texture_core::NodeKind;
use brewed_texture_core::export_presets::{self, Planned, PlannedFile, Preset, Source};
use brewed_texture_core::nodes::DEPTH_8;
use brewed_texture_engine::export::{self, Packed};
use brewed_texture_engine::{ExportJob, Request};
use iced::Task;

use super::{App, Dialog, Message, blocking};

/// The export dialog, open.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDialog {
    pub preset: Preset,
    /// The graph's normal maps are DirectX's (Y−), not OpenGL's.
    pub directx: bool,
    /// What the files of a preset are written as (see `export::DEPTH_8`); the Outputs as they
    /// are, as each says.
    pub depth: u32,
    pub folder: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum ExportMessage {
    Open,
    Preset(Preset),
    DirectX(bool),
    Depth(u32),
    /// Choose the folder (a dialog of the system), and the one chosen.
    Browse,
    Folder(Option<PathBuf>),
    Export,
}

impl App {
    pub(crate) fn export_message(&mut self, message: ExportMessage) -> Task<Message> {
        if let ExportMessage::Open = message {
            self.close_popups();
            let dialog = ExportDialog {
                preset: self.ui.export_preset,
                directx: export_presets::graph_normals_directx(self.graph()),
                depth: DEPTH_8,
                folder: self.ui.export_folder.clone().or_else(|| self.doc.path().and_then(|p| p.parent()).map(Path::to_path_buf)),
            };
            self.ui.dialog = Some(Dialog::Export(dialog));
            return Task::none();
        }
        let Some(Dialog::Export(dialog)) = &mut self.ui.dialog else { return Task::none() };
        match message {
            ExportMessage::Open => {}
            ExportMessage::Preset(preset) => dialog.preset = preset,
            ExportMessage::DirectX(directx) => dialog.directx = directx,
            ExportMessage::Depth(depth) => dialog.depth = depth,
            ExportMessage::Browse => {
                // Where the folder is, or where the settings open file dialogs.
                let folder = dialog.folder.clone();
                let folder = folder.or_else(|| self.dialog_folder(None, false));
                return Task::perform(
                    blocking(move || {
                        let mut picker = rfd::FileDialog::new().set_title("Export to folder");
                        if let Some(folder) = folder {
                            picker = picker.set_directory(folder);
                        }
                        picker.pick_folder()
                    }),
                    |picked| Message::Export(ExportMessage::Folder(picked.flatten())),
                );
            }
            ExportMessage::Folder(Some(folder)) => dialog.folder = Some(folder),
            ExportMessage::Folder(None) => {}
            ExportMessage::Export => {
                let dialog = dialog.clone();
                let Some(folder) = dialog.folder.clone() else { return Task::none() };
                self.ui.dialog = None;
                self.ui.export_preset = dialog.preset;
                self.ui.export_folder = Some(folder.clone());
                let jobs = self.export_dialog_jobs(&dialog, &folder);
                if jobs.is_empty() {
                    self.set_status("Nothing to export: the graph has no Output of what the preset writes", true);
                    return Task::none();
                }
                self.res.exports_pending += jobs.len();
                self.set_status(format!("Exporting {} file(s) to {}…", jobs.len(), folder.display()), false);
                self.send(Request::Export(jobs));
            }
        }
        Task::none()
    }

    /// The files the dialog writes of the graph being edited: planned, each with its path.
    pub(crate) fn export_plan(&self, dialog: &ExportDialog, folder: &Path) -> Vec<(PathBuf, PlannedFile)> {
        if dialog.preset == Preset::Maps {
            // Every Output, as each says: its name made a file's, its format's extension (as
            // File → Export wrote them before there were presets).
            let outputs: Vec<&brewed_texture_core::Node> = self.graph().nodes_of(NodeKind::OUTPUT).collect();
            let names: Vec<(String, u32)> = outputs.iter().map(|n| (n.text("name").to_string(), n.enum_value("bit_depth"))).collect();
            return export::output_paths(folder, &names)
                .into_iter()
                .zip(outputs)
                .map(|(path, n)| (path, PlannedFile { name: n.text("name").to_string(), what: Planned::AsItIs(n.id), said: String::new() }))
                .collect();
        }
        let plan = export_presets::plan(self.graph(), &self.doc.entry().name, dialog.preset, dialog.directx);
        let extension = export::extension(dialog.depth);
        plan.into_iter().map(|f| (folder.join(format!("{}.{extension}", export::sanitize_file_name(&f.name))), f)).collect()
    }

    fn export_dialog_jobs(&self, dialog: &ExportDialog, folder: &Path) -> Vec<ExportJob> {
        if dialog.preset == Preset::Maps {
            return self.export_jobs(folder);
        }
        // Each file of what arrives at the Outputs, in the channels and format the preset says.
        self.export_plan(dialog, folder)
            .into_iter()
            .filter_map(|(path, file)| {
                let pack: Vec<Packed> = match file.what {
                    Planned::AsItIs(node) => (0..3).map(|channel| Packed::Channel { node, channel, invert: false }).collect(),
                    Planned::Pack(sources) => sources
                        .into_iter()
                        .map(|s| match s {
                            Source::Map { map: node, channel, invert } => Packed::Channel { node, channel, invert },
                            Source::Value(v) => Packed::Value(v),
                        })
                        .collect(),
                };
                let node = pack.iter().find_map(|p| match p {
                    Packed::Channel { node, .. } => Some(*node),
                    Packed::Value(_) => None,
                })?;
                Some(ExportJob { node, path, format: 0, bit_depth: dialog.depth, pack })
            })
            .collect()
    }
}
