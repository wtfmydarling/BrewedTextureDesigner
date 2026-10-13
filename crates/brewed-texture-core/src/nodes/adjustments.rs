//! Adjustments: per-pixel color and value remapping.

use super::*;

const SHUFFLE_SOURCES: &[&str] = &["A.Red", "A.Green", "A.Blue", "A.Alpha", "B.Red", "B.Green", "B.Blue", "B.Alpha", "Zero", "One"];
const SHUFFLE_SOURCE_IDS: &[&str] = &["a_red", "a_green", "a_blue", "a_alpha", "b_red", "b_green", "b_blue", "b_alpha", "zero", "one"];

pub static LEVELS: NodeDef = NodeDef {
    id: "levels",
    name: "Levels",
    category: Category::Adjustments,
    description: "Remaps input levels with gamma, then output levels. The properties show the input histogram.",
    keywords: "contrast gamma black white point histogram",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        float("in_low", "Input Low", 0.0, 0.0, 1.0).bounded(),
        float("in_high", "Input High", 1.0, 0.0, 1.0).bounded(),
        // The slider covers the useful gammas; the handle on the histogram reaches further,
        // right up to black and white, and a gamma of zero or less is no curve at all.
        float_log("gamma", "Gamma", 1.0, 0.1, 10.0).clamped(0.01, 100.0),
        float("out_low", "Output Low", 0.0, 0.0, 1.0).bounded(),
        float("out_high", "Output High", 1.0, 0.0, 1.0).bounded(),
        choice("channels", "Channels", 0, &["RGB", "RGBA", "R", "G", "B", "A"], &["rgb", "rgba", "r", "g", "b", "a"]),
        boolean("clamp", "Clamp", true),
    ],
    role: Role::Image,
};

pub static HISTOGRAM_SCAN: NodeDef = NodeDef {
    id: "histogram_scan",
    name: "Histogram Scan",
    category: Category::Adjustments,
    description: "Thresholds a grayscale image with a movable, softenable cut: the basic tool to turn \
                  noise and height into masks.",
    keywords: "threshold mask cut contrast",
    inputs: GRAY_IN,
    outputs: GRAY_OUT,
    params: &[
        float("position", "Position", 0.5, 0.0, 1.0).bounded().tip("Input level that becomes mid gray; higher keeps less."),
        float("contrast", "Contrast", 0.5, 0.0, 1.0).bounded().tip("0 keeps the input as a ramp, 1 is a hard threshold."),
        boolean("invert", "Invert", false),
    ],
    role: Role::Image,
};

pub static INVERT: NodeDef = NodeDef {
    id: "invert",
    name: "Invert",
    category: Category::Adjustments,
    description: "Inverts colors.",
    keywords: "negative",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[boolean("invert_alpha", "Invert Alpha", false)],
    role: Role::Image,
};

pub static GRAYSCALE: NodeDef = NodeDef {
    id: "grayscale",
    name: "Grayscale",
    category: Category::Adjustments,
    description: "Converts to grayscale.",
    keywords: "desaturate luminance channel",
    inputs: COLOR_IN,
    outputs: GRAY_OUT,
    params: &[choice(
        "mode",
        "Mode",
        0,
        &["Luminance", "Average", "Red", "Green", "Blue", "Alpha", "Max"],
        &["luminance", "average", "red", "green", "blue", "alpha", "max"],
    )],
    role: Role::Image,
};

pub static HSL: NodeDef = NodeDef {
    id: "hsl",
    name: "HSL",
    category: Category::Adjustments,
    description: "Hue, saturation and lightness adjustment.",
    keywords: "hue saturation lightness color",
    inputs: COLOR_IN,
    outputs: COLOR_OUT,
    params: &[
        angle("hue", "Hue", 0.0, -180.0, 180.0),
        float("saturation", "Saturation", 0.0, -1.0, 1.0).bounded(),
        float("lightness", "Lightness", 0.0, -1.0, 1.0).bounded(),
    ],
    role: Role::Image,
};

pub static GRADIENT_MAP: NodeDef = NodeDef {
    id: "gradient_map",
    name: "Gradient Map",
    category: Category::Adjustments,
    description: "Colors an image with a ramp of any number of stops, its luminance (or a channel) picking the \
                  color. Pick the gradient from the screen: drag across anything, and its colors become the ramp.",
    keywords: "colorize ramp lut palette",
    // Either kind: the channel it reads is one of a color image.
    inputs: IMAGE_IN,
    outputs: COLOR_OUT,
    params: &[
        gradient("gradient", "Gradient", &[(0.0, [0.05, 0.04, 0.03, 1.0]), (0.5, [0.45, 0.25, 0.12, 1.0]), (1.0, [0.95, 0.9, 0.8, 1.0])]),
        choice("channel", "Read", 0, &["Luminance", "Red", "Green", "Blue", "Alpha", "Average"], &["luminance", "red", "green", "blue", "alpha", "average"])
            .tip("Which value of the input picks the position in the gradient."),
        boolean("keep_alpha", "Keep Input Alpha", true).tip("Multiply the gradient's alpha with the input's alpha."),
    ],
    role: Role::Image,
};

pub static CHANNEL_SHUFFLE: NodeDef = NodeDef {
    id: "channel_shuffle",
    name: "Channel Shuffle",
    category: Category::Adjustments,
    description: "Builds an image from arbitrary channels of two inputs.",
    keywords: "swizzle pack merge split channels",
    inputs: &[color_image("a", "A"), color_image_opt("b", "B")],
    outputs: COLOR_OUT,
    params: &[
        choice("red", "Red", 0, SHUFFLE_SOURCES, SHUFFLE_SOURCE_IDS),
        choice("green", "Green", 1, SHUFFLE_SOURCES, SHUFFLE_SOURCE_IDS),
        choice("blue", "Blue", 2, SHUFFLE_SOURCES, SHUFFLE_SOURCE_IDS),
        choice("alpha", "Alpha", 3, SHUFFLE_SOURCES, SHUFFLE_SOURCE_IDS),
    ],
    role: Role::Image,
};

pub static GRADIENT_DYNAMIC: NodeDef = NodeDef {
    id: "gradient_dynamic",
    name: "Gradient Dynamic",
    category: Category::Adjustments,
    description: "A gradient map whose gradient is an image: each value picks the color along a row or column of \
                  the Gradient image, at Position, or where the Position Map says for each pixel.",
    keywords: "gradient map colorize dynamic remap lookup image",
    inputs: &[gray("input", "Input"), color_image("gradient", "Gradient"), gray_opt("position_map", "Position Map")],
    outputs: COLOR_OUT,
    params: &[
        choice("orientation", "Orientation", 0, &["Horizontal", "Vertical"], &["horizontal", "vertical"])
            .tip("Whether the colors are read along a row of the Gradient image or along a column."),
        float("position", "Position", 0.5, 0.0, 1.0)
            .bounded()
            .tip("Which row (or column) of the Gradient image the colors are read along, where no Position Map says."),
    ],
    role: Role::Image,
};

/// Which channels of a color image a Curve changes.
pub const CURVE_CHANNELS: &[&str] = &["RGB", "Red", "Green", "Blue", "Alpha"];
const CURVE_CHANNEL_IDS: &[&str] = &["rgb", "red", "green", "blue", "alpha"];

pub static CURVE: NodeDef = NodeDef {
    id: "curve",
    name: "Curve",
    category: Category::Adjustments,
    description: "Maps each value through a curve: drag its points, click to add one, drag one out of the square \
                  to remove it. On a color image, the channels chosen go through it.",
    keywords: "curves tone contrast gamma remap tonal",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        curve("curve", "Curve", &[[0.0, 0.0], [1.0, 1.0]]),
        choice("channels", "Channels", 0, CURVE_CHANNELS, CURVE_CHANNEL_IDS).tip("On a color image, the channels the curve changes."),
    ],
    role: Role::Image,
};

pub static RGBA_MERGE: NodeDef = NodeDef {
    id: "rgba_merge",
    name: "RGBA Merge",
    category: Category::Adjustments,
    description: "Puts four grayscale images into the red, green, blue and alpha of one color image, as game \
                  engines take several maps in one (ORM: occlusion, roughness, metallic). Unconnected: its value.",
    keywords: "pack packing channels merge combine orm mask rgba",
    inputs: &[gray_opt("red", "Red"), gray_opt("green", "Green"), gray_opt("blue", "Blue"), gray_opt("alpha", "Alpha")],
    outputs: COLOR_OUT,
    params: &[
        float("red_value", "Red", 0.0, 0.0, 1.0).bounded().tip("The red where nothing is connected to Red."),
        float("green_value", "Green", 0.0, 0.0, 1.0).bounded().tip("The green where nothing is connected to Green."),
        float("blue_value", "Blue", 0.0, 0.0, 1.0).bounded().tip("The blue where nothing is connected to Blue."),
        float("alpha_value", "Alpha", 1.0, 0.0, 1.0).bounded().tip("The alpha where nothing is connected to Alpha."),
    ],
    role: Role::Image,
};

pub static POSTERIZE: NodeDef = NodeDef {
    id: "posterize",
    name: "Posterize",
    category: Category::Adjustments,
    description: "Quantizes values to a number of steps.",
    keywords: "quantize steps bands",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[int("steps", "Steps", 8, 2, 64).bounded()],
    role: Role::Image,
};
