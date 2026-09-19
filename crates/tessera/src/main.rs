#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tessera: a GPU accelerated, node based procedural texture editor.

mod app;
mod gpu;
mod highlight;
mod icons;
mod style;
mod view;
mod widget;

use std::path::PathBuf;

use app::{App, StartupOptions};

const USAGE: &str = "Tessera — GPU accelerated node based texture editor

usage: tessera [FILE.tessera]
       tessera --screenshot OUT.png [--scenario NAME] [--select KIND] [--preview KIND] [FILE]
       tessera --contact-sheet OUT.png [demo]
       tessera --bench

  --screenshot     start, wait until everything is computed, save a screenshot and quit
  --scenario       UI state for the screenshot: add-menu, file-menu, context-menu,
                   gradient, math, levels, inputs, transform, 3d, dialog
  --select KIND    select the first node of this type (e.g. math, levels)
  --preview KIND   show the first node of this type in the 2D view
  --contact-sheet  render every node type (or the demo graph) into one image
  --bench          measure GPU evaluation and the SIMD kernels";

fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,tessera=info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = StartupOptions::default();
    let mut i = 0;
    while i < args.len() {
        let value = |i: usize| args.get(i + 1).cloned();
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--bench" => {
                if let Err(e) = tessera_engine::tools::bench() {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--contact-sheet" => {
                let out = value(i).unwrap_or_else(|| "contact_sheet.png".into());
                let (graph, nodes) = if args.get(i + 2).map(String::as_str) == Some("demo") {
                    let (g, _) = tessera_core::demo::brick_wall();
                    let nodes = g
                        .nodes
                        .iter()
                        .filter(|n| n.def().has_image())
                        .map(|n| (n.id, n.def().name.to_string()))
                        .collect();
                    (g, nodes)
                } else {
                    tessera_engine::tools::gallery_graph()
                };
                if let Err(e) = tessera_engine::tools::contact_sheet(&graph, &nodes, std::path::Path::new(&out)) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--screenshot" => {
                options.screenshot = value(i).map(PathBuf::from);
                i += 1;
            }
            "--scenario" => {
                options.scenario = value(i);
                i += 1;
            }
            "--select" => {
                options.select = value(i);
                i += 1;
            }
            "--preview" => {
                options.preview = value(i);
                i += 1;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}\n\n{USAGE}");
                std::process::exit(2);
            }
            other => options.open = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    iced::application(move || App::boot(options.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(style::ui_font())
        .antialiasing(true)
        .window(iced::window::Settings {
            size: iced::Size::new(1680.0, 1000.0),
            min_size: Some(iced::Size::new(1000.0, 640.0)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
