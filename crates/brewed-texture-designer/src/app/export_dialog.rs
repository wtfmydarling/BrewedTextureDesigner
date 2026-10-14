//! The export dialog (File → Export…, Ctrl+E): what is written of the graph being edited (its
//! Outputs as they are, the files a game engine expects of a material, see
//! [`brewed_texture_core::export_presets`], or what a template of the user's says, see
//! [`templates_folder`]), in what, and where; the files listed before they are written.

use std::path::{Path, PathBuf};

use brewed_texture_core::NodeKind;
use brewed_texture_core::export_presets::{self, Planned, PlannedFile, Preset, Source};
use brewed_texture_core::nodes::DEPTH_8;
use brewed_texture_engine::export::{self, Packed};
use brewed_texture_engine::{ExportJob, Request};
use iced::Task;

use super::{App, Dialog, Message, blocking};

/// Where the templates of the user's are: each a file of JSON, as `texture_export::Template`
/// says (in the settings' folder; none in the tests, which read nothing of the user's).
pub fn templates_folder() -> Option<PathBuf> {
    if cfg!(test) { None } else { crate::workspace::folder().map(|f| f.join("Export Templates")) }
}

/// The export dialog, open.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDialog {
    pub preset: Preset,
    /// A template of the user's (of `templates`) chosen in place of the preset.
    pub template: Option<usize>,
    /// The templates of the user's, read as the dialog opened, and what could not be read of them.
    pub templates: Vec<texture_export::Template>,
    pub problems: Vec<String>,
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
    /// A template of the user's (of those the dialog read).
    Template(usize),
    /// Show the folder of the templates (made, with an example in it, where it is empty).
    OpenTemplates,
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
            let (templates, problems) = templates_folder().map_or_else(Default::default, |f| texture_export::read_templates(&f));
            let template = self.ui.export_template.as_ref().and_then(|name| templates.iter().position(|t| &t.name == name));
            let dialog = ExportDialog {
                preset: self.ui.export_preset,
                template,
                templates,
                problems,
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
            ExportMessage::Preset(preset) => {
                dialog.preset = preset;
                dialog.template = None;
            }
            ExportMessage::Template(index) => dialog.template = (index < dialog.templates.len()).then_some(index),
            ExportMessage::OpenTemplates => {
                let Some(folder) = templates_folder() else { return Task::none() };
                // (An example to start from where there is none yet: the Unreal Engine's, renamed.)
                let example = folder.join("Example.json");
                let empty = std::fs::read_dir(&folder).map_or(true, |mut entries| entries.next().is_none());
                let made = std::fs::create_dir_all(&folder).and_then(|()| {
                    if !empty {
                        return Ok(());
                    }
                    let mut template = Preset::Unreal.template().cloned().ok_or_else(|| std::io::Error::other("no template"))?;
                    template.name = "Example (Unreal Engine)".into();
                    template.about = format!("A copy of the Unreal Engine's to start from. {}", template.about);
                    std::fs::write(&example, serde_json::to_string_pretty(&template).unwrap_or_default())
                });
                match made {
                    Ok(()) => crate::log_file::reveal(if example.exists() { &example } else { &folder }),
                    Err(e) => self.set_status(format!("The folder of templates could not be made: {e}"), true),
                }
            }
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
                self.ui.export_template = dialog.template.and_then(|i| dialog.templates.get(i)).map(|t| t.name.clone());
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
        if dialog.as_they_are() {
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
        let name = &self.doc.entry().name;
        let plan = match dialog.chosen_template() {
            Some(template) => texture_export::plan_template(template, &export_presets::maps(self.graph()), name, dialog.directx),
            None => export_presets::plan(self.graph(), name, dialog.preset, dialog.directx),
        };
        let extension = export::extension(dialog.depth);
        plan.into_iter().map(|f| (folder.join(format!("{}.{extension}", export::sanitize_file_name(&f.name))), f)).collect()
    }

    fn export_dialog_jobs(&self, dialog: &ExportDialog, folder: &Path) -> Vec<ExportJob> {
        if dialog.as_they_are() {
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

impl ExportDialog {
    /// The template of the user's chosen, where one is.
    pub fn chosen_template(&self) -> Option<&texture_export::Template> {
        self.template.and_then(|i| self.templates.get(i))
    }

    /// Whether the Outputs are written as they are (no template packs them).
    pub fn as_they_are(&self) -> bool {
        self.chosen_template().is_none() && self.preset == Preset::Maps
    }

    /// The normal maps what is chosen writes: DirectX's.
    pub fn writes_directx(&self) -> bool {
        match self.chosen_template() {
            Some(template) => template.normals == texture_export::Normals::DirectX,
            None => self.preset.directx(),
        }
    }

    /// The name of what is chosen.
    pub fn chosen_name(&self) -> &str {
        self.chosen_template().map_or(self.preset.name(), |t| t.name.as_str())
    }
}
