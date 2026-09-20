//! Live picking on X11.
//!
//! The pointer and the keyboard are grabbed, so every click and key comes here and none
//! reaches the other programs; nothing covers the screen. The pixels under the pointer
//! are read from the screen as it moves, so nothing stops.
//!
//! - One color: the pointer is a loupe of the pixels around it (an ARGB cursor, which is
//!   never part of what is read from the screen), a click picks.
//! - A gradient: dragging draws a black path with two shaped windows; the pixels they
//!   are about to cover are read first and remembered, so the path is never picked up.
//!
//! Esc or a right click cancels.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::Event;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::render::{self, ConnectionExt as _, PictType};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    GrabMode, GrabStatus, ImageFormat, ImageOrder, Rectangle, StackMode, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::loupe::{self, Loupe};
use super::path::{PathSampler, Pixel};
use super::stroke::segment_distance;
use super::{Cancel, Mode, Outcome, color};

/// `ClientMessage` data: the pick was cancelled / time to refresh the loupe.
const CANCEL: u32 = 1;
const TICK: u32 = 2;
/// Glyphs of the cursor font.
const XC_CROSSHAIR: u16 = 34;
const ESCAPE_KEYSYM: u32 = 0xff1b;
/// The loupe follows what is on the screen at least this often while the pointer rests.
const REFRESH: Duration = Duration::from_millis(33);
const MIN_REFRESH: Duration = Duration::from_millis(8);

type Error = Box<dyn std::error::Error>;

pub fn pick(mode: Mode, cancel: &Cancel) -> Outcome {
    let conn = match x11rb::connect(None) {
        Ok((conn, screen)) => (Arc::new(conn), screen),
        Err(e) => return Outcome::Failed(format!("Cannot connect to the X server: {e}")),
    };
    match Picker::new(mode, conn, cancel).and_then(|mut picker| picker.run(cancel)) {
        Ok(outcome) => outcome,
        Err(e) => Outcome::Failed(format!("The eyedropper failed: {e}")),
    }
}

/// How the pixels read from the screen are laid out.
struct Format {
    bytes_per_pixel: usize,
    scanline_pad: usize,
    msb_first: bool,
    masks: [u32; 3],
}

impl Format {
    /// RGB of pixel `(x, y)` of an image `width` pixels wide.
    fn pixel(&self, data: &[u8], width: usize, x: usize, y: usize) -> Option<[u8; 3]> {
        let stride = (width * self.bytes_per_pixel * 8).div_ceil(self.scanline_pad) * self.scanline_pad / 8;
        let at = y * stride + x * self.bytes_per_pixel;
        let bytes = data.get(at..at + self.bytes_per_pixel)?;
        let value = bytes.iter().enumerate().fold(0u32, |v, (i, b)| {
            let shift = if self.msb_first { (self.bytes_per_pixel - 1 - i) * 8 } else { i * 8 };
            v | (*b as u32) << shift
        });
        Some(self.masks.map(|mask| {
            if mask == 0 {
                return 0;
            }
            let bits = mask.count_ones();
            let v = (value & mask) >> mask.trailing_zeros();
            ((v * 255 + ((1 << bits) - 1) / 2) / ((1 << bits) - 1)) as u8
        }))
    }
}

/// The two windows drawing the path: a light edge and the black line over it, each
/// shaped to the path (they need no compositor).
struct Line {
    edge: Window,
    core: Window,
    /// Radii of the black line and of its edge, in pixels.
    core_radius: f32,
    edge_radius: f32,
    /// Pixels the path covers, and what they showed before.
    covered: HashSet<Pixel>,
}

struct Picker {
    conn: Arc<RustConnection>,
    mode: Mode,
    root: Window,
    size: (i32, i32),
    /// The monitors (RandR); pixels elsewhere are not on any screen.
    monitors: Vec<(i32, i32, i32, i32)>,
    format: Format,
    scale: f32,
    escape: Vec<u8>,
    /// Receives the app's cancel and the refresh ticks.
    messenger: Window,
    crosshair: u32,
    /// ARGB32, for the loupe cursor; none without a recent enough RENDER.
    argb: Option<render::Pictformat>,
    loupe: Option<(u32, Vec<Option<[u8; 3]>>)>,
    refreshed: Option<(Instant, Pixel)>,
    pressed: bool,
    right_pressed: bool,
    path: PathSampler,
    /// The colors along the path, in order.
    colors: Vec<[f32; 4]>,
    line: Option<Line>,
    hidden: HashMap<Pixel, [f32; 4]>,
    ticking: Arc<AtomicBool>,
}

impl Picker {
    fn new(mode: Mode, (conn, screen): (Arc<RustConnection>, usize), cancel: &Cancel) -> Result<Picker, Error> {
        let setup = conn.setup();
        let screen = setup.roots.get(screen).ok_or("no X screen")?;
        let root = screen.root;
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.visual_id == screen.root_visual)
            .ok_or("the screen's visual is unknown")?;
        let bits = setup.pixmap_formats.iter().find(|f| f.depth == screen.root_depth).ok_or("the screen's pixel format is unknown")?;
        if bits.bits_per_pixel < 8 || bits.bits_per_pixel % 8 != 0 {
            return Err("the screen has too few colors to pick from".into());
        }
        let format = Format {
            bytes_per_pixel: bits.bits_per_pixel as usize / 8,
            scanline_pad: bits.scanline_pad.max(8) as usize,
            msb_first: setup.image_byte_order == ImageOrder::MSB_FIRST,
            masks: [visual.red_mask, visual.green_mask, visual.blue_mask],
        };
        let size = (screen.width_in_pixels as i32, screen.height_in_pixels as i32);
        let monitors = conn
            .randr_get_monitors(root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.monitors.iter().map(|m| (m.x as i32, m.y as i32, m.x as i32 + m.width as i32, m.y as i32 + m.height as i32)).collect())
            .filter(|m: &Vec<_>| !m.is_empty())
            .unwrap_or_else(|| vec![(0, 0, size.0, size.1)]);
        let scale = xft_dpi(&conn, root).map_or(1.0, |dpi| dpi / 96.0);
        let escape = escape_keycodes(&conn)?;

        let messenger = conn.generate_id()?;
        conn.create_window(0, messenger, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new())?;
        let font = conn.generate_id()?;
        conn.open_font(font, b"cursor")?;
        let crosshair = conn.generate_id()?;
        conn.create_glyph_cursor(crosshair, font, font, XC_CROSSHAIR, XC_CROSSHAIR + 1, 0, 0, 0, 0xffff, 0xffff, 0xffff)?;
        conn.close_font(font)?;
        let argb = argb_format(&conn);

        let mut picker = Picker {
            conn: conn.clone(),
            mode,
            root,
            size,
            monitors,
            format,
            scale,
            escape,
            messenger,
            crosshair,
            argb,
            loupe: None,
            refreshed: None,
            pressed: false,
            right_pressed: false,
            path: PathSampler::default(),
            colors: Vec::new(),
            line: None,
            hidden: HashMap::new(),
            ticking: Arc::new(AtomicBool::new(true)),
        };
        {
            let conn = conn.clone();
            cancel.on_cancel(move || {
                let _ = send(&conn, messenger, CANCEL);
            });
        }
        if mode == Mode::Color {
            picker.refresh_loupe()?;
        }
        picker.grab()?;
        picker.start_ticking();
        Ok(picker)
    }

    fn cursor(&self) -> u32 {
        match (&self.loupe, self.mode) {
            (Some((cursor, _)), Mode::Color) => *cursor,
            _ => self.crosshair,
        }
    }

    /// Take the pointer and the keyboard; another program may hold them for a moment.
    fn grab(&self) -> Result<(), Error> {
        let events = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let started = Instant::now();
        loop {
            let status = self
                .conn
                .grab_pointer(false, self.root, events, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, self.cursor(), x11rb::CURRENT_TIME)?
                .reply()?
                .status;
            if status == GrabStatus::SUCCESS {
                break;
            }
            if started.elapsed() > Duration::from_secs(1) {
                return Err("another program is holding the mouse".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // Without the keyboard, Esc still works in Tessera's window.
        for _ in 0..25 {
            let reply = self.conn.grab_keyboard(false, self.root, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?.reply()?;
            if reply.status == GrabStatus::SUCCESS {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.conn.flush()?;
        Ok(())
    }

    /// Wakes the event loop regularly, so the loupe follows what changes on the screen.
    fn start_ticking(&self) {
        let (conn, messenger, ticking) = (self.conn.clone(), self.messenger, self.ticking.clone());
        let spawned = std::thread::Builder::new().name("eyedropper ticks".into()).spawn(move || {
            while ticking.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(16));
                if send(&conn, messenger, TICK).is_err() {
                    break;
                }
            }
        });
        // Without ticks the loupe only changes when the pointer moves.
        drop(spawned);
    }

    fn run(&mut self, cancel: &Cancel) -> Result<Outcome, Error> {
        loop {
            if cancel.is_cancelled() {
                return Ok(Outcome::Cancelled);
            }
            match self.conn.wait_for_event()? {
                Event::MotionNotify(e) => {
                    let p = (e.root_x as i32, e.root_y as i32);
                    match self.mode {
                        Mode::Color if self.refreshed.is_none_or(|(at, _)| at.elapsed() >= MIN_REFRESH) => self.refresh_loupe()?,
                        Mode::Gradient if self.pressed => self.extend_path(p)?,
                        _ => {}
                    }
                }
                Event::ButtonPress(e) => match e.detail {
                    1 => {
                        self.pressed = true;
                        if self.mode == Mode::Gradient {
                            self.clear_path()?;
                            self.extend_path((e.root_x as i32, e.root_y as i32))?;
                        }
                    }
                    3 => self.right_pressed = true,
                    _ => {}
                },
                Event::ButtonRelease(e) => match e.detail {
                    1 if std::mem::take(&mut self.pressed) => {
                        let p = (e.root_x as i32, e.root_y as i32);
                        match self.mode {
                            Mode::Color => {
                                if let Some(c) = self.read(p, (1, 1))?.pixel(p) {
                                    return Ok(Outcome::Color(color(c)));
                                }
                            }
                            Mode::Gradient => {
                                self.extend_path(p)?;
                                if self.colors.len() >= 2 {
                                    return Ok(Outcome::Path(std::mem::take(&mut self.colors)));
                                }
                                // A click without a drag: nothing to pick yet.
                                self.clear_path()?;
                            }
                        }
                    }
                    3 if self.right_pressed => return Ok(Outcome::Cancelled),
                    _ => {}
                },
                Event::KeyPress(e) if self.escape.contains(&e.detail) => return Ok(Outcome::Cancelled),
                Event::ClientMessage(e) if e.window == self.messenger => match e.data.as_data32()[0] {
                    CANCEL => return Ok(Outcome::Cancelled),
                    _ if self.mode == Mode::Color => {
                        let moved = self.pointer()?.is_some_and(|p| self.refreshed.is_none_or(|(_, q)| q != p));
                        if moved || self.refreshed.is_none_or(|(at, _)| at.elapsed() >= REFRESH) {
                            self.refresh_loupe()?;
                        }
                    }
                    _ => {}
                },
                Event::Error(e) => return Err(format!("{e:?}").into()),
                _ => {}
            }
        }
    }

    fn pointer(&self) -> Result<Option<Pixel>, Error> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        Ok(reply.same_screen.then_some((reply.root_x as i32, reply.root_y as i32)))
    }

    fn on_monitor(&self, (x, y): Pixel) -> bool {
        self.monitors.iter().any(|&(l, t, r, b)| x >= l && y >= t && x < r && y < b)
    }

    /// Read a rectangle of the screen (clipped to it).
    fn read(&self, (x, y): Pixel, (w, h): (i32, i32)) -> Result<Read<'_>, Error> {
        let (x0, y0) = (x.clamp(0, self.size.0), y.clamp(0, self.size.1));
        let (x1, y1) = ((x + w).clamp(0, self.size.0), (y + h).clamp(0, self.size.1));
        let area = (x0, y0, x1, y1);
        if x1 <= x0 || y1 <= y0 {
            return Ok(Read { picker: self, area, data: Vec::new() });
        }
        let reply = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, x0 as i16, y0 as i16, (x1 - x0) as u16, (y1 - y0) as u16, !0)?.reply()?;
        Ok(Read { picker: self, area, data: reply.data })
    }

    /// Show the pixels around the pointer in the loupe cursor.
    fn refresh_loupe(&mut self) -> Result<(), Error> {
        let Some(format) = self.argb else { return Ok(()) };
        let Some(p) = self.pointer()? else { return Ok(()) };
        self.refreshed = Some((Instant::now(), p));
        let half = (loupe::PIXELS / 2) as i32;
        let n = loupe::PIXELS as i32;
        let read = self.read((p.0 - half, p.1 - half), (n, n))?;
        let pixels: Vec<Option<[u8; 3]>> = (0..n)
            .flat_map(|dy| (0..n).map(move |dx| (p.0 - half + dx, p.1 - half + dy)))
            .map(|q| if self.on_monitor(q) { read.pixel(q) } else { None })
            .collect();
        drop(read);
        if self.loupe.as_ref().is_some_and(|(_, shown)| *shown == pixels) {
            return Ok(());
        }
        let cursor = self.make_cursor(&loupe::render(&pixels, self.scale), format)?;
        self.conn.change_active_pointer_grab(cursor, x11rb::CURRENT_TIME, EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION)?;
        if let Some((old, _)) = self.loupe.replace((cursor, pixels)) {
            self.conn.free_cursor(old)?;
        }
        self.conn.flush()?;
        Ok(())
    }

    fn make_cursor(&self, loupe: &Loupe, format: render::Pictformat) -> Result<u32, ReplyOrIdError> {
        let conn = &*self.conn;
        let size = loupe.size as u16;
        let pixmap = conn.generate_id()?;
        conn.create_pixmap(32, pixmap, self.root, size, size)?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, pixmap, &CreateGCAux::new())?;
        // ARGB32, premultiplied, in the server's byte order.
        let data: Vec<u8> = loupe
            .rgba
            .chunks_exact(4)
            .flat_map(|p| if self.format.msb_first { [p[3], p[0], p[1], p[2]] } else { [p[2], p[1], p[0], p[3]] })
            .collect();
        conn.put_image(ImageFormat::Z_PIXMAP, pixmap, gc, size, size, 0, 0, 0, 32, &data)?;
        let picture = conn.generate_id()?;
        conn.render_create_picture(picture, pixmap, format, &render::CreatePictureAux::new())?;
        let cursor = conn.generate_id()?;
        conn.render_create_cursor(cursor, picture, loupe.hotspot.0 as u16, loupe.hotspot.1 as u16)?;
        conn.render_free_picture(picture)?;
        conn.free_gc(gc)?;
        conn.free_pixmap(pixmap)?;
        Ok(cursor)
    }

    fn clear_path(&mut self) -> Result<(), Error> {
        self.path = PathSampler::default();
        self.colors.clear();
        self.hidden.clear();
        if let Some(line) = &mut self.line {
            for window in [line.edge, line.core] {
                self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, window, 0, 0, &[])?;
            }
            line.covered.clear();
            self.conn.flush()?;
        }
        Ok(())
    }

    fn line(&mut self) -> Result<&mut Line, Error> {
        if self.line.is_none() {
            let make = |background: u32| -> Result<Window, Error> {
                let window = self.conn.generate_id()?;
                let aux = CreateWindowAux::new().background_pixel(background).override_redirect(1);
                self.conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, self.root, 0, 0, self.size.0 as u16, self.size.1 as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux)?;
                // Nothing of it shows until the path is drawn; it never takes the mouse.
                self.conn.shape_rectangles(shape::SO::SET, shape::SK::BOUNDING, ClipOrdering::UNSORTED, window, 0, 0, &[])?;
                self.conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, window, 0, 0, &[])?;
                self.conn.map_window(window)?;
                self.conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))?;
                Ok(window)
            };
            let white = self.pixel_value([205, 205, 205]);
            let black = self.pixel_value([0, 0, 0]);
            let edge = make(white)?;
            let core = make(black)?;
            let scale = self.scale.clamp(1.0, 4.0);
            self.line = Some(Line { edge, core, core_radius: 1.5 * scale, edge_radius: 2.5 * scale, covered: HashSet::new() });
        }
        Ok(self.line.as_mut().expect("line"))
    }

    /// A color as a pixel value of the screen's visual.
    fn pixel_value(&self, rgb: [u8; 3]) -> u32 {
        self.format.masks.iter().zip(rgb).fold(0, |v, (&mask, c)| {
            if mask == 0 {
                return v;
            }
            let bits = mask.count_ones();
            v | ((c as u32 * ((1 << bits) - 1) / 255) << mask.trailing_zeros())
        })
    }

    /// Extend the path to the pointer. A fast mouse jumps far between two events: the way
    /// there is walked in steps, so that no step reads half the screen at once.
    fn extend_path(&mut self, to: Pixel) -> Result<(), Error> {
        let from = self.path.last_pixel().unwrap_or(to);
        for step in super::path::steps((from.0 as f32, from.1 as f32), (to.0 as f32, to.1 as f32)) {
            self.extend_path_step((step.0.round() as i32, step.1.round() as i32))?;
        }
        Ok(())
    }

    /// Extend the path by one step: read the colors it crosses, and those its drawing
    /// will cover, then draw it.
    fn extend_path_step(&mut self, to: Pixel) -> Result<(), Error> {
        let from = self.path.last_pixel().unwrap_or(to);
        let pixels = self.path.advance((to.0 as f32 + 0.5, to.1 as f32 + 0.5));
        let (core_radius, edge_radius) = {
            let line = self.line()?;
            (line.core_radius, line.edge_radius)
        };
        let r = edge_radius.ceil() as i32;
        let (bx0, by0) = (from.0.min(to.0) - r, from.1.min(to.1) - r);
        let (bx1, by1) = (from.0.max(to.0) + r + 1, from.1.max(to.1) + r + 1);
        let mut edge = Vec::new();
        let mut core = Vec::new();
        for y in by0..by1 {
            for x in bx0..bx1 {
                let d = segment_distance(from, to, (x, y));
                if d < edge_radius {
                    edge.push((x, y));
                }
                if d < core_radius {
                    core.push((x, y));
                }
            }
        }
        let read = self.read((bx0, by0), (bx1 - bx0, by1 - by0))?;
        let mut hidden = Vec::new();
        let mut colors = Vec::new();
        {
            let line = self.line.as_ref().expect("line");
            for &p in &pixels {
                if !self.on_monitor(p) {
                    continue;
                }
                let c = self.hidden.get(&p).copied().or_else(|| read.pixel(p).map(color));
                colors.extend(c.map(|c| (p, c)));
            }
            for &p in &edge {
                if !line.covered.contains(&p)
                    && let Some(c) = read.pixel(p)
                {
                    hidden.push((p, color(c)));
                }
            }
        }
        drop(read);
        for (_, c) in colors {
            if self.colors.len() < super::path::MAX_COLORS {
                self.colors.push(c);
            }
        }
        for (p, c) in hidden {
            self.hidden.entry(p).or_insert(c);
        }
        let line = self.line.as_mut().expect("line");
        let new_edge: Vec<Pixel> = edge.into_iter().filter(|p| line.covered.insert(*p)).collect();
        let (edge_window, core_window) = (line.edge, line.core);
        for (window, pixels) in [(edge_window, new_edge), (core_window, core)] {
            let rects = runs(&pixels);
            for chunk in rects.chunks(4096) {
                self.conn.shape_rectangles(shape::SO::UNION, shape::SK::BOUNDING, ClipOrdering::UNSORTED, window, 0, 0, chunk)?;
            }
        }
        self.conn.flush()?;
        Ok(())
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.ticking.store(false, Ordering::SeqCst);
        let conn = &self.conn;
        let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
        let _ = conn.ungrab_keyboard(x11rb::CURRENT_TIME);
        if let Some(line) = self.line.take() {
            let _ = conn.destroy_window(line.core);
            let _ = conn.destroy_window(line.edge);
        }
        if let Some((cursor, _)) = self.loupe.take() {
            let _ = conn.free_cursor(cursor);
        }
        let _ = conn.free_cursor(self.crosshair);
        let _ = conn.destroy_window(self.messenger);
        let _ = conn.flush();
    }
}

/// Pixels read from a rectangle of the screen.
struct Read<'a> {
    picker: &'a Picker,
    area: (i32, i32, i32, i32),
    data: Vec<u8>,
}

impl Read<'_> {
    fn pixel(&self, (x, y): Pixel) -> Option<[u8; 3]> {
        let (x0, y0, x1, y1) = self.area;
        if x < x0 || y < y0 || x >= x1 || y >= y1 {
            return None;
        }
        self.picker.format.pixel(&self.data, (x1 - x0) as usize, (x - x0) as usize, (y - y0) as usize)
    }
}

/// Pixels as rectangles: runs along the rows.
fn runs(pixels: &[Pixel]) -> Vec<Rectangle> {
    let mut sorted: Vec<Pixel> = pixels.to_vec();
    sorted.sort_unstable_by_key(|&(x, y)| (y, x));
    sorted.dedup();
    let mut rects: Vec<Rectangle> = Vec::new();
    for (x, y) in sorted {
        let (Ok(x), Ok(y)) = (i16::try_from(x), i16::try_from(y)) else { continue };
        match rects.last_mut() {
            Some(r) if r.y == y && r.x as i32 + r.width as i32 == x as i32 => r.width += 1,
            _ => rects.push(Rectangle { x, y, width: 1, height: 1 }),
        }
    }
    rects
}

fn send(conn: &RustConnection, window: Window, what: u32) -> Result<(), Error> {
    let event = ClientMessageEvent::new(32, window, AtomEnum::NOTICE, [what, 0, 0, 0, 0]);
    conn.send_event(false, window, EventMask::NO_EVENT, event)?;
    conn.flush()?;
    Ok(())
}

/// The keycodes of Esc.
fn escape_keycodes(conn: &RustConnection) -> Result<Vec<u8>, Error> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let mapping = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let per = mapping.keysyms_per_keycode.max(1) as usize;
    Ok(mapping
        .keysyms
        .chunks(per)
        .enumerate()
        .filter(|(_, syms)| syms.contains(&ESCAPE_KEYSYM))
        .map(|(i, _)| min + i as u8)
        .collect())
}

/// The ARGB32 picture format, when the server can make cursors of it (RENDER 0.5).
fn argb_format(conn: &RustConnection) -> Option<render::Pictformat> {
    let version = conn.render_query_version(0, 11).ok()?.reply().ok()?;
    if (version.major_version, version.minor_version) < (0, 5) {
        return None;
    }
    let formats = conn.render_query_pict_formats().ok()?.reply().ok()?;
    formats
        .formats
        .iter()
        .find(|f| {
            let d = &f.direct;
            f.type_ == PictType::DIRECT
                && f.depth == 32
                && (d.alpha_shift, d.alpha_mask, d.red_shift, d.red_mask, d.green_shift, d.green_mask, d.blue_shift, d.blue_mask)
                    == (24, 0xff, 16, 0xff, 8, 0xff, 0, 0xff)
        })
        .map(|f| f.id)
}

/// The desktop's scale, from `Xft.dpi` (what toolkits scale by on X11).
fn xft_dpi(conn: &RustConnection, root: Window) -> Option<f32> {
    let reply = conn.get_property(false, root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 20).ok()?.reply().ok()?;
    let text = String::from_utf8_lossy(&reply.value).into_owned();
    text.lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|dpi| dpi.is_finite() && *dpi >= 48.0 && *dpi <= 480.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_decoded_in_every_layout() {
        // 24-bit color in 32-bit pixels, both byte orders.
        let lsb = Format { bytes_per_pixel: 4, scanline_pad: 32, msb_first: false, masks: [0xff0000, 0xff00, 0xff] };
        assert_eq!(lsb.pixel(&[0x30, 0x20, 0x10, 0, 0x33, 0x22, 0x11, 0], 2, 1, 0), Some([0x11, 0x22, 0x33]));
        let msb = Format { msb_first: true, ..lsb };
        assert_eq!(msb.pixel(&[0, 0x10, 0x20, 0x30], 1, 0, 0), Some([0x10, 0x20, 0x30]));
        // 16-bit 565 with padded rows: 3 pixels take 8 bytes a row.
        let rgb565 = Format { bytes_per_pixel: 2, scanline_pad: 32, msb_first: false, masks: [0xf800, 0x07e0, 0x001f] };
        let row = [0u8, 0, 0, 0, 0, 0, 0, 0];
        let mut data = row.to_vec();
        data.extend([0xff, 0xff, 0, 0, 0x1f, 0, 0, 0]);
        assert_eq!(rgb565.pixel(&data, 3, 0, 1), Some([255, 255, 255]));
        assert_eq!(rgb565.pixel(&data, 3, 2, 1), Some([0, 0, 255]));
        // Out of the data: none rather than a panic.
        assert_eq!(rgb565.pixel(&data, 3, 3, 5), None);
    }

    #[test]
    fn pixels_become_row_runs() {
        let rects = runs(&[(3, 1), (1, 1), (2, 1), (5, 1), (2, 2), (2, 1)]);
        let as_tuples: Vec<_> = rects.iter().map(|r| (r.x, r.y, r.width, r.height)).collect();
        assert_eq!(as_tuples, [(1, 1, 3, 1), (5, 1, 1, 1), (2, 2, 1, 1)]);
    }
}

/// These run the real eyedropper on an X server that is only for them (Xvfb): a window
/// of their own shows the reference image, the input is simulated with XTEST.
/// `Xvfb :99 & DISPLAY=:99 cargo test -p tessera -- --ignored --test-threads 1 eyedropper::x11`
#[cfg(test)]
mod live_tests {
    use std::sync::Mutex;

    use x11rb::protocol::xproto::{BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT};
    use x11rb::protocol::xtest::ConnectionExt as _;

    use super::*;
    use crate::eyedropper::{self, Host};

    static SERIAL: Mutex<()> = Mutex::new(());
    const REFERENCE: (i32, i32, i32, i32) = (160, 160, 301, 80);

    /// The reference image: black → red → white across.
    fn ramp(x: i32) -> [u8; 3] {
        let t = x as f32 / (REFERENCE.2 - 1) as f32;
        let (r, g) = if t < 0.5 { (t * 2.0, 0.0) } else { (1.0, (t - 0.5) * 2.0) };
        [(r * 255.0).round() as u8, (g * 255.0).round() as u8, (g * 255.0).round() as u8]
    }

    struct Session {
        conn: RustConnection,
        root: Window,
        window: Window,
    }

    impl Session {
        /// Connect to the test X server and show the reference image; `None` without one.
        fn open() -> Option<Session> {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                eprintln!("a Wayland session: the X11 eyedropper is not used");
                return None;
            }
            let (conn, screen) = x11rb::connect(None).ok()?;
            let screen = &conn.setup().roots[screen];
            let (root, depth) = (screen.root, screen.root_depth);
            let (x, y, w, h) = REFERENCE;
            let pixmap = conn.generate_id().ok()?;
            conn.create_pixmap(depth, pixmap, root, w as u16, h as u16).ok()?;
            let gc = conn.generate_id().ok()?;
            conn.create_gc(gc, pixmap, &CreateGCAux::new()).ok()?;
            let data: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| {
                let [r, g, b] = ramp(x);
                [b, g, r, 0]
            })).collect();
            for (i, rows) in data.chunks(w as usize * 4 * 16).enumerate() {
                let n = rows.len() / (w as usize * 4);
                conn.put_image(ImageFormat::Z_PIXMAP, pixmap, gc, w as u16, n as u16, 0, (i * 16) as i16, 0, depth, rows).ok()?;
            }
            let window = conn.generate_id().ok()?;
            let aux = CreateWindowAux::new().background_pixmap(pixmap).override_redirect(1);
            conn.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, x as i16, y as i16, w as u16, h as u16, 0, WindowClass::INPUT_OUTPUT, 0, &aux).ok()?;
            conn.map_window(window).ok()?;
            conn.flush().ok()?;
            std::thread::sleep(Duration::from_millis(200));
            Some(Session { conn, root, window })
        }

        fn fake(&self, kind: u8, detail: u8, at: Option<Pixel>) {
            let (x, y) = at.unwrap_or((0, 0));
            self.conn.xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.root, x as i16, y as i16, 0).unwrap();
            self.conn.flush().unwrap();
            // Let the eyedropper see each step (it reads the screen as it goes).
            std::thread::sleep(Duration::from_millis(3));
        }

        fn move_to(&self, p: Pixel) {
            self.fake(MOTION_NOTIFY_EVENT, 0, Some(p));
        }

        /// Save the test X server's screen around the reference image, when asked for.
        fn snapshot(&self, name: &str) {
            let (x, y, w, h) = (REFERENCE.0 - 40, REFERENCE.1 - 40, REFERENCE.2 + 80, REFERENCE.3 + 80);
            let reply = self.conn.get_image(ImageFormat::Z_PIXMAP, self.root, x as i16, y as i16, w as u16, h as u16, !0).unwrap().reply().unwrap();
            let rgba: Vec<u8> = reply.data.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
            crate::eyedropper::save_snapshot(name, w as u32, h as u32, &rgba, false);
        }

        /// Wait until the eyedropper holds the pointer.
        fn wait_for_grab(&self) {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                let reply = self.conn.grab_pointer(false, self.root, EventMask::NO_EVENT, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, x11rb::CURRENT_TIME).unwrap().reply().unwrap();
                if reply.status == GrabStatus::ALREADY_GRABBED {
                    return;
                }
                self.conn.ungrab_pointer(x11rb::CURRENT_TIME).unwrap();
                self.conn.flush().unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the eyedropper did not grab the pointer");
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            let _ = self.conn.destroy_window(self.window);
            let _ = self.conn.flush();
        }
    }

    fn near(a: [f32; 4], b: [u8; 3]) -> bool {
        (0..3).all(|i| ((a[i] * 255.0).round() as i32 - b[i] as i32).abs() <= 1)
    }

    #[test]
    #[ignore = "needs an X server of its own"]
    fn a_gradient_is_picked_along_a_freehand_path() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = Session::open() else { return };
        let (x0, y0) = (REFERENCE.0, REFERENCE.1 + 40);
        s.move_to((x0, y0));
        let pick = eyedropper::pick(Mode::Gradient, Host::default());
        s.wait_for_grab();
        s.fake(BUTTON_PRESS_EVENT, 1, None);
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
        std::thread::sleep(Duration::from_millis(100));
        s.snapshot("eyedropper_x11_path");
        s.fake(BUTTON_RELEASE_EVENT, 1, None);
        let Outcome::Path(colors) = futures::executor::block_on(pick) else { panic!("no path") };
        assert!(colors.len() >= 330, "{} colors", colors.len());
        assert!(near(colors[0], ramp(0)), "{:?}", colors[0]);
        assert!(near(colors[colors.len() - 31], ramp(300)));
        // Every color is the image's: nothing of the drawn path is picked up.
        for c in &colors {
            assert!((0..301).any(|x| near(*c, ramp(x))), "{c:?} is not in the image");
        }
    }

    #[test]
    #[ignore = "needs an X server of its own"]
    fn a_color_is_picked_with_a_click() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = Session::open() else { return };
        s.move_to((REFERENCE.0 + 10, REFERENCE.1 + 30));
        let pick = eyedropper::pick(Mode::Color, Host::default());
        s.wait_for_grab();
        for x in [60, 110, 75] {
            s.move_to((REFERENCE.0 + x, REFERENCE.1 + 30));
            std::thread::sleep(Duration::from_millis(30));
        }
        s.fake(BUTTON_PRESS_EVENT, 1, None);
        s.fake(BUTTON_RELEASE_EVENT, 1, None);
        assert_eq!(futures::executor::block_on(pick), Outcome::Color(color(ramp(75))));
    }

    #[test]
    #[ignore = "needs an X server of its own"]
    fn a_pick_can_be_cancelled() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = Session::open() else { return };
        // By the app.
        let pick = eyedropper::pick(Mode::Gradient, Host::default());
        s.wait_for_grab();
        eyedropper::cancel();
        assert_eq!(futures::executor::block_on(pick), Outcome::Cancelled);
        // With Esc.
        let pick = eyedropper::pick(Mode::Color, Host::default());
        s.wait_for_grab();
        let escape = escape_keycodes(&s.conn).unwrap()[0];
        s.fake(KEY_PRESS_EVENT, escape, None);
        s.fake(KEY_RELEASE_EVENT, escape, None);
        assert_eq!(futures::executor::block_on(pick), Outcome::Cancelled);
        // A click without a drag picks nothing; a right click cancels.
        let pick = eyedropper::pick(Mode::Gradient, Host::default());
        s.wait_for_grab();
        s.fake(BUTTON_PRESS_EVENT, 1, None);
        s.fake(BUTTON_RELEASE_EVENT, 1, None);
        s.fake(BUTTON_PRESS_EVENT, 3, None);
        s.fake(BUTTON_RELEASE_EVENT, 3, None);
        assert_eq!(futures::executor::block_on(pick), Outcome::Cancelled);
    }
}
