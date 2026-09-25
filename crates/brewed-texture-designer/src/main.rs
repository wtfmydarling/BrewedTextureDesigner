#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Brewed Texture Designer: a GPU accelerated, node based procedural texture editor.

mod app;
#[cfg(any(test, feature = "dev-tools"))]
mod autopilot;
mod brand;
mod environment;
mod eyedropper;
mod gpu;
mod highlight;
mod icons;
mod picker;
mod scrub;
mod style;
mod view;
mod widget;
mod workspace;

#[cfg(test)]
mod ui_tests;

use std::path::PathBuf;

use app::{App, StartupOptions};
use brewed_texture_engine::render::Job;

const USAGE: &str = "Brewed Texture Designer — GPU accelerated node based texture editor

usage: brewed-texture-designer [FILE.brewtex]
       brewed-texture-designer --render PACKAGE.brewtex [--graph NAME]... [--set NAME=VALUE]...
                               [--size N|WxH] [--precision 16|32] [--out DIR]

  --render         compute the graphs of a package without a window and write their
                   Output nodes as File > Export Outputs does: into DIR (the current folder
                   by default), in a folder for each graph when there are several; print
                   each file written. Exit code 0: all went well, 1: something went wrong
                   on the way (said on stderr), 2: nothing could be rendered
  --graph NAME     render this graph only (again for more)
  --set NAME=VALUE set an Input Value: 0.5, 4, true, 0.2,0.8, #9E4229 or 0.6,0.3,0.2
  --size N|WxH     the texture size: 2048, or 2048x1024
  --precision      16 or 32: grayscale images in half or full floats, whatever the
                   graphs say
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
  --scenario       UI state for the screenshot: add-menu, file-menu, context-menu,
                   gradient, math, levels, inputs, transform, tiling, dialog, tile,
                   choices, lighting-menu, tile-sampler, shape:NAME (mat-ball …),
                   environment:FILE, exposure:EV, blur:0..1; several joined by ';'
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

fn main() -> iced::Result {
    attach_console();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,brewed_texture_designer=info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = StartupOptions::default();
    #[cfg(any(test, feature = "dev-tools"))]
    let (mut flight, mut seed) = (None, 1);
    let mut render: Option<Job> = None;
    let mut i = 0;
    while i < args.len() {
        let value = |i: usize| args.get(i + 1).cloned();
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            #[cfg(feature = "dev-tools")]
            "--bench" => {
                if let Err(e) = brewed_texture_engine::tools::bench() {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            #[cfg(feature = "dev-tools")]
            "--contact-sheet" => {
                let out = value(i).unwrap_or_else(|| "contact_sheet.png".into());
                let (graph, nodes) = if args.get(i + 2).map(String::as_str) == Some("demo") {
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
                if let Err(e) = brewed_texture_engine::tools::contact_sheet(&graph, &nodes, std::path::Path::new(&out)) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            #[cfg(any(test, feature = "dev-tools"))]
            "--screenshot" => {
                options.dev.screenshot = value(i).map(PathBuf::from);
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
                let minutes = value(i).and_then(|m| m.parse::<f32>().ok());
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
                let Some(v) = value(i) else { usage_error(&format!("{} needs a value", args[i])) };
                let job = render.get_or_insert_with(Job::default);
                match args[i].as_str() {
                    "--render" => job.package = PathBuf::from(v),
                    "--graph" => job.graphs.push(v),
                    "--set" => match v.split_once('=') {
                        Some((name, text)) => job.values.push((name.trim().to_string(), text.to_string())),
                        None => usage_error(&format!("--set {v}: write NAME=VALUE")),
                    },
                    "--size" => job.size = Some(size(&v).unwrap_or_else(|| usage_error(&format!("--size {v}: write 2048 or 2048x1024")))),
                    "--precision" => {
                        job.precision = Some(match v.as_str() {
                            "16" | "half" => brewed_texture_core::Precision::Half,
                            "32" | "full" => brewed_texture_core::Precision::Full,
                            _ => usage_error(&format!("--precision {v}: write 16 or 32")),
                        })
                    }
                    _ => job.out = PathBuf::from(v),
                }
                i += 1;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}\n\n{}", usage());
                std::process::exit(2);
            }
            other => options.open = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    if let Some(job) = render {
        std::process::exit(render_package(job));
    }

    // The layout of the last session, kept for the next one (not for screenshots, which
    // show the program as it starts the first time, nor for the autopilot).
    #[cfg(any(test, feature = "dev-tools"))]
    let as_it_was = {
        options.dev.autopilot = flight.map(|minutes| autopilot::Flight { minutes, seed });
        options.dev.screenshot.is_none() && options.dev.autopilot.is_none()
    };
    #[cfg(not(any(test, feature = "dev-tools")))]
    let as_it_was = true;
    if as_it_was {
        options.workspace = Some(workspace::Workspace::load());
    }

    // A daemon: besides the main window, the eyedropper opens a window on every monitor.
    iced::daemon(move || App::boot(options.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(style::ui_font())
        .antialiasing(true)
        .run()
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
