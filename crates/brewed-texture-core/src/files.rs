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

/// The bytes of the file at `path`: of the package that holds it, or from disk.
pub fn read(path: &str) -> Result<Arc<Vec<u8>>, String> {
    if is_embedded(path) {
        return embedded(path).ok_or_else(|| format!("{} is not in its package", crate::package::file_name(path)));
    }
    std::fs::read(path).map(Arc::new).map_err(|e| format!("Cannot open '{path}': {e}"))
}

/// Is the file at `path` there: embedded and held, or on disk (which this asks the disk).
pub fn is_there(path: &str) -> bool {
    if is_embedded(path) { embedded(path).is_some() } else { Path::new(path).is_file() }
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
}
