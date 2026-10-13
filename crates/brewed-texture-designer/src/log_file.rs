//! The log, written to a file beside the settings as well as to standard error: a release build
//! started from the file manager has no console on Windows, and nobody reads standard error
//! where it has one, so what went wrong would be told nowhere. The log of the run before is kept
//! (`.1`); each is held to [`CAP`], so that a warning said over and over cannot fill the disk.
//! Help → Reveal Log File shows it in the file manager.

use std::cell::Cell;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The name of the log in the program's folder (see [`crate::workspace::folder`]).
pub const NAME: &str = "brewed-texture-designer.log";

/// How large a log grows, at most: then it says it stops there. Only a panic is written past it
/// (see [`last_words`]), into the room of [`RESERVE`].
pub const CAP: u64 = 4 * 1024 * 1024;
pub const RESERVE: u64 = 64 * 1024;

thread_local! {
    /// Whether what this thread writes now is a panic, written past the cap.
    static LAST_WORDS: Cell<bool> = const { Cell::new(false) };
}

/// Run `f`, what it logs written to the file even when the log has reached its cap: what a panic
/// says (see [`install_panic_hook`]).
pub fn last_words(f: impl FnOnce()) {
    LAST_WORDS.set(true);
    f();
    LAST_WORDS.set(false);
}

/// Where the log goes: to standard error, and to the file while it has room.
pub struct Sink {
    file: Option<File>,
    stderr: bool,
    written: u64,
    cap: u64,
}

impl Sink {
    /// The log of this run in `folder` (made if it is not there), the one before kept as `.1`;
    /// with where it is. Without the file where it cannot be made: standard error only.
    pub fn open(folder: Option<&Path>, cap: u64) -> (Sink, Option<PathBuf>) {
        let file = folder.and_then(|folder| {
            let path = folder.join(NAME);
            let made = std::fs::create_dir_all(folder).and_then(|_| {
                // (Where it cannot be kept, it is written over: this run's log matters more.)
                if path.exists() {
                    let _ = std::fs::rename(&path, previous(&path));
                }
                File::create(&path)
            });
            match made {
                Ok(file) => Some((file, path)),
                Err(e) => {
                    // (No logger yet: said where it can be.)
                    eprintln!("{}: {e}; the log goes to standard error only", path.display());
                    None
                }
            }
        });
        let (file, path) = file.unzip();
        (Sink { file, stderr: true, written: 0, cap }, path)
    }

    /// Without standard error (for the tests).
    #[cfg(test)]
    fn quiet(mut self) -> Sink {
        self.stderr = false;
        self
    }
}

/// Where the log of the run before this one is kept.
pub fn previous(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

impl Write for Sink {
    /// A record whole, or not at all; never fails (what cannot be written is let go): a logger
    /// that failed would have nowhere to say so.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.stderr {
            let _ = std::io::stderr().write_all(buf);
        }
        if let Some(file) = &mut self.file {
            let room = if LAST_WORDS.get() { self.cap + RESERVE } else { self.cap };
            let length = buf.len() as u64;
            if self.written + length <= room {
                let _ = file.write_all(buf);
                self.written += length;
            } else if self.written <= self.cap {
                let said = format!("[the log stops here: it has reached {} KB; only a panic is written after this]\n", self.cap / 1024);
                let _ = file.write_all(said.as_bytes());
                // (Said once.)
                self.written = self.cap + 1;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
        Ok(())
    }
}

/// Start the log (`filter` as `RUST_LOG` would have it when that is not set): to standard error,
/// and to the file in `folder`. Where the file is, when it could be made.
pub fn start(folder: Option<&Path>, filter: &str) -> Option<PathBuf> {
    let (sink, path) = Sink::open(folder, CAP);
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter)).target(env_logger::Target::Pipe(Box::new(sink))).init();
    path
}

/// A panic, wherever it happens, is logged with where it happened (into the file, past its cap
/// if it has to be) before the hook there was says it as it does.
pub fn install_panic_hook() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let said = panicked(info.payload(), info.location(), thread.name());
        last_words(|| log::error!("{said}"));
        report(info);
    }));
}

/// What is logged of a panic: what it said, where, and on what thread.
pub fn panicked(payload: &(dyn std::any::Any + Send), at: Option<&std::panic::Location<'_>>, thread: Option<&str>) -> String {
    let message = match (payload.downcast_ref::<&str>(), payload.downcast_ref::<String>()) {
        (Some(text), _) => text,
        (_, Some(text)) => text.as_str(),
        _ => "(says nothing that is text)",
    };
    let at = at.map_or_else(|| "somewhere unknown".to_string(), |at| format!("{}:{}:{}", at.file(), at.line(), at.column()));
    format!("panic on the thread {}, at {at}: {message}", thread.unwrap_or("without a name"))
}

/// Show `file` in the file manager: selected in its folder where the system can do that, else
/// its folder opened. Not in the tests, which open nothing outside the program.
pub fn reveal(file: &Path) {
    if cfg!(test) {
        log::info!("not revealed: {}", file.display());
        return;
    }
    #[cfg(windows)]
    let shown = {
        use std::os::windows::process::CommandExt;
        // (As it is written, quotes and all: explorer reads its arguments itself.)
        let mut select = std::ffi::OsString::from("/select,\"");
        select.push(file);
        select.push("\"");
        std::process::Command::new("explorer").raw_arg(select).spawn().map(drop)
    };
    #[cfg(target_os = "macos")]
    let shown = std::process::Command::new("open").arg("-R").arg(file).spawn().map(drop);
    #[cfg(all(unix, not(target_os = "macos")))]
    let shown = {
        // The file manager is asked over D-Bus (the freedesktop.org FileManager1 interface,
        // which selects the file); off the window's thread, as the file manager may have to
        // start first. Where none answers, the folder is opened.
        let (file, uri) = (file.to_path_buf(), file_uri(file));
        std::thread::Builder::new()
            .name("reveal".into())
            .spawn(move || {
                let asked = zbus::blocking::Connection::session().and_then(|bus| {
                    bus.call_method(
                        Some("org.freedesktop.FileManager1"),
                        "/org/freedesktop/FileManager1",
                        Some("org.freedesktop.FileManager1"),
                        "ShowItems",
                        &(vec![uri.as_str()], ""),
                    )
                });
                if let Err(e) = asked {
                    log::info!("the file manager did not show {}: {e}; its folder is opened", file.display());
                    let folder = file.parent().unwrap_or(&file);
                    if let Err(e) = std::process::Command::new("xdg-open").arg(folder).spawn() {
                        log::warn!("{}: {e}", folder.display());
                    }
                }
            })
            .map(drop)
    };
    if let Err(e) = shown {
        log::warn!("{}: {e}", file.display());
    }
}

/// `path` as a `file://` URI: its bytes as they are, those that are not plain written as `%XX`.
#[cfg(all(unix, not(target_os = "macos")))]
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => uri.push(byte as char),
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("brewed-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    /// The log is made in the folder of the settings (made too if it is not there yet); the log
    /// of the run before is kept beside it, that of the run before that is gone.
    #[test]
    fn the_log_of_the_run_before_is_kept() {
        let folder = folder("kept");
        for run in ["first", "second", "third"] {
            let (sink, path) = Sink::open(Some(&folder), CAP);
            let path = path.expect("a log");
            assert_eq!(path, folder.join(NAME));
            writeln!(sink.quiet(), "the {run} run").unwrap();
        }
        let read = |path: PathBuf| std::fs::read_to_string(path).unwrap();
        assert_eq!(read(folder.join(NAME)), "the third run\n");
        assert_eq!(read(previous(&folder.join(NAME))), "the second run\n");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A log stops growing at its cap, and says so, once; a panic is still written after that.
    #[test]
    fn a_log_stops_at_its_cap_but_a_panic_is_still_written() {
        let folder = folder("cap");
        let (sink, path) = Sink::open(Some(&folder), 1000);
        let mut sink = sink.quiet();
        for _ in 0..500 {
            sink.write_all(b"the same warning, again\n").unwrap();
        }
        last_words(|| sink.write_all(b"panic on the thread main\n").unwrap());
        sink.write_all(b"after the panic\n").unwrap();
        let log = std::fs::read_to_string(path.unwrap()).unwrap();
        assert!(log.len() < 1000 + 200, "{} bytes", log.len());
        assert_eq!(log.matches("the log stops here").count(), 1);
        assert!(log.ends_with("panic on the thread main\n"), "{log}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Where the log cannot be made (a file where its folder would be), it goes to standard error
    /// only, and that is all.
    #[test]
    fn a_log_that_cannot_be_made_is_done_without() {
        let folder = folder("none");
        std::fs::write(&folder, "not a folder").unwrap();
        let (sink, path) = Sink::open(Some(&folder), CAP);
        assert!(path.is_none());
        sink.quiet().write_all(b"said nowhere, and no harm done\n").unwrap();
        let (_, path) = Sink::open(None, CAP);
        assert!(path.is_none());
        let _ = std::fs::remove_file(&folder);
    }

    /// What is logged of a panic says what it said and where, as text however it was given.
    #[test]
    fn a_panic_is_logged_with_where_it_happened() {
        let at = std::panic::Location::caller();
        let said = panicked(&"the index is out of bounds", Some(at), Some("main"));
        assert!(said.contains("the index is out of bounds") && said.contains("main"), "{said}");
        assert!(said.contains(&format!("{}:{}", at.file(), at.line())), "{said}");
        let said = panicked(&format!("{} nodes", 3), None, None);
        assert!(said.contains("3 nodes") && said.contains("somewhere unknown"), "{said}");
        assert!(panicked(&42, None, None).contains("nothing that is text"));
    }

    /// A file is shown by the file manager as a URI: its name as it is, byte for byte, whatever
    /// the encoding.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_file_is_named_to_the_file_manager_byte_for_byte() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/home/me/.config/caf\xe9 au lait.log"));
        assert_eq!(file_uri(path), "file:///home/me/.config/caf%E9%20au%20lait.log");
    }
}
