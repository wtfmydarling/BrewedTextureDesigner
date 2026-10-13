//! Files dropped on a window: Brewed Texture Designer's addition to smithay-clipboard 0.7.3
//! (`patches/README.md` in its repository says why it is kept, and what keeps it in the build).
//!
//! Why here, in the clipboard: a compositor may send a drag to one `wl_data_device` of a client
//! only. Mutter (GNOME) does, and the selection with it: to the one made last. In a program
//! built on winit that is this crate's, made for the clipboard once a window is there. A data
//! device made anywhere else for the drags gets none of them there, or, made later, takes the
//! selection away from this one, and nothing is pasted any more. So the one data device there is
//! takes the drags too.
//!
//! Only files are taken: a drag that offers `text/uri-list`, for Copy alone (a move could have
//! its source delete the file). The list is read without blocking the loop, and said once the
//! drop is done, with the surface it was dropped on and where: [`dropped_files`].

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Mutex, OnceLock};

/// The mime type of a list of files (RFC 2483).
pub(crate) const URI_LIST: &str = "text/uri-list";

/// A list larger than this is not the files somebody dragged.
pub(crate) const MAX_URI_LIST: usize = 16 << 20;

/// Files dropped on a surface of the program.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedFiles {
    /// The `wl_surface` they were dropped on: the address of its proxy, which is what
    /// `raw-window-handle` gives as the surface of a window.
    pub surface: usize,
    /// Where on the surface, in its own coordinates (logical pixels).
    pub x: f64,
    pub y: f64,
    /// The `text/uri-list` as its source wrote it: a URI to a line.
    pub uri_list: String,
}

struct Drops {
    said: SyncSender<DroppedFiles>,
    heard: Mutex<Option<Receiver<DroppedFiles>>>,
}

/// (Drops nobody listens to are not kept beyond these.)
const KEPT: usize = 32;

fn drops() -> &'static Drops {
    static DROPS: OnceLock<Drops> = OnceLock::new();
    DROPS.get_or_init(|| {
        let (said, heard) = sync_channel(KEPT);
        Drops { said, heard: Mutex::new(Some(heard)) }
    })
}

/// What is dropped on the windows of the program, from now on: for the first that asks (there
/// is one receiver; whoever asks after gets `None`).
pub fn dropped_files() -> Option<Receiver<DroppedFiles>> {
    drops().heard.lock().ok()?.take()
}

pub(crate) fn say(dropped: DroppedFiles) {
    let _ = drops().said.try_send(dropped);
}
