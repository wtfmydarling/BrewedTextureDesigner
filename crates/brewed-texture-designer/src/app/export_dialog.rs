//! The export window (File → Export…, Ctrl+E): what is written of the graph being edited (its
//! Outputs as they are, the files a game engine expects of a material, see
//! [`brewed_texture_core::export_presets`], or what a template of the user's says, see
//! [`templates_folder`]), in what, and where; the templates made and changed; the files listed
//! before they are written. The window is the `export-window` crate's; this keeps it, does what
//! is done in it, and writes the files and the templates.

use std::path::{Path, PathBuf};

use brewed_texture_core::NodeKind;
use brewed_texture_core::export_presets::{self, Planned, PlannedFile, Preset, Source};
use brewed_texture_core::nodes::{DEPTH_8, DEPTH_16, DEPTH_16F, DEPTH_32F};
use brewed_texture_engine::export::{self, Packed};
use brewed_texture_engine::{ExportJob, Request};
use export_window::{Chosen, Event};
use iced::Task;
use texture_export::{Format, Template};

use super::{App, Dialog, Message, blocking};

/// Where the templates of the user's are: each a file of JSON, as `texture_export::Template`
/// says (in the settings' folder; none in the tests, which read and write nothing of the user's).
pub fn templates_folder() -> Option<PathBuf> {
    if cfg!(test) { None } else { crate::workspace::folder().map(|f| f.join("Export Templates")) }
}

/// What the engine writes a file of `format` as (see `brewed_texture_engine::export`).
pub fn depth_of(format: Format) -> u32 {
    match format {
        Format::Png8 => DEPTH_8,
        Format::Png16 => DEPTH_16,
        Format::Exr16 => DEPTH_16F,
        Format::Exr32 => DEPTH_32F,
    }
}

/// The export window, open.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDialog {
    /// What the files are written with.
    pub chosen: Chosen,
    /// The templates of the user's, read as the window opened, each with the file it is kept in
    /// (none where they are not kept); and what could not be read.
    pub templates: Vec<Template>,
    pub paths: Vec<Option<PathBuf>>,
    pub problems: Vec<String>,
    /// The graph's normal maps are DirectX's (Y−), not OpenGL's.
    pub directx: bool,
    /// The format of a file whose template says none.
    pub format: Format,
    pub folder: Option<PathBuf>,
    /// The window's own: its tab, the template shown, the channel chosen, its menus.
    pub window: export_window::State,
}

#[derive(Clone, Debug)]
pub enum ExportMessage {
    Open,
    /// What is done in the window.
    Window(Event),
    /// The folder chosen in a dialog of the system.
    Folder(Option<PathBuf>),
}

impl ExportDialog {
    /// The template the files are written with (none: the Outputs as they are).
    pub fn template(&self) -> Option<&Template> {
        self.chosen.template(&self.templates)
    }

    /// Whether the Outputs are written as they are (no template packs them).
    pub fn as_they_are(&self) -> bool {
        self.template().is_none()
    }

    /// The template of the user's `index` written to its file, where it is kept.
    fn keep(&self, index: usize) -> Result<(), String> {
        match (self.paths.get(index).cloned().flatten(), self.templates.get(index)) {
            (Some(path), Some(template)) => texture_export::write_template(&path, template).map_err(|e| format!("{}: {e}", path.display())),
            _ => Ok(()),
        }
    }

    /// A template of the user's more: kept in a file of its own, where they are kept; shown.
    fn add(&mut self, template: Template) -> Result<(), String> {
        let path = templates_folder().map(|folder| texture_export::new_template_path(&folder, &template.name));
        self.templates.push(template);
        self.paths.push(path);
        let index = self.templates.len() - 1;
        self.window.update(&Event::Show(Chosen::Mine(index)));
        self.keep(index)
    }
}

impl App {
    pub(crate) fn export_message(&mut self, message: ExportMessage) -> Task<Message> {
        if let ExportMessage::Open = message {
            self.close_popups();
            let (read, problems) = templates_folder().map_or_else(Default::default, |f| texture_export::read_templates(&f));
            let (paths, templates): (Vec<_>, Vec<_>) = read.into_iter().map(|(path, template)| (Some(path), template)).unzip();
            let mine = self.ui.export_template.as_ref().and_then(|name| templates.iter().position(|t: &Template| &t.name == name));
            let chosen = mine.map_or(Chosen::Preset(self.ui.export_preset), Chosen::Mine);
            let dialog = ExportDialog {
                chosen,
                templates,
                paths,
                problems,
                directx: export_presets::graph_normals_directx(self.graph()),
                format: self.ui.export_format,
                folder: self.ui.export_folder.clone().or_else(|| self.doc.path().and_then(|p| p.parent()).map(Path::to_path_buf)),
                window: export_window::State::new(chosen),
            };
            self.ui.dialog = Some(Dialog::Export(Box::new(dialog)));
            return Task::none();
        }
        let Some(Dialog::Export(dialog)) = &mut self.ui.dialog else { return Task::none() };
        let event = match message {
            ExportMessage::Open => return Task::none(),
            ExportMessage::Folder(Some(folder)) => {
                dialog.folder = Some(folder);
                return Task::none();
            }
            ExportMessage::Folder(None) => return Task::none(),
            ExportMessage::Window(event) => event,
        };
        dialog.window.update(&event);
        let mut said: Result<(), String> = Ok(());
        match event {
            Event::ExportWith(chosen) => dialog.chosen = chosen,
            Event::DirectX(directx) => dialog.directx = directx,
            Event::Format(format) => dialog.format = format,
            Event::Browse => {
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
            Event::OpenFolder => {
                if let Some(folder) = &dialog.folder {
                    crate::log_file::reveal(folder);
                }
            }
            Event::New => {
                let mut template = Template { name: "New Template".into(), about: String::new(), normals: Default::default(), files: Vec::new() };
                let _ = template.apply(texture_export::Edit::AddFile(texture_export::Layout::Rgb));
                said = dialog.add(template);
            }
            Event::Duplicate(of) => {
                if let Some(mut template) = of.template(&dialog.templates).cloned() {
                    template.name = format!("{} (copy)", template.name);
                    said = dialog.add(template);
                }
            }
            Event::Delete(index) if index < dialog.templates.len() => {
                if let Some(Some(path)) = dialog.paths.get(index)
                    && let Err(e) = std::fs::remove_file(path)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    said = Err(format!("{}: {e}", path.display()));
                }
                if said.is_ok() {
                    dialog.templates.remove(index);
                    dialog.paths.remove(index);
                    // What named the ones after it names them still.
                    let after = |chosen: Chosen| match chosen {
                        Chosen::Mine(i) if i == index => Chosen::Preset(Preset::Unreal),
                        Chosen::Mine(i) if i > index => Chosen::Mine(i - 1),
                        chosen => chosen,
                    };
                    dialog.chosen = after(dialog.chosen);
                    let shown = after(dialog.window.shown);
                    dialog.window.update(&Event::Show(shown));
                }
            }
            Event::Edit(index, edit) => {
                said = dialog.templates.get_mut(index).ok_or_else(|| "no such template".to_string()).and_then(|t| t.apply(edit));
                if said.is_ok() {
                    said = dialog.keep(index);
                }
            }
            Event::OpenTemplates => {
                let Some(folder) = templates_folder() else { return Task::none() };
                // (An example to start from where there is none yet: the Unreal Engine's, renamed.)
                let example = folder.join("Example.json");
                let empty = std::fs::read_dir(&folder).map_or(true, |mut entries| entries.next().is_none());
                let made = std::fs::create_dir_all(&folder).map_err(|e| e.to_string()).and_then(|()| {
                    if !empty {
                        return Ok(());
                    }
                    let mut template = Preset::Unreal.template().cloned().ok_or("no template")?;
                    template.name = "Example (Unreal Engine)".into();
                    template.about = format!("A copy of the Unreal Engine's to start from. {}", template.about);
                    texture_export::write_template(&example, &template).map_err(|e| e.to_string())
                });
                match made {
                    Ok(()) => crate::log_file::reveal(if example.exists() { &example } else { &folder }),
                    Err(e) => said = Err(format!("The folder of templates could not be made: {e}")),
                }
            }
            Event::Export => return self.export_now(),
            Event::Cancel => self.ui.dialog = None,
            _ => {}
        }
        if let Err(e) = said {
            self.set_status(e, true);
        }
        Task::none()
    }

    /// Write the files the window lists, and close it; what was chosen is offered next time.
    fn export_now(&mut self) -> Task<Message> {
        let Some(Dialog::Export(dialog)) = &self.ui.dialog else { return Task::none() };
        let dialog = dialog.as_ref().clone();
        let Some(folder) = dialog.folder.clone() else { return Task::none() };
        self.ui.dialog = None;
        match dialog.chosen {
            Chosen::Preset(preset) => {
                self.ui.export_preset = preset;
                self.ui.export_template = None;
            }
            Chosen::Mine(_) => self.ui.export_template = dialog.template().map(|t| t.name.clone()),
        }
        self.ui.export_format = dialog.format;
        self.ui.export_folder = Some(folder.clone());
        let jobs = self.export_dialog_jobs(&dialog, &folder);
        if jobs.is_empty() {
            self.set_status("Nothing to export: the graph has no Output of what the template writes", true);
            return Task::none();
        }
        self.res.exports_pending += jobs.len();
        self.set_status(format!("Exporting {} file(s) to {}…", jobs.len(), folder.display()), false);
        self.send(Request::Export(jobs));
        Task::none()
    }

    /// What the parts of the files' names stand for: the graph's name, the package's.
    pub(crate) fn export_values(&self) -> Vec<(String, String)> {
        let package = self.doc.path().and_then(|p| p.file_stem()).map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
        vec![("graph".into(), self.doc.entry().name.clone()), ("package".into(), package)]
    }

    /// The files the window writes of the graph being edited: planned, each with its path and
    /// the format it is written in (an Output as it is: as its node says, `None`).
    pub(crate) fn export_plan(&self, dialog: &ExportDialog, folder: &Path) -> Vec<(PathBuf, PlannedFile, Option<Format>)> {
        let Some(template) = dialog.template() else {
            // Every Output, as each says: its name made a file's, its format's extension (as
            // File → Export wrote them before there were templates).
            let outputs: Vec<&brewed_texture_core::Node> = self.graph().nodes_of(NodeKind::OUTPUT).collect();
            let names: Vec<(String, u32)> = outputs.iter().map(|n| (n.text("name").to_string(), n.enum_value("bit_depth"))).collect();
            return export::output_paths(folder, &names)
                .into_iter()
                .zip(outputs)
                .map(|(path, n)| (path, PlannedFile { name: n.text("name").to_string(), what: Planned::AsItIs(n.id), said: String::new(), format: None }, None))
                .collect();
        };
        let values = self.export_values();
        let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let plan = texture_export::plan_template(template, &export_presets::maps(self.graph()), &values, dialog.directx);
        let formats: Vec<Format> = plan.iter().map(|f| f.format.unwrap_or(dialog.format)).collect();
        let named: Vec<(String, &str)> = plan.iter().zip(&formats).map(|(f, format)| (f.name.clone(), format.extension())).collect();
        let paths = texture_export::unique_paths(folder, &named);
        paths.into_iter().zip(plan).zip(formats).map(|((path, file), format)| (path, file, Some(format))).collect()
    }

    fn export_dialog_jobs(&self, dialog: &ExportDialog, folder: &Path) -> Vec<ExportJob> {
        if dialog.as_they_are() {
            return self.export_jobs(folder);
        }
        // Each file of what arrives at the Outputs, in the channels and format the template says.
        self.export_plan(dialog, folder)
            .into_iter()
            .filter_map(|(path, file, format)| {
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
                // (A file of values alone is not written: it has no size of its own.)
                let node = pack.iter().find_map(|p| match p {
                    Packed::Channel { node, .. } => Some(*node),
                    Packed::Value(_) => None,
                })?;
                Some(ExportJob { node, path, format: 0, bit_depth: depth_of(format.unwrap_or(dialog.format)), pack })
            })
            .collect()
    }
}
