# What ArtCraft's Crafting Apps do that we can use

A review of the public `storytold/*` repositories as they were on 2026-10-12, read for what Brewed
Texture Designer can take from them. Nothing of theirs is copied here or into our code: this file
says how their programs are built and what follows for ours.

How it was read: every repository cloned (shallow); every `docs/` page, `AGENTS.md`, manifest,
workflow and packaging script; the code that carries the architecture (registries, sessions,
automation, control servers, plug-in hosts, file formats, xtask); the rest sampled by search. The
programs together are about 3 million lines of Rust, so "all of it line by line" is not what this
is.

Repositories: photocraft (Photoshop), vectorcraft (Illustrator), effectcraft (After Effects),
filmcraft (Premiere Pro), lightcraft (Lightroom), designcraft (InDesign), pdfcraft (Acrobat),
cadcraft (AutoCAD), soundcraft (Pro Tools), wordcraft / gridcraft / deckcraft (Word, Excel,
PowerPoint), craft-libs (shared Rust libraries), craft-launcher, craft-common-actions (shared CI),
craft-fonts, photocraft-corpus, craft-repo-status-app. No program of theirs does what Substance
Designer does.

Contents:

1. [Made for agents](#1-made-for-agents) — the part that matters most
2. [The engine under the commands](#2-the-engine-under-the-commands-effectcrafts-session)
3. [The GPU out in the world](#3-the-gpu-out-in-the-world-lightcrafts-docsgpu-pipelinemd)
4. [What the other apps add to the agent side](#4-what-the-other-apps-add-to-the-agent-side) — with a correction about plug-ins
5. [How the repositories keep themselves honest](#5-how-the-repositories-keep-themselves-honest)
6. [What their programs have that the originals do not](#6-what-their-programs-have-that-the-originals-do-not)
7. [Smaller things worth taking as they are](#7-smaller-things-worth-taking-as-they-are)
8. [For Brewed Texture Designer: what I would do, in order](#8-for-brewed-texture-designer-what-i-would-do-in-order)
9. [Where we fit beside them](#9-where-we-fit-beside-them)
10. [Sizes, for planning](#10-sizes-for-planning)
11. [Crates they trust](#11-crates-they-trust)
12. [Their algorithms that would make texture nodes](#12-their-algorithms-that-would-make-texture-nodes)

Part II — the code, read closely:

13. [CADCraft's engine](#13-cadcrafts-engine-79000-lines-790-tests-the-window-is-14--of-the-program) — the session, `execute`, the check after every command, AUDIT, interactive commands
14. [VectorCraft's tools](#14-vectorcrafts-tools-a-crate-of-25000-lines-that-knows-no-window) — begin / preview / commit
15. [EffectCraft's document, and its GPU compositor beside our engine](#15-effectcrafts-document-and-its-gpu-compositor-beside-our-engine)
16. [Ours beside theirs, honestly](#16-ours-beside-theirs-honestly) — **the comparison**
17. [How their engines reach the outside world](#17-how-their-engines-reach-the-outside-world)
18. [Small things seen in CADCraft's window](#18-small-things-seen-in-cadcrafts-window)
19. [Features seen in the three nearest programs that would suit ours](#19-features-seen-in-the-three-nearest-programs-that-would-suit-ours)
20. [What a command looks like — theirs, and ours as it would be](#20-what-a-command-looks-like--theirs-and-ours-as-it-would-be)

Part III — the other nine, by what recurs:

21. [Checkers that return a report](#21-checkers-that-return-a-report) — audit, preflight
22. [Keeping a saved file meaning the same thing](#22-keeping-a-saved-file-meaning-the-same-thing) — migration, pinned pictures
23. [Traps of the Linux desktop they fell into](#23-traps-of-the-linux-desktop-they-fell-into-we-use-the-same-libraries) — **file drops on Wayland do not reach us**; the window itself (23a)
24. [Queues of small jobs](#24-queues-of-small-jobs-lightcrafts-preview-crate-1250-lines)
25. [Measuring the distance to the original](#25-measuring-the-distance-to-the-original)
26. [What their own measurements taught them](#26-what-their-own-measurements-taught-them)
27. [What "a replacement" is taken to mean, and how behaviour is measured](#27-what-a-replacement-is-taken-to-mean-and-how-behaviour-is-measured)
28. [Crates of theirs that could be used as they are](#28-crates-of-theirs-that-could-be-used-as-they-are)
29. [The browser and FreeBSD: what each cost them](#29-the-browser-and-freebsd-what-each-cost-them)
30. [**What reading their code found in ours**](#30-what-reading-their-code-found-in-ours) — eleven checked gaps, seven things found sound

---

## 1. Made for agents

The one idea under everything: **every action of the program is a command with a stable id and
JSON parameters, kept in one registry**, and the menu, the shortcuts, the panels, the command
line, the control channel and the MCP server all run those same commands. An agent can therefore
do whatever a person can, and — as important — can *find out* what there is to do without reading
the source.

### 1.1 Three ways in (EffectCraft's `docs/agents.md`)

| Way in | For | Window |
|---|---|---|
| MCP server (`effectcraft-cli mcp`) | Claude Code and other MCP clients | none (headless), or `--bridge PORT` to a running app |
| CLI (`effectcraft-cli exec / get / set / run / script / render-frame`) | scripts and CI; `--json` gives one JSON document; exit status 1 on an error, 2 on a wrong usage | none |
| Control channel (`effectcraft --control PORT`) | driving and *seeing* the live window | yes |

The repository ships a ready `.mcp.json` with two servers: headless with the demo project, and
bridged to a running app. An agent that opens the repository has the program in its hands at once.

### 1.2 How small the MCP server is

`crates/automation` in EffectCraft is about 2,300 lines without its tests, depends on the engine,
`serde_json` and `image` only, and is written by hand: JSON-RPC 2.0, one message a line over
stdio, no async runtime ("starts instantly"). Its parts:

- `server.rs` (about 300 lines): `initialize` (answers with the client's protocol version when it
  knows it; a paragraph of `instructions` that says how the program is driven), `tools/list`,
  `tools/call`, `resources/list|read`, `ping`. Input is read on its own thread so that a long
  render can report progress, be cancelled, and let other requests through meanwhile.
- `backend.rs`: where a tool runs — `Headless(Session)` in the process, or `Bridge(client)` to the
  app's control channel. The same tools work on both.
- `tools.rs`: the catalogue. Each tool is a static record: name, description, a function giving
  its JSON Schema, a function running it, and whether it needs the live app. The catalogue is
  shared by the MCP server and the CLI's subcommands, "so both behave the same".
- `bridge.rs`: a blocking client of the control channel; loopback addresses only; reconnects once
  if the app was restarted; never resends a request the app may already have run.
- `confine.rs`: `--automation-read-root DIR` / `--automation-write-root DIR`. Every command's file
  parameters must be absolute paths inside the roots (links resolved, no `..`); with no write root
  nothing is written; the tools that drive the window like a person are refused while confined.
- `autosave.rs`: `mcp --autosave` writes a checkpoint after each tool call that changed the
  project, *before the reply is sent*: killing the server right after a reply loses nothing.
- `long_job.rs`: progress notifications (only when the client sent a progress token) and
  `notifications/cancelled` for renders; a cancelled export removes exactly the files it opened.

What makes it pleasant for an agent, all of it cheap to do:

- **A tool call that fails says so in-band** (`isError: true` with the message), panics included
  (`catch_unwind` around every tool: "a panic becomes an error result and the session keeps
  serving").
- **An unknown argument is an error that lists the accepted ones** ("unknown argument "index" for
  layer.select; expected: layers, add, toggle"), never silently ignored.
- **A reference that matches nothing is an error**, "never a silent no-op".
- **Replies carry what the next call needs**: `effect.apply` returns the paths of the effect's
  parameters, `layer.new…` the layer's id.
- Things are named three ways: by id, by `"#n"` (place in the list), or by name.
- `batch {steps}`: several commands as one undo step; a parameter `"$1.layer"` is the first step's
  result; a failing step rolls all of them back and is named.
- Tools are annotated read-only / destructive / idempotent, so a client can ask before the
  dangerous ones only.
- Two resources, `effectcraft://document` and `effectcraft://commands`, give the same as the
  inspect and list tools.
- `list_commands {filter, enabled_only, schemas}` and `describe_command`: every command with its
  label, menu path, shortcut, parameters, a JSON Schema, and **whether it can run now and why
  not**.
- `render_frame` returns the picture itself (a PNG, the longest side capped), so the agent looks
  at what it made.
- `history` lists the whole branching undo tree; `goto` jumps to any state.

### 1.3 Seeing and operating the window

The desktop app, started with `--control PORT`, listens on 127.0.0.1 only (`control_server.rs`,
270 lines): one JSON request a line, one reply a line. It is hardened as a thing on a network
port should be: a line that is not a JSON object with a `method` (a browser's cross-origin
`fetch`, a stray `nc`) gets one error and the connection is closed *before anything after it
runs* — there is a test with an attacker's HTTP POST; lines are capped at 4 MiB, connections at
16. PhotoCraft adds a 256-bit token (a file with mode 0600) that must be the first request.
Requests are handed to the UI thread through a channel and answered within 60 s.

In bridge mode the agent gets `screenshot {panel, id, max_side}`, `ui_inspect` (the UI's state as
JSON), `ui_elements {prefix}` (every control with its *automation id* and where it is),
`ui_click`, `ui_drag`, `ui_key`, `ui_type`, `ui_set`. Controls have stable ids of the form
`timeline.prop.<uid>.menu.<entry>`, `dialog.unsaved.save`, `viewer.puppetPin.<uid>.rotate`. The
loop their docs recommend: `ui_elements` to find an id, `ui_click` to act, `screenshot` to check.

### 1.4 The documentation is written for the agent, from agents' sessions

`docs/agents.md` is 670 lines and reads as a manual for a colleague: a table of tools, "a typical
session" in five calls, a **cookbook** (goal → the calls), and **pitfalls** — "things that tripped
up a long agent-driven session", each with the issue it became (layer time against composition
time; a script that throws keeps its edits unless wrapped in an undo group; save early when
headless). When an agent stumbles, the fix is either in the program or in this page.

### 1.5 The same interface tests the program

`crates/automation/tests/qa/` holds scenarios named for seats — "a motion designer's seat": a real
MCP server over a headless session, spoken to in JSON-RPC exactly as a client would, building
whole projects and checking the rendered pixels numerically (a point track applied to a null
follows a synthetic square to within a pixel on frames 0, 5 and 11; Warp Stabilizer halves the
measured jitter; Roto Brush's matte has the square's centre and area). So the agent interface
cannot rot: the product's own end-to-end tests go through it.

### 1.6 The nose

What the video showed is `track.mask` with `method: "faceDetailed"`: the mask drawn around a face
becomes the face's outline on every frame, and a "Face Track Points" effect gets a keyed point
for each landmark — eyebrows, eyes, pupils, **nose tip**, nostrils, mouth, chin, jaw. The
landmark model (MediaPipe Face Landmarker, named with its authors and licence) is downloaded and
verified by a command (`face.model.download {id, wait: true}`); a classical tracker is built in
for when it is absent. Binding text to the point is `prop.pickWhip` or an expression. Every step
is a command, each waits with `"wait": true`, and the agent checks itself with `render_frame`.
The person did not need to remember After Effects because the *program* could be asked what it
can do.

### 1.7 What this means for us

We have the right bones — the document model (`brewed-texture-core`) and the evaluation
(`brewed-texture-engine`) know nothing of the window — and three things that are close relatives
of theirs: `--render` (a headless render), `--screenshot`/`--scenario` (the window drawn to a
file) and the autopilot (the program driven by a script). What we lack is the registry: our
actions are variants of the UI's `Message`, reachable only from inside the window.

In order of what unlocks what:

1. **A command registry in the core**: id (`graph.addNode`, `node.setParam`, `link.connect`,
   `package.save`, `export.run`…), label, parameters with a schema, "can it run now, and why
   not", run. `PackageEdit`/`GraphEdit` are most of the way there already: they are data, they are
   undoable, and the UI already sends them.
2. **`brewed-texture-cli`**, a second binary without the window: `commands`, `exec`, `get`,
   `set`, `render`, `mcp`. `--render` moves there.
3. **An MCP server** over it, by hand, as small as theirs: `command_list`, `command_run`,
   `command_batch`, `doc_inspect` (the graph as JSON: nodes, parameters, links, input values),
   `render_preview` (a node's or an output's picture as a PNG), `undo`/`redo`. For a node editor
   this is a natural fit: "make me a rusty metal with scratches" is a list of add-node, set-param
   and connect commands, checked by looking at the output.
4. **The bridge**: `--control PORT` in the designer, so an agent sees the live graph and the 3D
   view; our `--screenshot` machinery becomes its `screenshot`.
5. **`docs/agents.md` and a `.mcp.json`** in the repository, and scenarios of the autopilot's kind
   run through MCP.

---

## 2. The engine under the commands (EffectCraft's `Session`)

How a command is written and run — small, and nearly all of it portable:

- **A command is one line of a table**, made by a macro:
  `cmd!("layer.newSolid", "Solid...", ["Layer", "New"], Some("Cmd+Y"), "{name?, color? #hex|[r,g,b], width?, height?}", has_comp, new_solid)`
  — id, label, where it is in the menus, its key, its parameters *as a line of text*, a function
  saying whether it can run (returning the reason when it cannot), the function that runs it.
  Queries are declared with `query!` and are not journaled.
- **The parameters' text is the schema.** `{name?, color?, …}` is parsed: the keys are what the
  command accepts (`?` = optional), a JSON Schema is generated from them, and a call with any
  other key is refused with the list. No derive macros, no second description to keep in step.
- **The menu, the key table and the command palette are built from the registry.** A file per
  area (`commands/layer.rs`, `mask.rs`, `track.rs`… about 75 of them), each with a function that
  returns its commands; new areas are new files, so several agents do not fight over one.
- **`Session::execute(id, params)`** looks the command up; if it "cannot run now" but the caller
  named its targets explicitly (an agent does not need a selection), it runs anyway; the run is
  wrapped in the last-resort guard (a panic becomes "…failed with an internal error; it was
  stopped and your project is still open"); then it is added to the journal (the last 10,000).
  The journal is what actions/macros are recorded from.
- **`Session::edit(label, merge_key, |project, state| …)`** is the only way the project changes.
  The closure works on a *copy* (the project is behind an `Arc`, so a copy shares what did not
  change): an edit that fails or panics leaves the project as it was. After it: things that must
  always hold are put right in one place (analyses made of other frames are dropped, and so on);
  **if nothing changed there is no undo step and the project is not "modified"**; edits with the
  same merge key (one drag of a slider) fold into one step. "Modified" is "is this the very state
  that was saved" (a pointer comparison), so undoing back to the saved state clears the mark.
- **The history is a tree** ("better than After Effects, whose undo is a straight line"): undo,
  then do something else, and what was undone stays as a branch one can jump back to.
- **Background jobs** (`jobs.rs`): a closure runs on a worker with a handle for progress and
  cancel, and *returns a function* that the session runs on its own thread when it sees the job
  finished — that is where the project is edited, as one undo step. One list of every job
  (`jobs.list`, `jobs.cancel`, a Progress panel). Each long command takes `"wait": true` for
  callers that want to block. In the browser the same jobs go to Web Workers.

Against ours: we are closer than it looks. `Edit`/`PackageEdit` are already "the only way the
package changes" and are data; `Document` keeps a `History` of whole packages whose graphs are
behind `Arc` (a step copies only the graph it changes) — the same design as theirs, in all
twelve of their programs — with merge keys, labels and a seal. What we do not have: ids and a
schema for the edits, "why can't this run now", the journal, a history one can branch in, "not
modified" again after undoing back to what was saved (ours counts revisions; theirs compares the
state with the saved one), a guard around each edit, and one list of background work with
cancel.

The same shape is in every one of their apps, which is the best evidence that it works: the
document is an immutable snapshot with shared parts; an edit runs on a clone and replaces it on
success; continuous gestures go through a merging entry (`execute_merged`), and tools emit
*begin / preview / commit* so that a drag is one step; caches are keyed by revisions; whatever
runs elsewhere (SoundCraft's audio thread, render workers) receives new snapshots through a
channel and never touches the live document. PdfCraft goes one further: after every edit an
incremental write produces the working file (the original bytes plus the changes), so crash
recovery is nearly free.

---

## 3. The GPU out in the world (LightCraft's `docs/gpu-pipeline.md`)

LightCraft develops photographs with wgpu compute shaders, with a CPU pipeline as the reference —
the nearest thing they have to our engine. The page is a record of what broke on real users'
machines. Every item below has an issue number behind it there, and most apply to us as they are.

**Starting**

- **wgpu loads the driver of every backend in the instance's set while it lists adapters**, even
  when it then picks another. A Vulkan driver that crashes there (Intel UHD 630, `igvk64.dll`,
  Windows 11) kills the process before a window exists, and a native crash cannot be caught. So
  they let wgpu touch only the backend they mean to use: **DX12 alone on Windows**, Metal on
  macOS, Vulkan on Linux. *We ask for `vulkan,dx12` on Windows.*
- The other program's experience cuts the other way, which is why this is a judgement and not
  a rule: in EffectCraft "Direct3D 12 fails (FXC can't compile the `pointwise` kernel)" and
  Windows runs on Vulkan. We have met FXC's limits ourselves and written around them, so both
  work for us; with both working, a launch guard (below) covers the driver that crashes, without
  giving up either.
- **The DX12 shader compiler is pinned to FXC.** Left to choose, wgpu loads whichever
  `dxcompiler.dll` the search path finds first — some other program's, perhaps an old one without
  `dxil.dll`; the warning it gives is treated as an error, the device is lost, the window never
  opens. *Checked for ours: in the wgpu we build with (27) the default is FXC unless
  `WGPU_DX12_COMPILER` says otherwise, and iced takes that default — so this trap is not open
  to us today. It would open with a wgpu whose default is "auto".*
- **A crash sentinel**: a marker file written just before the device is made and the kernels are
  compiled, removed as soon as that returns. Found at the next start, it means the process died
  inside the driver: the program starts without the GPU path, says so, and re-arms when the user
  turns it back on.
- The window's renderer is probed in order (DX12, Vulkan, GL, then OpenGL by another library),
  each inside error scopes; the one that worked is remembered; a start that dies before the
  window is up is relaunched (four times at most) with that renderer tried last.
- The compute device is made on a background thread after the window is up (we warm the kernels
  the same way).
- A known trap, written down for users: with NVIDIA's *Shader Cache Size* set to Disabled, the
  driver walks the whole system drive while the device is created — a black window for minutes.

**While it runs**

- **Short submissions.** One export recorded as a single submission lasting seconds was reset by
  an Intel driver's hang check *without any error reaching wgpu*: valid JPEGs, entirely black.
  Now a render submits after every stage and every 16 million kernel invocations, and the
  per-pixel stage runs in bands of about 4 megapixels (Windows resets a GPU after ~2 s).
- **Error scopes around every render** (out of memory, validation, internal), a device-lost
  callback, and a readback wait with a timeout — before, a lost device left the readback waiting
  for ever. (We met the same thing from the other side and added `Readback::wait`.)
- **Never trust the result blindly**: the output is cleared first and the last stage writes alpha
  255 everywhere, so any pixel without it means work did not run; an entirely black result is
  rendered again on the CPU.
- **Limits**: the device is made with the adapter's limits and every buffer is checked against
  them when it is created; a render that would exceed them falls back for that render only.
  `LIGHTCRAFT_GPU_LIMITS=webgpu` makes a small device on purpose, to reproduce a weaker GPU; tests
  inject faults (`inject_fault`) and check the fallback.
- A table says, for each kind of failure, what happens to this render and to later ones.
- **Why the GPU is not used is always findable**: `perf.gpuReason`, `perf.gpuFallback` in the
  inspect output, in Help ▸ System Info and in the settings.

**Quitting**

- A render still inside the driver while the process exits crashes there (NVIDIA Vulkan on
  Linux), and no panic hook catches it. Every entry into the device counts as work in flight; on
  exit new work is refused, running work is waited for, with one deadline of 2 s for all of it.

**Memory**

- One budget (the smaller of a quarter of RAM and 1.5 GiB), shared by all caches and evicted
  least-recently-used across them; GPU buffers are pooled and reused by best fit (allocating and
  clearing fresh 100–300 MB buffers every pass "cost as much as the passes") and freed after 3 s
  idle. A command reports what every cache holds.

**Correctness**

- The same settings rendered on CPU and GPU must agree to within 3 steps of 255 in the worst
  pixel and half a step on average; measured: 1 and 0.0003. Parameters shared by both are computed
  once and uploaded, "so the two implementations cannot drift apart".

**Where our engine stands on these** (checked in `gpu/mod.rs`): errors no scope caught are
written down and shown in the status bar (`on_uncaptured_error`), and validation scopes guard
the making of kernels — so a refused shader is a message, not a crash. There is no scope for
*out of memory* and no device-lost callback. An 8K picture at 32 bits float is a gigabyte; a
graph of them can outrun a video card, and what follows today is whatever the driver does. Theirs:
an out-of-memory scope around the work, the pool emptied, one more try, and then a plain
sentence naming what did not fit.

What to take: the backend set on Windows, the sentinel, short submissions for large graphs, error
scopes with a device-lost callback, the result check, a made-small device for tests, waiting for
work in flight at exit, and "why not the GPU" shown to the user. We have no CPU reference, so
where they fall back we can only stop and say why — which is still far better than a black
texture written to disk.

---

## 4. What the other apps add to the agent side

Every one of the twelve has a "Built for agents" part in its README, a `docs/mcp.md`, a
`docs/control-protocol.md` and a `*-cli`. They agreed on a common core of tool names
(`command_list`, `command_run`, `command_batch`, `doc_inspect`, `render_preview`) so an agent
that knows one knows the others. Beyond what EffectCraft does:

- **PdfCraft, `mcp --compact`**: the tool list an agent must read is cut to about ten core tools
  plus `tool_search` and `tool_call`, which find and run every other one — "the list costs far
  fewer tokens. Every tool still works."
- **PdfCraft, the window from the accessibility tree**: `inspect query=rotate` and
  `click label="Organize pages"` work on the widget tree the toolkit already builds for screen
  readers (labels, positions, focusability), so no widget needs an automation id written by
  hand. The same work makes the program usable with a screen reader.
- **PdfCraft, the control file**: the port and a random token are written to a file only the user
  can read; the client refuses a control file that is a symbolic link or that another user owns
  or can write ("another user could create the file there first and receive your commands").
- **PdfCraft, the server is opt-in and can be compiled out** (`--no-default-features`).
- **SoundCraft**: "Programmatic calls never open dialogs, and `session.inspect` reports exactly
  what changed, so an agent can check its own work." And a *health report* as a command
  (`session.audio_health`): what is missing or mismatched, with a suggestion for each — the same
  JSON the window shows.
- **LightCraft**: `develop.controls` lists every slider with its range, default and present
  value; `ui.clickWidget {id: "slider:effects.clarity"}`; a pointer gesture is a list of events.
  The screenshots of its README are made end to end by scripts in `docs/showcase/` through the
  control channel, so they are never stale.
- **LightCraft, drafts while dragging**: a slider drag renders drafts, the release renders in full.
- **PhotoCraft, actions**: `actions.record` / `stop` / `play` — a macro is the journal's entries
  between two points, replayable in a batch over a folder (`photocraft-cli batch --actions
  grade.json --in ./raw --out ./graded`).
- **PhotoCraft, design tokens live**: in debug builds a JSON file of theme values is re-read when
  it changes — the look is tuned with the program running, without a build.
- **CADCraft**: its command line (the AutoCAD kind) *is* the script language: `cadcraft-cli run
  --script 'CIRCLE 22,3 1\n' --save out.dxf --export out.png`.

### Plug-ins — a correction

Earlier I said none of their programs runs the plug-ins of the program it stands in for. That
holds for the proprietary formats (Photoshop's 8BF/UXP, After Effects' SDK, Pro Tools' AAX —
"needs Avid's SDK licence and PACE signing: owner decision, likely never"). It does not hold for
open formats: **SoundCraft hosts CLAP, VST3 and Audio Units**, which is what people's plug-in
libraries actually are. Each host is its own small crate (3,000, 4,500 and 1,800 lines), the only
crates allowed `unsafe`, and only in a private `ffi` module with a `SAFETY:` note on every block;
their public API is safe and never panics; on wasm they compile to stubs. They say plainly what
in-process hosting cannot do: "a plugin that itself crashes takes the process down with it".

The pattern, then: **their own sandboxed format for new plug-ins (WebAssembly, no imports, fuel
and memory limits, a manifest from which the dialog is generated), plus the open formats the
field already uses, plus the documented scripting language of the original** (After Effects'
`.jsx`, AutoCAD's command scripts, AutoLISP planned).

For a texture program the open formats of the field are not plug-in binaries but **MaterialX**
(node graphs of materials and patterns, an open standard of the Academy Software Foundation),
**OpenColorIO**, **OpenEXR**, **glTF** and **USD**. Reading and writing MaterialX graphs would
be to us what VST3 is to SoundCraft: the thing other programs already speak. That is a more
useful aim than `.sbsar`.

---

## 5. How the repositories keep themselves honest

The code is written by many agents at once, so nearly every rule is something a machine checks.

### 5.1 One command for "is it fit to merge"

`cargo xtask ci` = format check, clippy with warnings as errors, tests, the layering check, the
asset audit, the wasm build check. `xtask` is a small Rust program in the workspace
(`.cargo/config.toml`: `xtask = "run -p xtask --"`), so the tooling needs nothing but cargo and
runs the same on every system. Its other commands, across the apps:

| Command | What it does |
|---|---|
| `layers` | a table "crate → layer" checked against `cargo metadata`: a crate may depend only on lower layers; UI toolkits only in the top one; format crates on nothing of the workspace; an unregistered crate is an error |
| `wasm` | `cargo check --target wasm32-unknown-unknown` for the crates that must stay portable |
| `assets` | every file that is not code has an entry in `ATTRIBUTION.toml` (author, source, licence, **sha256**); `ATTRIBUTION.md` is generated from it |
| `parity` | regenerates the list of the original's menu items, live or missing |
| `scorecard [--check]` | regenerates `docs/scorecard.md` from TOML checklists, perf numbers and corpus floors; CI fails if it is stale |
| `perf [--quick] [--update-baseline]` | runs the release benches, merges them by scenario, checks `perf/budgets.toml` and `perf/baseline.json` |
| `corpus`, `test-corpus` | fetches real-world test files at pinned commits, sha256-verified, never committed; runs the tests that need them |
| `stats` | tests and lines per crate |
| `i18n-coverage` | how much of the interface each language covers |
| `version set X.Y.Z` | the one place the version changes |
| `web [--serve]` | builds the browser version and serves it with the right headers |

### 5.2 Never crash, as lints

- Workspace: `unsafe_code = "forbid"`. Every crate root: `#![deny(clippy::unwrap_used,
  clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`;
  `clippy.toml` allows them in tests. The scorecard *counts* the crates that carry the attribute.
- `unsafe` lives in one named crate per app, by a written decision (FilmCraft's ADR 0001:
  `crates/platform` for OS media APIs; SoundCraft's plug-in hosts; PhotoCraft's tablet crate):
  `unsafe_code = "deny"` crate-wide, allowed only in named FFI modules,
  `clippy::undocumented_unsafe_blocks = "deny"`, a `SAFETY:` note on every block, a safe public
  API, no panic across the FFI boundary, compiles (as a stub) on every target. *We have `unsafe`
  in twelve files of the designer (eyedropper, scrubbing, tool window, clock, window size…): the
  same move — one `brewed-platform` crate — would let everything else forbid it.*
- A hardware path "never makes a file undecodable, and never changes a picture": it declines
  what it cannot do, and one that fails midway is replaced by the software one without the
  caller noticing, counted in the statistics.
- `panic_hunt`: every command of the registry is run with four sets of hostile parameters, each
  on its own thread with a 4 s deadline; a panic or a hang fails the test and names the command.
- Parser checklist (PhotoCraft's security book), for every count, offset, size or length from a
  file: a type that holds the field without truncation; limits checked *before* allocating;
  checked arithmetic; ranges verified before slicing; decoded bytes, objects and recursion
  charged against a budget; a format-specific error; tests at zero, the maximum, one past it,
  truncated, and inconsistent.
- A threat model that names agents as a threat actor: "a malicious, compromised, or
  over-privileged MCP client", "a project/action file that induces unsafe paths or expensive
  commands".

### 5.3 Tests

- **Through the agent's door** (section 1.5).
- **A reference for every fast path**: CPU against GPU, cached against fresh, the fast blur
  against the plain one.
- **Each effect** has a determinism test and a behaviour test; a registry-wide test checks that
  effects reading the clock are declared time-dependent; simulation effects have pinned pixel
  hashes (re-pinned with an environment variable after an intended change — as our
  `nodes.txt` is).
- **Docs generated from the registry, checked by a test**: `docs/effects.md` (306 effects, 3,017
  parameters) is written by `UPDATE_DOCS=1 cargo test … effects_doc_is_current`, which otherwise
  fails when the page is stale. *A node reference for us, straight from `nodes::all()`.*
- **Outside oracles, never linked**: ffmpeg/ffprobe, qpdf, poppler — used when installed.
- **The window without a window**: `cargo run -p …-ui-egui --example snapshot -- --out ui.png
  --step '{control request}'` (our `--screenshot --scenario` and the test harness's snapshots).
- **`cargo nextest`** with a group that limits GPU tests to four at a time: 297 s → 120 s.
- **Fuzzing in CI** for a fixed number of seconds per target; five fuzz crates in PhotoCraft.
- **Failure injection**: the atomic writer takes a file system as a parameter so tests can fail
  each step; the GPU has `inject_fault`.

### 5.4 Performance as a contract

`perf/budgets.toml`: each scenario names a bench and a row, a metric (p50/p95/max) and a budget
in milliseconds. `enforce = false` marks a target not yet met — it is *reported* as over budget
but only a regression fails (more than 15 % and more than 1 ms over the baseline); a scenario the
code cannot run yet carries `not_measurable` and the reason: "never invent a number". A nightly
workflow runs it. The dev profile builds dependencies optimised and uses
`debug = "line-tables-only"` (panics keep file and line; test binaries link about twice as fast).

### 5.5 Documents that say where the program stands

Each app has the same set, each page headed by a status line (last reviewed, last updated, the
kind of change, the target version) and ending in a revision history:

- `ROADMAP.md`: one page — stage, the numbers, what is next.
- `docs/target-app-parity.md`: the assessment against the original, dimension by dimension, with
  weights, evidence and hours.
- `docs/gaps.md`: every known shortfall, ranked, each with what is missing, why it matters, an
  estimate, and whether it needs a human decision.
- `docs/*-parity.md`: checklists per dimension (UI, file formats, hardware, localization…).
- `docs/parity-checklist.md` and `docs/scorecard.md`: generated; "its numbers are measured;
  prefer them to estimates".
- Decisions that need the owner are marked as such and left alone.
- `log/devlog.md` (local, not committed): a terse entry per finished task — "sessions can end
  abruptly, so the dev log plus a green tree is how the next agent picks up".

### 5.6 Rules for agents working side by side (PhotoCraft's `AGENTS.md` §6)

Own target directory each (`CARGO_TARGET_DIR=target/agent-<name>`); edit only the files you own;
shared files get small surgical edits; new commands go in a new module; if someone else's
half-done edit breaks the build, wait — don't fix their files; write manifests to a temporary
file and move them into place, because one broken `Cargo.toml` breaks everyone's build.

---

## 6. What their programs have that the originals do not

Collected from their own notes ("beyond After Effects", "better than Photoshop"…):

| What | Where | For us |
|---|---|---|
| **Driven by agents**: MCP, CLI, control channel over one command registry | all twelve | section 1; the largest single thing |
| **A history that branches**; any state reachable | EffectCraft | Substance Designer's undo is a line too |
| **The same project renders the same bits** every time, whatever the number of threads; a test renders twice on one thread and on several and compares every float | EffectCraft | a procedural texture should be the same bytes on every run; we compare within ±1 across GPUs |
| **Runs on Linux, FreeBSD and in the browser** | all | Substance Designer has Linux builds for some distributions only; no browser |
| **Sandboxed plug-ins**: one `.wasm` for every system, cannot touch files or the network, cannot hang the program | PhotoCraft, VectorCraft, EffectCraft | Designer's plug-ins are unsandboxed Python and cannot add nodes at all |
| **A list of what an export could not express**, returned with the file (`file.exportLottie` → `warnings`) | EffectCraft, PhotoCraft (Affinity import), CADCraft (DXF round trip) | every importer and exporter of ours should return what it dropped |
| **Open neighbours' formats read and written**: OpenRaster, Paint.NET, Affinity (read), Lottie, OTIO, FCPXML | PhotoCraft, EffectCraft, FilmCraft | MaterialX, glTF, USD for us |
| **Unknown parts of a file are carried through** a load and save untouched | PhotoCraft (PSD blocks), CADCraft (DXF entities) | our package format already keeps other entries and `extras` |
| **Interface scale as a setting** (75–200 %) | EffectCraft | we removed ours ("the OS does DPI") |
| **A light theme and following the system's appearance live** | SoundCraft, others | — |
| **Numeric fields scrubbed by dragging their labels** | VectorCraft | we have scrubbing |
| **No account, no cloud, no telemetry**; network only when asked | all | same as ours |
| **Interactive lessons** that highlight the control to use and can "show me" | EffectCraft | section 7 |
| **Local models, named and verified**: a command lists each model with its authors, licence, size, URL and SHA-256; download and install are commands; a classical method is always built in | EffectCraft, LightCraft | if we ever add "AI" nodes, this is the honest way |
| **A documented native format** (JSON, or a zip of JSON and content-addressed blobs) | VectorCraft, PhotoCraft | ours is, in `docs/package-format.md` |

---

## 7. Smaller things worth taking as they are

- **A test that no setting does nothing** (PhotoCraft `prefs_usage.rs`): every key of the
  preferences is either read somewhere outside the preferences module or listed as hidden until
  implemented — never both; found by scanning the sources. "A setting that appears in the dialog
  but that nothing reads is a control that silently does nothing."
- **One place for everything that takes a while** (LightCraft `activity`): a registry of running
  jobs; starting one returns a guard, and dropping the guard removes the row — "also when the
  worker panics, so a dead job never leaves a stuck bar"; progress in counts, bytes or percent;
  cancel is the flag the job already checks; quitting asks while a cancellable job runs; a row
  appears only once its job is half a second old. The rule beside it: "don't add a progress
  window, panel or progress toast of your own".
- **A journal of edits beside the snapshots** (LightCraft's catalog): state changes only through
  operations; applying one returns its inverse (as ours do); operations are appended to a log as
  JSON lines and replayed after the last snapshot. A crash then loses nothing, where our recovery
  snapshot every two minutes can lose two minutes. A newer file is refused by an older build
  "instead of reading part of it".
- **Lessons made of commands** (EffectCraft `learn.rs`): a step names the control to highlight
  (by its automation id), the commands that complete it when the user does them, and the
  commands "Show me" runs. Progress belongs to the session, not the document.
- **The launch guard** (EffectCraft, LightCraft): a marker left in the settings folder from a
  launch until its window has drawn for three seconds; found by the next launch, graphics are
  switched to the safe choice and the user is told once. The marker is *locked* while its launch
  runs, so a second copy started meanwhile is not taken for a crash.
- **Logs in a file** in the settings folder as well as on standard error ("a start from a desktop
  menu has no terminal, so this file is what to attach to a bug report"), with `RUST_LOG`.
- **Help ▸ System Info**: adapter, backend, why the GPU is or is not used, versions — one thing to
  copy into a bug report.
- **Theme values re-read from a file while the program runs** (debug builds only).
- **The AppImage registers its own icon** (a hidden desktop entry under `~/.local/share`), since
  Wayland takes a window's icon from the entry named by its app id; marked as its own so it never
  touches others'; removable.
- **Version, commit and build date** in `--version` and About.
- **Every binary's release is a draft first**, with `SHA256SUMS.txt`; a launcher installs and
  updates the whole set, checking each download against those sums.
- **Fonts are never committed**: an optional build input from another repository, with every
  font's licence shipped beside it.
- **Reference files of the original are never committed** and never read into the code: behaviour
  is observed, captured outside the repository.
- **`README` screenshots are made by scripts** through the control channel.
- **A status line and a revision table on every document.**
- **A generated large project and a bench of everyday operations** (EffectCraft
  `bench --ops`: 200 compositions, 5,000 layers, 300 footage items): open, save, a steady frame of
  the window drawn headless, scrolling a long list, a small edit, a large edit, undo and redo —
  with a before/after table in the docs. It found a menu rebuilt for every row (862 ms a frame →
  under 7) and panels copying the document every frame. We found ours of that kind by accident
  (the link-drag check, 1.4 s → 0.05 ms); a generated package of a few thousand nodes with such
  a bench would find them on purpose.
- **Results keyed by what they are made of** (EffectCraft's layer cache): a layer's finished
  pixels are kept under a hash of its evaluated inputs, so editing one layer re-renders only that
  one, undo finds the earlier result again, and entries "never go stale". A **disk cache** under
  the same keys (compressed, checksummed, atomically written, least-recently-used eviction under a
  size limit, a Purge command, a command reporting its size) makes results survive a restart: a
  heavy project opens with its pictures already there. Designer recomputes a package on opening.
  (We tried an image cache in the engine once and took it out; this is how theirs is keyed, for
  if we try again.)
- **While the user drags**, the view shows "the newest completed revision while the exact
  requested revision renders; out-of-order completions cannot move it backwards"; queued work
  that is obsolete is dropped, running work finishes into the cache.
- **Lists draw, and register for automation, only the rows in view.**
- **Lazy opening**: a file carries what is needed to show the project; nothing is decoded until
  it is needed; system fonts are scanned on a background thread from launch, names only.
- **Arithmetic in every numeric field** (PhotoCraft): `1920/2`, `(30+15)*2`, `pi`; plain numbers
  apply live, arithmetic when editing finishes; the field's limits and rounding still hold; "never
  as scripts". We already have an expression parser in the core.
- **Interface scale and interface font size as two settings** (PhotoCraft): scale Auto (follows
  the system, fractional scales, changes as the window moves between monitors) or fixed 75–300 %;
  font size Tiny / Small / Medium / Large independent of it.
- **Text contrast as a test**: "every text colour holds 4.5:1 on every surface and on hovered,
  pressed and selected states (`theme.rs` tests)".
- **"A label never overflows its row"**: it wraps, or is cut with `…` and shows itself whole on
  hover — the rule we arrived at too (`view::cut`).
- **A renderer that cannot be made is an error, not a crash** (PdfCraft
  `docs/windows-renderer-startup.md`): one machine's DX12 could not compile the toolkit's own
  shader; wgpu's default handler made that fatal, so the fallback never ran and the program left
  with code 101 and no window — and "a GUI-subsystem EXE has no console to display this message".
  The creation is now wrapped so the error comes back and another renderer is tried.

---

## 8. For Brewed Texture Designer: what I would do, in order

Where we already stand (measured on 2026-10-12): the core knows nothing of the window or the GPU
and the engine nothing of the window; non-test code has no `unwrap()` and no `panic!` (57
`expect`, 4 `unreachable!`, about 465 places that index without a check); saves are atomic; the
engine's thread survives a panic and reports it; there is crash recovery, a history of versions,
a headless `--render`, the autopilot and a UI monkey; licences are checked by a test. Edits are
already data: `GraphEdit` (`AddNode`, `Connect`, `SetParam`, `SetExposed`, `SetBypass`,
`SetResolution`…) and `PackageEdit` (`AddGraph`, `Import`, `Relink`, `GraphFromSelection`,
`Batch`…).

**Step 1 — lock in what is true (a day).** A test that fails when a lower crate gains a
dependency it may not have. Clippy's `unwrap_used`, `panic`, `todo`, `unimplemented` denied in
every crate, allowed in tests. A `brewed-platform` crate for the twelve files with `unsafe`,
everything else `forbid`. A test that no setting does nothing. A node reference page generated
from the registry.

**Step 2 — a session under the window, and the command registry on it (the large one).**
First the layer itself (sections 13.1 and 17): what `App` holds that is not the window — the
shelf of open packages, the selection, the views, autosave, the library, versions, settings —
moves into a `Session` in a crate that knows no iced; what it needs from outside it is given as
services (files, settings, background work) and what it wants done it leaves as events (a
dialog to show, text for the clipboard, a link to open). The window keeps drawing and turns
its messages into calls on the session. Then one table: id, label, menu
place, key, parameters as a line of text from which the accepted keys and a schema are derived,
"can it run now and why not", run. The edits become commands almost one for one
(`graph.addNode`, `graph.connect`, `node.setParam`, `package.import`…); queries are added
(`doc.inspect`, `node.params` with each parameter's range and default, `graph.outputs`,
`package.health`). The window's menus, keys and Shortcuts dialog are built from the table; the
window sends commands where it now builds edits. A journal. A guard around each run.
`panic_hunt` over the table.

**Step 3 — `brewed-texture-cli`.** `commands`, `exec`, `get`, `set`, `run`, `render`, with
`--json` and honest exit codes. `--render` moves here; the designer keeps the window.

**Step 4 — the MCP server**, by hand, about the size of theirs: `command_list`, `command_run`,
`command_batch` (one undo step, `$N.key` references, rolled back on a failure), `doc_inspect`,
`render_preview` (any node's picture, capped), `undo`, `redo`, `--root` to confine files,
`--autosave`. A `.mcp.json` and a `docs/agents.md` with a cookbook. Scenarios through it as
tests: "build a brick wall from nothing and check the normal map".

**Step 5 — the bridge.** `--control` in the designer (loopback, a token file), `screenshot`,
`ui_inspect`; our harness already finds texts and clicks points.

**Step 6 — the GPU in the field** (section 3): DX12 alone by default on Windows or a launch
guard; short submissions for large graphs; a device-lost callback; "why not" in a System Info
window; work in flight waited for at exit.

**Step 7 — what the registry makes cheap**: sandboxed plug-ins (their ABI is a good model:
no imports, a manifest, fuel), interactive lessons, macros, an edit journal for recovery.

**Step 8 — files of others**: MaterialX first (open, and what other programs speak); `.sbs` as
an importer crate that depends on nothing of ours and returns a list of everything it could not
carry; `.sbsar` parameters only.

Steps 1–5 are the "foundation for big work" — and they are what would let an agent build a
material in our program the way one tracked a nose in theirs.

---

## 9. Where we fit beside them

- **They have no texture program, and they list ours' subject as a gap.** EffectCraft's
  `docs/gaps.md` (gap 13) and `file-format-parity.md` say After Effects 26 imports "Substance 3D
  materials `.sbsar`" with mesh displacement and that EffectCraft reads none; there is no decision
  recorded and no code. So nobody in that family has touched `.sbsar` either.
- **Our engine could be to their programs what Substance Engine is to Adobe's.** They are Rust on
  wgpu, as we are. If `brewed-texture-engine` is kept an embeddable library with a small, steady
  face — open a package, list its outputs and input values, set values, render to textures or
  pixels — a program of theirs could host a `.brewtex` material directly, with no file of Adobe's
  anywhere. That argues for keeping the engine free of anything of the designer (it is), for a
  documented package format (it is), and for publishing the core and the engine as crates.
- **Their conventions are worth matching where it costs nothing**, so an agent or a person who
  knows one of their programs knows ours: the tool names of the common MCP core
  (`command_list`, `command_run`, `command_batch`, `doc_inspect`, `render_preview`),
  `--control PORT`, `--automation-read-root` / `--automation-write-root`, a `*-cli` beside the
  program, `docs/agents.md`, `docs/gaps.md`.
- **Their code is under MIT or Apache-2.0**, so where a piece is plainly reusable — the packaging
  scripts for AppImage, Flatpak, deb/rpm, MSI and notarized DMG; the release workflow; the
  layering checker — it may be taken with its notice, which is quicker than writing it again.
  (Nothing has been taken so far.) The whole of one program's packaging is about a thousand
  lines: `linux/package.sh` (AppImage with delta updates, deb, rpm, tarball; 183 lines),
  `nfpm.yaml` (81), `AppRun` (94), two Flatpak manifests (116), `windows/package.ps1` and the
  WiX file (333), `macos/package.sh` (175). One rule from it worth remembering whatever we use:
  "Build on an old distro (Ubuntu 22.04, glibc 2.35) so the binaries run on newer ones."
- **For a custom mesh in the 3D view**, EffectCraft reads glTF/GLB (meshes, PBR materials), OBJ,
  STL and FBX 7 in pure Rust — a reference for which parts of those formats matter.
- **Colour management**: EffectCraft reads OpenColorIO configuration files from the published
  format and documentation, "not from OpenColorIO's source" — matrices, LUT files, log and
  exponent transforms, CDL, looks, displays and views — in pure Rust. Designer has OCIO/ACES;
  this shows the size of doing it ourselves.
- **Translations**: English source strings are the lookup keys; one TSV file per language;
  plural rules per language (Russian: one / few / many); switched without a restart; tests
  enforce coverage. Nineteen languages in PhotoCraft, contributed by users. Our interface is
  English only and our README Russian only: an open project will want both the other way round
  as well.

---

## 10. Sizes, for planning

How large each piece of the agent side is in their code, without tests — a measure of what the
same would cost us once the registry exists:

| Piece | Where | Lines |
|---|---|---:|
| Parsing a parameter text into accepted keys and a schema; the unknown-key check | EffectCraft `commands/mod.rs` | ~130 |
| The last-resort guard | `guard.rs` | ~25 |
| `batch` with `$N.key` references, rollback, folding into one undo step | `commands/batch.rs` | ~170 |
| The branching history | `history.rs` | ~350 |
| MCP server (protocol loop) | `automation/server.rs` | ~310 |
| MCP backend (headless or bridge) | `backend.rs` + `bridge.rs` | ~240 |
| MCP tools with schemas | `tools.rs` | ~1,000 |
| File confinement | `confine.rs` | ~280 |
| Headless autosave | `autosave.rs` | ~140 |
| Progress and cancel of long calls | `long_job.rs` | ~180 |
| **A whole minimal MCP crate** (CADCraft: execute, list, inspect, query, render, files) | `crates/mcp` | **~600** |
| The control server in the app | `control_server.rs` | ~270 |
| The automation-id registry | `ui-egui/automation.rs` | ~50 |
| Control methods (inspect, click, drag, key, type, screenshot) | `ui-egui/control.rs` | ~660 |
| The CLI | `apps/effectcraft-cli/src/main.rs` | ~1,300 |
| A sandboxed plug-in host (manifest, runtime, apply, registry) | PhotoCraft `crates/plugins` | ~1,100 |
| The layering checker with its tests | `xtask/src/layers.rs` | ~400 |
| Interactive lessons | `learn.rs` | ~525 |
| Background jobs with progress and cancel | `jobs.rs` | ~330 |

Their registries hold from 226 commands (DeckCraft) to about 2,000 (EffectCraft). Ours would
start near 60: the edits, the file commands, the view commands, and the queries.

## 11. Crates they trust

Named in their workspace manifests, with how many of the twelve apps use each: `serde`,
`serde_json`, `thiserror`, `log`, `rfd`, `egui_kittest` (all); `rayon`, `proptest`, `image`
(10); `skrifa`, `harfrust` (text shaping, 6–7); `kurbo` (curves, 6); `quick-xml` (6); `arboard`
(clipboard, 6); `zip` (5); `vello_cpu` (vector rasterizing on the CPU, 4); `krilla` (writing PDF,
4); `wasmi` (plug-ins); `boa_engine` (JavaScript, for After Effects' expressions and scripts and
PDF's scripts — a pure-Rust engine that also builds for the browser); `moxcms` (ICC colour);
`web-time` (a clock that works in the browser); `arc-swap`; `muda` (native menus); `candle`
(running neural models); `cpal` (audio); `hayro` (reading PDF); `resvg`/`usvg`; `exr`; `dhat`
(heap profiling behind a feature). All permissive. For scripting ours, `boa_engine` is the
choice they made where a known language was wanted; `wasmi` where safety was.

## 12. Their algorithms that would make texture nodes

Pointers for later, each implemented in pure Rust from published papers (named in the module
headers):

- **Filling a hole from its surroundings** — PatchMatch with multi-scale completion: PhotoCraft
  `crates/algo/src/content_aware.rs`, `inpaint.rs`; LightCraft `crates/inpaint` (deterministic:
  a fixed seed, the same result whatever the number of threads). For us: removing seams and
  objects in scans, "make it tile" for photographs, cloning patches.
- **Seamless blending** — Poisson: PhotoCraft `algo/src/poisson.rs`.
- **Denoising** — non-local means and a neural denoiser run as wgpu kernels: PhotoCraft
  `nonlocal.rs`, LightCraft `crates/denoise`, `crates/gpu/src/nn.rs`. For scans.
- **Large blurs** — running-sum box passes across all cores (a radius-180 Gaussian on 3.6 MP in
  under a second); three box passes each way equal the Gaussian on CPU and GPU alike.
- **Generators** — EffectCraft's Generate and Noise families: Fractal Noise, Turbulent Noise,
  Cell Pattern, Curl Noise, 4-Color Gradient, Grid, Checkerboard, Radio Waves, Lightning,
  Scribble, Dust & Scratches, Add/Match/Remove Grain, Median. A list to check our Noise types
  and generators against.
- **Tracing a picture into curves** and **exact booleans of curves** — VectorCraft `crates/trace`,
  `crates/pathops`: for a Shape or SVG node that combines shapes.
- **Panorama stitching, HDR merging, lens corrections** — PhotoCraft `panorama.rs`, `hdr.rs`,
  `lens`; LightCraft `crates/merge`, `crates/lensdb`: for the HDRI side of the 3D view.

---

# Part II — the code, read closely: CADCraft, VectorCraft, EffectCraft, and ours beside them

## 13. CADCraft's engine (79,000 lines, 790 tests; the window is 14 % of the program)

CADCraft is the smallest of the twelve (156,000 lines) and the plainest to read. Its `engine`
crate is half the program; `ui-egui` is 21,000 lines.

### 13.1 The session holds everything that is not pixels on a screen

`Session { docs: Vec<DocState>, active, settings, running: Option<Running>, clipboard, cursor,
log, last_point, … }` and per drawing `DocState { doc: Arc<Drawing>, undo, redo, selection,
previous_selection, path, title, saved: Arc<Drawing>, revision, views, view_history, … }`.

So the **selection, the view (zoom and pan), the clipboard, the open documents, the command in
progress and what the command line said** all live in the engine. The window translates the
pointer to drawing coordinates and calls `session.idle_click(p, shift)`, `session.input(…)`,
`session.zoom_about(…)`; it keeps nothing an agent or a test would need.

*This is the one real difference of structure between their programs and ours.* Our lower
layers match theirs; but our "session" — the shelf of open packages, the views, autosave,
settings, the library, versions, what is selected — is the `App` struct of the designer crate,
written against iced's `Task` and `Message`. It is about 8,300 lines (`app.rs` and `app/`), a
tenth of the program. Moved under the window into a crate of its own, it is the thing a CLI, an
MCP server and the window would all drive.

### 13.2 `execute` does the bookkeeping, so commands do not

```
before = (doc.clone(), selection.clone())        // two pointer copies
result = catch_unwind(|| run(session, params))   // a panic: put `before` back, report it
after_command hooks                               // groups, constraints, associativity put right
refuse_non_finite(before)                         // see below
if spec.undoable && !Arc::ptr_eq(before.doc, doc) { push_undo(label, before) }
```

- A command just changes the drawing. Whether there is an undo step is decided here, by
  comparing the pointer before and after: **a command cannot forget to be undoable, and one that
  changed nothing leaves no step**. Undo marks, groups and limits (`undoctl.rs`) sit on top of
  the same stack, "so marks, groups, Control and the stack limit apply to all of them alike; a
  command never needs to know about them".
- **The snapshot holds the selection too**: undo brings back what was selected.
- **A check after every command** (`finite.rs`): "Finite inputs can still overflow: moving by
  1e308 twice… Infinite coordinates break extents, inspection and rendering, and are silently
  changed by a save and reopen, so the command fails and the drawing stays as it was." Only the
  objects the command changed are checked — found by comparing pointers between the two
  snapshots — and every number in them, by passing them through a serializer that looks at
  numbers. *For us: one check after every edit that the graph still holds (no cycle, links to
  sockets that exist, values finite and in range), putting the package back if not.*

### 13.3 AUDIT: the document can examine and repair itself

`audit.rs`: `check` returns issues, each `{kind: a stable word, location, handle, message, fix}`;
`repair` fixes them, and "never guesses at content": a missing layer is made again with default
properties, a reference to something missing is pointed at the standard thing, a block that
contains itself loses the reference that closes the loop, objects sharing a handle get new ones.
The same report is a command (so an agent can read it) and a dialog. *We do parts of this while
loading (ids out of range renumbered) and in the Explorer's warnings; one audit of a package,
as a command with this shape of report, would gather them.*

### 13.4 Interactive commands are state machines in the engine

```rust
pub trait Interactive: Send {
    fn name(&self) -> &'static str;
    fn prompt(&self, s: &Session) -> Prompt;                       // what is asked, what is accepted, keywords, default
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step>; // Point | Keyword | Text | Pick | Enter | Cancel
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind>; // the rubber band, as geometry
    fn drag(&self, s: &Session, cursor: Vec2) -> Option<Drag>;      // "these objects under this matrix"
}
```

Every command therefore runs two ways: with JSON parameters at once (`execute`, never a dialog),
or step by step through prompts — and the step-by-step way is the same whether a person clicks,
types at the command line, or an agent sends `command_line {text: "line 0,0 @10,0 c"}`. The
window only feeds inputs and draws what `preview` returns. `drag` is there for speed: a move of
ten thousand objects is "these, under this matrix", drawn from geometry uploaded once.

### 13.5 The seam to the window is data

`render::build(drawing) → DisplayList`: polylines, triangles and points in world coordinates,
each tagged with the object it belongs to and its colour. The window uploads the list to the
GPU; the same list is drawn on the CPU for PNG export, the plot preview and the tests. The
session speaks to the window through data as well: prompts, previews, an event list.

### 13.6 A store that copies little

`EntityStore`: chunks of at most 1,024 `Arc<Entity>` behind `Arc`, and an index from handle to
chunk in 64 shards, each behind `Arc`. Cloning a drawing of a million objects copies about a
thousand pointers; changing one object copies one chunk and one shard. And because unchanged
objects are *the same pointers* in two snapshots, "what did this command change" is a walk
comparing pointers. Our graphs are behind `Arc` per graph and hold `Vec<Node>`; at a few
thousand nodes that is fine. `Vec<Arc<Node>>` would give the same trick — what changed between
two states, for free — if we ever need it for telling the engine what to recompute.

---

## 14. VectorCraft's tools (a crate of 25,000 lines that knows no window)

`vectorcraft-tools` depends on the document, geometry, colour and text crates and on nothing of
the interface. Its own summary: "pointer events in → commands and overlays out".

```rust
pub trait Tool: Send {
    fn id(&self) -> &'static str;
    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action>;
    fn key(&mut self, cx: &ToolContext, key: ToolKey, mods: Mods) -> Vec<Action>;
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay>;      // marquee, path, anchor, handle, label, measure…
    fn cursor(&self, cx: &ToolContext, p: Point, mods: Mods) -> Cursor;
    fn busy(&self) -> bool;                                    // a drag or an open path in progress
    fn claims_key(&self, cx: &ToolContext, key: ToolKey) -> bool;
    …
}
pub enum Action {
    Begin(String),            // start: snapshot the document; the label is the undo name
    Preview(String, Value),   // apply this command to the snapshot, replacing the last preview
    Commit,                   // keep the last preview as one undo step and one journal entry
    Cancel,                   // put the snapshot back
    Exec(String, Value),      // a command at once
    Dialog(String, Value), SwitchTool(String), Notify(String),
}
```

"Tools never mutate the document directly… This makes every gesture replayable by the control
channel and MCP, and keeps tools testable without a UI." Pointer events are plain data
(`{kind, pos in document coordinates, mods, pressure}`) and can come from JSON. What is drawn
over the canvas while a tool works is a list of `Overlay` values; the pointer's shape is a
`Cursor` value. PhotoCraft and DesignCraft do the same; CADCraft's `Interactive` is the same
idea for a command line.

**Begin / Preview / Commit against our merge keys.** Ours folds edits with the same key made
within 1.5 s into one step. Theirs says outright where a gesture starts and ends, and each
preview is applied *to the state at the start*, not on top of the last preview: nothing can
drift, Escape in the middle is "put the snapshot back", and a pause of two seconds in the
middle of a drag does not make two steps. Ours is simpler and has served; theirs is the more
exact, and the one to move to when edits become commands.

Our graph view is the same shape in small — an `Interaction` state machine that publishes
`GraphMessage`s — but written inside iced's canvas program, 1,450 lines in all. It does not
need to move for agents (an agent connects two nodes with a command, not by dragging); it would
only if we wanted gestures replayed.

---

## 15. EffectCraft's document, and its GPU compositor beside our engine

- **Addresses.** Everything that can be set is a property in a tree, addressed by a path:
  `transform/position`, `effects/#1/blurriness`, `masks/#1/feather`, or `@uid`. Effects, masks
  and shape contents are groups in the same tree. For us: `nodes/<id>/<param>`,
  `inputs/<name>`, `outputs/<name>` — one way to name anything, for `get`, `set` and expose.
- **A save is read back before it is written**: "`to_file_json` checks that the text reads back
  before anything is written (JSON can't hold NaN or infinity), and files are written
  atomically". A bug in writing then cannot destroy the file it was about to replace. *Ours
  does the same (checked): `project.rs` writes only JSON that "reads back as it was (else the
  file is not written)".*
- **Two version marks**: `savedBy` (the program's version) and `schema`, which "changes when a
  saved value changes meaning"; an older file is converted on opening, and an older program
  refuses a newer schema "rather than render wrongly". What the editor had open (the active
  composition, the tabs) is saved beside the project and is not part of it — as our
  `editor.json` is.
- **What the session asks of the window is a list of events**: `ProjectChanged {revision}`,
  `Toast`, `OpenUrl`, `CopyImage`, `Frontend {command, params}` (a command only a window can
  do), `PurgeCaches` — "headless sessions ignore these". That is how a session stays free of
  clipboard, dialogs and browser.
- **The GPU is an accelerator of a reference, by a trait**: `Accelerator { comp_frame, 
  supports_effect, effects(chain), raster_3d, render_3d, particles, auto_pick }`, each returning
  `None` for "not handled — the CPU does it". `Backend::Auto` *measures* which compositor is
  faster for each composition and remembers.

The compositor itself (`crates/gpu`, 19,600 lines of Rust and 14,800 of WGSL in 28 files) next
to our engine (12,200 and 3,500):

| | Theirs | Ours |
|---|---|---|
| Kernels | compute shaders, one module and one WGSL file per family of effects | fragment kernels by name with variants per output format; kernels generated from text (Pixel Math, FX-Map) checked before use |
| Working textures | a pool; a released texture is reused once every encoder that could read it is submitted | a pool of scratch images, trimmed to a budget, its bytes reported |
| First use | pipelines made on a background thread at launch | the same (`warm_up`) |
| To the screen | `render_display` leaves a texture for the window, skipping the readback | the engine and the window share one device; the views sample the engine's textures |
| Reading back | deferred readbacks keyed by content, for the browser | `Readback::wait`, bands for results over the buffer limit |
| A reference to check against | the CPU renderer, every effect | none: tests on a software GPU, compared within ±1 |
| Chains | a chain of GPU effects is uploaded and read back once | results stay on the GPU between nodes |
| 3D | a raster pipeline with PBR, image-based light, shadow maps, depth of field, and a CPU rasterizer as reference | a preview: PBR with an HDRI (SH irradiance, prefiltered specular), displacement, MSAA |

In kind, the two are the same machine. Theirs is far larger because it stands in for a whole
compositor; ours has one thing theirs lacks (one device for engine and window from the start)
and lacks one thing theirs has (a reference to fall back on).

---

## 16. Ours beside theirs, honestly

Measured, not felt. "Theirs" is what all twelve programs share.

| | Theirs | Ours | |
|---|---|---|---|
| Document model free of window and GPU | yes, checked by a tool | yes (`core`: serde, flate2, sha2, rayon), not checked | **same** |
| Evaluation free of the window | yes | yes (`engine` has no iced) | **same** |
| Undo | whole states sharing what did not change; labels; merging of a gesture | the same (`History<(Package, GraphId)>`, graphs behind `Arc`, merge keys, labels, seal) | **same** |
| Edits as data | commands with JSON | `Edit` / `PackageEdit` enums | **same in substance**; theirs have ids and schemas |
| What to compute again | a key per layer: a hash of its *evaluated* inputs (`render/src/cache.rs`); only what has a new key is rendered | a key per node: "a hash of its type, effective parameters, resolution and the versions of its inputs. Only nodes whose key changed are recomputed" (`engine.rs`) | **same idea**; theirs also keeps earlier results under their keys, in memory and on disk |
| Parameters described as data, the panel generated from them | `ParamSpec { id, name, default, ui }`, with a function saying which are shown | `ParamDef` with the kind and range, `visible_if` and `gray_if` as data, `limits`, `exposable`, ids for menu options in files, options shown as pictures, search words, groups, tips | **ours carries more** |
| Atomic saves | temp, fsync, rename; failure-injection tests | temp, fsync, rename, directory fsync, retries on Windows | **same** |
| Crash recovery | autosave to a recovery folder | recovery sessions with locks, plus a history of versions per file | **ours does more** |
| File format | JSON, or zip of JSON and blobs; versioned; unknown parts kept in some | zip of JSON and content-addressed files; a written specification; unknown entries and `extras` kept; deterministic bytes | **same or better** |
| Panics | lints forbid them; a guard around every command | none in non-test code by habit (0 `unwrap`, 0 `panic!`), not by lint; the engine thread guarded, the window's not | **nearly**; theirs is enforced |
| `unsafe` | one named crate per program | twelve files of the designer | **theirs** |
| A session under the window | yes — the largest crate of each program | no: `App` in the designer, bound to iced | **theirs — the one structural gap** |
| Commands, CLI, MCP, control port | all twelve | `--render` only | **theirs** |
| Tests through a public door | QA scenarios over MCP | the autopilot and the harness, through the window's own messages | different doors |
| Random sequences of actions with invariants | I found none by search (they run each command once with hostile parameters, and property-test their file formats) | a UI monkey (seeds past 5,500: stale state, cut text, save/load and undo/redo round trips) and an autopilot that times every change | **ours** |
| Several displays and scales | handled, and documented | handled, with tests of placement on any set of screens and of the 3D view at six scales | **same or ours** |
| Own windows for tools (Color Editor) | EffectCraft moves a panel "into its own native window on any screen"; the shared docking library leaves native windows to each program | real windows of the system, placed on Windows, X11 and macOS | **same** |
| Engine and window on one GPU device | listed as an open item in LightCraft; a display texture in EffectCraft | from the start | **ours** |
| A save is read back before it replaces the file | yes (EffectCraft) | yes (`project.rs`) | **same** |
| Limits on what a file may make the program allocate | per format: sizes, counts, nesting, unpacked bytes | `archive.rs`: at most 100,000 entries, every entry read under a byte limit *while* it is unpacked, a test with a bomb; budgets per kind of entry | **same** |
| Logs a user can send | a file in the settings folder, Help ▸ Reveal Log File, a System Report | standard error only — nothing to send from a program started by a double click | **theirs** |
| Out of video memory | caught, the pool emptied, one more try, then a sentence | written to the status bar as whatever the driver said | **theirs** |
| CI, releases, installers | every system, signed | none | **theirs** |
| Translations | up to nineteen languages | English | **theirs** |
| Size | 157,000 to 490,000 lines each, many agents in parallel | 80,000 lines | — |

What this says: **the decisions that are expensive to change later were made the same way in
our program as in theirs** — a document that knows no window, edits as data, undo by shared
states, atomic writes, a documented format, a GPU engine apart from the interface. Those are
the ones their own architecture document warns about ("each would force a rewrite later").
What ours lacks is of two kinds: things built *on top* of that base, which the base makes
possible (the registry, the CLI, MCP — and the session layer they need), and things *around* it
that a project gets when it is opened to the world (CI, installers, translations). Neither kind
means the base was wrong.

And one thing in fairness to both sides: their rules are written down and checked by machines
because many agents write the code at once, and a rule nobody checks does not survive that.
Ours has held by attention. Opened to contributors, ours will need the checks too (section 8,
step 1).

---

## 17. How their engines reach the outside world

An engine that knows no window still has to read files, keep settings, export, run scripts. In
EffectCraft every one of those is a trait the engine *defines* and somebody else implements:

| Trait (in `engine`) | What it stands for | Who implements it |
|---|---|---|
| `Services` | read a file, write one (atomically), exists, size and time, list a folder | `FsServices` on a computer; a table of picked files in the browser; memory in tests |
| `ConfigStore` | named pieces of text: `prefs.json`, `shortcuts.json`, the launch marker | a folder in the system's settings place; the browser's storage; `MemoryConfig` in tests — or none |
| `FileOps` | list, write, remove: what auto-save and recovery need | the file system; browser storage |
| `Importer`, `Exporter` | probe a media file; render a queue to files | the media and export crates (FilmCraft's codecs) |
| `Offload` | run a job somewhere else | threads; Web Workers |
| the script runner, the expression host, the plug-in loader | function pointers and `Arc<dyn …>` | the script, expression and plug-in crates |

A small crate, `host` (about a hundred lines of wiring), builds "a fully wired `Session`":
`Session { exporter: Some(…), importer: Some(…), expr: Some(…), script: Some(…),
plugin_loader: …, ..Default::default() }`. The desktop app, the CLI, the MCP server and the
browser all start from it. **Every service is optional**: a bare `Session::new()` works, which
is what most tests use, and what keeps the engine compiling for the browser.

Going the other way, the session never calls the window; it leaves **events** for whoever is
listening (`ProjectChanged {revision}`, `Toast`, `OpenUrl`, `CopyImage`, `Frontend {command}`,
`PurgeCaches`), and a headless caller simply drops them.

For us this is the recipe for lifting the session out of the designer: what `App` does through
iced today — file dialogs, the clipboard, opening a link, spawning background work, the settings
file — becomes either a service the session is given or an event it leaves.

**Their programs already consume each other as libraries.** EffectCraft's `Cargo.toml` lists
fourteen FilmCraft crates (codecs, media, colour, time, interchange) as git dependencies pinned
to one commit. So "another program hosts our engine" needs no new mechanism on their side: a
line in a manifest.

## 18. Small things seen in CADCraft's window

- **An overlay must take the pointer**: "Every overlay registers a widget that senses clicks
  *and* drags, so the toolkit hands the pointer to the overlay instead of the canvas underneath
  (a click on the ViewCube must never also pick or start a selection window in the drawing)."
  And what an overlay does goes through commands.
- **One source of truth for what is being typed**: the value boxes beside the cursor ("Dynamic
  Input") are "a view of" the command line's text, not a second state.
- **Typing while dragging**: `40` Tab `90` Enter places a point 40 units away at 90°; a typed
  value "locks" and holds the cursor. For us: typing a number while a slider or a node is being
  dragged.
- **A palette of the selected object's main properties near the cursor** (Quick Properties).
- **Settings by name** (`sysvars`): every setting can be read and set by its name from the
  command line, a script or an agent — the same table the dialogs edit.

---

## 19. Features seen in the three nearest programs that would suit ours

From their command tables (EffectCraft's has some forty groups — `layer`, `view`, `file`,
`keys`, `comp`, `track`, `mask`, `renderQueue`, `help`… — and the groups say what a program can
do at a glance).

**The views**

- **A snapshot to compare with** (`view.takeSnapshot`, `view.showSnapshot`): keep what the view
  shows now, go on working, and flip back to it — before and after a change, without undoing.
  For the 2D view of a node and for the 3D view.
- **A working resolution that is not part of the document's history** (`view.res.full | half |
  third | quarter | auto | custom`): the picture is computed at a fraction of its size while one
  works; it is saved with the project but "setting it is no undo step", and results cached at
  the full size stay valid. For us: compute the whole graph at ½ or ¼ while editing a heavy one,
  with one switch, without touching the graph's Texture size.
- **Exposure of the view** (`view.exposure`, `view.resetExposure`): for pictures whose values
  go past 1 (heights, HDR) — the view's, not the picture's.
- **A region of interest** (`view.setRegionOfInterest`): compute and show only a rectangle.
- **Named views of the 3D scene** (`view.3d.front | back | left | right | top | bottom`, three
  custom ones with their own keys, `lookAtAll`, `reset3DView`).

**Files**

- **Increment and Save** (`file.incrementAndSave`): `wall_003` → `wall_004` in one key — what
  pipelines that keep every version on disk use.
- **Save a Copy**, **Revert** (back to the file as saved), **Reveal in the file manager**.
- **Watch Folder** — "render farm without a farm": the program watches a folder for projects
  with something queued, renders them, and writes `<project>.status.json` (rendering, then done
  or failed, with each output's path) beside each; a project with a status file is not taken
  again. One pass is a command, so the CLI and tests use the same code. *For us: a folder of
  packages whose outputs are kept up to date — a build step for a game's textures.*
- **A template with its controls, rendered from the command line**: `effectcraft-cli render
  --template lower-third.ectemplate --values values.json --out 'title_[#####].png'`; unknown
  controls and wrong values "fail before rendering". Ours is `--render PACKAGE --set NAME=VALUE`;
  a file of values is the missing half.

**Help**

- **System Info and a System Report** (`help.systemInfo`, `help.systemReport`), **Reveal Log
  File**, **Enable Logging**, **Report an Issue** (opens the tracker with the versions filled
  in), `app.capabilities` (what this build can do — which codecs, whether plug-ins, whether the
  GPU).

**Editing**

- **Undo brings back the selection** (CADCraft: the snapshot holds it).
- **A command palette** of every command with its key — it falls out of the registry.
- **Keys as presets**: export and import a set of shortcuts (`shortcuts.*`).
- **A value typed while dragging** (CADCraft's Dynamic Input).
- **AUDIT** — check and repair, with a report (section 13.3).

**Reuse**

- **Named presets of an object's settings** (VectorCraft's Graphic Styles, 17 commands): save
  how a thing is set up under a name, apply it to others, keep libraries of them. For us: presets
  of a node's parameters, and of a graph's input values — Designer has the second.
- **Data sets** (VectorCraft `variable.*` / `dataset.*`, PhotoCraft `doc.importDataSets`): a table
  (a CSV) whose rows are sets of values; one export per row, named by a pattern
  (`{name}|{index}|{document}`). For us: a table of input values → a family of textures from one
  graph, in one command.
- **Swatches and colour themes** as a panel and a library.

**Colour** (VectorCraft's `color` crate)

- **Harmony rules**: from one colour, the colours that go with it — complementary, split
  complementary, analogous, triad, tetrad, monochromatic, shades, and a dozen more — each a
  small table of "turn the hue by this, scale saturation and brightness by that". For the Color
  Editor, and for making the stops of a Gradient Map from one colour.
- **Palette files read and written**: `.gpl` (the plain-text palette GIMP, Krita and Inkscape
  share), `.ase` (swatch exchange), and their own JSON. PhotoCraft reads gradient presets as
  well. For us: a palette or a gradient file dropped on a Gradient Map becomes its stops.
- "Colours keep the model the user picked them in… so documents don't drift when converting
  back and forth" — the rule we reached for black keeping its hue.

---

## 20. What a command looks like — theirs, and ours as it would be

One of theirs, whole (EffectCraft, `commands/layer.rs`):

```rust
fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let (cid, lid) = layer_p(s, p, "layer.rename")?;                 // `layer` if given, else the selected one
    let name = str_p(p, "name").ok_or_else(|| bad("layer.rename", "missing `name`"))?.to_string();
    s.edit("Rename Layer", None, |proj, _| {                          // label of the undo step, no merge key
        layer_mut(proj, cid, lid)?.name = name.clone();
        Ok(())
    })?;
    Ok(Value::Null)
}
// and one line of the table:
cmd!("layer.rename", "Rename", ["Layer"], Some("Enter"), "{layer?, name}", has_layers, rename)
```

Three little helpers carry most of it: `str_p` / `f_p` / `b_p` read a parameter; `layer_p`
turns "an id, `#n`, a name, or nothing = the selection" into a target, with an error that
names what was not found; `bad(command, message)` makes the error.

Ours would be shorter still, because the edit and its checks exist:

```rust
fn set_param(s: &mut Session, p: &Value) -> Result<Value> {
    let (graph, node) = node_p(s, p, "node.setParam")?;              // `node`: id or name; `graph?`: name, else the open one
    let key = str_p(p, "param").ok_or_else(|| bad("node.setParam", "missing `param`"))?;
    let value = value_p(s, graph, node, key, p.get("value"))?;       // read by the parameter's kind; out of range is an error
    s.apply(graph, Edit::SetParam { node, key: key.into(), value })?; // the edit we have today
    Ok(json!({}))
}
cmd!("node.setParam", "Set Parameter", [], None, "{node, param, value, graph?}", has_graph, set_param)
```

and `node.params {node}` would answer with what `ParamDef` already holds — kind, range,
default, the options of a menu, whether it can be exposed, whether it is shown now — which is
exactly what an agent needs to know before it sets anything.

---

# Part III — the other nine, by what recurs

## 21. Checkers that return a report

The same shape appears in five programs, each time as a library function, a command, and a
dialog over the same report:

| Program | Checker | What it returns |
|---|---|---|
| CADCraft | `audit::check` / `repair` | issues `{kind, location, handle, message, fix}`; repairs "never guess at content" |
| PdfCraft | `preflight::verify` / `convert` (PDF/A) | issues against a standard; `convert` "fixes what can be fixed without changing how pages look … and returns what remains" |
| PdfCraft | `a11y::check` | 32 rules in seven categories, each **Passed / Failed / Needs manual check / Skipped**, with the elements at fault; an HTML report |
| PdfCraft | `audit` | finds redactions that only cover the text instead of removing it |
| SoundCraft | `session.audio_health` | what is missing or mismatched, with a suggestion for each |
| EffectCraft | `footage.check`, `effect.warning` | missing media; what an effect would warn about |

The rules of the pattern: every finding has a **stable word** for its kind (so scripts and
agents can act on it); the check **only reads**; the fixer is separate, fixes only what cannot
change meaning, and says what is left; "needs a person to look" is an honest fourth answer.

Two places for it in a texture program:

- **An audit of a package**: links to nodes or sockets that are gone, graphs that use
  themselves through others, files that are missing or unused, parameters outside their limits,
  ids used twice, input values nothing reads.
- **A preflight of an export**, per target (Unreal, Unity, glTF…): sizes that are not powers of
  two, a normal map that is not of unit length or has the wrong green, values outside 0–1 where
  the format clips, a map the target expects and the graph does not give, a colour map marked
  as data or the reverse, a picture that does not tile where it is meant to. Our Export window
  already says what it substitutes for a missing map; this is the same thing made general.

## 22. Keeping a saved file meaning the same thing

For a program whose files are *recipes* — theirs for motion graphics, ours for textures — a
later version must draw an old file as it was drawn. How EffectCraft holds to that:

- **Instances are refreshed from the registry when a file is opened** (`effects/src/migrate.rs`,
  `upgrade_instance`): a parameter whose id changed is found through a table of
  `(effect, old id, new id)`; parameters added since are created with their defaults; a menu
  value is remapped **by its label** when the list of options changed order, with a table for
  renamed labels. "Animation, expressions and values are kept."
- **A `schema` number that "changes when a saved value changes meaning"**, with a conversion
  written for each step, and older programs refusing newer files "rather than render wrongly".
- **An effect whose result must change keeps its old self** under "(Legacy)" or in an Obsolete
  group, so old projects still find what they were made with.
- **Pinned results** (`effects/tests/sim_golden.rs`): each simulation effect is rendered at
  default and other settings, several times, two bit depths and two sizes; every frame's pixels
  are hashed and compared with hashes written in the test — "any change to the CPU output of
  these effects, however small, fails the test". An intended change prints the new table
  (`SIM_GOLDEN_PRINT=1`). They found the catch too: the bits depend on the system's maths
  library, so the hashes are pinned for one system, and elsewhere the test checks only that
  each case comes out the same twice.
- **Golden pictures** (FilmCraft's `golden` crate): small made-up projects rendered by the CPU
  reference and compared with PNGs kept in the repository (`FILMCRAFT_BLESS=1` writes them
  anew); the GPU is then compared with the CPU.

VectorCraft's native format (`crates/format`) adds three ideas about files themselves:

- **Any earlier version can be written**, for someone on an older program (`save_with`); every
  version is read; a newer one is refused.
- **A second form of the content travels in the file for programs that cannot read it**: with
  "Create PDF-Compatible File" the file carries a PDF of every artboard. *For us: a package
  could carry its outputs already computed — pictures any program can read — beside the graph
  that makes them: a texture set for whoever has no engine, a recipe for whoever has.*
- A picture that is only linked is saved as a small stand-in unless "Include Linked Files" is
  asked for; the linked file is read again on opening.

Where we stand: menu options are saved by stable ids, not by place — better than remapping by
label; `tests/fixtures/nodes.txt` fails when a node's type, parameters, options or sockets
change without being meant to; files carry a version and migrations. What we do not have is
the last line of defence: **a small picture of every node at its defaults (and a few other
settings) kept in the repository and compared within a step or two**, so that a change to a
shader which changes what users' materials look like cannot pass unnoticed. With no CPU
reference, those pictures would be our reference.

---

## 23. Traps of the Linux desktop they fell into (we use the same libraries)

Each is from the code of PhotoCraft's desktop app, with the issue that found it. They matter the
more as this program's own author is moving to Linux.

- **A file dialog that never appears, and says nothing** (`file_dialog_probe.rs`): the dialog
  library we both use (`rfd`) "talks to the XDG file-chooser portal through `libdbus-1`, which
  it loads at runtime, and falls back to `zenity`. When neither works (a minimal window-manager
  session without a portal backend) it logs the failure and answers 'nothing picked', which the
  app took for Cancel: the menu item did nothing and said nothing." They look for the pieces
  once, in the background, and when a dialog comes back empty and none of them is there, say
  what to install. "Anything it can't tell counts as present, so a working system never sees
  the message."
- **Libraries loaded only when the window opens** (`linux_libs.rs`): the windowing and GPU
  libraries (X11/Wayland, xkbcommon, Vulkan, EGL) are not listed in the executable; winit and
  wgpu load them at run time, "and some of those crates panic when a library is missing".
  Before the window is made they read the dynamic linker's cache (`ldconfig -p`) for what the
  session needs and, if one is missing, "print the package to install and exit non-zero instead
  of crashing". Where the cache cannot be read (musl, NixOS, containers) they start silently.
- **Files dropped on the window on Wayland do nothing**: "winit 0.30, the windowing library
  under egui, has no Wayland drag and drop" — and it is under iced 0.14 too. They show a notice
  on Wayland with the command to start under XWayland, and offer paste from the file manager
  instead. *Checked in the source of the winit we build with (0.30.13): its X11, Windows and
  macOS parts send the dropped-file event; its Wayland part has no code for it. Our program
  opens a package, makes a Bitmap node or links a font from a file dropped on the window
  (`App::file_dropped`) — so in a Wayland session that does nothing, and says nothing. Not yet
  seen with my own eyes in the VM.*
- **A pen on Wayland gives no pressure** through winit, so with a pen attached their window
  opens through XWayland by default.
- **The window's icon on Wayland** comes from a desktop entry named by the app id; an AppImage
  installs none, so it writes a hidden one for itself (section 7).
*Checked for ours: the main window is made with no application id on Linux —
  `platform_specific` is filled in only for Windows (`app.rs`), and iced hands winit the empty
  default as the Wayland app id and the X11 class. A window icon is given, which X11 uses; on
  Wayland the shell will show a generic icon and cannot tie the window to a launcher. One line
  now (`application_id`), and a desktop entry of the same name when there is a package.*
- **Following the system's light or dark appearance**: read from the desktop portal once, then
  its `SettingChanged` signal — "no polling, and the UI repaints only when the value changes".
- **Fonts in a Flatpak**: the sandbox sees only some font folders; they read the fontconfig
  configuration for the rest and document the override that grants another folder.
- **System fonts**: the folders `/usr/share/fonts`, `/usr/local/share/fonts`,
  `~/.local/share/fonts`, `~/.fonts`, `$XDG_DATA_HOME/fonts`, `fonts` in each `$XDG_DATA_DIRS`,
  and every `<dir>` the fontconfig configuration names.
- **Quitting while the GPU works** crashes inside NVIDIA's Vulkan driver on Linux (section 3).

## 23a. The window itself

- **A window without the system's title bar, on Windows** (PhotoCraft `window_frame.rs`; we
  draw our own title bar the same way, `decorations: false`): "winit 0.30 gives an undecorated
  window the `WS_CAPTION | WS_BORDER` styles and hides the non-client area… On some systems
  (reported: Windows 10 LTSB, Intel HD Graphics 4600, 1366 × 768) the desktop compositor still
  reserves that frame: the content is drawn offset by the caption and border, leaving black
  bars at the window's top-left, while the pointer keeps the un-offset client coordinates, so
  it no longer points at what is drawn under it." Their cure strips the two styles and checks
  every frame (winit puts them back) — and that cure made a second fault: a maximized window
  without a caption covers the taskbar and the shell takes it for a full-screen program. *If a
  user of ours ever reports black bars at the top left with the pointer off by a title bar's
  height, this is it.*
- **Where the window was** (EffectCraft `window.rs`): size, position and "maximized" kept in
  the settings folder; restored "moved onto a monitor that is still connected"; the first launch
  "fits the window to its monitor, opening it maximized where the default size leaves no room
  for a taskbar or Dock" — the rule we wrote as `fitted_window`. Position is `None` "where the
  system doesn't say (Wayland)".
- **Portable mode** (PhotoCraft `app_dirs.rs`): a marker file beside the executable
  (`portable.txt`) puts every setting and recovery file in a folder beside it, "so a portable
  copy on a USB stick leaves nothing in `%APPDATA%`"; the Windows zip ships with the marker,
  the installer does not; a read-only place falls back with a warning.
- **One variable moves all of the program's own files** (`PHOTOCRAFT_CONFIG_DIR`): for tests,
  agents and odd setups.
- **On a Mac, a file opened from Finder is not on the command line** (PhotoCraft
  `apple_events.rs`): "Finder double-clicks, Open With, drops on the Dock icon and `open -a … 
  file` don't pass paths on the command line: [the system] sends the running (or just-launched)
  app [an] Apple event. winit 0.30 doesn't handle it." They use a small audited crate that
  registers the handler. *For ours this bites the day there is a Mac bundle with `.brewtex`
  tied to it: the program would start, empty.*
- **Quitting on a Mac** (PhotoCraft `mac_window.rs`): letting the toolkit close the window left,
  on some Macs, "a windowless app that only Force Quit ends"; they quit through the system's own
  `terminate:`, scheduled so the toolkit is out of its event handler first.
- **File dialogs differ by system in how they may be shown**: "Linux shows dialogs on a thread
  of their own, macOS as sheets" on the window; a blocking dialog run inside the frame handler
  on a Mac aborted the program when an event the toolkit had held back arrived.
- **Which GPU**: on machines with two — a laptop's built-in and discrete, or a desktop whose
  monitors hang off different cards — several of their issues were the wrong one being chosen:
  "the integrated GPU without a monitor crashed its driver and took every monitor down"; "a
  Wayland compositor may not show frames from another GPU". *Checked for ours: iced asks for a
  high-performance adapter compatible with the window's surface, which is the safe rule.*

## 24. Queues of small jobs (LightCraft's `preview` crate, 1,250 lines)

- **A pool with slots** (`pool.rs`): "Each job targets a *slot* (a grid cell, the loupe…).
  Submitting for a slot replaces a job for that slot that hasn't started yet, so scrolling or
  dragging never builds a backlog. Workers take the highest priority first (newest first among
  equals)." A finished job carries the key it was submitted with, "to drop stale results".
  Dropping the pool discards what is queued and waits, bounded, for what is running.
- **A cache with a generation**: clearing bumps it; "jobs captured before that clear cannot
  repopulate it".
- **Keys of what is kept on disk** are `(content hash, settings hash, size, renderer version)`
  — the version of the program that drew it is in the key, so a new version never shows an old
  version's pictures.
- **128-bit hashes of file contents** double as duplicate detection.

For our thumbnails (the nodes', the Explorer's, the Library's): the slot is the node or the
file; a newer request for the same slot replaces the queued one; what is visible goes first.

---

## 25. Measuring the distance to the original

How every one of their programs knows, as a number, how far it is from the program it stands in
for (SoundCraft is the plainest example):

- A text file, one line an item: `crates/engine/catalog/menus.txt` — "the incumbent's menu tree
  (feature names observed black-box)", 512 lines such as `File > Import > Audio...`.
- A command is declared with its menu path and label; "a menu item lights up when a command has
  the same menu path and label (or an alias maps it)". The menus of the program are *built from
  the catalog*, so an item nobody has written yet is there, greyed.
- `cargo xtask parity` writes `docs/parity-checklist.md`: "Menu items implemented by the engine:
  397 / 512 (77.5 %)", a table per menu, and the list of what is not yet there.
- A test holds a floor that is only ever raised.
- They are plain that this number "measures wiring, not behaviour"; behaviour is judged
  separately, dimension by dimension, with weights (`docs/target-app-parity.md`).

For a program meant to stand in for Substance Designer the catalog would be two lists, both of
names that Adobe's public documentation gives: the **nodes of the library**, by category, and
the **menus**. Our node registry already has every node's name; an alias table says which of
ours answers which of theirs ("Tile Sampler" is ours by the same name; their Quadrant / Iterate
/ Switch graph is our FX-Map's program). The page that comes out — so many of so many nodes, so
many menu items, and the list of what is missing — would replace the list I keep by hand, and
would be the honest answer to "how much of Designer is this?".

---

## 26. What their own measurements taught them

Their documents are frank about themselves, and what they say is worth more to us than any
feature.

- **"Does the feature exist?" and "can someone do real work with it?" are two numbers.**
  EffectCraft: feature breadth ≈ 91 %, ready for real work ≈ 45 %. PhotoCraft: breadth ≈ 76 %,
  ready ≈ 45 %; for someone who needs only the essentials ≈ 61 %.
- **The first estimate was far too kind.** PhotoCraft, a week in, called itself "82 % of the
  way" with 140–220 hours to go. Then people used it: "about 1,230 issues have been filed since
  the first release… most of them against features already counted as present." The estimate
  became 2,300–4,100 hours. EffectCraft: "An outside contributor found four bugs in features
  marked done within a day; users have found well over a hundred more since."
- **A checklist written by oneself measures oneself.** "The self-graded 92-item checklist says
  ≈ 99 %; measuring against After Effects itself, rather than our catalogue, finds what that
  checklist never listed."
- **What holds "ready" down is not features**: files of the original that cannot be opened
  ("Opening existing After Effects work ≈ 0 %"), "behaviour nobody has checked against" the
  original ("≈ 50 %, mostly unmeasured" — one comparison done so far, 168 samples), "reliability
  outside macOS", and the plug-ins and presets real projects depend on.
- **So their order of work puts fidelity, reliability and stability before new features**, and
  every closed issue comes with a regression test ("179 issues were closed with regression tests
  between 5 and 10 October").

- **How fast, and at what cost.** PhotoCraft's progress log: "2026-09-30 First commit";
  "2026-10-01 Menu breadth 224 → 532 of 625 in a day"; by 10 October 1,004 merged requests and
  490,000 lines. And its own summary of the state that speed bought: "users of 0.5/0.6 have 579
  open issues, 22 of 25 performance budgets are missed, and PSDs we write are not yet reliably
  accepted by Photoshop"; next on the list is "the 150-layer GPU crash". Breadth can be bought
  with many agents in two weeks; what cannot is each thing having been tried by someone who
  cares how it feels. Ours has been built the other way round — narrower, each step looked at —
  and that is neither better nor worse as a method, but it is why a comparison by size says
  little.

For us, read plainly: the day this program is opened to others is the day its real list of
faults begins, and that list will be about what already exists. The same four things will hold
our "ready" down — Designer's files that cannot be opened, nodes that do not behave quite as
the ones people know, machines unlike this one, and the library of ready materials people
expect. None of that is a reason to wait; it is a reason to have the registry, the tests through
it, the pictures pinned per node and the launch guard *before* the door opens, because those
are what turn a report from a stranger into a test.

---

## 27. What "a replacement" is taken to mean, and how behaviour is measured

**Gates, not a percentage** (EffectCraft `docs/fidelity/ae-2026-acceptance.md`): "Feature
presence, internal CPU/GPU agreement, and a polished interface are separate evidence; none
establishes this target on its own." Each gate names the evidence it needs and where things
stand today; "maintain per-case pass/fail/untested results, tolerances and repeatable commands;
do not replace them with a global parity percentage."

| Their gate | Evidence they ask for | The same gate for a Designer replacement |
|---|---|---|
| Work projects | representative projects recreated or imported, edited, saved, reopened and delivered without lost state | representative materials rebuilt or imported, edited, saved, reopened, exported |
| Workflow | pointer and keyboard cases for every panel, tool, shortcut, undo, dialog and selection | the same |
| Animation | values sampled from the original for each kind of property | **node fidelity**: each node's picture at set parameters against the original's |
| Render fidelity | test scenes compared at each bit depth and colour space | graphs compared at 8 bits, 16F and 32F; colour and data maps; tiling |
| Media and delivery | results "checked in downstream tools" | the exported maps opened in Unreal, Unity, Blender, a glTF viewer |
| Performance | matched timings, warm and cold, memory and VRAM, long sessions | the same, on set graphs at 2K, 4K, 8K |
| Platforms | "native Windows, macOS and Linux runs with recorded GPU, driver, backend, device limits and fault cases" | the same |
| Recovery | "save/reopen, interrupted render, invalid input, exhausted resources, lost GPU and cache failures without silent corruption" | the same |

**Behaviour measured against the original, without taking anything of it**
(`docs/fidelity/README.md`): the *same script of their own* is run in both programs — the
original through its documented scripting interface, theirs through the CLI — the numbers are
recorded and compared within a stated tolerance. "Raw host observations … stay in [an ignored
folder]. Only the original harness and aggregate scores are tracked. No Adobe artwork, presets,
screenshots, frames or implementation code are included." The first run found a real fault at
once (an easing rule normalized where the original does not: an error of 12 degrees), fixed to
within 3e-11.

For us the same is possible by whoever owns Designer: its Python interface and `sbsrender` can
render a node at set parameters to a file; our CLI renders ours; a script compares. Only the
script and the scores would be kept — never Designer's pictures. That is how "follows Substance
Designer in essence" could become, node by node, a measured statement.

---

## 28. Crates of theirs that could be used as they are

Their format and algorithm crates are written to stand alone ("no workspace deps;
publishable"), are under MIT or Apache-2.0, forbid `unsafe` and panics, and their own programs
already take each other's by a pinned git revision. Nothing stops ours doing the same, with
their notice kept. Checked: what each needs besides itself.

| Crate | Lines | Depends on | What it would give us |
|---|---:|---|---|
| `lightcraft-inpaint` | 845 | **nothing** | filling a hole from its surroundings (PatchMatch): `complete(w, h, channels, &[f32], hole: &[bool], params) -> Option<Vec<f32>>`, deterministic. A Content-Aware Fill node; seams and objects taken out of scans; tiling a photograph |
| `photocraft-cms` | 5,300 | **nothing** | ICC colour management in pure Rust: profiles, transforms, the four intents, soft proof. Reading the profile of an imported picture; a colour-managed view |
| `effectcraft-model` | 4,300 | its `geom` (500 lines), `serde`, `image`, `miniz_oxide` | glTF 2.0 / GLB, OBJ with MTL, STL, FBX 7 → meshes with materials. **A custom mesh in the 3D view** without writing a parser |
| `filmcraft-color` | 2,500 | `serde` | transfer functions, camera log curves, gamuts, LUT files (`.cube`) — a LUT node; colour spaces |
| `photocraft-psd` | 16,100 | `thiserror`, `flate2`, `serde` | reading and writing PSD/PSB losslessly: layers of a PSD as pictures in a package |
| `lightcraft-tiff` | 2,000 | `serde`, `thiserror` | TIFF and BigTIFF in both byte orders |
| `vectorcraft-pathops` | 4,900 | its `geom`, `kurbo`, `linesweeper` | exact booleans of curves — for a Shape node that unites and cuts shapes |
| `lightcraft-raw` | 27,100 | five of its sibling crates | camera raw files decoded to linear light — photographs for scan processing |

Cautions: the code is two weeks old and not on crates.io, so it comes by git revision and a
build then needs GitHub; the interface may change between revisions (pin, and wrap each in one
module of ours); and every one taken is a line in NOTICE and in Third-Party Notices — which our
licence test would check by itself, since git dependencies carry their licence in cargo's
metadata like any other.

---

## 29. The browser and FreeBSD: what each cost them

**FreeBSD is nearly free once Linux works.** Their whole FreeBSD support is one job in CI: a
FreeBSD 15 virtual machine on an ordinary Linux runner (`vmactions/freebsd-vm`), `pkg install`
of the windowing and graphics libraries (libxkbcommon, wayland, libX11 and friends, mesa-libs,
vulkan-loader, gtk3, fontconfig), rustup, then `cargo build --workspace --all-targets --locked`
and `cargo test`. The release adds a tarball laid out like `/usr/local`. Our code already
speaks of "free Unix" where it means Linux and the BSDs alike (`cfg(free_unix)`), so for us
this is a CI job and whatever it finds — once there is CI.

**The browser is a different program wearing the same code** (`docs/web.md` of FilmCraft and
EffectCraft). What it took:

- No threads (they would need a special page and a nightly compiler): everything that ran on
  workers is done a little at a time on the one thread, within a budget of milliseconds per
  frame; `rayon` runs inline. Later, Web Workers each with an engine of its own.
- No waiting for the GPU: a readback "can't be waited for", so a frame renders in passes — a
  result that is not there yet starts asynchronously, the step returns a placeholder, and the
  caller renders again when the device has delivered.
- No file system: a table of files the user picked; what the program writes is offered as a
  download; recovery and caches live in the browser's private storage.
- No `std::time::Instant`, `SystemTime::now`, `thread::sleep`, `temp_dir`, process id: each
  panics there; a browser-safe clock crate instead.
- Dialogs, the clipboard, audio, video decoding: each through the browser's own interface, behind
  the same service traits as on a computer.
- A size limit on the download enforced by the build (24 MiB), with per-crate optimisation
  settings to stay under it; fonts fetched on demand, not built in.
- A panic shows a "stopped working" page with the message and a Reload button, not a frozen
  canvas.
- Agents reach it through a JavaScript object (`window.filmcraft`) in place of the control port.

So a browser build is only possible because the session never touches the system directly
(section 17). For us it would start from the same place — the session layer and its services —
and would then need the engine's readbacks and its worker thread rewritten to yield. A real
project, not a switch; worth knowing the price, not worth paying before the rest.

**In passing, from their CI files**: every third-party action is pinned to a commit hash with
the version as a comment (`actions/checkout@3d3c42e… # v7.0.1`); builds use `--locked`; test
corpora and optional fonts are pinned to commits and checked by SHA-256; releases are drafts
with `SHA256SUMS.txt`; macOS builds are signed and notarized, Windows installers signed.

---

## 30. What reading their code found in ours

Each of these began as a trap one of their programs fell into and wrote down; each was then
looked for in our sources, or in the sources of the libraries we build with. "Checked" means
read in the code, not yet seen happen.

**Faults and gaps (checked)**

| # | In our program | Where | Their fix |
|---|---|---|---|
| 1 | **A file dropped on the window in a Wayland session does nothing, silently.** winit 0.30.13 sends the event on X11, Windows and macOS only | `App::file_dropped`; winit `platform_impl/linux/wayland` has no such code | a notice on Wayland; files pasted from the file manager |
| 2 | **A command-line argument that is not valid Unicode ends the program before a window opens** — a file manager can pass such a file name on Linux | `main.rs`: `std::env::args()` | `args_os()`, and a notice naming the path that could not be read |
| 3 | **A crash leaves nothing behind.** Release builds have no console on Windows, logs go to standard error only, and there is no panic hook outside the autopilot | `main.rs` (`windows_subsystem = "windows"`, `env_logger` to stderr) | a log file in the settings folder, the last two kept; a hook that logs the panic; Help ▸ Reveal Log File |
| 4 | **The window's own thread has no last-resort guard**: a panic in `update` or `view` ends the program (recovery then offers what autosave kept) | `App::update` | every entry point guarded; the document put back; a sentence shown |
| 5 | **Running out of video memory is not caught as such**; a lost device is not noticed | `gpu/mod.rs`: validation scopes and `on_uncaptured_error` only | out-of-memory scope, pool emptied, one retry, then "this did not fit"; a device-lost callback |
| 6 | **On Windows the automatic choice asks wgpu for Vulkan and DX12 both**; a Vulkan driver that crashes while adapters are listed would end the program before the window, and nothing would be learned from it | `settings.rs` (`"vulkan,dx12"`) | a launch guard: a marker left until the window has drawn for a few seconds; found at the next start, the safe backend is used and the user is told |
| 7 | **The main window has no application id on Linux**: generic icon and no tie to a launcher on Wayland | `app.rs` `main_window_settings` | an id, and a desktop entry of the same name |
| 8 | **A file dialog can come back empty without having been shown** (no portal, no `zenity`), and that reads as Cancel | wherever `rfd` is used | look for the pieces once; say what to install |
| 9 | "Modified" stays set after undoing back to exactly what was saved | `Document::is_dirty` (revision counters) | compare the state with the saved one |
| 10 | `unsafe` in twelve files of the designer; the never-panic habit and the layering are not enforced by anything | — | section 5.2; section 8, step 1 |
| 11 | No picture of any node is pinned: a shader change that alters what materials look like would pass every test | — | section 22 |

**Looked for and found sound**

| Their trap | Ours |
|---|---|
| A save that would not read back replaces a good file | `project.rs` writes only what "reads back as it was" |
| A file makes the program allocate without bound | `archive.rs`: entry count and byte limits enforced while unpacking; a bomb test |
| wgpu picks up some other program's DX12 compiler | wgpu 27 defaults to FXC; iced takes the default |
| A linked file changed by another program is not noticed, or the check blocks the window | looked at every two seconds and on return to the window, off the UI thread — as theirs |
| The engine's thread is left running at exit | joined when the service is dropped |
| Every pass allocates and clears fresh textures | a pool, trimmed to a budget |
| Everything is recomputed after a small change | a key per node; only changed keys recompute |

Items 1 to 3 and 7 are small and matter most to someone about to live on Linux. Item 12 is in
the next section.

---

## 31. The patches they carry for the windowing library we share

PdfCraft keeps its own copy of **winit 0.30.13 — the very version we build with** — in
`vendor/winit`, wired in with `[patch.crates-io]`, and a table in `vendor/README.md` that says
for each change what it is, why, which upstream request it comes from, and which test "keeps
the patch in the build". Eleven changes. Three are about things that have already happened to
us.

**1. The window that grows when dragged to a display of another scale (Windows 11).**
Their words: "`WM_DPICHANGED` applies the rect Windows suggests. It used to nudge the window
onto the monitor `MonitorFromWindow` names, which during a drag is still the monitor being
left; the nudge sent another `WM_DPICHANGED`, so a window dragged to a monitor with another
scale factor grew and stayed on the wrong monitor at the wrong scale. Backport of winit master
488c036a (winit#4341), which no 0.30 release has."

I read the code in the winit we build with (`platform_impl/windows/event_loop.rs`, about line
2309 on): the nudge is there — "Check to see if the new window rect is on the monitor with the
new DPI factor. If it isn't, offset the window so that it is." Their change is eight lines: on
Windows 11 (build 22000 and later) take the suggested rect as it is.

This is our own old trouble. `window_size.rs` begins: "Windows makes a window that large as it
moves onto a display of a larger scale: one moved onto a television at 300 % grows 2.4 times",
and what we did was hold the window to what the GPU can draw. That treats what followed (a
surface too large to make); the cause is this, and upstream has fixed it — only not in any
release of the line that iced 0.14 uses.

**4. The white window at start (Windows).** "A hidden window is not maximized until it is
shown, and a hidden window shown while maximized gets one `ShowWindow(SW_MAXIMIZE)`:
`SW_MAXIMIZE` shows a hidden window, so [the program], which keeps [its window] hidden until
its first frame is drawn, appeared unpainted (white) when it restored a maximized window, and
could lose its focus." (winit#4587.) We met the same white flash and went round it by cloaking
the window; this is where it came from.

**11. Everything too large on X11.** "On X11 without `Xft.dpi`, the scale factor comes from the
X screen's configured DPI before the output's physical density from RandR: Xorg can
deliberately run a dense panel at 96 DPI, and only winit applications then looked zoomed."
Their own change, not upstream's. Worth trying on the Xfce session.

The rest: (2) on Wayland an input method stopped working until restart whenever a seat lost any
capability — a tablet unplugged, a remote session ending; (3, 5–10) input-method faults on
Windows, X11 and macOS, for Korean and Japanese mostly — a caret not drawn while composing, a
syllable lost or doubled, the emoji picker's text dropped. None of those shows on a machine
that types Latin and Cyrillic; all of them will be found within a week of a public release.

And for the renderer's side, the fault we met as "the window stays empty for good": their
toolkit's renderer "configured the window's size as is, and `Surface::configure` panics beyond
the limit: a window restored at another monitor's scale … asked an 8192-pixel device for
8600 × 3423" — fixed in their copy by fitting the surface within the device's limit. We hold
the window's size instead; same fault, same place.

**What this offers us.** We use winit through iced, from crates.io, so the same
`[patch.crates-io] winit = { path = "vendor/winit" }` works for us. Changes 1 and 4 are small,
come from upstream's own merged requests, and would let two of our workarounds become
belt-and-braces instead of the only defence; 11 is theirs and would want trying first. Their
way of carrying a patch is the part to copy whatever we take: the table, the link upstream, and
**a test that fails if the patch falls out of the build** (theirs:
`winit_carries_the_windows_11_monitor_scale_fix`).

| # | In our program | Where | Their fix |
|---|---|---|---|
| 12 | **A window dragged to a display of another scale grows and may stay at the wrong scale** (Windows 11) — a fault of winit 0.30.13, fixed upstream but in no 0.30 release; we treat the symptom | winit `WM_DPICHANGED`; our `window_size.rs` | carry upstream's fix (winit#4341) as a patch |
| 13 | **On Linux the Open and Import dialogs hide files whose extension is in capitals** (`IMG_0001.JPG`, `SCAN.PNG`): rfd 0.17.2 makes `*.{extension}` of each filter entry for the portal and for zenity, and those patterns are case-sensitive | `app/packages.rs`, `app.rs`: `add_filter(label, &extensions)`; rfd `backend/xdg_desktop_portal.rs` line 58 | on Linux and the BSDs every letter of an extension becomes a class (`[jJ][pP][gG]`), "as the portal spec suggests"; Windows and macOS ignore case already |

<!-- end -->
