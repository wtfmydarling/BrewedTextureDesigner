//! Filters: transforms, blending, blurs and distortions.

use super::*;

pub const BLEND_MODES: &[&str] = &[
    "Normal",
    "Add",
    "Subtract",
    "Multiply",
    "Screen",
    "Overlay",
    "Soft Light",
    "Hard Light",
    "Darken (Min)",
    "Lighten (Max)",
    "Difference",
    "Exclusion",
    "Divide",
    "Color Dodge",
    "Color Burn",
    "Linear Burn",
    "Linear Light",
    "Vivid Light",
    "Pin Light",
];
/// What a file calls each of [`BLEND_MODES`].
const BLEND_MODE_IDS: &[&str] = &[
    "normal",
    "add",
    "subtract",
    "multiply",
    "screen",
    "overlay",
    "soft_light",
    "hard_light",
    "darken",
    "lighten",
    "difference",
    "exclusion",
    "divide",
    "color_dodge",
    "color_burn",
    "linear_burn",
    "linear_light",
    "vivid_light",
    "pin_light",
];

pub static TRANSFORM_2D: NodeDef = NodeDef {
    id: "transform_2d",
    name: "Transform 2D",
    category: Category::Filters,
    description: "Moves, rotates and scales the input. Drag the gizmo in the 2D view.",
    keywords: "move rotate scale offset translate",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
        angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
        vec2("scale", "Scale", [1.0, 1.0], 0.01, 4.0),
        vec2("pivot", "Pivot", [0.5, 0.5], 0.0, 1.0),
        choice("filtering", "Filtering", 0, &["Bilinear", "Nearest"], &["bilinear", "nearest"]),
        color("matte", "Matte Color", [0.0, 0.0, 0.0, 0.0]).tip("Fill outside the image on axes that do not tile."),
    ],
    role: Role::Image,
};

pub static BLEND: NodeDef = NodeDef {
    id: "blend",
    name: "Blend",
    category: Category::Filters,
    description: "Blends a foreground over a background with an optional mask.",
    keywords: "mix composite layer multiply add screen overlay",
    inputs: &[image("foreground", "Foreground"), image("background", "Background"), gray_opt("mask", "Mask")],
    outputs: IMAGE_OUT,
    params: &[
        choice("mode", "Blending Mode", 0, BLEND_MODES, BLEND_MODE_IDS),
        float("opacity", "Opacity", 1.0, 0.0, 1.0).bounded(),
        choice("alpha", "Alpha Blending", 0, &["Use Foreground Alpha", "Ignore Alpha"], &["foreground", "ignore"]),
        boolean("clamp", "Clamp Result", true),
    ],
    role: Role::Image,
};

pub static BLUR: NodeDef = NodeDef {
    id: "blur",
    name: "Blur",
    category: Category::Filters,
    description: "High quality Gaussian blur; the cost does not grow with the radius.",
    keywords: "gaussian soften smooth",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[pixels("radius", "Radius", 8.0, 0.0, 128.0)
        .at_least(0.0)
        .tip("Blur radius in pixels of a 1024 px image, so the look is the same at every resolution.")],
    role: Role::Image,
};

pub static DIRECTIONAL_BLUR: NodeDef = NodeDef {
    id: "directional_blur",
    name: "Directional Blur",
    category: Category::Filters,
    description: "Motion blur along one direction.",
    keywords: "motion streak smear",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        angle("angle", "Angle", 0.0, -180.0, 180.0),
        pixels("length", "Length", 24.0, 0.0, 256.0).at_least(0.0).tip("Length of the streak in pixels of a 1024 px image."),
        boolean("symmetric", "Symmetric", true).tip("Blur in both directions instead of trailing behind."),
    ],
    role: Role::Image,
};

pub static SLOPE_BLUR: NodeDef = NodeDef {
    id: "slope_blur",
    name: "Slope Blur",
    category: Category::Filters,
    description: "Smears the input along the slopes of a height map (dripping, erosion, fibers).",
    keywords: "erosion drip smear flow",
    inputs: &[image("input", "Input"), gray("slope", "Slope Map")],
    outputs: IMAGE_OUT,
    params: &[
        pixels("intensity", "Intensity", 16.0, -128.0, 128.0).tip("Distance travelled along the slope, in pixels of a 1024 px image. Negative flows uphill."),
        int("samples", "Samples", 16, 2, 64).bounded(),
        choice("mode", "Mode", 0, &["Blur", "Min", "Max"], &["blur", "min", "max"]),
    ],
    role: Role::Image,
};

pub static SHARPEN: NodeDef = NodeDef {
    id: "sharpen",
    name: "Sharpen",
    category: Category::Filters,
    description: "Unsharp-mask sharpening.",
    keywords: "detail crisp unsharp",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        float("intensity", "Intensity", 1.0, 0.0, 10.0),
        pixels("radius", "Radius", 1.5, 0.1, 16.0).at_least(0.0).tip("Detail radius in pixels of a 1024 px image."),
    ],
    role: Role::Image,
};

pub static WARP: NodeDef = NodeDef {
    id: "warp",
    name: "Warp",
    category: Category::Filters,
    description: "Displaces the input along the slopes of a gradient map.",
    keywords: "distort displace",
    inputs: &[image("input", "Input"), gray("warp", "Warp Map")],
    outputs: IMAGE_OUT,
    params: &[float("intensity", "Intensity", 0.1, 0.0, 1.0)],
    role: Role::Image,
};

pub static DIRECTIONAL_WARP: NodeDef = NodeDef {
    id: "directional_warp",
    name: "Directional Warp",
    category: Category::Filters,
    description: "Displaces the input in one direction by the brightness of an intensity map.",
    keywords: "distort displace offset",
    inputs: &[image("input", "Input"), gray("intensity", "Intensity Map")],
    outputs: IMAGE_OUT,
    params: &[
        angle("angle", "Angle", 0.0, -180.0, 180.0),
        float("intensity", "Intensity", 0.1, -1.0, 1.0).tip("Displacement for white, as a fraction of the image."),
    ],
    role: Role::Image,
};

pub static MIRROR: NodeDef = NodeDef {
    id: "mirror",
    name: "Mirror",
    category: Category::Filters,
    description: "Mirrors one half of the image onto the other.",
    keywords: "symmetry flip reflect",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        choice(
            "mode",
            "Mode",
            0,
            &["Left to Right", "Right to Left", "Top to Bottom", "Bottom to Top", "Four-way"],
            &["left_to_right", "right_to_left", "top_to_bottom", "bottom_to_top", "four_way"],
        ),
        float("offset", "Axis", 0.5, 0.0, 1.0),
    ],
    role: Role::Image,
};

pub static PIXEL_MATH: NodeDef = NodeDef {
    id: "pixel_math",
    name: "Pixel Math",
    category: Category::Filters,
    description: "An expression computed for every pixel on the GPU. Unknown words are image inputs (img.rgb, \
                  sample(img, x, y)); u, v, width, height and vectors are built in. One value is gray, a vector \
                  color.",
    keywords: "expression formula shader per pixel custom processor color",
    inputs: NO_SOCKETS,
    outputs: IMAGE_OUT,
    params: &[
        expression("expression", "Expression", "u * v", true),
        choice("channel", "Read Channel", 0, READ_CHANNELS, READ_CHANNEL_IDS).fixed().tip("Which channel of connected images the variables read."),
        boolean("auto_bind", "Auto-bind", true).fixed().tip("Unconnected variables named like a graph variable take that variable's value."),
    ],
    role: Role::Image,
};

/// Built-in per-pixel variables of the Pixel Math node.
pub const PIXEL_MATH_BUILTINS: &[&str] = &["u", "v", "px", "py", "width", "height"];

/// Which channel of the images its expression names a Pixel Math or an FX-Map reads.
pub const READ_CHANNELS: &[&str] = &["Luminance", "Red", "Green", "Blue", "Alpha"];
pub(crate) const READ_CHANNEL_IDS: &[&str] = &["luminance", "red", "green", "blue", "alpha"];

/// What a Distance node makes of the distances, in the order of its "mode" parameter.
pub const DISTANCE_MODES: &[&str] = &["From the Shapes", "Into the Shapes", "Signed"];
const DISTANCE_MODE_IDS: &[&str] = &["outside", "inside", "signed"];

pub static DISTANCE: NodeDef = NodeDef {
    id: "distance",
    name: "Distance",
    category: Category::Filters,
    description: "How far each pixel is from the shapes of a mask: white on them, fading to black at the distance; \
                  or inward from their edges; or signed. Distances cross the edges where it tiles.",
    keywords: "distance field sdf glow outline expand grow spread jump flood",
    inputs: &[gray("mask", "Mask")],
    outputs: GRAY_OUT,
    params: &[
        choice("mode", "Mode", 0, DISTANCE_MODES, DISTANCE_MODE_IDS).tip(
            "From the Shapes: 1 on them, 0 at the distance. Into the Shapes: 0 at their edges, 1 at the distance \
             into them. Signed: 0.5 at the edges, 1 at the distance into them, 0 at the distance away.",
        ),
        pixels("distance", "Distance", 64.0, 1.0, 512.0).at_least(0.0).tip("How far it goes."),
        float("threshold", "Threshold", 0.5, 0.0, 1.0).bounded().tip("Where the mask is a shape: at this value and above."),
    ],
    role: Role::Image,
};

pub static NON_UNIFORM_BLUR: NodeDef = NodeDef {
    id: "non_uniform_blur",
    name: "Non-uniform Blur",
    category: Category::Filters,
    description: "Blurs each pixel as much as a map says: black not at all, white by the radius. Without a map, \
                  everywhere by the radius. The cost is the same for any radius.",
    keywords: "blur variable depth of field map mask soft",
    inputs: &[image("input", "Input"), gray_opt("map", "Blur Map")],
    outputs: IMAGE_OUT,
    params: &[pixels("radius", "Radius", 16.0, 0.0, 256.0).at_least(0.0).tip("How far white on the map blurs.")],
    role: Role::Image,
};

pub static MAKE_IT_TILE: NodeDef = NodeDef {
    id: "make_it_tile",
    name: "Make It Tile",
    category: Category::Filters,
    description: "Makes an image that does not tile, a photo say, tile: copies of it shifted by half the image \
                  cover the seams, each shown where its own seams are not, blended over the width.",
    keywords: "tile tileable seamless seam photo repeat wrap",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[float("blend", "Blend Width", 0.08, 0.01, 0.24).bounded().tip("Where one copy gives way to the next, as a part of the image.")],
    role: Role::Image,
};

/// What a Flood Fill gives each shape, in the order of its "output" parameter.
pub const FLOOD_FILL_OUTPUTS: &[&str] = &["Random Grayscale", "Random Color"];
const FLOOD_FILL_OUTPUT_IDS: &[&str] = &["random_gray", "random_color"];

pub static FLOOD_FILL: NodeDef = NodeDef {
    id: "flood_fill",
    name: "Flood Fill",
    category: Category::Filters,
    description: "Gives each shape of a mask (connected across the edges where it tiles) a random gray or color of \
                  its own: every brick, tile or puzzle piece different. Black around the shapes.",
    keywords: "flood fill random color grayscale islands regions per tile variation connected",
    inputs: &[gray("mask", "Mask")],
    outputs: IMAGE_OUT,
    params: &[
        choice("output", "Output", 0, FLOOD_FILL_OUTPUTS, FLOOD_FILL_OUTPUT_IDS).fixed(),
        int("seed", "Seed", 0, 0, 1000),
        float("threshold", "Threshold", 0.5, 0.0, 1.0).bounded().tip("Where the mask is a shape: at this value and above."),
    ],
    role: Role::Image,
};
