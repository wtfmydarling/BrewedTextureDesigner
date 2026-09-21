//! The application icon.
//!
//! The artwork is drawn in code ([`art`]). On Windows, build.rs embeds it in the
//! executable in every size the shell asks for; the windows then load it from there so
//! the taskbar shows a sharp icon at any display scale.

pub mod art;

/// The icon as an image for the interface (drawn at 32 px, shown at 16 px: sharp on
/// high-DPI screens).
pub fn logo() -> iced::widget::image::Handle {
    static LOGO: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| iced::widget::image::Handle::from_rgba(32, 32, art::icon_rgba(32))).clone()
}

/// The icon for the About dialog (shown at 48 px).
pub fn logo_large() -> iced::widget::image::Handle {
    static LOGO: std::sync::OnceLock<iced::widget::image::Handle> = std::sync::OnceLock::new();
    LOGO.get_or_init(|| iced::widget::image::Handle::from_rgba(128, 128, art::icon_rgba(128))).clone()
}

/// The icon given to iced windows (title bar, and the taskbar where there is no
/// executable icon).
pub fn window_icon() -> Option<iced::window::Icon> {
    const SIZE: u32 = 64;
    iced::window::icon::from_rgba(art::icon_rgba(SIZE), SIZE, SIZE).ok()
}

/// Use the icons embedded in the executable for a window (both the big one of the
/// taskbar and the small one of the title bar), sized for the window's display scale.
#[cfg(windows)]
pub fn use_embedded_icon(window: &dyn iced::window::Window) {
    use iced::window::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
        SendMessageW, WM_SETICON,
    };

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else { return };
    let hwnd = win32.hwnd.get() as HWND;
    // SAFETY: plain Win32 calls on a live window of this thread; the resource id is the
    // icon group build.rs embeds (a missing resource just yields a null handle).
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let dpi = GetDpiForWindow(hwnd).max(96);
        for (kind, metric_x, metric_y) in [(ICON_BIG, SM_CXICON, SM_CYICON), (ICON_SMALL, SM_CXSMICON, SM_CYSMICON)] {
            let (cx, cy) = (GetSystemMetricsForDpi(metric_x, dpi), GetSystemMetricsForDpi(metric_y, dpi));
            let icon = LoadImageW(module, ICON_RESOURCE as usize as *const u16, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR);
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as WPARAM, icon as LPARAM);
            }
        }
    }
}

/// Resource id of the icon group in the executable (see build.rs).
#[cfg(windows)]
const ICON_RESOURCE: u16 = 1;

#[cfg(test)]
mod tests {
    use super::art;

    #[test]
    fn icon_art_is_opaque_inside_and_transparent_in_the_corners() {
        for size in art::ICON_SIZES {
            let px = art::icon_rgba(size);
            assert_eq!(px.len(), (size * size * 4) as usize);
            let at = |x: u32, y: u32| &px[((y * size + x) * 4) as usize..((y * size + x) * 4 + 4) as usize];
            assert_eq!(at(0, 0)[3], 0, "{size}: corner");
            assert_eq!(at(size / 2, size / 2)[3], 255, "{size}: center");
            // White foam on top of the mug, amber beer in it.
            let pick = |x: f32, y: f32| at((size as f32 * x) as u32, (size as f32 * y) as u32).to_vec();
            let foam = pick(0.36, 0.33);
            assert!(foam[0] > 200 && foam[2] > 180, "{size}: foam {foam:?}");
            let beer = (0..size).flat_map(|x| (0..size).map(move |y| (x, y))).map(|(x, y)| at(x, y).to_vec());
            assert!(beer.filter(|p| p[0] > 200 && p[2] < 120 && p[3] == 255).count() > (size * size / 20) as usize, "{size}: no beer");
            if let Some(dir) = std::env::var_os("TESSERA_UI_SNAPSHOTS") {
                let path = std::path::Path::new(&dir).join(format!("icon_{size}.png"));
                image::RgbaImage::from_raw(size, size, px).unwrap().save(path).unwrap();
            }
        }
    }
}
