//! The eyedropper: picks a color, or a gradient along a path dragged with the mouse,
//! from anything on the screen — another monitor, a browser, Tessera itself.
//!
//! The pick is live wherever the platform allows it, like the system color pickers:
//! nothing is captured in advance and nothing on the screen stops.
//!
//! - Windows and X11: invisible windows (or a pointer grab) catch the mouse, the pixels
//!   are read from the screen as it moves, the loupe is the mouse cursor, and the path is
//!   drawn by a window that lets the clicks through.
//! - Wayland: colors are picked with the desktop's own color picker (the XDG desktop
//!   portal). Gradients are picked on transparent surfaces covering the screen; the
//!   pixels are read as the mouse moves, either directly (wlr-screencopy) or from the
//!   live stream the desktop gives through its screen cast portal (GNOME, KDE).
//! - macOS: the system color sampler picks colors; gradients are picked on a capture of
//!   the screen ([`crate::picker`]).

pub(crate) mod loupe;
mod path;
#[cfg(any(windows, free_unix))]
mod stroke;

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(free_unix)]
mod portal;
#[cfg(free_unix)]
mod wayland;
#[cfg(windows)]
mod windows;
#[cfg(free_unix)]
mod x11;

#[cfg(not(target_os = "macos"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

pub use path::{Trace, gradient_from_path};

/// What to pick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One color, with a click.
    Color,
    /// The colors along a path dragged with the mouse.
    Gradient,
}

/// How a pick ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// A color, as shown on the screen (sRGB, 0..1, opaque).
    Color([f32; 4]),
    /// The colors along the dragged path, in order (at least two).
    Path(Vec<[f32; 4]>),
    Cancelled,
    /// The pick could not be done; why, for the status bar.
    Failed(String),
    /// Not possible live here: pick on a capture of the screen ([`crate::picker`]).
    Unsupported,
}

/// What the platform needs to know about the app to pick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Host {
    /// Tessera's Wayland connection (`wl_display`), when it runs on Wayland: the pick's
    /// surfaces belong to the app, so the compositor lets them cover the screen.
    #[cfg(free_unix)]
    pub wayland_display: Option<wayland::Display>,
}

impl Host {
    /// What the eyedropper needs to know about the app, from its main window.
    pub fn of(window: &dyn iced::window::Window) -> Host {
        #[cfg(free_unix)]
        {
            use iced::window::raw_window_handle::RawDisplayHandle;
            if let Ok(handle) = window.display_handle()
                && let RawDisplayHandle::Wayland(wayland) = handle.as_raw()
            {
                return Host { wayland_display: Some(wayland::Display::new(wayland.display)) };
            }
        }
        let _ = window;
        Host::default()
    }
}

/// Prepare a window that shows a capture of its monitor ([`crate::picker`]) to look
/// like the screen itself.
pub fn prepare_overlay(window: &dyn iced::window::Window) {
    #[cfg(target_os = "macos")]
    macos::prepare_overlay(window);
    let _ = window;
}

/// Start picking. The future ends with the pick; [`cancel`] ends it early.
pub fn pick(mode: Mode, host: Host) -> impl Future<Output = Outcome> + Send + 'static {
    let cancel = Cancel::default();
    *lock(&CURRENT) = Some(cancel.clone());
    let (tx, rx) = futures::channel::oneshot::channel();
    let finish = {
        let cancel = cancel.clone();
        move |outcome: Outcome| {
            {
                let mut current = lock(&CURRENT);
                if current.as_ref().is_some_and(|c| Arc::ptr_eq(&c.0, &cancel.0)) {
                    *current = None;
                }
            }
            let _ = tx.send(outcome);
        }
    };
    start(mode, host, cancel, finish);
    async move { rx.await.unwrap_or_else(|_| Outcome::Failed("The eyedropper stopped unexpectedly".into())) }
}

/// Stop the pick in progress, if any.
pub fn cancel() {
    if let Some(cancel) = lock(&CURRENT).take() {
        cancel.cancel();
    }
}

/// The pick in progress.
static CURRENT: Mutex<Option<Cancel>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs a platform's pick on a thread of its own (each has its own event loop) and
/// reports how it ended, even if it panics.
#[cfg(not(target_os = "macos"))]
fn start(mode: Mode, host: Host, cancel: Cancel, finish: impl FnOnce(Outcome) + Send + 'static) {
    let finish = Arc::new(Mutex::new(Some(finish)));
    let report = {
        let finish = finish.clone();
        move |outcome| {
            if let Some(finish) = lock(&finish).take() {
                finish(outcome);
            }
        }
    };
    let run = {
        let report = report.clone();
        move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| platform_pick(mode, host, &cancel)))
                .unwrap_or_else(|_| Outcome::Failed("The eyedropper stopped unexpectedly".into()));
            report(outcome);
        }
    };
    if let Err(e) = std::thread::Builder::new().name("eyedropper".into()).spawn(run) {
        report(Outcome::Failed(format!("Cannot start the eyedropper: {e}")));
    }
}

#[cfg(target_os = "macos")]
fn start(mode: Mode, _host: Host, cancel: Cancel, finish: impl FnOnce(Outcome) + Send + 'static) {
    macos::pick(mode, cancel, Box::new(finish));
}

#[cfg(not(target_os = "macos"))]
fn platform_pick(mode: Mode, host: Host, cancel: &Cancel) -> Outcome {
    #[cfg(windows)]
    {
        let _ = host;
        windows::pick(mode, cancel)
    }
    #[cfg(free_unix)]
    {
        // A Wayland session: X11 would only see the programs running under Xwayland.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("WAYLAND_SOCKET").is_some() {
            return wayland::pick(mode, host.wayland_display, cancel);
        }
        if std::env::var_os("DISPLAY").is_some() {
            return x11::pick(mode, cancel);
        }
        Outcome::Failed("Neither Wayland nor X11 is available to pick colors from the screen".into())
    }
    #[cfg(not(any(windows, free_unix)))]
    {
        let _ = (mode, host, cancel);
        Outcome::Unsupported
    }
}

/// Lets the app stop a pick: the platform's event loop is woken up to see it.
#[derive(Clone, Default)]
pub(crate) struct Cancel(Arc<CancelState>);

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    wake: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Cancel {
    #[cfg_attr(target_os = "macos", allow(dead_code, reason = "the system sampler cannot be stopped"))]
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Call `wake` when the pick is cancelled (right away if it is already), to interrupt
    /// the event loop running it.
    #[cfg_attr(target_os = "macos", allow(dead_code, reason = "the system sampler cannot be stopped"))]
    pub fn on_cancel(&self, wake: impl Fn() + Send + 'static) {
        let mut slot = lock(&self.0.wake);
        if self.is_cancelled() {
            wake();
        } else {
            *slot = Some(Box::new(wake));
        }
    }

    fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        if let Some(wake) = lock(&self.0.wake).take() {
            wake();
        }
    }
}

/// A color read from the screen (8 bits per channel) as a color parameter value.
pub(crate) fn color(rgb: [u8; 3]) -> [f32; 4] {
    [rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, 1.0]
}

/// Save an image the tests draw, when snapshots are asked for (`TESSERA_UI_SNAPSHOTS`).
#[cfg(test)]
pub(crate) fn save_snapshot(name: &str, width: u32, height: u32, rgba: &[u8], premultiplied: bool) {
    let Some(dir) = std::env::var_os("TESSERA_UI_SNAPSHOTS") else { return };
    let mut rgba = rgba.to_vec();
    if premultiplied {
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = p[3] as f32 / 255.0;
            if a > 0.0 {
                for c in &mut p[..3] {
                    *c = (*c as f32 / a).round().min(255.0) as u8;
                }
            }
        }
    }
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    image::RgbaImage::from_raw(width, height, rgba).expect("snapshot size").save(path).expect("save snapshot");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_wakes_the_pick_once() {
        let cancel = Cancel::default();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        cancel.on_cancel(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        assert!(!cancel.is_cancelled());
        cancel.cancel();
        cancel.cancel();
        assert!(cancel.is_cancelled());
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        // Registered after the cancel: woken right away.
        let w = woken.clone();
        cancel.on_cancel(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(woken.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn screen_colors_become_parameter_values() {
        assert_eq!(color([255, 0, 51]), [1.0, 0.0, 0.2, 1.0]);
    }
}

