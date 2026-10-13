//! What the program is made with of others' work that asks to be named: what Help → Third-Party
//! Notices lists (see `view::notices`), as programs list it: each work, its license, and a link
//! to where it is. The model of the Matball, the font of the interface, Rust's standard library,
//! and the crates built into the program whose licenses ask for it.
//!
//! Of the crates, only what is in the program as it is built for Windows, Linux and macOS (not
//! what is built only for other systems, nor what only builds it: build scripts, procedural
//! macros, the tests' crates), each crate once, whatever its versions; not one whose license asks
//! nothing of a program built of it (zlib's, the Unlicense, 0BSD, CC0, Boost's), where it may be
//! taken under one.
//!
//! The list is kept, deflated, in assets/third-party/crates.json.z: `cargo run --features
//! dev-tools -- --third-party` writes it again (see [`write_crates`]), and a test sees that it was
//! written of the Cargo.lock there is.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Under what the program itself is, as About says it: free to use, study, change and share,
/// under either license of the two Rust's own crates are under (LICENSE-MIT and LICENSE-APACHE
/// beside the sources; NOTICE names what in it is others').
pub const OWN_LICENSE: &str = "Free and open source, under the MIT License or the Apache License 2.0, as you choose.";

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

/// A work of others the program is made with: what it is, its license, and where it is (a crate's
/// repository, the model's page).
#[derive(Clone, Debug, PartialEq)]
pub struct Work {
    pub kind: Kind,
    pub name: String,
    pub license: String,
    pub link: String,
}

/// The crates, as assets/third-party/crates.json.z keeps them: each its name, license and link.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Crates {
    crates: Vec<(String, String, String)>,
    /// Every package of Cargo.lock the list was made of (name@version), in the program or not:
    /// what the test compares Cargo.lock with.
    read_of: Vec<String>,
}

const CRATES: &[u8] = include_bytes!("../assets/third-party/crates.json.z");

fn read_crates() -> Crates {
    use std::io::Read;
    let mut json = Vec::new();
    match flate2::read::ZlibDecoder::new(CRATES)
        .read_to_end(&mut json)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_slice::<Crates>(&json).map_err(|e| e.to_string()))
    {
        Ok(crates) => crates,
        Err(e) => {
            log::error!("the list of the crates: {e}");
            Crates::default()
        }
    }
}

/// Every work, in the order the window lists them: the model, the font, the language, then the
/// crates by name.
pub fn works() -> &'static [Work] {
    static WORKS: OnceLock<Vec<Work>> = OnceLock::new();
    WORKS.get_or_init(|| {
        let work = |kind, name: &str, license: &str, link: &str| Work { kind, name: name.into(), license: license.into(), link: link.into() };
        let mut works = vec![
            work(Kind::Model, "Shader Ball JL 01 by Craneo (the Matball, changed)", "CC BY 4.0", "https://skfb.ly/ozVxI"),
            work(Kind::Font, "Inter", "OFL-1.1", "https://github.com/rsms/inter"),
            work(Kind::Software, "Rust standard library", "MIT OR Apache-2.0", "https://github.com/rust-lang/rust"),
        ];
        let mut crates = read_crates().crates;
        crates.sort_by_key(|(name, ..)| name.to_lowercase());
        works.extend(crates.into_iter().map(|(name, license, link)| Work { kind: Kind::Software, name, license, link }));
        works
    })
}

// ---------------------------------------------------------------------------- reading the crates

/// Licenses that ask nothing of a program built of what is under them: nothing to say of it
/// (zlib's asks it of sources only, Boost's not of machine code).
#[cfg(any(test, feature = "dev-tools"))]
const ASK_NOTHING: &[&str] = &["Zlib", "Unlicense", "0BSD", "CC0-1.0", "BSL-1.0", "MIT-0", "WTFPL"];

/// The ways a crate may be taken, of its license's SPDX expression: each a set of licenses taken
/// together (A AND B), any one of them (A OR B; MIT/Apache-2.0 as crates once wrote it); WITH an
/// exception is the license it is of.
#[cfg(any(test, feature = "dev-tools"))]
fn choices(expression: &str) -> Vec<Vec<String>> {
    // Or of ands, of the words in brackets first.
    fn parse(words: &[String], at: &mut usize) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        let mut current: Vec<Vec<String>> = vec![Vec::new()];
        while *at < words.len() {
            let w = words[*at].clone();
            *at += 1;
            match w.as_str() {
                "(" => {
                    let inner = parse(words, at);
                    current = current.iter().flat_map(|c| inner.iter().map(move |i| [c.clone(), i.clone()].concat())).collect();
                }
                ")" => break,
                "OR" | "/" => out.append(&mut std::mem::replace(&mut current, vec![Vec::new()])),
                "AND" => {}
                "WITH" => *at += 1,
                license => {
                    for c in &mut current {
                        c.push(license.to_string());
                    }
                }
            }
        }
        out.append(&mut current);
        out.retain(|o| !o.is_empty());
        out
    }
    let spaced = expression.replace('(', " ( ").replace(')', " ) ").replace('/', " / ");
    let words: Vec<String> = spaced.split_whitespace().map(String::from).collect();
    parse(&words, &mut 0)
}

/// Whether a crate may be taken under a license that asks nothing (see [`ASK_NOTHING`]).
#[cfg(any(test, feature = "dev-tools"))]
fn asks_nothing(expression: &str) -> bool {
    choices(expression).iter().any(|o| o.iter().all(|l| ASK_NOTHING.contains(&l.as_str())))
}

/// Make the list of the crates built into the program for Windows, Linux and macOS, as `cargo
/// metadata` finds them, and write it to `out` (see the module): of each, its name, its license as
/// it says it (MIT/Apache-2.0 as MIT OR Apache-2.0), and its repository (or its home page, or its
/// page on crates.io).
#[cfg(feature = "dev-tools")]
pub fn write_crates(out: &std::path::Path) -> Result<(), String> {
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
    let mut own: BTreeSet<String> = BTreeSet::new();
    for platform in PLATFORMS {
        let m = metadata(Some(platform))?;
        own.extend(m["workspace_members"].as_array().ok_or("no workspace")?.iter().filter_map(|id| id.as_str()).map(str::to_string));
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
            let macros = packages
                .get(&id)
                .and_then(|p| p["targets"].as_array())
                .is_some_and(|t| t.iter().any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "proc-macro"))));
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

    let mut crates: BTreeMap<String, (String, String)> = BTreeMap::new();
    let (mut built, mut asking_nothing) = (0, 0);
    for id in &linked {
        let p = &packages[id];
        // (Not the workspace's own crates; winit, kept in patches/winit, is not one of them.)
        if own.contains(id) {
            continue;
        }
        built += 1;
        let s = |k: &str| p[k].as_str().unwrap_or_default().trim().to_string();
        let (name, expression) = (s("name"), s("license"));
        if asks_nothing(&expression) {
            asking_nothing += 1;
            continue;
        }
        let license = expression.replace(" / ", " OR ").replace('/', " OR ");
        let link = [s("repository"), s("homepage")].into_iter().find(|l| !l.is_empty()).unwrap_or_else(|| format!("https://crates.io/crates/{name}"));
        crates.entry(name).or_insert((license, link));
    }
    let list = Crates { crates: crates.into_iter().map(|(name, (license, link))| (name, license, link)).collect(), read_of };
    let json = serde_json::to_vec(&list).map_err(|e| e.to_string())?;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&json).map_err(|e| e.to_string())?;
    let bytes = z.finish().map_err(|e| e.to_string())?;
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{built} crates are built into the program; {asking_nothing} of them ask nothing; the other {} listed ({} kB deflated), written to {}",
        list.crates.len(),
        bytes.len().div_ceil(1024),
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list was made of the Cargo.lock there is: when a crate is added or changed, it is made
    /// again (`cargo run --features dev-tools -- --third-party`). Every work has its license and
    /// its link; none is of a system the program is not built for; the model, under CC BY, says
    /// its author and links to it.
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
        assert_eq!(read_crates().read_of, in_lock, "the list is of another Cargo.lock: cargo run --features dev-tools -- --third-party");
        let works = works();
        assert!(works.len() > 20 && works.iter().all(|w| !w.license.is_empty() && w.link.starts_with("http")));
        // (wgpu-core-deps-windows-linux-android is of Windows and Linux too.)
        for elsewhere in ["android-activity", "android_system_properties", "ndk-sys", "jni-sys", "web-sys", "js-sys", "wasm-bindgen"] {
            assert!(!works.iter().any(|w| w.name == elsewhere), "{elsewhere}: not built into the program for a computer");
        }
        let names: Vec<&str> = works.iter().map(|w| w.name.as_str()).collect();
        assert!(names.contains(&"wgpu") && names.contains(&"iced") && names.contains(&"winit"), "{names:?}");
        assert_eq!(names.iter().filter(|n| **n == "wgpu").count(), 1, "each crate once, whatever its versions");
        let ball = &works[0];
        assert!(ball.name.contains("Shader Ball JL 01") && ball.name.contains("Craneo") && ball.license == "CC BY 4.0" && ball.link == "https://skfb.ly/ozVxI");
    }

    /// The program is under the two licenses it says it is: what cargo is told, what About says,
    /// and the texts beside the sources; NOTICE names every work in it that is not a crate, with
    /// its license.
    #[test]
    fn the_program_is_under_the_licenses_it_says() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "MIT OR Apache-2.0");
        assert!(env!("CARGO_PKG_REPOSITORY").starts_with("https://github.com/"), "where the sources are");
        assert!(OWN_LICENSE.contains("MIT License") && OWN_LICENSE.contains("Apache License 2.0"));
        let mit = include_str!("../../../LICENSE-MIT");
        assert!(mit.starts_with("MIT License") && mit.contains("Permission is hereby granted, free of charge") && mit.contains("Brewed Texture Designer"));
        let apache = include_str!("../../../LICENSE-APACHE");
        assert!(
            apache.contains("Apache License") && apache.contains("Version 2.0, January 2004") && apache.trim_end().ends_with("END OF TERMS AND CONDITIONS")
        );
        let notice = include_str!("../../../NOTICE");
        assert!(notice.contains("LICENSE-MIT") && notice.contains("LICENSE-APACHE"));
        for (named, license) in [("Shader Ball JL 01", "Creative Commons Attribution 4.0"), ("Inter", "SIL Open Font License 1.1")] {
            assert!(works().iter().any(|w| w.kind != Kind::Software && w.name.contains(named)), "{named} is in the program");
            assert!(notice.contains(named) && notice.contains(license), "NOTICE names {named}, under {license}");
        }
        assert_eq!(works().iter().filter(|w| w.kind != Kind::Software).count(), 2, "a work that is not a crate is named in NOTICE too");
    }

    /// The winit and the smithay-clipboard the program is built with are the ones kept in
    /// `patches` (Cargo.lock has them from no registry), each with its change still in it (see
    /// patches/README.md): in winit, from Windows 11 on, the rect the system suggests is taken
    /// as a window comes to a display of another scale; in smithay-clipboard, the files dropped
    /// on a window on Wayland are taken by its data device and said to the program. A
    /// dependency moved, or a folder copied again from crates.io, would drop them without a
    /// word: a window would grow again between displays, a file dropped on Wayland be lost
    /// again. And winit makes no data device of its own for the drags: on GNOME it would get
    /// none of them, and anywhere else each file would be dropped twice.
    #[test]
    fn the_crates_kept_changed_are_built_with_their_changes() {
        let lock = include_str!("../../../Cargo.lock");
        let manifest = include_str!("../../../Cargo.toml");
        for (name, version) in [("winit", "0.30.13"), ("smithay-clipboard", "0.7.3")] {
            let built: Vec<&str> = lock.split("[[package]]").filter(|p| p.lines().any(|l| l.trim() == format!("name = \"{name}\""))).collect();
            assert_eq!(built.len(), 1, "one {name} is built");
            assert!(built[0].contains(&format!("version = \"{version}\"")) && !built[0].contains("source ="), "not the one of crates.io: {}", built[0]);
            assert!(manifest.contains(&format!("{name} = {{ path = \"patches/{name}\" }}")), "{name} is taken from patches");
        }
        let scale = include_str!("../../../patches/winit/src/platform_impl/windows/event_loop.rs").replace("\r\n", "\n");
        let taken = "if !WIN10_BUILD_VERSION.is_some_and(|build| build < 22000) {\n                new_outer_rect = suggested_rect;\n            } else {";
        assert!(scale.contains(taken), "the rect Windows suggests is taken, from Windows 11 on");
        let seat = include_str!("../../../patches/winit/src/platform_impl/linux/wayland/seat/mod.rs");
        assert!(!seat.contains("data_device"), "winit makes no data device");
        let device = include_str!("../../../patches/smithay-clipboard/src/state.rs");
        let (accepted, said) = ("offer.accept_mime_type(offer.serial, Some(URI_LIST.into()));", "crate::dropped::say(DroppedFiles {");
        assert!(device.contains(accepted) && device.contains(said), "the clipboard's data device takes the files dragged, and says them dropped");
        assert!(include_str!("../../../patches/smithay-clipboard/src/lib.rs").contains("pub use dropped::{DroppedFiles, dropped_files};"));
        let beside = include_str!("../../../patches/README.md");
        assert!(beside.contains("winit/pull/4341") && beside.contains("## smithay-clipboard 0.7.3"), "what is changed is said beside it");
    }

    /// No crate built into the program asks that the program be under its own license (the
    /// GPL's kind), or keeps anyone from using it: each may be taken under licenses that only
    /// ask to be named. The program could not be under MIT or Apache-2.0 otherwise.
    #[test]
    fn every_crate_lets_the_program_be_under_its_own_licenses() {
        const NAMED_ONLY: &[&str] = &["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Unicode-DFS-2016", "NCSA", "OFL-1.1"];
        let crates = read_crates().crates;
        assert!(crates.len() > 20);
        for (name, license, _) in &crates {
            let free = choices(license).iter().any(|taken| taken.iter().all(|l| NAMED_ONLY.contains(&l.as_str()) || ASK_NOTHING.contains(&l.as_str())));
            assert!(free, "{name} is under {license}: not a license the program may be built of");
        }
    }

    /// Of a choice, one license that asks nothing leaves the crate out; licenses taken together
    /// are all taken.
    #[test]
    fn a_crate_that_may_be_taken_under_a_license_that_asks_nothing_is_left_out() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(choices("MIT OR Apache-2.0"), [s(&["MIT"]), s(&["Apache-2.0"])]);
        assert_eq!(choices("MIT/Apache-2.0").len(), 2);
        assert_eq!(choices("(MIT OR Apache-2.0) AND Unicode-3.0"), [s(&["MIT", "Unicode-3.0"]), s(&["Apache-2.0", "Unicode-3.0"])]);
        assert_eq!(choices("Apache-2.0 WITH LLVM-exception OR MIT"), [s(&["Apache-2.0"]), s(&["MIT"])]);
        assert!(
            asks_nothing("Zlib OR Apache-2.0 OR MIT")
                && asks_nothing("Unlicense OR MIT")
                && asks_nothing("BSL-1.0")
                && asks_nothing("0BSD OR MIT OR Apache-2.0")
        );
        assert!(!asks_nothing("MIT OR Apache-2.0") && !asks_nothing("(MIT OR Apache-2.0) AND Unicode-3.0"));
    }
}
