//! The program's windows besides the main one that stay with it (the Color Editor's): above the
//! main window, never under it when the main window is clicked, and not above other programs;
//! made small with it. An owned window on Windows (no button of its own in the taskbar), a
//! transient one on X11, a floating panel that hides while the program is not the one in use
//! on macOS. Wayland lets no program say so: there it is a window as others are.
//!
//! On Windows, where screens of other scales sit side by side, such a window is put where it
//! was in the screens' own pixels, which every screen agrees on (a place said in the program's
//! points meant other places on screens of other scales).

/// What a window is owned by: the main window, as the system knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    #[cfg(windows)]
    Win32(isize),
    #[cfg(free_unix)]
    X11(u32),
    /// Nothing to own by (Wayland), or what needs no owner (macOS: the window floats).
    None,
}

/// Where a window is on the screens, and how large it is: in the screens' pixels (Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// What `window` (the main one) is, to own others by.
pub fn owner_of(window: &dyn iced::window::Window) -> Owner {
    #[cfg(any(windows, free_unix))]
    {
        use iced::window::raw_window_handle::RawWindowHandle;
        let Ok(handle) = window.window_handle() else { return Owner::None };
        match handle.as_raw() {
            #[cfg(windows)]
            RawWindowHandle::Win32(h) => Owner::Win32(h.hwnd.get()),
            #[cfg(free_unix)]
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).map_or(Owner::None, Owner::X11),
            #[cfg(free_unix)]
            RawWindowHandle::Xcb(h) => Owner::X11(h.window.get()),
            _ => Owner::None,
        }
    }
    #[cfg(not(any(windows, free_unix)))]
    {
        let _ = window;
        Owner::None
    }
}

/// Make `window` stay with `owner` (see the module): before it is shown, which is when the
/// systems look.
pub fn own(window: &dyn iced::window::Window, owner: Owner) {
    #[cfg(windows)]
    windows::own(window, owner);
    #[cfg(free_unix)]
    x11::own(window, owner);
    #[cfg(target_os = "macos")]
    {
        let _ = owner;
        macos::own(window);
    }
    #[cfg(not(any(windows, free_unix, target_os = "macos")))]
    let _ = (window, owner);
}

/// Put `window` at `place` (else near the pointer, on the screen the pointer is on), whole on
/// that screen where it fits: on Windows (elsewhere the window is put where its settings say).
#[cfg(windows)]
pub fn put(window: &dyn iced::window::Window, place: Option<Place>) {
    windows::put(window, place);
}

/// Where `window` is now: on Windows (elsewhere the program's points say it).
pub fn place_of(window: &dyn iced::window::Window) -> Option<Place> {
    #[cfg(windows)]
    {
        windows::place_of(window)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

#[cfg(windows)]
mod windows {
    use super::{Owner, Place};
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWLP_HWNDPARENT, GetCursorPos, GetWindowRect, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };

    fn hwnd_of(window: &dyn iced::window::Window) -> Option<HWND> {
        let handle = window.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get() as HWND),
            _ => None,
        }
    }

    pub fn own(window: &dyn iced::window::Window, owner: Owner) {
        let (Some(hwnd), Owner::Win32(owner)) = (hwnd_of(window), owner) else { return };
        // (GWLP_HWNDPARENT of a window that is not a child is its owner.)
        // SAFETY: two live windows of this thread: the one owned, and the main one.
        #[cfg(target_pointer_width = "64")]
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner)
        };
        // SAFETY: as above.
        #[cfg(target_pointer_width = "32")]
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongW(hwnd, GWLP_HWNDPARENT, owner as i32)
        };
    }

    /// The part of the screen `monitor` windows are put in: without the taskbar.
    fn work_area(monitor: HMONITOR) -> Option<RECT> {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: a monitor the system gave, and a MONITORINFO of its size.
        (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then_some(info.rcWork)
    }

    pub fn put(window: &dyn iced::window::Window, place: Option<Place>) {
        let Some(hwnd) = hwnd_of(window) else { return };
        let Some(now) = place_of(window) else { return };
        let place = place.unwrap_or_else(|| {
            // Near the pointer: its corner a little left of it and below.
            let mut at = POINT::default();
            // SAFETY: a POINT to fill.
            unsafe { GetCursorPos(&mut at) };
            Place { x: at.x - 120, y: at.y + 18, ..now }
        });
        // Whole on the screen it is put on (or the nearest), where it fits.
        let wanted = RECT { left: place.x, top: place.y, right: place.x + place.width, bottom: place.y + place.height };
        // SAFETY: a plain call with a RECT.
        let monitor = unsafe { MonitorFromRect(&wanted, MONITOR_DEFAULTTONEAREST) };
        let (mut x, mut y) = (place.x, place.y);
        if let Some(area) = work_area(monitor) {
            x = x.min(area.right - place.width).max(area.left);
            y = y.min(area.bottom - place.height).max(area.top);
        }
        // Moved first, then sized: moved onto a screen of another scale, the system scales it
        // (and winit with it), which a size given with the move would be scaled by.
        // SAFETY: a live window of this thread.
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, place.width, place.height, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    pub fn place_of(window: &dyn iced::window::Window) -> Option<Place> {
        let hwnd = hwnd_of(window)?;
        let mut rect = RECT::default();
        // SAFETY: a live window, and a RECT to fill.
        (unsafe { GetWindowRect(hwnd, &mut rect) } != 0).then(|| Place {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }
}

#[cfg(free_unix)]
mod x11 {
    use super::Owner;
    use iced::window::raw_window_handle::RawWindowHandle;
    use x11rb::protocol::xproto::{AtomEnum, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    /// Said transient for the main window: the window manager keeps it above it, out of the
    /// taskbar. (On a connection of its own: windows are the server's, whoever made them.)
    pub fn own(window: &dyn iced::window::Window, owner: Owner) {
        let Owner::X11(owner) = owner else { return };
        let Ok(handle) = window.window_handle() else { return };
        let own = match handle.as_raw() {
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
            RawWindowHandle::Xcb(h) => Some(h.window.get()),
            _ => None,
        };
        let Some(own) = own else { return };
        let Ok((connection, _)) = x11rb::connect(None) else { return };
        // (Checked: the server has it before the connection closes.)
        let said = connection
            .change_property32(PropMode::REPLACE, own, AtomEnum::WM_TRANSIENT_FOR, AtomEnum::WINDOW, &[owner])
            .map_err(x11rb::errors::ReplyError::from)
            .and_then(|cookie| cookie.check());
        match said {
            Ok(()) => log::info!("the window {own:#x} is kept with the main one, {owner:#x} (transient for it)"),
            Err(e) => log::warn!("a window could not be kept with the main one: {e}"),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use iced::window::raw_window_handle::RawWindowHandle;
    use objc2_app_kit::{NSFloatingWindowLevel, NSView};

    /// A floating panel: above the program's windows, hidden while another program is in use.
    pub fn own(window: &dyn iced::window::Window) {
        let Ok(handle) = window.window_handle() else { return };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return };
        // SAFETY: the handle is the live NSView of one of the app's windows, used on the main
        // thread (where window tasks run).
        let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
        if let Some(window) = view.window() {
            window.setLevel(NSFloatingWindowLevel);
            window.setHidesOnDeactivate(true);
        }
    }
}
