//! The names of the files an export writes.

use std::path::{Path, PathBuf};

/// `name` as the beginning of the name of a file a preset writes: what a file name cannot
/// have, and blanks, as `_`.
pub fn file_stem(name: &str) -> String {
    let stem: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if stem.is_empty() { "graph".into() } else { stem }
}

/// Make a name safe to use as a file name.
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name.trim().chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(['.', ' ']);
    // Short enough for every file system (255 bytes), with a number and the extension.
    let mut end = cleaned.len().min(200);
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    let cleaned = cleaned[..end].trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return "output".to_string();
    }
    // Names Windows keeps for devices, whatever follows the first dot: a file of that name
    // cannot be written there (before Windows 11). Everywhere the same name, for the same
    // files wherever a project is exported.
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
        "LPT7", "LPT8", "LPT9",
    ];
    let stem = cleaned.split('.').next().unwrap_or_default();
    if DEVICES.iter().any(|d| d.eq_ignore_ascii_case(stem.trim_end())) {
        return format!("{stem}_{}", &cleaned[stem.len()..]);
    }
    cleaned.to_string()
}

/// Unique file paths `dir/name.extension` for a list of (name, extension), each name made safe
/// ([`sanitize_file_name`]); of two the same, the later gets a number (names are compared
/// whatever their case, as Windows does).
pub fn unique_paths(dir: &Path, files: &[(String, &str)]) -> Vec<PathBuf> {
    let mut used = std::collections::HashSet::new();
    files
        .iter()
        .map(|(name, extension)| {
            let base = sanitize_file_name(name);
            let mut candidate = base.clone();
            let mut n = 2;
            while !used.insert(format!("{}.{extension}", candidate.to_lowercase())) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            dir.join(format!("{candidate}.{extension}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(sanitize_file_name("base color"), "base color");
        assert_eq!(sanitize_file_name("a/b:c*"), "a_b_c_");
        assert_eq!(sanitize_file_name("  "), "output");
        assert_eq!(sanitize_file_name("..."), "output");
        // Device names of Windows.
        assert_eq!(sanitize_file_name("aux"), "aux_");
        assert_eq!(sanitize_file_name("CON.map"), "CON_.map");
        assert_eq!(sanitize_file_name("Com1 "), "Com1_");
        assert_eq!(sanitize_file_name("console"), "console");
        assert_eq!(sanitize_file_name("nul_map"), "nul_map");
        // Long names, cut where a character ends.
        let long = sanitize_file_name(&"я".repeat(300));
        assert!(long.len() <= 200 && long.chars().all(|c| c == 'я'), "{}", long.len());
        assert_eq!(sanitize_file_name(&format!("{}. .x", "a".repeat(199))), "a".repeat(199));
        let paths = unique_paths(Path::new("d"), &[("a".into(), "png"), ("A".into(), "png"), ("a".into(), "exr")]);
        let names: Vec<String> = paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.png", "A_2.png", "a.exr"]);
        assert_eq!(file_stem("Rusty Metal"), "Rusty_Metal");
        assert_eq!(file_stem(" "), "graph");
    }
}
