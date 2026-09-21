//! Picking on macOS.
//!
//! One color is picked with the system's color sampler: the round loupe of the Color
//! panel, live, over everything, with Esc to cancel. Gradients are picked on a capture of
//! the screen ([`crate::picker`]), in windows prepared here to look exactly like it.

use std::cell::Cell;
use std::ffi::{CStr, c_void};

use block2::RcBlock;
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace, NSView, NSWindowAnimationBehavior};

use super::{Cancel, Mode, Outcome};

/// `kCGScreenSaverWindowLevel`: above the menu bar and the Dock.
const ABOVE_EVERYTHING: isize = 1000;

/// Start picking (on the main thread, where the app's update runs).
pub fn pick(mode: Mode, cancel: Cancel, finish: Box<dyn FnOnce(Outcome) + Send>) {
    // The sampler has no way to be stopped: it ends with a click or Esc.
    let _ = cancel;
    let sampler_available = AnyClass::get(c"NSColorSampler").is_some();
    if mode == Mode::Gradient || !sampler_available || objc2::MainThreadMarker::new().is_none() {
        // Gradients, and macOS before 10.15: pick on a capture.
        finish(Outcome::Unsupported);
        return;
    }
    let finish = Cell::new(Some(finish));
    let handler = RcBlock::new(move |color: *mut NSColor| {
        // SAFETY: the sampler passes a valid color, or nil when cancelled.
        let outcome = match unsafe { color.as_ref() } {
            Some(color) => srgb(color).map_or(Outcome::Failed("The picked color cannot be converted to sRGB".into()), Outcome::Color),
            None => Outcome::Cancelled,
        };
        if let Some(finish) = finish.take() {
            finish(outcome);
        }
    });
    let sampler = NSColorSampler::new();
    // SAFETY: called on the main thread; the sampler keeps itself alive until the
    // session ends and calls the handler once, on the main thread.
    unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
}

/// A color as its sRGB components (what the screen showed, as a color parameter value).
fn srgb(color: &NSColor) -> Option<[f32; 4]> {
    let color = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let c = [color.redComponent(), color.greenComponent(), color.blueComponent()].map(|v| (v as f32).clamp(0.0, 1.0));
    c.iter().all(|v| v.is_finite()).then_some([c[0], c[1], c[2], 1.0])
}

/// Make a window showing a capture of its monitor look like the screen itself: above the
/// menu bar and the Dock, without a shadow or an animation.
pub fn prepare_overlay(window: &dyn iced::window::Window) {
    use iced::window::raw_window_handle::RawWindowHandle;
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return };
    // SAFETY: the handle is the live NSView of one of the app's windows, used on the main
    // thread (where window tasks run).
    let view: &NSView = unsafe { appkit.ns_view.cast().as_ref() };
    if let Some(window) = view.window() {
        window.setLevel(ABOVE_EVERYTHING);
        window.setHasShadow(false);
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    }
}

/// A function of the system frameworks the app links, by name: they belong to versions
/// of macOS the app also runs without.
pub fn symbol<F: Copy>(name: &CStr) -> Option<F> {
    assert_eq!(size_of::<F>(), size_of::<*mut c_void>(), "a function pointer");
    // SAFETY: looking up a function of the system frameworks the app links.
    let f = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    // SAFETY: the caller names the function and gives its signature; a pointer to it is
    // the size of the pointer dlsym returns.
    (!f.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&f) })
}

/// Screen captures need the user's permission (macOS 10.15 and later); without it they
/// only show the desktop picture.
pub fn screen_capture_allowed() -> Result<(), String> {
    type Check = unsafe extern "C" fn() -> bool;
    let (Some(preflight), Some(request)): (Option<Check>, Option<Check>) =
        (symbol(c"CGPreflightScreenCaptureAccess"), symbol(c"CGRequestScreenCaptureAccess"))
    else {
        // Older systems ask for no permission.
        return Ok(());
    };
    // SAFETY: plain CoreGraphics calls.
    if unsafe { preflight() } {
        return Ok(());
    }
    // Shows the system's request (once) and adds the app to the list in the settings.
    unsafe { request() };
    Err("Allow Brewed Texture Designer to record the screen (System Settings → Privacy & Security → Screen Recording), then pick again".into())
}
