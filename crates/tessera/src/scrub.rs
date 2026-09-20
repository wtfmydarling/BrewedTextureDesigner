//! Dragging a value as far as the user likes.
//!
//! A number field is scrubbed by dragging it sideways, and the screen is only so wide: at
//! its edge the mouse stops and the value stops with it. So while a value is dragged the
//! pointer is put back where the drag started after every move, and only the distance it
//! travelled counts. The pointer never reaches an edge, the cursor is hidden meanwhile,
//! and it is right where the user left it when the button comes up.
//!
//! Windows, X11 and macOS let a program move the pointer. Wayland does not: there the
//! drag stays what it was, bounded by the screen.

/// Where the pointer was when a drag started, in the screen's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    x: f64,
    y: f64,
}

impl Anchor {
    /// Remember where the pointer is, to put it back there after every move. `None` where
    /// a program may not move the pointer (Wayland), so the caller keeps to plain dragging.
    pub fn hold() -> Option<Anchor> {
        // A test moves the pointer of whoever runs it, so it never does: only the one the
        // test made up itself.
        #[cfg(test)]
        return fake::position().map(|(x, y)| Anchor { x, y });
        #[cfg(not(test))]
        platform::position().map(|(x, y)| Anchor { x, y })
    }

    /// Put the pointer back where the drag started. False if it could not be done, so the
    /// caller stops trying.
    pub fn recenter(&self) -> bool {
        #[cfg(test)]
        return fake::warp(self.x, self.y);
        #[cfg(not(test))]
        platform::warp(self.x, self.y)
    }
}

/// A pointer the tests make up, to drag without touching the real one.
#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    static POINTER: Mutex<Option<((f64, f64), usize)>> = Mutex::new(None);

    fn lock() -> std::sync::MutexGuard<'static, Option<((f64, f64), usize)>> {
        POINTER.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Put a pointer at `at` for this test; until then, and after [`forget`], a drag finds
    /// none and leaves the pointer alone.
    pub fn place(at: (f64, f64)) {
        *lock() = Some((at, 0));
    }

    pub fn forget() {
        *lock() = None;
    }

    /// How many times the pointer has been put back.
    pub fn recentered() -> usize {
        lock().map_or(0, |(_, n)| n)
    }

    pub(super) fn position() -> Option<(f64, f64)> {
        lock().map(|(at, _)| at)
    }

    pub(super) fn warp(x: f64, y: f64) -> bool {
        let mut guard = lock();
        match &mut *guard {
            Some((at, count)) => {
                *at = (x, y);
                *count += 1;
                true
            }
            None => false,
        }
    }
}

#[cfg(windows)]
#[cfg_attr(test, allow(dead_code, reason = "a test drags a pointer of its own, never the one on the desk"))]
mod platform {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    pub fn position() -> Option<(f64, f64)> {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: `p` is a valid POINT for the call to fill in.
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f64, p.y as f64))
    }

    pub fn warp(x: f64, y: f64) -> bool {
        // SAFETY: a plain call; the process owns the foreground window while dragging.
        unsafe { SetCursorPos(x as i32, y as i32) != 0 }
    }
}

#[cfg(free_unix)]
#[cfg_attr(test, allow(dead_code, reason = "a test drags a pointer of its own, never the one on the desk"))]
mod platform {
    use std::sync::{Mutex, OnceLock};

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, Window};
    use x11rb::rust_connection::RustConnection;

    /// A connection of our own to the X server: winit has one, but it belongs to its event
    /// loop. Opened once, on the first drag.
    fn x11() -> Option<&'static Mutex<(RustConnection, Window)>> {
        static X11: OnceLock<Option<Mutex<(RustConnection, Window)>>> = OnceLock::new();
        X11.get_or_init(|| {
            // Under Wayland, X11 would only see the programs running through Xwayland, and
            // the pointer it moves is not the one the user is dragging with.
            if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("WAYLAND_SOCKET").is_some() {
                return None;
            }
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            Some(Mutex::new((conn, root)))
        })
        .as_ref()
    }

    fn with<T>(f: impl FnOnce(&RustConnection, Window) -> Option<T>) -> Option<T> {
        let guard = x11()?.lock().ok()?;
        let (conn, root) = &*guard;
        f(conn, *root)
    }

    pub fn position() -> Option<(f64, f64)> {
        with(|conn, root| {
            let p = conn.query_pointer(root).ok()?.reply().ok()?;
            p.same_screen.then(|| (p.root_x as f64, p.root_y as f64))
        })
    }

    pub fn warp(x: f64, y: f64) -> bool {
        with(|conn, root| {
            conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, x as i16, y as i16).ok()?.check().ok()?;
            Some(())
        })
        .is_some()
    }
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code, reason = "a test drags a pointer of its own, never the one on the desk"))]
mod platform {
    use std::ffi::c_void;

    use crate::eyedropper::macos::symbol;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    pub fn position() -> Option<(f64, f64)> {
        // An event with no source carries the pointer where it is now, in screen
        // coordinates with the origin at the top left — the same ones the warp takes.
        let create: unsafe extern "C" fn(*const c_void) -> *mut c_void = symbol(c"CGEventCreate")?;
        let location: unsafe extern "C" fn(*mut c_void) -> CGPoint = symbol(c"CGEventGetLocation")?;
        let release: unsafe extern "C" fn(*mut c_void) = symbol(c"CFRelease")?;
        // SAFETY: the symbols come from CoreGraphics with the signatures above; the event
        // is released right after its location is read.
        unsafe {
            let event = create(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let p = location(event);
            release(event);
            (p.x.is_finite() && p.y.is_finite()).then_some((p.x, p.y))
        }
    }

    pub fn warp(x: f64, y: f64) -> bool {
        let Some(warp): Option<unsafe extern "C" fn(CGPoint) -> i32> = symbol(c"CGWarpMouseCursorPosition") else {
            return false;
        };
        // Moving the pointer makes the system ignore the mouse for a quarter of a second,
        // which would swallow the rest of the drag; linking them again ends that at once.
        let associate: Option<unsafe extern "C" fn(i32) -> i32> = symbol(c"CGAssociateMouseAndMouseCursorPosition");
        // SAFETY: both are plain CoreGraphics calls with the signatures above.
        unsafe {
            let ok = warp(CGPoint { x, y }) == 0;
            if let Some(associate) = associate {
                associate(1);
            }
            ok
        }
    }
}

#[cfg(not(any(windows, free_unix, target_os = "macos")))]
#[cfg_attr(test, allow(dead_code, reason = "a test drags a pointer of its own, never the one on the desk"))]
mod platform {
    pub fn position() -> Option<(f64, f64)> {
        None
    }

    pub fn warp(_x: f64, _y: f64) -> bool {
        false
    }
}
