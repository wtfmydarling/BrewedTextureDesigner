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

/// How the edge of a bevel rises, in the order of its "profile" parameter.
pub const BEVEL_PROFILES: &[&str] = &["Linear", "Smooth", "Round", "Concave"];

pub static BEVEL: NodeDef = NodeDef {
    id: "bevel",
    name: "Bevel",
    category: Category::Effects,
    description: "A height map of the shapes of a mask with their edges beveled: rising from the edge to full \
                  height over the width, in the profile chosen; outward, the slope goes down around the shapes. \
                  Where the texture tiles, the shapes go across its edges.",
    keywords: "bevel chamfer round edge height inset rounded corner",
    inputs: &[gray("mask", "Mask")],
    outputs: GRAY_OUT,
    params: &[
        pixels("width", "Width", 24.0, 1.0, 256.0).at_least(0.0).tip("How wide the slope is."),
        choice("profile", "Profile", 2, BEVEL_PROFILES).tip(
            "Linear: a straight slope. Smooth: eased at both ends. Round: a quarter circle, steep at the edge. \
             Concave: hollowed, steep at the top.",
        ),
        choice("direction", "Direction", 0, &["Inward", "Outward"])
            .tip("Inward: the slope is inside the shapes. Outward: around them, the shapes at full height."),
        float("threshold", "Threshold", 0.5, 0.0, 1.0).bounded().tip("Where the mask is a shape: at this value and above."),
    ],
    role: Role::Image,
};

pub static HEIGHT_BLEND: NodeDef = NodeDef {
    id: "height_blend",
    name: "Height Blend",
    category: Category::Effects,
    description: "Two height maps together, the higher one showing where it is higher: stones in sand, moss \
                  in the cracks. The top one is raised by the offset; the contrast sharpens where one gives \
                  way to the other. It gives the height, or where the top one shows.",
    keywords: "height blend combine max mix terrain layer",
    inputs: &[gray("top", "Top"), gray("bottom", "Bottom"), gray_opt("mask", "Mask")],
    outputs: GRAY_OUT,
    params: &[
        float("offset", "Height Offset", 0.0, -1.0, 1.0).tip("Raises the top height map (lowers it, below 0)."),
        float("contrast", "Contrast", 0.9, 0.0, 1.0).bounded().tip("How sharply one gives way to the other."),
        choice("output", "Output", 0, &["Height", "Mask"]).tip("The heights together, or where the top one shows."),
    ],
    role: Role::Image,
};

pub static NORMAL_COMBINE: NodeDef = NodeDef {
    id: "normal_combine",
    name: "Normal Combine",
    category: Category::Effects,
    description: "The details of one normal map on another: Reoriented bends the detail over the base, \
                  Whiteout and UDN add their slopes. A mask says where the detail goes.",
    keywords: "normal combine blend detail reoriented rnm whiteout udn",
    inputs: &[color_image("base", "Base"), color_image("detail", "Detail"), gray_opt("mask", "Mask")],
    outputs: COLOR_OUT,
    params: &[
        choice("technique", "Technique", 0, &["Reoriented", "Whiteout", "UDN"]),
        float("intensity", "Detail Intensity", 1.0, 0.0, 2.0).at_least(0.0),
    ],
    role: Role::Image,
};
