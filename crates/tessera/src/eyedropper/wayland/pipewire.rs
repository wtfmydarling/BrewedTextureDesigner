//! Reading the screen through PipeWire, the way the desktop portal hands it over (GNOME,
//! KDE and any desktop with a screen cast portal). The stream is live: the frames keep
//! coming while the pick lasts, so nothing on the screen has to stop.
//!
//! libpipewire is loaded when the eyedropper needs it, so nothing has to be installed to
//! build Tessera, and desktops that never need it never load it.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::fd::{IntoRawFd, OwnedFd};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock};

use super::Pixels;
use crate::eyedropper::stroke::Area;

// ---------------------------------------------------------------- the library

type Handle = *mut c_void;

macro_rules! functions {
    ($($name:ident: unsafe extern "C" fn($($arg:ty),* $(,)?) $(-> $ret:ty)?),* $(,)?) => {
        /// The parts of libpipewire the eyedropper uses.
        #[allow(non_snake_case)]
        struct Library {
            $($name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Library {
            /// Load libpipewire; `None` if it is not installed.
            fn load() -> Option<Library> {
                // SAFETY: loading a system library by its usual name and looking up its
                // documented functions.
                unsafe {
                    let handle = libc::dlopen(c"libpipewire-0.3.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
                    if handle.is_null() {
                        return None;
                    }
                    let mut missing = false;
                    let mut symbol = |name: &CStr| -> *mut c_void {
                        let f = libc::dlsym(handle, name.as_ptr());
                        missing |= f.is_null();
                        f
                    };
                    $(let $name = symbol(concat_cstr!($name));)*
                    if missing {
                        return None;
                    }
                    Some(Library { $($name: std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>($name),)* })
                }
            }
        }
    };
}

macro_rules! concat_cstr {
    ($name:ident) => {{
        const NAME: &str = concat!(stringify!($name), "\0");
        const SYMBOL: &CStr = match CStr::from_bytes_with_nul(NAME.as_bytes()) {
            Ok(name) => name,
            Err(_) => c"",
        };
        SYMBOL
    }};
}

functions! {
    pw_init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char),
    pw_thread_loop_new: unsafe extern "C" fn(*const c_char, *const c_void) -> Handle,
    pw_thread_loop_destroy: unsafe extern "C" fn(Handle),
    pw_thread_loop_start: unsafe extern "C" fn(Handle) -> c_int,
    pw_thread_loop_stop: unsafe extern "C" fn(Handle),
    pw_thread_loop_lock: unsafe extern "C" fn(Handle),
    pw_thread_loop_unlock: unsafe extern "C" fn(Handle),
    pw_thread_loop_get_loop: unsafe extern "C" fn(Handle) -> Handle,
    pw_context_new: unsafe extern "C" fn(Handle, Handle, usize) -> Handle,
    pw_context_destroy: unsafe extern "C" fn(Handle),
    pw_context_connect_fd: unsafe extern "C" fn(Handle, c_int, Handle, usize) -> Handle,
    pw_core_disconnect: unsafe extern "C" fn(Handle) -> c_int,
    pw_properties_new_string: unsafe extern "C" fn(*const c_char) -> Handle,
    pw_stream_new: unsafe extern "C" fn(Handle, *const c_char, Handle) -> Handle,
    pw_stream_destroy: unsafe extern "C" fn(Handle),
    pw_stream_add_listener: unsafe extern "C" fn(Handle, *mut SpaHook, *const StreamEvents, *mut c_void),
    pw_stream_connect: unsafe extern "C" fn(Handle, c_int, u32, u32, *const *const u8, u32) -> c_int,
    pw_stream_update_params: unsafe extern "C" fn(Handle, *const *const u8, u32) -> c_int,
    pw_stream_dequeue_buffer: unsafe extern "C" fn(Handle) -> *mut PwBuffer,
    pw_stream_queue_buffer: unsafe extern "C" fn(Handle, *mut PwBuffer) -> c_int,
}

fn library() -> Option<&'static Library> {
    static LIBRARY: OnceLock<Option<Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let library = Library::load()?;
            // SAFETY: the first call into the library, as it requires.
            unsafe { (library.pw_init)(null_mut(), null_mut()) };
            Some(library)
        })
        .as_ref()
}

// ---------------------------------------------------------------- the library's types

#[repr(C)]
struct SpaHook {
    link: [*mut c_void; 2],
    callbacks: [*const c_void; 2],
    removed: *mut c_void,
    private: *mut c_void,
}

impl Default for SpaHook {
    fn default() -> SpaHook {
        SpaHook { link: [null_mut(); 2], callbacks: [null(); 2], removed: null_mut(), private: null_mut() }
    }
}

#[repr(C)]
struct SpaChunk {
    offset: u32,
    size: u32,
    stride: i32,
    flags: i32,
}

#[repr(C)]
struct SpaData {
    kind: u32,
    flags: u32,
    fd: i64,
    map_offset: u32,
    max_size: u32,
    data: *mut c_void,
    chunk: *mut SpaChunk,
}

#[repr(C)]
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut c_void,
    datas: *mut SpaData,
}

#[repr(C)]
struct PwBuffer {
    buffer: *mut SpaBuffer,
    user_data: *mut c_void,
    size: u64,
    requested: u64,
    time: u64,
}

/// `pw_stream_events`, version 2.
#[repr(C)]
struct StreamEvents {
    version: u32,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
    control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
    param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const u8)>,
    add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    process: Option<unsafe extern "C" fn(*mut c_void)>,
    drained: Option<unsafe extern "C" fn(*mut c_void)>,
    command: Option<unsafe extern "C" fn(*mut c_void, *const c_void)>,
    trigger_done: Option<unsafe extern "C" fn(*mut c_void)>,
}

const PW_DIRECTION_INPUT: c_int = 0;
const PW_STREAM_FLAG_AUTOCONNECT: u32 = 1;
const PW_STREAM_FLAG_MAP_BUFFERS: u32 = 1 << 2;
const PW_STREAM_FLAG_DONT_RECONNECT: u32 = 1 << 7;

// ---------------------------------------------------------------- the frames

/// The video formats the eyedropper takes, as (SPA video format, the RGB of a pixel).
const FORMATS: [(u32, fn(&[u8]) -> [u8; 3]); 8] = [
    (8, |p| [p[2], p[1], p[0]]),  // BGRx
    (12, |p| [p[2], p[1], p[0]]), // BGRA
    (7, |p| [p[0], p[1], p[2]]),  // RGBx
    (11, |p| [p[0], p[1], p[2]]), // RGBA
    (9, |p| [p[1], p[2], p[3]]),  // xRGB
    (13, |p| [p[1], p[2], p[3]]), // ARGB
    (10, |p| [p[3], p[2], p[1]]), // xBGR
    (14, |p| [p[3], p[2], p[1]]), // ABGR
];

/// The frame a monitor's stream is showing, held until the next one arrives.
#[derive(Default)]
struct Frame {
    /// The buffer given back to PipeWire when another frame arrives.
    held: Option<*mut PwBuffer>,
    pixels: *const u8,
    length: usize,
    stride: usize,
    size: (i32, i32),
    /// Which of [`FORMATS`] the pixels are in.
    format: Option<usize>,
}

// SAFETY: the frame is only read while its buffer is held (PipeWire does not touch a
// buffer that is out of its queue), and every use of the pointers goes through the mutex.
unsafe impl Send for Frame {}

/// What a stream's callbacks need.
struct StreamData {
    stream: Mutex<Handle>,
    frame: Mutex<Frame>,
    /// What the stream is doing, and what went wrong (for the log).
    status: Mutex<(c_int, String)>,
    library: &'static Library,
}

// SAFETY: the handles are only used with the thread loop's lock held, or from the
// PipeWire thread itself.
unsafe impl Send for StreamData {}
unsafe impl Sync for StreamData {}

/// One monitor's stream of frames.
pub struct Stream {
    handle: Handle,
    data: Arc<StreamData>,
    /// Kept alive as long as the stream listens.
    _hook: Box<SpaHook>,
}

/// The monitors the desktop is streaming.
pub struct Streams {
    library: &'static Library,
    thread_loop: Handle,
    context: Handle,
    core: Handle,
    streams: Vec<Stream>,
}

impl Streams {
    /// Connect to the PipeWire the portal gave and follow every monitor of the session.
    pub fn open(fd: OwnedFd, nodes: &[u32]) -> Result<Streams, String> {
        let library = library().ok_or("PipeWire is not installed")?;
        // SAFETY: the calls below follow libpipewire's documented order; everything is
        // destroyed in `Drop`, with the thread loop stopped first.
        unsafe {
            let thread_loop = (library.pw_thread_loop_new)(c"tessera eyedropper".as_ptr(), null());
            if thread_loop.is_null() {
                return Err("Cannot start PipeWire".into());
            }
            let mut streams = Streams { library, thread_loop, context: null_mut(), core: null_mut(), streams: Vec::new() };
            let loop_ = (library.pw_thread_loop_get_loop)(thread_loop);
            streams.context = (library.pw_context_new)(loop_, null_mut(), 0);
            if streams.context.is_null() {
                return Err("Cannot start PipeWire".into());
            }
            (library.pw_thread_loop_lock)(thread_loop);
            // PipeWire closes the socket itself when the core goes away.
            let socket = fd.into_raw_fd();
            streams.core = (library.pw_context_connect_fd)(streams.context, socket, null_mut(), 0);
            if streams.core.is_null() {
                libc::close(socket);
                (library.pw_thread_loop_unlock)(thread_loop);
                return Err("The desktop's screen stream could not be opened".into());
            }
            for node in nodes {
                match Stream::new(library, streams.core, *node) {
                    Ok(stream) => streams.streams.push(stream),
                    Err(e) => {
                        (library.pw_thread_loop_unlock)(thread_loop);
                        return Err(e);
                    }
                }
            }
            (library.pw_thread_loop_unlock)(thread_loop);
            if (library.pw_thread_loop_start)(thread_loop) < 0 {
                return Err("Cannot start PipeWire".into());
            }
            Ok(streams)
        }
    }

    /// The size of a monitor's frames, once one has arrived.
    pub fn size(&self, index: usize) -> Option<(i32, i32)> {
        let frame = self.streams.get(index)?.data.frame.lock().ok()?;
        (frame.format.is_some() && frame.size.0 > 0 && !frame.pixels.is_null()).then_some(frame.size)
    }

    /// The pixels of an area of a monitor, from its latest frame.
    pub fn read(&self, index: usize, area: Area) -> Option<Pixels> {
        let frame = self.streams.get(index)?.data.frame.lock().ok()?;
        let decode = FORMATS.get(frame.format?)?.1;
        if frame.pixels.is_null() {
            return None;
        }
        // SAFETY: the buffer is held (PipeWire does not write into it meanwhile) and its
        // mapped memory is `length` bytes long.
        let data = unsafe { std::slice::from_raw_parts(frame.pixels, frame.length) };
        let (x0, y0, x1, y1) = area;
        let mut rgb = Vec::with_capacity(((x1 - x0).max(0) * (y1 - y0).max(0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let color = (x >= 0 && y >= 0 && x < frame.size.0 && y < frame.size.1)
                    .then(|| {
                        let at = y as usize * frame.stride + x as usize * 4;
                        data.get(at..at + 4).map(decode)
                    })
                    .flatten();
                rgb.push(color);
            }
        }
        Some(Pixels { area, rgb })
    }

    /// Whether every monitor has sent a frame.
    pub fn ready(&self) -> bool {
        (0..self.streams.len()).all(|i| self.size(i).is_some())
    }

    /// What a stream is doing (for the log and the tests): its state and its error.
    pub fn status(&self, index: usize) -> Option<(c_int, String)> {
        Some(self.streams.get(index)?.data.status.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }
}

impl Drop for Streams {
    fn drop(&mut self) {
        let library = self.library;
        // SAFETY: the thread loop is stopped first, so nothing runs while the stream, the
        // core and the context are destroyed, in that order.
        unsafe {
            (library.pw_thread_loop_stop)(self.thread_loop);
            for stream in self.streams.drain(..) {
                let mut frame = stream.data.frame.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(held) = frame.held.take() {
                    (library.pw_stream_queue_buffer)(stream.handle, held);
                }
                *stream.data.stream.lock().unwrap_or_else(|e| e.into_inner()) = null_mut();
                drop(frame);
                (library.pw_stream_destroy)(stream.handle);
            }
            if !self.core.is_null() {
                (library.pw_core_disconnect)(self.core);
            }
            if !self.context.is_null() {
                (library.pw_context_destroy)(self.context);
            }
            (library.pw_thread_loop_destroy)(self.thread_loop);
        }
    }
}

impl Stream {
    /// Follow one node of the session (the thread loop must be locked).
    unsafe fn new(library: &'static Library, core: Handle, node: u32) -> Result<Stream, String> {
        unsafe {
            // The node the desktop gave, by id (the property is how PipeWire wants it now).
            let properties = std::ffi::CString::new(format!("media.type=Video media.category=Capture media.role=Screen target.object={node}"))
                .map_err(|_| "Cannot name the screen stream".to_owned())?;
            let properties = (library.pw_properties_new_string)(properties.as_ptr());
            let handle = (library.pw_stream_new)(core, c"tessera eyedropper".as_ptr(), properties);
            if handle.is_null() {
                return Err("The desktop's screen stream could not be opened".into());
            }
            let data = Arc::new(StreamData {
                stream: Mutex::new(handle),
                frame: Mutex::new(Frame::default()),
                status: Mutex::new((0, String::new())),
                library,
            });
            let mut hook = Box::new(SpaHook::default());
            let events = &EVENTS;
            // The data outlives the listener: the stream is destroyed in `Streams::drop`.
            (library.pw_stream_add_listener)(handle, &mut *hook, events, Arc::as_ptr(&data) as *mut c_void);
            let formats = enum_formats();
            let params = [formats.as_ptr()];
            let flags = PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS | PW_STREAM_FLAG_DONT_RECONNECT;
            if (library.pw_stream_connect)(handle, PW_DIRECTION_INPUT, node, flags, params.as_ptr(), 1) < 0 {
                (library.pw_stream_destroy)(handle);
                return Err("The desktop's screen stream could not be opened".into());
            }
            Ok(Stream { handle, data, _hook: hook })
        }
    }
}

static EVENTS: StreamEvents = StreamEvents {
    version: 2,
    destroy: None,
    state_changed: Some(on_state_changed),
    control_info: None,
    io_changed: None,
    param_changed: Some(on_param_changed),
    add_buffer: None,
    remove_buffer: None,
    process: Some(on_process),
    drained: None,
    command: None,
    trigger_done: None,
};

/// The stream started, stopped or failed.
unsafe extern "C" fn on_state_changed(data: *mut c_void, _old: c_int, state: c_int, error: *const c_char) {
    // SAFETY: `data` is the stream's own data, alive as long as the stream listens.
    let Some(data) = (unsafe { (data as *const StreamData).as_ref() }) else { return };
    // SAFETY: PipeWire passes a string or nothing.
    let error = unsafe { error.as_ref().map(|_| CStr::from_ptr(error).to_string_lossy().into_owned()) };
    if let Some(error) = &error {
        log::warn!("the screen stream failed: {error}");
    }
    *data.status.lock().unwrap_or_else(|e| e.into_inner()) = (state, error.unwrap_or_default());
}

/// The format was agreed on: remember it and ask for buffers we can read.
unsafe extern "C" fn on_param_changed(data: *mut c_void, id: u32, param: *const u8) {
    const SPA_PARAM_FORMAT: u32 = 4;
    // SAFETY: `data` is the stream's own data, alive as long as the stream listens.
    let Some(data) = (unsafe { (data as *const StreamData).as_ref() }) else { return };
    if id != SPA_PARAM_FORMAT || param.is_null() {
        return;
    }
    // SAFETY: PipeWire passes a pod whose header says how long it is.
    let Some(pod) = (unsafe { Pod::read(param) }) else { return };
    let (Some(format), Some(size)) = (pod.property(0x20001).and_then(Pod::id), pod.property(0x20003).and_then(Pod::rectangle)) else { return };
    let format = FORMATS.iter().position(|(f, _)| *f == format);
    {
        let mut frame = data.frame.lock().unwrap_or_else(|e| e.into_inner());
        frame.format = format;
        frame.size = (size.0 as i32, size.1 as i32);
    }
    let stride = size.0 as i32 * 4;
    let buffers = buffer_params(stride, stride * size.1 as i32);
    let params = [buffers.as_ptr()];
    let stream = *data.stream.lock().unwrap_or_else(|e| e.into_inner());
    if !stream.is_null() {
        // SAFETY: called from the stream's own callback, on PipeWire's thread.
        unsafe { (data.library.pw_stream_update_params)(stream, params.as_ptr(), 1) };
    }
}

/// A frame arrived: hold it and give back the one before.
unsafe extern "C" fn on_process(data: *mut c_void) {
    // SAFETY: `data` is the stream's own data, alive as long as the stream listens.
    let Some(data) = (unsafe { (data as *const StreamData).as_ref() }) else { return };
    let stream = *data.stream.lock().unwrap_or_else(|e| e.into_inner());
    if stream.is_null() {
        return;
    }
    let library = data.library;
    // SAFETY: called by PipeWire on its own thread, with the stream alive.
    unsafe {
        let mut latest: *mut PwBuffer = null_mut();
        // Only the newest frame matters: the older ones go straight back.
        loop {
            let buffer = (library.pw_stream_dequeue_buffer)(stream);
            if buffer.is_null() {
                break;
            }
            if !latest.is_null() {
                (library.pw_stream_queue_buffer)(stream, latest);
            }
            latest = buffer;
        }
        if latest.is_null() {
            return;
        }
        let mut frame = data.frame.lock().unwrap_or_else(|e| e.into_inner());
        let (mut pixels, mut length, mut stride) = (null(), 0, 0);
        if let Some(buffer) = (*latest).buffer.as_ref()
            && buffer.n_datas > 0
            && let Some(first) = buffer.datas.as_ref()
            && !first.data.is_null()
            && let Some(chunk) = first.chunk.as_ref()
            && chunk.stride > 0
        {
            pixels = (first.data as *const u8).add(chunk.offset as usize % first.max_size.max(1) as usize);
            length = (first.max_size as usize).saturating_sub(chunk.offset as usize % first.max_size.max(1) as usize);
            stride = chunk.stride as usize;
        }
        if pixels.is_null() {
            // A frame without pixels (it can happen while starting): keep the last one.
            (library.pw_stream_queue_buffer)(stream, latest);
            return;
        }
        if let Some(previous) = frame.held.replace(latest) {
            (library.pw_stream_queue_buffer)(stream, previous);
        }
        frame.pixels = pixels;
        frame.length = length;
        frame.stride = stride;
    }
}

// ---------------------------------------------------------------- SPA pods

/// A pod being written (the format of PipeWire's parameters).
#[derive(Default)]
struct PodWriter {
    bytes: Vec<u8>,
}

impl PodWriter {
    fn raw(&mut self, value: u32) {
        self.bytes.extend(value.to_ne_bytes());
    }

    /// A pod with a body of `size` bytes, padded to eight.
    fn pod(&mut self, kind: u32, body: &[u8]) {
        self.raw(body.len() as u32);
        self.raw(kind);
        self.bytes.extend(body);
        while self.bytes.len() % 8 != 0 {
            self.bytes.push(0);
        }
    }

    fn id(&mut self, key: u32, value: u32) {
        self.raw(key);
        self.raw(0);
        self.pod(TYPE_ID, &value.to_ne_bytes());
    }

    fn int(&mut self, key: u32, value: i32) {
        self.raw(key);
        self.raw(0);
        self.pod(TYPE_INT, &value.to_ne_bytes());
    }

    /// A property whose value is one of several (the first is preferred).
    fn choice(&mut self, key: u32, choice: u32, kind: u32, values: &[[u8; 8]], value_size: usize) {
        self.raw(key);
        self.raw(0);
        let mut body = Vec::new();
        body.extend(choice.to_ne_bytes());
        body.extend(0u32.to_ne_bytes());
        body.extend((value_size as u32).to_ne_bytes());
        body.extend(kind.to_ne_bytes());
        for value in values {
            body.extend(&value[..value_size]);
        }
        self.pod(TYPE_CHOICE, &body);
    }

    /// Wrap what `fill` writes in an object pod.
    fn object(kind: u32, id: u32, fill: impl FnOnce(&mut PodWriter)) -> Vec<u8> {
        let mut inner = PodWriter::default();
        inner.raw(kind);
        inner.raw(id);
        fill(&mut inner);
        let mut out = PodWriter::default();
        out.pod(TYPE_OBJECT, &inner.bytes);
        out.bytes
    }
}

const TYPE_ID: u32 = 3;
const TYPE_INT: u32 = 4;
const TYPE_RECTANGLE: u32 = 10;
const TYPE_FRACTION: u32 = 11;
const TYPE_CHOICE: u32 = 19;
const TYPE_OBJECT: u32 = 15;
const OBJECT_FORMAT: u32 = 0x40003;
const OBJECT_PARAM_BUFFERS: u32 = 0x40004;
const CHOICE_RANGE: u32 = 1;
const CHOICE_ENUM: u32 = 3;
const CHOICE_FLAGS: u32 = 4;

/// The video formats and sizes the eyedropper takes.
fn enum_formats() -> Vec<u8> {
    let rectangle = |w: u32, h: u32| {
        let mut v = [0u8; 8];
        v[..4].copy_from_slice(&w.to_ne_bytes());
        v[4..].copy_from_slice(&h.to_ne_bytes());
        v
    };
    PodWriter::object(OBJECT_FORMAT, 3 /* SPA_PARAM_EnumFormat */, |pod| {
        pod.id(1, 2); // media type: video
        pod.id(2, 1); // media subtype: raw
        // The first value is the preferred one, and it is among the choices as well.
        let formats: Vec<[u8; 8]> = std::iter::once(&FORMATS[0])
            .chain(FORMATS.iter())
            .map(|(f, _)| {
                let mut v = [0u8; 8];
                v[..4].copy_from_slice(&f.to_ne_bytes());
                v
            })
            .collect();
        pod.choice(0x20001, CHOICE_ENUM, TYPE_ID, &formats, 4);
        pod.choice(0x20003, CHOICE_RANGE, TYPE_RECTANGLE, &[rectangle(1920, 1080), rectangle(1, 1), rectangle(16384, 16384)], 8);
        let fraction = |n: u32, d: u32| {
            let mut v = [0u8; 8];
            v[..4].copy_from_slice(&n.to_ne_bytes());
            v[4..].copy_from_slice(&d.to_ne_bytes());
            v
        };
        pod.choice(0x20004, CHOICE_RANGE, TYPE_FRACTION, &[fraction(60, 1), fraction(0, 1), fraction(1000, 1)], 8);
    })
}

/// Buffers the eyedropper can read: plain memory, not the graphics card's.
fn buffer_params(stride: i32, size: i32) -> Vec<u8> {
    PodWriter::object(OBJECT_PARAM_BUFFERS, 5 /* SPA_PARAM_Buffers */, |pod| {
        let int = |v: i32| {
            let mut b = [0u8; 8];
            b[..4].copy_from_slice(&v.to_ne_bytes());
            b
        };
        pod.choice(1, CHOICE_RANGE, TYPE_INT, &[int(4), int(2), int(16)], 4);
        pod.int(2, 1); // one block of memory per buffer
        pod.int(3, size);
        pod.int(4, stride);
        pod.int(5, 16);
        // Memory the CPU can read: a pointer or a file descriptor to map.
        pod.choice(6, CHOICE_FLAGS, TYPE_INT, &[int((1 << 1) | (1 << 2))], 4);
    })
}

/// A pod being read.
struct Pod<'a> {
    kind: u32,
    body: &'a [u8],
}

impl<'a> Pod<'a> {
    /// The pod a pointer from PipeWire points at.
    ///
    /// # Safety
    ///
    /// `at` must point at a pod: a size and a type, then that many bytes.
    unsafe fn read(at: *const u8) -> Option<Pod<'a>> {
        if at.is_null() {
            return None;
        }
        // SAFETY: the caller promises a pod, which starts with its size.
        unsafe {
            let size = u32::from_ne_bytes(*(at as *const [u8; 4])) as usize;
            let kind = u32::from_ne_bytes(*(at.add(4) as *const [u8; 4]));
            (size <= 1 << 20).then(|| Pod { kind, body: std::slice::from_raw_parts(at.add(8), size) })
        }
    }

    fn word(&self, at: usize) -> Option<u32> {
        Some(u32::from_ne_bytes(self.body.get(at..at + 4)?.try_into().ok()?))
    }

    /// The value of a property of an object pod, whatever wraps it.
    fn property(&self, key: u32) -> Option<Pod<'_>> {
        if self.kind != TYPE_OBJECT {
            return None;
        }
        let mut at = 8;
        while at + 16 <= self.body.len() {
            let this = self.word(at)?;
            let size = self.word(at + 8)? as usize;
            let kind = self.word(at + 12)?;
            let body = self.body.get(at + 16..at + 16 + size)?;
            if this == key {
                let pod = Pod { kind, body };
                return Some(pod.unwrapped());
            }
            at += 16 + size.next_multiple_of(8);
        }
        None
    }

    /// The value itself, if this pod holds a choice of values.
    fn unwrapped(self) -> Pod<'a> {
        if self.kind != TYPE_CHOICE || self.body.len() < 16 {
            return self;
        }
        let word = |at: usize| u32::from_ne_bytes(self.body[at..at + 4].try_into().unwrap_or_default());
        let (size, kind) = (word(8) as usize, word(12));
        match self.body.get(16..16 + size) {
            Some(body) => Pod { kind, body },
            None => self,
        }
    }

    fn id(self) -> Option<u32> {
        (self.kind == TYPE_ID || self.kind == TYPE_INT).then(|| self.word(0))?
    }

    fn rectangle(self) -> Option<(u32, u32)> {
        (self.kind == TYPE_RECTANGLE).then(|| Some((self.word(0)?, self.word(4)?)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What PipeWire's own builder makes of the same parameters (from a C program
    /// against the installed headers), to the byte.
    const FORMATS_POD: &str = "f00000000f0000000300040003000000010000000000000004000000030000000200000000000000020000000000000004000000030000000100000000000000010002000000000034000000130000000300000000000000040000000300000008000000080000000c000000070000000b000000090000000d0000000a0000000e00000000000000030002000000000028000000130000000100000000000000080000000a000000800700003804000001000000010000000040000000400000040002000000000028000000130000000100000000000000080000000b0000003c000000010000000000000001000000e803000001000000";
    const BUFFERS_POD: &str = "c00000000f000000040004000500000001000000000000001c0000001300000001000000000000000400000004000000040000000200000010000000000000000200000000000000040000000400000001000000000000000300000000000000040000000400000000000c000000000004000000000000000400000004000000000400000000000005000000000000000400000004000000100000000000000006000000000000001400000013000000040000000000000004000000040000000600000000000000";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_parameters_are_the_ones_pipewire_builds() {
        assert_eq!(hex(&enum_formats()), FORMATS_POD, "the formats the eyedropper takes");
        assert_eq!(hex(&buffer_params(1024, 1024 * 768)), BUFFERS_POD, "the buffers it can read");
    }

    #[test]
    fn formats_are_written_as_pods() {
        let pod = enum_formats();
        // A pod: its size, its type, and a body that long, padded to eight.
        let size = u32::from_ne_bytes(pod[..4].try_into().unwrap()) as usize;
        assert_eq!(u32::from_ne_bytes(pod[4..8].try_into().unwrap()), TYPE_OBJECT);
        assert_eq!(pod.len(), 8 + size.next_multiple_of(8));
        // SAFETY: a pod just written.
        let read = unsafe { Pod::read(pod.as_ptr()) }.expect("pod");
        assert_eq!(read.property(1).and_then(Pod::id), Some(2), "video");
        assert_eq!(read.property(2).and_then(Pod::id), Some(1), "raw");
        assert_eq!(read.property(0x20001).and_then(Pod::id), Some(FORMATS[0].0), "the first format");
        assert_eq!(read.property(0x20003).and_then(Pod::rectangle), Some((1920, 1080)), "the preferred size");
        assert_eq!(read.property(0x30001).map(|p| p.kind), None, "no such property");
    }

    #[test]
    fn buffer_parameters_ask_for_memory_to_read() {
        let pod = buffer_params(1024, 1024 * 768);
        // SAFETY: a pod just written.
        let read = unsafe { Pod::read(pod.as_ptr()) }.expect("pod");
        assert_eq!(read.property(3).and_then(Pod::id), Some(1024 * 768));
        assert_eq!(read.property(4).and_then(Pod::id), Some(1024));
        assert_eq!(read.property(6).and_then(Pod::id), Some((1 << 1) | (1 << 2)));
    }

    #[test]
    fn broken_pods_are_not_read() {
        assert!(unsafe { Pod::read(std::ptr::null()) }.is_none());
        let pod = Pod { kind: TYPE_OBJECT, body: &[0; 12] };
        assert!(pod.property(1).is_none());
        // A property that says it is longer than the pod.
        let mut body = vec![0u8; 8];
        body.extend(1u32.to_ne_bytes());
        body.extend(0u32.to_ne_bytes());
        body.extend(u32::MAX.to_ne_bytes());
        body.extend(TYPE_ID.to_ne_bytes());
        assert!(Pod { kind: TYPE_OBJECT, body: &body }.property(1).is_none());
    }
}

/// These read a real PipeWire stream: a test picture published by GStreamer, which the
/// desktop's screen cast looks exactly like. Run them with a PipeWire and that picture:
/// `pipewire & gst-launch-1.0 videotestsrc pattern=red ! video/x-raw,format=BGRx,width=320,height=240 ! pipewiresink &`
/// then `cargo test -p tessera -- --ignored eyedropper::wayland::pipewire`.
#[cfg(test)]
mod live_tests {
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use super::*;

    /// The node of a video stream published to PipeWire, from `pw-dump`.
    fn published_video_node() -> Option<u32> {
        let dump = std::process::Command::new("pw-dump").output().ok()?;
        let objects: serde_json::Value = serde_json::from_slice(&dump.stdout).ok()?;
        objects.as_array()?.iter().find_map(|object| {
            let properties = object.get("info")?.get("props")?;
            let class = properties.get("media.class")?.as_str()?;
            (class == "Stream/Output/Video").then(|| object.get("id")?.as_u64().map(|id| id as u32))?
        })
    }

    fn socket() -> Option<std::os::fd::OwnedFd> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        let path = std::path::Path::new(&runtime).join(std::env::var("PIPEWIRE_REMOTE").unwrap_or_else(|_| "pipewire-0".into()));
        Some(UnixStream::connect(path).ok()?.into())
    }

    #[test]
    #[ignore = "needs a PipeWire with a video stream"]
    fn a_stream_of_the_screen_is_read_pixel_by_pixel() {
        let (Some(socket), Some(node)) = (socket(), published_video_node()) else {
            eprintln!("no PipeWire with a published video stream: test skipped");
            return;
        };
        let streams = Streams::open(socket, &[node]).expect("streams");
        let started = Instant::now();
        while !streams.ready() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let size = streams.size(0).unwrap_or_else(|| panic!("no frame; the stream is {:?}", streams.status(0)));
        assert!(size.0 > 0 && size.1 > 0, "{size:?}");
        // The test picture is one color: every pixel of it is that color.
        let pixels = streams.read(0, (0, 0, size.0.min(64), size.1.min(64))).expect("pixels");
        let first = pixels.get((0, 0)).expect("a pixel");
        println!("stream {node}: {}x{} pixels, the first is {first:?}", size.0, size.1);
        for y in 0..size.1.min(64) {
            for x in 0..size.0.min(64) {
                assert_eq!(pixels.get((x, y)), Some(first), "every pixel of the picture is the same");
            }
        }
        assert!(first.iter().any(|c| *c > 32), "the picture is not black: {first:?}");
        // Outside the area asked for: nothing.
        assert_eq!(pixels.get((-1, 0)), None);
        assert_eq!(pixels.get((size.0, 0)), None);
        // A second read follows the stream (the frames keep coming).
        let again = streams.read(0, (0, 0, 2, 2)).expect("pixels");
        assert_eq!(again.get((1, 1)), Some(first));
    }
}
