#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Brewed Texture Designer: a GPU accelerated, node based procedural texture editor.

mod app;
#[cfg(any(test, feature = "dev-tools"))]
mod autopilot;
mod brand;
mod clock;
#[cfg(feature = "dev-tools")]
mod compose;
mod environment;
mod eyedropper;
mod gpu;
mod highlight;
mod icons;
mod log_file;
mod picker;
mod previews;
mod scrub;
mod settings;
mod shortcuts;
mod style;
mod third_party;
mod tool_window;
mod view;
mod widget;
mod window_size;
mod workspace;

#[cfg(test)]
mod ui_tests;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use app::{App, StartupOptions};
use brewed_texture_engine::render::Job;

const USAGE: &str = "Brewed Texture Designer — GPU accelerated node based texture editor

usage: brewed-texture-designer [FILE.brewtex]...
       brewed-texture-designer --render PACKAGE.brewtex [--graph NAME]... [--set NAME=VALUE]...
                               [--size N|WxH] [--precision 8|16|32] [--out DIR]

  --render         compute the graphs of a package without a window and write their
                   Output nodes as File > Export… writes them as they are: into DIR (the
                   current folder by default), in a folder for each graph when there are
                   several; print each file written. Exit code 0: all went well, 1:
                   something went wrong on the way (said on stderr), 2: nothing could be
                   rendered
  --graph NAME     render this graph (again for more). Without it: the graphs no other
                   graph of the package uses; those it uses are computed in them
  --set NAME=VALUE set an Input Value: 0.5, 4, true, 0.2,0.8, #9E4229 or 0.6,0.3,0.2;
                   an image input takes a picture file: --set emblem=logo.png
  --size N|WxH     the texture size: 2048, or 2048x1024
  --precision      8, 16 or 32: images in bytes, half floats or floats, whatever the
                   graphs say (a node that says its own keeps it)
  --out DIR        where the files go";

/// The tools for developing the program, in a build that has them (the `dev-tools` feature).
#[cfg(feature = "dev-tools")]
const DEV_USAGE: &str = "
tools for developing the program:
       brewed-texture-designer --screenshot OUT.png [--scenario NAME] [--select KIND] [--preview KIND] [FILE]
       brewed-texture-designer --contact-sheet OUT.png [demo]
       brewed-texture-designer --bench
       brewed-texture-designer --autopilot [MINUTES] [--seed N]

  --screenshot     start, wait until everything is computed, save a screenshot and quit
  --picture OUT    the same, saving the picture of the model a saved package gets (PNG, or JPEG
                   for a .jpg); --picture-size W[xH] says how large (512 square), --picture-lit
                   lights it as the 3D view is lit (its environment image) and not as a studio
  --export DIR     the same, exporting every output of the graph into DIR (as File → Export)
  --compose ...    pictures made of pictures, for the templates and lessons (--compose help)
  --third-party    make the list of the crates again (Help → Third-Party Notices): each its
                   license and its repository, as cargo has them
  --scenario       UI state for the screenshot: add-menu, file-menu, context-menu, home,
                   learn, home-about, example:ID (brick-wall, fx-maps, pixel-math), gradient, math, levels, inputs, transform, tiling, dialog, tile,
                   choices, usage, lighting-menu, tile-sampler, shape:NAME (matball …),
                   settings, settings:PAGE, shortcuts, about, notices, new-graph, export, versions,
                   template:ID (pbr, studio-panorama, scan-processing …), show:KIND, param:KIND.KEY=VALUE, graph:NAME,
                   picture:INPUT=FILE, uniform:OUTPUT=V[,G,B], size:N,
                   environment:FILE, light:preview(-right), exposure:EV, blur:0..1; several joined by ';'
  --select KIND    select the first node of this type (e.g. math, levels)
  --preview KIND   show the first node of this type in the 2D view
  --contact-sheet  render every node type (or the demo graph) into one image
  --bench          measure GPU evaluation and the SIMD kernels
  --autopilot      drive the app by itself: scenes (texture sizes, dragged values, every
                   node, files, exports, the 3D view), then random steps for MINUTES (10
                   by default); log what it does and what goes wrong, and quit
  --seed N         the autopilot's random steps (the same seed, the same steps)";

/// What the command line takes.
#[cfg(not(feature = "dev-tools"))]
fn usage() -> String {
    USAGE.to_string()
}

/// What the command line takes, the tools for developing the program too.
#[cfg(feature = "dev-tools")]
fn usage() -> String {
    format!("{USAGE}\n{DEV_USAGE}")
}

/// What is logged when `RUST_LOG` does not say.
const LOG_FILTER: &str = "warn,brewed_texture_designer=info";

/// What the command line asks for.
enum Asked {
    /// Something done at once, with no window: what the command line takes said, a tool for
    /// developing the program run.
    Now(Box<dyn FnOnce() -> Result<(), String>>),
    /// The window (or a render without one).
    Run(Box<Run>),
}

/// What the program is started with.
struct Run {
    options: StartupOptions,
    /// A render from the command line, with no window.
    render: Option<Job>,
    #[cfg(any(test, feature = "dev-tools"))]
    flight: Option<f32>,
    #[cfg(any(test, feature = "dev-tools"))]
    seed: u64,
}

fn main() -> iced::Result {
    #[cfg(target_arch = "x86_64")]
    if !runs_x86_64_v3() {
        cannot_run();
    }
    attach_console();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Run {
        mut options,
        render,
        #[cfg(any(test, feature = "dev-tools"))]
        flight,
        #[cfg(any(test, feature = "dev-tools"))]
        seed,
    } = match read_args(&args) {
        Asked::Now(now) => {
            log_file::start(None, LOG_FILTER);
            if let Err(e) = now() {
                eprintln!("{e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Asked::Run(run) => *run,
    };

    // The settings and the layout of the last session, kept for the next one (not for
    // screenshots, which show the program as it starts the first time, nor for the autopilot).
    #[cfg(any(test, feature = "dev-tools"))]
    let as_it_was = {
        options.dev.autopilot = flight.map(|minutes| autopilot::Flight { minutes, seed });
        options.dev.screenshot.is_none() && options.dev.autopilot.is_none()
    };
    #[cfg(not(any(test, feature = "dev-tools")))]
    let as_it_was = true;
    // The log: on standard error, and in a file beside the settings for a user's window (not for
    // a render from the command line, which would put away the log of the window's last run);
    // a panic is written in it before it is said as it was.
    options.log = log_file::start(workspace::folder().filter(|_| as_it_was && render.is_none()).as_deref(), LOG_FILTER);
    log_file::install_panic_hook();
    // (Read before anything else runs: they say the graphics API, which the environment
    // gives iced and the engine.)
    options.settings = settings::Store::open(workspace::folder().filter(|_| as_it_was).map(|f| f.join("settings.toml")));
    let chosen = settings::choose_graphics(&options.settings.settings());
    options.graphics_from_settings = chosen;
    let antialiasing = options.settings.settings().antialiasing;
    let mut store = options.settings.clone();

    if let Some(job) = render {
        std::process::exit(render_package(job));
    }

    // Given no package to open, the program starts on the Home screen (not for screenshots and
    // the autopilot, which start on the demo).
    options.home = as_it_was && options.open.is_empty();
    if as_it_was {
        options.workspace = Some(workspace::Workspace::load());
        // Unsaved work, kept to be recovered after a crash, beside the settings (not in the
        // temporary folder, which a system may empty as it starts again after the power went).
        options.recovery = workspace::folder().map(|f| f.join("Recovery"));
        // The earlier versions of the packages, there too: never beside the packages.
        options.versions = workspace::folder().map(|f| f.join("History"));
    }
    // The autopilot keeps its own beside its log, as a user's is kept.
    #[cfg(any(test, feature = "dev-tools"))]
    if let Some(flight) = &options.dev.autopilot {
        options.recovery = Some(autopilot::folder(flight.seed).join("recovery"));
        options.versions = Some(autopilot::folder(flight.seed).join("history"));
    }

    // A daemon: besides the main window, the eyedropper opens a window on every monitor.
    let ran = iced::daemon(move || App::boot(options.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .font(style::UI_FONTS[0])
        .font(style::UI_FONTS[1])
        .default_font(style::ui_font())
        .antialiasing(antialiasing)
        .run();
    if let Err(iced::Error::GraphicsCreationFailed(e)) = &ran {
        let said = if chosen { settings::graphics_failed(&mut store, &e.to_string()) } else { settings::environment_failed(&e.to_string()) };
        log::error!("{said}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(brewed_texture_core::APP_NAME)
            .set_description(said)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
    ran
}

/// Whether this processor, and the system, run what the program is built for on x86-64 (see
/// `.cargo/config.toml`): AVX2, FMA, BMI, F16C, LZCNT and MOVBE, with the system keeping the
/// 256-bit registers. Asked of the processor itself: `is_x86_feature_detected!` takes what the
/// build enables for granted.
#[cfg(target_arch = "x86_64")]
fn runs_x86_64_v3() -> bool {
    use std::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};
    let bit = |reg: u32, n: u32| reg & (1 << n) != 0;
    if __cpuid(0).eax < 7 || __cpuid(0x8000_0000).eax < 0x8000_0001 {
        return false;
    }
    let (basic, leaf7, extended) = (__cpuid(1), __cpuid_count(7, 0), __cpuid(0x8000_0001));
    // (XGETBV is there where the system says it saves the registers, OSXSAVE.)
    // SAFETY: only called where OSXSAVE says the instruction is there.
    let ymm = bit(basic.ecx, 27) && unsafe { _xgetbv(0) } & 0b110 == 0b110;
    let avx = bit(basic.ecx, 28) && bit(basic.ecx, 12) && bit(basic.ecx, 29) && bit(basic.ecx, 22);
    let avx2 = bit(leaf7.ebx, 5) && bit(leaf7.ebx, 3) && bit(leaf7.ebx, 8) && bit(extended.ecx, 5);
    ymm && avx && avx2
}

/// On a processor the program is not built for: say so, and stop (rather than stop with no
/// word at the first instruction it cannot run). Nothing is made at run time here, the message
/// least of all: what runs before this may already be what such a processor cannot run.
#[cfg(target_arch = "x86_64")]
#[inline(never)]
fn cannot_run() -> ! {
    #[cfg(windows)]
    // SAFETY: two static strings that end in a zero, and no window to own the message.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let text = windows_sys::w!("Brewed Texture Designer needs a processor with AVX2: Intel Core since 2013 (Haswell), AMD since 2015. This one has not.");
        MessageBoxW(std::ptr::null_mut(), text, windows_sys::w!("Brewed Texture Designer"), MB_OK | MB_ICONERROR);
    }
    eprintln!("Brewed Texture Designer needs a processor with AVX2: Intel Core since 2013 (Haswell), AMD since 2015. This one has not.");
    std::process::exit(1);
}

/// Read the command line (the arguments after the program's name). Options are read as text,
/// paths are kept as they are given: a file manager on Linux can give a file named in another
/// encoding than Unicode, which is opened, or said not to be (see `App::open_file`).
fn read_args(args: &[OsString]) -> Asked {
    let mut options = StartupOptions::default();
    #[cfg(any(test, feature = "dev-tools"))]
    let (mut flight, mut seed) = (None, 1);
    let mut render: Option<Job> = None;
    let mut i = 0;
    while i < args.len() {
        // The argument after this one: as text, or as the path it is.
        #[cfg(any(test, feature = "dev-tools"))]
        let value = |i: usize| args.get(i + 1).map(|v| text(v).to_string());
        #[cfg(any(test, feature = "dev-tools"))]
        let path = |i: usize| args.get(i + 1).map(PathBuf::from);
        let Some(arg) = args[i].to_str() else {
            options.open.push(PathBuf::from(&args[i]));
            i += 1;
            continue;
        };
        match arg {
            "--help" | "-h" => {
                return Asked::Now(Box::new(|| {
                    println!("{}", usage());
                    Ok(())
                }));
            }
            #[cfg(feature = "dev-tools")]
            "--bench" => return Asked::Now(Box::new(brewed_texture_engine::tools::bench)),
            #[cfg(feature = "dev-tools")]
            "--third-party" => {
                let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/third-party/crates.json.z");
                return Asked::Now(Box::new(move || third_party::write_crates(&out)));
            }
            #[cfg(feature = "dev-tools")]
            "--compose" => {
                let rest: Vec<String> = args[i + 1..].iter().map(|a| text(a).to_string()).collect();
                return Asked::Now(Box::new(move || compose::run(&rest)));
            }
            #[cfg(feature = "dev-tools")]
            "--contact-sheet" => {
                let out = path(i).unwrap_or_else(|| "contact_sheet.png".into());
                let demo = args.get(i + 2).and_then(|a| a.to_str()) == Some("demo");
                return Asked::Now(Box::new(move || {
                    let (graph, nodes) = if demo {
                        let (g, _) = brewed_texture_core::demo::graph();
                        let nodes = g
                            .nodes
                            .iter()
                            .filter(|n| n.def().has_image())
                            .map(|n| (n.id, n.def().name.to_string()))
                            .collect();
                        (g, nodes)
                    } else {
                        brewed_texture_engine::tools::gallery_graph()
                    };
                    brewed_texture_engine::tools::contact_sheet(&graph, &nodes, &out)
                }));
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--screenshot" => {
                options.dev.screenshot = path(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--picture" => {
                options.dev.picture = path(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--picture-lit" => options.dev.picture_lit = true,
            #[cfg(any(test, feature = "dev-tools"))]
            "--export" => {
                options.dev.export = path(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--picture-size" => {
                // WIDTH or WIDTHxHEIGHT.
                options.dev.picture_size = value(i).and_then(|s| {
                    let mut sides = s.split('x').map(|n| n.trim().parse::<u32>().ok());
                    let width = sides.next()??;
                    Some([width, sides.next().flatten().unwrap_or(width)])
                });
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--scenario" => {
                options.dev.scenario = value(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--select" => {
                options.dev.select = value(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--preview" => {
                options.dev.preview = value(i);
                i += 1;
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--autopilot" => {
                // How long is optional.
                let minutes = args.get(i + 1).and_then(|m| m.to_str()?.parse::<f32>().ok());
                if minutes.is_some() {
                    i += 1;
                }
                flight = Some(minutes.unwrap_or(10.0));
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--seed" => {
                seed = value(i).and_then(|s| s.parse().ok()).unwrap_or(seed);
                i += 1;
            }
            "--render" | "--graph" | "--set" | "--size" | "--precision" | "--out" => {
                let Some(given) = args.get(i + 1) else { usage_error(&format!("{arg} needs a value")) };
                let job = render.get_or_insert_with(Job::default);
                match arg {
                    "--render" => job.package = PathBuf::from(given),
                    "--out" => job.out = PathBuf::from(given),
                    _ => {
                        let v = text(given);
                        match arg {
                            "--graph" => job.graphs.push(v.to_string()),
                            "--set" => match v.split_once('=') {
                                Some((name, text)) => job.values.push((name.trim().to_string(), text.to_string())),
                                None => usage_error(&format!("--set {v}: write NAME=VALUE")),
                            },
                            "--size" => job.size = Some(size(v).unwrap_or_else(|| usage_error(&format!("--size {v}: write 2048 or 2048x1024")))),
                            _ => {
                                job.precision = Some(match v {
                                    "8" => brewed_texture_core::Precision::Bits8,
                                    "16" | "half" => brewed_texture_core::Precision::Half,
                                    "32" | "full" => brewed_texture_core::Precision::Full,
                                    _ => usage_error(&format!("--precision {v}: write 8, 16 or 32")),
                                })
                            }
                        }
                    }
                }
                i += 1;
            }
            other if other.starts_with("--") => usage_error(&format!("unknown option {other}")),
            _ => options.open.push(PathBuf::from(&args[i])),
        }
        i += 1;
    }
    Asked::Run(Box::new(Run {
        options,
        render,
        #[cfg(any(test, feature = "dev-tools"))]
        flight,
        #[cfg(any(test, feature = "dev-tools"))]
        seed,
    }))
}

/// An argument read as text (an option, or a value that is not a path); not Unicode, it is a
/// mistake, which names it as well as it can be written.
fn unicode(arg: &OsStr) -> Result<&str, String> {
    arg.to_str().ok_or_else(|| format!("{}: not Unicode text (only a file to open may be named otherwise)", arg.to_string_lossy()))
}

/// [`unicode`], or stop at the mistake.
fn text(arg: &OsStr) -> &str {
    unicode(arg).unwrap_or_else(|e| usage_error(&e))
}

/// Stop at a mistake in the command line.
fn usage_error(message: &str) -> ! {
    eprintln!("{message}\n\n{}", usage());
    std::process::exit(2);
}

/// "2048" or "2048x1024".
fn size(text: &str) -> Option<[u32; 2]> {
    let (w, h) = text.split_once(['x', 'X', '×']).unwrap_or((text, text));
    Some([w.trim().parse().ok()?, h.trim().parse().ok()?])
}

/// Render a package from the command line, and say what was written (on the standard
/// output, a file a line) and what went wrong. The exit code: 0 when all went well, 1 when
/// something went wrong on the way, 2 when nothing could be rendered.
fn render_package(mut job: Job) -> i32 {
    if job.package.as_os_str().is_empty() {
        usage_error("--render needs the package to render");
    }
    if job.out.as_os_str().is_empty() {
        job.out = PathBuf::from(".");
    }
    let gpu = match brewed_texture_engine::Gpu::headless() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    match brewed_texture_engine::render::render(&job, gpu) {
        Ok(report) => {
            for file in &report.written {
                println!("{}", file.display());
            }
            for problem in &report.problems {
                eprintln!("warning: {problem}");
            }
            for note in &report.notes {
                eprintln!("note: {note}");
            }
            if report.problems.is_empty() && !report.written.is_empty() { 0 } else { 1 }
        }
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    }
}

/// Release builds are GUI programs on Windows, without a console: when started from a
/// terminal, write to that terminal (for --help, --bench and errors).
fn attach_console() {
    #[cfg(windows)]
    // SAFETY: no preconditions; fails harmlessly without a parent console.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A file name that is not Unicode text: "café.brewtex" in Latin-1 on Unix, with a lone half
    /// of a surrogate pair on Windows (what each system can give a program).
    pub(crate) fn not_unicode() -> OsString {
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"caf\xe9.brewtex".to_vec())
        };
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0x63, 0x61, 0x66, 0xD800, 0x2E, 0x62, 0x72, 0x65, 0x77, 0x74, 0x65, 0x78])
        };
        assert!(name.to_str().is_none());
        name
    }

    fn run(args: &[&OsStr]) -> Run {
        match read_args(&args.iter().map(OsString::from).collect::<Vec<_>>()) {
            Asked::Run(run) => *run,
            Asked::Now(_) => panic!("the window, or a render, is asked for"),
        }
    }

    /// A file to open whose name is not Unicode text (a file manager on Linux can give one) is
    /// kept as it was given, beside the others: no panic before the window opens.
    #[test]
    fn a_file_named_in_another_encoding_is_kept_as_given() {
        let odd = not_unicode();
        let run = run(&[&odd, OsStr::new("walls.brewtex")]);
        assert_eq!(run.options.open, [PathBuf::from(&odd), PathBuf::from("walls.brewtex")]);
        assert!(run.render.is_none());
        assert!(matches!(read_args(&[odd, OsString::from("--help")]), Asked::Now(_)), "and the options after it are read");
    }

    /// Options are read as text, the paths they take as they are given.
    #[test]
    fn options_are_read_as_text_and_their_paths_as_given() {
        let odd = not_unicode();
        let run = run(&[
            OsStr::new("--render"),
            &odd,
            OsStr::new("--graph"),
            OsStr::new("Walls"),
            OsStr::new("--size"),
            OsStr::new("512"),
            OsStr::new("--out"),
            &odd,
        ]);
        let job = run.render.expect("a render");
        assert_eq!((job.package, job.out), (PathBuf::from(&odd), PathBuf::from(&odd)));
        assert_eq!((job.graphs, job.size), (vec!["Walls".to_string()], Some([512, 512])));
        assert!(run.options.open.is_empty());
    }

    /// What has to be text and is not is a mistake that names it, as well as it can be written.
    #[test]
    fn a_value_that_is_not_text_is_named_as_a_mistake() {
        assert_eq!(unicode(OsStr::new("Walls")), Ok("Walls"));
        let said = unicode(&not_unicode()).expect_err("not text");
        assert!(said.starts_with("caf\u{FFFD}") && said.contains("not Unicode"), "{said}");
    }
}
