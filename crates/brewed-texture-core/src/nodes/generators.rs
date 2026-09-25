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
pub const TILE_PATTERN_IMAGE: u32 = 14;

pub static UNIFORM_COLOR: NodeDef = NodeDef {
    id: "uniform_color",
    name: "Uniform Color",
    category: Category::Generators,
    description: "Fills the image with a single color.",
    keywords: "solid fill constant flat",
    inputs: NO_SOCKETS,
    outputs: COLOR_OUT,
    params: &[color("color", "Color", [0.5, 0.5, 0.5, 1.0])],
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
        choice("shape", "Shape", 0, SHAPES).pictured(),
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
        choice("pattern", "Pattern", 1, TILE_PATTERNS).pictured(),
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
        choice("blend_mode", "Overlap", 0, &["Max", "Add"]).tip("How overlapping tiles combine."),
        int("seed", "Seed", 0, 0, 9999).bounded(),
    ],
    role: Role::Image,
};

/// What gives each tile of a Tile Sampler its color: its luminance only (the patterns in their
/// own colors), a random color of its own, or the Color Map where it stands.
pub const TILE_SAMPLER_COLORS: &[&str] = &["Luminance", "Random Colors", "Color Map"];
/// How the tiles of a Tile Sampler that overlap combine.
pub const TILE_SAMPLER_OVERLAPS: &[&str] = &["Max", "Add", "Stack"];

pub static TILE_SAMPLER: NodeDef = NodeDef {
    id: "tile_sampler",
    name: "Tile Sampler",
    category: Category::Generators,
    description: "Places patterns on a grid, each tile varied at random and by maps: which tiles stay (Mask Map), \
                  how big they are (Scale Map), how far they are turned (Rotation Map) and their color (Color Map). \
                  With several patterns connected, each tile takes one of them at random.",
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
        choice("pattern", "Pattern", 1, TILE_PATTERNS).pictured(),
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
        choice("color", "Color", 0, TILE_SAMPLER_COLORS),
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
        choice("blend_mode", "Overlap", 0, TILE_SAMPLER_OVERLAPS)
            .tip("How overlapping tiles combine: the brighter wins, they add up, or one lies over another in a random order."),
        int("seed", "Seed", 0, 0, 9999).bounded(),
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
        choice("type", "Type", 0, &["Linear", "Mirrored Linear", "Radial", "Angular", "Diamond"]).pictured(),
        angle("angle", "Angle", 0.0, -180.0, 180.0),
        vec2("center", "Center", [0.5, 0.5], 0.0, 1.0),
        float("repeat", "Repeat", 1.0, 0.1, 16.0),
        float("offset", "Offset", 0.0, -1.0, 1.0),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

/// Noise types, in the order the menu shows them (files store the index). Each is a way of
/// making noise of its own, with a look of its own, not a look of another one.
pub const NOISE_TYPES: &[&str] = &[
    "Perlin",
    "Simplex",
    "Value",
    "Billow",
    "Ridged",
    "Liquid",
    "Marble",
    "Wood",
    "Veins",
    "Streaks",
    "Flow",
    "Fibers",
    "Scratches",
    "Ripples",
    "Turing",
    "Raindrops",
    "Caustics",
    "Cells",
    "Cracks",
    "Crystal",
    "Mosaic",
    "Stones",
    "Scales",
    "Blocks",
    "Truchet",
    "Maze",
    "Spots",
    "Blotches",
    "Craters",
    "White Noise",
];
/// What each type is found by in the menu's search besides its name: what it looks like
/// and what it is called elsewhere.
const NOISE_WORDS: &[&str] = &[
    "gradient fbm fractal clouds smoke soft",
    "gradient fbm fractal clouds smooth",
    "fbm fractal clouds soft blurry",
    "turbulence puffy clouds cotton cumulus",
    "mountains ridges terrain multifractal",
    "warp swirl swirls smoke fluid",
    "stone bands veins turbulence",
    "grain rings plank timber tree",
    "lines lightning coastline electric ice",
    "brushed metal anisotropic directional stripes",
    "streamlines wind hair brush strokes current curl",
    "fur hair fabric cloth strands grass",
    "lines wear damage scuffs",
    "gabor fingerprint sand stripes",
    "reaction diffusion labyrinth coral zebra brain stripes spots",
    "rain drops ripples rings circles water puddle waves",
    "water light pool underwater",
    "voronoi worley cellular",
    "cell edges voronoi borders dry mud paint broken",
    "facets voronoi gem crumpled shards",
    "voronoi flat patches patchwork stained glass",
    "cobblestones pebbles rocks pavement voronoi",
    "fish dragon reptile roof shingles",
    "pixels squares pixelated digital",
    "arcs pipes tubes loops",
    "labyrinth corridors paths",
    "dots dirt blobs",
    "camouflage patches moisture stains",
    "moon pits holes impacts",
    "random grain static pixels",
];
const NOISE_PERLIN: u32 = 0;
const NOISE_SIMPLEX: u32 = 1;
const NOISE_VALUE: u32 = 2;
const NOISE_BILLOW: u32 = 3;
const NOISE_RIDGED: u32 = 4;
const NOISE_LIQUID: u32 = 5;
const NOISE_MARBLE: u32 = 6;
pub const NOISE_WOOD: u32 = 7;
pub const NOISE_VEINS: u32 = 8;
const NOISE_STREAKS: u32 = 9;
const NOISE_FLOW: u32 = 10;
pub const NOISE_FIBERS: u32 = 11;
pub const NOISE_SCRATCHES: u32 = 12;
const NOISE_RIPPLES: u32 = 13;
const NOISE_TURING: u32 = 14;
const NOISE_RAINDROPS: u32 = 15;
const NOISE_CAUSTICS: u32 = 16;
pub const NOISE_CELLS: u32 = 17;
pub const NOISE_CRACKS: u32 = 18;
const NOISE_CRYSTAL: u32 = 19;
const NOISE_MOSAIC: u32 = 20;
const NOISE_STONES: u32 = 21;
const NOISE_SCALES: u32 = 22;
const NOISE_BLOCKS: u32 = 23;
const NOISE_TRUCHET: u32 = 24;
const NOISE_MAZE: u32 = 25;
const NOISE_SPOTS: u32 = 26;
const NOISE_BLOTCHES: u32 = 27;
const NOISE_CRATERS: u32 = 28;
/// White noise: one random value per pixel.
pub const NOISE_WHITE: u32 = 29;
/// Every type but white noise, which is one value per pixel.
const NOISE_SCALED: &[u32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
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
];

pub static NOISE: NodeDef = NodeDef {
    id: "noise",
    name: "Noise",
    category: Category::Generators,
    description: "Tileable procedural noises: clouds, ridges, swirls, marble, wood, veins, streaks, flow, fibers, \
                  scratches, ripples, Turing patterns, raindrops, caustics, cells, cracks, crystals, mosaic, stones, \
                  scales, blocks, Truchet tiles, mazes, spots, blotches, craters and white noise.",
    keywords: "perlin simplex value fbm clouds billow ridged liquid warp swirl marble wood veins streaks flow \
               fibers fur hair scratches ripples gabor turing reaction raindrops rain caustics voronoi worley cells cracks \
               crystal mosaic stones scales blocks truchet maze spots dirt blotches craters white random grain",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        choice("type", "Type", 0, NOISE_TYPES).pictured().searched(NOISE_WORDS),
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
            .when("type", &[NOISE_LIQUID, NOISE_MARBLE, NOISE_CAUSTICS, NOISE_CRACKS]),
        int("stretch", "Stretch", 8, 1, 32)
            .at_least(1.0)
            .tip("How many times longer than tall the streaks, or the knots of the grain, are.")
            .when("type", &[NOISE_STREAKS, NOISE_WOOD]),
        int("rings", "Rings", 12, 1, 64)
            .clamped(1.0, 256.0)
            .tip("How close together the rings of the grain are.")
            .when("type", &[NOISE_WOOD]),
        angle("angle", "Angle", 0.0, -180.0, 180.0)
            .tip("Which way the flow, the fibers or the ripples run.")
            .when("type", &[NOISE_FLOW, NOISE_FIBERS, NOISE_RIPPLES]),
        float("spread", "Spread", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 keeps them all running the same way; 1 lets them turn a long way from it.")
            .when("type", &[NOISE_FLOW, NOISE_FIBERS, NOISE_RIPPLES]),
        float("jitter", "Jitter", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 puts every cell's point in its middle (a regular grid), 1 anywhere in the cell.")
            .when("type", &[NOISE_CELLS, NOISE_CRACKS, NOISE_CRYSTAL, NOISE_MOSAIC, NOISE_STONES, NOISE_SCALES]),
        float("density", "Density", 0.5, 0.0, 1.0)
            .bounded()
            .tip("How much of the image is covered.")
            .when("type", &[NOISE_TURING, NOISE_RAINDROPS, NOISE_SPOTS, NOISE_BLOTCHES, NOISE_CRATERS]),
        float("line_density", "Density", 0.15, 0.0, 1.0)
            .bounded()
            .tip("How many lines there are.")
            .when("type", &[NOISE_FIBERS, NOISE_SCRATCHES]),
        float("length", "Length", 2.0, 0.1, 6.0)
            .clamped(0.01, 6.0)
            .tip("How long the lines, or the streaks of the flow, are.")
            .when("type", &[NOISE_FLOW, NOISE_FIBERS, NOISE_SCRATCHES]),
        pixels("width", "Width", 1.5, 0.0, 16.0)
            .at_least(0.0)
            .tip("How wide the lines are in the middle (they get thinner towards their ends).")
            .when("type", &[NOISE_FIBERS, NOISE_SCRATCHES]),
        pixels("line_width", "Width", 3.0, 0.0, 16.0)
            .at_least(0.0)
            .tip("How wide the lines are.")
            .when("type", &[NOISE_VEINS, NOISE_CRACKS]),
        float("thickness", "Thickness", 0.25, 0.0, 1.0)
            .bounded()
            .tip("How thick the tubes or corridors are, as a share of a cell.")
            .when("type", &[NOISE_TRUCHET, NOISE_MAZE]),
        int("seed", "Seed", 0, 0, 9999).bounded(),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

/// How the picture of an SVG node fills the texture.
pub const SVG_FITS: &[&str] = &["Fit", "Fill", "Stretch"];
/// What an SVG node makes of the picture.
pub const SVG_OUTPUTS: &[&str] = &["Color", "Coverage"];
/// Where the text of a Text node is, from its position.
pub const TEXT_ALIGNS: &[&str] = &["Left", "Center", "Right"];

pub static SVG: NodeDef = NodeDef {
    id: "svg",
    name: "SVG",
    category: Category::Generators,
    description: "Vector graphics from an SVG file, drawn sharp at any texture size: in their colors, or as how \
                  much of each pixel they cover (a mask). Moved, turned and scaled without losing an edge; where \
                  the texture tiles, what leaves one edge comes back at the other.",
    keywords: "vector svg illustrator inkscape shape logo pattern mask file",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        file("path", "File", &["svg"]),
        choice("fit", "Fit", 0, SVG_FITS)
            .fixed()
            .tip("Fit: all of the picture, as large as the texture holds it. Fill: the texture covered, the rest \
                  cut. Stretch: to the texture's shape."),
        choice("output", "Output", 0, SVG_OUTPUTS)
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
    description: "A line of text, drawn sharp at any texture size: white where the letters are. In its font \
                  file, or in Inter, built in, for what the font does not have; the same on every system. Where \
                  the texture tiles, what leaves one edge comes back at the other.",
    keywords: "text font letters numbers label words typography type",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        text("text", "Text", "Text"),
        file("font", "Font", &["ttf", "otf", "ttc"]).tip("A font file (TrueType, OpenType); none: Inter, built in."),
        pixels("size", "Size", 200.0, 4.0, 1024.0).at_least(0.0).tip("How high the letters are (the font's em)."),
        choice("align", "Alignment", 1, TEXT_ALIGNS).tip("Which end of the line is at the position."),
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
    params: &[file("path", "File", &["png", "jpg", "jpeg", "tga", "bmp", "tif", "tiff", "exr", "hdr"])],
    role: Role::Image,
};
