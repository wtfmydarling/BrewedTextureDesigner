#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use eframe::{egui, egui_wgpu, wgpu};
use ferrotex::app::{FerrotexApp, StartupOptions};

fn usage() {
    println!(
        "Ferrotex — GPU accelerated node based texture editor\n\n\
         usage: ferrotex [FILE.ftx]\n\
         \x20      ferrotex --screenshot OUT.png [--select KIND] [FILE.ftx]\n\
         \x20      ferrotex --contact-sheet OUT.png [demo]"
    );
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,ferrotex=info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = StartupOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                usage();
                return Ok(());
            }
            "--contact-sheet" => {
                let out = args.get(i + 1).cloned().unwrap_or_else(|| "contact_sheet.png".into());
                let (graph, nodes) = if args.get(i + 2).map(String::as_str) == Some("demo") {
                    let (g, _) = ferrotex::demo::brick_wall();
                    let nodes = g.nodes.iter().filter(|n| n.def().has_image).map(|n| (n.id, n.def().name.to_string())).collect();
                    (g, nodes)
                } else {
                    ferrotex::tools::gallery_graph()
                };
                if let Err(e) = ferrotex::tools::contact_sheet(&graph, &nodes, std::path::Path::new(&out)) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--screenshot" => {
                i += 1;
                options.screenshot = args.get(i).map(PathBuf::from);
            }
            "--ui-test" => options.ui_test = true,
            "--select" => {
                i += 1;
                options.select = args.get(i).cloned();
            }
            other => options.open = Some(PathBuf::from(other)),
        }
        i += 1;
    }

    // Compute shaders need a "real" backend (Vulkan / DX12 / Metal), not GL.
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::PRIMARY;
    setup.power_preference = wgpu::PowerPreference::HighPerformance;
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ferrotex")
            .with_inner_size([1680.0, 1000.0])
            .with_min_inner_size([960.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        ..Default::default()
    };
    let ui_test = options.ui_test;
    let result = eframe::run_native(
        "Ferrotex",
        native,
        Box::new(move |cc| match FerrotexApp::new(cc, options) {
            Ok(app) => Ok(Box::new(app) as Box<dyn eframe::App>),
            Err(e) => Err(e.into()),
        }),
    );
    if ui_test {
        let failed = ferrotex::app::UI_TEST_FAILURES.load(std::sync::atomic::Ordering::SeqCst);
        std::process::exit(if result.is_ok() && failed == 0 { 0 } else { 1 });
    }
    result
}
