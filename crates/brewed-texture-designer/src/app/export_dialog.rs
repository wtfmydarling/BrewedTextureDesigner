//! The export window (File → Export…, Ctrl+E): what is written of the graphs of the packages
//! open (each checked in its list; the graph being edited, at first), with which template (the
//! Outputs as they are, the files a game engine expects of a material, see
//! [`brewed_texture_core::export_presets`], or what a template of the user's says, see
//! [`templates_folder`]), as what, how large, and where; the templates made and changed (the
//! program's too: each made again as it was with Reset); the files listed before they are
//! written; the settings kept with the packages (Save settings). The window is the
//! `export-window` crate's; this keeps it, does what is done in it, and writes the files, the
//! templates and the settings.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use brewed_texture_core::export_presets::{self, ExportSettings, GraphExportSettings, Planned, PlannedFile, Preset, RAW_OUTPUTS, Source};
use brewed_texture_core::nodes::{
    DEPTH_8, DEPTH_16, DEPTH_16F, DEPTH_32F, DEPTH_BMP8, DEPTH_DXT1, DEPTH_DXT2, DEPTH_DXT3, DEPTH_DXT4, DEPTH_DXT5, DEPTH_HDR32F, DEPTH_JPEG8, DEPTH_TGA8,
    DEPTH_TIFF8, DEPTH_TIFF16, DEPTH_TIFF32F,
};
use brewed_texture_core::package::PackageEdit;
use brewed_texture_core::{GraphId, NodeId, NodeKind};
use brewed_texture_engine::export::Packed;
use brewed_texture_engine::{ExportJob, GraphExport, Request};
use export_window::{Chosen, Event, Own, Setting, Settings, Which};
use iced::Task;
use texture_export::{FileFormat, Format, Normals, NormalsWritten, Size, Template};

use super::{App, Dialog, Message, blocking};

/// Where the templates of the user's are: each a file of JSON, as `texture_export::Template`
/// says (in the settings' folder; none in the tests, which read and write nothing of the user's).
/// The program's presets, as the user changed them, are in its folder `Built-in`, each by its
/// id (see [`Preset::id`]).
pub fn templates_folder() -> Option<PathBuf> {
    if cfg!(test) { None } else { crate::workspace::folder().map(|f| f.join("Export Templates")) }
}

/// Where the program's preset `preset`, as the user changed it, is kept.
fn changed_path(preset: Preset) -> Option<PathBuf> {
    templates_folder().map(|f| f.join("Built-in").join(format!("{}.json", preset.id())))
}

/// The formats, by what the engine writes each as (see `brewed_texture_engine::export`): an
/// Output's format, an export's.
const DEPTHS: [(Format, u32); 16] = [
    (Format::Png8, DEPTH_8),
    (Format::Png16, DEPTH_16),
    (Format::Jpeg8, DEPTH_JPEG8),
    (Format::Tga8, DEPTH_TGA8),
    (Format::Bmp8, DEPTH_BMP8),
    (Format::Tiff8, DEPTH_TIFF8),
    (Format::Tiff16, DEPTH_TIFF16),
    (Format::Tiff32, DEPTH_TIFF32F),
    (Format::Exr16, DEPTH_16F),
    (Format::Exr32, DEPTH_32F),
    (Format::Hdr32, DEPTH_HDR32F),
    (Format::Dxt1, DEPTH_DXT1),
    (Format::Dxt2, DEPTH_DXT2),
    (Format::Dxt3, DEPTH_DXT3),
    (Format::Dxt4, DEPTH_DXT4),
    (Format::Dxt5, DEPTH_DXT5),
];

/// What the engine writes a file of `format` as.
pub fn depth_of(format: Format) -> u32 {
    DEPTHS.iter().find(|(f, _)| *f == format).map_or(DEPTH_8, |(_, d)| *d)
}

/// The format an Output of the format `depth` writes.
pub fn format_of(depth: u32) -> Format {
    DEPTHS.iter().find(|(_, d)| *d == depth).map_or(Format::Png8, |(f, _)| *f)
}

/// `preset`, as the window chooses it: its place among the program's.
pub fn chosen_of(preset: Preset) -> Chosen {
    Chosen::Preset(Preset::ALL.iter().position(|p| *p == preset).unwrap_or(0))
}

/// A row of the list of what is exported: a package open (a heading: `graph` none), or a graph
/// of it, checked to be exported, with what it says of its own, the size it is made at and the
/// way its normal maps point.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportRow {
    pub key: u64,
    pub graph: Option<GraphId>,
    pub name: String,
    pub checked: bool,
    pub own: Own,
    pub size: [u32; 2],
    pub normals: Normals,
}

/// The export window, open.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportDialog {
    /// The templates of the user's, read as the window opened, each with the file it is kept in
    /// (none where they are not kept); and what could not be read.
    pub templates: Vec<Template>,
    pub paths: Vec<Option<PathBuf>>,
    pub problems: Vec<String>,
    /// The program's presets as the user changed them, in the order of [`Preset::ALL`] (none:
    /// as the program makes them).
    pub changed: Vec<Option<Template>>,
    /// Where and what all are exported with (what a package or a graph says of its own is its
    /// row's).
    pub global: Settings,
    /// The packages open and their graphs.
    pub rows: Vec<ExportRow>,
    /// The window's own: its tab, the row and the template shown, its menus.
    pub window: export_window::State,
}

#[derive(Clone, Debug)]
pub enum ExportMessage {
    Open,
    /// What is done in the window.
    Window(Event),
    /// The folder chosen in a dialog of the system, of the global settings (`None`) or of a row.
    Folder(Option<usize>, Option<PathBuf>),
}

/// A file the window writes: of which row, where, of what, as what and how large.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedExport {
    pub row: usize,
    pub path: PathBuf,
    pub file: PlannedFile,
    pub format: Format,
    pub size: [u32; 2],
}

impl ExportDialog {
    /// The program's presets, as the window offers them (as the user changed them).
    pub fn presets(&self) -> Vec<export_window::Preset<'_>> {
        Preset::ALL
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let changed = self.changed.get(i).and_then(Option::as_ref);
                export_window::Preset { name: p.name(), template: changed.or(p.template()), changed: changed.is_some() }
            })
            .collect()
    }

    /// The template `chosen` is (none: the Outputs as they are).
    pub fn template_of(&self, chosen: Chosen) -> Option<&Template> {
        match chosen {
            Chosen::Preset(i) => self.changed.get(i).and_then(Option::as_ref).or_else(|| Preset::ALL.get(i).and_then(|p| p.template())),
            Chosen::Mine(i) => self.templates.get(i),
        }
    }

    /// The name of what `chosen` is, as settings keep it.
    fn name_of(&self, chosen: Chosen) -> String {
        match chosen {
            Chosen::Preset(i) => Preset::ALL.get(i).map_or(RAW_OUTPUTS, |p| p.name()).to_string(),
            Chosen::Mine(i) => self.templates.get(i).map_or_else(String::new, |t| t.name.clone()),
        }
    }

    /// What is called `name`: a preset of the program's first, else one of the user's.
    fn named(&self, name: &str) -> Option<Chosen> {
        Preset::ALL.iter().position(|p| p.name() == name).map(Chosen::Preset).or_else(|| self.templates.iter().position(|t| t.name == name).map(Chosen::Mine))
    }

    /// The settings of the row `i`: the global ones, then what its package says of its own (of a
    /// graph), then what it says.
    pub fn settings_of(&self, i: usize) -> Settings {
        let Some(row) = self.rows.get(i) else { return self.global.clone() };
        match row.graph {
            None => self.global.with(&row.own),
            Some(_) => {
                let package = self.rows.iter().find(|r| r.key == row.key && r.graph.is_none());
                package.map_or_else(|| self.global.clone(), |p| self.global.with(&p.own)).with(&row.own)
            }
        }
    }

    /// The folder the files of the row `i` go to (none: none chosen).
    pub fn folder_of(&self, i: usize) -> Option<PathBuf> {
        self.settings_of(i).folder.map(PathBuf::from)
    }

    /// The template of the user's `index` written to its file, where it is kept.
    fn keep(&self, index: usize) -> Result<(), String> {
        match (self.paths.get(index).cloned().flatten(), self.templates.get(index)) {
            (Some(path), Some(template)) => texture_export::write_template(&path, template).map_err(|e| format!("{}: {e}", path.display())),
            _ => Ok(()),
        }
    }

    /// The program's preset `i` as the user changed it written to its file, where they are kept;
    /// as the program makes it, its file gone.
    fn keep_changed(&self, i: usize) -> Result<(), String> {
        let Some(path) = Preset::ALL.get(i).and_then(|p| changed_path(*p)) else { return Ok(()) };
        match self.changed.get(i).and_then(Option::as_ref) {
            Some(template) => std::fs::create_dir_all(path.parent().unwrap_or(&path))
                .map_err(|e| e.to_string())
                .and_then(|()| texture_export::write_template(&path, template).map_err(|e| format!("{}: {e}", path.display()))),
            None => match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
                _ => Ok(()),
            },
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

    /// Every choice of a template of the user's after `index`, which is gone, as it was; of it,
    /// the Unreal Engine's.
    fn forget_mine(&mut self, index: usize) {
        let after = |chosen: Chosen| match chosen {
            Chosen::Mine(i) if i == index => chosen_of(Preset::Unreal),
            Chosen::Mine(i) if i > index => Chosen::Mine(i - 1),
            chosen => chosen,
        };
        self.global.template = after(self.global.template);
        for row in &mut self.rows {
            row.own.template = row.own.template.map(after);
        }
        let shown = after(self.window.shown);
        self.window.update(&Event::Show(shown));
    }
}

impl App {
    /// The packages open and their graphs, as the list of what is exported has them: each graph
    /// as its package keeps it (or, where it keeps nothing, the graph edited checked alone).
    fn export_rows(&self, dialog: &ExportDialog) -> Vec<ExportRow> {
        let mut rows = Vec::new();
        for (key, doc) in self.packages() {
            let kept = doc.package().export.as_ref();
            let graphs = &doc.package().graphs;
            if graphs.is_empty() {
                continue;
            }
            let folder = |f: &String| (!f.is_empty()).then(|| f.clone());
            let own = kept.map_or_else(Own::default, |k| Own {
                folder: folder(&k.folder),
                template: k.template.as_deref().and_then(|name| dialog.named(name)),
                format: k.format,
                size: k.size,
                normals: k.normals,
            });
            rows.push(ExportRow { key, graph: None, name: doc.title(), checked: false, own, size: [0, 0], normals: Normals::OpenGL });
            for entry in graphs {
                let saved = kept.and_then(|k| k.graphs.iter().find(|g| g.graph == entry.id));
                let own = saved.map_or_else(Own::default, |g| Own {
                    folder: folder(&g.folder),
                    template: g.template.as_deref().and_then(|name| dialog.named(name)),
                    format: g.format,
                    size: g.size,
                    normals: g.normals,
                });
                let edited = key == self.doc_key && entry.id == self.doc.current();
                rows.push(ExportRow {
                    key,
                    graph: Some(entry.id),
                    name: entry.name.clone(),
                    checked: kept.map_or(edited, |_| saved.is_some_and(|g| g.checked)),
                    own,
                    size: entry.graph.resolution,
                    normals: if export_presets::graph_normals_directx(&entry.graph) { Normals::DirectX } else { Normals::OpenGL },
                });
            }
        }
        // (A package is checked where all its graphs are.)
        heading_checks(&mut rows);
        rows
    }

    /// The global settings as the export before left them (what a package keeps is its own):
    /// at first, the Outputs as they are, as large as they are made, in the package's folder.
    fn export_global(&self, dialog: &ExportDialog) -> Settings {
        let folder = self.ui.export_folder.clone().or_else(|| self.doc.path().and_then(|p| p.parent()).map(Path::to_path_buf));
        Settings {
            folder: folder.map(|f| f.display().to_string()),
            template: dialog.named(&self.ui.export_template).unwrap_or(chosen_of(Preset::Maps)),
            format: self.ui.export_format,
            size: self.ui.export_size,
            normals: self.ui.export_normals,
        }
    }

    pub(crate) fn export_message(&mut self, message: ExportMessage) -> Task<Message> {
        if let ExportMessage::Open = message {
            self.close_popups();
            let (read, mut problems) = templates_folder().map_or_else(Default::default, |f| texture_export::read_templates(&f));
            let (paths, templates): (Vec<_>, Vec<_>) = read.into_iter().map(|(path, template)| (Some(path), template)).unzip();
            // The program's presets as the user changed them.
            let changed = Preset::ALL
                .iter()
                .map(|p| {
                    let path = changed_path(*p).filter(|path| p.template().is_some() && path.exists())?;
                    match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| texture_export::template_of(&text)) {
                        Ok(template) => Some(template),
                        Err(e) => {
                            problems.push(format!("{}: {e}", path.display()));
                            None
                        }
                    }
                })
                .collect();
            let mut dialog = ExportDialog {
                templates,
                paths,
                problems,
                changed,
                global: Settings {
                    folder: None,
                    template: chosen_of(Preset::Maps),
                    format: FileFormat::AsMade,
                    size: Size::AsMade,
                    normals: NormalsWritten::AsTemplate,
                },
                rows: Vec::new(),
                window: export_window::State::new(chosen_of(Preset::Maps), &[]),
            };
            dialog.global = self.export_global(&dialog);
            dialog.rows = self.export_rows(&dialog);
            dialog.window = export_window::State::new(dialog.global.template, &dialog.presets());
            self.ui.dialog = Some(Dialog::Export(Box::new(dialog)));
            return Task::none();
        }
        let Some(Dialog::Export(dialog)) = &mut self.ui.dialog else { return Task::none() };
        let event = match message {
            ExportMessage::Open => return Task::none(),
            ExportMessage::Folder(row, Some(folder)) => {
                let folder = Some(folder.display().to_string());
                match row.and_then(|i| dialog.rows.get_mut(i)) {
                    Some(row) => row.own.folder = folder,
                    None => dialog.global.folder = folder,
                }
                return Task::none();
            }
            ExportMessage::Folder(_, None) => return Task::none(),
            ExportMessage::Window(event) => event,
        };
        dialog.window.update(&event);
        let mut said: Result<(), String> = Ok(());
        match event {
            Event::Check(i, checked) => {
                if let Some(row) = dialog.rows.get(i).cloned() {
                    for other in dialog.rows.iter_mut().filter(|r| r.key == row.key && (row.graph.is_none() || r.graph == row.graph)) {
                        other.checked = checked;
                    }
                    heading_checks(&mut dialog.rows);
                }
            }
            Event::IncludeAll(checked) => dialog.rows.iter_mut().for_each(|r| r.checked = checked),
            Event::Set(row, setting) => match row {
                None => match setting {
                    Setting::Template(t) => dialog.global.template = t,
                    Setting::Format(f) => dialog.global.format = f,
                    Setting::Size(s) => dialog.global.size = s,
                    Setting::Normals(n) => dialog.global.normals = n,
                },
                Some(i) => {
                    if let Some(own) = dialog.rows.get_mut(i).map(|r| &mut r.own) {
                        match setting {
                            Setting::Template(t) => own.template = Some(t),
                            Setting::Format(f) => own.format = Some(f),
                            Setting::Size(s) => own.size = Some(s),
                            Setting::Normals(n) => own.normals = Some(n),
                        }
                    }
                }
            },
            Event::Unset(i, which) => {
                if let Some(own) = dialog.rows.get_mut(i).map(|r| &mut r.own) {
                    match which {
                        Which::Folder => own.folder = None,
                        Which::Template => own.template = None,
                        Which::Format => own.format = None,
                        Which::Size => own.size = None,
                        Which::Normals => own.normals = None,
                    }
                }
            }
            Event::Browse(row) => {
                // Where its folder is, or where the settings open file dialogs.
                let folder = match row {
                    Some(i) => dialog.folder_of(i),
                    None => dialog.global.folder.as_ref().map(PathBuf::from),
                };
                let folder = folder.or_else(|| self.dialog_folder(None, false));
                return Task::perform(
                    blocking(move || {
                        let mut picker = rfd::FileDialog::new().set_title("Export to folder");
                        if let Some(folder) = folder {
                            picker = picker.set_directory(folder);
                        }
                        picker.pick_folder()
                    }),
                    move |picked| Message::Export(ExportMessage::Folder(row, picked.flatten())),
                );
            }
            Event::OpenFolder(row) => {
                let folder = match row {
                    Some(i) => dialog.folder_of(i),
                    None => dialog.global.folder.as_ref().map(PathBuf::from),
                };
                if let Some(folder) = folder {
                    crate::log_file::reveal(&folder);
                }
            }
            Event::New => {
                let mut template = Template { name: "New Template".into(), about: String::new(), normals: Default::default(), files: Vec::new() };
                let name = format!("{}_1", export_presets::TOKENS[0].written());
                let _ = template.apply(texture_export::Edit::AddFile { layout: texture_export::Layout::Rgb, name });
                said = dialog.add(template);
            }
            Event::Duplicate(of) => {
                if let Some(mut template) = dialog.template_of(of).cloned() {
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
                    dialog.forget_mine(index);
                }
            }
            Event::Edit(Chosen::Mine(index), edit) => {
                said = dialog.templates.get_mut(index).ok_or_else(|| "no such template".to_string()).and_then(|t| t.apply(edit));
                if said.is_ok() {
                    said = dialog.keep(index);
                }
            }
            Event::Edit(Chosen::Preset(i), edit) => {
                // A preset of the program's changed where it is: kept as changed (as it was made,
                // no more).
                if let Some(made) = Preset::ALL.get(i).and_then(|p| p.template()) {
                    let mut template = dialog.changed.get(i).cloned().flatten().unwrap_or_else(|| made.clone());
                    said = template.apply(edit);
                    if said.is_ok() {
                        dialog.changed[i] = (template != *made).then_some(template);
                        said = dialog.keep_changed(i);
                    }
                }
            }
            Event::Reset(i) => {
                if let Some(changed) = dialog.changed.get_mut(i) {
                    *changed = None;
                    said = dialog.keep_changed(i);
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
            Event::SaveSettings => return self.save_export_settings(),
            Event::ResetAll => self.reset_export_settings(),
            Event::Export => return self.export_now(),
            Event::Cancel => self.ui.dialog = None,
            _ => {}
        }
        if let Err(e) = said {
            self.set_status(e, true);
        }
        Task::none()
    }

    /// Every setting as it is by default: the Outputs as they are, as large as they are made, in
    /// the package's folder; the graph edited alone checked, none saying anything of its own.
    fn reset_export_settings(&mut self) {
        let folder = self.doc.path().and_then(|p| p.parent()).map(Path::to_path_buf);
        let edited = (self.doc_key, self.doc.current());
        let Some(Dialog::Export(dialog)) = &mut self.ui.dialog else { return };
        dialog.global = Settings {
            folder: folder.map(|f| f.display().to_string()),
            template: chosen_of(Preset::Maps),
            format: FileFormat::AsMade,
            size: Size::AsMade,
            normals: NormalsWritten::AsTemplate,
        };
        for row in &mut dialog.rows {
            row.own = Own::default();
            row.checked = (row.key, row.graph) == (edited.0, Some(edited.1));
        }
        heading_checks(&mut dialog.rows);
        dialog.window.update(&Event::Select(None));
    }

    /// Keep the settings with each package open (one step to undo each): saved with it, they are
    /// what its export offers next time.
    fn save_export_settings(&mut self) -> Task<Message> {
        let Some(Dialog::Export(dialog)) = &self.ui.dialog else { return Task::none() };
        let dialog = dialog.as_ref().clone();
        let mut kept = 0;
        let keys: Vec<u64> = dialog.rows.iter().filter(|r| r.graph.is_none()).map(|r| r.key).collect();
        for key in keys {
            let package = dialog.rows.iter().find(|r| r.key == key && r.graph.is_none()).map(|r| r.own.clone()).unwrap_or_default();
            let settings = ExportSettings {
                folder: package.folder.unwrap_or_default(),
                template: package.template.map(|t| dialog.name_of(t)),
                format: package.format,
                size: package.size,
                normals: package.normals,
                graphs: dialog
                    .rows
                    .iter()
                    .filter(|r| r.key == key)
                    .filter_map(|r| {
                        Some(GraphExportSettings {
                            graph: r.graph?,
                            checked: r.checked,
                            folder: r.own.folder.clone().unwrap_or_default(),
                            template: r.own.template.map(|t| dialog.name_of(t)),
                            format: r.own.format,
                            size: r.own.size,
                            normals: r.own.normals,
                        })
                    })
                    .collect(),
            };
            if let Some(doc) = self.package_doc_mut(key) {
                match doc.apply_package(PackageEdit::SetExport(Some(Box::new(settings)))) {
                    Ok(_) => kept += 1,
                    Err(e) => self.set_status(e, true),
                }
            }
        }
        self.set_status(format!("The export settings are kept with {kept} package(s): save them to keep them"), false);
        Task::none()
    }

    /// Write the files the window lists, and close it; what was chosen is offered next time.
    fn export_now(&mut self) -> Task<Message> {
        let Some(Dialog::Export(dialog)) = &self.ui.dialog else { return Task::none() };
        let dialog = dialog.as_ref().clone();
        // (Each graph checked has its folder.)
        let checked: Vec<usize> = (0..dialog.rows.len()).filter(|&i| dialog.rows[i].checked && dialog.rows[i].graph.is_some()).collect();
        if checked.iter().any(|&i| dialog.folder_of(i).is_none()) {
            return Task::none();
        }
        self.ui.dialog = None;
        self.ui.export_template = dialog.name_of(dialog.global.template);
        self.ui.export_format = dialog.global.format;
        self.ui.export_size = dialog.global.size;
        self.ui.export_normals = dialog.global.normals;
        self.ui.export_folder = dialog.global.folder.as_ref().map(PathBuf::from).or(self.ui.export_folder.take());
        let planned = self.export_plan(&dialog);
        let folders: std::collections::BTreeSet<PathBuf> = checked.iter().filter_map(|&i| dialog.folder_of(i)).collect();
        let mut count = 0;
        for (i, row) in dialog.rows.iter().enumerate() {
            let files: Vec<&PlannedExport> = planned.iter().filter(|p| p.row == i).collect();
            let (Some(graph), Some(first)) = (row.graph, files.first()) else { continue };
            let jobs: Vec<ExportJob> = files.iter().filter_map(|p| self.export_job(row, p)).collect();
            count += jobs.len();
            // The graph edited, as it is made: of the images there are. Else computed on its own,
            // as large as it is written.
            let edited = row.key == self.doc_key && graph == self.doc.current();
            if edited && first.size == row.size {
                self.send(Request::Export(jobs));
                continue;
            }
            let Some(flat) = self.flatten_graph(row.key, graph) else { continue };
            let mut flat_graph = flat.graph;
            flat_graph.resolution = first.size;
            let eval = Arc::new(brewed_texture_core::eval::evaluate(&flat_graph));
            self.send(Request::ExportGraph(GraphExport { graph: Arc::new(flat_graph), eval, jobs }));
        }
        if count == 0 {
            self.set_status("Nothing to export: the graphs checked have none of the maps the templates write", true);
            return Task::none();
        }
        self.res.exports_pending += count;
        let to = match folders.len() {
            1 => folders.first().map_or_else(String::new, |f| f.display().to_string()),
            n => format!("{n} folders"),
        };
        self.set_status(format!("Exporting {count} file(s) to {to}…"), false);
        Task::none()
    }

    /// What the parts of the files' names stand for, of the graph `graph` of the package open as
    /// `key`: the graph's name, the package's.
    pub(crate) fn export_values_of(&self, key: u64, graph: Option<GraphId>) -> Vec<(String, String)> {
        let Some(doc) = self.package_doc(key) else { return Vec::new() };
        let package = doc.path().and_then(|p| p.file_stem()).map_or_else(|| "Untitled".to_string(), |s| s.to_string_lossy().into_owned());
        let name = graph.and_then(|g| doc.package().graph(g)).map_or_else(|| doc.entry().name.clone(), |e| e.name.clone());
        vec![("graph".into(), name), ("package".into(), package)]
    }

    /// The files the window writes of the graphs checked: planned, each with its path (in the
    /// folder of its graph; of none chosen, its name alone), the format it is written in and its
    /// size.
    pub(crate) fn export_plan(&self, dialog: &ExportDialog) -> Vec<PlannedExport> {
        let mut planned: Vec<PlannedExport> = Vec::new();
        for (i, row) in dialog.rows.iter().enumerate() {
            let (true, Some(graph)) = (row.checked, row.graph) else { continue };
            let Some(entry) = self.package_doc(row.key).and_then(|d| d.package().graph(graph)) else { continue };
            let settings = dialog.settings_of(i);
            let size = settings.size.of(entry.graph.resolution);
            let format_made = |node: NodeId| format_of(entry.graph.node(node).map_or(DEPTH_8, |n| n.enum_value("bit_depth")));
            let chosen = |made: Format| match settings.format {
                FileFormat::Of(format) => format,
                FileFormat::AsMade => made,
            };
            match dialog.template_of(settings.template) {
                // Each Output as it says: its name, channels and format (or the one chosen).
                None => {
                    for node in entry.graph.nodes_of(NodeKind::OUTPUT) {
                        let file = PlannedFile { name: node.text("name").to_string(), what: Planned::AsItIs(node.id), said: String::new(), format: None };
                        planned.push(PlannedExport { row: i, path: PathBuf::new(), file, format: chosen(format_made(node.id)), size });
                    }
                }
                Some(template) => {
                    let values = self.export_values_of(row.key, row.graph);
                    let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                    // Its normal maps as its Normal nodes make them, written as the settings say
                    // (as the template says, or one way whatever it says).
                    let directx = row.normals == Normals::DirectX;
                    let mut template = template.clone();
                    if let NormalsWritten::Of(way) = settings.normals {
                        template.normals = way;
                    }
                    for file in texture_export::plan_template(&template, &export_presets::maps(&entry.graph), &values, directx) {
                        // A file of no format of its own: as the settings say (as made: as the
                        // first map it is made of).
                        let first = match &file.what {
                            Planned::AsItIs(node) => Some(*node),
                            Planned::Pack(sources) => sources.iter().find_map(|s| if let Source::Map { map, .. } = s { Some(*map) } else { None }),
                        };
                        let format = file.format.unwrap_or_else(|| chosen(first.map_or(Format::Png8, format_made)));
                        planned.push(PlannedExport { row: i, path: PathBuf::new(), file, format, size });
                    }
                }
            }
        }
        // Each file its own path, of all the graphs going to a folder at once (two graphs' Outputs
        // may be named alike).
        let folders: Vec<PathBuf> = planned.iter().map(|p| dialog.folder_of(p.row).unwrap_or_default()).collect();
        let mut done: Vec<&PathBuf> = Vec::new();
        for folder in &folders {
            if done.contains(&folder) {
                continue;
            }
            done.push(folder);
            let at: Vec<usize> = (0..planned.len()).filter(|&i| &folders[i] == folder).collect();
            let named: Vec<(String, &str)> = at.iter().map(|&i| (planned[i].file.name.clone(), planned[i].format.extension())).collect();
            let paths = texture_export::unique_paths(folder, &named);
            for (i, path) in at.into_iter().zip(paths) {
                planned[i].path = path;
            }
        }
        planned
    }

    /// What the engine writes of a file planned of `row`.
    fn export_job(&self, row: &ExportRow, planned: &PlannedExport) -> Option<ExportJob> {
        let entry = self.package_doc(row.key).and_then(|d| d.package().graph(row.graph?))?;
        let bit_depth = depth_of(planned.format);
        match &planned.file.what {
            Planned::AsItIs(node) => {
                let n = entry.graph.node(*node)?;
                // (Its channels as the graph edited computes them, where it is that one.)
                let edited = row.key == self.doc_key && row.graph == Some(self.doc.current());
                let format = if edited { self.eval.params(*node).map_or(n.enum_value("format"), |p| p.u("format")) } else { n.enum_value("format") };
                Some(ExportJob { node: *node, path: planned.path.clone(), format, bit_depth, pack: Vec::new() })
            }
            Planned::Pack(sources) => {
                let pack: Vec<Packed> = sources
                    .iter()
                    .map(|s| match *s {
                        Source::Map { map: node, channel, invert } => Packed::Channel { node, channel, invert },
                        Source::Value(v) => Packed::Value(v),
                    })
                    .collect();
                // (A file of values alone is not written: it has no size of its own.)
                let node = pack.iter().find_map(|p| match p {
                    Packed::Channel { node, .. } => Some(*node),
                    Packed::Value(_) => None,
                })?;
                Some(ExportJob { node, path: planned.path.clone(), format: 0, bit_depth, pack })
            }
        }
    }
}

/// Each package checked where all its graphs are.
fn heading_checks(rows: &mut [ExportRow]) {
    let keys: Vec<u64> = rows.iter().filter(|r| r.graph.is_none()).map(|r| r.key).collect();
    for key in keys {
        let all = rows.iter().filter(|r| r.key == key && r.graph.is_some()).all(|r| r.checked);
        if let Some(heading) = rows.iter_mut().find(|r| r.key == key && r.graph.is_none()) {
            heading.checked = all;
        }
    }
}
