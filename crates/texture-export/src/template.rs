//! Templates of an export: what files a preset writes, as data. The presets of the game
//! engines are such templates (in `templates/`, read as the program starts), and so is a
//! template of someone's own: a file of JSON in a folder the program reads (see
//! [`read_templates`]), as Substance Painter's output templates are, in essence.
//!
//! A template is a list of files. Each has a name, where `$name` is what is exported (a
//! graph's name, made a file's); the maps it needs (it is written where the program has a map
//! of one of them); and its parts, in the order of its channels: a map's red, green and blue
//! (`rgb`), one channel of a map (`one`), or a value (`value`). A part of a map the program has
//! not is its `missing` value, or left out where it is `optional`; without either, the file is
//! not written. Where a program has no map of a usage, one that stands for it is taken (a
//! diffuse color for the base color, glossiness turned over for the roughness, a displacement
//! for the height). A normal map's green is turned over where the program's normal maps are
//! not those the template writes.

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

/// A file a template writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// Its name, without extension: `$name` is what is exported.
    pub name: String,
    /// It is written where the program has a map of one of these (or of one that stands for it).
    pub needs: Vec<Usage>,
    /// Its channels, in order: gray of one; red, green and blue of three; and alpha of four.
    pub parts: Vec<Part>,
}

/// Channels of a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    /// A map's red, green and blue (of a gray map, its gray three times).
    Rgb(Input),
    /// One channel of a map.
    One(Input),
    /// A value everywhere, and what it is, said.
    Value { value: f32, said: String },
}

/// A map a part is made of.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub map: Usage,
    /// Of `one`: the map's channel, 0 red … 3 alpha.
    #[serde(default)]
    pub channel: u8,
    /// Turned over: 1 − it (smoothness of a roughness).
    #[serde(default)]
    pub invert: bool,
    /// What it is where the program has no such map.
    #[serde(default)]
    pub missing: Option<f32>,
    /// Left out where the program has no such map (the base color's alpha of an opacity).
    #[serde(default)]
    pub optional: bool,
}

/// The files `template` writes of `maps` (of what is called `name`), the program's normal maps
/// being DirectX's where `directx`. Of several maps of a usage, the first is taken.
pub fn plan_template<R: Copy>(template: &Template, maps: &[Map<R>], name: &str, directx: bool) -> Vec<PlannedFile<R>> {
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
    let stem = crate::file_stem(name);
    let mut files = Vec::new();
    'files: for file in &template.files {
        if !file.needs.iter().any(|&u| of(u).is_some()) {
            continue;
        }
        let mut sources: Vec<Source<R>> = Vec::new();
        // What each part is, said, and the channels it fills.
        let mut said: Vec<(usize, String)> = Vec::new();
        let mut flipped = false;
        for part in &file.parts {
            match part {
                Part::Value { value, said: what } => {
                    sources.push(Source::Value(*value));
                    said.push((1, format!("{} ({what})", number(*value))));
                }
                Part::Rgb(input) | Part::One(input) => {
                    let rgb = matches!(part, Part::Rgb(_));
                    let Some((map, over)) = of(input.map) else {
                        match (input.missing, input.optional) {
                            (_, true) => continue,
                            (Some(value), false) => {
                                sources.extend(std::iter::repeat_n(Source::Value(value), if rgb { 3 } else { 1 }));
                                said.push((if rgb { 3 } else { 1 }, format!("{} (no {})", number(value), input.map.word())));
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
                    said.push((if rgb { 3 } else { 1 }, format!("{}{}", if invert { "1 − " } else { "" }, map.name)));
                }
            }
        }
        if sources.is_empty() {
            continue;
        }
        let what = Planned::Pack(sources);
        let mut told = if said.len() == 1 {
            said.remove(0).1
        } else {
            let mut letters = ["R", "G", "B", "A"].into_iter();
            let mut parts = Vec::new();
            for (channels, what) in said {
                let mut tag = String::new();
                for _ in 0..channels {
                    tag.push_str(letters.next().unwrap_or(""));
                }
                parts.push(format!("{tag} {what}"));
            }
            parts.join(" · ")
        };
        if flipped {
            told.push_str(" (green turned over)");
        }
        files.push(PlannedFile { name: file.name.replace("$name", &stem), what, said: told });
    }
    files
}

/// A value as a file says it: 1, 0.5, 0.8.
fn number(value: f32) -> String {
    format!("{value}")
}

/// The templates of someone's own in `folder` (each a file of JSON, `.json`), by name; and what
/// is wrong with those that could not be read. A folder that is not there has none.
pub fn read_templates(folder: &Path) -> (Vec<Template>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return (Vec::new(), Vec::new()) };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))).collect();
    paths.sort();
    let mut templates = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| template_of(&text)) {
            Ok(template) => templates.push(template),
            Err(e) => problems.push(format!("{file}: {e}")),
        }
    }
    (templates, problems)
}

/// The template a text of JSON says, checked: files that have a name and parts, at most four
/// channels each.
pub fn template_of(text: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if template.name.trim().is_empty() {
        return Err("the template has no name".into());
    }
    for file in &template.files {
        let channels: usize = file.parts.iter().map(|p| if matches!(p, Part::Rgb(_)) { 3 } else { 1 }).sum();
        if file.name.trim().is_empty() || file.parts.is_empty() || channels > 4 || channels == 2 {
            return Err(format!("the file '{}' is to have a name and 1, 3 or 4 channels", file.name));
        }
        if file.needs.is_empty() {
            return Err(format!("the file '{}' needs no map: say which it is written of", file.name));
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
                { "name": "$name_N", "needs": ["Normal"], "parts": [{ "rgb": { "map": "Normal" } }] },
                {
                    "name": "$name_ORMH",
                    "needs": ["AmbientOcclusion", "Roughness", "Metallic", "Height"],
                    "parts": [
                        { "one": { "map": "AmbientOcclusion", "missing": 1.0 } },
                        { "one": { "map": "Roughness", "missing": 0.5 } },
                        { "one": { "map": "Metallic", "missing": 0.0 } },
                        { "one": { "map": "Height", "missing": 0.5 } }
                    ]
                },
                { "name": "$name_Emissive", "needs": ["Emissive"], "parts": [{ "rgb": { "map": "Emissive" } }] }
            ]
        }"#;
        let template = template_of(text).expect("a template");
        let maps = [map(1, "normal", Usage::Normal), map(2, "rough", Usage::Roughness), map(3, "height", Usage::Height)];
        let files = plan_template(&template, &maps, "Old Brick", false);
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
            (file(one, ""), "needs no map"),
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
        assert_eq!(templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Mine", "Unreal Engine"]);
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
