# Crates kept here, changed

A crate is kept in this folder when the program needs something of it that no release it can be
built with has. `[patch.crates-io]` in the workspace's `Cargo.toml` puts the copy here in place of
the one on crates.io; the folders are out of the workspace (`exclude`), so `cargo test --workspace`
does not run their own tests.

Each copy starts as the released crate, byte for byte, in a commit of its own, so that the commits
after it show exactly what was changed. Each change is listed below with why it is here and where
it is from. One test fails if a change falls out of the build, or a copy out of use:
`the_crates_kept_changed_are_built_with_their_changes` (in
`crates/brewed-texture-designer/src/third_party.rs`).

## winit 0.30.13

iced 0.14 is built on winit 0.30. Apache License 2.0 (`winit/LICENSE`). The folder goes when iced
is built on a winit that has the change.

**A window dragged to a display of another scale keeps its size (Windows 11).**
`src/platform_impl/windows/event_loop.rs`, at `WM_DPICHANGED`: from build 22000 on, the rect
Windows suggests is taken as it is. `dark_mode.rs`: the build number is seen by the event loop.

- **Why.** The code that follows asks `MonitorFromWindow`, which while a window is dragged still
  names the display it is leaving: the window was nudged back onto that display, which sent
  `WM_DPICHANGED` again, and the window grew and stayed on the wrong display at the wrong scale
  ([winit#4041](https://github.com/rust-windowing/winit/issues/4041)). Moved from a display at
  125 % to one at 300 %, the program's window became 2.4 times as large: larger than the GPU can
  draw (`window_size.rs` holds it to that, and still does).
- **From.** winit's own fix, [winit#4341](https://github.com/rust-windowing/winit/pull/4341)
  (commit 488c036a on its master), which no release of 0.30 has. Windows 10 keeps the earlier
  code, as it does there.
- **Tried.** Not by hand yet: the drag needs two displays of different scales on Windows 11.

## smithay-clipboard 0.7.3

The clipboard of iced 0.14 on Wayland (through `window_clipboard` and `clipboard_wayland`). MIT
(`smithay-clipboard/LICENSE`).

**Files dropped on a window on Wayland.** `src/dropped.rs` (new), `src/state.rs`, `src/lib.rs`.
The crate's data device, which took the selection only, takes the files dragged over a window
too: a drag that offers `text/uri-list` is accepted, for Copy only (a move could have its source
delete the file), and its list read without blocking the loop. Once it is dropped, the surface,
the place on it and the list are said through `smithay_clipboard::dropped_files()`. The program
listens (`crates/brewed-texture-designer/src/dropped_files.rs`) and takes the files as it takes
those a window says were dropped on it, which is how they come on X11, Windows and macOS.

- **Why.** winit 0.30 has no drag and drop on Wayland: a file dragged from the file manager onto
  the window did nothing, and nothing was said. (winit has it from 0.31 on,
  [winit#4571](https://github.com/rust-windowing/winit/pull/4571).) Starting under XWayland is
  no way out: niri's does not carry drags between Wayland and X11.
- **Why in the clipboard, and not in winit.** A compositor may send a drag to one
  `wl_data_device` of a program only. Mutter (GNOME) does, and the selection with it: to the one
  made last, which is this crate's (iced makes its clipboard once a window is there). This
  folder held a winit with a data device of its own before, the `wayland-dnd` branch of a fork
  of PhotoCraft (<https://github.com/MixaDoDs/photocraft/tree/wayland-dnd>), which works where
  the compositor sends a drag to every data device of a program, as niri does. Tried on GNOME
  (Fedora 44, Mutter 50, 2026-10-13), `wl_data_device.enter` came to the clipboard's device
  alone, nothing accepted the offer, and the drop was refused. A data device made later than
  the clipboard's would take the selection from it instead, and nothing could be pasted. So
  the one data device there is takes the drags; and winit makes none, or each file would be
  dropped twice where a compositor sends a drag to all of them.
- **From.** Written for this program, after that branch's `seat/dnd.rs`: what is accepted, Copy
  only, the list read off the loop.
- **Tried.** In a GNOME session (Fedora 44, Mutter 50, Wayland, 2026-10-13), the pointer driven
  through Mutter's RemoteDesktop interface: a picture dragged from Files onto the graph became a
  Bitmap node where it was let go, and nodes were still copied and pasted through the same data
  device. Not tried on KDE, Sway or niri: there a drag comes to every data device of a program,
  of which this is now the only one.
- **Goes when** the clipboard of iced and its winit take drags and the selection through one
  data device between them.
