//! What the file dialogs of the program show. Every filter of one is added here, through
//! [`Filters`]: a test reads the sources for a filter added another way.
//!
//! rfd makes a glob of each extension of a filter, `*.png`, for the dialogs of Linux and the
//! BSDs (the XDG desktop portal's, and zenity's where there is no portal), and those dialogs
//! match a glob as it is written: a file named `SCAN.PNG` or `IMG_0001.JPG` was not shown there
//! to open or to import, though the dialogs of Windows and macOS, which do not tell capitals
//! apart, showed it. So there, a dialog that opens files is given each letter of an extension as
//! a class of both its cases, `*.[pP][nN][gG]`, as the portal's documentation says to.

/// Do the file dialogs of this system tell capitals apart in the extensions of a filter? Those
/// of Linux and the BSDs do; those of Windows and macOS do not.
const CAPITALS_APART: bool = cfg!(all(unix, not(target_os = "macos")));

/// How a file dialog is told which files it shows.
pub trait Filters {
    /// Show the files with these extensions (lower case) under `name`, in a dialog that opens
    /// files: named in any case of letters, on every system (see [`open_filter_extensions`]).
    fn filter_to_open(self, name: impl Into<String>, extensions: &[&str]) -> Self;

    /// Show the files with these extensions under `name`, in a dialog that saves one: the
    /// extensions as they are, on every system. A dialog that adds an extension to a name typed
    /// without one (and asks before saving over the file so named) adds the first of them: on
    /// Windows and macOS rfd tells the dialog which; on Linux and the BSDs rfd adds none, but
    /// KDE's dialog, behind the portal, reads it from the filter's first glob, only when that is
    /// plain (`*.brewtex`; a class of cases, `*.[bB][rR][eE][wW][tT][eE][xX]`, it passes over,
    /// and KDE's portal makes one plain first only since Plasma 6.4). Given classes, KDE's would
    /// add none, and a package saved as `walls` would go over `walls.brewtex` unasked: the
    /// program adds the extension itself.
    fn filter_to_save(self, name: impl Into<String>, extensions: &[&str]) -> Self;
}

impl Filters for rfd::FileDialog {
    fn filter_to_open(self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.add_filter(name, &open_filter_extensions(extensions, CAPITALS_APART))
    }

    fn filter_to_save(self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.add_filter(name, extensions)
    }
}

/// What rfd is given for the extensions (lower case) of a filter of a dialog that opens files,
/// where the dialogs tell capitals apart (`capitals_apart`) and where they do not: there, each
/// letter as a class of both its cases (`jpg`: `[jJ][pP][gG]`; `mp4`: `[mM][pP]4`); elsewhere,
/// the extensions as they are.
fn open_filter_extensions(extensions: &[&str], capitals_apart: bool) -> Vec<String> {
    extensions
        .iter()
        .map(|extension| {
            if !capitals_apart {
                return extension.to_string();
            }
            let mut classes = String::new();
            for c in extension.chars() {
                if c.is_ascii_alphabetic() {
                    classes.extend(['[', c.to_ascii_lowercase(), c.to_ascii_uppercase(), ']']);
                } else {
                    classes.push(c);
                }
            }
            classes
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Does the glob rfd makes of `extension` (`*.<extension>`) match the file `name`, as the
    /// portal and zenity match one: letter for letter, a class in brackets by any of its own?
    fn shows(extension: &str, name: &str) -> bool {
        let mut letters = Vec::new();
        let mut glob = extension.chars();
        while let Some(c) = glob.next() {
            letters.push(if c == '[' { glob.by_ref().take_while(|l| *l != ']').collect() } else { c.to_string() });
        }
        let name: Vec<char> = name.chars().collect();
        let Some(dot) = name.len().checked_sub(letters.len() + 1) else { return false };
        name[dot] == '.' && name[dot + 1..].iter().zip(&letters).all(|(c, letter)| letter.contains(*c))
    }

    /// Where the dialogs tell capitals apart, a dialog that opens files shows them however
    /// their names are written: each letter of an extension a class of both its cases (what is
    /// no letter as it is), and the file named `IMG_0001.JPG` there, as `img_0001.jpg` is.
    /// Where they do not, the extensions are given as they are. Both, on every system.
    #[test]
    fn a_dialog_that_opens_files_shows_them_in_any_case() {
        let extensions = ["jpg", "brewtex", "mp4", "*"];
        let classes = open_filter_extensions(&extensions, true);
        assert_eq!(classes, ["[jJ][pP][gG]", "[bB][rR][eE][wW][tT][eE][xX]", "[mM][pP]4", "*"]);
        assert_eq!(open_filter_extensions(&extensions, false), extensions);
        for name in ["IMG_0001.JPG", "img_0001.jpg", "Scan.Jpg", "wall.old.jPG"] {
            assert!(shows(&classes[0], name), "{name} is shown");
        }
        for name in ["IMG_0001.JPEG", "IMG_0001.JPG.txt", "JPG", "jpg"] {
            assert!(!shows(&classes[0], name), "{name} is not shown");
        }
        assert!(!shows("jpg", "IMG_0001.JPG"), "the plain glob, as it was, did not show it");
        assert!(shows(&classes[1], "WALL.BREWTEX"));
        assert!(shows(&classes[2], "clip.MP4"), "what is no letter stays as it is");
    }

    /// No filter of a file dialog is added but through [`Filters`]: one added another way would
    /// show the files to open on Linux and the BSDs only as their extensions are written. The
    /// sources of the program are read for it.
    #[test]
    fn every_filter_of_a_file_dialog_is_added_here() {
        fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("a folder of sources").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    sources(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(files.len() > 40, "the sources are read: {}", files.len());
        let mut found = Vec::new();
        for file in files.iter().filter(|f| !f.ends_with("file_dialogs.rs")) {
            let text = std::fs::read_to_string(file).expect("a source");
            for (n, line) in text.lines().enumerate() {
                // (What is said in a comment adds nothing; a function named so is no call of it.)
                let code = line.split("//").next().unwrap_or_default();
                let added = code.match_indices("add_filter(").any(|(at, _)| {
                    let before = &code[..at];
                    let named = before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
                    !named && !before.trim_end().ends_with("fn")
                });
                if added {
                    found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
                }
            }
        }
        assert!(found.is_empty(), "a filter of a file dialog added without `Filters`:\n{}", found.join("\n"));
    }
}
