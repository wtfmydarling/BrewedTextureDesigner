//! Files dropped on a window on Wayland, where the window does not say them.
//!
//! On X11, Windows and macOS a file dropped on a window is the window's own event
//! (`window::Event::FileDropped`). On Wayland winit 0.30, which iced is built on, says nothing
//! of a drag, and could not be made to on GNOME: Mutter sends a drag to one `wl_data_device` of
//! a program only, the one made last, which is the clipboard's (smithay-clipboard, made by iced
//! once a window is there). So it is the clipboard's data device that takes the drags
//! (`patches/smithay-clipboard`, changed for it: `patches/README.md`), and says here what was
//! dropped, on which surface and where.

use std::path::PathBuf;

use iced::window::raw_window_handle::RawWindowHandle;
use iced::{Point, Subscription};

/// Files dropped on a surface of the program.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    /// The `wl_surface` they were dropped on (the address of its proxy: see [`is_on`]).
    pub surface: usize,
    /// Where on it, as the pointer's place in the window is said.
    pub at: Point,
    /// The files of this computer among what was dropped, in its order.
    pub files: Vec<PathBuf>,
}

/// What is dropped on the program's windows on Wayland (nothing anywhere else: there the
/// windows say it themselves).
pub fn subscription() -> Subscription<Dropped> {
    #[cfg(free_unix)]
    {
        Subscription::run(drops)
    }
    #[cfg(not(free_unix))]
    {
        Subscription::none()
    }
}

/// Whether `surface` is the one `window` is drawn on.
pub fn is_on(window: &dyn iced::window::Window, surface: usize) -> bool {
    let Ok(handle) = window.window_handle() else { return false };
    matches!(handle.as_raw(), RawWindowHandle::Wayland(wayland) if wayland.surface.as_ptr() as usize == surface)
}

/// The drops the clipboard's data device says, for as long as the program runs. (It says them
/// to one listener: this, which is in every subscription of the program.)
#[cfg(free_unix)]
fn drops() -> impl futures::Stream<Item = Dropped> {
    let (said, heard) = futures::channel::mpsc::unbounded();
    if let Some(drops) = smithay_clipboard::dropped_files() {
        // (They come from the clipboard's thread through a channel that is waited on: on a
        // thread of its own, which sleeps until something is dropped.)
        let waited = std::thread::Builder::new().name("dropped-files".into()).spawn(move || {
            while let Ok(drop) = drops.recv() {
                let dropped =
                    Dropped { surface: drop.surface, at: Point::new(drop.x as f32, drop.y as f32), files: crate::copied_files::of_uri_list(&drop.uri_list) };
                if said.unbounded_send(dropped).is_err() {
                    break;
                }
            }
        });
        if let Err(e) = waited {
            log::warn!("files dropped on the window on Wayland are not taken: {e}");
        }
    }
    heard
}
