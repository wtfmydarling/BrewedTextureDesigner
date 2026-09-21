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
    outputs: IMAGE_OUT,
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
    outputs: IMAGE_OUT,
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

pub static GRADIENT: NodeDef = NodeDef {
    id: "gradient",
    name: "Gradient",
    category: Category::Generators,
    description: "Linear, radial, angular and diamond grayscale gradients.",
    keywords: "ramp linear radial angular diamond",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
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

/// Noise types, in the order the menu shows them (files store the index). Each is a
/// different way of making noise, with a pattern of its own; ways of looking at the same
/// pattern are the styles of a type.
pub const NOISE_TYPES: &[&str] = &[
    "Perlin",
    "Liquid",
    "Marble",
    "Streaks",
    "Ripples",
    "Blocks",
    "Cells",
    "Spots",
    "Scratches",
    "White Noise",
];
const NOISE_PERLIN: u32 = 0;
const NOISE_LIQUID: u32 = 1;
const NOISE_MARBLE: u32 = 2;
const NOISE_STREAKS: u32 = 3;
const NOISE_RIPPLES: u32 = 4;
const NOISE_BLOCKS: u32 = 5;
pub const NOISE_CELLS: u32 = 6;
pub const NOISE_SPOTS: u32 = 7;
pub const NOISE_SCRATCHES: u32 = 8;
/// White noise: one random value per pixel.
pub const NOISE_WHITE: u32 = 9;
/// Every type but white noise, which is one value per pixel.
const NOISE_SCALED: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];
/// Types made of octaves, each finer than the last.
const NOISE_FRACTAL: &[u32] = &[NOISE_PERLIN, NOISE_LIQUID, NOISE_MARBLE, NOISE_STREAKS, NOISE_BLOCKS];

/// What Perlin noise shows: its clouds, the folds between them, or the ridges along them.
pub const PERLIN_STYLES: &[&str] = &["Clouds", "Billow", "Ridged"];
/// What cells show: the distance to their middle, their borders, a flat color each, a
/// tilted facet each, or a rounded stone each.
pub const CELL_STYLES: &[&str] = &["Distance", "Edges", "Flat", "Facets", "Stones"];

pub static NOISE: NodeDef = NodeDef {
    id: "noise",
    name: "Noise",
    category: Category::Generators,
    description: "Tileable procedural noises: clouds, swirls, marble, streaks, ripples, blocks, cells, spots, \
                  scratches and white noise.",
    keywords: "perlin fbm clouds billow ridged liquid warp swirl marble veins wood streaks anisotropic brushed \
               ripples gabor fingerprint blocks pixel voronoi worley cells crystal facets mosaic edges cracks \
               stones spots dirt scratches fibers fur hair white random grain",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        choice("type", "Type", 0, NOISE_TYPES).pictured(),
        choice("perlin_style", "Style", 0, PERLIN_STYLES).pictured().when("type", &[NOISE_PERLIN]),
        choice("cell_style", "Style", 0, CELL_STYLES).pictured().when("type", &[NOISE_CELLS]),
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
            .tip("How far the noise is pushed around.")
            .when("type", &[NOISE_LIQUID, NOISE_MARBLE]),
        int("stretch", "Stretch", 8, 1, 32)
            .at_least(1.0)
            .tip("How many times longer the streaks are than they are tall.")
            .when("type", &[NOISE_STREAKS]),
        angle("angle", "Angle", 0.0, -180.0, 180.0)
            .tip("Which way the ripples run.")
            .when("type", &[NOISE_RIPPLES]),
        float("spread", "Spread", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 keeps every ripple running the same way; 1 lets them curl like a fingerprint.")
            .when("type", &[NOISE_RIPPLES]),
        float("jitter", "Jitter", 1.0, 0.0, 1.0)
            .bounded()
            .tip("0 puts every cell's point in its middle (a regular grid), 1 anywhere in the cell.")
            .when("type", &[NOISE_CELLS]),
        float("density", "Density", 0.5, 0.0, 1.0)
            .bounded()
            .tip("How much of the image is covered.")
            .when("type", &[NOISE_SPOTS]),
        float("line_density", "Density", 0.15, 0.0, 1.0)
            .bounded()
            .tip("How many lines there are: few for scratches, many for fibers or fur.")
            .when("type", &[NOISE_SCRATCHES]),
        float("length", "Length", 2.0, 0.1, 6.0)
            .clamped(0.01, 6.0)
            .tip("How long the lines are, in cells.")
            .when("type", &[NOISE_SCRATCHES]),
        pixels("width", "Width", 1.5, 0.0, 16.0)
            .at_least(0.0)
            .tip("How wide the lines are in the middle (they get thinner towards their ends).")
            .when("type", &[NOISE_SCRATCHES]),
        int("seed", "Seed", 0, 0, 9999).bounded(),
        boolean("invert", "Invert", false),
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
    outputs: IMAGE_OUT,
    params: &[file("path", "File", &["png", "jpg", "jpeg", "tga", "bmp", "tif", "tiff", "exr"])],
    role: Role::Image,
};
