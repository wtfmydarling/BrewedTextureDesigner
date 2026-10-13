use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use sctk::data_device_manager::data_device::{DataDevice, DataDeviceData, DataDeviceHandler};
use sctk::data_device_manager::data_offer::{DataOfferError, DataOfferHandler, DragOffer};
use sctk::data_device_manager::data_source::{CopyPasteSource, DataSourceHandler};
use sctk::data_device_manager::{DataDeviceManagerState, WritePipe};
use sctk::primary_selection::PrimarySelectionManagerState;
use sctk::primary_selection::device::{PrimarySelectionDevice, PrimarySelectionDeviceHandler};
use sctk::primary_selection::selection::{PrimarySelectionSource, PrimarySelectionSourceHandler};
use sctk::registry::{ProvidesRegistryState, RegistryState};
use sctk::seat::pointer::{PointerData, PointerEvent, PointerEventKind, PointerHandler};
use sctk::seat::{Capability, SeatHandler, SeatState};
use sctk::{
    delegate_data_device, delegate_pointer, delegate_primary_selection, delegate_registry,
    delegate_seat, registry_handlers,
};

use sctk::reexports::calloop::{LoopHandle, PostAction};
use sctk::reexports::client::globals::GlobalList;
use sctk::reexports::client::protocol::wl_data_device::WlDataDevice;
use sctk::reexports::client::protocol::wl_data_device_manager::DndAction;
use sctk::reexports::client::protocol::wl_data_source::WlDataSource;
use sctk::reexports::client::protocol::wl_keyboard::WlKeyboard;
use sctk::reexports::client::protocol::wl_pointer::WlPointer;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use sctk::reexports::protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
    zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
};
use wayland_backend::client::ObjectId;

use crate::dropped::{DroppedFiles, MAX_URI_LIST, URI_LIST};
use crate::mime::{ALLOWED_MIME_TYPES, MimeType, normalize_to_lf};

pub struct State {
    pub primary_selection_manager_state: Option<PrimarySelectionManagerState>,
    pub data_device_manager_state: Option<DataDeviceManagerState>,
    pub reply_tx: Sender<Result<String>>,
    pub exit: bool,

    registry_state: RegistryState,
    seat_state: SeatState,

    seats: HashMap<ObjectId, ClipboardSeatState>,
    /// The latest seat which got an event.
    latest_seat: Option<ObjectId>,

    loop_handle: LoopHandle<'static, Self>,
    queue_handle: QueueHandle<Self>,

    primary_sources: Vec<PrimarySelectionSource>,
    primary_selection_content: Rc<[u8]>,

    data_sources: Vec<CopyPasteSource>,
    data_selection_content: Rc<[u8]>,

    /// The files dragged over a surface now (Brewed Texture Designer's addition, as all that
    /// names a drag below: see `dropped.rs`).
    drag: Option<Drag>,
    /// How many drags of files there were: the next one's `Drag::id`.
    drags: u64,
}

impl State {
    #[must_use]
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<Self>,
        loop_handle: LoopHandle<'static, Self>,
        reply_tx: Sender<Result<String>>,
    ) -> Option<Self> {
        // NOTE: while it's mutable, it's not part of the hash compute.
        #[allow(clippy::mutable_key_type)]
        let mut seats = HashMap::new();

        let data_device_manager_state = DataDeviceManagerState::bind(globals, queue_handle).ok();
        let primary_selection_manager_state =
            PrimarySelectionManagerState::bind(globals, queue_handle).ok();

        // When both globals are not available nothing could be done.
        if data_device_manager_state.is_none() && primary_selection_manager_state.is_none() {
            return None;
        }

        let seat_state = SeatState::new(globals, queue_handle);
        for seat in seat_state.seats() {
            seats.insert(seat.id(), Default::default());
        }

        Some(Self {
            registry_state: RegistryState::new(globals),
            primary_selection_content: Rc::from([]),
            data_selection_content: Rc::from([]),
            queue_handle: queue_handle.clone(),
            primary_selection_manager_state,
            primary_sources: Vec::new(),
            data_device_manager_state,
            data_sources: Vec::new(),
            latest_seat: None,
            loop_handle,
            drag: None,
            drags: 0,
            exit: false,
            seat_state,
            reply_tx,
            seats,
        })
    }

    /// Store selection for the given target.
    ///
    /// Selection source is only created when `Some(())` is returned.
    pub fn store_selection(&mut self, ty: SelectionTarget, contents: String) -> Option<()> {
        let latest = self.latest_seat.as_ref()?;
        let seat = self.seats.get_mut(latest)?;

        if !seat.has_focus {
            return None;
        }

        let contents = Rc::from(contents.into_bytes());

        match ty {
            SelectionTarget::Clipboard => {
                let mgr = self.data_device_manager_state.as_ref()?;
                self.data_selection_content = contents;
                let source =
                    mgr.create_copy_paste_source(&self.queue_handle, ALLOWED_MIME_TYPES.iter());
                source.set_selection(seat.data_device.as_ref().unwrap(), seat.latest_serial);
                self.data_sources.push(source);
            },
            SelectionTarget::Primary => {
                let mgr = self.primary_selection_manager_state.as_ref()?;
                self.primary_selection_content = contents;
                let source =
                    mgr.create_selection_source(&self.queue_handle, ALLOWED_MIME_TYPES.iter());
                source.set_selection(seat.primary_device.as_ref().unwrap(), seat.latest_serial);
                self.primary_sources.push(source);
            },
        }

        Some(())
    }

    /// Load selection for the given target.
    pub fn load_selection(&mut self, ty: SelectionTarget) -> Result<()> {
        let latest = self
            .latest_seat
            .as_ref()
            .ok_or_else(|| Error::other("no events received on any seat"))?;
        let seat = self.seats.get_mut(latest).ok_or_else(|| Error::other("active seat lost"))?;

        if !seat.has_focus {
            return Err(Error::other("client doesn't have focus"));
        }

        let (read_pipe, mime_type) = match ty {
            SelectionTarget::Clipboard => {
                let selection = seat
                    .data_device
                    .as_ref()
                    .and_then(|data| data.data().selection_offer())
                    .ok_or_else(|| Error::other("selection is empty"))?;

                let mime_type =
                    selection.with_mime_types(MimeType::find_allowed).ok_or_else(|| {
                        Error::new(ErrorKind::NotFound, "supported mime-type is not found")
                    })?;

                (
                    selection.receive(mime_type.to_string()).map_err(|err| match err {
                        DataOfferError::InvalidReceive => Error::other("offer is not ready yet"),
                        DataOfferError::Io(err) => err,
                    })?,
                    mime_type,
                )
            },
            SelectionTarget::Primary => {
                let selection = seat
                    .primary_device
                    .as_ref()
                    .and_then(|data| data.data().selection_offer())
                    .ok_or_else(|| Error::other("selection is empty"))?;

                let mime_type =
                    selection.with_mime_types(MimeType::find_allowed).ok_or_else(|| {
                        Error::new(ErrorKind::NotFound, "supported mime-type is not found")
                    })?;

                (selection.receive(mime_type.to_string())?, mime_type)
            },
        };

        // Mark FD as non-blocking so we won't block ourselves.
        set_non_blocking(read_pipe.as_raw_fd())?;

        let mut reader_buffer = [0; 4096];
        let mut content = Vec::new();
        let _ = self.loop_handle.insert_source(read_pipe, move |_, file, state| {
            let file = unsafe { file.get_mut() };
            loop {
                match file.read(&mut reader_buffer) {
                    Ok(0) => {
                        let utf8 = String::from_utf8_lossy(&content);
                        let content = match utf8 {
                            Cow::Borrowed(_) => {
                                // Don't clone the read data.
                                let mut to_send = Vec::new();
                                mem::swap(&mut content, &mut to_send);
                                String::from_utf8(to_send).unwrap()
                            },
                            Cow::Owned(content) => content,
                        };

                        // Post-process the content according to mime type.
                        let content = match mime_type {
                            MimeType::TextPlainUtf8 | MimeType::TextPlain => {
                                normalize_to_lf(content)
                            },
                            MimeType::Utf8String => content,
                        };

                        let _ = state.reply_tx.send(Ok(content));
                        break PostAction::Remove;
                    },
                    Ok(n) => content.extend_from_slice(&reader_buffer[..n]),
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break PostAction::Continue,
                    Err(err) => {
                        let _ = state.reply_tx.send(Err(err));
                        break PostAction::Remove;
                    },
                };
            }
        });

        Ok(())
    }

    /// Read the `text/uri-list` of the drag `id`, without blocking the loop.
    fn read_dragged_list(&mut self, offer: &DragOffer, id: u64) -> Result<()> {
        let read_pipe = offer.receive(URI_LIST.into())?;

        // Mark FD as non-blocking so we won't block ourselves.
        set_non_blocking(read_pipe.as_raw_fd())?;

        let mut reader_buffer = [0; 4096];
        let mut list = Vec::new();
        self.loop_handle
            .insert_source(read_pipe, move |_, file, state| {
                let file = unsafe { file.get_mut() };
                loop {
                    // The drag left, or another came: nobody waits for this list.
                    if state.drag.as_ref().is_none_or(|drag| drag.id != id) {
                        break PostAction::Remove;
                    }
                    match file.read(&mut reader_buffer) {
                        Ok(0) => {
                            state.dragged_list_read(id, mem::take(&mut list));
                            break PostAction::Remove;
                        },
                        Ok(n) if list.len() + n > MAX_URI_LIST => {
                            state.dragged_list_read(id, Vec::new());
                            break PostAction::Remove;
                        },
                        Ok(n) => list.extend_from_slice(&reader_buffer[..n]),
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            break PostAction::Continue;
                        },
                        Err(err) if err.kind() == ErrorKind::Interrupted => (),
                        Err(_) => {
                            state.dragged_list_read(id, Vec::new());
                            break PostAction::Remove;
                        },
                    }
                }
            })
            .map(|_| ())
            .map_err(|_| Error::other("the list of what is dragged can't be watched"))
    }

    /// The list of the drag `id` is read to its end (empty: it could not be).
    fn dragged_list_read(&mut self, id: u64, list: Vec<u8>) {
        let Some(drag) = self.drag.as_mut().filter(|drag| drag.id == id) else { return };
        drag.list = Some(list);
        if drag.dropped {
            self.say_dropped();
        }
    }

    /// The drag is dropped and its list read: the source is told it is done with, and the
    /// program what was dropped.
    fn say_dropped(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let list = drag.list.unwrap_or_default();
        if let Some(offer) = drag_offer(&drag.data_device) {
            if !list.is_empty() && offer.selected_action == DndAction::Copy {
                offer.finish();
            }
            offer.destroy();
        }
        if !list.is_empty() {
            crate::dropped::say(DroppedFiles {
                surface: drag.surface,
                x: drag.x,
                y: drag.y,
                uri_list: String::from_utf8_lossy(&list).into_owned(),
            });
        }
    }

    fn send_request(&mut self, ty: SelectionTarget, write_pipe: WritePipe, mime: String) {
        // We can only send strings, so don't do anything with the mime-type.
        if MimeType::find_allowed(&[mime]).is_none() {
            return;
        }

        // Mark FD as non-blocking so we won't block ourselves.
        if set_non_blocking(write_pipe.as_raw_fd()).is_err() {
            return;
        }

        // Don't access the content on the state directly, since it could change during
        // the send.
        let contents = match ty {
            SelectionTarget::Clipboard => self.data_selection_content.clone(),
            SelectionTarget::Primary => self.primary_selection_content.clone(),
        };

        let mut written = 0;
        let _ = self.loop_handle.insert_source(write_pipe, move |_, file, _| {
            let file = unsafe { file.get_mut() };
            loop {
                match file.write(&contents[written..]) {
                    Ok(n) if written + n == contents.len() => {
                        written += n;
                        break PostAction::Remove;
                    },
                    Ok(n) => written += n,
                    Err(err) if err.kind() == ErrorKind::WouldBlock => break PostAction::Continue,
                    Err(_) => break PostAction::Remove,
                }
            }
        });
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        self.seats.insert(seat.id(), Default::default());
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        let seat_state = self.seats.get_mut(&seat.id()).unwrap();

        match capability {
            Capability::Keyboard => {
                seat_state.keyboard = Some(seat.get_keyboard(qh, seat.id()));

                // Selection sources are tied to the keyboard, so add/remove decives
                // when we gain/loss capability.

                if seat_state.data_device.is_none() && self.data_device_manager_state.is_some() {
                    seat_state.data_device = self
                        .data_device_manager_state
                        .as_ref()
                        .map(|mgr| mgr.get_data_device(qh, &seat));
                }

                if seat_state.primary_device.is_none()
                    && self.primary_selection_manager_state.is_some()
                {
                    seat_state.primary_device = self
                        .primary_selection_manager_state
                        .as_ref()
                        .map(|mgr| mgr.get_selection_device(qh, &seat));
                }
            },
            Capability::Pointer => {
                seat_state.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            },
            _ => (),
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        let seat_state = self.seats.get_mut(&seat.id()).unwrap();
        match capability {
            Capability::Keyboard => {
                seat_state.data_device = None;
                seat_state.primary_device = None;

                if let Some(keyboard) = seat_state.keyboard.take() {
                    if keyboard.version() >= 3 {
                        keyboard.release()
                    }
                }
            },
            Capability::Pointer => {
                if let Some(pointer) = seat_state.pointer.take() {
                    if pointer.version() >= 3 {
                        pointer.release()
                    }
                }
            },
            _ => (),
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        self.seats.remove(&seat.id());
    }
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        let seat = pointer.data::<PointerData>().unwrap().seat();
        let seat_id = seat.id();
        let seat_state = match self.seats.get_mut(&seat_id) {
            Some(seat_state) => seat_state,
            None => return,
        };

        let mut updated_serial = false;
        for event in events {
            match event.kind {
                PointerEventKind::Press { serial, .. }
                | PointerEventKind::Release { serial, .. } => {
                    updated_serial = true;
                    seat_state.latest_serial = serial;
                },
                _ => (),
            }
        }

        // Only update the seat we're using when the serial got updated.
        if updated_serial {
            self.latest_seat = Some(seat_id);
        }
    }
}

impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        data_device: &WlDataDevice,
        x: f64,
        y: f64,
        surface: &WlSurface,
    ) {
        // A new drag takes the place of one left unfinished.
        self.drag = None;

        let Some(offer) = drag_offer(data_device) else { return };
        if !offer.with_mime_types(|mimes| mimes.iter().any(|mime| mime == URI_LIST)) {
            // Not files: refused, so that the source shows that nothing is taken here.
            offer.accept_mime_type(offer.serial, None);
            offer.set_actions(DndAction::empty(), DndAction::empty());
            return;
        }
        offer.accept_mime_type(offer.serial, Some(URI_LIST.into()));
        // Copy only: a move could have the source delete the file.
        offer.set_actions(DndAction::Copy, DndAction::Copy);

        self.drags += 1;
        let mut drag = Drag {
            id: self.drags,
            data_device: data_device.clone(),
            surface: surface.id().as_ptr() as usize,
            x,
            y,
            list: None,
            dropped: false,
        };
        if self.read_dragged_list(&offer, drag.id).is_err() {
            drag.list = Some(Vec::new());
        }
        self.drag = Some(drag);
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        // After the drop the drag lives on until its list is read and said.
        if self.drag.as_ref().is_some_and(|drag| !drag.dropped) {
            self.drag = None;
        }
    }

    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        if let Some(drag) = self.drag.as_mut().filter(|drag| !drag.dropped) {
            drag.x = x;
            drag.y = y;
        }
    }

    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        let Some(drag) = self.drag.as_mut() else { return };
        drag.dropped = true;
        if drag.list.is_some() {
            self.say_dropped();
        }
    }

    // The selection is finished and ready to be used.
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataSourceHandler for State {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        mime: String,
        write_pipe: WritePipe,
    ) {
        self.send_request(SelectionTarget::Clipboard, write_pipe, mime)
    }

    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, deleted: &WlDataSource) {
        self.data_sources.retain(|source| source.inner() != deleted)
    }

    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlDataSource,
        _: Option<String>,
    ) {
    }

    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}

    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}

    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource) {}
}

impl DataOfferHandler for State {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        offer: &mut DragOffer,
        _: DndAction,
    ) {
        // (What the source lets be done with the files changed as they are dragged.)
        if self.drag.is_some() {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
    }

    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl ProvidesRegistryState for State {
    registry_handlers![SeatState];

    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
}

impl PrimarySelectionDeviceHandler for State {
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpPrimarySelectionDeviceV1,
    ) {
    }
}

impl PrimarySelectionSourceHandler for State {
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpPrimarySelectionSourceV1,
        mime: String,
        write_pipe: WritePipe,
    ) {
        self.send_request(SelectionTarget::Primary, write_pipe, mime);
    }

    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        deleted: &ZwpPrimarySelectionSourceV1,
    ) {
        self.primary_sources.retain(|source| source.inner() != deleted)
    }
}

impl Dispatch<WlKeyboard, ObjectId, State> for State {
    fn event(
        state: &mut State,
        _: &WlKeyboard,
        event: <WlKeyboard as sctk::reexports::client::Proxy>::Event,
        data: &ObjectId,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use sctk::reexports::client::protocol::wl_keyboard::Event as WlKeyboardEvent;
        let seat_state = match state.seats.get_mut(data) {
            Some(seat_state) => seat_state,
            None => return,
        };
        match event {
            WlKeyboardEvent::Key { serial, .. } | WlKeyboardEvent::Modifiers { serial, .. } => {
                seat_state.latest_serial = serial;
                state.latest_seat = Some(data.clone());
            },
            // NOTE both selections rely on keyboard focus.
            WlKeyboardEvent::Enter { serial, .. } => {
                seat_state.latest_serial = serial;
                seat_state.has_focus = true;
            },
            WlKeyboardEvent::Leave { .. } => {
                seat_state.latest_serial = 0;
                seat_state.has_focus = false;
            },
            _ => (),
        }
    }
}

delegate_seat!(State);
delegate_pointer!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
delegate_registry!(State);

#[derive(Debug, Clone, Copy)]
pub enum SelectionTarget {
    /// The target is clipboard selection.
    Clipboard,
    /// The target is primary selection.
    Primary,
}

/// Files dragged over a surface of the program.
struct Drag {
    /// Which drag: what is read of an earlier one's list is nothing to this one.
    id: u64,
    data_device: WlDataDevice,
    /// The surface it is over: the address of its proxy (see `DroppedFiles::surface`).
    surface: usize,
    /// Where on the surface it is.
    x: f64,
    y: f64,
    /// Its `text/uri-list`: `None` until all of it is read.
    list: Option<Vec<u8>>,
    /// It was dropped (maybe before its list was read to its end).
    dropped: bool,
}

/// The offer of the drag that is over a surface now.
fn drag_offer(data_device: &WlDataDevice) -> Option<DragOffer> {
    data_device.data::<DataDeviceData>()?.drag_offer()
}

#[derive(Debug, Default)]
struct ClipboardSeatState {
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    data_device: Option<DataDevice>,
    primary_device: Option<PrimarySelectionDevice>,
    has_focus: bool,

    /// The latest serial used to set the selection content.
    latest_serial: u32,
}

impl Drop for ClipboardSeatState {
    fn drop(&mut self) {
        if let Some(keyboard) = self.keyboard.take() {
            if keyboard.version() >= 3 {
                keyboard.release();
            }
        }

        if let Some(pointer) = self.pointer.take() {
            if pointer.version() >= 3 {
                pointer.release();
            }
        }
    }
}

fn set_non_blocking(raw_fd: RawFd) -> std::io::Result<()> {
    let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFL) };

    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let result = unsafe { libc::fcntl(raw_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}
