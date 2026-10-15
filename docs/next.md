# What comes next

What is still to be done, in the order it matters, and how the work goes. Written on 2026-10-15,
the last day on Windows before the move back to Linux: whoever picks the work up (a person, or
an assistant in a new session that has no memory of the old one) starts here.

## First: the GPU going away and running out

1. **A lost device.** The driver resetting, the GPU being taken away (a laptop switching GPUs,
   a remote session, a crash of the driver): today the program goes on with a dead device. It
   must notice (`wgpu::Device::set_device_lost_callback`, errors of the kind `DeviceLost`), say
   so in the status bar, make a new device and engine, give them the document as it is, and
   compute it again; nothing of the document is lost, only images.
2. **Video memory running out.** A graph at 8K in 32 bits float, or many graphs exported at once,
   can ask for more than the GPU has. A texture or buffer that cannot be made must be an error of
   that node (shown on it, the rest computed), never a panic; the engine then lets go of what it
   keeps that nothing shows (scratch images, images kept for the next change) and tries again
   once; the status bar says the GPU is out of memory and what was asked. An export at a size the
   GPU cannot hold says so instead of failing silently.
3. **The rest of the GPU's ways to fail**: a shader the driver refuses (already an error of the
   node), a readback that never finishes (time out, say so), the window's surface lost or
   outdated on a resize (make it again), a GPU too old for what is needed (say so at start, with
   the adapter's name).

Each of these wants a test where it can have one (the engine runs headless on lavapipe), and a
round in the autopilot that provokes it.

## The export window

- Try it with a real mouse, on Windows and on Linux (GNOME on Wayland, and X11): dragging maps
  onto channels and the menus that ask how a map is taken, moving output maps by their handles,
  the lists of SETTINGS, Save settings and its undo. The tests drive it by its events only.
- What a channel writes where the graph has no such map (skip the file, leave the channel out,
  fill with a value) is kept in templates but has no control any more: give it one in the
  channel's menu if it is missed.
- Painter's export has, besides: padding (dilation) of UV islands, and of each texture set the
  output maps of its template checked one by one. Both belong to the texture painter.
- The texture painter will list UV sets under its texture sets (a third level of the list of
  what is exported) and give its own presets and name parts (`$(mesh)`, `$(textureSet)`…); the
  crates `texture-export`, `export-window`, `drop-down` and `scroll-box` are shared for that.

## Releases

- 1.0.0-pre.1 is the first version on `main` (tag `v1.0.0-pre.1`). Its GitHub release is made
  from the tag (Releases → Draft a new release → the tag, "Set as a pre-release"), with the
  builds attached.
- Builds: the scripts of `packaging/` (see [packaging/README.md](../packaging/README.md)) make
  the Windows installer, the AppImage, the Flatpak and the macOS disk image; no `.deb` or `.rpm`.
  Still to do: run them on each system (the AppImage on Debian 13, for its older glibc), attach
  what they make to the release, and an offline Flatpak build (its crates listed) for Flathub.

## The rest, by priority

1. **Subgraphs**: resolution and tiling inherited from the graph that uses a graph, as in
   Substance Designer.
2. **Per-node resolution and bit depth** (relative to the parent, as in Substance Designer).
3. **A parametric runtime format** with engine plugins, like `.sbsar`. Reading or writing
   Adobe's own `.sbs`/`.sbsar` is wanted but has to be clean-room (their formats are closed):
   decide how before starting.
4. **Library nodes**: more compositions made as graphs.
5. **3D view**: a mesh of one's own; lights besides an environment.
6. **Plugins** (not now): WGSL code nodes and packages of graphs; scripting in WASM or an
   embedded language; never native libraries.
7. An x86_64 build for macOS (Apple Silicon is the target).

## How the work goes

- **Branches**: every commit goes to `dev`, pushed at once. `main` holds released versions and
  their patches only: it moves when its owner says a version is ready, never otherwise.
- **Before a commit**: `cargo fmt` (160 columns, `rustfmt.toml`); `cargo clippy --workspace
  --all-targets` with and without `--all-features`; `cargo test --workspace` on Linux and on
  Windows (on Linux without a GPU: `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json`);
  `cargo check --workspace --target aarch64-apple-darwin`. The program builds on Windows, Linux
  and macOS alike; nothing is turned off on any of them.
- **The interface**: depth in every screen (sunken lists, raised rows, menus above with
  shadows); plain, short, correct labels ("Project's settings", "Template's defaults", "Invert");
  every scrolling area is `scroll_box`, every floating menu `drop-down`; a program the user names
  as the reference (Substance Painter's export) is followed closely, then made better; nothing is
  read-only "for safety" when it can be changed and reset; every setting that a level above has
  is offered at each level ("Global settings (…)", "Package settings (…)").
- **The autopilot** (`--features dev-tools`, `--autopilot MINUTES --seed N`) runs on an idle
  machine only: other work on the GPU makes false findings.
