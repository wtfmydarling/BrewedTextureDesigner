//! The program's windows besides the main one that stay with it (the Color Editor's): above the
//! main window, never under it when the main window is clicked, and not above other programs;
//! made small with it. An owned window on Windows (no button of its own in the taskbar), a
//! transient one on X11, a floating panel that hides while the program is not the one in use
//! on macOS. Wayland lets no program say so: there it is a window as others are.
//!
//! Where several screens sit side by side, such a window is put where it was in the screens'
//! own pixels, which every screen agrees on (a place said in the program's points means other
//! places on screens of other scales): on Windows and on X11. It is put on a screen that is
//! there ([`fit`]): the one it was on may be gone since. macOS says places in points all its
//! screens share, and keeps windows on them itself; Wayland lets no program say where a window
//! is.

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

/// Where a window is on the screens, and how large it is: in the screens' pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The room a screen has for windows (without its taskbar, where the system says), in the
/// screens' pixels: a screen left of the first, or above it, is at places under 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Where a window wanted at `place` is put, the screens being `screens`: on the screen it is
/// most on — the nearest, where it is on none (the screen it was on is gone, or it was put past
/// every edge) — whole on it where it fits, its top left corner on it where it does not. As it
/// is wanted, where no screen is known.
#[cfg_attr(not(any(windows, free_unix, test)), allow(dead_code))]
pub fn fit(place: Place, screens: &[Area]) -> Place {
    let sides = |x: i32, y: i32, width: i32, height: i32| (i128::from(x), i128::from(y), i128::from(x) + i128::from(width), i128::from(y) + i128::from(height));
    let (left, top, right, bottom) = sides(place.x, place.y, place.width, place.height);
    // How much of the window a screen holds, and how far the screen is from it.
    let held = |a: &Area| {
        let (l, t, r, b) = sides(a.x, a.y, a.width, a.height);
        (right.min(r) - left.max(l)).max(0) * (bottom.min(b) - top.max(t)).max(0)
    };
    let away = |a: &Area| {
        let (l, t, r, b) = sides(a.x, a.y, a.width, a.height);
        let (dx, dy) = ((l - right).max(left - r).max(0), (t - bottom).max(top - b).max(0));
        dx * dx + dy * dy
    };
    let Some(screen) = screens.iter().filter(|a| a.width > 0 && a.height > 0).max_by_key(|&a| (held(a), -away(a))) else { return place };
    Place {
        x: place.x.min(screen.x.saturating_add(screen.width).saturating_sub(place.width)).max(screen.x),
        y: place.y.min(screen.y.saturating_add(screen.height).saturating_sub(place.height)).max(screen.y),
        ..place
    }
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

/// Put `window` at `place` (else near the pointer, on the screen the pointer is on), on a
/// screen that is there, whole on it where it fits (see [`fit`]): on Windows and X11 (elsewhere
/// the window is put where its settings say).
#[cfg(any(windows, free_unix))]
pub fn put(window: &dyn iced::window::Window, place: Option<Place>) {
    #[cfg(windows)]
    windows::put(window, place);
    #[cfg(free_unix)]
    x11::put(window, place);
}

/// Where `window` is now: on Windows and X11 (elsewhere the program's points say it).
pub fn place_of(window: &dyn iced::window::Window) -> Option<Place> {
    #[cfg(windows)]
    {
        windows::place_of(window)
    }
    #[cfg(free_unix)]
    {
        x11::place_of(window)
    }
    #[cfg(not(any(windows, free_unix)))]
    {
        let _ = window;
        None
    }
}

/// Where a window that is `now` is wanted: at `place`, else near the pointer at `pointer` (its
/// corner a little left of it and below), as large as it is.
#[cfg(any(windows, free_unix))]
fn wanted(place: Option<Place>, pointer: (i32, i32), now: Place) -> Place {
    place.unwrap_or(Place { x: pointer.0 - 120, y: pointer.1 + 18, ..now })
}

#[cfg(windows)]
mod windows {
    use super::{Area, Owner, Place};
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
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

    /// The room of every screen for windows: without its taskbar.
    pub(super) fn screens() -> Vec<Area> {
        unsafe extern "system" fn add(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            // SAFETY: a monitor the system gave and a MONITORINFO of its size; `data` is the
            // list this was called with, alive for the call.
            unsafe {
                if GetMonitorInfoW(monitor, &mut info) != 0 {
                    let r = info.rcWork;
                    (*(data as *mut Vec<Area>)).push(Area { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top });
                }
            }
            1
        }
        let mut areas: Vec<Area> = Vec::new();
        // SAFETY: the callback gets the list it fills, alive for the call.
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(add), &mut areas as *mut Vec<Area> as LPARAM) };
        areas
    }

    pub fn put(window: &dyn iced::window::Window, place: Option<Place>) {
        let Some(hwnd) = hwnd_of(window) else { return };
        let Some(now) = place_of(window) else { return };
        let mut pointer = POINT::default();
        // SAFETY: a POINT to fill.
        unsafe { GetCursorPos(&mut pointer) };
        let place = super::fit(super::wanted(place, (pointer.x, pointer.y), now), &screens());
        // Moved first, then sized: moved onto a screen of another scale, the system scales it
        // (and winit with it), which a size given with the move would be scaled by.
        // SAFETY: a live window of this thread.
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), place.x, place.y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
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
    use super::{Area, Owner, Place};
    use iced::window::raw_window_handle::RawWindowHandle;
    use x11rb::connection::Connection;
    use x11rb::errors::ReplyError;
    use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt as _, PropMode};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    /// The X11 window `window` is (none on Wayland), a connection to its server (one of its own:
    /// windows are the server's, whoever made them) and the root of its screen.
    fn reach(window: &dyn iced::window::Window) -> Option<(RustConnection, u32, u32)> {
        let handle = window.window_handle().ok()?;
        let own = match handle.as_raw() {
            RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok()?,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return None,
        };
        let (connection, screen) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        Some((connection, own, root))
    }

    /// Said transient for the main window: the window manager keeps it above it, out of the
    /// taskbar.
    pub fn own(window: &dyn iced::window::Window, owner: Owner) {
        let Owner::X11(owner) = owner else { return };
        let Some((connection, own, _)) = reach(window) else { return };
        // (Checked: the server has it before the connection closes.)
        let said = connection
            .change_property32(PropMode::REPLACE, own, AtomEnum::WM_TRANSIENT_FOR, AtomEnum::WINDOW, &[owner])
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        match said {
            Ok(()) => log::info!("the window {own:#x} is kept with the main one, {owner:#x} (transient for it)"),
            Err(e) => log::warn!("a window could not be kept with the main one: {e}"),
        }
    }

    /// The screens, as the server says them (RandR's monitors; none said: one, the whole root).
    pub(super) fn screens(connection: &RustConnection, root: u32) -> Vec<Area> {
        let monitors = connection.randr_get_monitors(root, true).ok().and_then(|cookie| cookie.reply().ok());
        let mut areas: Vec<Area> = monitors.map_or_else(Vec::new, |reply| {
            reply.monitors.iter().map(|m| Area { x: i32::from(m.x), y: i32::from(m.y), width: i32::from(m.width), height: i32::from(m.height) }).collect()
        });
        areas.retain(|a| a.width > 0 && a.height > 0);
        if areas.is_empty()
            && let Some(whole) = connection.get_geometry(root).ok().and_then(|cookie| cookie.reply().ok())
        {
            areas.push(Area { x: 0, y: 0, width: i32::from(whole.width), height: i32::from(whole.height) });
        }
        areas
    }

    pub(super) fn geometry(connection: &RustConnection, own: u32, root: u32) -> Result<Place, ReplyError> {
        let size = connection.get_geometry(own)?.reply()?;
        let at = connection.translate_coordinates(own, root, 0, 0)?.reply()?;
        Ok(Place { x: i32::from(at.dst_x), y: i32::from(at.dst_y), width: i32::from(size.width), height: i32::from(size.height) })
    }

    /// The window `own` put at `place` (else near the pointer), as large as it is: said to the
    /// server, and to the window manager, which puts a window being shown where it pleases
    /// unless the window's hints say where the user had it.
    pub(super) fn put_at(connection: &RustConnection, own: u32, root: u32, place: Option<Place>) -> Result<Place, ReplyError> {
        let now = geometry(connection, own, root)?;
        let pointer = connection.query_pointer(root)?.reply()?;
        let wanted = super::wanted(place.map(|p| Place { width: now.width, height: now.height, ..p }), (i32::from(pointer.root_x), i32::from(pointer.root_y)), now);
        let place = super::fit(wanted, &screens(connection, root));
        let mut hints = WmSizeHints::get_normal_hints(connection, own)?.reply()?.unwrap_or_default();
        hints.position = Some((WmSizeHintsSpecification::UserSpecified, place.x, place.y));
        hints.set_normal_hints(connection, own)?.check()?;
        connection.configure_window(own, &ConfigureWindowAux::new().x(place.x).y(place.y))?.check()?;
        Ok(place)
    }

    /// Before the window is shown (which is when the window manager looks).
    pub fn put(window: &dyn iced::window::Window, place: Option<Place>) {
        let Some((connection, own, root)) = reach(window) else { return };
        match put_at(&connection, own, root, place) {
            Ok(at) => log::info!("the window {own:#x} is put at {},{} ({} by {})", at.x, at.y, at.width, at.height),
            Err(e) => log::warn!("a window could not be put where it was: {e}"),
        }
    }

    pub fn place_of(window: &dyn iced::window::Window) -> Option<Place> {
        let (connection, own, root) = reach(window)?;
        geometry(&connection, own, root).ok()
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

#[cfg(test)]
mod tests {
    use super::*;

    const fn place(x: i32, y: i32, width: i32, height: i32) -> Place {
        Place { x, y, width, height }
    }

    const fn area(x: i32, y: i32, width: i32, height: i32) -> Area {
        Area { x, y, width, height }
    }

    /// Three screens of other sizes side by side, as a desk has them (and of other scales: a
    /// place in their pixels is the one thing they agree on): one of 1920 by 1080 on the left
    /// (at places under 0: left of the first), the first of 2560 by 1440, a television of 3840
    /// by 2160 on the right; none level with another, each without a taskbar 48 pixels high.
    const DESK: [Area; 3] = [area(-1920, 180, 1920, 1032), area(0, 0, 2560, 1392), area(2560, -360, 3840, 2112)];

    fn whole_on(p: Place, a: Area) -> bool {
        p.x >= a.x && p.y >= a.y && p.x + p.width <= a.x + a.width && p.y + p.height <= a.y + a.height
    }

    /// A window is put where it was on any of several screens, in their own pixels: left as it
    /// is where it is whole on one (the left one too, at places under 0), brought whole onto the
    /// screen it is most on where it hangs over an edge or over two screens, and onto the
    /// nearest where it is on none — the screen it was on gone since, or a place past every
    /// edge. A window larger than a screen has its top left corner on it (its title bar in
    /// reach). Nothing is moved where no screen is known.
    #[test]
    fn a_window_is_put_on_a_screen_that_is_there() {
        let editor = |x, y| place(x, y, 750, 425);
        // Whole on a screen: where it was, on each.
        for at in [editor(-1500, 400), editor(900, 500), editor(4000, 1000), editor(2560, -360), editor(-1920, 180)] {
            assert_eq!(fit(at, &DESK), at, "{at:?}");
        }
        // Over the edge between the first and the television: whole on the one it is most on.
        assert_eq!(fit(editor(2300, 300), &DESK), editor(2560, 300), "most on the television");
        assert_eq!(fit(editor(2000, 300), &DESK), editor(1810, 300), "most on the first");
        // Under the bottom of the left one (its taskbar, and the screens are not level).
        assert_eq!(fit(editor(-1000, 1000), &DESK), editor(-1000, 787));
        // The television gone: what was on it, whole on the nearest (the first).
        assert_eq!(fit(editor(5000, 1500), &DESK[..2]), editor(1810, 967));
        // The left one gone: back onto the first.
        assert_eq!(fit(editor(-1500, 400), &DESK[1..]), editor(0, 400));
        // Past every edge (a place nothing on the desk has): the nearest screen.
        assert!(whole_on(fit(editor(-90_000, 70_000), &DESK), DESK[0]));
        assert!(whole_on(fit(editor(i32::MAX - 10, i32::MIN + 10), &DESK), DESK[2]));
        assert!(whole_on(fit(editor(i32::MIN, i32::MAX), &DESK), DESK[0]));
        // Larger than its screen: its corner on the one it is most on.
        assert_eq!(fit(place(-1800, 300, 4000, 3000), &DESK), place(0, 0, 4000, 3000));
        // No screen known, or none with any room: as it is.
        assert_eq!(fit(editor(5, 5), &[]), editor(5, 5));
        assert_eq!(fit(editor(5, 5), &[area(0, 0, 0, 0)]), editor(5, 5));
    }

    /// Whatever the screens and wherever a window is wanted: it is put with its corner on a
    /// screen, whole on one where it was whole on one (and then not moved), as large as it was.
    #[test]
    fn any_window_on_any_screens_is_within_reach() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut roll = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n) as i32
        };
        for round in 0..4000 {
            let screens: Vec<Area> = (0..1 + roll(4)).map(|_| area(roll(9000) - 4500, roll(5000) - 2500, 640 + roll(3840), 480 + roll(2160))).collect();
            let wanted = place(roll(20000) - 10000, roll(12000) - 6000, 100 + roll(2500), 80 + roll(1500));
            let put = fit(wanted, &screens);
            assert_eq!((put.width, put.height), (wanted.width, wanted.height), "round {round}: its size is its own");
            let corner_on = |a: &Area| put.x >= a.x && put.x < a.x + a.width && put.y >= a.y && put.y < a.y + a.height;
            assert!(screens.iter().any(corner_on), "round {round}: {put:?} has its corner on none of {screens:?}");
            if screens.iter().any(|a| whole_on(wanted, *a)) {
                assert_eq!(put, wanted, "round {round}: whole on a screen, and moved");
            }
            if screens.iter().all(|a| wanted.width <= a.width && wanted.height <= a.height) {
                assert!(screens.iter().any(|a| whole_on(put, *a)), "round {round}: {put:?} fits every screen of {screens:?}, and is whole on none");
            }
            // (Put again where it is whole on a screen, it stays.)
            if screens.iter().any(|a| whole_on(put, *a)) {
                assert_eq!(fit(put, &screens), put, "round {round}");
            }
        }
    }

    /// The screens the system has now are ones a window can be put on: each with room, and the
    /// window of the editor, wanted anywhere, lands on one of them.
    #[cfg(windows)]
    #[test]
    fn the_systems_screens_have_room() {
        let screens = windows::screens();
        // (A session without a display — a service's — has none: nothing to say then.)
        for a in &screens {
            assert!(a.width > 0 && a.height > 0, "{a:?}");
        }
        if !screens.is_empty() {
            let put = fit(place(i32::MIN / 2, i32::MAX / 2, 750, 425), &screens);
            assert!(screens.iter().any(|a| put.x >= a.x && put.x < a.x + a.width && put.y >= a.y && put.y < a.y + a.height), "{put:?} of {screens:?}");
        }
    }

    /// On an X server that is only for the test (as the eyedropper's:
    /// `Xvfb :98 -screen 0 5760x2160x24 & DISPLAY=:98 cargo test -p brewed-texture-designer -- --ignored --test-threads 1 tool_window`),
    /// made to have two screens side by side, a window is put where it was on either, whole on
    /// one where it hung over an edge, back onto the one left when the other is gone, and near
    /// the pointer where it was nowhere yet; its hints say the place is the user's.
    #[cfg(free_unix)]
    #[test]
    #[ignore = "needs an X server only for it (Xvfb): see the test"]
    fn a_window_is_put_on_the_screens_of_an_x_server() {
        use x11rb::connection::Connection;
        use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
        use x11rb::protocol::randr::{ConnectionExt as _, MonitorInfo};
        use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};

        let (c, screen) = x11rb::connect(None).expect("an X server (see the test)");
        let root = &c.setup().roots[screen];
        let (root, wide, high) = (root.root, root.width_in_pixels, root.height_in_pixels);
        assert!(wide >= 3000 && high >= 1000, "the server's screen is {wide} by {high}: too small for two side by side (see the test)");
        let atom = |name: &str| c.intern_atom(false, name.as_bytes()).unwrap().reply().unwrap().atom;
        let (left, right) = (atom("brewed-left"), atom("brewed-right"));
        // (The left one takes the server's own output, so that the server's own screen — the
        // whole of it — is not said beside the two.)
        let output = c.randr_get_screen_resources_current(root).unwrap().reply().unwrap().outputs.first().copied();
        let half = wide / 2;
        let monitor = |name, x: u16, width: u16, outputs: Vec<u32>| MonitorInfo {
            name,
            primary: x == 0,
            automatic: false,
            x: x as i16,
            y: 0,
            width,
            height: high,
            width_in_millimeters: u32::from(width) / 4,
            height_in_millimeters: u32::from(high) / 4,
            outputs,
        };
        c.randr_set_monitor(root, monitor(left, 0, half, output.into_iter().collect())).unwrap().check().unwrap();
        c.randr_set_monitor(root, monitor(right, half, wide - half, Vec::new())).unwrap().check().unwrap();
        let (half, wide, high) = (i32::from(half), i32::from(wide), i32::from(high));
        let two = x11::screens(&c, root);
        assert_eq!(two.len(), 2, "{two:?}");
        assert!(two.contains(&area(0, 0, half, high)) && two.contains(&area(half, 0, wide - half, high)), "{two:?}");

        let own = c.generate_id().unwrap();
        c.create_window(x11rb::COPY_DEPTH_FROM_PARENT, own, root, 10, 10, 750, 425, 0, WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &CreateWindowAux::new()).unwrap().check().unwrap();
        let put = |at: Option<(i32, i32)>| {
            let put = x11::put_at(&c, own, root, at.map(|(x, y)| place(x, y, 750, 425))).unwrap();
            assert_eq!(x11::geometry(&c, own, root).unwrap(), put, "the server has it elsewhere");
            let hints = WmSizeHints::get_normal_hints(&c, own).unwrap().reply().unwrap().expect("its hints");
            assert!(matches!(hints.position, Some((WmSizeHintsSpecification::UserSpecified, x, y)) if (x, y) == (put.x, put.y)), "{:?}", hints.position);
            (put.x, put.y)
        };
        // Where it was, on either.
        assert_eq!(put(Some((300, 200))), (300, 200));
        assert_eq!(put(Some((half + 900, 700))), (half + 900, 700));
        // Over the edge between them: whole on the one it is most on.
        assert_eq!(put(Some((half - 100, 50))), (half, 50));
        assert_eq!(put(Some((half - 600, 50))), (half - 750, 50));
        // Past the bottom right corner: whole in it.
        assert_eq!(put(Some((wide + 500, high + 500))), (wide - 750, high - 425));
        // Nowhere yet: near the pointer, on a screen.
        let pointer = c.query_pointer(root).unwrap().reply().unwrap();
        let near = put(None);
        let by = fit(place(i32::from(pointer.root_x) - 120, i32::from(pointer.root_y) + 18, 750, 425), &two);
        assert_eq!(near, (by.x, by.y));
        // The right one gone: what was on it comes back onto the one left.
        c.randr_delete_monitor(root, right).unwrap().check().unwrap();
        assert_eq!(x11::screens(&c, root), vec![area(0, 0, half, high)]);
        assert_eq!(put(Some((half + 900, 700))), (half - 750, 700));

        c.randr_delete_monitor(root, left).unwrap().check().unwrap();
        c.destroy_window(own).unwrap().check().unwrap();
    }
}
