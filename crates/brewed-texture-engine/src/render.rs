//! Rendering a package without a window, for scripts and build pipelines (`--render`): its
//! graphs, all of them or those named, with their Input Values set as asked, at the size and
//! precision asked, computed on the GPU, and their Output nodes written as the export writes
//! them. The packages whose graphs they use are read from their files, as deep as they go.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use brewed_texture_core::package::{self, Library, Package};
use brewed_texture_core::{DataType, GraphId, NodeKind, Precision, Value, Variable, project};

use crate::engine::{Engine, Keep, evaluate_blocking};
use crate::export;
use crate::gpu::Gpu;

/// What to render.
#[derive(Clone, Debug, Default)]
pub struct Job {
    pub package: PathBuf,
    /// The graphs, by name; none: every graph of the package.
    pub graphs: Vec<String>,
    /// Input Values to set: their names, and their values as typed ("0.5", "4", "true",
    /// "0.2,0.8", "#9E4229" or "0.6,0.3,0.2").
    pub values: Vec<(String, String)>,
    pub size: Option<[u32; 2]>,
    pub precision: Option<Precision>,
    /// Where the files go: straight into it for one graph, into a folder named after each
    /// graph for several.
    pub out: PathBuf,
}

/// What rendering did: the files written, and what went wrong without stopping it.
#[derive(Debug, Default)]
pub struct Report {
    pub written: Vec<PathBuf>,
    pub problems: Vec<String>,
}

/// A package and the packages whose graphs it uses, as deep as they go.
pub struct Packages {
    /// The package's file, as packages name it (absolute, clean).
    key: String,
    root: Package,
    deps: HashMap<String, Package>,
}

impl Library for Packages {
    fn package(&self, path: Option<&str>) -> Option<&Package> {
        match path {
            None => Some(&self.root),
            Some(p) if p == self.key => Some(&self.root),
            Some(p) => self.deps.get(p),
        }
    }
}

/// A path as packages name one: absolute, without `.` and `..`.
fn key_of(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |d| d.join(path))
    };
    package::clean_path(&absolute).display().to_string()
}

impl Packages {
    /// Read the package at `path`, and every package whose graphs it uses: those that cannot be
    /// read go into `problems` (the nodes that use them make nothing).
    pub fn read(path: &Path, problems: &mut Vec<String>) -> Result<Packages, String> {
        let loaded = project::load(path)?;
        problems.extend(loaded.notes);
        let mut packages = Packages { key: key_of(path), root: loaded.package, deps: HashMap::new() };
        let mut unreadable = BTreeSet::new();
        // As deep as packages use packages (a loop of them ends as they are all read).
        for _ in 0..32 {
            let wanted: BTreeSet<String> = std::iter::once(&packages.root)
                .chain(packages.deps.values())
                .flat_map(Package::dependencies)
                .filter(|p| *p != packages.key && !packages.deps.contains_key(p) && !unreadable.contains(p))
                .collect();
            if wanted.is_empty() {
                break;
            }
            for p in wanted {
                match project::load(Path::new(&p)) {
                    Ok(loaded) => {
                        packages.deps.insert(p, loaded.package);
                    }
                    Err(e) => {
                        problems.push(format!("A package whose graphs are used cannot be read: {e}"));
                        unreadable.insert(p);
                    }
                }
            }
        }
        Ok(packages)
    }

    /// The graphs named, in their order (each at most once); every graph of the package when
    /// none is.
    pub fn graphs(&self, names: &[String]) -> Result<Vec<(GraphId, String)>, String> {
        let all = || self.root.graphs.iter().map(|g| (g.id, g.name.clone()));
        if names.is_empty() {
            return Ok(all().collect());
        }
        let mut chosen: Vec<(GraphId, String)> = Vec::new();
        for name in names {
            let found = all().find(|(_, n)| n == name).ok_or_else(|| {
                let known: Vec<String> = all().map(|(_, n)| format!("'{n}'")).collect();
                format!("The package has no graph '{name}' (it has {})", known.join(", "))
            })?;
            if !chosen.contains(&found) {
                chosen.push(found);
            }
        }
        Ok(chosen)
    }
}

/// The value `text` gives an Input Value of the type of `var`.
pub fn parse_value(var: &Variable, text: &str) -> Result<Value, String> {
    let text = text.trim();
    let numbers = |n: usize| -> Result<Vec<f32>, String> {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        if parts.len() != n {
            return Err(format!("'{text}' is not {n} numbers separated by commas"));
        }
        parts.iter().map(|p| p.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("'{p}' is not a number"))).collect()
    };
    let value = match var.ty {
        DataType::Float => Value::Float(numbers(1)?[0]),
        DataType::Image => return Err("an image input takes a picture: a file".into()),
        DataType::Int => Value::Int(text.parse::<i32>().map_err(|_| format!("'{text}' is not a whole number"))?),
        DataType::Bool => match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Value::Bool(true),
            "false" | "0" | "no" | "off" => Value::Bool(false),
            _ => return Err(format!("'{text}' is neither true nor false")),
        },
        DataType::Vec2 => {
            let v = numbers(2)?;
            Value::Vec2([v[0], v[1]])
        }
        DataType::Color => match text.strip_prefix('#') {
            Some(hex) => hex_color(hex).ok_or_else(|| format!("'{text}' is not a color like #9E4229 or #9E4229FF"))?,
            None => {
                let v = match numbers(3) {
                    Ok(v) => [v[0], v[1], v[2], 1.0],
                    Err(_) => {
                        let v = numbers(4)?;
                        [v[0], v[1], v[2], v[3]]
                    }
                };
                Value::Color(v)
            }
        },
    };
    Ok(value)
}

/// The picture file a value of the command line names: where it is from the folder the program
/// runs in.
fn picture(text: &str) -> String {
    let path = std::path::Path::new(text.trim());
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).display().to_string()
}

/// "RRGGBB" or "RRGGBBAA".
fn hex_color(hex: &str) -> Option<Value> {
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| f32::from(b) / 255.0);
    let alpha = if hex.len() == 8 { byte(6)? } else { 1.0 };
    Some(Value::Color([byte(0)?, byte(2)?, byte(4)?, alpha]))
}

/// Render `job` on `gpu`: an error when nothing can be rendered at all (the package cannot be
/// read, a graph or an Input Value asked for is not there, a value does not parse); what else
/// goes wrong is in the report.
pub fn render(job: &Job, gpu: Gpu) -> Result<Report, String> {
    let mut report = Report::default();
    let mut packages = Packages::read(&job.package, &mut report.problems)?;
    let graphs = packages.graphs(&job.graphs)?;
    if let Some([w, h]) = job.size
        && !((1..=8192).contains(&w) && (1..=8192).contains(&h))
    {
        return Err(format!("{w}×{h}: a side is at least 1 and at most 8192 pixels"));
    }
    // Every value asked for is an Input Value of a graph rendered, of the right type.
    for (name, text) in &job.values {
        let mut found = false;
        for (id, _) in &graphs {
            let Some(var) = packages.root.graph(*id).and_then(|g| g.graph.variables.iter().find(|v| v.name == *name)) else { continue };
            if var.image.is_some() {
                // A picture: a file that is there.
                if !std::path::Path::new(picture(text).as_str()).is_file() {
                    return Err(format!("{name}: '{text}' is not a file"));
                }
            } else {
                parse_value(var, text).map_err(|e| format!("{name}: {e}"))?;
            }
            found = true;
        }
        if !found {
            return Err(format!("No graph rendered has an Input Value '{name}'"));
        }
    }
    let mut engine = Engine::new(gpu);
    // Only the Output nodes are read: the rest lives while the evaluation needs it.
    engine.set_keep(Keep::Read);
    for (id, name) in &graphs {
        // The values asked for, set before the graph is laid out: what they drive in the graphs
        // it uses takes them too.
        if let Some(entry) = packages.root.graph_mut(*id) {
            let graph = std::sync::Arc::make_mut(&mut entry.graph);
            for (key, text) in &job.values {
                if let Some(var) = graph.variables.iter_mut().find(|v| v.name == *key) {
                    match &mut var.image {
                        Some(image) => image.picture = picture(text),
                        None => var.value = parse_value(var, text)?,
                    }
                }
            }
        }
        let flat = package::flatten(&packages, Some(&packages.key), *id).ok_or_else(|| format!("The graph '{name}' cannot be laid out"))?;
        report.problems.extend(flat.problems.values().map(|p| format!("{name}: {p}")));
        let mut graph = flat.graph;
        if let Some(size) = job.size {
            graph.resolution = size;
        }
        if let Some(precision) = job.precision {
            graph.precision = precision;
        }
        let outputs: Vec<&brewed_texture_core::Node> = graph.nodes_of(NodeKind::OUTPUT).collect();
        if outputs.is_empty() {
            report.problems.push(format!("{name}: the graph has no Output node"));
            continue;
        }
        let dir = if graphs.len() == 1 { job.out.clone() } else { job.out.join(export::sanitize_file_name(name)) };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            report.problems.push(format!("{}: {e}", dir.display()));
            continue;
        }
        engine.ask(outputs.iter().map(|n| n.id).collect());
        let eval = evaluate_blocking(&mut engine, &graph);
        for node in &graph.nodes {
            let error = eval.node(node.id).and_then(|n| n.error().map(str::to_string)).or_else(|| engine.state(node.id)?.error.clone());
            if let Some(e) = error {
                report.problems.push(format!("{name}: {}: {e}", node.def().name));
            }
        }
        let names: Vec<(String, u32)> = outputs.iter().map(|n| (n.text("name").to_string(), n.enum_value("bit_depth"))).collect();
        for ((node, path), (_, depth)) in outputs.iter().zip(export::output_paths(&dir, &names)).zip(&names) {
            let format = eval.params(node.id).map_or(node.enum_value("format"), |p| p.u("format"));
            match engine.read_stored(node.id).and_then(|stored| export::write_stored(&path, &stored, format, *depth)) {
                Ok(()) => report.written.push(path),
                Err(e) => report.problems.push(format!("{}: {e}", path.display())),
            }
        }
    }
    let errors = engine.gpu.take_errors();
    report.problems.extend(errors.into_iter().map(|e| format!("GPU: {e}")));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brewed_texture_core::Graph;

    fn var(ty: DataType) -> Variable {
        Variable::new("v".into(), ty)
    }

    #[test]
    fn values_are_read_as_their_input_wants_them() {
        assert_eq!(parse_value(&var(DataType::Float), " 0.25 "), Ok(Value::Float(0.25)));
        assert_eq!(parse_value(&var(DataType::Int), "7"), Ok(Value::Int(7)));
        assert_eq!(parse_value(&var(DataType::Bool), "Yes"), Ok(Value::Bool(true)));
        assert_eq!(parse_value(&var(DataType::Bool), "0"), Ok(Value::Bool(false)));
        assert_eq!(parse_value(&var(DataType::Vec2), "0.5, -1"), Ok(Value::Vec2([0.5, -1.0])));
        assert_eq!(parse_value(&var(DataType::Color), "#FF8000"), Ok(Value::Color([1.0, 128.0 / 255.0, 0.0, 1.0])));
        assert_eq!(parse_value(&var(DataType::Color), "#FF800080"), Ok(Value::Color([1.0, 128.0 / 255.0, 0.0, 128.0 / 255.0])));
        assert_eq!(parse_value(&var(DataType::Color), "0.1,0.2,0.3"), Ok(Value::Color([0.1, 0.2, 0.3, 1.0])));
        assert_eq!(parse_value(&var(DataType::Color), "0.1,0.2,0.3,0.4"), Ok(Value::Color([0.1, 0.2, 0.3, 0.4])));
        for (ty, text) in [
            (DataType::Float, "half"),
            (DataType::Float, "NaN"),
            (DataType::Int, "2.5"),
            (DataType::Bool, "maybe"),
            (DataType::Vec2, "1"),
            (DataType::Color, "#FF80"),
            (DataType::Color, "#GG8000"),
            (DataType::Color, "1,2"),
        ] {
            assert!(parse_value(&var(ty), text).is_err(), "{ty:?} {text}");
        }
    }

    /// A package of two graphs, one using the other, saved, then rendered: all of it, one graph
    /// by name with an Input Value set and another size, and what cannot be.
    #[test]
    fn a_package_renders_to_files() {
        let Ok(gpu) = Gpu::headless() else { return };
        let dir = std::env::temp_dir().join(format!("brewed-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut package, _) = brewed_texture_core::demo::package();
        let mut small = Graph { resolution: [64, 64], ..Graph::default() };
        let disc = small.add_node(NodeKind::SHAPE, [0.0, 0.0]);
        let out = small.add_node(NodeKind::OUTPUT, [200.0, 0.0]);
        small.connect(disc, "o:output", out, "i:input").unwrap();
        package.graphs.push(brewed_texture_core::package::GraphEntry { id: GraphId(77), name: "Disc".into(), graph: std::sync::Arc::new(small) });
        let file = dir.join("walls.brewtex");
        let open = package.first();
        project::save(&file, &package, &HashMap::new(), open).unwrap();

        // Every graph, each in a folder of its own.
        let all = render(&Job { package: file.clone(), out: dir.join("all"), size: Some([128, 128]), ..Job::default() }, gpu).unwrap();
        assert!(all.problems.is_empty(), "{:?}", all.problems);
        assert_eq!(all.written.len(), 6, "{:?}", all.written);
        let wall = dir.join("all").join(export::sanitize_file_name(&package.graphs[0].name));
        for name in ["basecolor.png", "normal.png", "height.png", "roughness.png", "ambient_occlusion.png"] {
            let written = image::open(wall.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((written.width(), written.height()), (128, 128), "{name}");
        }
        let disc = image::open(dir.join("all").join("Disc").join("output.png")).unwrap();
        assert_eq!((disc.width(), disc.height()), (128, 128));

        // The wall alone, with another number of bricks: straight into the folder, and not
        // what it was.
        let wall_name = package.graphs[0].name.clone();
        let job = |bricks: &str, out: &str| Job {
            package: file.clone(),
            graphs: vec![wall_name.clone()],
            values: vec![("bricks_x".into(), bricks.into())],
            size: Some([128, 128]),
            out: dir.join(out),
            ..Job::default()
        };
        let four = render(&job("4", "four"), Gpu::headless().unwrap()).unwrap();
        let nine = render(&job("9", "nine"), Gpu::headless().unwrap()).unwrap();
        assert_eq!((four.written.len(), nine.written.len()), (5, 5));
        let read = |p: &Path| image::open(p).unwrap().into_rgba8().into_raw();
        assert_ne!(read(&dir.join("four").join("height.png")), read(&dir.join("nine").join("height.png")));

        // What is not there, or not a value, is said, and nothing is rendered.
        let wrong = |job: Job| render(&job, Gpu::headless().unwrap()).unwrap_err();
        assert!(wrong(Job { graphs: vec!["Roof".into()], ..job("4", "x") }).contains("no graph 'Roof'"));
        assert!(wrong(Job { values: vec![("bricks_z".into(), "4".into())], ..job("4", "x") }).contains("bricks_z"));
        assert!(wrong(job("four", "x")).contains("bricks_x"));
        assert!(wrong(Job { size: Some([0, 64]), ..job("4", "x") }).contains("8192"));
        assert!(wrong(Job { package: dir.join("gone.brewtex"), ..job("4", "x") }).contains("gone.brewtex"));

        // The worn bricks in a graph of their own, which the wall uses: a value set reaches what
        // it drives in there too.
        let wall = package.first();
        let g = &package.graph(wall).unwrap().graph;
        let worn: Vec<brewed_texture_core::NodeId> = g
            .nodes
            .iter()
            .filter(|n| (n.kind == NodeKind::BLEND && n.is_exposed("opacity")) || [NodeKind::LEVELS, NodeKind::BLUR].contains(&n.kind))
            .map(|n| n.id)
            .collect();
        assert_eq!(worn.len(), 3);
        let edit = brewed_texture_core::package::PackageEdit::GraphFromSelection { graph: wall, nodes: worn, name: "Worn".into() };
        edit.apply(&mut package).unwrap();
        project::save(&file, &package, &HashMap::new(), open).unwrap();
        let worn = |wear: &str, out: &str| {
            let job = Job { values: vec![("wear".into(), wear.into())], ..job("4", out) };
            let report = render(&job, Gpu::headless().unwrap()).unwrap();
            assert!(report.problems.is_empty(), "{:?}", report.problems);
            read(&dir.join(out).join("height.png"))
        };
        assert_ne!(worn("0.1", "clean"), worn("0.9", "worn"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
