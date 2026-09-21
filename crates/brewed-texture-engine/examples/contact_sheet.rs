//! Render every image node type side by side: `cargo run -p brewed-texture-engine --example
//! contact_sheet -- OUT.png [demo]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "contact_sheet.png".into());
    let (graph, nodes) = if args.get(1).map(String::as_str) == Some("demo") {
        let (g, _) = brewed_texture_core::demo::brick_wall();
        let nodes = g.nodes.iter().filter(|n| n.def().has_image()).map(|n| (n.id, n.def().name.to_string())).collect();
        (g, nodes)
    } else {
        brewed_texture_engine::tools::gallery_graph()
    };
    if let Err(e) = brewed_texture_engine::tools::contact_sheet(&graph, &nodes, std::path::Path::new(&out)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
