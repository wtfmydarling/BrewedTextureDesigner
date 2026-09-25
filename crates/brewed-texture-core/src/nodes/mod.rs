//! Registry of node types.
//!
//! A node type is plain data: a [`NodeDef`] lists its sockets, its parameters (with
//! ranges, defaults, visibility rules and tooltips) and its [`Role`]. Everything else is
//! driven by these descriptions: the graph derives sockets from them, the UI builds the
//! library and the property editors from them, and the engine looks up the GPU kernel by
//! the node's id. Adding a node type means adding a definition here (and, for image
//! nodes, a kernel in the engine); no front-end code has to change.

mod adjustments;
mod effects;
mod filters;
mod generators;
mod output;
mod values;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::curve::Curve;
use crate::gradient::Gradient;
use crate::types::{DataType, ParamValue, Tiling};

pub use adjustments::CURVE_CHANNELS;
pub use effects::BEVEL_PROFILES;
pub use filters::{BLEND_MODES, DISTANCE_MODES, FLOOD_FILL_OUTPUTS, PIXEL_MATH_BUILTINS};
pub use generators::{
    SVG_FITS, SVG_OUTPUTS, TEXT_ALIGNS,
    NOISE_CELLS, NOISE_CRACKS, NOISE_FIBERS, NOISE_SCRATCHES, NOISE_TYPES, NOISE_VEINS, NOISE_WHITE, NOISE_WOOD,
    SHAPE_POLYGON, SHAPE_ROUNDED, SHAPE_STAR, SHAPES, TILE_PATTERN_IMAGE, TILE_SAMPLER_COLORS, TILE_SAMPLER_OVERLAPS,
};
pub use output::{
    OUTPUT_FORMATS, USAGE_AO, USAGE_BASE_COLOR, USAGE_EMISSIVE, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL,
    USAGE_ROUGHNESS, output_format_channels,
};
pub use values::{DEFAULT_MATH_EXPRESSION, IMAGE_INPUT_KINDS};

// ---------------------------------------------------------------------------- categories

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Values,
    Generators,
    Filters,
    Effects,
    Adjustments,
    Output,
    /// Graphs used as nodes (see [`Role::Instance`]): none of them is in the library, the
    /// packages are.
    Graphs,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Values,
        Category::Generators,
        Category::Filters,
        Category::Effects,
        Category::Adjustments,
        Category::Output,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Values => "Inputs & Values",
            Category::Generators => "Generators",
            Category::Filters => "Filters",
            Category::Effects => "Height & Normal",
            Category::Adjustments => "Adjustments",
            Category::Output => "Output",
            Category::Graphs => "Graphs",
        }
    }
}

// ---------------------------------------------------------------------------- sockets

/// What an image is, as in Substance Designer: grayscale (one channel, half the memory of
/// a color image) or color (red, green, blue and alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageKind {
    Gray,
    Color,
}

impl ImageKind {
    pub fn name(self) -> &'static str {
        match self {
            ImageKind::Gray => "Grayscale",
            ImageKind::Color => "Color",
        }
    }
}

/// What an image socket takes or gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// A grayscale image; a color image arriving here is read as its luminance.
    Gray,
    /// A color image; a grayscale image arriving here is read as gray in every channel.
    Color,
    /// Either. An output of this kind is what its `Any` inputs are: color when one of them
    /// is, grayscale otherwise.
    Any,
}

#[derive(Clone, Copy, Debug)]
pub struct SocketDef {
    pub key: &'static str,
    pub label: &'static str,
    pub ty: DataType,
    /// For image sockets, which kind of image; value sockets ignore it.
    pub slot: Slot,
    /// Optional inputs do not produce a warning when they are not connected.
    pub optional: bool,
}

/// An image input of either kind (or, as an output, one that follows its inputs).
pub(crate) const fn image(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Any, optional: false }
}

pub(crate) const fn image_opt(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Any, optional: true }
}

pub(crate) const fn gray(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Gray, optional: false }
}

pub(crate) const fn gray_opt(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Gray, optional: true }
}

pub(crate) const fn color_image(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Color, optional: false }
}

pub(crate) const fn color_image_opt(key: &'static str, label: &'static str) -> SocketDef {
    SocketDef { key, label, ty: DataType::Image, slot: Slot::Color, optional: true }
}

pub(crate) const fn value(key: &'static str, label: &'static str, ty: DataType) -> SocketDef {
    SocketDef { key, label, ty, slot: Slot::Any, optional: false }
}

pub(crate) const NO_SOCKETS: &[SocketDef] = &[];
/// An output of the kind of the inputs.
pub(crate) const IMAGE_OUT: &[SocketDef] = &[image("output", "Output")];
pub(crate) const GRAY_OUT: &[SocketDef] = &[gray("output", "Output")];
pub(crate) const COLOR_OUT: &[SocketDef] = &[color_image("output", "Output")];
/// An input of either kind.
pub(crate) const IMAGE_IN: &[SocketDef] = &[image("input", "Input")];
pub(crate) const GRAY_IN: &[SocketDef] = &[gray("input", "Input")];
pub(crate) const COLOR_IN: &[SocketDef] = &[color_image("input", "Input")];

// ---------------------------------------------------------------------------- parameters

#[derive(Clone, Copy, Debug)]
pub enum ParamKind {
    /// `min` / `max` are the slider range; typed values may go beyond it.
    Float {
        default: f32,
        min: f32,
        max: f32,
        log: bool,
        suffix: &'static str,
    },
    Int {
        default: i32,
        min: i32,
        max: i32,
    },
    Bool {
        default: bool,
    },
    Vec2 {
        default: [f32; 2],
        min: f32,
        max: f32,
    },
    Color {
        default: [f32; 4],
    },
    Enum {
        default: u32,
        options: &'static [&'static str],
    },
    Text {
        default: &'static str,
    },
    /// Path of a file on disk (e.g. the image of a Bitmap node).
    File {
        extensions: &'static [&'static str],
    },
    /// Source of a Math (`pixel: false`) or Pixel Math (`pixel: true`) expression. Its free
    /// variables become input sockets of the node.
    Expression {
        default: &'static str,
        pixel: bool,
    },
    Gradient {
        default: &'static [(f32, [f32; 4])],
    },
    /// A tone curve, through its points (see [`crate::curve`]).
    Curve {
        default: &'static [[f32; 2]],
    },
}

/// A parameter of a node: of a node type (`'static`), or of a graph used as a node, whose
/// parameters are the graph's input values (see [`crate::instance`]).
#[derive(Clone, Copy, Debug)]
pub struct ParamDef<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub kind: ParamKind,
    pub tooltip: &'a str,
    /// Only shown (and only has a socket) while the enum parameter `.0` has one of the
    /// values `.1`. If that enum is itself exposed, its value is only known at evaluation
    /// time, so the parameter stays visible.
    pub visible_if: Option<(&'static str, &'static [u32])>,
    pub exposed_by_default: bool,
    /// Whether the parameter can get an input socket. Structural settings (tiling, output
    /// format, file paths, expressions, gradients) cannot.
    pub exposable: bool,
    /// Values the parameter is held to, whatever is typed or dragged. Most parameters have
    /// none: a slider covers the useful range and going past it is how unusual textures are
    /// found. Only what would mean nothing (a gamma of zero, a third of a tile) is bounded.
    pub limits: Option<(f32, f32)>,
    /// The options are chosen by how they look: the menu shows each as a picture of what
    /// the node makes with it, not only its name.
    pub pictured: bool,
    /// Words each option is found by in its menu's search besides its name, one string of
    /// words per option (or none).
    pub words: &'static [&'static str],
}

impl<'a> ParamDef<'a> {
    pub(crate) const fn new(key: &'a str, label: &'a str, kind: ParamKind) -> Self {
        let exposable = matches!(
            kind,
            ParamKind::Float { .. }
                | ParamKind::Int { .. }
                | ParamKind::Bool { .. }
                | ParamKind::Vec2 { .. }
                | ParamKind::Color { .. }
                | ParamKind::Enum { .. }
        );
        Self {
            key,
            label,
            kind,
            tooltip: "",
            visible_if: None,
            exposed_by_default: false,
            exposable,
            limits: None,
            pictured: false,
            words: &[],
        }
    }
    /// Hold the value to the range of the slider: outside it the parameter means nothing.
    pub(crate) const fn bounded(mut self) -> Self {
        self.limits = match self.kind {
            ParamKind::Float { min, max, .. } | ParamKind::Vec2 { min, max, .. } => Some((min, max)),
            ParamKind::Int { min, max, .. } => Some((min as f32, max as f32)),
            _ => None,
        };
        self
    }
    /// Hold the value to `min` and above, leaving the top open.
    pub(crate) const fn at_least(mut self, min: f32) -> Self {
        self.limits = Some((min, f32::INFINITY));
        self
    }
    /// Hold the value to a range of its own, wider than the slider.
    pub(crate) const fn clamped(mut self, min: f32, max: f32) -> Self {
        self.limits = Some((min, max));
        self
    }

    /// Show the options as pictures of what the node makes with each.
    pub(crate) const fn pictured(mut self) -> Self {
        self.pictured = true;
        self
    }
    /// Words each option is found by besides its name (one string of words per option).
    pub(crate) const fn searched(mut self, words: &'static [&'static str]) -> Self {
        self.words = words;
        self
    }

    pub(crate) const fn tip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = tooltip;
        self
    }
    pub(crate) const fn when(mut self, key: &'static str, values: &'static [u32]) -> Self {
        self.visible_if = Some((key, values));
        self
    }
    pub(crate) const fn exposed(mut self) -> Self {
        self.exposed_by_default = true;
        self
    }
    /// Not exposable: a structural setting of the node.
    pub(crate) const fn fixed(mut self) -> Self {
        self.exposable = false;
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
            ParamKind::Text { default } | ParamKind::Expression { default, .. } => {
                ParamValue::Text(default.to_string())
            }
            ParamKind::File { .. } => ParamValue::Text(String::new()),
            ParamKind::Gradient { default } => ParamValue::Gradient(Gradient::from_pairs(default)),
            ParamKind::Curve { default } => ParamValue::Curve(Curve::from_points(default)),
        }
    }

    pub fn enum_options(&self) -> &'static [&'static str] {
        match self.kind {
            ParamKind::Enum { options, .. } => options,
            _ => &[],
        }
    }

    /// Does option `option` match a search? Every word of the query has to be in the
    /// option's name, or begin one of its words.
    pub fn option_matches(&self, option: usize, query: &str) -> bool {
        let Some(name) = self.enum_options().get(option) else { return false };
        let name = name.to_lowercase();
        let words = self.words.get(option).map(|w| w.to_lowercase()).unwrap_or_default();
        query
            .to_lowercase()
            .split_whitespace()
            .all(|q| name.contains(q) || words.split_whitespace().any(|w| w.starts_with(q)))
    }

    pub fn can_expose(&self) -> bool {
        self.exposable
    }

    /// Does a stored value have the right shape for this parameter?
    pub fn accepts(&self, value: &ParamValue) -> bool {
        matches!(
            (self.kind, value),
            (ParamKind::Float { .. }, ParamValue::Float(_))
                | (ParamKind::Int { .. }, ParamValue::Int(_))
                | (ParamKind::Bool { .. }, ParamValue::Bool(_))
                | (ParamKind::Vec2 { .. }, ParamValue::Vec2(_))
                | (ParamKind::Color { .. }, ParamValue::Color(_))
                | (ParamKind::Enum { .. }, ParamValue::Enum(_))
                | (ParamKind::Text { .. } | ParamKind::File { .. } | ParamKind::Expression { .. }, ParamValue::Text(_))
                | (ParamKind::Gradient { .. }, ParamValue::Gradient(_))
                | (ParamKind::Curve { .. }, ParamValue::Curve(_))
        )
    }

    /// Bring a value of the right shape into the valid domain (enum index in range,
    /// finite numbers, sorted gradient). Values of the wrong shape become the default.
    pub fn sanitize(&self, value: ParamValue) -> ParamValue {
        if !self.accepts(&value) {
            return self.default_value();
        }
        let bound = |v: f32| match self.limits {
            Some((lo, hi)) => v.clamp(lo, hi),
            None => v,
        };
        let fin = |v: f32, d: f32| bound(if v.is_finite() { v } else { d });
        match (self.kind, value) {
            (ParamKind::Float { default, .. }, ParamValue::Float(v)) => ParamValue::Float(fin(v, default)),
            (ParamKind::Int { default, .. }, ParamValue::Int(v)) => {
                ParamValue::Int(fin(v as f32, default as f32).round() as i32)
            }
            (ParamKind::Vec2 { default, .. }, ParamValue::Vec2(v)) => {
                ParamValue::Vec2([fin(v[0], default[0]), fin(v[1], default[1])])
            }
            (ParamKind::Color { default }, ParamValue::Color(c)) => {
                ParamValue::Color(std::array::from_fn(|i| fin(c[i], default[i])))
            }
            (ParamKind::Enum { options, .. }, ParamValue::Enum(v)) => {
                ParamValue::Enum(v.min(options.len().saturating_sub(1) as u32))
            }
            (_, ParamValue::Gradient(g)) => ParamValue::Gradient(g.normalized()),
            (_, ParamValue::Curve(c)) => ParamValue::Curve(c.normalized()),
            (_, v) => v,
        }
    }
}

pub(crate) const fn float(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: false, suffix: "" })
}
pub(crate) const fn float_log(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: true, suffix: "" })
}
/// A length in pixels of a 1024 px image; scaled with the resolution so the look does
/// not depend on it.
pub(crate) const fn pixels(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: false, suffix: " px" })
}
pub(crate) const fn angle(key: &'static str, label: &'static str, default: f32, min: f32, max: f32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Float { default, min, max, log: false, suffix: "°" })
}
pub(crate) const fn int(key: &'static str, label: &'static str, default: i32, min: i32, max: i32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Int { default, min, max })
}
pub(crate) const fn boolean(key: &'static str, label: &'static str, default: bool) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Bool { default })
}
pub(crate) const fn vec2(key: &'static str, label: &'static str, default: [f32; 2], min: f32, max: f32) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Vec2 { default, min, max })
}
pub(crate) const fn color(key: &'static str, label: &'static str, default: [f32; 4]) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Color { default })
}
pub(crate) const fn choice(
    key: &'static str,
    label: &'static str,
    default: u32,
    options: &'static [&'static str],
) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Enum { default, options })
}
pub(crate) const fn text(key: &'static str, label: &'static str, default: &'static str) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Text { default })
}
pub(crate) const fn file(key: &'static str, label: &'static str, extensions: &'static [&'static str]) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::File { extensions })
}
pub(crate) const fn expression(key: &'static str, label: &'static str, default: &'static str, pixel: bool) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Expression { default, pixel })
}
pub(crate) const fn gradient(key: &'static str, label: &'static str, default: &'static [(f32, [f32; 4])]) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Gradient { default })
}
pub(crate) const fn curve(key: &'static str, label: &'static str, default: &'static [[f32; 2]]) -> ParamDef<'static> {
    ParamDef::new(key, label, ParamKind::Curve { default }).fixed()
}

/// The tiling parameter every node carries (always first in the parameter list).
pub const TILING_PARAM: ParamDef<'static> = choice("tiling", "Tiling", Tiling::Both as u32, &Tiling::LABELS).fixed().tip(
    "How the node treats the image borders: a tiling axis wraps around the edge, so \
     filters and generators produce seamless results; without tiling, filters clamp to the \
     edge and generators leave the outside empty.",
);

/// The kind of image a node makes: [`NodeDef::output_kind`], except where its parameters
/// decide. (A Bitmap is the kind of its file, which only the engine knows once it is read.)
pub fn image_kind(kind: NodeKind, params: &crate::eval::Params, inputs: &[Option<ImageKind>]) -> ImageKind {
    if kind == NodeKind::TILE_GENERATOR {
        // Tiles of a shape are grayscale; tiles of an image are what the image is.
        let image = params.u("pattern") == TILE_PATTERN_IMAGE && inputs.first() == Some(&Some(ImageKind::Color));
        return if image { ImageKind::Color } else { ImageKind::Gray };
    }
    if kind == NodeKind::PIXEL_MATH {
        // One value is gray; a list of them, the channels of a color image.
        let values = crate::expr::compile_cached(params.text("expression")).as_ref().as_ref().map_or(1, |p| p.components());
        return if values > 1 { ImageKind::Color } else { ImageKind::Gray };
    }
    if kind == NodeKind::TILE_SAMPLER {
        // Tiles in colors of their own, or of the Color Map, are in color; so are tiles of
        // images when one of them is.
        let images = params.u("pattern") == TILE_PATTERN_IMAGE && inputs.iter().take(4).any(|k| *k == Some(ImageKind::Color));
        return if params.u("color") != 0 || images { ImageKind::Color } else { ImageKind::Gray };
    }
    if kind == NodeKind::IMAGE_INPUT {
        return if params.u("kind") == 1 { ImageKind::Color } else { ImageKind::Gray };
    }
    if kind == NodeKind::FLOOD_FILL {
        return if params.u("output") == 1 { ImageKind::Color } else { ImageKind::Gray };
    }
    if kind == NodeKind::SVG {
        // In its colors, or how much of each pixel it covers.
        return if params.u("output") == 0 { ImageKind::Color } else { ImageKind::Gray };
    }
    kind.def().output_kind(inputs)
}

// ---------------------------------------------------------------------------- node types

/// What a node produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// An image computed on the GPU (thumbnail, 2D view, tiling).
    Image,
    /// A CPU value (Math, Compose …).
    Value,
    /// Reads the graph variable named by the node; the output has the given type.
    Variable(DataType),
    /// A graph output: an image that can be exported.
    Output,
    /// A graph used as a node: its sockets and parameters are the graph's image inputs,
    /// outputs and input values (see [`crate::instance`]), and it shows its first output.
    Instance,
}

pub struct NodeDef {
    /// Stable identifier used in files and by the engine to find the kernel.
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub description: &'static str,
    /// Extra words that find the node in searches.
    pub keywords: &'static str,
    /// Fixed input sockets.
    pub inputs: &'static [SocketDef],
    pub outputs: &'static [SocketDef],
    pub params: &'static [ParamDef<'static>],
    pub role: Role,
}

impl NodeDef {
    pub fn param(&self, key: &str) -> Option<&'static ParamDef<'static>> {
        if key == TILING_PARAM.key {
            return Some(&TILING_PARAM);
        }
        self.params.iter().find(|p| p.key == key)
    }

    /// All parameters, the common tiling parameter first.
    pub fn all_params(&self) -> impl Iterator<Item = &'static ParamDef<'static>> + use<> {
        std::iter::once(&TILING_PARAM).chain(self.params.iter())
    }

    /// Does the node produce an image (thumbnail, 2D view, meaningful tiling)?
    pub fn has_image(&self) -> bool {
        matches!(self.role, Role::Image | Role::Output | Role::Instance)
    }

    pub fn is_value_node(&self) -> bool {
        !self.has_image()
    }

    /// The expression parameter of Math / Pixel Math nodes.
    pub fn expression_param(&self) -> Option<&'static ParamDef<'static>> {
        self.params.iter().find(|p| matches!(p.kind, ParamKind::Expression { .. }))
    }

    /// True for Pixel Math: the expression runs per pixel and its variables are images.
    pub fn is_pixel_expression(&self) -> bool {
        matches!(self.expression_param().map(|p| p.kind), Some(ParamKind::Expression { pixel: true, .. }))
    }

    /// The kind of image the node makes from the kinds of the images arriving at its fixed
    /// inputs (`None` where nothing arrives). A few nodes decide otherwise from their
    /// parameters or their file (see [`image_kind`]).
    pub fn output_kind(&self, inputs: &[Option<ImageKind>]) -> ImageKind {
        let slot = match self.role {
            // An output shows what arrives at it.
            Role::Output => Slot::Any,
            _ => self.outputs.first().map_or(Slot::Any, |o| o.slot),
        };
        match slot {
            Slot::Gray => ImageKind::Gray,
            Slot::Color => ImageKind::Color,
            Slot::Any => {
                let color = self.inputs.iter().zip(inputs).any(|(s, k)| s.slot == Slot::Any && *k == Some(ImageKind::Color));
                if color { ImageKind::Color } else { ImageKind::Gray }
            }
        }
    }

    /// Does `query` (lower case) find this node?
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim();
        q.is_empty()
            || self.name.to_lowercase().contains(q)
            || self.keywords.to_lowercase().split_whitespace().any(|k| k.starts_with(q))
            || self.category.name().to_lowercase().contains(q)
    }
}

/// A node type: a cheap copyable handle to its [`NodeDef`]. Serialized as the id.
#[derive(Clone, Copy)]
pub struct NodeKind(&'static NodeDef);

impl PartialEq for NodeKind {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for NodeKind {}
impl std::hash::Hash for NodeKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}
impl PartialOrd for NodeKind {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NodeKind {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.id.cmp(other.0.id)
    }
}
impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.id)
    }
}
impl Serialize for NodeKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0.id)
    }
}
impl<'de> Deserialize<'de> for NodeKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        NodeKind::from_id(&id).ok_or_else(|| serde::de::Error::custom(format!("unknown node type '{id}'")))
    }
}

impl NodeKind {
    pub fn id(self) -> &'static str {
        self.0.id
    }

    pub fn def(self) -> &'static NodeDef {
        self.0
    }

    pub fn from_id(id: &str) -> Option<NodeKind> {
        NodeKind::ALL.iter().copied().chain([NodeKind::GRAPH_INSTANCE]).find(|k| k.id() == id)
    }

    /// Input node type reading a variable of type `ty`.
    pub fn input_for(ty: DataType) -> Option<NodeKind> {
        NodeKind::ALL.iter().copied().find(|k| k.def().role == Role::Variable(ty))
    }

    // Inputs & values.
    pub const FLOAT_INPUT: NodeKind = NodeKind(&values::FLOAT_INPUT);
    pub const INT_INPUT: NodeKind = NodeKind(&values::INT_INPUT);
    pub const BOOL_INPUT: NodeKind = NodeKind(&values::BOOL_INPUT);
    pub const VEC2_INPUT: NodeKind = NodeKind(&values::VEC2_INPUT);
    pub const COLOR_INPUT: NodeKind = NodeKind(&values::COLOR_INPUT);
    pub const MATH: NodeKind = NodeKind(&values::MATH);
    pub const COMPOSE_VEC2: NodeKind = NodeKind(&values::COMPOSE_VEC2);
    pub const COMPOSE_COLOR: NodeKind = NodeKind(&values::COMPOSE_COLOR);
    pub const IMAGE_INPUT: NodeKind = NodeKind(&values::IMAGE_INPUT);
    // Generators.
    pub const UNIFORM_COLOR: NodeKind = NodeKind(&generators::UNIFORM_COLOR);
    pub const SHAPE: NodeKind = NodeKind(&generators::SHAPE);
    pub const TILE_GENERATOR: NodeKind = NodeKind(&generators::TILE_GENERATOR);
    pub const TILE_SAMPLER: NodeKind = NodeKind(&generators::TILE_SAMPLER);
    pub const GRADIENT: NodeKind = NodeKind(&generators::GRADIENT);
    pub const NOISE: NodeKind = NodeKind(&generators::NOISE);
    pub const BITMAP: NodeKind = NodeKind(&generators::BITMAP);
    pub const SVG: NodeKind = NodeKind(&generators::SVG);
    pub const TEXT: NodeKind = NodeKind(&generators::TEXT);
    // Filters.
    pub const TRANSFORM_2D: NodeKind = NodeKind(&filters::TRANSFORM_2D);
    pub const BLEND: NodeKind = NodeKind(&filters::BLEND);
    pub const BLUR: NodeKind = NodeKind(&filters::BLUR);
    pub const DIRECTIONAL_BLUR: NodeKind = NodeKind(&filters::DIRECTIONAL_BLUR);
    pub const SLOPE_BLUR: NodeKind = NodeKind(&filters::SLOPE_BLUR);
    pub const SHARPEN: NodeKind = NodeKind(&filters::SHARPEN);
    pub const WARP: NodeKind = NodeKind(&filters::WARP);
    pub const DIRECTIONAL_WARP: NodeKind = NodeKind(&filters::DIRECTIONAL_WARP);
    pub const MIRROR: NodeKind = NodeKind(&filters::MIRROR);
    pub const PIXEL_MATH: NodeKind = NodeKind(&filters::PIXEL_MATH);
    pub const DISTANCE: NodeKind = NodeKind(&filters::DISTANCE);
    pub const NON_UNIFORM_BLUR: NodeKind = NodeKind(&filters::NON_UNIFORM_BLUR);
    pub const MAKE_IT_TILE: NodeKind = NodeKind(&filters::MAKE_IT_TILE);
    pub const FLOOD_FILL: NodeKind = NodeKind(&filters::FLOOD_FILL);
    // Height & normal.
    pub const NORMAL: NodeKind = NodeKind(&effects::NORMAL);
    pub const AMBIENT_OCCLUSION: NodeKind = NodeKind(&effects::AMBIENT_OCCLUSION);
    pub const CURVATURE: NodeKind = NodeKind(&effects::CURVATURE);
    pub const EDGE_DETECT: NodeKind = NodeKind(&effects::EDGE_DETECT);
    pub const EMBOSS: NodeKind = NodeKind(&effects::EMBOSS);
    pub const BEVEL: NodeKind = NodeKind(&effects::BEVEL);
    pub const HEIGHT_BLEND: NodeKind = NodeKind(&effects::HEIGHT_BLEND);
    pub const NORMAL_COMBINE: NodeKind = NodeKind(&effects::NORMAL_COMBINE);
    // Adjustments.
    pub const LEVELS: NodeKind = NodeKind(&adjustments::LEVELS);
    pub const HISTOGRAM_SCAN: NodeKind = NodeKind(&adjustments::HISTOGRAM_SCAN);
    pub const INVERT: NodeKind = NodeKind(&adjustments::INVERT);
    pub const GRAYSCALE: NodeKind = NodeKind(&adjustments::GRAYSCALE);
    pub const HSL: NodeKind = NodeKind(&adjustments::HSL);
    pub const GRADIENT_MAP: NodeKind = NodeKind(&adjustments::GRADIENT_MAP);
    pub const CURVE: NodeKind = NodeKind(&adjustments::CURVE);
    pub const GRADIENT_DYNAMIC: NodeKind = NodeKind(&adjustments::GRADIENT_DYNAMIC);
    pub const CHANNEL_SHUFFLE: NodeKind = NodeKind(&adjustments::CHANNEL_SHUFFLE);
    pub const RGBA_MERGE: NodeKind = NodeKind(&adjustments::RGBA_MERGE);
    pub const POSTERIZE: NodeKind = NodeKind(&adjustments::POSTERIZE);
    // Output.
    pub const OUTPUT: NodeKind = NodeKind(&output::OUTPUT);
    /// A graph used as a node (see [`crate::instance`]). Not in [`NodeKind::ALL`]: it is made
    /// from a graph, not from the library.
    pub const GRAPH_INSTANCE: NodeKind = NodeKind(&values::GRAPH_INSTANCE);

    /// Every node type, in library order.
    pub const ALL: &'static [NodeKind] = &[
        NodeKind::FLOAT_INPUT,
        NodeKind::INT_INPUT,
        NodeKind::BOOL_INPUT,
        NodeKind::VEC2_INPUT,
        NodeKind::COLOR_INPUT,
        NodeKind::MATH,
        NodeKind::COMPOSE_VEC2,
        NodeKind::COMPOSE_COLOR,
        NodeKind::IMAGE_INPUT,
        NodeKind::UNIFORM_COLOR,
        NodeKind::SHAPE,
        NodeKind::TILE_GENERATOR,
        NodeKind::TILE_SAMPLER,
        NodeKind::GRADIENT,
        NodeKind::NOISE,
        NodeKind::BITMAP,
        NodeKind::SVG,
        NodeKind::TEXT,
        NodeKind::TRANSFORM_2D,
        NodeKind::BLEND,
        NodeKind::BLUR,
        NodeKind::DIRECTIONAL_BLUR,
        NodeKind::NON_UNIFORM_BLUR,
        NodeKind::SLOPE_BLUR,
        NodeKind::SHARPEN,
        NodeKind::WARP,
        NodeKind::DIRECTIONAL_WARP,
        NodeKind::MIRROR,
        NodeKind::PIXEL_MATH,
        NodeKind::DISTANCE,
        NodeKind::FLOOD_FILL,
        NodeKind::MAKE_IT_TILE,
        NodeKind::NORMAL,
        NodeKind::AMBIENT_OCCLUSION,
        NodeKind::CURVATURE,
        NodeKind::EDGE_DETECT,
        NodeKind::EMBOSS,
        NodeKind::BEVEL,
        NodeKind::HEIGHT_BLEND,
        NodeKind::NORMAL_COMBINE,
        NodeKind::LEVELS,
        NodeKind::HISTOGRAM_SCAN,
        NodeKind::INVERT,
        NodeKind::GRAYSCALE,
        NodeKind::HSL,
        NodeKind::GRADIENT_MAP,
        NodeKind::GRADIENT_DYNAMIC,
        NodeKind::CURVE,
        NodeKind::CHANNEL_SHUFFLE,
        NodeKind::RGBA_MERGE,
        NodeKind::POSTERIZE,
        NodeKind::OUTPUT,
    ];

    /// Node types of a category, in library order.
    pub fn in_category(category: Category) -> impl Iterator<Item = NodeKind> {
        NodeKind::ALL.iter().copied().filter(move |k| k.def().category == category)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_resolvable() {
        let mut ids: Vec<&str> = NodeKind::ALL.iter().map(|k| k.id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate node id");
        for k in NodeKind::ALL {
            assert_eq!(NodeKind::from_id(k.id()), Some(*k));
            assert!(k.id().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{:?}", k);
        }
        for ty in DataType::VALUE_TYPES {
            assert!(NodeKind::input_for(ty).is_some(), "no input node for {ty:?}");
        }
    }

    #[test]
    fn params_are_consistent() {
        for kind in NodeKind::ALL {
            let d = kind.def();
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
                    assert!((default as usize) < options.len(), "{}::{}", d.name, p.key);
                }
                if p.pictured {
                    assert!(
                        matches!(p.kind, ParamKind::Enum { .. }) && d.has_image(),
                        "{}::{}: only the choices of image nodes have pictures",
                        d.name,
                        p.key
                    );
                }
                assert!(p.accepts(&p.default_value()));
                if p.exposable {
                    assert!(
                        p.default_value().socket_type().is_some(),
                        "{}::{} exposable without socket",
                        d.name,
                        p.key
                    );
                }
            }
            match d.role {
                Role::Image => assert!(d.outputs.iter().any(|o| o.ty == DataType::Image), "{}", d.name),
                Role::Variable(ty) => assert_eq!(d.outputs[0].ty, ty),
                Role::Output => assert!(d.outputs.is_empty()),
                Role::Value => assert!(!d.outputs.is_empty()),
                Role::Instance => unreachable!("graphs used as nodes are not in the library"),
            }
            assert!(!d.description.is_empty(), "{} has no description", d.name);
        }
        const { assert!(!TILING_PARAM.exposable, "tiling must not be exposable") };
    }

    /// What each node makes, as in Substance Designer: generators of shapes and noise make
    /// grayscale images, colorizing makes color, and the filters pass on what they are given.
    #[test]
    fn image_kinds() {
        use ImageKind::{Color, Gray};
        let params = crate::eval::Params::default();
        let kind = |k: NodeKind, inputs: &[Option<ImageKind>]| image_kind(k, &params, inputs);
        assert_eq!(kind(NodeKind::NOISE, &[]), Gray);
        assert_eq!(kind(NodeKind::UNIFORM_COLOR, &[]), Color);
        assert_eq!(kind(NodeKind::GRADIENT_MAP, &[Some(Gray)]), Color);
        assert_eq!(kind(NodeKind::GRAYSCALE, &[Some(Color)]), Gray);
        assert_eq!(kind(NodeKind::NORMAL, &[Some(Gray)]), Color);
        assert_eq!(kind(NodeKind::LEVELS, &[Some(Gray)]), Gray);
        assert_eq!(kind(NodeKind::LEVELS, &[Some(Color)]), Color);
        assert_eq!(kind(NodeKind::BLEND, &[Some(Gray), Some(Gray), None]), Gray);
        assert_eq!(kind(NodeKind::BLEND, &[Some(Gray), Some(Color), None]), Color);
        // A mask is grayscale whatever arrives at it: it does not make the blend color.
        assert_eq!(kind(NodeKind::BLEND, &[Some(Gray), Some(Gray), Some(Color)]), Gray);
        // Tiles of a shape are grayscale even with a color image at the pattern input.
        assert_eq!(kind(NodeKind::TILE_GENERATOR, &[Some(Color)]), Gray);
        assert_eq!(kind(NodeKind::OUTPUT, &[Some(Color)]), Color);
        assert_eq!(kind(NodeKind::BLUR, &[None]), Gray, "nothing arriving is grayscale");
        // An image input is what it says it takes.
        assert_eq!(kind(NodeKind::IMAGE_INPUT, &[]), Gray);
        let color = crate::eval::Params { values: vec![("kind".into(), ParamValue::Enum(1))] };
        assert_eq!(image_kind(NodeKind::IMAGE_INPUT, &color, &[]), Color);
        // Tiles of a sampler: of shapes, gray; of images, what they are (a color map alone does
        // not make them color); in colors of their own, color.
        let sampler = |values: Vec<(&str, u32)>, inputs: &[Option<ImageKind>]| {
            let params = crate::eval::Params { values: values.into_iter().map(|(k, v)| (k.to_string(), ParamValue::Enum(v))).collect() };
            image_kind(NodeKind::TILE_SAMPLER, &params, inputs)
        };
        let images = TILE_PATTERN_IMAGE;
        assert_eq!(sampler(vec![("pattern", 0)], &[Some(Color)]), Gray);
        assert_eq!(sampler(vec![("pattern", images)], &[None, Some(Color)]), Color);
        assert_eq!(sampler(vec![("pattern", images)], &[Some(Gray), None, None, None, None, None, None, Some(Color)]), Gray);
        assert_eq!(sampler(vec![("pattern", 0), ("color", 1)], &[]), Color);
        assert_eq!(sampler(vec![("pattern", 0), ("color", 2)], &[]), Color);
    }

    #[test]
    fn search() {
        assert!(NodeKind::BLUR.def().matches("blu"));
        assert!(NodeKind::GRADIENT_MAP.def().matches("colorize"));
        assert!(!NodeKind::BLUR.def().matches("zzz"));
    }
}
