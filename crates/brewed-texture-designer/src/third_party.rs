//! What the program is made with of others' work, each under its license: what Help → Third-Party
//! Notices shows (see `view::notices`). The model of the Matball, the font of the interface, Rust's
//! standard library, and every crate the program is built of: their names, versions, authors,
//! licenses and the texts of those licenses, as each crate gives them.
//!
//! The crates' are read out of the crates themselves and kept, deflated, in
//! assets/third-party/crates.json.z: `cargo run --features dev-tools -- --third-party` writes it
//! again (see [`write_crates`]), and a test sees that every package of Cargo.lock is in it.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// What a work is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Font,
    Software,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Model, Kind::Font, Kind::Software];

    pub fn heading(self) -> &'static str {
        match self {
            Kind::Model => "Models",
            Kind::Font => "Fonts",
            Kind::Software => "Software",
        }
    }
}

/// A work of others the program is made with.
#[derive(Clone, Debug)]
pub struct Work {
    pub kind: Kind,
    pub name: String,
    pub version: String,
    /// Who made it.
    pub by: String,
    /// Its license (an SPDX expression, or the license's name).
    pub license: String,
    /// Where it is (the crate's repository, the model's page).
    pub link: String,
    /// What the program uses it for, or what was changed of it (a work under Creative Commons
    /// Attribution says what was changed).
    pub note: String,
    /// Its notices and the texts of its licenses (in [`Notices::texts`]).
    pub texts: Vec<usize>,
}

pub struct Notices {
    pub works: Vec<Work>,
    pub texts: Vec<String>,
}

/// A crate, as assets/third-party/crates.json.z keeps it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Crate {
    name: String,
    version: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    license: String,
    #[serde(default)]
    repository: String,
    texts: Vec<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Crates {
    texts: Vec<String>,
    crates: Vec<Crate>,
}

const CRATES: &[u8] = include_bytes!("../assets/third-party/crates.json.z");
const INTER_LICENSE: &str = include_str!("../assets/fonts/Inter-LICENSE.txt");
/// The texts of the licenses Rust's standard library is under, as its toolchain gives them.
const MIT: &str = include_str!("../assets/third-party/MIT.txt");
const APACHE: &str = include_str!("../assets/third-party/Apache-2.0.txt");

fn read_crates() -> Crates {
    use std::io::Read;
    let mut json = Vec::new();
    match flate2::read::ZlibDecoder::new(CRATES).read_to_end(&mut json).map_err(|e| e.to_string()).and_then(|_| {
        serde_json::from_slice::<Crates>(&json).map_err(|e| e.to_string())
    }) {
        Ok(crates) => crates,
        Err(e) => {
            log::error!("the notices of the crates: {e}");
            Crates::default()
        }
    }
}

/// Every work, in the order the window lists them: the model, the font, the language, then the
/// crates by name.
pub fn notices() -> &'static Notices {
    static NOTICES: OnceLock<Notices> = OnceLock::new();
    NOTICES.get_or_init(|| {
        let crates = read_crates();
        let mut texts = crates.texts;
        let mut text = |t: &str| {
            texts.push(t.trim().to_string());
            texts.len() - 1
        };
        let mut works = vec![
            Work {
                kind: Kind::Model,
                name: "Matball: Shader Ball JL 01".into(),
                version: String::new(),
                by: "Craneo".into(),
                license: "CC BY 4.0".into(),
                link: "https://skfb.ly/ozVxI".into(),
                note: "The Matball of the 3D view. Changed: its modifiers applied and made triangles in Blender, its \
                       tangents worked out, turned to stand up as the 3D view has it, made the size of the view's other \
                       models, its texture coordinates scaled so that a texture is as large on it as on the sphere; its own \
                       textures are not used."
                    .into(),
                texts: vec![text(
                    "\"Shader Ball JL 01\" (https://skfb.ly/ozVxI) by Craneo is licensed under Creative Commons \
                     Attribution (http://creativecommons.org/licenses/by/4.0/).\n\n\
                     Creative Commons Attribution 4.0 International: you may share and adapt the work for any purpose, \
                     even commercially, as long as you give appropriate credit, provide a link to the license, and \
                     indicate if changes were made. The license: https://creativecommons.org/licenses/by/4.0/legalcode",
                )],
            },
            Work {
                kind: Kind::Font,
                name: "Inter".into(),
                version: "4".into(),
                by: "The Inter Project Authors (Rasmus Andersson)".into(),
                license: "OFL-1.1".into(),
                link: "https://github.com/rsms/inter".into(),
                note: "The font of the interface (Inter Variable and its italics), built into the program.".into(),
                texts: vec![text(INTER_LICENSE)],
            },
            Work {
                kind: Kind::Software,
                name: "Rust standard library".into(),
                version: String::new(),
                by: "The Rust Project Developers".into(),
                license: "MIT OR Apache-2.0".into(),
                link: "https://github.com/rust-lang/rust".into(),
                note: "The language the program is written in: its standard library is built into the program."
                    .into(),
                texts: vec![
                    text(&MIT.replace("<year> <copyright holders>", "The Rust Project Developers")),
                    text(APACHE),
                ],
            },
        ];
        let mut crates = crates.crates;
        crates.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.version.cmp(&b.version)));
        works.extend(crates.into_iter().map(|c| Work {
            kind: Kind::Software,
            name: c.name,
            version: c.version,
            by: c.authors.join(", "),
            license: c.license,
            link: c.repository,
            note: String::new(),
            texts: c.texts,
        }));
        Notices { works, texts }
    })
}

/// Read the notices of every crate Cargo.lock names (all those of every platform and feature)
/// out of the crates themselves, as `cargo metadata` finds them, and write them to `out`: their
/// license files (LICENSE*, LICENCE*, COPYING*, COPYRIGHT*, NOTICE*), each text once; where a
/// crate has none, the standard text of each license its expression names (from
/// assets/third-party, or from `licenses`, the toolchain's), MIT's with its authors.
#[cfg(feature = "dev-tools")]
pub fn write_crates(out: &std::path::Path, licenses: &std::path::Path) -> Result<(), String> {
    use std::collections::HashMap;
    use std::io::Write;
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked", "--all-features", "--manifest-path", manifest])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr)));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let ours = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/third-party");
    let standard = |id: &str| {
        let file = format!("{id}.txt");
        std::fs::read_to_string(ours.join(&file)).or_else(|_| std::fs::read_to_string(licenses.join(&file))).ok()
    };
    let mut crates = Crates::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut keep = |texts: &mut Vec<String>, t: String| -> usize {
        let t = t.replace("\r\n", "\n").trim().to_string();
        *index.entry(t.clone()).or_insert_with(|| {
            texts.push(t);
            texts.len() - 1
        })
    };
    let packages = metadata["packages"].as_array().ok_or("no packages")?;
    let mut missing = Vec::new();
    for p in packages {
        // (The workspace's own crates have no source.)
        if p["source"].is_null() {
            continue;
        }
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let dir = std::path::Path::new(p["manifest_path"].as_str().ok_or("no manifest")?).parent().ok_or("no folder")?.to_path_buf();
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|f| {
                let name = f.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
                f.is_file() && ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE"].iter().any(|w| name.starts_with(w))
            })
            .collect();
        for sub in ["LICENSES", "licenses", "license"] {
            if let Ok(entries) = std::fs::read_dir(dir.join(sub)) {
                files.extend(entries.flatten().map(|e| e.path()).filter(|f| f.is_file()));
            }
        }
        if let Some(f) = p["license_file"].as_str() {
            files.push(dir.join(f));
        }
        files.sort();
        files.dedup();
        let authors: Vec<String> =
            p["authors"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut texts = Vec::new();
        for f in files {
            if let Ok(t) = std::fs::read_to_string(&f) {
                texts.push(keep(&mut crates.texts, t));
            }
        }
        if texts.is_empty() {
            // The standard texts of what the expression names (those of the licenses it gives a
            // choice of that have none are not missed while one of them has).
            let mut lacking = Vec::new();
            let license = s("license");
            for id in license.split(|c: char| c == '/' || c == '(' || c == ')' || c.is_whitespace()).filter(|w| !w.is_empty() && *w != "OR" && *w != "AND" && *w != "WITH") {
                match standard(id) {
                    Some(t) => {
                        let holders = if authors.is_empty() { format!("The {} developers", s("name")) } else { authors.join(", ") };
                        texts.push(keep(&mut crates.texts, t.replace("<year> <copyright holders>", &holders)));
                    }
                    None => lacking.push(id.to_string()),
                }
            }
            if texts.is_empty() {
                missing.push(format!("{} {}: {}", s("name"), s("version"), lacking.join(", ")));
            }
        }
        crates.crates.push(Crate { name: s("name"), version: s("version"), authors, license: s("license"), repository: s("repository"), texts });
    }
    if !missing.is_empty() {
        log::warn!("no text of: {}", missing.join("; "));
    }
    crates.crates.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
    let json = serde_json::to_vec(&crates).map_err(|e| e.to_string())?;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&json).map_err(|e| e.to_string())?;
    let bytes = z.finish().map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{} crates, {} texts ({} kB of JSON, {} kB deflated) written to {}",
        crates.crates.len(),
        crates.texts.len(),
        json.len() / 1024,
        bytes.len() / 1024,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every package Cargo.lock names (of every platform: what the program is built of on
    /// Windows, Linux or macOS) has its notices, with a text: the crates were read again when a
    /// crate was added or changed. If not: `cargo run --features dev-tools -- --third-party`.
    #[test]
    fn every_crate_has_its_notices() {
        let lock = include_str!("../../../Cargo.lock");
        let n = notices();
        let mut missing = Vec::new();
        for package in lock.split("[[package]]").skip(1) {
            let field = |k: &str| package.lines().find_map(|l| l.strip_prefix(&format!("{k} = \""))).map(|v| v.trim_end_matches('"').to_string());
            // (The workspace's own crates have no source.)
            if field("source").is_none() {
                continue;
            }
            let (name, version) = (field("name").unwrap_or_default(), field("version").unwrap_or_default());
            match n.works.iter().find(|w| w.kind == Kind::Software && w.name == name && w.version == version) {
                Some(w) if !w.texts.is_empty() => {}
                Some(_) => missing.push(format!("{name} {version} (no text)")),
                None => missing.push(format!("{name} {version}")),
            }
        }
        assert!(missing.is_empty(), "no notices of {missing:?}: cargo run --features dev-tools -- --third-party");
        assert!(n.works.iter().all(|w| w.texts.iter().all(|t| *t < n.texts.len())));
        // The model, under CC BY: its author, the link to it and to the license, what changed.
        let ball = &n.works[0];
        assert!(ball.name.contains("Shader Ball JL 01") && ball.by == "Craneo" && ball.link == "https://skfb.ly/ozVxI");
        assert!(n.texts[ball.texts[0]].contains("http://creativecommons.org/licenses/by/4.0/") && ball.note.contains("Changed"));
    }
}
