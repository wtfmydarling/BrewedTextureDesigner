//! The main window is never larger than the GPU can draw. What is drawn into a window, and what
//! iced draws it with, are textures of its size, of at most [`gpu::largest_texture`] a side.
//! Larger, the window's surface cannot be made, and iced keeps a texture for it that could not
//! be made either: the window stays empty for good, whatever its size after. Windows makes a
//! window that large as it moves onto a display of a larger scale: one moved onto a television
//! at 300 % grows 2.4 times.

use iced::{Size, Task, window};

use crate::app::Message;
use crate::gpu;

/// Hold the window `id` to that size: as iced holds a window's size, what it is dragged to on
/// every system (iced keeps it in pixels, whatever the scale later), and on Windows, every size
/// it is given, whoever gives it: the system too, as the window moves between displays, which
/// iced's hold does not reach.
pub fn hold(id: window::Id) -> Task<Message> {
    let largest = gpu::largest_texture() as f32;
    let dragged = window::scale_factor(id).then(move |system| {
        let side = largest / system.max(0.01);
        window::set_max_size(id, Some(Size::new(side, side)))
    });
    #[cfg(windows)]
    let given = window::run(id, windows::hold).discard();
    #[cfg(not(windows))]
    let given = Task::none();
    Task::batch([dragged, given])
}

#[cfg(windows)]
mod windows {
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOSIZE, WINDOWPOS, WM_WINDOWPOSCHANGING};

    /// Which of the window's subclasses this is.
    const SUBCLASS: usize = 0x6272_6577; // "brew"

    /// Every size `window` is given, held to what the GPU can draw, before anything else sees
    /// it (winit answers the message itself, so what Windows would hold it to is never asked).
    pub fn hold(window: &dyn iced::window::Window) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
        let hwnd = win32.hwnd.get() as HWND;
        // SAFETY: a live window, on the thread that owns it (iced runs this there); the
        // procedure is a plain function, alive as long as the program.
        unsafe { SetWindowSubclass(hwnd, Some(held), SUBCLASS, 0) };
    }

    /// The window's messages, before winit's: a new size of the window no larger than a
    /// texture may be.
    unsafe extern "system" fn held(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
        if message == WM_WINDOWPOSCHANGING && lparam != 0 {
            let largest = i32::try_from(crate::gpu::largest_texture()).unwrap_or(i32::MAX);
            // SAFETY: the message's lparam is the WINDOWPOS of the change, to be changed in place.
            let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
            if pos.flags & SWP_NOSIZE == 0 {
                pos.cx = pos.cx.min(largest);
                pos.cy = pos.cy.min(largest);
            }
        }
        // SAFETY: the message as it came, to the procedures after this one.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}
