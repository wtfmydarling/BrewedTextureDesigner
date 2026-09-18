fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--contact-sheet" {
        let (graph, nodes) = if args.get(3).map(String::as_str) == Some("demo") {
            let (g, _) = ferrotex::demo::brick_wall();
            let nodes = g.nodes.iter().filter(|n| n.def().has_image).map(|n| (n.id, n.def().name.to_string())).collect();
            (g, nodes)
        } else {
            ferrotex::tools::gallery_graph()
        };
        if let Err(e) = ferrotex::tools::contact_sheet(&graph, &nodes, std::path::Path::new(&args[2])) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
