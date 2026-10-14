//! Templates of an export: what files a preset writes, as data. The presets of the game
//! engines are such templates (in `templates/`, read as the program starts), and so is a
//! template of someone's own: a file of JSON in a folder the program reads (see
//! [`read_templates`]), as Substance Painter's output templates are, in essence.
//!
//! A template is a list of files. Each has a name, where `$(graph)` and the other parts of
//! [`crate::naming`] stand for what is exported; the maps it needs (it is written where the
//! program has a map of one of them; those its parts are made of, where it says none); the
//! format it is written in, where it says one; and its parts, in the order of its channels: a
//! map's red, green and blue (`rgb`), one channel of a map (`one`), or a value (`value`). A part
//! of a map the program has not is its `missing` value, or left out where it is `optional`;
//! without either, the file is not written. Where a program has no map of a usage, one that
//! stands for it is taken (a diffuse color for the base color, glossiness turned over for the
//! roughness, a displacement for the height). A normal map's green is turned over where the
//! program's normal maps are not those the template writes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::plan::{Map, Planned, PlannedFile, Source, Usage};

/// What files an export writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub name: String,
    #[serde(default)]
    pub about: String,
    /// The normal maps it writes: DirectX's (Y−), or OpenGL's (Y+).
    #[serde(default)]
    pub normals: Normals,
    pub files: Vec<File>,
}

/// Which way a normal map's green points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Normals {
    /// Y+: OpenGL's, Unity's, glTF's, Blender's.
    #[default]
    OpenGL,
    /// Y−: DirectX's, Unreal Engine's.
    DirectX,
}

/// What a file is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    Png8,
    Png16,
    Exr16,
    Exr32,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Png8, Format::Png16, Format::Exr16, Format::Exr32];

    pub fn name(self) -> &'static str {
        match self {
            Format::Png8 => "PNG 8 bits",
            Format::Png16 => "PNG 16 bits",
            Format::Exr16 => "EXR 16 bits float",
            Format::Exr32 => "EXR 32 bits float",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Png8 | Format::Png16 => "png",
            Format::Exr16 | Format::Exr32 => "exr",
        }
    }
}

/// A file a template writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// Its name, without extension: `$(graph)` and the other parts of [`crate::naming`] stand
    /// for what is exported.
    pub name: String,
    /// It is written where the program has a map of one of these (or of one that stands for it);
    /// where it says none, of one its parts are made of; where they are values alone, always.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Usage>,
    /// What it is written as, where not as the export says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    /// Its channels, in order: gray of one; red, green and blue of three; and alpha of four.
    pub parts: Vec<Part>,
}

impl File {
    /// How many channels it has.
    pub fn channels(&self) -> usize {
        self.parts.iter().map(Part::channels).sum()
    }

    /// The maps it needs: those it says, or else those its parts are made of.
    pub fn needed(&self) -> Vec<Usage> {
        if !self.needs.is_empty() {
            return self.needs.clone();
        }
        let mut used = Vec::new();
        for part in &self.parts {
            if let Part::Rgb(input) | Part::One(input) = part
                && !used.contains(&input.map)
            {
                used.push(input.map);
            }
        }
        used
    }
}

/// Channels of a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    /// A map's red, green and blue (of a gray map, its gray three times).
    Rgb(Input),
    /// One channel of a map.
    One(Input),
    /// A value everywhere, in one channel, or in three where `rgb`; and what it is, said (where
    /// it says nothing, it is a channel no map is given yet).
    Value {
        value: f32,
        #[serde(default)]
        said: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rgb: bool,
    },
}

impl Part {
    /// How many channels of its file it fills.
    pub fn channels(&self) -> usize {
        match self {
            Part::Rgb(_) | Part::Value { rgb: true, .. } => 3,
            Part::One(_) | Part::Value { .. } => 1,
        }
    }

    /// A part of `channels` no map is given yet: 0 in each.
    pub fn empty(channels: usize) -> Part {
        Part::Value { value: 0.0, said: String::new(), rgb: channels == 3 }
    }
}

/// A map a part is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub map: Usage,
    /// Of `one`: the map's channel, 0 red … 3 alpha.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub channel: u8,
    /// Turned over: 1 − it (smoothness of a roughness).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// What it is where the program has no such map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<f32>,
    /// Left out where the program has no such map (the base color's alpha of an opacity).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl Input {
    /// A map's part, as it is: no channel but the first, not turned over, and the file not
    /// written without it.
    pub fn of(map: Usage) -> Input {
        Input { map, channel: 0, invert: false, missing: None, optional: false }
    }
}

/// What a part of a map is where the program has no such map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Missing {
    /// A value in its place.
    Value(f32),
    /// The part is left out.
    LeaveOut,
    /// The file is not written.
    NoFile,
}

/// A change to a template: what the window (or anyone) does to one.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Name(String),
    About(String),
    Normals(Normals),
    /// A file of so many channels, as `Layout` lays them out, its parts no map yet.
    AddFile(Layout),
    RemoveFile(usize),
    /// The file `from` put at `to`.
    MoveFile {
        from: usize,
        to: usize,
    },
    FileName {
        file: usize,
        name: String,
    },
    Format {
        file: usize,
        format: Option<Format>,
    },
    /// The part made of a map (`Some`; turned over where `invert`: smoothness of a roughness), or
    /// a value (`None`).
    Map {
        file: usize,
        part: usize,
        map: Option<Usage>,
        invert: bool,
    },
    Channel {
        file: usize,
        part: usize,
        channel: u8,
    },
    Invert {
        file: usize,
        part: usize,
        invert: bool,
    },
    Missing {
        file: usize,
        part: usize,
        missing: Missing,
    },
    Value {
        file: usize,
        part: usize,
        value: f32,
    },
}

/// How the channels of a new file are laid out: as Substance Painter offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// One channel.
    Gray,
    /// A map's red, green and blue.
    Rgb,
    /// Red, green and blue, each of a map of its own.
    Separate,
    /// A map's red, green and blue, and alpha of another.
    RgbAlpha,
    /// Red, green, blue and alpha, each of a map of its own.
    SeparateAlpha,
}

impl Layout {
    pub const ALL: [Layout; 5] = [Layout::Gray, Layout::Rgb, Layout::Separate, Layout::RgbAlpha, Layout::SeparateAlpha];

    /// As Substance Painter names it.
    pub fn name(self) -> &'static str {
        match self {
            Layout::Gray => "Gray",
            Layout::Rgb => "RGB",
            Layout::Separate => "R+G+B",
            Layout::RgbAlpha => "RGB+A",
            Layout::SeparateAlpha => "R+G+B+A",
        }
    }

    fn parts(self) -> Vec<Part> {
        match self {
            Layout::Gray => vec![Part::empty(1)],
            Layout::Rgb => vec![Part::empty(3)],
            Layout::Separate => vec![Part::empty(1), Part::empty(1), Part::empty(1)],
            Layout::RgbAlpha => vec![Part::empty(3), Part::empty(1)],
            Layout::SeparateAlpha => vec![Part::empty(1), Part::empty(1), Part::empty(1), Part::empty(1)],
        }
    }
}

impl Template {
    /// Make `edit`; what it names that is not there changes nothing, and says so.
    pub fn apply(&mut self, edit: Edit) -> Result<(), String> {
        let there = |files: &Vec<File>, at: usize| if at < files.len() { Ok(()) } else { Err(format!("the template has no file {}", at + 1)) };
        // (A part's map, where it is one.)
        fn input(file: &mut File, at: usize) -> Result<&mut Input, String> {
            match file.parts.get_mut(at) {
                Some(Part::Rgb(input) | Part::One(input)) => Ok(input),
                Some(Part::Value { .. }) => Err("that channel is a value, not a map".into()),
                None => Err(format!("the file has no part {}", at + 1)),
            }
        }
        match edit {
            Edit::Name(name) => self.name = name,
            Edit::About(about) => self.about = about,
            Edit::Normals(normals) => self.normals = normals,
            Edit::AddFile(layout) => {
                let name = format!("$(graph)_{}", self.files.len() + 1);
                self.files.push(File { name, needs: Vec::new(), format: None, parts: layout.parts() });
            }
            Edit::RemoveFile(at) => {
                there(&self.files, at)?;
                self.files.remove(at);
            }
            Edit::MoveFile { from, to } => {
                there(&self.files, from)?;
                let moved = self.files.remove(from);
                self.files.insert(to.min(self.files.len()), moved);
            }
            Edit::FileName { file: at, name } => self.files.get_mut(at).ok_or("no such file")?.name = name,
            Edit::Format { file: at, format } => self.files.get_mut(at).ok_or("no such file")?.format = format,
            Edit::Map { file: at, part, map, invert } => {
                let file = self.files.get_mut(at).ok_or("no such file")?;
                let slot = file.parts.get_mut(part).ok_or("no such part")?;
                let channels = slot.channels();
                *slot = match map {
                    Some(map) if channels == 3 => Part::Rgb(Input { invert, ..Input::of(map) }),
                    Some(map) => Part::One(Input { invert, ..Input::of(map) }),
                    None => Part::Value { value: 0.0, said: "value".into(), rgb: channels == 3 },
                };
                // (What it needs is what it is made of, now.)
                file.needs.clear();
            }
            Edit::Channel { file: at, part, channel } => {
                let file = self.files.get_mut(at).ok_or("no such file")?;
                if !matches!(file.parts.get(part), Some(Part::One(_))) {
                    return Err("only a channel of one is taken of a map".into());
                }
                input(file, part)?.channel = channel.min(3);
            }
            Edit::Invert { file: at, part, invert } => input(self.files.get_mut(at).ok_or("no such file")?, part)?.invert = invert,
            Edit::Missing { file: at, part, missing } => {
                let input = input(self.files.get_mut(at).ok_or("no such file")?, part)?;
                (input.missing, input.optional) = match missing {
                    Missing::Value(value) => (Some(value.clamp(0.0, 1.0)), false),
                    Missing::LeaveOut => (None, true),
                    Missing::NoFile => (None, false),
                };
            }
            Edit::Value { file: at, part, value } => match self.files.get_mut(at).ok_or("no such file")?.parts.get_mut(part) {
                Some(Part::Value { value: v, .. }) => *v = value.clamp(0.0, 1.0),
                _ => return Err("that channel is a map, not a value".into()),
            },
        }
        Ok(())
    }
}

/// The files `template` writes of `maps`, named with `values` for the parts of names (see
/// [`crate::naming`]: `("graph", "Rusty Metal")`), the program's normal maps being DirectX's
/// where `directx`. Of several maps of a usage, the first is taken.
pub fn plan_template<R: Copy>(template: &Template, maps: &[Map<R>], values: &[(&str, &str)], directx: bool) -> Vec<PlannedFile<R>> {
    // The map of a usage, or where the program has none, of one that stands for it, and whether
    // it is turned over to be it.
    let of = |usage: Usage| -> Option<(&Map<R>, bool)> {
        let find = |u: Usage| maps.iter().find(|m| m.usage == Some(u));
        find(usage).map(|m| (m, false)).or_else(|| match usage {
            Usage::BaseColor => find(Usage::Diffuse).map(|m| (m, false)),
            Usage::Roughness => find(Usage::Glossiness).map(|m| (m, true)),
            Usage::Height => find(Usage::Displacement).map(|m| (m, false)),
            _ => None,
        })
    };
    let flip = directx != (template.normals == Normals::DirectX);
    let mut files = Vec::new();
    'files: for file in &template.files {
        let needs = file.needed();
        if !needs.is_empty() && !needs.iter().any(|&u| of(u).is_some()) {
            continue;
        }
        let mut sources: Vec<Source<R>> = Vec::new();
        // What each part is, said, and the channels it fills.
        let mut said: Vec<(usize, String)> = Vec::new();
        let mut flipped = false;
        for part in &file.parts {
            match part {
                Part::Value { value, said: what, rgb } => {
                    let channels = if *rgb { 3 } else { 1 };
                    sources.extend(std::iter::repeat_n(Source::Value(*value), channels));
                    let what = if what.is_empty() { number(*value) } else { format!("{} ({what})", number(*value)) };
                    said.push((channels, what));
                }
                Part::Rgb(input) | Part::One(input) => {
                    let rgb = matches!(part, Part::Rgb(_));
                    let channels = if rgb { 3 } else { 1 };
                    let Some((map, over)) = of(input.map) else {
                        match (input.missing, input.optional) {
                            (_, true) => continue,
                            (Some(value), false) => {
                                sources.extend(std::iter::repeat_n(Source::Value(value), channels));
                                said.push((channels, format!("{} (no {})", number(value), input.map.word())));
                                continue;
                            }
                            (None, false) => continue 'files,
                        }
                    };
                    let invert = input.invert != over;
                    if rgb {
                        // A map's color, or a gray map's gray in all three.
                        let gray = input.map.gray();
                        for channel in 0..3 {
                            let turn = input.map == Usage::Normal && flip && channel == 1;
                            flipped |= turn;
                            let channel = if gray { 0 } else { channel };
                            sources.push(Source::Map { map: map.reference, channel, invert: invert != turn });
                        }
                    } else {
                        let turn = input.map == Usage::Normal && flip && input.channel == 1;
                        flipped |= turn;
                        sources.push(Source::Map { map: map.reference, channel: input.channel.min(3), invert: invert != turn });
                    }
                    said.push((channels, format!("{}{}", if invert { "1 − " } else { "" }, map.name)));
                }
            }
        }
        // (A file of two channels, of a part left out: PNG has none such.)
        if !matches!(sources.len(), 1 | 3 | 4) {
            continue;
        }
        let mut told = if said.len() == 1 {
            said.remove(0).1
        } else {
            let mut letters = ["R", "G", "B", "A"].into_iter();
            let mut parts = Vec::new();
            for (channels, what) in said {
                let tag: String = (0..channels).filter_map(|_| letters.next()).collect();
                parts.push(format!("{tag} {what}"));
            }
            parts.join(" · ")
        };
        if flipped {
            told.push_str(" (green turned over)");
        }
        files.push(PlannedFile { name: crate::naming::expand(&file.name, values), what: Planned::Pack(sources), said: told, format: file.format });
    }
    files
}

/// A value as a file says it: 1, 0.5, 0.8.
fn number(value: f32) -> String {
    format!("{value}")
}

/// The templates of someone's own in `folder` (each a file of JSON, `.json`), each with its
/// file, in the order of their files' names; and what is wrong with those that could not be
/// read. A folder that is not there has none.
pub fn read_templates(folder: &Path) -> (Vec<(PathBuf, Template)>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return (Vec::new(), Vec::new()) };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))).collect();
    paths.sort();
    let mut templates = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| template_of(&text)) {
            Ok(template) => templates.push((path, template)),
            Err(e) => problems.push(format!("{file}: {e}")),
        }
    }
    (templates, problems)
}

/// Where a new template called `name` is kept in `folder`: its name made a file's, numbered
/// where another file has it.
pub fn new_template_path(folder: &Path, name: &str) -> PathBuf {
    let base = crate::sanitize_file_name(name);
    let mut path = folder.join(format!("{base}.json"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{base} {n}.json"));
        n += 1;
    }
    path
}

/// Write `template` to `path`, as JSON a person reads (the folder made where it is not).
pub fn write_template(path: &Path, template: &Template) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let text = serde_json::to_string_pretty(template).map_err(std::io::Error::other)?;
    std::fs::write(path, text + "\n")
}

/// The template a text of JSON says, checked: files that have a name and 1, 3 or 4 channels.
pub fn template_of(text: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if template.name.trim().is_empty() {
        return Err("the template has no name".into());
    }
    for file in &template.files {
        if file.name.trim().is_empty() || !matches!(file.channels(), 1 | 3 | 4) {
            return Err(format!("the file '{}' is to have a name and 1, 3 or 4 channels", file.name));
        }
    }
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Preset;

    fn map(reference: u32, name: &str, usage: Usage) -> Map<u32> {
        Map { reference, name: name.into(), usage: Some(usage) }
    }

    /// A template of someone's own, as a file of JSON says it: Unreal's base color and normal,
    /// and occlusion, roughness, metallic and height packed into one file (a value where there
    /// is no map, the height's file not written without the maps it needs), named as it says.
    #[test]
    fn a_template_of_ones_own_writes_what_it_says() {
        let text = r#"{
            "name": "ORMH",
            "about": "Occlusion, roughness, metallic and height in one file.",
            "normals": "DirectX",
            "files": [
                { "name": "$(graph)_N", "needs": ["Normal"], "parts": [{ "rgb": { "map": "Normal" } }] },
                {
                    "name": "$(graph)_ORMH",
                    "needs": ["AmbientOcclusion", "Roughness", "Metallic", "Height"],
                    "parts": [
                        { "one": { "map": "AmbientOcclusion", "missing": 1.0 } },
                        { "one": { "map": "Roughness", "missing": 0.5 } },
                        { "one": { "map": "Metallic", "missing": 0.0 } },
                        { "one": { "map": "Height", "missing": 0.5 } }
                    ]
                },
                { "name": "$(graph)_Emissive", "needs": ["Emissive"], "parts": [{ "rgb": { "map": "Emissive" } }] }
            ]
        }"#;
        let template = template_of(text).expect("a template");
        let maps = [map(1, "normal", Usage::Normal), map(2, "rough", Usage::Roughness), map(3, "height", Usage::Height)];
        let files = plan_template(&template, &maps, &[("graph", "Old Brick")], false);
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Old_Brick_N", "Old_Brick_ORMH"], "no emissive map, no file of it");
        let source = |map, channel, invert| Source::Map { map, channel, invert };
        assert_eq!(files[0].what, Planned::Pack(vec![source(1, 0, false), source(1, 1, true), source(1, 2, false)]), "OpenGL's made DirectX's");
        assert_eq!(files[0].said, "normal (green turned over)");
        assert_eq!(files[1].what, Planned::Pack(vec![Source::Value(1.0), source(2, 0, false), Source::Value(0.0), source(3, 0, false)]));
        assert_eq!(files[1].said, "R 1 (no occlusion) · G rough · B 0 (no metallic) · A height");
        // Written as it was read.
        assert_eq!(template_of(&serde_json::to_string(&template).expect("JSON")), Ok(template));
    }

    /// What is wrong with a template is said, and nothing of it is taken: not JSON, a part of no
    /// known kind, a file of two channels, a file that needs no map, no name.
    #[test]
    fn a_template_that_cannot_be_read_says_why() {
        let file = |parts: &str, needs: &str| format!(r#"{{ "name": "T", "files": [{{ "name": "x", "needs": [{needs}], "parts": [{parts}] }}] }}"#);
        let one = r#"{ "one": { "map": "Height" } }"#;
        for (text, said) in [
            ("{".to_string(), "EOF"),
            (file(r#"{ "two": { "map": "Height" } }"#, r#""Height""#), "unknown variant"),
            (file(&format!("{one}, {one}"), r#""Height""#), "1, 3 or 4 channels"),
            (file(one, r#""Heights""#), "unknown variant"),
            (r#"{ "name": " ", "files": [] }"#.to_string(), "no name"),
        ] {
            let error = template_of(&text).expect_err(&text);
            assert!(error.contains(said), "{text}: {error}");
        }
    }

    /// The templates of a folder are read, by the names of their files; one that cannot be read
    /// is said, by its file's name, and the others are read all the same.
    #[test]
    fn templates_are_read_from_a_folder() {
        let folder = std::env::temp_dir().join(format!("texture-export-templates-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let unreal = serde_json::to_string(Preset::Unreal.template().expect("Unreal's")).expect("JSON");
        std::fs::write(folder.join("b.json"), &unreal).expect("written");
        std::fs::write(folder.join("a.json"), unreal.replace("Unreal Engine", "Mine")).expect("written");
        std::fs::write(folder.join("c.json"), "not JSON").expect("written");
        std::fs::write(folder.join("notes.txt"), "not a template").expect("written");
        let (templates, problems) = read_templates(&folder);
        assert_eq!(templates.iter().map(|(_, t)| t.name.as_str()).collect::<Vec<_>>(), ["Mine", "Unreal Engine"]);
        assert_eq!(templates[0].0, folder.join("a.json"), "each with its file");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("c.json: "), "{problems:?}");
        assert_eq!(read_templates(&folder.join("not there")), (Vec::new(), Vec::new()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// What a file of a preset is made of is said channel by channel: the gray of a map in three
    /// of them as RGB, a value with what it stands for.
    #[test]
    fn packed_files_say_what_each_channel_is() {
        let maps = [map(1, "metal", Usage::Metallic), map(2, "rough", Usage::Roughness)];
        let urp = crate::plan(&maps, "x", Preset::UnityUrp, false);
        assert_eq!(urp[0].said, "RGB metal · A 1 − rough");
        let hdrp = crate::plan(&maps, "x", Preset::UnityHdrp, false);
        assert_eq!(hdrp[0].said, "R metal · G 1 (no occlusion) · B 1 (detail mask) · A 1 − rough");
        let base = crate::plan(&[map(3, "alpha", Usage::Opacity)], "x", Preset::Gltf, false);
        assert_eq!(base[0].said, "RGB 0.8 (no base color) · A alpha");
    }
}

#[cfg(test)]
mod edits {
    use super::*;

    /// A template made by edits, as the window makes one: a file laid out as asked, its channels
    /// given maps (one turned over, one left out where there is none, one of a value where there
    /// is none), its name, its format; files moved and removed; what names nothing said and
    /// changing nothing. Kept in a file and read back, it is the same.
    #[test]
    fn templates_are_made_by_edits_and_kept() {
        let mut t = Template { name: "Mine".into(), about: String::new(), normals: Normals::OpenGL, files: Vec::new() };
        t.apply(Edit::AddFile(Layout::RgbAlpha)).expect("a file");
        t.apply(Edit::AddFile(Layout::Separate)).expect("another");
        assert_eq!(t.files.iter().map(File::channels).collect::<Vec<_>>(), [4, 3]);
        t.apply(Edit::Map { file: 0, part: 0, map: Some(Usage::BaseColor), invert: false }).expect("RGB of a map");
        t.apply(Edit::Map { file: 0, part: 1, map: Some(Usage::Opacity), invert: false }).expect("alpha of a map");
        t.apply(Edit::Missing { file: 0, part: 1, missing: Missing::LeaveOut }).expect("left out");
        t.apply(Edit::Map { file: 1, part: 0, map: Some(Usage::Roughness), invert: true }).expect("smoothness");
        t.apply(Edit::Missing { file: 1, part: 0, missing: Missing::Value(2.0) }).expect("a value");
        t.apply(Edit::FileName { file: 1, name: "$(graph)_S".into() }).expect("a name");
        t.apply(Edit::Format { file: 1, format: Some(Format::Exr32) }).expect("a format");
        assert!(matches!(&t.files[0].parts[0], Part::Rgb(input) if input.map == Usage::BaseColor));
        assert!(matches!(&t.files[1].parts[0], Part::One(Input { map: Usage::Roughness, invert: true, missing: Some(1.0), .. })), "kept within 0 … 1");
        assert_eq!(t.files[0].needed(), [Usage::BaseColor, Usage::Opacity], "what it needs is what it is made of");
        assert!(t.apply(Edit::Channel { file: 0, part: 0, channel: 2 }).is_err(), "a channel of one alone");
        assert!(t.apply(Edit::Value { file: 1, part: 0, value: 0.5 }).is_err(), "a map is not a value");
        assert!(t.apply(Edit::RemoveFile(7)).is_err());
        // Planned: base color without its alpha where there is no opacity; the smoothness of the
        // roughness; a file of a map the graph has not, not written.
        let color = Map { reference: 1u32, name: "color".into(), usage: Some(Usage::BaseColor) };
        let rough = Map { reference: 2u32, name: "rough".into(), usage: Some(Usage::Roughness) };
        let files = plan_template(&t, &[color.clone(), rough], &[("graph", "Wall")], false);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].said, "color");
        assert_eq!((files[1].name.as_str(), files[1].format), ("Wall_S", Some(Format::Exr32)));
        assert_eq!(files[1].said, "R 1 − rough · G 0 · B 0", "the others no map yet");
        assert_eq!(plan_template(&t, &[color], &[("graph", "Wall")], false).len(), 1, "no roughness, no file of it");
        t.apply(Edit::MoveFile { from: 1, to: 0 }).expect("moved");
        assert_eq!(t.files[0].name, "$(graph)_S");
        // Kept, and read back the same.
        let folder = std::env::temp_dir().join(format!("texture-export-kept-{}", std::process::id()));
        let path = new_template_path(&folder, &t.name);
        write_template(&path, &t).expect("written");
        assert_eq!(new_template_path(&folder, &t.name), folder.join("Mine 2.json"), "another of the name gets a number");
        let (read, problems) = read_templates(&folder);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(read, [(path, t)]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
