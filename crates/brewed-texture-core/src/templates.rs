//! What a new graph starts as (File → New Package…, New Graph…, the Home screen), as
//! Substance Designer's templates; and the packages the Home screen's Learn opens, made here
//! rather than kept as files: they are what the program makes of them now.

use crate::graph::{Graph, NodeId, Precision};
use crate::nodes::{NodeKind, USAGE_AO, USAGE_BASE_COLOR, USAGE_EMISSIVE, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL, USAGE_ROUGHNESS};
use crate::package::Package;
use crate::types::ParamValue;

/// A category of templates, as Substance Designer lists its.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Pbr,
    Filters,
    PixelProcessor,
    FxMap,
    Others,
}

impl Category {
    pub const ALL: [Category; 5] = [Category::Pbr, Category::Filters, Category::PixelProcessor, Category::FxMap, Category::Others];

    pub fn name(self) -> &'static str {
        match self {
            Category::Pbr => "PBR (Metallic/Roughness)",
            Category::Filters => "Filters",
            Category::PixelProcessor => "Pixel Processor",
            Category::FxMap => "FX-Map",
            Category::Others => "Others",
        }
    }
}

/// What a new graph starts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Template {
    /// The maps of a physically based material, metallic and rough, each an Output made for
    /// the 3D view, fed a uniform value to be replaced.
    Pbr,
    /// Those, and an emissive map.
    PbrEmissive,
    /// A graph that changes a grayscale image: an image input to an output.
    GrayFilter,
    /// The same, of color.
    ColorFilter,
    /// Pixel Math to an output.
    PixelProcessor,
    /// An FX-Map to an output.
    FxMap,
    /// Nothing.
    Empty,
}

impl Template {
    pub const ALL: [Template; 7] =
        [Template::Pbr, Template::PbrEmissive, Template::GrayFilter, Template::ColorFilter, Template::PixelProcessor, Template::FxMap, Template::Empty];

    /// What names it where a name of words will not do (what the Home screen is told of it).
    pub fn id(self) -> &'static str {
        match self {
            Template::Pbr => "pbr",
            Template::PbrEmissive => "pbr-emissive",
            Template::GrayFilter => "gray-filter",
            Template::ColorFilter => "color-filter",
            Template::PixelProcessor => "pixel-processor",
            Template::FxMap => "fx-map",
            Template::Empty => "empty",
        }
    }

    pub fn from_id(id: &str) -> Option<Template> {
        Template::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn category(self) -> Category {
        match self {
            Template::Pbr | Template::PbrEmissive => Category::Pbr,
            Template::GrayFilter | Template::ColorFilter => Category::Filters,
            Template::PixelProcessor => Category::PixelProcessor,
            Template::FxMap => Category::FxMap,
            Template::Empty => Category::Others,
        }
    }

    /// Its name in its category.
    pub fn name(self) -> &'static str {
        match self {
            Template::Pbr => "Base",
            Template::PbrEmissive => "Base with Emissive",
            Template::GrayFilter => "Grayscale Filter",
            Template::ColorFilter => "Color Filter",
            Template::PixelProcessor => "Pixel Processor",
            Template::FxMap => "FX-Map",
            Template::Empty => "Empty",
        }
    }

    /// Its name where its category is not said with it (the Home screen's row).
    pub fn full_name(self) -> &'static str {
        match self {
            Template::Pbr => "PBR Material",
            Template::PbrEmissive => "PBR with Emissive",
            Template::Empty => "Empty Graph",
            other => other.name(),
        }
    }

    /// What kind of graph it makes.
    pub fn kind(self) -> &'static str {
        match self {
            Template::Pbr | Template::PbrEmissive => "Standard material",
            Template::GrayFilter | Template::ColorFilter => "Filter",
            Template::PixelProcessor | Template::FxMap => "Generator",
            Template::Empty => "Empty graph",
        }
    }

    /// What it is, in a few words.
    pub fn about(self) -> &'static str {
        match self {
            Template::Pbr => {
                "The maps of a physically based material, as game engines and renderers read them: each an Output the \
                 3D view shows, fed a uniform value to replace with what makes it."
            }
            Template::PbrEmissive => "The maps of Base, and an emissive map: what glows of itself, black where nothing does.",
            Template::GrayFilter => {
                "Changes a grayscale image: the image the graph takes, what is done to it, and its output. Used as a node, \
                 it filters what is connected to it."
            }
            Template::ColorFilter => {
                "Changes a color image: the image the graph takes, what is done to it, and its output. Used as a node, it \
                 filters what is connected to it."
            }
            Template::PixelProcessor => "Every pixel computed by Pixel Math: an expression of where it is (u, v), of values and of images read anywhere.",
            Template::FxMap => "Patterns scattered, stacked and colored as the program of an FX-Map says.",
            Template::Empty => "A graph with nothing in it.",
        }
    }

    /// The Outputs it has: their names, as their files are named.
    pub fn outputs(self) -> &'static [&'static str] {
        match self {
            Template::Pbr => &["basecolor", "normal", "roughness", "metallic", "height", "ambientocclusion"],
            Template::PbrEmissive => &["basecolor", "normal", "roughness", "metallic", "height", "ambientocclusion", "emissive"],
            Template::GrayFilter | Template::ColorFilter | Template::PixelProcessor | Template::FxMap => &["output"],
            Template::Empty => &[],
        }
    }

    /// The graph, its images `size` pixels wide and high, kept in `precision`.
    pub fn graph(self, size: [u32; 2], precision: Precision) -> Graph {
        let mut g = Graph { resolution: size, precision, ..Graph::default() };
        match self {
            Template::Pbr | Template::PbrEmissive => {
                // Each map: what feeds it (to be replaced), its value, and what the 3D view takes it for.
                let mut maps: Vec<(&str, [f32; 4], bool, u32)> = vec![
                    ("basecolor", [0.5, 0.5, 0.5, 1.0], false, USAGE_BASE_COLOR),
                    ("normal", [0.5, 0.5, 1.0, 1.0], false, USAGE_NORMAL),
                    ("roughness", [0.5, 0.5, 0.5, 1.0], true, USAGE_ROUGHNESS),
                    ("metallic", [0.0, 0.0, 0.0, 1.0], true, USAGE_METALLIC),
                    ("height", [0.5, 0.5, 0.5, 1.0], true, USAGE_HEIGHT),
                    ("ambientocclusion", [1.0, 1.0, 1.0, 1.0], true, USAGE_AO),
                ];
                if self == Template::PbrEmissive {
                    maps.push(("emissive", [0.0, 0.0, 0.0, 1.0], false, USAGE_EMISSIVE));
                }
                let rows = maps.len() as f32;
                for (row, (name, color, gray, usage)) in maps.into_iter().enumerate() {
                    let y = row as f32 * ROW;
                    let value = g.add_node(NodeKind::UNIFORM_COLOR, [0.0, y]);
                    set(&mut g, value, "color", ParamValue::Color(color));
                    set(&mut g, value, "output", ParamValue::Enum(u32::from(gray)));
                    let output = output(&mut g, name, usage, [340.0, y]);
                    let _ = g.connect(value, "o:output", output, "i:input");
                }
                frame(
                    &mut g,
                    "Replace each uniform value with what makes the map",
                    "Each Output is one map of the material, named as its file is, and the 3D view uses it as its \
                     usage says. The uniform colors only hold a value: connect what makes each map in their place.",
                    [-30.0, -110.0],
                    [568.0, 110.0 + (rows - 1.0) * ROW + TALL + 30.0],
                    GRAY,
                );
            }
            Template::GrayFilter | Template::ColorFilter => {
                let input = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
                let _ = g.bind_variable(input, "input");
                if self == Template::ColorFilter
                    && let Some(image) = g.variable_mut("input").and_then(|v| v.image.as_mut())
                {
                    image.kind = crate::nodes::ImageKind::Color;
                }
                let output = output(&mut g, "output", 0, [420.0, 0.0]);
                let _ = g.connect(input, "o:output", output, "i:input");
                frame(
                    &mut g,
                    "Put what changes the image between its input and its output",
                    "The Input is the image the filter is given where it is used as a node; the Output is what it \
                     gives back. Connect the nodes that change it between them.",
                    [-30.0, -90.0],
                    [648.0, 90.0 + TALL + 30.0],
                    GRAY,
                );
            }
            Template::PixelProcessor => {
                let math = g.add_node(NodeKind::PIXEL_MATH, [0.0, 0.0]);
                set(&mut g, math, "expression", ParamValue::Text("vec3(u, v, 0.5)".into()));
                let output = output(&mut g, "output", 0, [340.0, 0.0]);
                let _ = g.connect(math, "o:output", output, "i:input");
                frame(
                    &mut g,
                    "Pixel Math: every pixel is what its expression says",
                    "The expression is worked out for every pixel on the GPU: u and v say where the pixel is, 0 to 1 \
                     across and down, and vec3(u, v, 0.5) makes a color of them. Change it in the properties.",
                    [-30.0, -110.0],
                    [568.0, 110.0 + 216.0 + 30.0],
                    GRAY,
                );
            }
            Template::FxMap => {
                let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
                let output = output(&mut g, "output", 0, [340.0, 0.0]);
                let _ = g.connect(fx, "o:output", output, "i:input");
                frame(
                    &mut g,
                    "An FX-Map: its program says where each splat goes, how big and how bright",
                    "The program runs once for every splat: x and y say where it goes, size how big, value how \
                     bright. How many there are, and the pattern each draws, are in the properties.",
                    [-30.0, -110.0],
                    [568.0, 110.0 + FX_TALL + 30.0],
                    GRAY,
                );
            }
            Template::Empty => {}
        }
        g
    }
}

fn set(g: &mut Graph, node: NodeId, key: &str, value: ParamValue) {
    if let Some(p) = g.node_mut(node).and_then(|n| n.param_mut(key)) {
        p.value = value;
    }
}

/// How far apart the rows of nodes are: as tall as a node with its picture, and a gap.
const ROW: f32 = 240.0;
/// How tall a node with its picture and one row of sockets is drawn, and an FX-Map.
const TALL: f32 = 198.0;
const FX_TALL: f32 = 296.0;
/// The colors of frames: gray for what a template says to do, blue for what the nodes of an
/// example do.
const GRAY: [f32; 4] = [0.49, 0.49, 0.49, 1.0];
const HOW: [f32; 4] = [0.28, 0.51, 0.8, 1.0];

/// A frame titled `title` that says `description` under it, at `pos`, `size` big.
fn frame(g: &mut Graph, title: &str, description: &str, pos: [f32; 2], size: [f32; 2], color: [f32; 4]) {
    let id = g.add_comment(title.into(), pos, size);
    if let Some(c) = g.comment_mut(id) {
        c.description = description.into();
        c.color = color;
    }
}

/// An Output named `name`, which the 3D view takes for `usage`.
fn output(g: &mut Graph, name: &str, usage: u32, pos: [f32; 2]) -> NodeId {
    let output = g.add_node(NodeKind::OUTPUT, pos);
    set(g, output, "name", ParamValue::Text(name.into()));
    set(g, output, "usage", ParamValue::Enum(usage));
    output
}

/// What the Home screen's Learn opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Example {
    /// The brick wall (see [`crate::demo`]).
    BrickWall,
    /// Programs of FX-Maps: scattered discs, bricks, clouds, confetti, dots.
    FxMaps,
    /// Pixel Math with vectors: a normal from a height, and three maps packed in one.
    PixelMath,
}

impl Example {
    pub const ALL: [Example; 3] = [Example::BrickWall, Example::FxMaps, Example::PixelMath];

    /// What names it where a name of words will not do.
    pub fn id(self) -> &'static str {
        match self {
            Example::BrickWall => "brick-wall",
            Example::FxMaps => "fx-maps",
            Example::PixelMath => "pixel-math",
        }
    }

    pub fn from_id(id: &str) -> Option<Example> {
        Example::ALL.into_iter().find(|e| e.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Example::BrickWall => "A brick wall",
            Example::FxMaps => "FX-Map programs",
            Example::PixelMath => "Pixel Math with vectors",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Example::BrickWall => {
                "Bricks from a Tile Generator, worn by noise, colored by a gradient: input values drive the \
                 graph, and five outputs make a material in the 3D view."
            }
            Example::FxMaps => {
                "Five FX-Maps and their programs: splats scattered at random, bricks laid in rows, clouds made of \
                 ever finer splats added up, confetti in colors, dots left out at random."
            }
            Example::PixelMath => {
                "Vectors in Pixel Math: a normal map worked out from a height by how it slopes, and three maps \
                 packed into the channels of one (occlusion, roughness, metallic)."
            }
        }
    }

    /// The package, and the node it shows in the 2D view first.
    pub fn package(self) -> (Package, Option<NodeId>) {
        match self {
            Example::BrickWall => crate::demo::package(),
            Example::FxMaps => fx_maps(),
            Example::PixelMath => pixel_math(),
        }
    }
}

/// A graph of the FX-Map example: an FX-Map running `program` into an Output, over a
/// background, if any, in a frame that says what it does.
struct FxExample {
    title: &'static str,
    about: &'static str,
    program: &'static str,
    count: i32,
    /// From which depth to which.
    depths: (i32, i32),
    pattern: u32,
    blend: u32,
    background: Option<f32>,
    output: &'static str,
    usage: u32,
}

impl FxExample {
    fn graph(&self) -> (Graph, NodeId) {
        let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
        let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
        set(&mut g, fx, "program", ParamValue::Text(self.program.into()));
        set(&mut g, fx, "count", ParamValue::Int(self.count));
        set(&mut g, fx, "from_depth", ParamValue::Int(self.depths.0));
        set(&mut g, fx, "depth", ParamValue::Int(self.depths.1));
        set(&mut g, fx, "pattern", ParamValue::Enum(self.pattern));
        set(&mut g, fx, "blend", ParamValue::Enum(self.blend));
        let mut left = -30.0;
        if let Some(v) = self.background {
            let b = g.add_node(NodeKind::UNIFORM_COLOR, [-320.0, 0.0]);
            set(&mut g, b, "color", ParamValue::Color([v, v, v, 1.0]));
            set(&mut g, b, "output", ParamValue::Enum(1));
            let _ = g.connect(b, "o:output", fx, "i:background");
            left = -350.0;
        }
        let out = output(&mut g, self.output, self.usage, [400.0, 0.0]);
        let _ = g.connect(fx, "o:output", out, "i:input");
        frame(&mut g, self.title, self.about, [left, -150.0], [598.0 - left, 150.0 + FX_TALL + 30.0], HOW);
        (g, fx)
    }
}

fn fx_maps() -> (Package, Option<NodeId>) {
    const MAX: u32 = 0;
    const ADD: u32 = 1;
    const STACK: u32 = 2;
    let examples = [
        (
            "Scattered",
            FxExample {
                title: "64 splats at random",
                about: "At depth 0 the whole image is one cell, and it draws Count splats: 64. The program says what \
                        each is, reading rand(k), a random number of its own: where it goes (x, y), how big, how far \
                        turned and how bright. Where splats overlap, Max keeps the brighter. The Output is a height: \
                        the 3D view raises the model by it.",
                program: crate::nodes::DEFAULT_FX_MAP_PROGRAM,
                count: 64,
                depths: (0, 0),
                pattern: 0,
                blend: MAX,
                background: None,
                output: "scattered",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Bricks",
            FxExample {
                title: "Bricks laid in rows",
                about: "Depth 3 splits the image into 8 x 8 cells, and each draws Count splats: 2, numbered by i. \
                        The program lays the first in the upper half of its cell and the second in the lower half, \
                        half a brick along (x = 0.5 * i): rows of bricks, each as wide as its cell and not quite \
                        half as high, each as bright as its own random number says.",
                program: "# Two bricks a cell, the lower row half a brick along.\ny = 0.25 + 0.5 * i\nx = 0.5 * i\nsize = 0.94, 0.44\nvalue = 0.45 + 0.55 * rand(1)",
                count: 2,
                depths: (3, 3),
                pattern: 2,
                blend: MAX,
                background: None,
                output: "bricks",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Clouds",
            FxExample {
                title: "Clouds of ever finer splats",
                about: "From depth 0 to depth 8 every split of the image draws, one splat a cell: 1 at depth 0, 4 at \
                        1, 16 at 2, and 65 536 at 8. Each is a soft blob a little larger than its cell, lighter or \
                        darker at random, and the finer the weaker (0.6^depth). Add sums them over the gray of the \
                        background: clouds, made as noise is made of octaves.",
                program: "# A splat at every depth, each finer one weaker: clouds.\nsize = 1.4\nvalue = (rand(1) - 0.5) * 0.6^depth",
                count: 1,
                depths: (0, 8),
                pattern: 12,
                blend: ADD,
                background: Some(0.5),
                output: "clouds",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Confetti",
            FxExample {
                title: "Confetti in colors",
                about: "600 splats in the one cell of depth 0, each a little strip placed, sized and turned at \
                        random. A program that gives color makes the image a color one; Stack lays each later \
                        splat over the earlier ones. The Output is a base color: the 3D view shows it as the \
                        model's colors.",
                program: "x = rand(1); y = rand(2)\nsize = 0.02 + 0.03 * rand(3), 0.01 + 0.01 * rand(4)\nangle = 360 * rand(5)\ncolor = rand(6), rand(7), rand(8)",
                count: 600,
                depths: (0, 0),
                pattern: 1,
                blend: STACK,
                background: None,
                output: "confetti",
                usage: USAGE_BASE_COLOR,
            },
        ),
        (
            "Dots",
            FxExample {
                title: "A dot in every cell, some left out",
                about: "Depth 5 splits the image into 32 x 32 cells, each with one dot in its middle (x and y are 0.5 \
                        when the program does not say). Its size and brightness are random; draw = rand(3) < 0.8 \
                        leaves about one in five out: draw is 0 where the comparison does not hold.",
                program: "# A dot in every cell of a 32 x 32 grid, one in five left out.\nsize = 0.9 * rand(1)^0.5\nvalue = 0.3 + 0.7 * rand(2)\ndraw = rand(3) < 0.8",
                count: 1,
                depths: (5, 5),
                pattern: 0,
                blend: MAX,
                background: None,
                output: "dots",
                usage: USAGE_HEIGHT,
            },
        ),
    ];
    let mut graphs = examples.iter().map(|(name, example)| (*name, example.graph()));
    let (name, (first, preview)) = graphs.next().expect("graphs");
    let mut package = Package::new(name, first);
    for (name, (graph, _)) in graphs {
        package.add_graph(name, graph);
    }
    (package, Some(preview))
}

fn pixel_math() -> (Package, Option<NodeId>) {
    // A normal from a height.
    let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
    let height = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let normal = g.add_node(NodeKind::PIXEL_MATH, [300.0, 0.0]);
    set(
        &mut g,
        normal,
        "expression",
        ParamValue::Text(
            "# How steeply the height h rises across and down, in its pixels.\n\
             dx = sample(h, u + 1 / width, v) - sample(h, u - 1 / width, v)\n\
             dy = sample(h, u, v + 1 / height) - sample(h, u, v - 1 / height)\n\
             n = normalize(vec3(-dx, -dy, 0.02))\n\
             # From -1..1 to the colors of a normal map.\n\
             n * 0.5 + 0.5"
                .into(),
        ),
    );
    let _ = g.connect(height, "o:output", normal, &crate::graph::var_socket_key("h"));
    let out = output(&mut g, "normal", USAGE_NORMAL, [600.0, 0.0]);
    let _ = g.connect(normal, "o:output", out, "i:input");
    let heights = output(&mut g, "height", USAGE_HEIGHT, [600.0, ROW]);
    let _ = g.connect(height, "o:output", heights, "i:input");
    frame(
        &mut g,
        "A normal worked out from how the height slopes",
        "Pixel Math works out every pixel of its image from its expression. Here it reads the height h a pixel \
         to either side: dx and dy are how much it rises across and down. The normal leans away from the rise: \
         vec3(-dx, -dy, 0.02), made one long by normalize, the small z saying how steep a slope looks. n * 0.5 + \
         0.5 turns it from -1..1 into the colors of a normal map. The normal lights the model in the 3D view; the \
         height raises it.",
        [-30.0, -130.0],
        [828.0, 130.0 + ROW + TALL + 30.0],
        HOW,
    );
    let mut package = Package::new("Normal from Height", g);

    // Three maps in the channels of one.
    let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
    let mut maps = Vec::new();
    for (row, (kind, name)) in [(crate::nodes::NOISE_CELLS, "ao"), (0, "rough"), (crate::nodes::NOISE_WEAVE, "metal")].into_iter().enumerate() {
        let noise = g.add_node(NodeKind::NOISE, [0.0, row as f32 * ROW]);
        set(&mut g, noise, "type", ParamValue::Enum(kind));
        maps.push((noise, name));
    }
    let packed = g.add_node(NodeKind::PIXEL_MATH, [300.0, ROW]);
    set(&mut g, packed, "expression", ParamValue::Text("# Occlusion in red, roughness in green, metallic in blue.\nvec3(ao, rough, metal)".into()));
    for (noise, name) in maps {
        let _ = g.connect(noise, "o:output", packed, &crate::graph::var_socket_key(name));
    }
    let out = output(&mut g, "orm", crate::nodes::USAGE_BASE_COLOR, [600.0, ROW]);
    let _ = g.connect(packed, "o:output", out, "i:input");
    frame(
        &mut g,
        "Three maps in the channels of one",
        "Game engines read several gray maps from the channels of one picture: fewer files, less memory. Each \
         Noise here is one map: occlusion, roughness, metallic. vec3(ao, rough, metal) makes one color of the \
         three, occlusion in red, roughness in green, metallic in blue: ORM, as Unreal Engine packs them. The \
         Output shows it as the model's base color, to see all three at once.",
        [-30.0, -130.0],
        [828.0, 130.0 + 2.0 * ROW + TALL + 30.0],
        HOW,
    );
    package.add_graph("Channel Packing", g);
    (package, Some(normal))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every template and example is whole: its links lead somewhere, nothing in it says
    /// anything is wrong, and a template's outputs are those it says, in its category.
    #[test]
    fn templates_and_examples_are_whole() {
        let pbr = Template::Pbr.graph([512, 256], Precision::Full);
        assert_eq!((pbr.resolution, pbr.precision), ([512, 256], Precision::Full));
        assert!(Template::Empty.graph([2048, 2048], Precision::Half).nodes.is_empty());
        let mut graphs = Vec::new();
        for template in Template::ALL {
            let g = template.graph([512, 512], Precision::Half);
            let outputs: Vec<&str> = g.nodes_of(NodeKind::OUTPUT).map(|n| n.text("name")).collect();
            assert_eq!(outputs, template.outputs(), "{template:?}");
            assert_eq!(Template::from_id(template.id()), Some(template));
            assert!(Category::ALL.contains(&template.category()));
            graphs.push(g);
        }
        assert!(Category::ALL.iter().all(|c| Template::ALL.iter().any(|t| t.category() == *c)), "no category is empty");
        for example in Example::ALL {
            let (package, preview) = example.package();
            assert!(preview.is_some_and(|id| package.graphs[0].graph.node(id).is_some()), "{example:?} shows one of its nodes");
            graphs.extend(package.graphs.iter().map(|g| Graph::clone(&g.graph)));
        }
        for g in &graphs {
            let ev = crate::eval::evaluate(g);
            for n in &g.nodes {
                let e = ev.node(n.id).unwrap();
                assert!(e.errors.is_empty() && e.warnings.is_empty(), "{}: {:?} {:?}", n.def().name, e.errors, e.warnings);
            }
            for out in g.nodes_of(NodeKind::OUTPUT) {
                assert!(g.link_into(out.id, "i:input").is_some(), "output {} is not connected", out.text("name"));
            }
        }
    }
}
