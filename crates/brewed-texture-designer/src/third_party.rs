//! What the program is made with of others' work that asks to be named: what Help → Third-Party
//! Notices shows (see `view::notices`). The model of the Matball, the font of the interface,
//! Rust's standard library, and the crates built into the program whose licenses ask the
//! program to carry their notices.
//!
//! Of the crates, only what is in the program as it is built for Windows, Linux and macOS (not
//! what is built only for other systems, nor what only builds it: build scripts, procedural
//! macros, the tests' crates), and of a crate that may be taken under one license or another,
//! the license that asks the least: one that asks nothing of a program built of it (zlib's, the
//! Unlicense, 0BSD, CC0, Boost's) leaves the crate out; Apache's asks a program only for a copy
//! of itself and the notices of the crate's NOTICE file, where it has one — one copy for every
//! crate taken under it — where MIT's asks for each crate's copyright notice. Crates whose
//! notices are the same text (all those under Apache's, the crates of one author) are one entry,
//! each crate once with all its versions.
//!
//! The crates' notices are read out of the crates themselves and kept, deflated, in
//! assets/third-party/crates.json.z: `cargo run --features dev-tools -- --third-party` writes it
//! again (see [`write_crates`]), and a test sees that it was written of the Cargo.lock there is.

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

/// A work of others the program is made with (of the crates: those that carry the same notice).
#[derive(Clone, Debug)]
pub struct Work {
    pub kind: Kind,
    pub name: String,
    pub version: String,
    /// Who made it.
    pub by: String,
    /// The license it is used under.
    pub license: String,
    /// Where it is (the crate's repository, the model's page).
    pub link: String,
    /// What the program uses it for, or what was changed of it (a work under Creative Commons
    /// Attribution says what was changed); of crates, which they are.
    pub note: String,
    /// Its notices and the texts of its license (in [`Notices::texts`]).
    pub texts: Vec<usize>,
}

pub struct Notices {
    pub works: Vec<Work>,
    pub texts: Vec<String>,
}

/// Crates that carry the same notices, as assets/third-party/crates.json.z keeps them: each with
/// its versions (all of them in the program) and its authors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Group {
    crates: Vec<(String, Vec<String>, Vec<String>)>,
    license: String,
    #[serde(default)]
    repository: String,
    texts: Vec<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Crates {
    texts: Vec<String>,
    groups: Vec<Group>,
    /// Every package of Cargo.lock the notices were read of (name@version), in the program or
    /// not: what the test compares Cargo.lock with.
    #[serde(default)]
    read_of: Vec<String>,
}

const CRATES: &[u8] = include_bytes!("../assets/third-party/crates.json.z");
const INTER_LICENSE: &str = include_str!("../assets/fonts/Inter-LICENSE.txt");
/// The text of the license Rust's standard library is taken under here (it may be taken under
/// MIT's or Apache's: Apache's asks the least), as its toolchain gives it.
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
                license: "Apache-2.0".into(),
                link: "https://github.com/rust-lang/rust".into(),
                note: "The language the program is written in: its standard library is built into the program.".into(),
                texts: vec![text(APACHE)],
            },
        ];
        let mut groups = crates.groups;
        groups.sort_by_key(|g| g.crates.first().map(|(n, ..)| n.to_lowercase()).unwrap_or_default());
        works.extend(groups.into_iter().map(|g| {
            let names: Vec<&str> = g.crates.iter().map(|(n, ..)| n.as_str()).collect();
            let name = match names.as_slice() {
                [one] => one.to_string(),
                [a, b] => format!("{a}, {b}"),
                // (Those under Apache's license with nothing more to say: most of them.)
                many if many.len() > 12 => format!("{} crates under {}", many.len(), g.license),
                [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
                [] => String::new(),
            };
            let version = match g.crates.as_slice() {
                [(_, versions, _)] => versions.join(", "),
                _ => String::new(),
            };
            // Who made them, each once.
            let mut by: Vec<&str> = Vec::new();
            for (_, _, authors) in &g.crates {
                for a in authors {
                    if !by.contains(&a.as_str()) {
                        by.push(a);
                    }
                }
            }
            let note = if g.crates.len() > 1 {
                let listed: Vec<String> = g
                    .crates
                    .iter()
                    .map(|(n, v, a)| if a.is_empty() { format!("{n} {}", v.join(", ")) } else { format!("{n} {} ({})", v.join(", "), a.join(", ")) })
                    .collect();
                format!("These crates, which carry the same notices: {}.", listed.join("; "))
            } else {
                String::new()
            };
            Work { kind: Kind::Software, name, version, by: by.join(", "), license: g.license, link: g.repository, note, texts: g.texts }
        }));
        Notices { works, texts }
    })
}

// ---------------------------------------------------------------------------- reading the crates

/// Licenses that ask nothing of a program built of what is under them: no notice to carry
/// (zlib's asks it of sources only, Boost's not of machine code).
#[cfg(any(test, feature = "dev-tools"))]
const ASK_NOTHING: &[&str] = &["Zlib", "Unlicense", "0BSD", "CC0-1.0", "BSL-1.0", "MIT-0", "WTFPL"];

/// How much a license asks a program built of it to carry: the least first (Apache's, a copy of
/// itself, one for every crate under it, and a crate's NOTICE where it has one; MIT's and ISC's,
/// each crate's copyright notice; BSD's, that and its conditions; Unicode's; MPL's, where its
/// sources are).
#[cfg(any(test, feature = "dev-tools"))]
fn asks(id: &str) -> u32 {
    match id {
        i if ASK_NOTHING.contains(&i) => 0,
        "Apache-2.0" => 1,
        "MIT" | "ISC" => 2,
        "BSD-2-Clause" | "BSD-3-Clause" => 3,
        "Unicode-3.0" | "Unicode-DFS-2016" => 4,
        "MPL-2.0" => 5,
        _ => 9,
    }
}

/// An SPDX expression as the choices it gives: each the licenses to take together (`/` read
/// as OR, as old crates write it; a license WITH an exception as the license).
#[cfg(any(test, feature = "dev-tools"))]
fn choices(expression: &str) -> Vec<Vec<String>> {
    fn or(t: &[&str], i: &mut usize) -> Vec<Vec<String>> {
        let mut out = and(t, i);
        while t.get(*i) == Some(&"OR") {
            *i += 1;
            out.extend(and(t, i));
        }
        out
    }
    fn and(t: &[&str], i: &mut usize) -> Vec<Vec<String>> {
        let mut out = atom(t, i);
        while t.get(*i) == Some(&"AND") {
            *i += 1;
            let next = atom(t, i);
            out = out.iter().flat_map(|a| next.iter().map(move |b| a.iter().chain(b).cloned().collect())).collect();
        }
        out
    }
    fn atom(t: &[&str], i: &mut usize) -> Vec<Vec<String>> {
        match t.get(*i) {
            Some(&"(") => {
                *i += 1;
                let inner = or(t, i);
                if t.get(*i) == Some(&")") {
                    *i += 1;
                }
                inner
            }
            Some(id) => {
                *i += 1;
                if t.get(*i) == Some(&"WITH") {
                    *i += 2;
                }
                vec![vec![id.to_string()]]
            }
            None => vec![Vec::new()],
        }
    }
    let spaced = expression.replace('/', " OR ").replace('(', " ( ").replace(')', " ) ");
    let tokens: Vec<&str> = spaced.split_whitespace().collect();
    let mut i = 0;
    or(&tokens, &mut i)
}

/// What license a license file is the text of, by its name or by what it says (`None`: a file
/// of notices, of copyrights, or of something else).
#[cfg(any(test, feature = "dev-tools"))]
fn license_of(file: &str, text: &str) -> Option<&'static str> {
    let name = file.to_uppercase();
    let hints = [
        ("UNLICENSE", "Unlicense"),
        ("MIT", "MIT"),
        ("APACHE", "Apache-2.0"),
        ("ZLIB", "Zlib"),
        ("BSD", "BSD-3-Clause"),
        ("ISC", "ISC"),
        ("UNICODE", "Unicode-3.0"),
        ("MPL", "MPL-2.0"),
        ("BOOST", "BSL-1.0"),
        ("BSL", "BSL-1.0"),
        ("CC0", "CC0-1.0"),
    ];
    if let Some((_, id)) = hints.iter().find(|(hint, _)| name.contains(hint)) {
        return Some(id);
    }
    let said = |w: &str| text.contains(w);
    if said("Permission is hereby granted, free of charge") {
        Some("MIT")
    } else if said("Apache License") && said("Version 2.0") {
        Some("Apache-2.0")
    } else if said("Redistribution and use in source and binary forms") {
        Some("BSD-3-Clause")
    } else if said("This software is provided 'as-is'") {
        Some("Zlib")
    } else if said("Permission to use, copy, modify, and/or distribute this software") {
        Some(if said("provided that the above copyright notice") { "ISC" } else { "0BSD" })
    } else if said("UNICODE LICENSE") || said("Unicode, Inc.") {
        Some("Unicode-3.0")
    } else if said("Mozilla Public License") {
        Some("MPL-2.0")
    } else if said("Boost Software License") {
        Some("BSL-1.0")
    } else if said("free and unencumbered software released into the public domain") {
        Some("Unlicense")
    } else if said("CC0 1.0") {
        Some("CC0-1.0")
    } else {
        None
    }
}

/// Read the notices of the crates built into the program for Windows, Linux and macOS out of
/// the crates themselves, as `cargo metadata` finds them, and write them to `out` (see the
/// module): of each crate, under the license that asks the least of its choices (of those whose
/// texts it has, if it has the texts of one), those texts — its license files of those licenses,
/// its COPYRIGHT, its NOTICE with Apache's — or where it has none, the standard text (from
/// assets/third-party, or from `licenses`, the toolchain's), MIT's with its authors.
#[cfg(feature = "dev-tools")]
pub fn write_crates(out: &std::path::Path, licenses: &std::path::Path) -> Result<(), String> {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::io::Write;
    const PLATFORMS: [&str; 4] = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"];
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let metadata = |platform: Option<&str>| -> Result<serde_json::Value, String> {
        let mut args = vec!["metadata", "--format-version", "1", "--locked", "--manifest-path", manifest];
        if let Some(p) = platform {
            args.extend(["--filter-platform", p]);
        }
        let output = std::process::Command::new("cargo").args(&args).output().map_err(|e| format!("cargo metadata: {e}"))?;
        if !output.status.success() {
            return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr)));
        }
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
    };
    // What the program is built of on each system: from the program, along what each crate needs
    // to run (not to build, nor to test), and not into procedural macros.
    let mut linked: BTreeSet<String> = BTreeSet::new();
    let mut packages: HashMap<String, serde_json::Value> = HashMap::new();
    for platform in PLATFORMS {
        let m = metadata(Some(platform))?;
        for p in m["packages"].as_array().ok_or("no packages")? {
            packages.insert(p["id"].as_str().unwrap_or_default().to_string(), p.clone());
        }
        let nodes: HashMap<String, &serde_json::Value> =
            m["resolve"]["nodes"].as_array().ok_or("no resolve")?.iter().map(|n| (n["id"].as_str().unwrap_or_default().to_string(), n)).collect();
        let root = packages.values().find(|p| p["name"] == "brewed-texture-designer").and_then(|p| p["id"].as_str()).ok_or("no program")?.to_string();
        let mut todo = vec![root];
        let mut seen = BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let macros = packages.get(&id).and_then(|p| p["targets"].as_array()).is_some_and(|t| {
                t.iter().any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "proc-macro")))
            });
            if macros {
                continue;
            }
            linked.insert(id.clone());
            for dep in nodes.get(&id).and_then(|n| n["deps"].as_array()).into_iter().flatten() {
                if dep["dep_kinds"].as_array().is_some_and(|k| k.iter().any(|k| k["kind"].is_null())) {
                    todo.push(dep["pkg"].as_str().unwrap_or_default().to_string());
                }
            }
        }
    }
    // Every package of Cargo.lock, for the test.
    let all = metadata(None)?;
    let mut read_of: Vec<String> = all["packages"]
        .as_array()
        .ok_or("no packages")?
        .iter()
        .filter(|p| !p["source"].is_null())
        .map(|p| format!("{}@{}", p["name"].as_str().unwrap_or_default(), p["version"].as_str().unwrap_or_default()))
        .collect();
    read_of.sort();
    read_of.dedup();

    let ours = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/third-party");
    let standard = |id: &str| {
        let file = format!("{id}.txt");
        std::fs::read_to_string(ours.join(&file)).or_else(|_| std::fs::read_to_string(licenses.join(&file))).ok()
    };
    let mut all_texts: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut keep = |texts: &mut Vec<String>, t: &str| -> usize {
        let t = t.replace("\r\n", "\n").trim().to_string();
        *index.entry(t.clone()).or_insert_with(|| {
            texts.push(t);
            texts.len() - 1
        })
    };
    // Each crate, all its versions in one: their texts, licenses and authors.
    #[derive(Default)]
    struct Record {
        versions: Vec<String>,
        authors: Vec<String>,
        licenses: BTreeSet<String>,
        texts: BTreeSet<usize>,
        repository: String,
    }
    let mut records: BTreeMap<String, Record> = BTreeMap::new();
    let (mut asking_nothing, mut missing) = (Vec::new(), Vec::new());
    let mut built = 0;
    for id in &linked {
        let p = &packages[id];
        // (The workspace's own crates have no source.)
        if p["source"].is_null() {
            continue;
        }
        built += 1;
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let (name, version, expression) = (s("name"), s("version"), s("license"));
        let options = choices(&expression);
        if options.iter().any(|o| o.iter().all(|l| asks(l) == 0)) {
            asking_nothing.push(format!("{name} {version} ({expression})"));
            continue;
        }
        let dir = std::path::Path::new(p["manifest_path"].as_str().ok_or("no manifest")?).parent().ok_or("no folder")?.to_path_buf();
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|f| {
                let n = f.file_name().map(|n| n.to_string_lossy().to_uppercase()).unwrap_or_default();
                f.is_file() && ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE"].iter().any(|w| n.starts_with(w))
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
        // Its files, each with the license it is the text of.
        let read: Vec<(String, Option<&str>, String)> = files
            .iter()
            .filter_map(|f| {
                let file = f.file_name()?.to_string_lossy().to_string();
                let text = std::fs::read_to_string(f).ok()?;
                let of = license_of(&file, &text);
                Some((file, of, text))
            })
            .collect();
        // (Apache's own text is a copy as good as any crate's.)
        let has = |l: &str| l == "Apache-2.0" || read.iter().any(|(_, of, _)| *of == Some(l));
        let cost = |o: &&Vec<String>| o.iter().map(|l| asks(l)).sum::<u32>();
        let chosen = options
            .iter()
            .filter(|o| o.iter().all(|l| has(l)))
            .min_by_key(cost)
            .or_else(|| options.iter().min_by_key(cost))
            .cloned()
            .unwrap_or_default();
        let authors: Vec<String> =
            p["authors"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        let mut texts = Vec::new();
        for license in &chosen {
            let mut found = false;
            // Apache's: the one copy of it (a crate's own is the same license, but for how it is
            // laid out and its appendix, which is how to apply it).
            if license != "Apache-2.0" {
                for (_, of, text) in &read {
                    if *of == Some(license.as_str()) {
                        texts.push(keep(&mut all_texts, text));
                        found = true;
                    }
                }
            }
            if !found {
                match standard(license) {
                    Some(t) => {
                        let holders = if authors.is_empty() { format!("The {name} developers") } else { authors.join(", ") };
                        texts.push(keep(&mut all_texts, &t.replace("<year> <copyright holders>", &holders)));
                    }
                    None => missing.push(format!("{name} {version}: {license}")),
                }
            }
        }
        // The notices Apache's asks to be kept; who holds the copyright, where a file of its own
        // says it and the license asks for the notice.
        let apache = chosen.iter().any(|l| l == "Apache-2.0");
        let notice = chosen.iter().any(|l| l != "Apache-2.0");
        for (file, of, text) in &read {
            let upper = file.to_uppercase();
            if of.is_none() && ((upper.starts_with("NOTICE") && apache) || (upper.starts_with("COPYRIGHT") && notice)) {
                texts.push(keep(&mut all_texts, text));
            }
        }
        let record = records.entry(name).or_default();
        record.versions.push(version);
        for a in authors {
            if !record.authors.contains(&a) {
                record.authors.push(a);
            }
        }
        record.licenses.insert(chosen.join(" AND "));
        record.texts.extend(texts);
        if record.repository.is_empty() {
            record.repository = s("repository");
        }
    }
    if !missing.is_empty() {
        return Err(format!("no text of: {}", missing.join("; ")));
    }
    // The crates by the texts they carry and their licenses: one entry each.
    let mut groups: BTreeMap<(Vec<usize>, String), Group> = BTreeMap::new();
    for (name, r) in records {
        let texts: Vec<usize> = r.texts.into_iter().collect();
        let license = r.licenses.into_iter().collect::<Vec<_>>().join("; ");
        let group = groups.entry((texts.clone(), license.clone())).or_insert_with(|| Group { license, texts, repository: r.repository, ..Group::default() });
        group.crates.push((name, r.versions, r.authors));
    }
    // Only the texts the entries carry, in the order they are first carried.
    let mut used: Vec<usize> = Vec::new();
    for g in groups.values() {
        for t in &g.texts {
            if !used.contains(t) {
                used.push(*t);
            }
        }
    }
    let crates = Crates {
        texts: used.iter().map(|&t| all_texts[t].clone()).collect(),
        groups: groups
            .into_values()
            .map(|mut g| {
                g.texts = g.texts.iter().map(|t| used.iter().position(|u| u == t).unwrap_or_default()).collect();
                g.crates.sort();
                g
            })
            .collect(),
        read_of,
    };
    let json = serde_json::to_vec(&crates).map_err(|e| e.to_string())?;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&json).map_err(|e| e.to_string())?;
    let bytes = z.finish().map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{built} crates are built into the program; {} of them ask nothing; the rest carry {} notices ({} texts, {} kB deflated), written to {}",
        asking_nothing.len(),
        crates.groups.len(),
        crates.texts.len(),
        bytes.len() / 1024,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The notices were read of the Cargo.lock there is: when a crate is added or changed, they are
    /// read again (`cargo run --features dev-tools -- --third-party`). Every entry has its text;
    /// none is of a system the program is not built for; the model, under CC BY, says its author,
    /// the link to it and to the license, and what was changed of it.
    #[test]
    fn the_notices_are_of_the_crates_there_are() {
        let lock = include_str!("../../../Cargo.lock");
        let mut in_lock: Vec<String> = Vec::new();
        for package in lock.split("[[package]]").skip(1) {
            let field = |k: &str| package.lines().find_map(|l| l.strip_prefix(&format!("{k} = \""))).map(|v| v.trim_end_matches('"').to_string());
            // (The workspace's own crates have no source.)
            if field("source").is_some() {
                in_lock.push(format!("{}@{}", field("name").unwrap_or_default(), field("version").unwrap_or_default()));
            }
        }
        in_lock.sort();
        in_lock.dedup();
        assert_eq!(read_crates().read_of, in_lock, "the notices are of another Cargo.lock: cargo run --features dev-tools -- --third-party");
        let n = notices();
        assert!(n.works.iter().all(|w| !w.texts.is_empty() && w.texts.iter().all(|t| *t < n.texts.len())));
        // (wgpu-core-deps-windows-linux-android is of Windows and Linux too.)
        for elsewhere in ["android-activity", "android_system_properties", "ndk-sys", "jni-sys", "web-sys", "js-sys", "wasm-bindgen"] {
            let said = |w: &Work| w.name.split([',', ' ']).any(|n| n == elsewhere) || w.note.contains(&format!("{elsewhere} "));
            assert!(!n.works.iter().any(said), "{elsewhere}: not built into the program for a computer");
        }
        // One entry for everything under Apache's license with nothing more to say.
        assert!(n.works.iter().filter(|w| w.license == "Apache-2.0" && w.kind == Kind::Software).count() <= 2);
        let ball = &n.works[0];
        assert!(ball.name.contains("Shader Ball JL 01") && ball.by == "Craneo" && ball.link == "https://skfb.ly/ozVxI");
        assert!(n.texts[ball.texts[0]].contains("http://creativecommons.org/licenses/by/4.0/") && ball.note.contains("Changed"));
    }

    /// Of a choice, the license that asks the least is taken; one that asks nothing leaves the
    /// crate out; licenses taken together are all taken.
    #[test]
    fn the_license_that_asks_the_least_is_taken() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(choices("MIT OR Apache-2.0"), [s(&["MIT"]), s(&["Apache-2.0"])]);
        assert_eq!(choices("MIT/Apache-2.0").len(), 2);
        assert_eq!(choices("(MIT OR Apache-2.0) AND Unicode-3.0"), [s(&["MIT", "Unicode-3.0"]), s(&["Apache-2.0", "Unicode-3.0"])]);
        assert_eq!(choices("Apache-2.0 WITH LLVM-exception OR MIT"), [s(&["Apache-2.0"]), s(&["MIT"])]);
        let nothing = |e: &str| choices(e).iter().any(|o| o.iter().all(|l| asks(l) == 0));
        assert!(nothing("Zlib OR Apache-2.0 OR MIT") && nothing("Unlicense OR MIT") && nothing("BSL-1.0") && nothing("0BSD OR MIT OR Apache-2.0"));
        assert!(!nothing("MIT OR Apache-2.0") && !nothing("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert_eq!(license_of("LICENSE-MIT", ""), Some("MIT"));
        assert_eq!(license_of("LICENSE", "Permission is hereby granted, free of charge, to any person"), Some("MIT"));
        assert_eq!(license_of("COPYRIGHT", "Copyrights in the project are retained by their contributors."), None);
    }
}
