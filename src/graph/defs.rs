//! Static definitions of every node kind: sockets, parameters and UI metadata.

use super::types::{DataType, ParamValue, Tiling};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeKind {
    // Values
    Input,
    Math,
    ComposeVec2,
    ComposeColor,
    // Generators
    UniformColor,
    Shape,
    TileGenerator,
    Gradient,
    Noise,
    Bitmap,
    // Filters
    Transform2D,
    Blend,
    Blur,
    Sharpen,
    Warp,
    Mirror,
    NormalMap,
    PixelMath,
    // Adjustments
    Levels,
    Invert,
    Grayscale,
    Hsl,
    GradientMap,
    ChannelShuffle,
    Posterize,
    // Output
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Values,
    Generators,
    Filters,
    Adjustments,
    Output,
}

impl Category {
    pub const ALL: [Category; 5] =
        [Category::Values, Category::Generators, Category::Filters, Category::Adjustments, Category::Output];

    pub fn name(self) -> &'static str {
        match self {
            Category::Values => "Inputs & Values",
            Category::Generators => "Generators",
            Category::Filters => "Filters",
            Category::Adjustments => "Adjustments",
            Category::Output => "Output",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SocketDef {
    pub key: &'static str,
    pub label: &'static str,
    pub ty: DataType,
}

const fn image(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image }
}

#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    Float { default: f32, min: f32, max: f32, log: bool, suffix: &'static str },
    Int { default: i32, min: i32, max: i32 },
    Bool { default: bool },
    Vec2 { default: [f32; 2], min: f32, max: f32 },
    Color { default: [f32; 4] },
    Enum { default: u32, options: &'static [&'static str] },
    Text { default: &'static str, multiline: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
    pub tooltip: &'static str,
    /// Only show the parameter when the enum parameter `.0` has one of the values `.1`.
    pub visible_if: Option<(&'static str, &'static [u32])>,
    pub exposed_by_default: bool,
}

impl ParamDef {
    const fn new(key: &'static str, label: &'static str, kind: ParamKind) -> Self {
        Self { key, label, kind, tooltip: "", visible_if: None, exposed_by_default: false }
    }
    const fn tip(mut self, tooltip: &'static str) -> Self {
        self.tooltip = tooltip;
        self
    }
    const fn when(mut self, key: &'static str, values: &'static [u32]) -> Self {
        self.visible_if = Some((key, values));
        self
    }
    const fn exposed(mut self) -> Self {
        self.exposed_by_default = true;
        self
    }

    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Float { default, .. } => ParamValue::Float(default),
            ParamKind::Int { default, .. } => ParamValue::Int(default),
            ParamKind::Bool { default } => ParamValue::Bool(default),
            ParamKind::Vec2 { default, .. } => ParamValue::Vec2(default),
            ParamKind::Color { default } => ParamValue::Color(default),
            ParamKind::Enum { default, .. } => ParamValue::Enum(default),
            ParamKind::Text { default, .. } => ParamValue::Text(default.to_string()),
        }
    }

    pub fn enum_options(&self) -> &'static [&'static str] {
        match self.kind {
            ParamKind::Enum { options, .. } => options,
            _ => &[],
        }
    }

    pub fn can_expose(&self) -> bool {
        !matches!(self.kind, ParamKind::Text { .. })
    }
}

const fn float(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: false, suffix: "" })
}
const fn float_log(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: true, suffix: "" })
}
const fn angle(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: false, suffix: "°" })
}
const fn int(key: &'static str, label: &'static str, default: i32, min: i32, max: i32) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Int { default, min, max })
}
const fn boolean(key: &'static str, label: &'static str, default: bool) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Bool { default })
}
const fn vec2(key: &'static str, label: &'static str, default: [f32; 2], min: f32, max: f32) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Vec2 { default, min, max })
}
const fn color(key: &'static str, label: &'static str, default: [f32; 4]) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Color { default })
}
const fn choice(key: &'static str, label: &'static str, default: u32, options: &'static [&'static str]) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Enum { default, options })
}
const fn text(key: &'static str, label: &'static str, default: &'static str) -> ParamDef {
    ParamDef::new(key, label, ParamKind::Text { default, multiline: false })
}

/// The tiling parameter every node carries (always first in the parameter list).
pub const TILING_PARAM: ParamDef = choice("tiling", "Tiling", Tiling::Both as u32, &Tiling::LABELS).tip(
    "How the node treats the image borders: tiling modes wrap around the edge on that axis, \
     'No tiling' clamps filters to the edge and leaves generators empty outside the frame.",
);

pub struct NodeDef {
    pub kind: NodeKind,
    pub name: &'static str,
    pub category: Category,
    pub description: &'static str,
    pub image_inputs: &'static [SocketDef],
    pub outputs: &'static [SocketDef],
    pub params: &'static [ParamDef],
    /// Produces a GPU image (shown as thumbnail and in the 2D view).
    pub has_image: bool,
}

impl NodeDef {
    pub fn param(&self, key: &str) -> Option<&'static ParamDef> {
        if key == TILING_PARAM.key {
            return Some(&TILING_PARAM);
        }
        self.params.iter().find(|p| p.key == key)
    }

    /// All parameters including the common tiling parameter.
    pub fn all_params(&self) -> impl Iterator<Item = &'static ParamDef> {
        std::iter::once(&TILING_PARAM).chain(self.params.iter())
    }

    pub fn is_value_node(&self) -> bool {
        !self.has_image
    }
}

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
pub const SHAPE_POLYGON: u32 = 3;
pub const SHAPE_STAR: u32 = 4;
pub const SHAPE_RING: u32 = 5;
pub const SHAPE_CROSS: u32 = 6;
pub const SHAPE_ROUNDED: u32 = 2;
/// Shapes with hard edges (softness applies); the rest are height profiles.
pub const SOLID_SHAPES: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 7];

pub const TILE_PATTERNS: &[&str] = &[
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

pub const OUTPUT_FORMATS: &[&str] = &["RGBA", "RGB", "RG", "R", "G", "B", "A"];

/// Channels (indices into RGBA) exported for each output format.
pub fn output_format_channels(format: u32) -> &'static [usize] {
    match format {
        0 => &[0, 1, 2, 3],
        1 => &[0, 1, 2],
        2 => &[0, 1],
        3 => &[0],
        4 => &[1],
        5 => &[2],
        _ => &[3],
    }
}

const SHUFFLE_SOURCES: &[&str] = &["A.Red", "A.Green", "A.Blue", "A.Alpha", "B.Red", "B.Green", "B.Blue", "B.Alpha", "Zero", "One"];

const NO_SOCKETS: &[SocketDef] = &[];
const IMAGE_OUT: &[SocketDef] = &[image("output", "Output")];
const IMAGE_IN: &[SocketDef] = &[image("input", "Input")];

pub const DEFAULT_MATH_EXPRESSION: &str = "(1+x)*sin(myVar)-2.4/cos(myVar2)";

macro_rules! def {
    ($kind:ident, $name:expr, $cat:ident, $desc:expr, $inputs:expr, $outputs:expr, $params:expr, $img:expr) => {
        NodeDef {
            kind: NodeKind::$kind,
            name: $name,
            category: Category::$cat,
            description: $desc,
            image_inputs: $inputs,
            outputs: $outputs,
            params: $params,
            has_image: $img,
        }
    };
}

pub static DEFS: &[NodeDef] = &[
    def!(Input, "Input", Values,
        "Reads one of the graph's input values (see the Input Values panel).",
        NO_SOCKETS, &[SocketDef { key: "value", label: "Value", ty: DataType::Float }], &[], false),
    def!(Math, "Math", Values,
        "Evaluates a scalar expression. Every unknown word becomes an input on the left.",
        NO_SOCKETS, &[SocketDef { key: "result", label: "Result", ty: DataType::Float }],
        &[
            text("expression", "Expression", DEFAULT_MATH_EXPRESSION),
            boolean("auto_bind", "Auto-bind inputs", true)
                .tip("Unconnected variables named like a graph input take that input's value."),
        ], false),
    def!(ComposeVec2, "Compose Vec2", Values,
        "Builds a 2D vector from two numbers.",
        NO_SOCKETS, &[SocketDef { key: "value", label: "Vec2", ty: DataType::Vec2 }],
        &[float("x", "X", 0.0, -1.0, 1.0).exposed(), float("y", "Y", 0.0, -1.0, 1.0).exposed()], false),
    def!(ComposeColor, "Compose Color", Values,
        "Builds an RGBA color from four numbers.",
        NO_SOCKETS, &[SocketDef { key: "value", label: "Color", ty: DataType::Color }],
        &[
            float("r", "Red", 0.5, 0.0, 1.0).exposed(),
            float("g", "Green", 0.5, 0.0, 1.0).exposed(),
            float("b", "Blue", 0.5, 0.0, 1.0).exposed(),
            float("a", "Alpha", 1.0, 0.0, 1.0),
        ], false),
    // ---------------------------------------------------------------- generators
    def!(UniformColor, "Uniform Color", Generators,
        "Fills the image with a single color.",
        NO_SOCKETS, IMAGE_OUT, &[color("color", "Color", [0.5, 0.5, 0.5, 1.0])], true),
    def!(Shape, "Shape", Generators,
        "Anti-aliased geometric shapes and height profiles.",
        NO_SOCKETS, IMAGE_OUT,
        &[
            choice("shape", "Shape", 0, SHAPES),
            vec2("size", "Size", [0.6, 0.6], 0.0, 2.0).tip("Extent of the shape relative to its cell (1 = touches the edges)."),
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
        ], true),
    def!(TileGenerator, "Tile Generator", Generators,
        "Scatters a pattern on a grid with per-tile randomness.",
        &[image("pattern", "Pattern")], IMAGE_OUT,
        &[
            choice("pattern", "Pattern", 1, TILE_PATTERNS),
            int("x_amount", "X Amount", 8, 1, 64),
            int("y_amount", "Y Amount", 8, 1, 64),
            vec2("size", "Size", [0.9, 0.9], 0.0, 2.0).tip("Pattern size relative to the cell."),
            float("scale", "Scale", 1.0, 0.0, 2.0),
            float("row_offset", "Row Offset", 0.0, 0.0, 1.0).tip("Shift of every other row (0.5 = bricks)."),
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
            choice("blend_mode", "Overlap", 0, &["Max", "Add"]),
            int("seed", "Seed", 0, 0, 9999),
        ], true),
    def!(Gradient, "Gradient", Generators,
        "Linear, radial, angular and diamond gradients.",
        NO_SOCKETS, IMAGE_OUT,
        &[
            choice("type", "Type", 0, &["Linear", "Mirrored Linear", "Radial", "Angular", "Diamond"]),
            angle("angle", "Angle", 0.0, -180.0, 180.0),
            vec2("center", "Center", [0.5, 0.5], 0.0, 1.0),
            float("repeat", "Repeat", 1.0, 0.1, 16.0),
            float("offset", "Offset", 0.0, -1.0, 1.0),
            boolean("invert", "Invert", false),
        ], true),
    def!(Noise, "Noise", Generators,
        "Tileable procedural noises (fBm, ridged, cells, white).",
        NO_SOCKETS, IMAGE_OUT,
        &[
            choice("type", "Type", 0, &["Perlin fBm", "Value fBm", "Ridged", "Cells", "Cell Edges", "White Noise"]),
            int("scale", "Scale", 8, 1, 128).tip("Number of noise cells across the image (integer so it tiles)."),
            int("octaves", "Octaves", 5, 1, 10).when("type", &[0, 1, 2]),
            float("roughness", "Roughness", 0.5, 0.0, 1.0).when("type", &[0, 1, 2]),
            float("jitter", "Jitter", 1.0, 0.0, 1.0).when("type", &[3, 4]),
            int("seed", "Seed", 0, 0, 9999),
            boolean("invert", "Invert", false),
        ], true),
    def!(Bitmap, "Bitmap", Generators,
        "Loads an image file (PNG, JPEG, TGA, BMP).",
        NO_SOCKETS, IMAGE_OUT, &[text("path", "File", "")], true),
    // ---------------------------------------------------------------- filters
    def!(Transform2D, "Transform 2D", Filters,
        "Offset, rotate and scale the input. Drag the gizmo in the 2D view.",
        IMAGE_IN, IMAGE_OUT,
        &[
            vec2("offset", "Offset", [0.0, 0.0], -1.0, 1.0),
            angle("rotation", "Rotation", 0.0, -180.0, 180.0).tip("Degrees, counter-clockwise."),
            vec2("scale", "Scale", [1.0, 1.0], 0.01, 4.0),
            vec2("pivot", "Pivot", [0.5, 0.5], 0.0, 1.0),
            choice("filtering", "Filtering", 0, &["Bilinear", "Nearest"]),
            color("matte", "Matte Color", [0.0, 0.0, 0.0, 0.0]).tip("Fill outside the image when tiling is off."),
        ], true),
    def!(Blend, "Blend", Filters,
        "Blends a foreground over a background with an optional mask.",
        &[image("foreground", "Foreground"), image("background", "Background"), image("mask", "Mask")], IMAGE_OUT,
        &[
            choice("mode", "Blending Mode", 0, BLEND_MODES),
            float("opacity", "Opacity", 1.0, 0.0, 1.0),
            choice("alpha", "Alpha Blending", 0, &["Use Foreground Alpha", "Ignore Alpha"]),
            boolean("clamp", "Clamp Result", true),
        ], true),
    def!(Blur, "Blur", Filters,
        "High quality Gaussian blur.",
        IMAGE_IN, IMAGE_OUT,
        &[float("radius", "Radius", 8.0, 0.0, 128.0)
            .tip("Blur radius in pixels at 1024 px, so the look is the same at every resolution.")], true),
    def!(Sharpen, "Sharpen", Filters,
        "Unsharp-mask sharpening.",
        IMAGE_IN, IMAGE_OUT,
        &[
            float("intensity", "Intensity", 1.0, 0.0, 10.0),
            float("radius", "Radius", 1.5, 0.1, 16.0).tip("Detail radius in pixels at 1024 px."),
        ], true),
    def!(Warp, "Warp", Filters,
        "Displaces the input along the slopes of a gradient map.",
        &[image("input", "Input"), image("warp", "Warp Map")], IMAGE_OUT,
        &[float("intensity", "Intensity", 0.1, 0.0, 1.0)], true),
    def!(Mirror, "Mirror", Filters,
        "Mirrors one half of the image onto the other.",
        IMAGE_IN, IMAGE_OUT,
        &[
            choice("mode", "Mode", 0, &["Left → Right", "Right → Left", "Top → Bottom", "Bottom → Top", "Four-way"]),
            float("offset", "Axis", 0.5, 0.0, 1.0),
        ], true),
    def!(NormalMap, "Normal", Filters,
        "Converts a height map into a tangent-space normal map.",
        IMAGE_IN, IMAGE_OUT,
        &[
            float("intensity", "Intensity", 4.0, 0.0, 32.0),
            choice("format", "Format", 0, &["OpenGL (Y+)", "DirectX (Y-)"]),
        ], true),
    def!(PixelMath, "Pixel Math", Filters,
        "Evaluates an expression for every pixel on the GPU. Unknown words become image inputs; \
         u, v, px, py, width and height are built in.",
        NO_SOCKETS, IMAGE_OUT,
        &[
            text("expression", "Expression", "u * v"),
            choice("channel", "Read Channel", 0, &["Luminance", "Red", "Green", "Blue", "Alpha"])
                .tip("Which channel of connected images the variables read."),
            boolean("auto_bind", "Auto-bind inputs", true)
                .tip("Unconnected variables named like a graph input take that input's value."),
        ], true),
    // ---------------------------------------------------------------- adjustments
    def!(Levels, "Levels", Adjustments,
        "Remaps input levels with gamma, then output levels.",
        IMAGE_IN, IMAGE_OUT,
        &[
            float("in_low", "Input Low", 0.0, 0.0, 1.0),
            float("in_high", "Input High", 1.0, 0.0, 1.0),
            float_log("gamma", "Gamma", 1.0, 0.1, 10.0),
            float("out_low", "Output Low", 0.0, 0.0, 1.0),
            float("out_high", "Output High", 1.0, 0.0, 1.0),
            choice("channels", "Channels", 0, &["RGB", "RGBA", "R", "G", "B", "A"]),
            boolean("clamp", "Clamp", true),
        ], true),
    def!(Invert, "Invert", Adjustments,
        "Inverts colors.",
        IMAGE_IN, IMAGE_OUT, &[boolean("invert_alpha", "Invert Alpha", false)], true),
    def!(Grayscale, "Grayscale", Adjustments,
        "Converts to grayscale.",
        IMAGE_IN, IMAGE_OUT,
        &[choice("mode", "Mode", 0, &["Luminance", "Average", "Red", "Green", "Blue", "Alpha", "Max"])], true),
    def!(Hsl, "HSL", Adjustments,
        "Hue, saturation and lightness adjustment.",
        IMAGE_IN, IMAGE_OUT,
        &[
            angle("hue", "Hue", 0.0, -180.0, 180.0),
            float("saturation", "Saturation", 0.0, -1.0, 1.0),
            float("lightness", "Lightness", 0.0, -1.0, 1.0),
        ], true),
    def!(GradientMap, "Gradient Map", Adjustments,
        "Colorizes a grayscale image with a color ramp.",
        IMAGE_IN, IMAGE_OUT,
        &[
            choice("stops", "Stops", 1, &["2 colors", "3 colors", "4 colors"]),
            color("color_a", "Color A (0)", [0.05, 0.04, 0.03, 1.0]),
            color("color_b", "Color B", [0.45, 0.25, 0.12, 1.0]).when("stops", &[1, 2]),
            float("pos_b", "Position B", 0.5, 0.0, 1.0).when("stops", &[1, 2]),
            color("color_c", "Color C", [0.75, 0.55, 0.3, 1.0]).when("stops", &[2]),
            float("pos_c", "Position C", 0.75, 0.0, 1.0).when("stops", &[2]),
            color("color_d", "Color D (1)", [0.95, 0.9, 0.8, 1.0]),
        ], true),
    def!(ChannelShuffle, "Channel Shuffle", Adjustments,
        "Builds an image from arbitrary channels of two inputs.",
        &[image("a", "A"), image("b", "B")], IMAGE_OUT,
        &[
            choice("red", "Red", 0, SHUFFLE_SOURCES),
            choice("green", "Green", 1, SHUFFLE_SOURCES),
            choice("blue", "Blue", 2, SHUFFLE_SOURCES),
            choice("alpha", "Alpha", 3, SHUFFLE_SOURCES),
        ], true),
    def!(Posterize, "Posterize", Adjustments,
        "Quantizes values to a number of steps.",
        IMAGE_IN, IMAGE_OUT, &[int("steps", "Steps", 8, 2, 64)], true),
    // ---------------------------------------------------------------- output
    def!(Output, "Output", Output,
        "A graph output that can be exported to an image file.",
        IMAGE_IN, NO_SOCKETS,
        &[
            text("name", "Name", "output"),
            choice("format", "Format", 0, OUTPUT_FORMATS).tip("Channels written by the export."),
            choice("bit_depth", "Bit Depth", 0, &["8 bit", "16 bit"]),
        ], true),
];

pub fn def(kind: NodeKind) -> &'static NodeDef {
    DEFS.iter().find(|d| d.kind == kind).expect("every node kind has a definition")
}

/// An entry in the node library panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibraryItem {
    pub label: &'static str,
    pub kind: NodeKind,
    /// For input nodes: the type of the graph input to create.
    pub input_type: Option<DataType>,
    pub category: Category,
}

pub fn library() -> Vec<LibraryItem> {
    let mut items = vec![
        LibraryItem { label: "Float Input", kind: NodeKind::Input, input_type: Some(DataType::Float), category: Category::Values },
        LibraryItem { label: "Integer Input", kind: NodeKind::Input, input_type: Some(DataType::Int), category: Category::Values },
        LibraryItem { label: "Boolean Input", kind: NodeKind::Input, input_type: Some(DataType::Bool), category: Category::Values },
        LibraryItem { label: "Vec2 Input", kind: NodeKind::Input, input_type: Some(DataType::Vec2), category: Category::Values },
        LibraryItem { label: "Color Input", kind: NodeKind::Input, input_type: Some(DataType::Color), category: Category::Values },
    ];
    for d in DEFS.iter().filter(|d| d.kind != NodeKind::Input) {
        items.push(LibraryItem { label: d.name, kind: d.kind, input_type: None, category: d.category });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_one_definition() {
        let mut kinds: Vec<NodeKind> = DEFS.iter().map(|d| d.kind).collect();
        let n = kinds.len();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), n);
        assert_eq!(n, 26);
    }

    #[test]
    fn params_are_consistent() {
        for d in DEFS {
            let mut keys: Vec<&str> = d.all_params().map(|p| p.key).collect();
            let n = keys.len();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), n, "duplicate parameter key in {}", d.name);
            for p in d.params {
                if let Some((key, _)) = p.visible_if {
                    assert!(
                        matches!(d.param(key).map(|p| p.kind), Some(ParamKind::Enum { .. })),
                        "{}::{} depends on a missing enum",
                        d.name,
                        p.key
                    );
                }
                if let ParamKind::Enum { default, options } = p.kind {
                    assert!((default as usize) < options.len());
                }
            }
            if d.has_image && d.kind != NodeKind::Output {
                assert!(d.outputs.iter().any(|o| o.ty == DataType::Image), "{}", d.name);
            }
        }
    }
}
