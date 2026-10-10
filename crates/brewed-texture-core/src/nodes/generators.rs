//! Generators: nodes that create images from nothing (or from a file).

use super::*;

pub const SHAPES: &[&str] = &[
    "Disc",
    "Square",
    "Rounded Square",
    "Polygon",
    "Star",
    "Ring",
    "Cross",
    "Capsule",
    "Paraboloid",
    "Cone",
    "Pyramid",
    "Bell",
    "Gaussian",
    "Waves",
];
const SHAPE_IDS: &[&str] = &[
    "disc",
    "square",
    "rounded_square",
    "polygon",
    "star",
    "ring",
    "cross",
    "capsule",
    "paraboloid",
    "cone",
    "pyramid",
    "bell",
    "gaussian",
    "waves",
];
pub const SHAPE_ROUNDED: u32 = 2;
pub const SHAPE_POLYGON: u32 = 3;
pub const SHAPE_STAR: u32 = 4;
pub const SHAPE_RING: u32 = 5;
pub const SHAPE_CROSS: u32 = 6;
/// Shapes with hard edges (softness applies); the rest are height profiles.
pub const SOLID_SHAPES: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 7];

const TILE_PATTERNS: &[&str] = &[
    "Disc",
    "Square",
    "Rounded Square",
    "Polygon",
    "Star",
    "Ring",
    "Cross",
    "Capsule",
    "Paraboloid",
    "Cone",
    "Pyramid",
    "Bell",
    "Gaussian",
    "Waves",
    "Input Image",
];
const TILE_PATTERN_IDS: &[&str] = &[
    "disc",
    "square",
    "rounded_square",
    "polygon",
    "star",
    "ring",
    "cross",
    "capsule",
    "paraboloid",
    "cone",
    "pyramid",
    "bell",
    "gaussian",
    "waves",
    "image",
];
pub const TILE_PATTERN_IMAGE: u32 = 14;

/// What a Uniform Color makes, in the order of its "output" parameter.
pub const UNIFORM_OUTPUTS: &[&str] = &["Color", "Grayscale"];
const UNIFORM_OUTPUT_IDS: &[&str] = &["color", "gray"];

pub static UNIFORM_COLOR: NodeDef = NodeDef {
    id: "uniform_color",
    name: "Uniform Color",
    category: Category::Generators,
    description: "Fills the image with a single color.",
    keywords: "solid fill constant flat grayscale",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        // (Grayscale, a level of gray: what it makes is its luminance.)
        color("color", "Color", [0.5, 0.5, 0.5, 1.0]).gray_when("output", &[1]),
        choice("output", "Output", 0, UNIFORM_OUTPUTS, UNIFORM_OUTPUT_IDS)
            .fixed()
            .tip("Color: the color. Grayscale: its luminance, in a grayscale image."),
    ],
    role: Role::Image,
};

/// A number in every pixel: what a map holds that is not a color (a color's channels are from 0
/// to 1; an index of refraction is 1.5).
pub static UNIFORM_VALUE: NodeDef = NodeDef {
    id: "uniform_value",
    name: "Uniform Value",
    category: Category::Generators,
    description: "Fills the image with a number: what a map holds that is not a color, as an index of refraction of 1.5.",
    keywords: "constant number value float ior data map solid fill flat",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[float("value", "Value", 0.5, 0.0, 1.0).tip(
        "The number every pixel holds. From 0 to 1 it is a level of gray; an index of refraction, a distance or a \
         luminance takes it past them (an Output of floats keeps it: 8 bits hold it between 0 and 1).",
    )],
    role: Role::Image,
};

pub static SHAPE: NodeDef = NodeDef {
    id: "shape",
    name: "Shape",
    category: Category::Generators,
    description: "Anti-aliased geometric shapes and height profiles.",
    keywords: "disc circle square polygon star ring cross capsule cone pyramid bell gaussian waves",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        choice("shape", "Shape", 0, SHAPES, SHAPE_IDS).pictured(),
        vec2("size", "Size", [0.6, 0.6], 0.0, 2.0)
            .tip("Extent of the shape relative to its cell (1 = touches the edges)."),
        float("scale", "Scale", 1.0, 0.0, 2.0),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
        vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
        int("repeat", "Repeat", 1, 1, 32).bounded().tip("Number of copies per side."),
        float("softness", "Softness", 0.01, 0.0, 1.0).bounded().when("shape", SOLID_SHAPES),
        int("sides", "Sides", 6, 3, 16).bounded().when("shape", &[SHAPE_POLYGON, SHAPE_STAR]),
        float("inner_radius", "Inner Radius", 0.5, 0.05, 1.0).when("shape", &[SHAPE_STAR]),
        float("thickness", "Thickness", 0.2, 0.01, 1.0).when("shape", &[SHAPE_RING, SHAPE_CROSS]),
        float("corner_radius", "Corner Radius", 0.25, 0.0, 1.0).when("shape", &[SHAPE_ROUNDED]),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

pub static TILE_GENERATOR: NodeDef = NodeDef {
    id: "tile_generator",
    name: "Tile Generator",
    category: Category::Generators,
    description: "Scatters a pattern on a grid with per-tile randomness (bricks, tiles, dots, scales …).",
    keywords: "tiles bricks pattern grid scatter",
    inputs: &[image_opt("pattern", "Pattern")],
    outputs: IMAGE_OUT,
    params: &[
        choice("pattern", "Pattern", 1, TILE_PATTERNS, TILE_PATTERN_IDS).pictured(),
        int("x_amount", "X Amount", 8, 1, 64).bounded(),
        int("y_amount", "Y Amount", 8, 1, 64).bounded(),
        vec2("size", "Size", [0.9, 0.9], 0.0, 2.0).tip("Pattern size relative to the cell."),
        float("scale", "Scale", 1.0, 0.0, 2.0),
        float("offset", "Offset", 0.0, -1.0, 1.0).tip(
            "Shift of every row relative to the previous one, in tiles (0.5 gives a brick bond). \
             Whole tiles also move the random variations along with the tiles.",
        ),
        boolean("vertical_offset", "Vertical Offset", false)
            .tip("Shift columns vertically instead of shifting rows horizontally."),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0),
        angle("random_rotation", "Random Rotation", 0.0, 0.0, 180.0),
        float("random_scale", "Random Scale", 0.0, 0.0, 1.0).bounded(),
        float("random_offset", "Random Offset", 0.0, 0.0, 1.0).bounded(),
        float("luminance", "Luminance", 1.0, 0.0, 1.0).bounded(),
        float("random_luminance", "Random Luminance", 0.0, 0.0, 1.0).bounded(),
        float("mask_random", "Mask Random", 0.0, 0.0, 1.0).bounded().tip("Probability of removing a tile."),
        float("softness", "Softness", 0.02, 0.0, 1.0).bounded(),
        int("sides", "Sides", 6, 3, 16).bounded().when("pattern", &[SHAPE_POLYGON, SHAPE_STAR]),
        float("inner_radius", "Inner Radius", 0.5, 0.05, 1.0).when("pattern", &[SHAPE_STAR]),
        float("thickness", "Thickness", 0.2, 0.01, 1.0).when("pattern", &[SHAPE_RING, SHAPE_CROSS]),
        float("corner_radius", "Corner Radius", 0.25, 0.0, 1.0).when("pattern", &[SHAPE_ROUNDED]),
        choice("blend_mode", "Overlap", 0, &["Max", "Add"], &["max", "add"]).tip("How overlapping tiles combine."),
        int("seed", "Seed", 0, 0, 9999).bounded(),
    ],
    role: Role::Image,
};

/// What gives each tile of a Tile Sampler its color: its luminance only (the patterns in their
/// own colors), a random color of its own, or the Color Map where it stands.
pub const TILE_SAMPLER_COLORS: &[&str] = &["Luminance", "Random Colors", "Color Map"];
const TILE_SAMPLER_COLOR_IDS: &[&str] = &["luminance", "random", "color_map"];
/// How the tiles of a Tile Sampler that overlap combine.
pub const TILE_SAMPLER_OVERLAPS: &[&str] = &["Max", "Add", "Stack"];
const TILE_SAMPLER_OVERLAP_IDS: &[&str] = &["max", "add", "stack"];

pub static TILE_SAMPLER: NodeDef = NodeDef {
    id: "tile_sampler",
    name: "Tile Sampler",
    category: Category::Generators,
    description: "Places patterns on a grid, each tile varied at random and by maps: which stay (Mask), their size \
                  (Scale), turn (Rotation) and color (Color). With several patterns, each tile takes one.",
    keywords: "scatter splatter tiles stones pebbles instances grid maps sampler",
    inputs: &[
        image_opt("pattern_1", "Pattern 1"),
        image_opt("pattern_2", "Pattern 2"),
        image_opt("pattern_3", "Pattern 3"),
        image_opt("pattern_4", "Pattern 4"),
        gray_opt("mask_map", "Mask Map"),
        gray_opt("scale_map", "Scale Map"),
        gray_opt("rotation_map", "Rotation Map"),
        color_image_opt("color_map", "Color Map"),
    ],
    outputs: IMAGE_OUT,
    params: &[
        choice("pattern", "Pattern", 1, TILE_PATTERNS, TILE_PATTERN_IDS).pictured(),
        int("x_amount", "X Amount", 8, 1, 64).bounded(),
        int("y_amount", "Y Amount", 8, 1, 64).bounded(),
        vec2("size", "Size", [1.0, 1.0], 0.0, 2.0).tip("Pattern size relative to the cell."),
        float("scale", "Scale", 0.8, 0.0, 2.0),
        float("random_scale", "Random Scale", 0.0, 0.0, 1.0).bounded(),
        float("scale_map", "Scale Map Amount", 1.0, 0.0, 1.0)
            .bounded()
            .tip("How much the Scale Map scales the tiles: at 1, black makes a tile vanish and white leaves it whole."),
        float("random_position", "Random Position", 0.0, 0.0, 1.0).bounded().tip("How far a tile may move from its cell, in cells."),
        float("offset", "Offset", 0.0, -1.0, 1.0).tip("Shift of every row relative to the previous one, in tiles (0.5 gives a brick bond)."),
        boolean("vertical_offset", "Vertical Offset", false).tip("Shift columns vertically instead of shifting rows horizontally."),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0),
        angle("random_rotation", "Random Rotation", 0.0, 0.0, 180.0),
        angle("rotation_map", "Rotation Map Angle", 180.0, -360.0, 360.0).tip("How far white on the Rotation Map turns a tile."),
        float("random_mirror", "Random Mirror", 0.0, 0.0, 1.0)
            .bounded()
            .tip("The chance of a tile being mirrored across, and, apart from that, up and down."),
        choice("color", "Color", 0, TILE_SAMPLER_COLORS, TILE_SAMPLER_COLOR_IDS),
        float("luminance", "Luminance", 1.0, 0.0, 1.0).bounded(),
        float("random_luminance", "Random Luminance", 0.0, 0.0, 1.0).bounded(),
        float("mask_random", "Mask Random", 0.0, 0.0, 1.0).bounded().tip("The chance of a tile being left out."),
        float("mask_threshold", "Mask Map Threshold", 0.5, 0.0, 1.0)
            .bounded()
            .tip("A tile stays where the Mask Map at its center is at least this."),
        boolean("mask_invert", "Invert Mask Map", false),
        float("softness", "Softness", 0.02, 0.0, 1.0).bounded().when("pattern", SOLID_SHAPES),
        int("sides", "Sides", 6, 3, 16).bounded().when("pattern", &[SHAPE_POLYGON, SHAPE_STAR]),
        float("inner_radius", "Inner Radius", 0.5, 0.05, 1.0).when("pattern", &[SHAPE_STAR]),
        float("thickness", "Thickness", 0.2, 0.01, 1.0).when("pattern", &[SHAPE_RING, SHAPE_CROSS]),
        float("corner_radius", "Corner Radius", 0.25, 0.0, 1.0).when("pattern", &[SHAPE_ROUNDED]),
        choice("blend_mode", "Overlap", 0, TILE_SAMPLER_OVERLAPS, TILE_SAMPLER_OVERLAP_IDS)
            .tip("How overlapping tiles combine: the brighter wins, they add up, or one lies over another in a random order."),
        int("seed", "Seed", 0, 0, 9999).bounded(),
    ],
    role: Role::Image,
};

/// What an FX-Map program reads of each splat (see [`FX_MAP`]).
pub const FX_MAP_BUILTINS: &[&str] = &["i", "count", "depth", "cells", "col", "row", "u", "v", "seed", "width", "height"];

/// What an FX-Map program sets of each splat, each with how many values it takes (see the tip
/// of the program of [`FX_MAP`]).
pub const FX_MAP_OUTPUTS: &[(&str, usize, usize)] = &[
    ("x", 1, 1),
    ("y", 1, 1),
    ("size", 1, 2),
    ("angle", 1, 1),
    ("value", 1, 1),
    ("color", 3, 4),
    ("opacity", 1, 1),
    ("draw", 1, 1),
    ("image", 1, 1),
];

/// What each of [`FX_MAP_OUTPUTS`] is where the program does not set it.
pub const FX_MAP_STARTS: &[&[f64]] =
    &[&[0.5], &[0.5], &[1.0, 1.0], &[0.0], &[1.0], &[1.0, 1.0, 1.0, 1.0], &[1.0], &[1.0], &[1.0]];

/// The language of FX-Map programs.
pub static FX_MAP_RULES: crate::expr::script::ScriptRules =
    crate::expr::script::ScriptRules { builtins: FX_MAP_BUILTINS, outputs: FX_MAP_OUTPUTS };

/// How the splats of an FX-Map combine with what is under them.
pub const FX_MAP_BLENDS: &[&str] = &["Max", "Add", "Stack", "Subtract", "Min", "Multiply"];
const FX_MAP_BLEND_IDS: &[&str] = &["max", "add", "stack", "subtract", "min", "multiply"];
pub const FX_MAP_BLEND_MIN: u32 = 4;
pub const FX_MAP_BLEND_MULTIPLY: u32 = 5;

/// The most splats an FX-Map draws: its cells times its count, at all its depths.
pub const MAX_SPLATS: u64 = 1 << 24;

/// The program a new FX-Map runs: splats of every size, turn and brightness, anywhere.
pub const DEFAULT_FX_MAP_PROGRAM: &str = "# Each splat: where it is in its cell, how big, turned and bright.
x = rand(1)
y = rand(2)
size = 0.05 + 0.15 * rand(3)
angle = 360 * rand(4)
value = 0.3 + 0.7 * rand(5)";

pub static FX_MAP: NodeDef = NodeDef {
    id: "fx_map",
    name: "FX-Map",
    category: Category::Generators,
    description: "Draws patterns as its program says: the image is split in four again and again (Depth), each \
                  cell draws Count splats, and the program gives each its place, size, turn, color and opacity.",
    keywords: "fxmap fx map splatter scatter splat quadrant iterate pattern program script stamps particles substance",
    inputs: &[
        image_opt("background", "Background"),
        image_opt("pattern_1", "Pattern 1"),
        image_opt("pattern_2", "Pattern 2"),
        image_opt("pattern_3", "Pattern 3"),
        image_opt("pattern_4", "Pattern 4"),
    ],
    outputs: IMAGE_OUT,
    params: &[
        script("program", "Program", DEFAULT_FX_MAP_PROGRAM).tip(
            "What each splat is, one name = value after another. It sets x and y, where the splat is \
             across and down its cell, from 0 to 1 (0.5, its middle); size, how big it is, a share of its \
             cell, or two values, width and height (1); angle, how far it is turned, in degrees \
             counter-clockwise (0); value, how bright (1); color, red, green, blue and alpha, which makes \
             the image a color one; opacity (1); draw, 0 to leave it out (1); image, which pattern input it \
             draws, 1 to 4 (1). It reads i, the splat's number in its cell from 0, and count; depth, and \
             cells, how many there are across at it; col and row, its cell's; u and v, the middle of its \
             cell in the image, 0 to 1; seed; width and height of the image in pixels; and rand(k), the \
             splat's k-th random number. Other names are inputs: an image connected to one is read at u, v \
             (sample(image, x, y) reads it anywhere), and a name of an input value of the graph reads it.",
        ),
        int("count", "Count", 64, 1, 256).at_least(1.0).tip("How many splats each cell draws; the program's i counts them from 0."),
        int("depth", "Depth", 0, 0, 12)
            .bounded()
            .tip("How many times the image is split in four: at 0 the whole image is the one cell, at 1 it has 4, at 2 16 (4 × 4), and so on."),
        int("from_depth", "From Depth", 0, 0, 12).bounded().tip(
            "The splits drawn from: every depth from this one to Depth draws its cells, the finer over the \
             coarser (as clouds are made of ever finer ones). At Depth or more, only Depth.",
        ),
        choice("pattern", "Pattern", 0, TILE_PATTERNS, TILE_PATTERN_IDS).pictured(),
        float("softness", "Softness", 0.02, 0.0, 1.0).bounded().when("pattern", SOLID_SHAPES),
        int("sides", "Sides", 6, 3, 16).bounded().when("pattern", &[SHAPE_POLYGON, SHAPE_STAR]),
        float("inner_radius", "Inner Radius", 0.5, 0.05, 1.0).when("pattern", &[SHAPE_STAR]),
        float("thickness", "Thickness", 0.2, 0.01, 1.0).when("pattern", &[SHAPE_RING, SHAPE_CROSS]),
        float("corner_radius", "Corner Radius", 0.25, 0.0, 1.0).when("pattern", &[SHAPE_ROUNDED]),
        choice("blend", "Blending", 0, FX_MAP_BLENDS, FX_MAP_BLEND_IDS).tip(
            "How each splat combines with what is under it: the brighter wins, they add up, the later lies \
             over the earlier, it takes itself away, the darker wins, or it multiplies what is under it. A \
             grayscale pattern covers as much as it is bright; a color one, as its alpha says.",
        ),
        int("seed", "Seed", 0, 0, 9999).bounded(),
        choice("channel", "Read Channel", 0, READ_CHANNELS, filters::READ_CHANNEL_IDS)
            .fixed()
            .tip("Which channel of the images connected to the program's names it reads."),
        boolean("auto_bind", "Auto-bind", true)
            .fixed()
            .tip("Names of the program nothing is connected to take the value of the graph's input value of that name."),
    ],
    role: Role::Image,
};

pub static GRADIENT: NodeDef = NodeDef {
    id: "gradient",
    name: "Gradient",
    category: Category::Generators,
    description: "Linear, radial, angular and diamond grayscale gradients.",
    keywords: "ramp linear radial angular diamond",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        choice(
            "type",
            "Type",
            0,
            &["Linear", "Mirrored Linear", "Radial", "Angular", "Diamond"],
            &["linear", "mirrored_linear", "radial", "angular", "diamond"],
        )
        .pictured(),
        angle("angle", "Angle", 0.0, -180.0, 180.0),
        vec2("center", "Center", [0.5, 0.5], 0.0, 1.0),
        float("repeat", "Repeat", 1.0, 0.1, 16.0),
        float("offset", "Offset", 0.0, -1.0, 1.0),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

/// Noise types, in the order the menu shows them (a file says them as [`NOISE_IDS`] does). Each
/// is a way of making noise of its own, with a look of its own, not a look of another one.
pub const NOISE_TYPES: &[&str] = &[
    "Perlin",
    "Simplex",
    "Value",
    "Gaussian",
    "Billow",
    "Ridged",
    "Plasma",
    "Liquid",
    "Marble",
    "Wood",
    "Veins",
    "Streaks",
    "Flow",
    "Curl",
    "Fibers",
    "Scratches",
    "Directional Scratches",
    "Ripples",
    "Waveform",
    "Turing",
    "Raindrops",
    "Caustics",
    "Cells",
    "Voronoi",
    "Voronoi Fractal",
    "Cracks",
    "Crystal",
    "Mosaic",
    "Stones",
    "Scales",
    "Weave",
    "Blocks",
    "Truchet",
    "Maze",
    "Spots",
    "Blotches",
    "Moisture",
    "Dirt",
    "Craters",
    "White Noise",
];
/// What a file calls each of [`NOISE_TYPES`].
const NOISE_IDS: &[&str] = &[
    "perlin",
    "simplex",
    "value",
    "gaussian",
    "billow",
    "ridged",
    "plasma",
    "liquid",
    "marble",
    "wood",
    "veins",
    "streaks",
    "flow",
    "curl",
    "fibers",
    "scratches",
    "directional_scratches",
    "ripples",
    "waveform",
    "turing",
    "raindrops",
    "caustics",
    "cells",
    "voronoi",
    "voronoi_fractal",
    "cracks",
    "crystal",
    "mosaic",
    "stones",
    "scales",
    "weave",
    "blocks",
    "truchet",
    "maze",
    "spots",
    "blotches",
    "moisture",
    "dirt",
    "craters",
    "white_noise",
];
/// What each type is found by in the menu's search besides its name: what it looks like
/// and what it is called elsewhere.
const NOISE_WORDS: &[&str] = &[
    "gradient fbm fractal sum clouds smoke soft",
    "gradient fbm fractal clouds smooth",
    "fbm fractal clouds soft blurry",
    "gaussian blobs soft round smooth sparse convolution",
    "turbulence puffy clouds cotton cumulus",
    "mountains ridges terrain multifractal creased",
    "plasma bands folds turbulence psychedelic lightning",
    "warp swirl swirls smoke fluid",
    "stone bands veins turbulence",
    "grain rings plank timber tree",
    "lines lightning coastline electric ice",
    "brushed metal anisotropic directional stripes",
    "streamlines wind hair brush strokes current curl fluid",
    "curl eddies vortices swirls smoke fluid streamlines divergence free",
    "fur hair fabric cloth strands grass messy",
    "lines wear damage scuffs",
    "directional scratches brushed lines wear one way",
    "gabor fingerprint sand stripes",
    "waveform audio sound bars equalizer lines",
    "reaction diffusion labyrinth coral zebra brain stripes spots",
    "rain drops ripples rings circles water puddle waves",
    "water light pool underwater",
    "voronoi worley cellular",
    "voronoi edges borders cells distance f2 worley tiles",
    "voronoi fractal edges cells octaves",
    "cell edges voronoi borders dry mud paint broken",
    "facets voronoi gem crumpled shards",
    "voronoi flat patches patchwork stained glass",
    "cobblestones pebbles rocks pavement voronoi microscope",
    "fish dragon reptile roof shingles",
    "weave fabric cloth textile threads twill basket satin canvas linen",
    "pixels squares pixelated digital",
    "arcs pipes tubes loops",
    "labyrinth corridors paths",
    "dots dirt blobs gaussian bnw",
    "camouflage patches moisture stains",
    "moisture wet damp stains water marks tide",
    "dirt grime grunge specks dust",
    "moon pits holes impacts",
    "random grain static pixels",
];
const NOISE_PERLIN: u32 = 0;
const NOISE_SIMPLEX: u32 = 1;
const NOISE_VALUE: u32 = 2;
const NOISE_GAUSSIAN: u32 = 3;
const NOISE_BILLOW: u32 = 4;
const NOISE_RIDGED: u32 = 5;
const NOISE_PLASMA: u32 = 6;
const NOISE_LIQUID: u32 = 7;
const NOISE_MARBLE: u32 = 8;
pub const NOISE_WOOD: u32 = 9;
pub const NOISE_VEINS: u32 = 10;
const NOISE_STREAKS: u32 = 11;
const NOISE_FLOW: u32 = 12;
const NOISE_CURL: u32 = 13;
pub const NOISE_FIBERS: u32 = 14;
pub const NOISE_SCRATCHES: u32 = 15;
pub const NOISE_DIRECTIONAL_SCRATCHES: u32 = 16;
const NOISE_RIPPLES: u32 = 17;
const NOISE_WAVEFORM: u32 = 18;
const NOISE_TURING: u32 = 19;
const NOISE_RAINDROPS: u32 = 20;
const NOISE_CAUSTICS: u32 = 21;
pub const NOISE_CELLS: u32 = 22;
const NOISE_VORONOI: u32 = 23;
const NOISE_VORONOI_FRACTAL: u32 = 24;
pub const NOISE_CRACKS: u32 = 25;
const NOISE_CRYSTAL: u32 = 26;
const NOISE_MOSAIC: u32 = 27;
const NOISE_STONES: u32 = 28;
const NOISE_SCALES: u32 = 29;
pub const NOISE_WEAVE: u32 = 30;
const NOISE_BLOCKS: u32 = 31;
const NOISE_TRUCHET: u32 = 32;
const NOISE_MAZE: u32 = 33;
const NOISE_SPOTS: u32 = 34;
const NOISE_BLOTCHES: u32 = 35;
const NOISE_MOISTURE: u32 = 36;
const NOISE_DIRT: u32 = 37;
const NOISE_CRATERS: u32 = 38;
/// White noise: one random value per pixel.
pub const NOISE_WHITE: u32 = 39;
/// Every type but white noise, which is one value per pixel.
const NOISE_SCALED: &[u32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    32, 33, 34, 35, 36, 37, 38,
];
/// Types made of octaves, each finer than the last.
const NOISE_FRACTAL: &[u32] = &[
    NOISE_PERLIN,
    NOISE_SIMPLEX,
    NOISE_VALUE,
    NOISE_BILLOW,
    NOISE_RIDGED,
    NOISE_LIQUID,
    NOISE_MARBLE,
    NOISE_VEINS,
    NOISE_STREAKS,
    NOISE_BLOCKS,
    NOISE_BLOTCHES,
    NOISE_GAUSSIAN,
    NOISE_PLASMA,
    NOISE_VORONOI_FRACTAL,
    NOISE_MOISTURE,
    NOISE_DIRT,
];

/// The ways a weave's threads cross (see the Noise node's Weave).
pub const WEAVES: &[&str] = &["Plain", "Twill", "Basket", "Satin"];
const WEAVE_IDS: &[&str] = &["plain", "twill", "basket", "satin"];
/// How many threads a weave repeats after, each way: its scale is made a whole number of
/// them, so it tiles.
pub const WEAVE_REPEATS: [i32; 4] = [2, 4, 4, 5];

pub static NOISE: NodeDef = NodeDef {
    id: "noise",
    name: "Noise",
    category: Category::Generators,
    description: "Tileable noises: clouds, ridges, plasma, marble, wood, flow, curl, fibers, scratches, waves, \
                  cells, cracks, stones, scales, woven cloth, mazes, spots, dirt, craters and more.",
    keywords: "perlin simplex value gaussian fbm clouds billow ridged plasma liquid warp swirl marble wood veins \
               streaks flow curl fibers fur hair scratches ripples waveform gabor turing reaction raindrops rain \
               caustics voronoi worley cells cracks crystal mosaic stones scales weave fabric cloth blocks truchet \
               maze spots dirt grunge blotches moisture craters white random grain",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        choice("type", "Type", 0, NOISE_TYPES, NOISE_IDS).pictured().searched(NOISE_WORDS),
        int("scale", "Scale", 8, 1, 128)
            .bounded()
            .tip("Number of noise cells across the image (an integer, so it tiles).")
            .when("type", NOISE_SCALED),
        int("octaves", "Octaves", 5, 1, 10)
            .bounded()
            .tip("Layers of ever finer detail on top of each other.")
            .when("type", NOISE_FRACTAL),
        float("roughness", "Roughness", 0.5, 0.0, 1.0)
            .bounded()
            .tip("How strong each finer layer is compared to the one before.")
            .when("type", NOISE_FRACTAL),
        float("distortion", "Distortion", 1.0, 0.0, 4.0)
            .tip("How strongly the pattern is bent: how far it swirls, how much the cracks wobble, how sharply \
                  the light gathers.")
            .when("type", &[NOISE_LIQUID, NOISE_MARBLE, NOISE_CAUSTICS, NOISE_CRACKS, NOISE_PLASMA]),
        int("stretch", "Stretch", 8, 1, 32)
            .at_least(1.0)
            .tip("How many times longer than tall the streaks, or the knots of the grain, are.")
            .when("type", &[NOISE_STREAKS, NOISE_WOOD]),
        int("rings", "Rings", 12, 1, 64)
            .clamped(1.0, 256.0)
            .tip("How close together the rings of the grain are.")
            .when("type", &[NOISE_WOOD]),
        angle("angle", "Angle", 0.0, -180.0, 180.0)
            .tip("Which way the flow, the fibers, the scratches or the ripples run.")
            .when("type", &[NOISE_FLOW, NOISE_FIBERS, NOISE_RIPPLES, NOISE_DIRECTIONAL_SCRATCHES]),
        float("spread", "Spread", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 keeps them all running the same way; 1 lets them turn a long way from it.")
            .when("type", &[NOISE_FLOW, NOISE_FIBERS, NOISE_RIPPLES, NOISE_DIRECTIONAL_SCRATCHES]),
        float("jitter", "Jitter", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 puts every cell's point in its middle (a regular grid), 1 anywhere in the cell.")
            .when(
                "type",
                &[NOISE_CELLS, NOISE_VORONOI, NOISE_VORONOI_FRACTAL, NOISE_CRACKS, NOISE_CRYSTAL, NOISE_MOSAIC, NOISE_STONES, NOISE_SCALES],
            ),
        float("density", "Density", 0.5, 0.0, 1.0)
            .bounded()
            .tip("How much of the image is covered; for a waveform, how loud it is.")
            .when(
                "type",
                &[NOISE_TURING, NOISE_RAINDROPS, NOISE_SPOTS, NOISE_BLOTCHES, NOISE_MOISTURE, NOISE_DIRT, NOISE_CRATERS, NOISE_WAVEFORM],
            ),
        float("line_density", "Density", 0.15, 0.0, 1.0)
            .bounded()
            .tip("How many lines there are.")
            .when("type", &[NOISE_FIBERS, NOISE_SCRATCHES, NOISE_DIRECTIONAL_SCRATCHES]),
        float("length", "Length", 2.0, 0.1, 6.0)
            .clamped(0.01, 6.0)
            .tip("How long the lines, or the streaks of the flow, are.")
            .when("type", &[NOISE_FLOW, NOISE_CURL, NOISE_FIBERS, NOISE_SCRATCHES, NOISE_DIRECTIONAL_SCRATCHES]),
        pixels("width", "Width", 1.5, 0.0, 16.0)
            .at_least(0.0)
            .tip("How wide the lines are in the middle (they get thinner towards their ends).")
            .when("type", &[NOISE_FIBERS, NOISE_SCRATCHES, NOISE_DIRECTIONAL_SCRATCHES]),
        pixels("line_width", "Width", 3.0, 0.0, 16.0)
            .at_least(0.0)
            .tip("How wide the lines are.")
            .when("type", &[NOISE_VEINS, NOISE_CRACKS]),
        float("thickness", "Thickness", 0.25, 0.0, 1.0)
            .bounded()
            .tip("How thick the tubes, corridors or threads are, as a share of a cell; for a waveform, how wide \
                  its bars are.")
            .when("type", &[NOISE_TRUCHET, NOISE_MAZE, NOISE_WEAVE, NOISE_WAVEFORM]),
        choice("weave", "Weave", 0, WEAVES, WEAVE_IDS)
            .tip("How the threads cross: Plain, over one and under one; Twill, over two and under two a step \
                  along each row, in diagonal ribs; Basket, two threads together as Plain; Satin, over four and \
                  under one, the crossings spread out. (The scale is taken as a whole number of repeats, so the \
                  weave tiles.)")
            .when("type", &[NOISE_WEAVE]),
        int("seed", "Seed", 0, 0, 9999).bounded(),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

/// How the picture of an SVG node fills the texture.
pub const SVG_FITS: &[&str] = &["Fit", "Fill", "Stretch"];
const SVG_FIT_IDS: &[&str] = &["fit", "fill", "stretch"];
/// What an SVG node makes of the picture.
pub const SVG_OUTPUTS: &[&str] = &["Color", "Coverage"];
const SVG_OUTPUT_IDS: &[&str] = &["color", "coverage"];
/// Where the text of a Text node is, from its position.
pub const TEXT_ALIGNS: &[&str] = &["Left", "Center", "Right"];
const TEXT_ALIGN_IDS: &[&str] = &["left", "center", "right"];

pub static SVG: NodeDef = NodeDef {
    id: "svg",
    name: "SVG",
    category: Category::Generators,
    description: "Vector graphics from an SVG file, sharp at any texture size: in their colors, or as a mask of \
                  what they cover. Moved, turned and scaled without losing an edge; they wrap where it tiles.",
    keywords: "vector svg illustrator inkscape shape logo pattern mask file",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        file("path", "File", &["svg"]),
        choice("fit", "Fit", 0, SVG_FITS, SVG_FIT_IDS)
            .fixed()
            .tip("Fit: all of the picture, as large as the texture holds it. Fill: the texture covered, the rest \
                  cut. Stretch: to the texture's shape."),
        choice("output", "Output", 0, SVG_OUTPUTS, SVG_OUTPUT_IDS)
            .fixed()
            .tip("Color: the picture's colors. Coverage: grayscale, how much of each pixel the picture covers."),
        vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
        float_log("scale", "Scale", 1.0, 0.05, 20.0).at_least(0.001),
    ],
    role: Role::Image,
};

pub static TEXT: NodeDef = NodeDef {
    id: "text",
    name: "Text",
    category: Category::Generators,
    description: "A line of text, sharp at any texture size: white where the letters are. In its font, or in Inter \
                  (built in) for what the font lacks; the same on every system. It wraps where it tiles.",
    keywords: "text font letters numbers label words typography type",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        text("text", "Text", "Text"),
        file("font", "Font", &["ttf", "otf", "ttc"]).tip("A font file (TrueType, OpenType); none: Inter, built in."),
        pixels("size", "Size", 200.0, 4.0, 1024.0).at_least(0.0).tip("How high the letters are (the font's em)."),
        choice("align", "Alignment", 1, TEXT_ALIGNS, TEXT_ALIGN_IDS).tip("Which end of the line is at the position."),
        float("spacing", "Letter Spacing", 0.0, -0.3, 1.0).tip("Between the letters, in ems."),
        vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
    ],
    role: Role::Image,
};

pub static BITMAP: NodeDef = NodeDef {
    id: "bitmap",
    name: "Bitmap",
    category: Category::Generators,
    description: "Loads an image file (PNG, JPEG, TGA, BMP, TIFF, EXR), resampled to the graph size.",
    keywords: "image file picture photo load",
    inputs: NO_SOCKETS,
    outputs: COLOR_OUT,
    params: &[file("path", "File", PICTURE_FILES)],
    role: Role::Image,
};
