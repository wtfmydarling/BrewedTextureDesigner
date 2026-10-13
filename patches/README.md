# Crates kept here, changed

A crate is kept in this folder when the program needs something of it that no release it can be
built with has. `[patch.crates-io]` in the workspace's `Cargo.toml` puts the copy here in place of
the one on crates.io; the folder is out of the workspace (`exclude`), so `cargo test --workspace`
does not run its own tests.

Each copy starts as the released crate, byte for byte, in a commit of its own, so that the commits
after it show exactly what was changed. Each change is listed below with why it is here, where it
is from, and the test that fails if it falls out of the build.

## winit 0.30.13

iced 0.14 is built on winit 0.30. Apache License 2.0 (`winit/LICENSE`). The folder goes when iced
is built on a winit that has both changes.

| Change | Why | From | Kept in the build by |
|---|---|---|---|
| **Files dropped on a window on Wayland.** `src/platform_impl/linux/wayland/seat/dnd.rs` (new), wired in `seat/mod.rs` and `state.rs`: `wl_data_device_manager` is bound and a data device made for each seat; a drag that offers `text/uri-list` is accepted for Copy only, its list read without blocking the loop, and said as X11 says it: `HoveredFile`, `DroppedFile`, `HoveredFileCancelled`, and the drag's place as `CursorMoved`. | winit 0.30 has no drag and drop on Wayland: a file dragged from the file manager onto the window did nothing, and nothing was said. Starting under XWayland is no way out (niri's does not carry drags between Wayland and X11). winit has it from 0.31 on ([winit#4571](https://github.com/rust-windowing/winit/pull/4571)). | The `wayland-dnd` branch of a fork of PhotoCraft, <https://github.com/MixaDoDs/photocraft/tree/wayland-dnd>, as it is there. | `the_winit_that_is_built_is_ours_with_its_changes` (in `crates/brewed-texture-designer/src/third_party.rs`). Tried by hand: see the README. |
| **A window dragged to a display of another scale keeps its size (Windows 11).** `src/platform_impl/windows/event_loop.rs`, at `WM_DPICHANGED`: from build 22000 on, the rect Windows suggests is taken as it is. `dark_mode.rs`: the build number is seen by the event loop. | The code that follows asks `MonitorFromWindow`, which while a window is dragged still names the display it is leaving: the window was nudged back onto that display, which sent `WM_DPICHANGED` again, and the window grew and stayed on the wrong display at the wrong scale ([winit#4041](https://github.com/rust-windowing/winit/issues/4041)). Moved from a display at 125 % to one at 300 %, the program's window became 2.4 times as large — larger than the GPU can draw (`window_size.rs` holds it to that, and still does). | winit's own fix, [winit#4341](https://github.com/rust-windowing/winit/pull/4341) (commit 488c036a on its master), which no release of 0.30 has. Windows 10 keeps the earlier code, as it does there. | The same test. The drag itself needs two displays of different scales on Windows 11. |
