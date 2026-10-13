//! Live picking on Windows.
//!
//! Every monitor is covered by a window that cannot be seen (1/255 opaque: enough for
//! the clicks to land on it) and is left out of screen captures. Nothing is captured in
//! advance: the pixels under the mouse are read from the screen with GDI as it moves,
//! so videos keep playing and what is picked is what is on the screen at that moment.
//!
//! - One color: the cursor is a loupe of the pixels around it, a click picks.
//! - A gradient: dragging draws a black path (in a click-through window, also left out
//!   of captures) and the colors under it become the gradient on release.
//!
//! Esc (a hotkey while picking), a right click or a change of the monitors cancels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};
use std::sync::Once;
use std::time::{Duration, Instant};

use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACK_BRUSH, BLENDFUNCTION, BitBlt, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GdiFlush, GetDC, GetStockObject, HBITMAP, HDC, HGDIOBJ, HMONITOR, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, MOD_NOREPEAT, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, VK_ESCAPE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HCURSOR, ICONINFO, IDC_ARROW,
    IDC_CROSS, KillTimer, LWA_ALPHA, LoadCursorW, MA_NOACTIVATE, MSG, PostThreadMessageW, RegisterClassExW, SW_SHOWNOACTIVATE, SetCursor,
    SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity, ShowWindow, ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
    WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_CAPTURECHANGED, WM_DISPLAYCHANGE, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::loupe::{self, Loupe};
use super::path::{PathSampler, Pixel};
use super::stroke::{self, Area, Stroke, segment_distance};
use super::{Cancel, Mode, Outcome, color};

const CLASS: *const u16 = windows_sys::w!("BrewedEyedropper");
/// Posted to the picking thread when the app cancels.
const WM_CANCEL: u32 = WM_APP + 1;
const HOTKEY_ESCAPE: i32 = 1;
const TIMER: usize = 1;
/// The loupe follows what is on the screen at least this often while the mouse rests.
const REFRESH: Duration = Duration::from_millis(33);
/// ... and at most this often while it moves.
const MIN_REFRESH: Duration = Duration::from_millis(8);

/// Pick on the calling thread (it runs its own message loop until the pick ends).
pub fn pick(mode: Mode, cancel: &Cancel) -> Outcome {
    // SAFETY: plain Win32 calls; every handle is created, used and destroyed on this
    // thread, and the window procedure only touches the thread's own state.
    let thread = unsafe { GetCurrentThreadId() };
    cancel.on_cancel(move || {
        // SAFETY: posting to a thread that may have exited merely fails.
        unsafe { PostThreadMessageW(thread, WM_CANCEL, 0, 0) };
    });
    unsafe {
        // Physical pixels everywhere, whatever the scale of each monitor.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let picker = match Picker::new(mode) {
            Ok(picker) => picker,
            Err(e) => return Outcome::Failed(e),
        };
        PICKER.with(|p| *p.borrow_mut() = Some(picker));
        let outcome = run(cancel);
        // Out of the thread's state first: messages sent while tearing down go to the
        // default window procedure.
        let picker = PICKER.with(|p| p.borrow_mut().take());
        drop(picker);
        outcome
    }
}

thread_local! {
    static PICKER: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

/// The message loop, until the pick has an outcome.
unsafe fn run(cancel: &Cancel) -> Outcome {
    let mut msg: MSG = unsafe { zeroed() };
    loop {
        if let Some(outcome) = PICKER.with(|p| p.borrow_mut().as_mut().and_then(|p| p.outcome.take())) {
            return outcome;
        }
        if cancel.is_cancelled() {
            return Outcome::Cancelled;
        }
        // SAFETY: `msg` is a valid out pointer.
        match unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } {
            0 | -1 => return Outcome::Cancelled,
            _ => {}
        }
        if msg.hwnd.is_null() && msg.message == WM_CANCEL {
            return Outcome::Cancelled;
        }
        // SAFETY: a message just retrieved for this thread.
        unsafe { DispatchMessageW(&msg) };
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Messages sent while the state is borrowed (Win32 calls made while handling another
    // message) get the default handling. A panic ends the pick instead of the program.
    let handled = catch_unwind(AssertUnwindSafe(|| {
        PICKER.with(|p| match p.try_borrow_mut() {
            Ok(mut picker) => picker.as_mut().and_then(|picker| picker.message(hwnd, msg, wparam, lparam)),
            Err(_) => None,
        })
    }));
    match handled {
        Ok(Some(result)) => result,
        Ok(None) => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        Err(_) => {
            PICKER.with(|p| {
                if let Ok(mut picker) = p.try_borrow_mut()
                    && let Some(picker) = picker.as_mut()
                {
                    picker.outcome = Some(Outcome::Failed("The eyedropper stopped unexpectedly".into()));
                }
            });
            0
        }
    }
}

/// The cursor showing the loupe, and what it was made from.
struct LoupeCursor {
    cursor: HCURSOR,
    shown: Vec<Option<[u8; 3]>>,
    /// The display scale it was made for.
    scale: u32,
}

struct Monitor {
    rect: RECT,
    /// The window covering it.
    window: HWND,
    scale: f32,
}

struct Picker {
    mode: Mode,
    monitors: Vec<Monitor>,
    screen: Screen,
    /// Windows can leave the picker's windows out of what is read from the screen.
    excluded: bool,
    hotkey: bool,
    /// The loupe cursor (one color) and what it shows.
    loupe: Option<LoupeCursor>,
    refreshed: Option<(Instant, POINT)>,
    /// The left button went down on the picker (its release picks).
    pressed: bool,
    path: PathSampler,
    /// The colors along the path, in order.
    colors: Vec<[f32; 4]>,
    line: Option<Line>,
    /// Colors under the drawn path, read before it covered them (needed when the
    /// picker's windows show up in what is read from the screen).
    hidden: HashMap<Pixel, [f32; 4]>,
    outcome: Option<Outcome>,
}

impl Picker {
    unsafe fn new(mode: Mode) -> Result<Picker, String> {
        static REGISTER: Once = Once::new();
        let instance = unsafe { GetModuleHandleW(null()) };
        REGISTER.call_once(|| {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                // The cursor is set on every WM_SETCURSOR.
                hCursor: null_mut(),
                hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) },
                lpszClassName: CLASS,
                // SAFETY: all other fields are integers or nullable handles.
                ..unsafe { zeroed() }
            };
            unsafe { RegisterClassExW(&class) };
        });

        let screen = unsafe { Screen::new() }.ok_or("Cannot read the screen")?;
        let excluded = capture_exclusion_available();
        #[cfg(test)]
        let excluded = excluded && !tests::NO_EXCLUSION.load(std::sync::atomic::Ordering::SeqCst);
        let mut rects: Vec<RECT> = Vec::new();
        unsafe extern "system" fn add(_: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> i32 {
            // SAFETY: `data` is the vector passed below, `rect` the monitor's rectangle.
            unsafe { (*(data as *mut Vec<RECT>)).push(*rect) };
            1
        }
        unsafe { EnumDisplayMonitors(null_mut(), null(), Some(add), &mut rects as *mut Vec<RECT> as LPARAM) };
        let mut picker = Picker {
            mode,
            monitors: Vec::new(),
            screen,
            excluded,
            hotkey: false,
            loupe: None,
            refreshed: None,
            pressed: false,
            path: PathSampler::default(),
            colors: Vec::new(),
            line: None,
            hidden: HashMap::new(),
            outcome: None,
        };
        for rect in rects {
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            if w <= 0 || h <= 0 {
                continue;
            }
            let window = unsafe {
                CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    CLASS,
                    null(),
                    WS_POPUP,
                    rect.left,
                    rect.top,
                    w,
                    h,
                    null_mut(),
                    null_mut(),
                    instance,
                    null(),
                )
            };
            if window.is_null() {
                continue;
            }
            unsafe {
                // Invisible, but opaque enough to be clicked.
                SetLayeredWindowAttributes(window, 0, 1, LWA_ALPHA);
                prepare(window, excluded);
            }
            let dpi = unsafe { GetDpiForWindow(window) };
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            #[cfg(test)]
            super::lock(&tests::OVERLAYS).push(window as isize);
            picker.monitors.push(Monitor { rect, window, scale });
        }
        let Some(first) = picker.monitors.first().map(|m| m.window) else {
            return Err("No monitor to pick from".into());
        };
        unsafe {
            // Esc anywhere cancels (the windows never take the keyboard focus).
            picker.hotkey = RegisterHotKey(first, HOTKEY_ESCAPE, MOD_NOREPEAT, VK_ESCAPE as u32) != 0;
            SetTimer(first, TIMER, 15, None);
        }
        if mode == Mode::Color {
            picker.refresh_loupe();
        }
        for m in &picker.monitors {
            unsafe { ShowWindow(m.window, SW_SHOWNOACTIVATE) };
        }
        picker.set_cursor();
        Ok(picker)
    }

    fn owns(&self, hwnd: HWND) -> bool {
        self.monitors.iter().any(|m| m.window == hwnd)
    }

    fn message(&mut self, hwnd: HWND, msg: u32, _wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => Some(MA_NOACTIVATE as LRESULT),
            WM_SETCURSOR => {
                self.set_cursor();
                Some(1)
            }
            WM_MOUSEMOVE => {
                match self.mode {
                    Mode::Color if self.refreshed.is_none_or(|(at, _)| at.elapsed() >= MIN_REFRESH) => self.refresh_loupe(),
                    Mode::Gradient if self.pressed => {
                        if let Some(p) = cursor() {
                            self.extend_path(p);
                        }
                    }
                    _ => {}
                }
                Some(0)
            }
            WM_LBUTTONDOWN => {
                self.pressed = true;
                unsafe { SetCapture(hwnd) };
                if self.mode == Mode::Gradient {
                    self.path = PathSampler::default();
                    self.colors.clear();
                    self.hidden.clear();
                    self.line = None;
                    if let Some(p) = cursor() {
                        self.extend_path(p);
                    }
                }
                Some(0)
            }
            WM_LBUTTONUP => {
                if !std::mem::take(&mut self.pressed) {
                    return Some(0);
                }
                match self.mode {
                    Mode::Color => {
                        if let Some(c) = cursor().and_then(|p| self.read_pixel(p)) {
                            self.outcome = Some(Outcome::Color(color(c)));
                        }
                    }
                    Mode::Gradient => {
                        if let Some(p) = cursor() {
                            self.extend_path(p);
                        }
                        if self.colors.len() >= 2 {
                            self.outcome = Some(Outcome::Path(std::mem::take(&mut self.colors)));
                        } else {
                            // A click without a drag: nothing to pick yet.
                            self.path = PathSampler::default();
                            self.colors.clear();
                            self.hidden.clear();
                            self.line = None;
                        }
                    }
                }
                unsafe { ReleaseCapture() };
                Some(0)
            }
            WM_RBUTTONUP => {
                self.outcome = Some(Outcome::Cancelled);
                Some(0)
            }
            WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Some(0),
            WM_CAPTURECHANGED => {
                // Another window took the mouse in the middle of a drag.
                if self.pressed && !self.owns(lparam as HWND) {
                    self.outcome = Some(Outcome::Cancelled);
                }
                Some(0)
            }
            WM_TIMER => {
                if self.mode == Mode::Color {
                    let moved = cursor().is_some_and(|p| self.refreshed.is_none_or(|(_, q)| q.x != p.x || q.y != p.y));
                    if moved || self.refreshed.is_none_or(|(at, _)| at.elapsed() >= REFRESH) {
                        self.refresh_loupe();
                    }
                }
                Some(0)
            }
            WM_HOTKEY | WM_DISPLAYCHANGE => {
                self.outcome = Some(Outcome::Cancelled);
                Some(0)
            }
            _ => None,
        }
    }

    fn set_cursor(&self) {
        // SAFETY: shared system cursors and the picker's own loupe cursor.
        unsafe {
            match (&self.loupe, self.mode) {
                (Some(loupe), Mode::Color) => SetCursor(loupe.cursor),
                _ => SetCursor(LoadCursorW(null_mut(), IDC_CROSS)),
            };
        }
    }

    fn monitor_at(&self, p: POINT) -> Option<&Monitor> {
        self.monitors.iter().find(|m| inside(&m.rect, p.x, p.y))
    }

    /// Show the pixels around the cursor in the loupe.
    fn refresh_loupe(&mut self) {
        let Some(p) = cursor() else { return };
        self.refreshed = Some((Instant::now(), p));
        let half = (loupe::PIXELS / 2) as i32;
        let rect = RECT { left: p.x - half, top: p.y - half, right: p.x + half + 1, bottom: p.y + half + 1 };
        let pixels: Vec<Option<[u8; 3]>> = match unsafe { self.screen.read(rect) } {
            Some(read) => (rect.top..rect.bottom)
                .flat_map(|y| (rect.left..rect.right).map(move |x| (x, y)))
                .map(|(x, y)| self.monitors.iter().any(|m| inside(&m.rect, x, y)).then(|| read.pixel(x, y)))
                .collect(),
            None => return,
        };
        let scale = self.monitor_at(p).map_or(1.0, |m| m.scale);
        let key = scale.to_bits();
        if self.loupe.as_ref().is_some_and(|l| l.shown == pixels && l.scale == key) {
            return;
        }
        let Some(cursor) = (unsafe { make_cursor(&loupe::render(&pixels, scale)) }) else { return };
        let previous = self.loupe.replace(LoupeCursor { cursor, shown: pixels, scale: key });
        self.set_cursor();
        if let Some(LoupeCursor { cursor: previous, .. }) = previous {
            // SAFETY: a cursor made by `make_cursor`, no longer the current one.
            unsafe { DestroyIcon(previous) };
        }
    }

    fn read_pixel(&mut self, p: POINT) -> Option<[u8; 3]> {
        self.monitor_at(p)?;
        let read = unsafe { self.screen.read(RECT { left: p.x, top: p.y, right: p.x + 1, bottom: p.y + 1 }) }?;
        Some(read.pixel(p.x, p.y))
    }

    /// Extend the path to the cursor. A fast mouse jumps far between two messages: the
    /// way there is walked in steps, so that no step reads half the screen at once.
    fn extend_path(&mut self, to: POINT) {
        let from = self.path.last_pixel().unwrap_or((to.x, to.y));
        for step in super::path::steps((from.0 as f32, from.1 as f32), (to.x as f32, to.y as f32)) {
            self.extend_path_step(POINT { x: step.0.round() as i32, y: step.1.round() as i32 });
        }
    }

    /// Extend the path by one step: read the colors it crosses, then draw it.
    fn extend_path_step(&mut self, to: POINT) {
        let to = (to.x, to.y);
        let from = self.path.last_pixel().unwrap_or(to);
        let pixels = self.path.advance((to.0 as f32 + 0.5, to.1 as f32 + 0.5));
        let scale = self.monitor_at(POINT { x: from.0, y: from.1 }).map_or(1.0, |m| m.scale);
        let line = self.line.get_or_insert_with(|| Line::new(scale, self.excluded));
        let stroke = line.stroke.bounds(from, to);
        let mut area = RECT { left: stroke.0, top: stroke.1, right: stroke.2, bottom: stroke.3 };
        for &(x, y) in &pixels {
            area = union(area, RECT { left: x, top: y, right: x + 1, bottom: y + 1 });
        }
        let Some(read) = (unsafe { self.screen.read(area) }) else { return };
        for &(x, y) in &pixels {
            if !self.monitors.iter().any(|m| inside(&m.rect, x, y)) {
                continue;
            }
            if self.colors.len() < super::path::MAX_COLORS {
                let c = self.hidden.get(&(x, y)).copied().unwrap_or_else(|| color(read.pixel(x, y)));
                self.colors.push(c);
            }
        }
        if !self.excluded {
            // The path is about to cover these pixels: keep what they showed.
            for y in stroke.1..stroke.3 {
                for x in stroke.0..stroke.2 {
                    if line.stroke.covers(segment_distance(from, to, (x, y))) && !line.covers((x, y)) {
                        self.hidden.entry((x, y)).or_insert_with(|| color(read.pixel(x, y)));
                    }
                }
            }
        }
        unsafe { line.draw(from, to) };
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        // SAFETY: the picker's own handles, released once.
        unsafe {
            if let Some(first) = self.monitors.first() {
                KillTimer(first.window, TIMER);
                if self.hotkey {
                    UnregisterHotKey(first.window, HOTKEY_ESCAPE);
                }
            }
            if self.owns(GetCapture()) {
                ReleaseCapture();
            }
            SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
            self.line = None;
            for m in &self.monitors {
                DestroyWindow(m.window);
            }
            if let Some(loupe) = self.loupe.take() {
                DestroyIcon(loupe.cursor);
            }
        }
    }
}

/// No fade-in, and out of screen captures when Windows can do it.
unsafe fn prepare(window: HWND, excluded: bool) {
    let disabled: i32 = 1;
    unsafe {
        DwmSetWindowAttribute(window, DWMWA_TRANSITIONS_FORCEDISABLED as u32, &disabled as *const i32 as *const c_void, 4);
        if excluded {
            SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

/// Windows 10 2004 (build 19041) and later leave windows out of captures; before that the
/// same setting blacks them out, which would be picked.
fn capture_exclusion_available() -> bool {
    // SAFETY: a valid, sized out structure.
    unsafe {
        let mut info: OSVERSIONINFOW = zeroed();
        info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOW>() as u32;
        RtlGetVersion(&mut info) == 0 && (info.dwMajorVersion, info.dwBuildNumber) >= (10, 19041)
    }
}

fn cursor() -> Option<POINT> {
    #[cfg(test)]
    if let Some((x, y)) = *super::lock(&tests::CURSOR) {
        return Some(POINT { x, y });
    }
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: a valid out pointer.
    (unsafe { GetCursorPos(&mut p) } != 0).then_some(p)
}

fn inside(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

/// A 32-bit top-down DIB section selected into a memory DC.
struct Bitmap {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Bitmap {
    unsafe fn new(width: i32, height: i32) -> Option<Bitmap> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                // SAFETY: the remaining fields are plain integers.
                ..unsafe { zeroed() }
            },
            // SAFETY: an array of plain integers.
            bmiColors: unsafe { zeroed() },
        };
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return None;
            }
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Bitmap { dc, bitmap, old, bits: bits as *mut u8, width, height })
        }
    }

    fn pixels(&self) -> &[u8] {
        // SAFETY: the DIB section holds width × height 32-bit pixels while it lives.
        unsafe { std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize * 4) }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` makes the access exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.width as usize * self.height as usize * 4) }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: the bitmap's own handles.
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// Reads pixels from the screen.
struct Screen {
    dc: HDC,
    buffer: Option<Bitmap>,
}

/// Pixels read from a rectangle of the desktop.
struct Read<'a> {
    rect: RECT,
    width: i32,
    bgra: &'a [u8],
}

impl Read<'_> {
    fn pixel(&self, x: i32, y: i32) -> [u8; 3] {
        let x = (x - self.rect.left).clamp(0, self.rect.right - self.rect.left - 1);
        let y = (y - self.rect.top).clamp(0, self.rect.bottom - self.rect.top - 1);
        let i = (y * self.width + x) as usize * 4;
        [self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]]
    }
}

impl Screen {
    unsafe fn new() -> Option<Screen> {
        let dc = unsafe { GetDC(null_mut()) };
        (!dc.is_null()).then_some(Screen { dc, buffer: None })
    }

    /// The pixels of a rectangle of the desktop, as they are on the screen right now.
    unsafe fn read(&mut self, rect: RECT) -> Option<Read<'_>> {
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return None;
        }
        if self.buffer.as_ref().is_none_or(|b| b.width < w || b.height < h) {
            let (bw, bh) = self.buffer.as_ref().map_or((0, 0), |b| (b.width, b.height));
            self.buffer = None;
            self.buffer = Some(unsafe { Bitmap::new(w.max(bw).max(64), h.max(bh).max(64)) }?);
        }
        let buffer = self.buffer.as_mut()?;
        unsafe {
            if BitBlt(buffer.dc, 0, 0, w, h, self.dc, rect.left, rect.top, SRCCOPY) == 0 {
                return None;
            }
            GdiFlush();
        }
        Some(Read { rect, width: buffer.width, bgra: buffer.pixels() })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.buffer = None;
        // SAFETY: the screen DC from `GetDC(null)`.
        unsafe { ReleaseDC(null_mut(), self.dc) };
    }
}

/// A cursor showing the loupe.
unsafe fn make_cursor(loupe: &Loupe) -> Option<HCURSOR> {
    let size = loupe.size as i32;
    let mut color = unsafe { Bitmap::new(size, size) }?;
    for (dst, src) in color.pixels_mut().chunks_exact_mut(4).zip(loupe.rgba.chunks_exact(4)) {
        dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
    }
    // The mask is required but unused: the color bitmap has alpha.
    let mask_bytes = vec![0u8; (size as usize).div_ceil(16) * 2 * size as usize];
    unsafe {
        let mask = CreateBitmap(size, size, 1, 1, mask_bytes.as_ptr() as *const c_void);
        if mask.is_null() {
            return None;
        }
        // The color bitmap must not be selected into a DC.
        SelectObject(color.dc, color.old);
        let info = ICONINFO { fIcon: 0, xHotspot: loupe.hotspot.0, yHotspot: loupe.hotspot.1, hbmMask: mask, hbmColor: color.bitmap };
        let cursor = CreateIconIndirect(&info);
        SelectObject(color.dc, color.bitmap);
        DeleteObject(mask);
        (!cursor.is_null()).then_some(cursor)
    }
}

/// The path being dragged, drawn in a click-through window that grows with it.
struct Line {
    window: HWND,
    /// The desktop pixels the window covers.
    area: Area,
    bitmap: Option<Bitmap>,
    /// Distance of each pixel of the window to the path (see [`Stroke`]).
    distance: Vec<u8>,
    stroke: Stroke,
    shown: bool,
}

/// Room added around the path when the window grows.
const LINE_MARGIN: i32 = 256;

impl Line {
    fn new(scale: f32, excluded: bool) -> Line {
        // SAFETY: a window of the registered class, destroyed with the line.
        let window = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if !window.is_null() {
            unsafe { prepare(window, excluded) };
        }
        Line { window, area: (0, 0, 0, 0), bitmap: None, distance: Vec::new(), stroke: Stroke::new(scale), shown: false }
    }

    /// Whether the path drawn so far covers a pixel.
    fn covers(&self, p: Pixel) -> bool {
        self.stroke.covered(self.area, &self.distance, p)
    }

    /// Grow the window to include an area; false if it cannot.
    unsafe fn include(&mut self, need: Area) -> bool {
        let (l, t, r, b) = self.area;
        if self.bitmap.is_some() && need.0 >= l && need.1 >= t && need.2 <= r && need.3 <= b {
            return true;
        }
        let grown = if self.bitmap.is_some() { (need.0.min(l), need.1.min(t), need.2.max(r), need.3.max(b)) } else { need };
        let area = (grown.0 - LINE_MARGIN, grown.1 - LINE_MARGIN, grown.2 + LINE_MARGIN, grown.3 + LINE_MARGIN);
        let (w, h) = (area.2 - area.0, area.3 - area.1);
        let Some(mut bitmap) = (unsafe { Bitmap::new(w, h) }) else { return false };
        let mut distance = stroke::empty_distances(area);
        if let Some(old) = &self.bitmap {
            let (ow, oh) = ((r - l) as usize, (b - t) as usize);
            let (dx, dy) = ((l - area.0) as usize, (t - area.1) as usize);
            let pixels = bitmap.pixels_mut();
            for y in 0..oh {
                let src = y * ow;
                let dst = (y + dy) * w as usize + dx;
                pixels[dst * 4..(dst + ow) * 4].copy_from_slice(&old.pixels()[src * 4..(src + ow) * 4]);
                distance[dst..dst + ow].copy_from_slice(&self.distance[src..src + ow]);
            }
        }
        self.bitmap = Some(bitmap);
        self.distance = distance;
        self.area = area;
        true
    }

    /// Draw a segment of the path and show it.
    unsafe fn draw(&mut self, a: Pixel, b: Pixel) {
        if self.window.is_null() {
            return;
        }
        let before = self.area;
        if !unsafe { self.include(self.stroke.bounds(a, b)) } {
            return;
        }
        let area = self.area;
        let Some(bitmap) = self.bitmap.as_mut() else { return };
        let changed = self.stroke.draw(a, b, area, &mut self.distance, bitmap.pixels_mut());
        let origin = POINT { x: area.0, y: area.1 };
        let size = SIZE { cx: area.2 - area.0, cy: area.3 - area.1 };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let dirty = RECT { left: changed.0 - area.0, top: changed.1 - area.1, right: changed.2 - area.0, bottom: changed.3 - area.1 };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: &origin,
            psize: &size,
            hdcSrc: bitmap.dc,
            pptSrc: &source,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            // The whole window the first time and after it moved or grew.
            prcDirty: if before != area || !self.shown { null() } else { &dirty },
        };
        unsafe {
            GdiFlush();
            UpdateLayeredWindowIndirect(self.window, &info);
            if !self.shown {
                self.shown = true;
                ShowWindow(self.window, SW_SHOWNOACTIVATE);
            }
        }
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        if !self.window.is_null() {
            // SAFETY: the line's own window.
            unsafe { DestroyWindow(self.window) };
        }
    }
}

/// These run the real eyedropper over a window of the test's own (its pixels are all
/// that is read from the screen); the mouse is simulated by messages to the eyedropper's
/// windows, the real one is left alone. They need a desktop:
/// `cargo test -p brewed-texture-designer -- --ignored --test-threads 1 eyedropper::windows`.
#[cfg(test)]
pub(super) mod tests {
    use std::sync::Mutex;

    use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, StretchDIBits};
    use windows_sys::Win32::UI::WindowsAndMessaging::{PM_REMOVE, PeekMessageW, SendMessageW, WM_PAINT};

    use super::*;
    use crate::eyedropper::{self, Host};

    /// Where the simulated mouse is (desktop pixels).
    pub static CURSOR: Mutex<Option<(i32, i32)>> = Mutex::new(None);
    /// The eyedropper's windows, as they are created.
    pub static OVERLAYS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    /// Pick as on Windows versions that cannot leave windows out of captures.
    pub static NO_EXCLUSION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    /// The tests share the mouse and the screen.
    static SERIAL: Mutex<()> = Mutex::new(());

    const REFERENCE: RECT = RECT { left: 160, top: 160, right: 160 + 301, bottom: 160 + 80 };

    /// The reference image: black → red → white across.
    fn ramp(x: i32) -> [u8; 3] {
        let t = x as f32 / (REFERENCE.right - REFERENCE.left - 1) as f32;
        let (r, g) = if t < 0.5 { (t * 2.0, 0.0) } else { (1.0, (t - 0.5) * 2.0) };
        [(r * 255.0).round() as u8, (g * 255.0).round() as u8, (g * 255.0).round() as u8]
    }

    unsafe extern "system" fn reference_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_PAINT {
                let mut ps: PAINTSTRUCT = zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let w = REFERENCE.right - REFERENCE.left;
                let row: Vec<u8> = (0..w)
                    .flat_map(|x| {
                        let [r, g, b] = ramp(x);
                        [b, g, r, 0]
                    })
                    .collect();
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: 1,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..zeroed()
                    },
                    bmiColors: zeroed(),
                };
                let h = REFERENCE.bottom - REFERENCE.top;
                StretchDIBits(dc, 0, 0, w, h, 0, 0, w, 1, row.as_ptr() as *const c_void, &info, DIB_RGB_COLORS, SRCCOPY);
                EndPaint(hwnd, &ps);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }

    fn pump(duration: Duration) {
        let end = Instant::now() + duration;
        while Instant::now() < end {
            // SAFETY: the test thread's own messages.
            unsafe {
                let mut msg: MSG = zeroed();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A window of the test's own showing the reference image, on top of everything.
    struct Reference(HWND);

    impl Reference {
        fn show() -> Reference {
            // SAFETY: a window of this thread, destroyed on drop.
            unsafe {
                SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                let class = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(reference_proc),
                    hInstance: GetModuleHandleW(null()),
                    lpszClassName: windows_sys::w!("BrewedEyedropperTest"),
                    ..zeroed()
                };
                RegisterClassExW(&class);
                let r = REFERENCE;
                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    windows_sys::w!("BrewedEyedropperTest"),
                    null(),
                    WS_POPUP,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    null_mut(),
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                );
                assert!(!hwnd.is_null());
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                pump(Duration::from_millis(400));
                Reference(hwnd)
            }
        }
    }

    impl Drop for Reference {
        fn drop(&mut self) {
            // SAFETY: the test's own window.
            unsafe { DestroyWindow(self.0) };
        }
    }

    /// Start a pick and wait for its windows.
    fn start(mode: Mode) -> impl Future<Output = Outcome> {
        OVERLAYS.lock().unwrap().clear();
        *CURSOR.lock().unwrap() = Some((REFERENCE.left + 5, REFERENCE.top + 5));
        let pick = eyedropper::pick(mode, Host::default());
        let started = Instant::now();
        while OVERLAYS.lock().unwrap().is_empty() && started.elapsed() < Duration::from_secs(5) {
            pump(Duration::from_millis(10));
        }
        pump(Duration::from_millis(100));
        pick
    }

    /// Simulate the mouse at a point: a message to an eyedropper window, handled before
    /// this returns.
    fn mouse(msg: u32, x: i32, y: i32) {
        *CURSOR.lock().unwrap() = Some((x, y));
        let window = *OVERLAYS.lock().unwrap().first().expect("eyedropper windows") as HWND;
        // MK_LBUTTON while the button is down.
        let buttons = if msg == WM_LBUTTONUP { 0 } else { 1 };
        // SAFETY: a window of the eyedropper's thread, which is running its message loop.
        unsafe { SendMessageW(window, msg, buttons, 0) };
    }

    /// Save what the screen shows around the reference image (only what this test put
    /// there), when snapshots are asked for.
    fn snapshot(name: &str) {
        if std::env::var_os("BREWED_UI_SNAPSHOTS").is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let r = REFERENCE;
        // SAFETY: reading the screen into a bitmap of this test.
        unsafe {
            let Some(mut screen) = Screen::new() else { return };
            let Some(read) = screen.read(r) else { return };
            let (w, h) = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
            let rgba: Vec<u8> = (r.top..r.bottom)
                .flat_map(|y| (r.left..r.right).map(move |x| (x, y)))
                .flat_map(|(x, y)| {
                    let [red, green, blue] = read.pixel(x, y);
                    [red, green, blue, 255]
                })
                .collect();
            crate::eyedropper::save_snapshot(name, w, h, &rgba, false);
        }
    }

    fn finish(pick: impl Future<Output = Outcome>) -> Outcome {
        let outcome = futures::executor::block_on(pick);
        *CURSOR.lock().unwrap() = None;
        outcome
    }

    fn rgb(c: [f32; 4]) -> [i32; 3] {
        [0, 1, 2].map(|i| (c[i] * 255.0).round() as i32)
    }

    /// The loupe as Windows draws it: over white and over black, so that the ring and the
    /// edges of the circle show whether the transparency is right.
    #[test]
    fn the_loupe_cursor_is_drawn_as_it_is_made() {
        use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, FillRect};
        use windows_sys::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};

        let mut pixels = vec![Some([40u8, 120, 200]); loupe::PIXELS * loupe::PIXELS];
        pixels[loupe::PIXELS * loupe::PIXELS / 2] = Some([250, 60, 40]);
        let image = loupe::render(&pixels, 1.0);
        let side = image.size as i32;
        // SAFETY: a bitmap and a cursor of this test, drawn into and destroyed here.
        unsafe {
            let cursor = make_cursor(&image).expect("cursor");
            for (name, background) in [("eyedropper_cursor_on_white", 0x00ffffff), ("eyedropper_cursor_on_black", 0)] {
                let bitmap = Bitmap::new(side, side).expect("bitmap");
                let brush = CreateSolidBrush(background);
                let rect = RECT { left: 0, top: 0, right: side, bottom: side };
                FillRect(bitmap.dc, &rect, brush);
                DeleteObject(brush);
                DrawIconEx(bitmap.dc, 0, 0, cursor, side, side, 0, std::ptr::null_mut(), DI_NORMAL);
                GdiFlush();
                let rgba: Vec<u8> = bitmap.pixels().chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
                // The picked pixel is in the middle, whatever is behind the cursor.
                let (hx, hy) = (image.hotspot.0 as usize, image.hotspot.1 as usize);
                let at = (hy * side as usize + hx) * 4;
                assert_eq!(&rgba[at..at + 3], &[250, 60, 40], "{name}: the middle pixel");
                // And a neighbor is the color around it.
                let side_by = ((hy - 20) * side as usize + hx) * 4;
                assert_eq!(&rgba[side_by..side_by + 3], &[40, 120, 200], "{name}: a magnified pixel");
                crate::eyedropper::save_snapshot(name, side as u32, side as u32, &rgba, false);
            }
            DestroyIcon(cursor);
        }
    }

    #[test]
    #[ignore = "needs a desktop"]
    fn a_gradient_is_picked_along_a_freehand_path() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        println!("capture exclusion: {}", capture_exclusion_available());
        gradient_pick();
        // Where the eyedropper's windows show in what is read from the screen, the path
        // must still not be picked up.
        NO_EXCLUSION.store(true, std::sync::atomic::Ordering::SeqCst);
        let result = std::panic::catch_unwind(gradient_pick);
        NO_EXCLUSION.store(false, std::sync::atomic::Ordering::SeqCst);
        result.unwrap();
    }

    fn gradient_pick() {
        let _reference = Reference::show();
        let pick = start(Mode::Gradient);
        let (x0, y0) = (REFERENCE.left, REFERENCE.top + 40);
        mouse(WM_LBUTTONDOWN, x0, y0);
        // Across the image in a wave, a few pixels per step: the path is drawn meanwhile
        // and must not be picked up.
        let mut x = x0;
        let mut y = y0;
        while x < REFERENCE.right - 1 {
            x = (x + 3).min(REFERENCE.right - 1);
            y = y0 + (((x - x0) as f32 / 20.0).sin() * 25.0) as i32;
            mouse(WM_MOUSEMOVE, x, y);
        }
        // And back a little, over the path already drawn.
        for back in 1..=10 {
            mouse(WM_MOUSEMOVE, x - back * 3, y);
        }
        snapshot("eyedropper_windows_path");
        mouse(WM_LBUTTONUP, x - 30, y);
        let Outcome::Path(colors) = finish(pick) else { panic!("no path") };
        assert!(colors.len() >= 330, "{} colors", colors.len());
        // Within a level: the invisible windows may darken what is read by 1/255.
        let near = |a: [i32; 3], b: [u8; 3]| a.iter().zip(b).all(|(a, b)| (a - b as i32).abs() <= 1);
        assert!(near(rgb(colors[0]), ramp(0)), "{:?}", rgb(colors[0]));
        assert!(near(rgb(colors[colors.len() - 31]), ramp(300)));
        // Every color is the image's at the column where it was taken: nothing of the
        // drawn path is picked up.
        for c in &colors {
            let c = rgb(*c);
            let close = (0..301).any(|x| near(c, ramp(x)));
            assert!(close, "{c:?} is not in the image");
        }
        // The gradient follows the colors along the path with few stops.
        let forth = &colors[..colors.len() - 30];
        let g = eyedropper::gradient_from_path(forth);
        assert!(g.stops.len() <= 12, "{} stops", g.stops.len());
        for (i, c) in forth.iter().enumerate() {
            let s = g.sample(i as f32 / (forth.len() - 1) as f32);
            assert!((0..3).all(|k| (s[k] - c[k]).abs() < 0.04), "at {i}: {s:?} for {c:?}");
        }
    }

    #[test]
    #[ignore = "needs a desktop"]
    fn a_color_is_picked_with_a_click() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _reference = Reference::show();
        let pick = start(Mode::Color);
        // The loupe follows the mouse meanwhile.
        for x in [10, 60, 110, 75] {
            mouse(WM_MOUSEMOVE, REFERENCE.left + x, REFERENCE.top + 30);
            pump(Duration::from_millis(20));
        }
        mouse(WM_LBUTTONDOWN, REFERENCE.left + 75, REFERENCE.top + 30);
        mouse(WM_LBUTTONUP, REFERENCE.left + 75, REFERENCE.top + 30);
        assert_eq!(finish(pick), Outcome::Color(color(ramp(75))));
    }

    #[test]
    #[ignore = "needs a desktop"]
    fn a_pick_can_be_cancelled() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let pick = start(Mode::Gradient);
        mouse(WM_LBUTTONDOWN, REFERENCE.left, REFERENCE.top);
        mouse(WM_MOUSEMOVE, REFERENCE.left + 50, REFERENCE.top + 5);
        let started = Instant::now();
        eyedropper::cancel();
        assert_eq!(finish(pick), Outcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(1));
        // A click without a drag picks nothing: the pick goes on until the right click.
        let pick = start(Mode::Gradient);
        mouse(WM_LBUTTONDOWN, REFERENCE.left, REFERENCE.top);
        mouse(WM_LBUTTONUP, REFERENCE.left, REFERENCE.top);
        mouse(WM_RBUTTONUP, REFERENCE.left, REFERENCE.top);
        assert_eq!(finish(pick), Outcome::Cancelled);
    }
}
