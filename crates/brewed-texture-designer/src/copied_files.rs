//! Files copied in a file manager, as the clipboard says them in text: what Paste takes as files
//! dropped on the window. GNOME Files and Dolphin say them one to a line, as paths or as `file:`
//! URIs (`text/uri-list`, RFC 2483, whose lines end in CRLF); Explorer's "Copy as path" says them
//! in quotes.

use std::path::PathBuf;

/// The files the clipboard's text says, in its order; `None` where it says anything else (a line
/// that is not a local file, or one that is not there): then it is not files that were copied.
pub fn of_text(text: &str) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();
    for line in text.lines().map(str::trim) {
        // (A uri-list's comments, and the blank line a list may end with.)
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix('"').and_then(|l| l.strip_suffix('"')).unwrap_or(line);
        let path = match line.strip_prefix("file:") {
            Some(uri) => of_uri(uri)?,
            None => PathBuf::from(line),
        };
        if !path.is_absolute() || !path.is_file() {
            return None;
        }
        files.push(path);
    }
    (!files.is_empty()).then_some(files)
}

/// The files of what was dragged from a file manager, a `text/uri-list`: its `file:` URIs of
/// this computer, in its order. What else it lists (a link, a file of another computer) is left
/// out; whether a file is there is not asked: who takes it says what is wrong with it. (Where a
/// dropped file is the window's own event, nothing asks for this.)
#[cfg(any(free_unix, test))]
pub fn of_uri_list(list: &str) -> Vec<PathBuf> {
    let lines = list.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'));
    lines.filter_map(|line| line.strip_prefix("file:").and_then(of_uri)).collect()
}

/// The path of a `file:` URI (what follows `file:`): `///home/me/a%20b.png`, or with the host,
/// `//localhost/…` (no other host: the file is not on this computer); `///C:/…` on Windows.
fn of_uri(uri: &str) -> Option<PathBuf> {
    let path = match uri.strip_prefix("//") {
        Some(rest) => {
            let slash = rest.find('/')?;
            if !matches!(&rest[..slash], "" | "localhost") {
                return None;
            }
            &rest[slash..]
        }
        None => uri,
    };
    let bytes = percent_decoded(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        let path = String::from_utf8(bytes).ok()?;
        // `/C:/Users/…`: the drive, not a folder of the root.
        let path = match path.as_bytes() {
            [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &path[1..],
            _ => &path,
        };
        Some(PathBuf::from(path.replace('/', "\\")))
    }
}

/// `%20` as the byte it stands for; `None` where a `%` stands for nothing.
fn percent_decoded(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            let hex = std::str::from_utf8(s.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two files copied in GNOME Files (a uri-list, its lines ending in CRLF, a name in Cyrillic
    /// and one with a space), in Dolphin, as paths, and with Explorer's "Copy as path": the same
    /// two files, in their order. Text that only looks like files is not taken for them.
    #[test]
    fn files_copied_in_a_file_manager_are_the_files_they_are() {
        let dir = std::env::temp_dir().join(format!("btd-copied-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("кирпич.png"), dir.join("old wood.svg"));
        std::fs::write(&a, b"").unwrap();
        std::fs::write(&b, b"").unwrap();
        let uri = |p: &PathBuf| {
            let s = p.to_str().unwrap().replace('\\', "/");
            let encoded: String =
                s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"/-._:".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect();
            // (`/home/…` on Linux and macOS; `C:/…` on Windows, after a slash of its own.)
            if encoded.starts_with('/') { format!("file://{encoded}") } else { format!("file:///{encoded}") }
        };
        let both = Some(vec![a.clone(), b.clone()]);
        let (pa, pb) = (a.display().to_string(), b.display().to_string());
        assert_eq!(of_text(&format!("{}\r\n{}\r\n", uri(&a), uri(&b))), both, "a uri-list");
        assert_eq!(of_text(&format!("# copied\n{}\n{}", uri(&a), uri(&b))), both, "with a comment, ends in LF");
        assert_eq!(of_text(&format!("{pa}\n{pb}")), both, "paths");
        assert_eq!(of_text(&format!("\"{pa}\"\r\n\"{pb}\"")), both, "paths in quotes");
        let localhost = uri(&a).replacen("file://", "file://localhost", 1);
        assert_eq!(of_text(&localhost), Some(vec![a.clone()]), "this computer, named");

        // What is dragged from a file manager: its files of this computer, there or not (who
        // takes them says so); a link and a file of another computer are left out.
        let gone = dir.join("not there.png");
        let elsewhere = uri(&a).replacen("file://", "file://elsewhere", 1);
        let dragged = format!("# dragged\r\n{}\r\nhttps://example.com/bricks.png\r\n{elsewhere}\r\n{}\r\n{}\r\n", uri(&a), uri(&gone), uri(&b));
        assert_eq!(of_uri_list(&dragged), vec![a.clone(), gone, b.clone()]);
        assert_eq!(of_uri_list("https://example.com/bricks.png\r\n"), Vec::<PathBuf>::new());

        for not in [
            String::new(),
            "\r\n".into(),
            "bricks".into(),
            "#RRGGBB".into(),
            format!("{pa}\na word"),
            uri(&a).replacen("file://", "file://elsewhere", 1),
            format!("{}%2", uri(&a)),
            "https://example.com/bricks.png".into(),
            dir.join("not there.png").display().to_string(),
            dir.display().to_string(),
        ] {
            assert_eq!(of_text(&not), None, "{not:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
