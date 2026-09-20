//! Picking on Wayland.
//!
//! Wayland programs may neither read the screen nor take the mouse from the others, so:
//!
//! - One color is picked with the desktop's own color picker, through the XDG desktop
//!   portal (GNOME, KDE, Hyprland...): live, over everything.
//! - Otherwise (gradients, desktops without that picker), transparent surfaces cover
//!   every output and catch the mouse (layer shell; fullscreen windows where there is
//!   none), and the pixels are read as it moves with wlr-screencopy (Sway, Hyprland,
//!   river, niri...), a small region at a time: nothing stops. The loupe is the cursor;
//!   the path is drawn on the surfaces, and what it is about to cover is read first.

mod screencopy;

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState, SimpleGlobal};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{Window, WindowConfigure, WindowDecorations, WindowHandler};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat, delegate_shm, delegate_simple,
    delegate_xdg_shell, delegate_xdg_window, registry_handlers,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_output::{Transform, WlOutput};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::Format;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;

use self::screencopy::Capture;
use super::loupe;
use super::path::{Pixel, Trace};
use super::stroke::{self, Area, Stroke};
use super::{Cancel, Mode, Outcome, color, portal};

/// The app's Wayland connection (`wl_display`).
#[derive(Clone, Copy, Debug)]
pub struct Display(usize);

impl Display {
    pub fn new(display: NonNull<c_void>) -> Display {
        Display(display.as_ptr() as usize)
    }
}

const BUTTON_LEFT: u32 = 0x110;
const BUTTON_RIGHT: u32 = 0x111;
const KEY_ESCAPE: u32 = 1;
/// How long the loop waits for events before looking at the time.
const TICK: Duration = Duration::from_millis(16);
/// The loupe follows what is on the screen at least this often while the pointer rests.
const REFRESH: Duration = Duration::from_millis(50);

pub fn pick(mode: Mode, display: Option<Display>, cancel: &Cancel) -> Outcome {
    if mode == Mode::Color {
        match portal::pick_color(cancel) {
            Ok(Some(c)) => return Outcome::Color(c),
            Ok(None) => return Outcome::Cancelled,
            Err(portal::Error::Unavailable(why)) => log::info!("no color picker from the desktop ({why}): picking with overlays"),
            Err(portal::Error::Failed(e)) => return Outcome::Failed(format!("The desktop's color picker failed: {e}")),
        }
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    match run(mode, display, cancel) {
        Ok(outcome) => outcome,
        Err(e) => Outcome::Failed(e),
    }
}

fn run(mode: Mode, display: Option<Display>, cancel: &Cancel) -> Result<Outcome, String> {
    let conn = match display {
        // SAFETY: the app's display lives as long as the app; the pick uses a queue of
        // its own on it and never disconnects it.
        Some(Display(ptr)) => Connection::from_backend(unsafe { wayland_backend::sys::client::Backend::from_foreign_display(ptr as *mut _) }),
        None => Connection::connect_to_env().map_err(|e| format!("Cannot connect to the Wayland display: {e}"))?,
    };
    let (globals, mut queue): (_, EventQueue<State>) = registry_queue_init(&conn).map_err(|e| format!("Wayland: {e}"))?;
    let qh = queue.handle();
    let bind_error = |what: &str| format!("The desktop has no {what}");
    let compositor = CompositorState::bind(&globals, &qh).map_err(|_| bind_error("compositor"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|_| bind_error("shared memory"))?;
    let pool = SlotPool::new(64 * 1024, &shm).map_err(|e| format!("Cannot allocate memory to draw: {e}"))?;
    let screencopy: Option<ZwlrScreencopyManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
    let Some(screencopy) = screencopy else {
        return Ok(Outcome::Failed(
            "This desktop does not let programs read the screen: pick the colors one by one with the color's eyedropper".into(),
        ));
    };
    let cursor_surface = compositor.create_surface(&qh);
    let mut state = State {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        layer_shell: LayerShell::bind(&globals, &qh).ok(),
        xdg_shell: XdgShell::bind(&globals, &qh).ok(),
        viewporter: SimpleGlobal::<WpViewporter, 1>::bind(&globals, &qh).ok().and_then(|g| g.get().ok().cloned()),
        compositor,
        shm,
        pool,
        screencopy,
        qh: qh.clone(),
        mode,
        overlays: Vec::new(),
        pointers: Vec::new(),
        keyboards: Vec::new(),
        cursor: Cursor { surface: cursor_surface, buffer: None, hotspot: (0, 0), scale: 0, loupe: None },
        enter_serial: None,
        pointer_at: None,
        pressed: false,
        right_pressed: false,
        trace: Trace::default(),
        colors: Vec::new(),
        hidden: HashMap::new(),
        pending: Vec::new(),
        batch: None,
        finishing: false,
        captures: HashMap::new(),
        next_capture: 0,
        loupe_read: None,
        refreshed: None,
        outcome: None,
    };
    if state.layer_shell.is_none() && state.xdg_shell.is_none() {
        return Err(bind_error("way to show surfaces"));
    }
    // The outputs, with their logical layout (xdg-output) and the seats.
    queue.roundtrip(&mut state).map_err(|e| format!("Wayland: {e}"))?;
    queue.roundtrip(&mut state).map_err(|e| format!("Wayland: {e}"))?;
    state.cover_outputs();
    if state.overlays.is_empty() {
        return Err("No monitor to pick from".into());
    }
    let started = Instant::now();
    let outcome = loop {
        if let Some(outcome) = state.outcome.take() {
            break outcome;
        }
        if cancel.is_cancelled() {
            break Outcome::Cancelled;
        }
        state.tick();
        queue.flush().map_err(|e| format!("Wayland: {e}"))?;
        queue.dispatch_pending(&mut state).map_err(|e| format!("Wayland: {e}"))?;
        // Wait for events (at most a tick), then read them.
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN)];
            let timeout = rustix::event::Timespec { tv_sec: 0, tv_nsec: TICK.as_nanos() as i64 };
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(n) if n > 0 => {
                    guard.read().map_err(|e| format!("Wayland: {e}"))?;
                }
                _ => drop(guard),
            }
        }
        queue.dispatch_pending(&mut state).map_err(|e| format!("Wayland: {e}"))?;
        // Nothing was ever shown: the desktop refused the surfaces.
        if started.elapsed() > Duration::from_secs(5) && state.overlays.iter().all(|o| o.size.is_none()) {
            break Outcome::Failed("The desktop did not show the eyedropper".into());
        }
    };
    state.close();
    let _ = queue.roundtrip(&mut state);
    Ok(outcome)
}

/// An output as the pick sees it.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// Position and size on the desktop, in logical units.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Size in pixels, upright (as the desktop shows it).
    pub pixels: (i32, i32),
    pub transform: Transform,
}

impl Geometry {
    fn of(info: &smithay_client_toolkit::output::OutputInfo) -> Option<Geometry> {
        let mode = info.modes.iter().find(|m| m.current).or(info.modes.first())?;
        let transform = info.transform;
        let (mw, mh) = mode.dimensions;
        let pixels = if transform as u32 % 2 == 1 { (mh, mw) } else { (mw, mh) };
        let scale = info.scale_factor.max(1);
        let (x, y) = info.logical_position.unwrap_or(info.location);
        let (width, height) = info.logical_size.unwrap_or((pixels.0 / scale, pixels.1 / scale));
        (width > 0 && height > 0 && pixels.0 > 0 && pixels.1 > 0).then_some(Geometry {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
            pixels,
            transform,
        })
    }

    pub fn scale(&self) -> (f32, f32) {
        (self.pixels.0 as f32 / self.width, self.pixels.1 as f32 / self.height)
    }

    fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[1] >= self.y && p[0] < self.x + self.width && p[1] < self.y + self.height
    }

    /// A desktop point in this output's pixels.
    fn to_pixels(&self, p: [f32; 2]) -> (f32, f32) {
        let (sx, sy) = self.scale();
        ((p[0] - self.x) * sx, (p[1] - self.y) * sy)
    }

    fn area(&self) -> Area {
        (0, 0, self.pixels.0, self.pixels.1)
    }
}

/// Pixels read from an area of an output.
pub struct Pixels {
    area: Area,
    rgb: Vec<Option<[u8; 3]>>,
}

impl Pixels {
    fn get(&self, (x, y): Pixel) -> Option<[u8; 3]> {
        let (x0, y0, x1, y1) = self.area;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return None;
        }
        self.rgb.get(((y - y0) * (x1 - x0) + (x - x0)) as usize).copied().flatten()
    }
}

enum Shell {
    Layer(LayerSurface),
    Window(Window),
}

/// The surface covering an output.
struct Overlay {
    output: WlOutput,
    geometry: Geometry,
    shell: Shell,
    viewport: Option<WpViewport>,
    /// Its size once the desktop has shown it (logical units).
    size: Option<(u32, u32)>,
    /// A transparent buffer, until the path is drawn.
    clear: Option<Buffer>,
    /// The path drawn on this output.
    canvas: Option<Canvas>,
}

impl Overlay {
    fn surface(&self) -> &WlSurface {
        match &self.shell {
            Shell::Layer(layer) => layer.wl_surface(),
            Shell::Window(window) => window.wl_surface(),
        }
    }
}

/// The path drawn on an output, at its pixels.
struct Canvas {
    stroke: Stroke,
    /// How far each of the output's pixels is from the path.
    distance: Vec<u8>,
    /// Two buffers: the desktop may keep showing one while the other is drawn. Each
    /// knows what it misses of the path (output pixels).
    buffers: [(Buffer, Option<Area>); 2],
    /// The buffers' size: the output's pixels, or coarser without a viewport.
    size: (i32, i32),
    /// Something was drawn that is not shown yet.
    changed: bool,
}

impl Canvas {
    /// Pixels of the path changed: both buffers miss them.
    fn changed(&mut self, area: Area) {
        for (_, stale) in &mut self.buffers {
            *stale = Some(match *stale {
                Some(s) => (s.0.min(area.0), s.1.min(area.1), s.2.max(area.2), s.3.max(area.3)),
                None => area,
            });
        }
        self.changed = true;
    }
}

struct Cursor {
    surface: WlSurface,
    buffer: Option<Buffer>,
    hotspot: (i32, i32),
    /// The buffer scale it was made for.
    scale: i32,
    /// What the loupe shows.
    loupe: Option<Vec<Option<[u8; 3]>>>,
}

/// Pointer positions whose pixels are being read.
struct Batch {
    /// The pixels the path crosses, in order.
    path: Vec<(usize, Pixel)>,
    /// The segments to draw on each output (in its pixels).
    segments: Vec<(usize, Pixel, Pixel)>,
    /// The regions read, by output.
    reads: Vec<(usize, usize)>,
    pixels: HashMap<usize, Pixels>,
}

pub struct State {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    layer_shell: Option<LayerShell>,
    xdg_shell: Option<XdgShell>,
    viewporter: Option<WpViewporter>,
    compositor: CompositorState,
    shm: Shm,
    pool: SlotPool,
    screencopy: ZwlrScreencopyManagerV1,
    qh: QueueHandle<State>,
    mode: Mode,
    overlays: Vec<Overlay>,
    pointers: Vec<WlPointer>,
    keyboards: Vec<WlKeyboard>,
    cursor: Cursor,
    /// The last time the pointer entered a surface (to set its cursor).
    enter_serial: Option<u32>,
    /// Where the pointer is on the desktop.
    pointer_at: Option<[f32; 2]>,
    pressed: bool,
    right_pressed: bool,
    trace: Trace,
    colors: Vec<[f32; 4]>,
    /// Colors under the drawn path, read before it covered them.
    hidden: HashMap<(usize, Pixel), [f32; 4]>,
    /// Pointer positions (while dragging) not read yet.
    pending: Vec<[f32; 2]>,
    batch: Option<Batch>,
    /// The button is up: the path ends once its last pixels are read.
    finishing: bool,
    captures: HashMap<usize, Capture>,
    next_capture: usize,
    /// The region read for the loupe (capture id, pointer position) or the click.
    loupe_read: Option<(usize, [f32; 2], bool)>,
    refreshed: Option<(Instant, [f32; 2])>,
    outcome: Option<Outcome>,
}

impl State {
    /// Cover every output with a transparent surface.
    fn cover_outputs(&mut self) {
        let outputs: Vec<WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            let Some(geometry) = self.outputs.info(&output).as_ref().and_then(Geometry::of) else { continue };
            let surface = self.compositor.create_surface(&self.qh);
            let viewport = self.viewporter.as_ref().map(|v| v.get_viewport(&surface, &self.qh, ()));
            let shell = if let Some(layer_shell) = &self.layer_shell {
                let layer = layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some("tessera-eyedropper"), Some(&output));
                layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                layer.set_exclusive_zone(-1);
                layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                layer.set_size(0, 0);
                layer.commit();
                Shell::Layer(layer)
            } else if let Some(xdg) = &self.xdg_shell {
                let window = xdg.create_window(surface, WindowDecorations::None, &self.qh);
                window.set_title("Tessera eyedropper");
                window.set_app_id("tessera");
                window.set_fullscreen(Some(&output));
                window.commit();
                Shell::Window(window)
            } else {
                continue;
            };
            self.overlays.push(Overlay { output, geometry, shell, viewport, size: None, clear: None, canvas: None });
        }
    }

    fn overlay_of(&self, surface: &WlSurface) -> Option<usize> {
        self.overlays.iter().position(|o| o.surface() == surface)
    }

    /// The output under a desktop point, and the point in its pixels.
    fn locate(&self, p: [f32; 2]) -> Option<(usize, (f32, f32))> {
        let i = self.overlays.iter().position(|o| o.geometry.contains(p))?;
        Some((i, self.overlays[i].geometry.to_pixels(p)))
    }

    /// The desktop was shown the surface: give it its (transparent) content.
    fn configured(&mut self, i: usize, size: (u32, u32)) {
        let overlay = &mut self.overlays[i];
        let (w, h) = if size.0 == 0 || size.1 == 0 {
            (overlay.geometry.width as u32, overlay.geometry.height as u32)
        } else {
            size
        };
        overlay.size = Some((w, h));
        if overlay.canvas.is_some() {
            return;
        }
        // One transparent pixel, stretched: nothing to see, and every click lands on it.
        let (bw, bh) = if overlay.viewport.is_some() { (1, 1) } else { (w as i32, h as i32) };
        let Ok((buffer, canvas)) = self.pool.create_buffer(bw, bh, bw * 4, Format::Argb8888) else { return };
        canvas.fill(0);
        let surface = overlay.surface().clone();
        if let Some(viewport) = &overlay.viewport {
            viewport.set_destination(w as i32, h as i32);
        }
        if buffer.attach_to(&surface).is_ok() {
            surface.damage_buffer(0, 0, bw, bh);
            surface.commit();
        }
        overlay.clear = Some(buffer);
    }

    /// Show the path drawn on an output, when its buffer is free.
    fn present(&mut self, i: usize) {
        let overlay = &mut self.overlays[i];
        let surface = overlay.surface().clone();
        let area = overlay.geometry.area();
        let Some(canvas) = &mut overlay.canvas else { return };
        if !canvas.changed {
            return;
        }
        // The buffer the desktop is not using; it may not have released either yet.
        let Some(slot) = canvas.buffers.iter_mut().find_map(|slot| slot.0.canvas(&mut self.pool).is_some().then_some(slot)) else { return };
        let Some(stale) = slot.1.take() else { return };
        let Some(pixels) = slot.0.canvas(&mut self.pool) else { return };
        let changed = canvas.stroke.render_scaled(area, &canvas.distance, stale, pixels, canvas.size);
        canvas.changed = false;
        if slot.0.attach_to(&surface).is_ok() {
            surface.damage_buffer(changed.0, changed.1, changed.2 - changed.0, changed.3 - changed.1);
            surface.commit();
        }
    }

    /// Make the canvas of an output, the first time the path reaches it.
    fn canvas(&mut self, i: usize) -> bool {
        if self.overlays[i].canvas.is_some() {
            return true;
        }
        let overlay = &self.overlays[i];
        let Some((w, h)) = overlay.size else { return false };
        let (sx, _) = overlay.geometry.scale();
        // Pixel for pixel with a viewport; otherwise at an integer scale, or logical units.
        let (pw, ph, buffer_scale) = if overlay.viewport.is_some() {
            (overlay.geometry.pixels.0, overlay.geometry.pixels.1, 1)
        } else if (sx - sx.round()).abs() < 0.01 && sx >= 1.0 {
            (w as i32 * sx.round() as i32, h as i32 * sx.round() as i32, sx.round() as i32)
        } else {
            (w as i32, h as i32, 1)
        };
        let mut make = || -> Option<Buffer> {
            let (buffer, pixels) = self.pool.create_buffer(pw, ph, pw * 4, Format::Argb8888).ok()?;
            pixels.fill(0);
            Some(buffer)
        };
        let (Some(first), Some(second)) = (make(), make()) else { return false };
        let overlay = &mut self.overlays[i];
        if let Some(viewport) = &overlay.viewport {
            viewport.set_destination(w as i32, h as i32);
        }
        overlay.surface().set_buffer_scale(buffer_scale);
        // Distances are kept at the output's pixels; a coarser buffer shows them scaled.
        let area = overlay.geometry.area();
        overlay.canvas = Some(Canvas {
            stroke: Stroke::new(overlay.geometry.scale().0),
            distance: stroke::empty_distances(area),
            // Both empty, like the path so far.
            buffers: [(first, None), (second, None)],
            size: (pw, ph),
            changed: false,
        });
        true
    }

    /// Ask for the pixels of an area of an output.
    fn read(&mut self, output: usize, area: Area) -> usize {
        let id = self.next_capture;
        self.next_capture += 1;
        let overlay = &self.overlays[output];
        let (x0, y0, x1, y1) = area;
        let (w, h) = overlay.geometry.pixels;
        let area = (x0.clamp(0, w), y0.clamp(0, h), x1.clamp(0, w), y1.clamp(0, h));
        let capture = Capture::start(&self.screencopy, &self.qh, id, output, &overlay.output, overlay.geometry, area);
        self.captures.insert(id, capture);
        id
    }

    /// A read is done (`None`: it failed).
    fn capture_done(&mut self, id: usize, pixels: Option<Pixels>) {
        let Some(capture) = self.captures.remove(&id) else { return };
        let output = capture.output();
        drop(capture);
        if let Some((loupe, at, click)) = self.loupe_read
            && loupe == id
        {
            self.loupe_read = None;
            self.loupe_ready(at, click, output, pixels);
            return;
        }
        let Some(batch) = &mut self.batch else { return };
        if batch.reads.iter().any(|(_, r)| *r == id) {
            match pixels {
                Some(pixels) => {
                    batch.pixels.insert(output, pixels);
                }
                None => {
                    self.outcome = Some(Outcome::Failed("The desktop did not let the screen be read".into()));
                    return;
                }
            }
            if batch.reads.iter().all(|(o, _)| batch.pixels.contains_key(o)) {
                self.batch_ready();
            }
        }
    }

    fn tick(&mut self) {
        for i in 0..self.overlays.len() {
            self.present(i);
        }
        match self.mode {
            Mode::Gradient => self.next_batch(),
            Mode::Color => {
                let stale = self.refreshed.is_none_or(|(at, p)| Some(p) != self.pointer_at || at.elapsed() >= REFRESH);
                if stale && self.loupe_read.is_none() {
                    self.refresh_loupe(false);
                }
            }
        }
    }

    // ---------------------------------------------------------------- one color

    /// Read the pixels around the pointer, for the loupe (or the pixel under it, for a
    /// click).
    fn refresh_loupe(&mut self, click: bool) {
        let Some(at) = self.pointer_at else { return };
        let Some((output, (x, y))) = self.locate(at) else { return };
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        let half = (loupe::PIXELS / 2) as i32;
        let area = if click { (x, y, x + 1, y + 1) } else { (x - half, y - half, x + half + 1, y + half + 1) };
        let id = self.read(output, area);
        self.loupe_read = Some((id, at, click));
        self.refreshed = Some((Instant::now(), at));
    }

    fn loupe_ready(&mut self, at: [f32; 2], click: bool, output: usize, pixels: Option<Pixels>) {
        let Some(pixels) = pixels else {
            if click {
                self.outcome = Some(Outcome::Failed("The desktop did not let the screen be read".into()));
            }
            return;
        };
        let (x, y) = self.overlays[output].geometry.to_pixels(at);
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        if click {
            self.outcome = Some(pixels.get((x, y)).map_or(Outcome::Failed("The picked pixel could not be read".into()), |c| Outcome::Color(color(c))));
            return;
        }
        let half = (loupe::PIXELS / 2) as i32;
        let around: Vec<Option<[u8; 3]>> = (-half..=half).flat_map(|dy| (-half..=half).map(move |dx| (x + dx, y + dy))).map(|p| pixels.get(p)).collect();
        if self.cursor.loupe.as_ref() == Some(&around) {
            return;
        }
        let scale = self.overlays[output].geometry.scale().0;
        let image = loupe::render(&around, scale);
        self.cursor.loupe = Some(around);
        self.set_cursor_image(&image.rgba, image.size, image.hotspot, scale);
    }

    /// Show an image (premultiplied RGBA, `size` × `size`) as the cursor.
    fn set_cursor_image(&mut self, rgba: &[u8], size: u32, hotspot: (u32, u32), scale: f32) {
        // Cursor surfaces take integer scales: the image is padded to a multiple.
        let n = (scale.round() as i32).max(1);
        let side = (size as i32 + n - 1) / n * n;
        let Ok((buffer, canvas)) = self.pool.create_buffer(side, side, side * 4, Format::Argb8888) else { return };
        canvas.fill(0);
        for y in 0..size as usize {
            for x in 0..size as usize {
                let s = (y * size as usize + x) * 4;
                let d = (y * side as usize + x) * 4;
                // ARGB8888 is BGRA in memory.
                canvas[d..d + 4].copy_from_slice(&[rgba[s + 2], rgba[s + 1], rgba[s], rgba[s + 3]]);
            }
        }
        let surface = self.cursor.surface.clone();
        surface.set_buffer_scale(n);
        if buffer.attach_to(&surface).is_ok() {
            surface.damage_buffer(0, 0, side, side);
            surface.commit();
        }
        let hotspot = (hotspot.0 as i32 / n, hotspot.1 as i32 / n);
        if hotspot != self.cursor.hotspot || n != self.cursor.scale {
            self.cursor.hotspot = hotspot;
            self.cursor.scale = n;
            self.apply_cursor(None);
        }
        self.cursor.buffer = Some(buffer);
    }

    /// Make the cursor surface the pointer's (again, after it enters a surface: `serial`).
    fn apply_cursor(&mut self, serial: Option<u32>) {
        if serial.is_some() {
            self.enter_serial = serial;
        }
        let Some(serial) = self.enter_serial else { return };
        for pointer in &self.pointers {
            pointer.set_cursor(serial, Some(&self.cursor.surface), self.cursor.hotspot.0, self.cursor.hotspot.1);
        }
    }

    /// A crosshair cursor, for gradients.
    fn crosshair(&mut self, scale: f32) {
        let n = (scale.round() as u32).max(1);
        let size = 23 * n;
        let c = size / 2;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        let mut put = |x: u32, y: u32, v: u8| {
            let i = ((y * size + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
        };
        // White outline, black line, a gap in the middle.
        for pass in [(n, 255u8), (0, 0u8)] {
            let (grow, v) = pass;
            for t in 0..size {
                if t + 3 * n > c && t < c + 3 * n + 1 {
                    continue;
                }
                for w in 0..=(2 * grow) {
                    let o = (c + w).saturating_sub(grow).min(size - 1);
                    put(t, o, v);
                    put(o, t, v);
                }
            }
        }
        self.set_cursor_image(&rgba, size, (c, c), n as f32);
    }

    // ---------------------------------------------------------------- gradients

    /// Read the pixels of the path so far, when no read is in progress.
    fn next_batch(&mut self) {
        if self.batch.is_some() {
            return;
        }
        if self.pending.is_empty() {
            if self.finishing {
                self.finish_path();
            }
            return;
        }
        let points = std::mem::take(&mut self.pending);
        let step = 0.5 / self.overlays.iter().map(|o| o.geometry.scale().0).fold(1.0, f32::max).min(8.0);
        let mut path = Vec::new();
        let mut segments = Vec::new();
        for p in points {
            let from = self.trace.last_pixel();
            let overlays = &self.overlays;
            let locate = |p: [f32; 2]| {
                let i = overlays.iter().position(|o| o.geometry.contains(p))?;
                Some((i, overlays[i].geometry.to_pixels(p)))
            };
            let pixels = self.trace.advance(p, step, locate);
            let mut previous = from;
            for &(o, px) in &pixels {
                let start = match previous {
                    Some((po, pp)) if po == o => pp,
                    _ => px,
                };
                segments.push((o, start, px));
                previous = Some((o, px));
            }
            path.extend(pixels);
        }
        // What to read on each output: the path's pixels and those its drawing covers.
        let mut areas: HashMap<usize, Area> = HashMap::new();
        for &(o, a, b) in &segments {
            let bounds = Stroke::new(self.overlays[o].geometry.scale().0).bounds(a, b);
            let e = areas.entry(o).or_insert(bounds);
            *e = (e.0.min(bounds.0), e.1.min(bounds.1), e.2.max(bounds.2), e.3.max(bounds.3));
        }
        let reads: Vec<(usize, usize)> = areas.into_iter().map(|(o, area)| (o, self.read(o, area))).collect();
        if reads.is_empty() {
            return;
        }
        self.batch = Some(Batch { path, segments, reads, pixels: HashMap::new() });
    }

    /// The pixels of a batch are read: take the path's colors, remember what its drawing
    /// covers, draw it.
    fn batch_ready(&mut self) {
        let Some(batch) = self.batch.take() else { return };
        for &(o, p) in &batch.path {
            let c = self.hidden.get(&(o, p)).copied().or_else(|| batch.pixels.get(&o)?.get(p).map(color));
            self.colors.extend(c);
        }
        for &(o, a, b) in &batch.segments {
            if !self.canvas(o) {
                continue;
            }
            let area = self.overlays[o].geometry.area();
            let canvas = self.overlays[o].canvas.as_mut().expect("canvas");
            let (x0, y0, x1, y1) = canvas.stroke.bounds(a, b);
            if let Some(pixels) = batch.pixels.get(&o) {
                for y in y0.max(0)..y1.min(area.3) {
                    for x in x0.max(0)..x1.min(area.2) {
                        if canvas.stroke.covers(stroke::segment_distance(a, b, (x, y)))
                            && !canvas.stroke.covered(area, &canvas.distance, (x, y))
                            && let Some(c) = pixels.get((x, y))
                        {
                            self.hidden.entry((o, (x, y))).or_insert(color(c));
                        }
                    }
                }
            }
            let changed = canvas.stroke.mark(a, b, area, &mut canvas.distance);
            canvas.changed(changed);
        }
        for i in 0..self.overlays.len() {
            self.present(i);
        }
        self.next_batch();
    }

    fn finish_path(&mut self) {
        self.finishing = false;
        if self.colors.len() >= 2 {
            self.outcome = Some(Outcome::Path(std::mem::take(&mut self.colors)));
            return;
        }
        // A click without a drag: nothing to pick yet.
        self.colors.clear();
        self.trace = Trace::default();
        self.hidden.clear();
        for i in 0..self.overlays.len() {
            let area = self.overlays[i].geometry.area();
            if let Some(canvas) = &mut self.overlays[i].canvas {
                canvas.distance.fill(255);
                canvas.changed(area);
            }
            self.present(i);
        }
    }

    // ---------------------------------------------------------------- input

    fn pointer_event(&mut self, event: &PointerEvent) {
        let Some(i) = self.overlay_of(&event.surface) else { return };
        let g = self.overlays[i].geometry;
        let at = [g.x + event.position.0 as f32, g.y + event.position.1 as f32];
        match event.kind {
            PointerEventKind::Enter { serial } => {
                self.pointer_at = Some(at);
                if self.cursor.buffer.is_none() && self.mode == Mode::Gradient {
                    self.crosshair(g.scale().0);
                }
                self.apply_cursor(Some(serial));
            }
            PointerEventKind::Motion { .. } => {
                self.pointer_at = Some(at);
                if self.pressed && self.mode == Mode::Gradient {
                    self.pending.push(at);
                }
            }
            PointerEventKind::Press { button: BUTTON_LEFT, .. } => {
                self.pointer_at = Some(at);
                self.pressed = true;
                if self.mode == Mode::Gradient {
                    self.pending.push(at);
                }
            }
            PointerEventKind::Release { button: BUTTON_LEFT, .. } if self.pressed => {
                self.pressed = false;
                self.pointer_at = Some(at);
                match self.mode {
                    Mode::Color => {
                        self.loupe_read = None;
                        self.refresh_loupe(true);
                    }
                    Mode::Gradient => {
                        self.pending.push(at);
                        self.finishing = true;
                    }
                }
            }
            PointerEventKind::Press { button: BUTTON_RIGHT, .. } => self.right_pressed = true,
            PointerEventKind::Release { button: BUTTON_RIGHT, .. } if self.right_pressed => self.outcome = Some(Outcome::Cancelled),
            _ => {}
        }
    }

    /// Take everything off the screen.
    fn close(&mut self) {
        for pointer in self.pointers.drain(..) {
            if pointer.version() >= 3 {
                pointer.release();
            }
        }
        for keyboard in self.keyboards.drain(..) {
            if keyboard.version() >= 3 {
                keyboard.release();
            }
        }
        self.captures.clear();
        for overlay in self.overlays.drain(..) {
            if let Some(viewport) = &overlay.viewport {
                viewport.destroy();
            }
            drop(overlay);
        }
        self.cursor.surface.destroy();
    }
}

// -------------------------------------------------------------------- Wayland handlers

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        // A monitor plugged in while picking: the layout changed, start again.
        if !self.overlays.is_empty() {
            self.outcome = Some(Outcome::Cancelled);
        }
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        let geometry = self.outputs.info(&output).as_ref().and_then(Geometry::of);
        if let Some(overlay) = self.overlays.iter_mut().find(|o| o.output == output)
            && let Some(geometry) = geometry
            && (geometry.pixels != overlay.geometry.pixels || geometry.transform != overlay.geometry.transform)
        {
            self.outcome = Some(Outcome::Cancelled);
        } else if let Some(overlay) = self.overlays.iter_mut().find(|o| o.output == output)
            && let Some(geometry) = geometry
        {
            overlay.geometry = geometry;
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        if self.overlays.iter().any(|o| o.output == output) {
            self.outcome = Some(Outcome::Cancelled);
        }
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.outcome = Some(Outcome::Cancelled);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        if let Some(i) = self.overlay_of(layer.wl_surface()) {
            self.configured(i, configure.new_size);
        }
    }
}

impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.outcome = Some(Outcome::Cancelled);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, window: &Window, configure: WindowConfigure, _: u32) {
        if let Some(i) = self.overlay_of(window.wl_surface()) {
            let size = configure.new_size;
            self.configured(i, (size.0.map_or(0, |w| w.get()), size.1.map_or(0, |h| h.get())));
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability) {
        match capability {
            Capability::Pointer => {
                if let Ok(pointer) = self.seats.get_pointer(qh, &seat) {
                    self.pointers.push(pointer);
                }
            }
            Capability::Keyboard => self.keyboards.push(seat.get_keyboard(qh, ())),
            _ => {}
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlPointer, events: &[PointerEvent]) {
        for event in events {
            self.pointer_event(event);
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(state: &mut State, _: &WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {
        if let wl_keyboard::Event::Key { key: KEY_ESCAPE, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } = event {
            state.outcome = Some(Outcome::Cancelled);
        }
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn event(_: &mut State, _: &ZwlrScreencopyManagerV1, _: <ZwlrScreencopyManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {}
}

impl Dispatch<WpViewport, ()> for State {
    fn event(_: &mut State, _: &WpViewport, _: <WpViewport as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<State>) {}
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_layer!(State);
delegate_xdg_shell!(State);
delegate_xdg_window!(State);
delegate_registry!(State);
delegate_simple!(State, WpViewporter, 1);

/// These run the real eyedropper in a compositor that is only for them (a headless
/// Sway): a surface of their own shows the reference image, the pointer is a virtual one.
/// `WAYLAND_DISPLAY=<the test compositor> cargo test -p tessera -- --ignored --test-threads 1 eyedropper::wayland`
#[cfg(test)]
mod live_tests {
    use std::sync::Mutex;

    use wayland_client::delegate_noop;
    use wayland_client::protocol::wl_pointer::ButtonState;
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

    use super::*;
    use crate::eyedropper::{self, Host};

    static SERIAL: Mutex<()> = Mutex::new(());
    /// Where the reference image is (logical units of the first output), and its size:
    /// within the smallest output the tests use (400 wide, portrait at 200%).
    const REFERENCE: (i32, i32, i32, i32) = (40, 160, 301, 80);

    /// The reference image: black → red → white across.
    fn ramp(x: i32) -> [u8; 3] {
        let t = x as f32 / (REFERENCE.2 - 1) as f32;
        let (r, g) = if t < 0.5 { (t * 2.0, 0.0) } else { (1.0, (t - 0.5) * 2.0) };
        [(r * 255.0).round() as u8, (g * 255.0).round() as u8, (g * 255.0).round() as u8]
    }

    struct Test {
        registry: RegistryState,
        outputs: OutputState,
        shm: Shm,
        configured: bool,
    }

    impl CompositorHandler for Test {
        fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: i32) {}
        fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: Transform) {}
        fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {}
        fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
        fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: &WlOutput) {}
    }
    impl OutputHandler for Test {
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.outputs
        }
        fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
        fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
        fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    }
    impl LayerShellHandler for Test {
        fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}
        fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
            self.configured = true;
        }
    }
    impl ShmHandler for Test {
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.shm
        }
    }
    impl ProvidesRegistryState for Test {
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry
        }
        registry_handlers![OutputState];
    }
    delegate_compositor!(Test);
    delegate_output!(Test);
    delegate_shm!(Test);
    delegate_layer!(Test);
    delegate_registry!(Test);
    delegate_noop!(Test: ignore WlSeat);
    delegate_noop!(Test: ZwlrVirtualPointerManagerV1);
    delegate_noop!(Test: ZwlrVirtualPointerV1);

    /// The test's side: the reference image and the virtual pointer.
    struct Session {
        queue: EventQueue<Test>,
        state: Test,
        _layer: LayerSurface,
        _buffer: Buffer,
        pointer: ZwlrVirtualPointerV1,
        /// The first output's logical size.
        extent: (u32, u32),
        time: u32,
    }

    impl Session {
        fn open() -> Option<Session> {
            let conn = Connection::connect_to_env().ok()?;
            let (globals, mut queue) = registry_queue_init::<Test>(&conn).ok()?;
            let qh = queue.handle();
            let compositor = CompositorState::bind(&globals, &qh).ok()?;
            let layers = LayerShell::bind(&globals, &qh).ok()?;
            let shm = Shm::bind(&globals, &qh).ok()?;
            let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).ok()?;
            let seat: WlSeat = globals.bind(&qh, 1..=7, ()).ok()?;
            let mut state = Test { registry: RegistryState::new(&globals), outputs: OutputState::new(&globals, &qh), shm, configured: false };
            queue.roundtrip(&mut state).ok()?;
            queue.roundtrip(&mut state).ok()?;
            let output = state.outputs.outputs().next()?;
            let extent = state.outputs.info(&output)?.logical_size.map(|(w, h)| (w as u32, h as u32))?;
            let (x, y, w, h) = REFERENCE;
            let layer = layers.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Bottom, Some("reference"), Some(&output));
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_margin(y, 0, 0, x);
            layer.set_size(w as u32, h as u32);
            layer.commit();
            while !state.configured {
                queue.blocking_dispatch(&mut state).ok()?;
            }
            let mut pool = SlotPool::new((w * h * 4) as usize, &state.shm).ok()?;
            let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Xrgb8888).ok()?;
            for (i, px) in canvas.chunks_exact_mut(4).enumerate() {
                let [r, g, b] = ramp(i as i32 % w);
                px.copy_from_slice(&[b, g, r, 255]);
            }
            buffer.attach_to(layer.wl_surface()).ok()?;
            layer.wl_surface().damage_buffer(0, 0, w, h);
            layer.commit();
            let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
            queue.roundtrip(&mut state).ok()?;
            std::thread::sleep(Duration::from_millis(200));
            Some(Session { queue, state, _layer: layer, _buffer: buffer, pointer, extent, time: 0 })
        }

        fn flush(&mut self) {
            self.queue.roundtrip(&mut self.state).unwrap();
            // Let the eyedropper see each step (it reads the screen as it goes).
            std::thread::sleep(Duration::from_millis(4));
        }

        fn move_to(&mut self, (x, y): (i32, i32)) {
            self.time += 5;
            self.pointer.motion_absolute(self.time, x as u32, y as u32, self.extent.0, self.extent.1);
            self.pointer.frame();
            self.flush();
        }

        fn button(&mut self, button: u32, pressed: bool) {
            self.time += 5;
            self.pointer.button(self.time, button, if pressed { ButtonState::Pressed } else { ButtonState::Released });
            self.pointer.frame();
            self.flush();
        }
    }

    /// Save the test compositor's screen around the reference image, when asked for (with
    /// `grim`, if it is there).
    fn snapshot(name: &str) {
        let Some(dir) = std::env::var_os("TESSERA_UI_SNAPSHOTS") else { return };
        std::thread::sleep(Duration::from_millis(100));
        let (x, y, w, h) = REFERENCE;
        let area = format!("{},{} {}x{}", x - 20, y - 40, w + 40, h + 80);
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        let _ = std::process::Command::new("grim").arg("-g").arg(area).arg(path).status();
    }

    fn near(a: [f32; 4], b: [u8; 3]) -> bool {
        (0..3).all(|i| ((a[i] * 255.0).round() as i32 - b[i] as i32).abs() <= 1)
    }

    /// Start a pick and give it time to cover the screen.
    fn start(s: &mut Session, mode: Mode) -> impl Future<Output = Outcome> + use<> {
        let pick = eyedropper::pick(mode, Host::default());
        std::thread::sleep(Duration::from_millis(500));
        // The pointer enters the new surfaces when it moves.
        s.move_to((REFERENCE.0 - 20, REFERENCE.1 - 20));
        pick
    }

    #[test]
    #[ignore = "needs a compositor of its own"]
    fn a_gradient_is_picked_along_a_freehand_path() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut s) = Session::open() else {
            eprintln!("no test compositor (with layer shell and virtual pointers)");
            return;
        };
        let pick = start(&mut s, Mode::Gradient);
        let (x0, y0) = (REFERENCE.0, REFERENCE.1 + 40);
        s.move_to((x0, y0));
        s.button(BUTTON_LEFT, true);
        let (mut x, mut y) = (x0, y0);
        while x < x0 + REFERENCE.2 - 1 {
            x = (x + 3).min(x0 + REFERENCE.2 - 1);
            y = y0 + (((x - x0) as f32 / 20.0).sin() * 25.0) as i32;
            s.move_to((x, y));
        }
        // And back a little, over the path already drawn.
        for _ in 0..10 {
            x -= 3;
            s.move_to((x, y));
        }
        snapshot("eyedropper_wayland_path");
        s.button(BUTTON_LEFT, false);
        let Outcome::Path(colors) = futures::executor::block_on(pick) else { panic!("no path") };
        // One color per pixel: more on denser outputs.
        assert!(colors.len() >= 330, "{} colors", colors.len());
        assert!(near(colors[0], ramp(0)), "{:?}", colors[0]);
        assert!(colors.iter().any(|c| near(*c, ramp(300))), "the path went to the end of the image");
        // Every color is the image's (scaled images mix neighbors): nothing of the drawn
        // path is picked up.
        for c in &colors {
            assert!((0..301).any(|x| near(*c, ramp(x))), "{c:?} is not in the image");
        }
    }

    #[test]
    #[ignore = "needs a compositor of its own"]
    fn a_color_is_picked_with_a_click() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut s) = Session::open() else { return };
        let pick = start(&mut s, Mode::Color);
        for x in [60, 110, 75] {
            s.move_to((REFERENCE.0 + x, REFERENCE.1 + 30));
            std::thread::sleep(Duration::from_millis(40));
        }
        s.button(BUTTON_LEFT, true);
        s.button(BUTTON_LEFT, false);
        // At fractional scales the image is resampled: a neighbor's color may be under it.
        let Outcome::Color(c) = futures::executor::block_on(pick) else { panic!("no color") };
        assert!((73..=77).any(|x| near(c, ramp(x))), "{c:?}");
    }

    #[test]
    #[ignore = "needs a compositor of its own"]
    fn a_pick_can_be_cancelled() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut s) = Session::open() else { return };
        let pick = start(&mut s, Mode::Gradient);
        eyedropper::cancel();
        assert_eq!(futures::executor::block_on(pick), Outcome::Cancelled);
        // A click without a drag picks nothing; a right click cancels.
        let pick = start(&mut s, Mode::Gradient);
        s.button(BUTTON_LEFT, true);
        s.button(BUTTON_LEFT, false);
        s.button(BUTTON_RIGHT, true);
        s.button(BUTTON_RIGHT, false);
        assert_eq!(futures::executor::block_on(pick), Outcome::Cancelled);
    }
}
