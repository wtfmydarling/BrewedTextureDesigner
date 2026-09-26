//! The files packages use (images, vector graphics, fonts): linked where they are on disk, or
//! held by the package itself (embedded), which writes them into its own file.
//!
//! An embedded file is named by what it holds: `embedded:<hash>/<name>`, the hash of its
//! bytes and the name it came with. The same name always means the same bytes, in any
//! package: a node that reads one reads it wherever it is pasted, and two packages that hold
//! the same file hold it once. The bytes of every embedded file of the packages read or made
//! in this run are kept here, for whatever reads files (the engine, the Explorer's previews):
//! [`read`] reads a file a package uses, whichever it is.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

const PREFIX: &str = "embedded:";

/// Is `path` the name of an embedded file (see the module)?
pub fn is_embedded(path: &str) -> bool {
    path.starts_with(PREFIX)
}

/// The hash of the bytes the embedded file `path` names (see the module): the same for the
/// same bytes, whatever name they came with.
pub fn embedded_hash(path: &str) -> Option<&str> {
    let (hash, _) = path.strip_prefix(PREFIX)?.split_once('/')?;
    (hash.len() == 16 && hash.bytes().all(|c| c.is_ascii_hexdigit())).then_some(hash)
}

/// The name of the embedded file that holds `bytes` and came as `name` (a path, of which the
/// file name is kept).
pub fn embedded_path(name: &str, bytes: &[u8]) -> String {
    let name = Path::new(name).file_name().map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());
    format!("{PREFIX}{:016x}/{}", fnv1a(bytes), name.replace(['/', '\\'], "_"))
}

/// FNV-1a: a hash that is the same in every build and on every system, as the names of files
/// written into packages have to be.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn store() -> &'static RwLock<HashMap<String, Arc<Vec<u8>>>> {
    static FILES: OnceLock<RwLock<HashMap<String, Arc<Vec<u8>>>>> = OnceLock::new();
    FILES.get_or_init(Default::default)
}

/// Keep the bytes of the embedded file `path` for whatever reads it.
pub fn keep(path: &str, bytes: &Arc<Vec<u8>>) {
    if let Ok(mut files) = store().write() {
        files.entry(path.to_string()).or_insert_with(|| bytes.clone());
    }
}

/// The bytes of the embedded file `path`, when a package holds it.
pub fn embedded(path: &str) -> Option<Arc<Vec<u8>>> {
    store().read().ok()?.get(path).cloned()
}

/// The largest file read: larger than any file a node reads can be (a picture of 8192 × 8192
/// 32-bit floats, the largest a Bitmap reads, takes 1 GB).
pub const MAX_FILE: u64 = 2 << 30;

/// The bytes of the file at `path` on disk; else why they cannot be read (a file larger than
/// [`MAX_FILE`] is not: it would only fill the memory; nor what is not a file, such as a
/// device, which can be read forever).
pub fn read_disk(path: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let too_big = |size: u64| {
        let gb = size as f64 / (1u64 << 30) as f64;
        format!("it is {gb:.1} GB; files of more than {} GB are not read", MAX_FILE >> 30)
    };
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("it is not a file".into());
    }
    if meta.len() > MAX_FILE {
        return Err(too_big(meta.len()));
    }
    // No more than that, whatever the file has grown to since.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    std::fs::File::open(path).and_then(|file| file.take(MAX_FILE + 1).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(bytes)
}

/// The bytes of the file at `path`: of the package that holds it, or from disk.
pub fn read(path: &str) -> Result<Arc<Vec<u8>>, String> {
    if is_embedded(path) {
        return embedded(path).ok_or_else(|| format!("{} is not in its package", crate::package::file_name(path)));
    }
    read_disk(path).map(Arc::new).map_err(|e| format!("Cannot open '{path}': {e}"))
}

/// Is the file at `path` there: embedded and held, or on disk (which this asks the disk).
pub fn is_there(path: &str) -> bool {
    if is_embedded(path) { embedded(path).is_some() } else { Path::new(path).is_file() }
}

/// Write the file at `path` as `write` writes a file (at the path it is given), so that
/// whenever the program, the system or the power stops, the file is as it was or as it is now,
/// whole: it is written beside it, forced to the disk, and put in its place in one step (and
/// where the system needs it, the folder's new entry is forced to the disk too). What watches
/// the folder (a game engine importing textures) sees the new file whole or not at all. A file
/// held open elsewhere for a moment (an indexer, a virus scanner, a synced folder) is waited
/// for, a second at most.
pub fn replace<E: std::fmt::Display>(path: &Path, write: impl FnOnce(&Path) -> Result<(), E>) -> Result<(), String> {
    let name = path.file_name().ok_or_else(|| format!("'{}' names no file", path.display()))?;
    let beside = path.with_file_name(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let failed = |e: String| {
        let _ = std::fs::remove_file(&beside);
        format!("Cannot write '{}': {e}", path.display())
    };
    write(&beside).map_err(|e| failed(e.to_string()))?;
    std::fs::OpenOptions::new().write(true).open(&beside).and_then(|file| file.sync_all()).map_err(|e| failed(e.to_string()))?;
    let mut waited = 0;
    while let Err(e) = std::fs::rename(&beside, path) {
        // (Windows replaces no file that is open elsewhere, unless it was opened to let it.)
        let held = matches!(e.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ResourceBusy);
        if !held || waited == 20 {
            return Err(failed(e.to_string()));
        }
        waited += 1;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
        if let Ok(dir) = std::fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// [`replace`] the file at `path` with `bytes`.
pub fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    replace(path, |beside| std::fs::write(beside, bytes))
}

/// Bytes as a package file writes them: base64.
pub(crate) mod base64_bytes {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => s.serialize_str(&STANDARD.encode(bytes.as_slice())),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Vec<u8>>>, D::Error> {
        let text: Option<String> = Option::deserialize(d)?;
        text.map(|t| STANDARD.decode(t.as_bytes()).map(Arc::new).map_err(serde::de::Error::custom)).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_files_are_named_by_what_they_hold() {
        let a = embedded_path("C:/textures/bricks.png", b"bricks");
        assert!(is_embedded(&a) && a.ends_with("/bricks.png"), "{a}");
        assert_eq!(a, embedded_path("/elsewhere/bricks.png", b"bricks"), "the same bytes, the same name");
        assert_ne!(a, embedded_path("bricks.png", b"stones"));
        assert_eq!(crate::package::file_name(&a), "bricks.png");
        // Held, it is read from here; a file of disk is read from disk.
        assert!(read(&a).is_err() && !is_there(&a));
        keep(&a, &Arc::new(b"bricks".to_vec()));
        assert_eq!(read(&a).unwrap().as_slice(), b"bricks");
        assert!(is_there(&a));
        assert!(!is_embedded("C:/textures/bricks.png"));
    }

    /// A file is replaced whole: what writes it fails, and it is as it was; written, it is what
    /// was written. Nothing is left beside it either way.
    #[test]
    fn files_are_replaced_whole() {
        let dir = std::env::temp_dir().join(format!("brewed-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("walls.brewtex");
        std::fs::write(&file, b"before").unwrap();
        let failed = replace(&file, |beside| {
            std::fs::write(beside, b"half")?;
            Err::<(), _>(std::io::Error::other("the disk is full"))
        });
        assert!(failed.unwrap_err().contains("the disk is full"));
        assert_eq!(std::fs::read(&file).unwrap(), b"before");
        write_whole(&file, b"after").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"after");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing left beside it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file held open elsewhere for a moment, not to be replaced meanwhile (as a virus scanner
    /// or an indexer holds files on Windows), is waited for.
    #[cfg(windows)]
    #[test]
    fn a_file_held_open_for_a_moment_is_waited_for() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("brewed-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("basecolor.png");
        std::fs::write(&file, b"before").unwrap();
        // Shared for reading only: not to be deleted or replaced while it is open.
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&file).unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(held);
        });
        write_whole(&file, b"after").unwrap();
        letting_go.join().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"after");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What is not a file (a folder; a pipe, which a reader waits at forever) is not read.
    #[test]
    fn only_files_are_read() {
        let dir = std::env::temp_dir().join(format!("brewed-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(read_disk(&file.to_string_lossy()).unwrap(), b"abc");
        assert!(read_disk(&dir.to_string_lossy()).unwrap_err().contains("not a file"));
        #[cfg(unix)]
        {
            let pipe = dir.join("pipe");
            assert!(std::process::Command::new("mkfifo").arg(&pipe).status().unwrap().success());
            assert!(read_disk(&pipe.to_string_lossy()).unwrap_err().contains("not a file"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
