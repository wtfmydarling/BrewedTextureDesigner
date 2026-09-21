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
        choice("filtering", "Filtering", 0, &["Bilinear", "Nearest"]),
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
        choice("mode", "Blending Mode", 0, BLEND_MODES),
        float("opacity", "Opacity", 1.0, 0.0, 1.0).bounded(),
        choice("alpha", "Alpha Blending", 0, &["Use Foreground Alpha", "Ignore Alpha"]),
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
    params: &[pixels("radius", "Radius", 8.0, 0.0, 128.0).at_least(0.0)
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
        pixels("intensity", "Intensity", 16.0, -128.0, 128.0)
            .tip("Distance travelled along the slope, in pixels of a 1024 px image. Negative flows uphill."),
        int("samples", "Samples", 16, 2, 64).bounded(),
        choice("mode", "Mode", 0, &["Blur", "Min", "Max"]),
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
        choice("mode", "Mode", 0, &["Left to Right", "Right to Left", "Top to Bottom", "Bottom to Top", "Four-way"]),
        float("offset", "Axis", 0.5, 0.0, 1.0),
    ],
    role: Role::Image,
};

pub static PIXEL_MATH: NodeDef = NodeDef {
    id: "pixel_math",
    name: "Pixel Math",
    category: Category::Filters,
    description: "Evaluates an expression for every pixel on the GPU. Unknown words become image inputs; \
                  u, v, px, py, width and height are built in.",
    keywords: "expression formula shader per pixel custom",
    inputs: NO_SOCKETS,
    outputs: GRAY_OUT,
    params: &[
        expression("expression", "Expression", "u * v", true),
        choice("channel", "Read Channel", 0, &["Luminance", "Red", "Green", "Blue", "Alpha"])
            .fixed()
            .tip("Which channel of connected images the variables read."),
        boolean("auto_bind", "Auto-bind", true)
            .fixed()
            .tip("Unconnected variables named like a graph variable take that variable's value."),
    ],
    role: Role::Image,
};

/// Built-in per-pixel variables of the Pixel Math node.
pub const PIXEL_MATH_BUILTINS: &[&str] = &["u", "v", "px", "py", "width", "height"];
