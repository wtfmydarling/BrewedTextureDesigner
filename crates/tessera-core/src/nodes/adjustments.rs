//! Adjustments: per-pixel color and value remapping.

use super::*;

const SHUFFLE_SOURCES: &[&str] =
    &["A.Red", "A.Green", "A.Blue", "A.Alpha", "B.Red", "B.Green", "B.Blue", "B.Alpha", "Zero", "One"];

pub static LEVELS: NodeDef = NodeDef {
    id: "levels",
    name: "Levels",
    category: Category::Adjustments,
    description: "Remaps input levels with gamma, then output levels. The properties show the input histogram.",
    keywords: "contrast gamma black white point histogram",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        float("in_low", "Input Low", 0.0, 0.0, 1.0),
        float("in_high", "Input High", 1.0, 0.0, 1.0),
        float_log("gamma", "Gamma", 1.0, 0.1, 10.0),
        float("out_low", "Output Low", 0.0, 0.0, 1.0),
        float("out_high", "Output High", 1.0, 0.0, 1.0),
        choice("channels", "Channels", 0, &["RGB", "RGBA", "R", "G", "B", "A"]),
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
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        float("position", "Position", 0.5, 0.0, 1.0).tip("Input level that becomes mid gray; higher keeps less."),
        float("contrast", "Contrast", 0.5, 0.0, 1.0).tip("0 keeps the input as a ramp, 1 is a hard threshold."),
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
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[choice("mode", "Mode", 0, &["Luminance", "Average", "Red", "Green", "Blue", "Alpha", "Max"])],
    role: Role::Image,
};

pub static HSL: NodeDef = NodeDef {
    id: "hsl",
    name: "HSL",
    category: Category::Adjustments,
    description: "Hue, saturation and lightness adjustment.",
    keywords: "hue saturation lightness color",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        angle("hue", "Hue", 0.0, -180.0, 180.0),
        float("saturation", "Saturation", 0.0, -1.0, 1.0),
        float("lightness", "Lightness", 0.0, -1.0, 1.0),
    ],
    role: Role::Image,
};

pub static GRADIENT_MAP: NodeDef = NodeDef {
    id: "gradient_map",
    name: "Gradient Map",
    category: Category::Adjustments,
    description: "Colorizes a grayscale image with a color ramp of any number of stops. The gradient can \
                  be picked along a line in the 2D view.",
    keywords: "colorize ramp lut palette",
    inputs: IMAGE_IN,
    outputs: IMAGE_OUT,
    params: &[
        gradient(
            "gradient",
            "Gradient",
            &[(0.0, [0.05, 0.04, 0.03, 1.0]), (0.5, [0.45, 0.25, 0.12, 1.0]), (1.0, [0.95, 0.9, 0.8, 1.0])],
        ),
        choice("channel", "Read", 0, &["Luminance", "Red", "Green", "Blue", "Alpha", "Average"])
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
    inputs: &[image("a", "A"), image_opt("b", "B")],
    outputs: IMAGE_OUT,
    params: &[
        choice("red", "Red", 0, SHUFFLE_SOURCES),
        choice("green", "Green", 1, SHUFFLE_SOURCES),
        choice("blue", "Blue", 2, SHUFFLE_SOURCES),
        choice("alpha", "Alpha", 3, SHUFFLE_SOURCES),
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
    params: &[int("steps", "Steps", 8, 2, 64)],
    role: Role::Image,
};
