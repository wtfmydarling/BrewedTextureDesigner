//! Height & normal: nodes that interpret an image as a height field.

use super::*;

pub static NORMAL: NodeDef = NodeDef {
    id: "normal",
    name: "Normal",
    category: Category::Effects,
    description: "Converts a height map into a tangent-space normal map.",
    keywords: "normal map bump height",
    inputs: GRAY_IN,
    outputs: COLOR_OUT,
    params: &[
        float("intensity", "Intensity", 4.0, 0.0, 32.0),
        choice("format", "Format", 0, &["OpenGL (Y+)", "DirectX (Y-)"]),
    ],
    role: Role::Image,
};

pub static AMBIENT_OCCLUSION: NodeDef = NodeDef {
    id: "ambient_occlusion",
    name: "Ambient Occlusion",
    category: Category::Effects,
    description: "Darkens cavities of a height map by tracing the horizon in many directions.",
    keywords: "ao cavity occlusion shadow",
    inputs: GRAY_IN,
    outputs: GRAY_OUT,
    params: &[
        pixels("radius", "Radius", 48.0, 1.0, 256.0).at_least(0.0).tip("Search distance in pixels of a 1024 px image."),
        float("height", "Height Depth", 0.03, 0.0, 0.5)
            .tip("Height of white relative to the image width: larger values make everything deeper."),
        float("intensity", "Intensity", 1.0, 0.0, 4.0),
    ],
    role: Role::Image,
};

pub static CURVATURE: NodeDef = NodeDef {
    id: "curvature",
    name: "Curvature",
    category: Category::Effects,
    description: "Convex areas of a height map become bright, concave areas dark (0.5 is flat).",
    keywords: "convex concave edges wear cavity",
    inputs: GRAY_IN,
    outputs: GRAY_OUT,
    params: &[
        pixels("radius", "Radius", 2.0, 0.5, 32.0).at_least(0.0).tip("Scale of the detected features, in pixels of a 1024 px image."),
        float("intensity", "Intensity", 16.0, 0.0, 64.0),
    ],
    role: Role::Image,
};

pub static EDGE_DETECT: NodeDef = NodeDef {
    id: "edge_detect",
    name: "Edge Detect",
    category: Category::Effects,
    description: "Finds edges (strong changes in brightness) with a Sobel filter.",
    keywords: "sobel outline contour",
    inputs: GRAY_IN,
    outputs: GRAY_OUT,
    params: &[
        pixels("width", "Width", 1.0, 0.5, 16.0).at_least(0.0).tip("Edge width in pixels of a 1024 px image."),
        float("intensity", "Intensity", 2.0, 0.0, 16.0),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

pub static EMBOSS: NodeDef = NodeDef {
    id: "emboss",
    name: "Emboss",
    category: Category::Effects,
    description: "Lights a height map from one direction; multiplies the optional color input.",
    keywords: "relief light shading bevel",
    inputs: &[gray("height", "Height"), image_opt("color", "Color")],
    outputs: IMAGE_OUT,
    params: &[
        angle("light_angle", "Light Angle", 135.0, -180.0, 180.0),
        angle("elevation", "Elevation", 35.0, 1.0, 90.0),
        float("intensity", "Intensity", 4.0, 0.0, 32.0),
        choice("output", "Output", 0, &["Shading (0.5 = flat)", "Lit color"]),
    ],
    role: Role::Image,
};
