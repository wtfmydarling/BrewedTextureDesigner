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
        choice("shape", "Shape", 0, SHAPES),
        vec2("size", "Size", [0.6, 0.6], 0.0, 2.0)
            .tip("Extent of the shape relative to its cell (1 = touches the edges)."),
        float("scale", "Scale", 1.0, 0.0, 2.0),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
        vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
        int("repeat", "Repeat", 1, 1, 32).tip("Number of copies per side."),
        float("softness", "Softness", 0.01, 0.0, 1.0).when("shape", SOLID_SHAPES),
        int("sides", "Sides", 6, 3, 16).when("shape", &[SHAPE_POLYGON, SHAPE_STAR]),
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
        choice("pattern", "Pattern", 1, TILE_PATTERNS),
        int("x_amount", "X Amount", 8, 1, 64),
        int("y_amount", "Y Amount", 8, 1, 64),
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
        float("random_scale", "Random Scale", 0.0, 0.0, 1.0),
        float("random_offset", "Random Offset", 0.0, 0.0, 1.0),
        float("luminance", "Luminance", 1.0, 0.0, 1.0),
        float("random_luminance", "Random Luminance", 0.0, 0.0, 1.0),
        float("mask_random", "Mask Random", 0.0, 0.0, 1.0).tip("Probability of removing a tile."),
        float("softness", "Softness", 0.02, 0.0, 1.0),
        int("sides", "Sides", 6, 3, 16).when("pattern", &[SHAPE_POLYGON, SHAPE_STAR]),
        float("inner_radius", "Inner Radius", 0.5, 0.05, 1.0).when("pattern", &[SHAPE_STAR]),
        float("thickness", "Thickness", 0.2, 0.01, 1.0).when("pattern", &[SHAPE_RING, SHAPE_CROSS]),
        float("corner_radius", "Corner Radius", 0.25, 0.0, 1.0).when("pattern", &[SHAPE_ROUNDED]),
        choice("blend_mode", "Overlap", 0, &["Max", "Add"]).tip("How overlapping tiles combine."),
        int("seed", "Seed", 0, 0, 9999),
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
        choice("type", "Type", 0, &["Linear", "Mirrored Linear", "Radial", "Angular", "Diamond"]),
        angle("angle", "Angle", 0.0, -180.0, 180.0),
        vec2("center", "Center", [0.5, 0.5], 0.0, 1.0),
        float("repeat", "Repeat", 1.0, 0.1, 16.0),
        float("offset", "Offset", 0.0, -1.0, 1.0),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

pub static NOISE: NodeDef = NodeDef {
    id: "noise",
    name: "Noise",
    category: Category::Generators,
    description: "Tileable procedural noises: Perlin and value fBm, ridged, cells, cell edges, white noise.",
    keywords: "perlin fbm clouds voronoi worley cells ridged white random",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        choice("type", "Type", 0, &["Perlin fBm", "Value fBm", "Ridged", "Cells", "Cell Edges", "White Noise"]),
        int("scale", "Scale", 8, 1, 128).tip("Number of noise cells across the image (an integer, so it tiles)."),
        int("octaves", "Octaves", 5, 1, 10).when("type", &[0, 1, 2]),
        float("roughness", "Roughness", 0.5, 0.0, 1.0).when("type", &[0, 1, 2]),
        float("jitter", "Jitter", 1.0, 0.0, 1.0).when("type", &[3, 4]),
        int("seed", "Seed", 0, 0, 9999),
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
