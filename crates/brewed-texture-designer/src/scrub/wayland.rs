//! Dragging a value on Wayland, where a program may not move the pointer.
//!
//! Instead the compositor is asked to lock the pointer where it is and to say how far the
//! mouse travelled (the pointer-constraints and relative-pointer protocols, which every
//! desktop that runs games supports). The pointer never reaches the edge of the screen,
//! because it never moves at all.
//!
//! The lock is put on the app's own window, through the connection the app already has:
//! the surface and the display come from the window handle, and the objects made here
//! live on a queue of their own, read once per frame while the drag lasts.

use std::os::fd::AsFd;
use std::ptr::NonNull;
use std::sync::Mutex;

use wayland_backend::sys::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::ZwpLockedPointerV1;
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::{Lifetime, ZwpPointerConstraintsV1};
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1;
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::{self, ZwpRelativePointerV1};

/// The window of the app, as the Wayland connection it belongs to sees it: the display and
/// the surface, kept as addresses because they are not ours to own.
static WINDOW: Mutex<Option<(usize, usize)>> = Mutex::new(None);

/// Remember the app's window, so a drag can lock the pointer over it.
pub fn remember(display: NonNull<std::ffi::c_void>, surface: NonNull<std::ffi::c_void>) {
    let mut slot = WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    *slot = Some((display.as_ptr() as usize, surface.as_ptr() as usize));
}

/// What the compositor tells us while the pointer is locked.
#[derive(Default)]
struct State {
    /// How far the mouse has travelled sideways since the last frame, in the window's
    /// pixels.
    dx: f64,
    /// Whether the seat has a pointer at all.
    pointer: bool,
}

/// The pointer, held still until this is dropped.
pub struct Locked {
    conn: Connection,
    queue: EventQueue<State>,
    state: State,
    lock: ZwpLockedPointerV1,
    relative: ZwpRelativePointerV1,
    pointer: WlPointer,
}

impl Locked {
    /// Lock the pointer over the app's window. `None` when this is not a Wayland session,
    /// or the compositor offers neither protocol, in which case the drag stays a plain one.
    pub fn hold() -> Option<Locked> {
        let (display, surface) = (*WINDOW.lock().unwrap_or_else(|e| e.into_inner()))?;
        // SAFETY: the display belongs to the app's own connection and outlives the drag;
        // the backend only borrows it.
        let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();

        let constraints: ZwpPointerConstraintsV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let relative_manager: ZwpRelativePointerManagerV1 = globals.bind(&qh, 1..=1, ()).ok()?;
        let seat: WlSeat = globals.bind(&qh, 1..=9, ()).ok()?;
        let mut state = State::default();
        // The capabilities of the seat arrive first: asking a seat with no pointer for one
        // is a protocol error, which would take the app's connection down with it.
        queue.roundtrip(&mut state).ok()?;
        if !state.pointer {
            seat.release();
            return None;
        }

        // The surface is the app's own; it is only named here, never driven.
        // SAFETY: the pointer comes from the window handle of the live window, and the
        // object belongs to the same display as the backend above.
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface as *mut _) }.ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;

        let pointer = seat.get_pointer(&qh, ());
        let relative = relative_manager.get_relative_pointer(&pointer, &qh, ());
        let lock = constraints.lock_pointer(&surface, &pointer, None, Lifetime::Oneshot, &qh, ());
        queue.roundtrip(&mut state).ok()?;
        Some(Locked { conn, queue, state, lock, relative, pointer })
    }

    /// How far the mouse has travelled sideways since the last call, in the window's
    /// pixels. Called once a frame while the drag lasts.
    pub fn travelled(&mut self) -> f32 {
        let _ = self.queue.flush();
        // Whatever has arrived, without waiting for more: the frame must not stall.
        if readable(&self.conn)
            && let Some(guard) = self.queue.prepare_read()
        {
            let _ = guard.read();
        }
        let _ = self.queue.dispatch_pending(&mut self.state);
        std::mem::take(&mut self.state.dx) as f32
    }
}

/// Is there anything to read from the compositor right now?
fn readable(conn: &Connection) -> bool {
    use rustix::event::{PollFd, PollFlags, poll};
    let fd = conn.as_fd();
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    let now = rustix::event::Timespec { tv_sec: 0, tv_nsec: 0 };
    matches!(poll(&mut fds, Some(&now)), Ok(n) if n > 0)
}

impl Drop for Locked {
    fn drop(&mut self) {
        // The pointer is free again, right where the drag started.
        self.lock.destroy();
        self.relative.destroy();
        self.pointer.release();
        let _ = self.queue.flush();
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, .. } = event
            && dx.is_finite()
        {
            state.dx += dx;
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(state: &mut Self, _: &WlSeat, event: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            state.pointer = capabilities.into_result().is_ok_and(|c| c.contains(wl_seat::Capability::Pointer));
        }
    }
}

/// Nothing of the rest is listened to: the lock says when it takes and breaks, the pointer
/// says nothing at all while it is locked.
macro_rules! ignore {
    ($($proxy:ty),* $(,)?) => {
        $(impl Dispatch<$proxy, ()> for State {
            fn event(
                _: &mut Self,
                _: &$proxy,
                _: <$proxy as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        })*
    };
}

ignore!(WlPointer, ZwpLockedPointerV1, ZwpPointerConstraintsV1, ZwpRelativePointerManagerV1);

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Locking the pointer needs a compositor and a window the pointer is over, so these tests
/// make both: a surface of their own, and a virtual pointer to move over it. They run in a
/// compositor of their own (`~/sway-test.sh`, see the README), like the eyedropper's.
#[cfg(test)]
mod live_tests {
    use std::time::Duration;

    use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
    use smithay_client_toolkit::output::{OutputHandler, OutputState};
    use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
    use smithay_client_toolkit::shell::WaylandSurface;
    use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
    use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
    use smithay_client_toolkit::shm::{Shm, ShmHandler};
    use smithay_client_toolkit::{delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm, registry_handlers};
    use wayland_client::delegate_noop;
    use wayland_client::protocol::wl_output::{Transform, WlOutput};
    use wayland_client::protocol::wl_shm::Format;
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
    use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

    use super::*;

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

    /// A window of the test's own, filling the screen, with a pointer to move over it.
    struct Window {
        queue: EventQueue<Test>,
        state: Test,
        layer: LayerSurface,
        _buffer: Buffer,
        pointer: ZwlrVirtualPointerV1,
        extent: (u32, u32),
        time: u32,
    }

    impl Window {
        fn open() -> Option<Window> {
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
            let (w, h) = state.outputs.info(&output)?.logical_size?;
            let surface = compositor.create_surface(&qh);
            let layer = layers.create_layer_surface(&qh, surface, Layer::Top, Some("scrub test"), Some(&output));
            layer.set_anchor(Anchor::all());
            layer.set_size(w as u32, h as u32);
            layer.commit();
            while !state.configured {
                queue.blocking_dispatch(&mut state).ok()?;
            }
            let mut pool = SlotPool::new((w * h * 4) as usize, &state.shm).ok()?;
            let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Xrgb8888).ok()?;
            canvas.fill(40);
            buffer.attach_to(layer.wl_surface()).ok()?;
            layer.wl_surface().damage_buffer(0, 0, w, h);
            layer.commit();
            let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
            queue.roundtrip(&mut state).ok()?;
            let mut window = Window { queue, state, layer, _buffer: buffer, pointer, extent: (w as u32, h as u32), time: 0 };
            // The pointer has to be over the window for the compositor to lock it there.
            window.move_to((w as u32 / 2, h as u32 / 2));
            std::thread::sleep(Duration::from_millis(100));
            window.flush();
            Some(window)
        }

        fn flush(&mut self) {
            self.queue.roundtrip(&mut self.state).expect("the test compositor");
            std::thread::sleep(Duration::from_millis(5));
        }

        fn move_to(&mut self, (x, y): (u32, u32)) {
            self.time += 5;
            self.pointer.motion_absolute(self.time, x, y, self.extent.0, self.extent.1);
            self.pointer.frame();
            self.flush();
        }

        /// Move the mouse itself, without saying where it lands: what a locked pointer has
        /// to work from.
        fn move_by(&mut self, dx: f64, dy: f64) {
            self.time += 5;
            self.pointer.motion(self.time, dx, dy);
            self.pointer.frame();
            self.flush();
        }

        /// Tell a drag where this window is, as the app does with its own.
        fn remember(&self) {
            let backend = self.layer.wl_surface().backend().upgrade().expect("the connection");
            let display = NonNull::new(backend.display_ptr().cast::<std::ffi::c_void>()).expect("a display");
            let surface = self.layer.wl_surface().id().as_ptr();
            super::remember(display, NonNull::new(surface.cast()).expect("a live surface"));
        }
    }

    /// The pointer is locked where it is and the mouse goes on saying how far it has
    /// travelled, as far as it likes: this is what lets a value be dragged past the edge of
    /// the screen on Wayland.
    #[test]
    #[ignore = "needs a compositor of its own"]
    fn the_locked_pointer_says_how_far_the_mouse_went() {
        let Some(mut window) = Window::open() else {
            eprintln!("no Wayland compositor for the test: skipped");
            return;
        };
        window.remember();
        let mut locked = Locked::hold().expect("the compositor locks the pointer");
        assert_eq!(locked.travelled(), 0.0, "nothing has moved yet");

        // The mouse is pushed the same way again and again, far further than any screen is
        // wide: every push counts, because the pointer itself never moves.
        let mut total = 0.0;
        for _ in 0..40 {
            window.move_by(200.0, 0.0);
            total += locked.travelled();
        }
        assert!(total > 7000.0, "{total} is less than the mouse travelled");
        // And the same way back.
        for _ in 0..40 {
            window.move_by(-200.0, 0.0);
            total += locked.travelled();
        }
        assert!(total.abs() < 400.0, "{total} is not back where it started");

        // Let go: the pointer is the user's again, and it is still where they left it.
        drop(locked);
        window.flush();
    }
}
