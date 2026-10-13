//! What a new graph starts as (File → New Graph…, the Home screen): the templates Substance
//! Designer 16 has, as far as they make sense here, named as it names them (its OpenPBR
//! materials, its studio, its samples of Pixel Processor and FX-Map, its scan processing), and
//! none of another company's program or format; and the packages the Home screen's Learn
//! opens, made here rather than kept as files: they are what the program makes of them now.

use crate::graph::{Graph, NodeId, Precision};
use crate::nodes::{DEPTH_16F, NodeKind, USAGE_BASE_COLOR, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL, USAGE_ROUGHNESS, Usage};
use crate::package::Package;
use crate::types::{ParamValue, Value};

/// A category of templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// Materials of OpenPBR, the open standard of the Academy Software Foundation that Adobe's
    /// and Autodesk's material models gave way to.
    OpenPbr,
    /// Light all around: environments.
    Hdri,
    PixelProcessor,
    FxMap,
    /// Materials made of scans.
    ScanProcessing,
    Filters,
    Others,
}

impl Category {
    pub const ALL: [Category; 7] =
        [Category::OpenPbr, Category::Hdri, Category::PixelProcessor, Category::FxMap, Category::ScanProcessing, Category::Filters, Category::Others];

    pub fn name(self) -> &'static str {
        match self {
            Category::OpenPbr => "OpenPBR",
            Category::Hdri => "HDRi",
            Category::PixelProcessor => "Pixel Processor",
            Category::FxMap => "FX-Map",
            Category::ScanProcessing => "Scan Processing",
            Category::Filters => "Filters",
            Category::Others => "Others",
        }
    }
}

/// What a new graph starts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Template {
    /// OpenPBR: the maps of its base, each an Output made for the 3D view, fed a uniform value
    /// to be replaced.
    Pbr,
    /// The base, its roughness running one way more than the other.
    Anisotropy,
    /// The base under a clear coat.
    Coat,
    /// Light that goes in under the surface and out elsewhere.
    Subsurface,
    /// The base under a fine fuzz.
    Fuzz,
    /// Every parameter of OpenPBR, each a map.
    Complete,
    /// An environment light: a studio, as a panorama (square, as Substance Designer's), in floats.
    StudioPanorama,
    /// A filter of Pixel Math: a box blur as wide as a map says.
    Convolution,
    /// An FX-Map: a grid of squares.
    Grid,
    /// An FX-Map: noise of splats at every split of the image.
    Noise,
    /// The maps of a scan made into those of a material: tiled, the light it was taken in
    /// taken away.
    ScanProcessing,
    /// A graph that changes a grayscale image: an image input to an output.
    GrayFilter,
    /// The same, of color.
    ColorFilter,
    /// Nothing.
    Empty,
}

/// A map of a material template: the Output's name (its file's), the uniform value it is fed,
/// whether that is gray, and its usage (the parameter of OpenPBR it fills, as Substance
/// Designer's templates give it).
struct Map(&'static str, [f32; 4], bool, Usage);

const fn gray(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}
/// A normal straight out of the surface, and a tangent along u, as their maps say them; a
/// tangent up the texture (along v), which draws the highlights of a sphere out up and down.
const FLAT: [f32; 4] = [0.5, 0.5, 1.0, 1.0];
const ALONG: [f32; 4] = [1.0, 0.5, 0.5, 1.0];
const UPWARD: [f32; 4] = [0.5, 1.0, 0.5, 1.0];

/// The maps of each OpenPBR template, in the order Substance Designer lists them, of the values
/// its templates have: the base of each is of a material its layer is for (brushed metal, car
/// paint, wax, velvet), the layer on all the way, so that the template is what it is named;
/// Complete has the standard's defaults (every layer off but the base).
const BASE: &[Map] = &[
    Map("base_color", gray(0.75), false, Usage::BaseColor),
    Map("base_metalness", gray(0.0), true, Usage::Metallic),
    Map("specular_roughness", gray(0.25), true, Usage::Roughness),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
];
const ANISOTROPY: &[Map] = &[
    Map("base_color", gray(0.8), false, Usage::BaseColor),
    Map("base_metalness", gray(1.0), true, Usage::Metallic),
    // (Drawn out, not to a hairline: a hairline along the sphere looks like a seam.)
    Map("specular_roughness", gray(0.3), true, Usage::Roughness),
    Map("specular_roughness_anisotropy", gray(0.7), true, Usage::SpecularRoughnessAnisotropy),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    // (Up the texture: its highlights stand up and down, where they are seen best.)
    Map("tangent", UPWARD, false, Usage::Tangent),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
];
const COAT: &[Map] = &[
    Map("base_color", [0.5, 0.13, 0.13, 1.0], false, Usage::BaseColor),
    Map("base_metalness", gray(1.0), true, Usage::Metallic),
    Map("specular_roughness", gray(0.5), true, Usage::Roughness),
    Map("coat_weight", gray(1.0), true, Usage::CoatWeight),
    Map("coat_color", gray(1.0), false, Usage::CoatColor),
    Map("coat_roughness", gray(0.05), true, Usage::CoatRoughness),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    Map("coat_normal", FLAT, false, Usage::CoatNormal),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
    Map("coat_roughness_anisotropy", gray(0.0), true, Usage::CoatRoughnessAnisotropy),
    Map("coat_tangent", ALONG, false, Usage::CoatTangent),
];
// Skin: its color, light going farther under it the redder it is.
const SUBSURFACE: &[Map] = &[
    Map("base_color", [0.8, 0.56, 0.46, 1.0], false, Usage::BaseColor),
    Map("specular_roughness", gray(0.45), true, Usage::Roughness),
    Map("subsurface_weight", gray(1.0), true, Usage::SubsurfaceWeight),
    Map("subsurface_color", [0.85, 0.58, 0.47, 1.0], false, Usage::SubsurfaceColor),
    Map("subsurface_radius", gray(0.6), true, Usage::SubsurfaceRadius),
    Map("subsurface_radius_scale", [1.0, 0.35, 0.2, 1.0], false, Usage::SubsurfaceRadiusScale),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
];
const FUZZ: &[Map] = &[
    Map("base_color", [0.0, 0.28, 0.4, 1.0], false, Usage::BaseColor),
    Map("base_metalness", gray(0.0), true, Usage::Metallic),
    Map("specular_roughness", gray(0.8), true, Usage::Roughness),
    // A fine fuzz, as a peach's or a kiwi's: a little of it, its fibers spread, a haze over the
    // color more than a glow at the edges.
    Map("fuzz_weight", gray(0.35), true, Usage::FuzzWeight),
    Map("fuzz_color", gray(1.0), false, Usage::FuzzColor),
    Map("fuzz_roughness", gray(0.6), true, Usage::FuzzRoughness),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
];
const COMPLETE: &[Map] = &[
    Map("base_weight", gray(1.0), true, Usage::BaseWeight),
    Map("base_color", gray(0.8), false, Usage::BaseColor),
    Map("base_metalness", gray(0.0), true, Usage::Metallic),
    Map("base_diffuse_roughness", gray(0.0), true, Usage::BaseDiffuseRoughness),
    Map("specular_weight", gray(1.0), true, Usage::SpecularWeight),
    Map("specular_color", gray(1.0), false, Usage::SpecularColor),
    Map("specular_roughness", gray(0.3), true, Usage::Roughness),
    Map("specular_roughness_anisotropy", gray(0.0), true, Usage::SpecularRoughnessAnisotropy),
    Map("specular_ior", gray(1.5), true, Usage::SpecularIor),
    Map("transmission_weight", gray(0.0), true, Usage::TransmissionWeight),
    Map("transmission_color", gray(1.0), false, Usage::TransmissionColor),
    Map("transmission_depth", gray(0.0), true, Usage::TransmissionDepth),
    Map("transmission_scatter", gray(0.0), false, Usage::TransmissionScatter),
    Map("transmission_scatter_anisotropy", gray(0.0), true, Usage::TransmissionScatterAnisotropy),
    Map("transmission_dispersion_scale", gray(0.0), true, Usage::TransmissionDispersionScale),
    Map("transmission_dispersion_abbe_number", gray(20.0), true, Usage::TransmissionDispersionAbbeNumber),
    Map("subsurface_weight", gray(0.0), true, Usage::SubsurfaceWeight),
    Map("subsurface_color", gray(0.8), false, Usage::SubsurfaceColor),
    Map("subsurface_radius", gray(1.0), true, Usage::SubsurfaceRadius),
    Map("subsurface_radius_scale", [1.0, 0.5, 0.25, 1.0], false, Usage::SubsurfaceRadiusScale),
    Map("subsurface_scatter_anisotropy", gray(0.0), true, Usage::SubsurfaceScatterAnisotropy),
    Map("coat_weight", gray(0.0), true, Usage::CoatWeight),
    Map("coat_color", gray(1.0), false, Usage::CoatColor),
    Map("coat_roughness", gray(0.0), true, Usage::CoatRoughness),
    Map("coat_roughness_anisotropy", gray(0.0), true, Usage::CoatRoughnessAnisotropy),
    Map("coat_ior", gray(1.6), true, Usage::CoatIor),
    Map("coat_darkening", gray(1.0), true, Usage::CoatDarkening),
    Map("fuzz_weight", gray(0.0), true, Usage::FuzzWeight),
    Map("fuzz_color", gray(1.0), false, Usage::FuzzColor),
    Map("fuzz_roughness", gray(0.5), true, Usage::FuzzRoughness),
    Map("emission_luminance", gray(0.0), true, Usage::EmissionLuminance),
    Map("emission_color", gray(1.0), false, Usage::Emissive),
    Map("thin_film_weight", gray(0.0), true, Usage::ThinFilmWeight),
    Map("thin_film_thickness", gray(0.5), true, Usage::ThinFilmThickness),
    Map("thin_film_ior", gray(1.4), true, Usage::ThinFilmIor),
    Map("opacity", gray(1.0), true, Usage::Opacity),
    Map("thin_walled", gray(0.0), true, Usage::ThinWalled),
    Map("height", gray(0.5), true, Usage::Height),
    Map("normal", FLAT, false, Usage::Normal),
    Map("tangent", ALONG, false, Usage::Tangent),
    Map("coat_normal", FLAT, false, Usage::CoatNormal),
    Map("coat_tangent", ALONG, false, Usage::CoatTangent),
    Map("ambient_occlusion", gray(1.0), true, Usage::AmbientOcclusion),
];

/// Whether a map of `usage` holds values past 1 (an index of refraction, a distance, a
/// luminance, an Abbe number) or under 0 (which way light scattered inside goes on, -1 to 1):
/// its Output writes floats, which keep them; eight bits would hold them between 0 and 1.
fn past_one(usage: Usage) -> bool {
    matches!(
        usage,
        Usage::SpecularWeight
            | Usage::TransmissionScatterAnisotropy
            | Usage::SubsurfaceScatterAnisotropy
            | Usage::SpecularIor
            | Usage::TransmissionDepth
            | Usage::TransmissionDispersionAbbeNumber
            | Usage::SubsurfaceRadius
            | Usage::CoatIor
            | Usage::EmissionLuminance
            | Usage::ThinFilmThickness
            | Usage::ThinFilmIor
    )
}

impl Template {
    pub const ALL: [Template; 14] = [
        Template::Pbr,
        Template::Anisotropy,
        Template::Coat,
        Template::Subsurface,
        Template::Fuzz,
        Template::Complete,
        Template::StudioPanorama,
        Template::Convolution,
        Template::Grid,
        Template::Noise,
        Template::ScanProcessing,
        Template::GrayFilter,
        Template::ColorFilter,
        Template::Empty,
    ];

    /// Those the Home screen offers, one of each kind; the window of a new graph has them all.
    pub const STARTERS: [Template; 6] =
        [Template::Pbr, Template::StudioPanorama, Template::Convolution, Template::Noise, Template::ScanProcessing, Template::Empty];

    /// What names it where a name of words will not do (what the Home screen is told of it,
    /// what the workspace keeps of the one chosen last).
    pub fn id(self) -> &'static str {
        match self {
            Template::Pbr => "pbr",
            Template::Anisotropy => "openpbr-anisotropy",
            Template::Coat => "openpbr-coat",
            Template::Subsurface => "openpbr-subsurface",
            Template::Fuzz => "openpbr-fuzz",
            Template::Complete => "openpbr-complete",
            Template::StudioPanorama => "studio-panorama",
            Template::Convolution => "convolution",
            Template::Grid => "fx-map-grid",
            Template::Noise => "fx-map-noise",
            Template::ScanProcessing => "scan-processing",
            Template::GrayFilter => "gray-filter",
            Template::ColorFilter => "color-filter",
            Template::Empty => "empty",
        }
    }

    pub fn from_id(id: &str) -> Option<Template> {
        Template::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn category(self) -> Category {
        match self {
            Template::Pbr | Template::Anisotropy | Template::Coat | Template::Subsurface | Template::Fuzz | Template::Complete => Category::OpenPbr,
            Template::StudioPanorama => Category::Hdri,
            Template::Convolution => Category::PixelProcessor,
            Template::Grid | Template::Noise => Category::FxMap,
            Template::ScanProcessing => Category::ScanProcessing,
            Template::GrayFilter | Template::ColorFilter => Category::Filters,
            Template::Empty => Category::Others,
        }
    }

    /// Its name in its category, as Substance Designer names it.
    pub fn name(self) -> &'static str {
        match self {
            Template::Pbr => "Base",
            Template::Anisotropy => "Anisotropy",
            Template::Coat => "Coat",
            Template::Subsurface => "Subsurface",
            Template::Fuzz => "Fuzz",
            Template::Complete => "Complete",
            Template::StudioPanorama => "Studio",
            Template::Convolution => "Convolution",
            Template::Grid => "Grid",
            Template::Noise => "Noise",
            Template::ScanProcessing => "Metallic Roughness",
            Template::GrayFilter => "Grayscale Filter",
            Template::ColorFilter => "Color Filter",
            Template::Empty => "Empty",
        }
    }

    /// What kind of graph it makes.
    pub fn kind(self) -> &'static str {
        match self {
            Template::Pbr | Template::Anisotropy | Template::Coat | Template::Subsurface | Template::Fuzz | Template::Complete | Template::ScanProcessing => {
                "Standard material"
            }
            Template::StudioPanorama => "Environment light",
            Template::Convolution | Template::GrayFilter | Template::ColorFilter => "Filter",
            Template::Grid | Template::Noise => "Generator",
            Template::Empty => "Empty graph",
        }
    }

    /// What it is, in a few words.
    pub fn about(self) -> &'static str {
        match self {
            Template::Pbr => {
                "The maps of an OpenPBR material: base color and metalness, specular roughness, normal, height and \
                 occlusion, each an Output fed a uniform value to replace with what makes it."
            }
            Template::Anisotropy => "Base, its roughness running one way more than the other (brushed metal), and the tangent it runs along.",
            Template::Coat => "Base under a clear coat (varnish, car paint): its weight, color, roughness, normal and anisotropy.",
            Template::Subsurface => "Light that goes in under the surface and out elsewhere (skin, wax, marble): how much, of what color, how far.",
            Template::Fuzz => "Base under a fine fuzz (velvet, dust, the skin of a peach): its weight, color and roughness.",
            Template::Complete => {
                "Every parameter of OpenPBR a map: base, specular, transmission, subsurface, coat, fuzz, emission, thin \
                 film, opacity and geometry."
            }
            Template::StudioPanorama => {
                "An environment light of a studio, made of HDRI tools (a softbox overhead, a strip light, a large panel \
                 behind, in the dark of the room): a panorama in 32-bit floats whose usage, Panorama, lights the 3D view."
            }
            Template::Convolution => {
                "A convolution filter of a grayscale image: each pixel the average of those around it, as far as a map says (a box blur that is not the same everywhere)."
            }
            Template::Grid => "A grid of squares, as many across and down as its input values say, drawn by one FX-Map.",
            Template::Noise => "Noise of splats: one at every split of the image, the finer the weaker, added up (as clouds are).",
            Template::ScanProcessing => {
                "A scan made a metallic/roughness material: its color, normals and height tiled, the light it was \
                 photographed in taken away, its roughness worked out."
            }
            Template::GrayFilter => {
                "Changes a grayscale image: the image the graph takes, what is done to it, and its output. Used as a node, \
                 it filters what is connected to it."
            }
            Template::ColorFilter => {
                "Changes a color image: the image the graph takes, what is done to it, and its output. Used as a node, it \
                 filters what is connected to it."
            }
            Template::Empty => "A graph with nothing in it.",
        }
    }

    /// The maps of a material template of OpenPBR.
    fn maps(self) -> Option<&'static [Map]> {
        match self {
            Template::Pbr => Some(BASE),
            Template::Anisotropy => Some(ANISOTROPY),
            Template::Coat => Some(COAT),
            Template::Subsurface => Some(SUBSURFACE),
            Template::Fuzz => Some(FUZZ),
            Template::Complete => Some(COMPLETE),
            _ => None,
        }
    }

    /// The Outputs it has: their names, as their files are named.
    pub fn outputs(self) -> Vec<&'static str> {
        if let Some(maps) = self.maps() {
            return maps.iter().map(|m| m.0).collect();
        }
        match self {
            Template::ScanProcessing => vec!["basecolor", "normal", "roughness", "metallic", "height"],
            Template::Empty => Vec::new(),
            _ => vec!["output"],
        }
    }

    /// The size and the precision of its graph, of those asked: a panorama is square, as Substance
    /// Designer makes it (as wide as asked; the 3D view reads it across once round and down from
    /// straight up to straight down, whatever its shape), and keeps its light, far above white, in
    /// 32-bit floats.
    pub fn fit(self, size: [u32; 2], precision: Precision) -> ([u32; 2], Precision) {
        match self {
            Template::StudioPanorama => ([size[0].max(32), size[0].max(32)], Precision::Full),
            _ => (size, precision),
        }
    }

    /// The graph, its images `size` pixels wide and high, kept in `precision` (as `fit` has
    /// them).
    pub fn graph(self, size: [u32; 2], precision: Precision) -> Graph {
        let (size, precision) = self.fit(size, precision);
        let mut g = Graph { resolution: size, precision, ..Graph::default() };
        if let Some(maps) = self.maps() {
            material(&mut g, self, maps);
            return g;
        }
        match self {
            Template::StudioPanorama => {
                // As Substance Designer's studio is made, as bright as a room is: its dark, and in
                // it a softbox overhead, a strip light to the right, a large panel behind.
                let room = g.add_node(NodeKind::HDRI_GRADIENT, [0.0, 0.0]);
                set(&mut g, room, "top", ParamValue::Color([0.0394, 0.0394, 0.0394, 1.0]));
                set(&mut g, room, "bottom", ParamValue::Color([0.0, 0.0, 0.0, 1.0]));
                set(&mut g, room, "contrast", ParamValue::Float(0.5));
                let softbox = g.add_node(NodeKind::HDRI_SHAPE_LIGHT, [300.0, 0.0]);
                set(&mut g, softbox, "position", ParamValue::Vec2([0.5, 0.2]));
                set(&mut g, softbox, "size", ParamValue::Vec2([0.4, 0.4]));
                set(&mut g, softbox, "hardness", ParamValue::Float(0.0));
                set(&mut g, softbox, "exposure", ParamValue::Float(2.0));
                set(&mut g, softbox, "hotspot_exposure", ParamValue::Float(4.0));
                set(&mut g, softbox, "hotspot_size", ParamValue::Float(1.0));
                let _ = g.connect(room, "o:output", softbox, "i:background");
                let strip = g.add_node(NodeKind::HDRI_LINE_LIGHT, [600.0, 0.0]);
                set(&mut g, strip, "point_1", ParamValue::Vec2([0.7, 0.0573]));
                set(&mut g, strip, "point_2", ParamValue::Vec2([0.8096, 0.4555]));
                set(&mut g, strip, "thickness", ParamValue::Float(0.025));
                set(&mut g, strip, "pattern", ParamValue::Enum(2));
                set(&mut g, strip, "color_mode", ParamValue::Enum(crate::nodes::LIGHT_COLOR_TEMPERATURE));
                set(&mut g, strip, "temperature", ParamValue::Float(8500.0));
                set(&mut g, strip, "exposure", ParamValue::Float(4.0));
                let _ = g.connect(softbox, "o:output", strip, "i:background");
                let panel = g.add_node(NodeKind::HDRI_PLANE_LIGHT, [900.0, 0.0]);
                set(&mut g, panel, "position", ParamValue::Vec2([0.0, 0.765]));
                set(&mut g, panel, "size", ParamValue::Vec2([0.5, 0.65]));
                set(&mut g, panel, "hardness", ParamValue::Float(0.628));
                set(&mut g, panel, "color_mode", ParamValue::Enum(crate::nodes::LIGHT_COLOR_TEMPERATURE));
                set(&mut g, panel, "temperature", ParamValue::Float(8205.0));
                set(&mut g, panel, "exposure", ParamValue::Float(0.5));
                let _ = g.connect(strip, "o:output", panel, "i:background");
                let panorama = output(&mut g, "output", Usage::Panorama.index(), [1200.0, 0.0]);
                // Its three channels, in 32-bit floats: an EXR.
                set(&mut g, panorama, "format", ParamValue::Enum(1));
                set(&mut g, panorama, "bit_depth", ParamValue::Enum(crate::nodes::DEPTH_32F));
                let _ = g.connect(panel, "o:output", panorama, "i:input");
                let said = "An environment light is a panorama: across, once round; down, from straight up to straight down \
                            (square here, as Substance Designer makes it). The HDRI tools draw a studio in it, each over the one before: \
                            Gradient Linear (HDRI) the dark of the room, a Shape Light the softbox overhead with its hotspot, \
                            a Line Light the strip light to the right, a Plane Light the large panel behind (across the seam: \
                            on both edges of the panorama). Their light is far brighter than white, kept in 32-bit floats. The \
                            Output's usage is Panorama: the 3D view is lit by it (the sun button: The Graph's Panorama).";
                let width = 1200.0 + 568.0 - 340.0;
                let top = said_height(said, width);
                frame(&mut g, "Light all around, as a panorama", said, [-30.0, -top], [width, top + 216.0 + 30.0], GRAY);
            }
            Template::Convolution => {
                // (Of a grayscale image: what Pixel Math samples of an image is one value.)
                let input = image_input(&mut g, "input", crate::nodes::ImageKind::Gray, gray(0.5), [0.0, 0.0]);
                let amount = image_input(&mut g, "amount", crate::nodes::ImageKind::Gray, gray(1.0), [0.0, ROW]);
                let blur = g.add_node(NodeKind::PIXEL_MATH, [300.0, 0.0]);
                set(
                    &mut g,
                    blur,
                    "expression",
                    ParamValue::Text(
                        "# How far around each pixel is averaged: up to 8 pixels, as far as the amount map says.\n\
                         dx = amount * 8 / width\n\
                         dy = amount * 8 / height\n\
                         # The pixel and the eight around it, that far off: their average.\n\
                         sum = sample(input, u - dx, v - dy) + sample(input, u, v - dy) + sample(input, u + dx, v - dy) + \
                         sample(input, u - dx, v) + input + sample(input, u + dx, v) + sample(input, u - dx, v + dy) + \
                         sample(input, u, v + dy) + sample(input, u + dx, v + dy)\n\
                         sum / 9"
                            .into(),
                    ),
                );
                let _ = g.connect(input, "o:output", blur, &crate::graph::var_socket_key("input"));
                let _ = g.connect(amount, "o:output", blur, &crate::graph::var_socket_key("amount"));
                let out = output(&mut g, "output", 0, [600.0, 0.0]);
                let _ = g.connect(blur, "o:output", out, "i:input");
                frame(
                    &mut g,
                    "A convolution: each pixel the average of those around it",
                    "Pixel Math reads the input a pixel's distance around each pixel (sample reads it anywhere) and \
                     averages the nine: a box blur. How far it reaches is the amount map's, up to 8 pixels: white \
                     blurs most, black not at all. Choose the pictures in Input Values.",
                    [-30.0, -130.0],
                    [828.0, 130.0 + ROW + TALL + 30.0],
                    GRAY,
                );
            }
            Template::Grid => {
                let across = int_input(&mut g, "x_amount", 8, [-300.0, 0.0]);
                let down = int_input(&mut g, "y_amount", 8, [-300.0, 110.0]);
                let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
                set(
                    &mut g,
                    fx,
                    "program",
                    ParamValue::Text(
                        "# One square a cell of the grid: i counts them, across, then down.\n\
                         draw = i < x_amount * y_amount\n\
                         x = (mod(i, x_amount) + 0.5) / x_amount\n\
                         y = (floor(i / x_amount) + 0.5) / y_amount\n\
                         size = 0.8 / x_amount, 0.8 / y_amount"
                            .into(),
                    ),
                );
                set(&mut g, fx, "count", ParamValue::Int(256));
                set(&mut g, fx, "pattern", ParamValue::Enum(2));
                set(&mut g, fx, "depth", ParamValue::Int(2));
                let _ = g.connect(across, "o:value", fx, &crate::graph::var_socket_key("x_amount"));
                let _ = g.connect(down, "o:value", fx, &crate::graph::var_socket_key("y_amount"));
                let out = output(&mut g, "output", 0, [400.0, 0.0]);
                let _ = g.connect(fx, "o:output", out, "i:input");
                frame(
                    &mut g,
                    "A grid drawn by one FX-Map",
                    "The FX-Map draws up to 256 splats (Count) in the one cell of depth 0; its program places each by \
                     its number i, across and then down, and leaves out those past x_amount × y_amount, the input \
                     values. Change them in Input Values.",
                    [-330.0, -130.0],
                    [928.0, 130.0 + FX_TALL + 2.0 * 20.0 + 30.0],
                    GRAY,
                );
            }
            Template::Noise => {
                let background = g.add_node(NodeKind::UNIFORM_COLOR, [-320.0, 0.0]);
                set(&mut g, background, "color", ParamValue::Color(gray(0.5)));
                set(&mut g, background, "output", ParamValue::Enum(1));
                let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
                set(
                    &mut g,
                    fx,
                    "program",
                    ParamValue::Text("# A splat at every depth, each finer one weaker.\nsize = 1.4\nvalue = (rand(1) - 0.5) * 0.6^depth".into()),
                );
                set(&mut g, fx, "count", ParamValue::Int(1));
                set(&mut g, fx, "from_depth", ParamValue::Int(0));
                set(&mut g, fx, "depth", ParamValue::Int(8));
                set(&mut g, fx, "pattern", ParamValue::Enum(12));
                set(&mut g, fx, "blend", ParamValue::Enum(1));
                let _ = g.connect(background, "o:output", fx, "i:background");
                let out = output(&mut g, "output", 0, [400.0, 0.0]);
                let _ = g.connect(fx, "o:output", out, "i:input");
                frame(
                    &mut g,
                    "Noise made of splats",
                    "Every split of the image, from depth 0 (one cell) to depth 8 (256 × 256 cells), draws a soft \
                     splat a cell, lighter or darker at random and the weaker the finer (0.6^depth); Add sums them \
                     over the gray of the background, as noise is made of octaves.",
                    [-350.0, -130.0],
                    [948.0, 130.0 + FX_TALL + 30.0],
                    GRAY,
                );
            }
            Template::ScanProcessing => scan_processing(&mut g),
            Template::GrayFilter | Template::ColorFilter => {
                let input = g.add_node(NodeKind::IMAGE_INPUT, [0.0, 0.0]);
                let _ = g.bind_variable(input, "input");
                if self == Template::ColorFilter
                    && let Some(image) = g.variable_mut("input").and_then(|v| v.image.as_mut())
                {
                    image.kind = crate::nodes::ImageKind::Color;
                }
                let output = output(&mut g, "output", 0, [420.0, 0.0]);
                let _ = g.connect(input, "o:output", output, "i:input");
                frame(
                    &mut g,
                    "Put what changes the image between its input and its output",
                    "The Input is the image the filter is given where it is used as a node; the Output is what it \
                     gives back. Connect the nodes that change it between them.",
                    [-30.0, -90.0],
                    [648.0, 90.0 + TALL + 30.0],
                    GRAY,
                );
            }
            _ => {}
        }
        g
    }
}

/// The graph of a material template: each map a uniform value into its Output, side by side in
/// as many columns as it takes to keep the graph from running far down, in a frame that says
/// what to do with them.
fn material(g: &mut Graph, template: Template, maps: &[Map]) {
    let columns = match maps.len() {
        0..=8 => 1,
        9..=16 => 2,
        _ => 4,
    };
    const PAIR: f32 = 620.0;
    for (i, Map(name, value, is_gray, usage)) in maps.iter().enumerate() {
        let [x, y] = [(i % columns) as f32 * PAIR, (i / columns) as f32 * ROW];
        // (A number where the map holds numbers: a color's channels are from 0 to 1.)
        let uniform = if past_one(*usage) {
            let uniform = g.add_node(NodeKind::UNIFORM_VALUE, [x, y]);
            set(g, uniform, "value", ParamValue::Float(value[0]));
            uniform
        } else {
            let uniform = g.add_node(NodeKind::UNIFORM_COLOR, [x, y]);
            set(g, uniform, "color", ParamValue::Color(*value));
            set(g, uniform, "output", ParamValue::Enum(u32::from(*is_gray)));
            uniform
        };
        let out = output(g, name, usage.index(), [x + 340.0, y]);
        if past_one(*usage) {
            set(g, out, "bit_depth", ParamValue::Enum(DEPTH_16F));
        }
        let _ = g.connect(uniform, "o:output", out, "i:input");
    }
    let rows = maps.len().div_ceil(columns) as f32;
    let layer = match template {
        Template::Complete => " Every layer is off but the base: give a layer its weight to see it.",
        Template::Pbr => "",
        _ => " The layer is on all the way: its weight is 1.",
    };
    let said = format!(
        "Each Output is one map of an OpenPBR material, named as the standard names it and of the usage of its \
         parameter, which the 3D view shows it by. The uniform colors and values only hold a value: connect what \
         makes each map in their place.{layer}"
    );
    let width = (columns as f32 - 1.0) * PAIR + 568.0;
    let top = said_height(&said, width);
    frame(g, "Replace each uniform value with what makes the map", &said, [-30.0, -top], [width, top + (rows - 1.0) * ROW + TALL + 30.0], GRAY);
}

/// How much room a frame `width` wide takes above its nodes for its title and for what it says,
/// at the most: as the graph view draws it (a title bar, lines of a size of text a little over
/// 12 points, about 7 points a letter), a line more than it takes, rounded up to 10.
fn said_height(said: &str, width: f32) -> f32 {
    let per_line = ((width - 16.0) / 7.25).max(1.0);
    let lines = (said.chars().count() as f32 / per_line).ceil() + 1.0;
    ((30.0 + 16.0 + lines * 16.9) / 10.0).ceil() * 10.0
}

/// An image input of the graph called `name`, taking `kind`, `color` while it has no picture.
fn image_input(g: &mut Graph, name: &str, kind: crate::nodes::ImageKind, color: [f32; 4], pos: [f32; 2]) -> NodeId {
    let node = g.add_node(NodeKind::IMAGE_INPUT, pos);
    let _ = g.bind_variable(node, name);
    if let Some(var) = g.variable_mut(name) {
        (var.value, var.default) = (Value::Color(color), Value::Color(color));
        if let Some(image) = var.image.as_mut() {
            image.kind = kind;
        }
    }
    node
}

/// An integer input value of the graph called `name`, `value` by default (1 to 16).
fn int_input(g: &mut Graph, name: &str, value: i32, pos: [f32; 2]) -> NodeId {
    let node = g.add_node(NodeKind::INT_INPUT, pos);
    let _ = g.bind_variable(node, name);
    if let Some(var) = g.variable_mut(name) {
        (var.value, var.default, var.min, var.max) = (Value::Int(value), Value::Int(value), Value::Int(1), Value::Int(16));
    }
    node
}

/// The graph of Scan Processing: the maps of a scan, each an image input, tiled; the color's
/// light taken away; roughness worked out; each map an Output, as Substance Designer's scan
/// template has them.
fn scan_processing(g: &mut Graph) {
    use crate::nodes::ImageKind;
    // The scan: an input a row, of the color it is while it has no picture (a flat normal),
    // each made to tile.
    let mut tiled = |name: &str, kind: ImageKind, color: [f32; 4], y: f32| {
        let input = image_input(g, name, kind, color, [0.0, y]);
        let tiled = g.add_node(NodeKind::MAKE_IT_TILE, [300.0, y]);
        let _ = g.connect(input, "o:output", tiled, "i:input");
        tiled
    };
    let albedo = tiled("albedo", ImageKind::Color, gray(0.5), 0.0);
    let normal = tiled("normal", ImageKind::Color, FLAT, 2.0 * ROW);
    // ("height" says the height of the image in pixels where expressions read it.)
    let height = tiled("heightmap", ImageKind::Gray, gray(0.5), 3.0 * ROW);
    // The light it was photographed in: the color blurred far; the color divided by it is the
    // surface's own.
    let light = g.add_node(NodeKind::BLUR, [600.0, 0.0]);
    set(g, light, "radius", ParamValue::Float(48.0));
    let _ = g.connect(albedo, "o:output", light, "i:input");
    let delit = g.add_node(NodeKind::PIXEL_MATH, [900.0, 0.0]);
    set(
        g,
        delit,
        "expression",
        ParamValue::Text(
            "# How bright the light was where the pixel is.\n\
             shade = light.r * 0.3 + light.g * 0.59 + light.b * 0.11\n\
             # The surface's own color: as bright everywhere as half white.\n\
             albedo.rgb / max(shade, 0.02) * 0.5"
                .into(),
        ),
    );
    let _ = g.connect(albedo, "o:output", delit, &crate::graph::var_socket_key("albedo"));
    let _ = g.connect(light, "o:output", delit, &crate::graph::var_socket_key("light"));
    let rough = g.add_node(NodeKind::PIXEL_MATH, [900.0, ROW]);
    set(
        g,
        rough,
        "expression",
        ParamValue::Text("# Darker in the color, rougher.\nlum = color.r * 0.3 + color.g * 0.59 + color.b * 0.11\n0.45 + 0.4 * (1 - clamp(lum, 0, 1))".into()),
    );
    let _ = g.connect(delit, "o:output", rough, &crate::graph::var_socket_key("color"));
    // A scan is of no metal: none.
    let metal = g.add_node(NodeKind::UNIFORM_COLOR, [900.0, 4.0 * ROW]);
    set(g, metal, "color", ParamValue::Color(gray(0.0)));
    set(g, metal, "output", ParamValue::Enum(1));
    // The maps, in the order their files are listed.
    let maps = [
        ("basecolor", USAGE_BASE_COLOR, delit, 0.0),
        ("normal", USAGE_NORMAL, normal, 2.0 * ROW),
        ("roughness", USAGE_ROUGHNESS, rough, ROW),
        ("metallic", USAGE_METALLIC, metal, 4.0 * ROW),
        ("height", USAGE_HEIGHT, height, 3.0 * ROW),
    ];
    for (name, usage, from, y) in maps {
        let out = output(g, name, usage, [1200.0, y]);
        let _ = g.connect(from, "o:output", out, "i:input");
    }
    frame(
        g,
        "A material from a scan",
        "A scan of a surface, as a camera and a scanner make it: its color, its normals and its height, each an \
         Input. Make It Tile blends each over its edges, so that it repeats without a seam. A photograph keeps the \
         light it was taken in: Blur finds that light, and the color divided by it is the surface's own. The \
         roughness follows the color, darker rougher; a scan is of no metal. The pictures of the scan are chosen \
         in Input Values.",
        [-30.0, -130.0],
        [1428.0, 130.0 + 4.0 * ROW + TALL + 30.0],
        GRAY,
    );
}

fn set(g: &mut Graph, node: NodeId, key: &str, value: ParamValue) {
    if let Some(p) = g.node_mut(node).and_then(|n| n.param_mut(key)) {
        p.value = value;
    }
}

/// How far apart the rows of nodes are: as tall as a node with its picture, and a gap.
const ROW: f32 = 240.0;
/// How tall a node with its picture and one row of sockets is drawn, and an FX-Map.
const TALL: f32 = 198.0;
const FX_TALL: f32 = 296.0;
/// The colors of frames: gray for what a template says to do, blue for what the nodes of an
/// example do.
const GRAY: [f32; 4] = [0.49, 0.49, 0.49, 1.0];
const HOW: [f32; 4] = [0.28, 0.51, 0.8, 1.0];

/// A frame titled `title` that says `description` under it, at `pos`, `size` big.
fn frame(g: &mut Graph, title: &str, description: &str, pos: [f32; 2], size: [f32; 2], color: [f32; 4]) {
    let id = g.add_comment(title.into(), pos, size);
    if let Some(c) = g.comment_mut(id) {
        c.description = description.into();
        c.color = color;
    }
}

/// An Output named `name`, which the 3D view takes for `usage`.
fn output(g: &mut Graph, name: &str, usage: u32, pos: [f32; 2]) -> NodeId {
    let output = g.add_node(NodeKind::OUTPUT, pos);
    set(g, output, "name", ParamValue::Text(name.into()));
    set(g, output, "usage", ParamValue::Enum(usage));
    output
}

/// What the Home screen's Learn opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Example {
    /// The brick wall (see [`crate::demo`]).
    BrickWall,
    /// Programs of FX-Maps: scattered discs, bricks, clouds, confetti, dots.
    FxMaps,
    /// Pixel Math with vectors: a normal from a height, and three maps packed in one.
    PixelMath,
}

impl Example {
    pub const ALL: [Example; 3] = [Example::BrickWall, Example::FxMaps, Example::PixelMath];

    /// What names it where a name of words will not do.
    pub fn id(self) -> &'static str {
        match self {
            Example::BrickWall => "brick-wall",
            Example::FxMaps => "fx-maps",
            Example::PixelMath => "pixel-math",
        }
    }

    pub fn from_id(id: &str) -> Option<Example> {
        Example::ALL.into_iter().find(|e| e.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Example::BrickWall => "A brick wall",
            Example::FxMaps => "FX-Map programs",
            Example::PixelMath => "Pixel Math with vectors",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Example::BrickWall => {
                "Bricks from a Tile Generator, worn by noise, colored by a gradient: input values drive the \
                 graph, and five outputs make a material in the 3D view."
            }
            Example::FxMaps => {
                "Five FX-Maps and their programs: splats scattered at random, bricks laid in rows, clouds made of \
                 ever finer splats added up, confetti in colors, dots left out at random."
            }
            Example::PixelMath => {
                "Vectors in Pixel Math: a normal map worked out from a height by how it slopes, and three maps \
                 packed into the channels of one (occlusion, roughness, metallic)."
            }
        }
    }

    /// The package, and the node it shows in the 2D view first.
    pub fn package(self) -> (Package, Option<NodeId>) {
        match self {
            Example::BrickWall => crate::demo::package(),
            Example::FxMaps => fx_maps(),
            Example::PixelMath => pixel_math(),
        }
    }
}

/// A graph of the FX-Map example: an FX-Map running `program` into an Output, over a
/// background, if any, in a frame that says what it does.
struct FxExample {
    title: &'static str,
    about: &'static str,
    program: &'static str,
    count: i32,
    /// From which depth to which.
    depths: (i32, i32),
    pattern: u32,
    blend: u32,
    background: Option<f32>,
    output: &'static str,
    usage: u32,
}

impl FxExample {
    fn graph(&self) -> (Graph, NodeId) {
        let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
        let fx = g.add_node(NodeKind::FX_MAP, [0.0, 0.0]);
        set(&mut g, fx, "program", ParamValue::Text(self.program.into()));
        set(&mut g, fx, "count", ParamValue::Int(self.count));
        set(&mut g, fx, "from_depth", ParamValue::Int(self.depths.0));
        set(&mut g, fx, "depth", ParamValue::Int(self.depths.1));
        set(&mut g, fx, "pattern", ParamValue::Enum(self.pattern));
        set(&mut g, fx, "blend", ParamValue::Enum(self.blend));
        let mut left = -30.0;
        if let Some(v) = self.background {
            let b = g.add_node(NodeKind::UNIFORM_COLOR, [-320.0, 0.0]);
            set(&mut g, b, "color", ParamValue::Color([v, v, v, 1.0]));
            set(&mut g, b, "output", ParamValue::Enum(1));
            let _ = g.connect(b, "o:output", fx, "i:background");
            left = -350.0;
        }
        let out = output(&mut g, self.output, self.usage, [400.0, 0.0]);
        let _ = g.connect(fx, "o:output", out, "i:input");
        frame(&mut g, self.title, self.about, [left, -150.0], [598.0 - left, 150.0 + FX_TALL + 30.0], HOW);
        (g, fx)
    }
}

fn fx_maps() -> (Package, Option<NodeId>) {
    const MAX: u32 = 0;
    const ADD: u32 = 1;
    const STACK: u32 = 2;
    let examples = [
        (
            "Scattered",
            FxExample {
                title: "64 splats at random",
                about: "At depth 0 the whole image is one cell, and it draws Count splats: 64. The program says what \
                        each is, reading rand(k), a random number of its own: where it goes (x, y), how big, how far \
                        turned and how bright. Where splats overlap, Max keeps the brighter. The Output is a height: \
                        the 3D view raises the model by it.",
                program: crate::nodes::DEFAULT_FX_MAP_PROGRAM,
                count: 64,
                depths: (0, 0),
                pattern: 0,
                blend: MAX,
                background: None,
                output: "scattered",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Bricks",
            FxExample {
                title: "Bricks laid in rows",
                about: "Depth 3 splits the image into 8 x 8 cells, and each draws Count splats: 2, numbered by i. \
                        The program lays the first in the upper half of its cell and the second in the lower half, \
                        half a brick along (x = 0.5 * i): rows of bricks, each as wide as its cell and not quite \
                        half as high, each as bright as its own random number says.",
                program: "# Two bricks a cell, the lower row half a brick along.\ny = 0.25 + 0.5 * i\nx = 0.5 * i\nsize = 0.94, 0.44\nvalue = 0.45 + 0.55 * rand(1)",
                count: 2,
                depths: (3, 3),
                pattern: 2,
                blend: MAX,
                background: None,
                output: "bricks",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Clouds",
            FxExample {
                title: "Clouds of ever finer splats",
                about: "From depth 0 to depth 8 every split of the image draws, one splat a cell: 1 at depth 0, 4 at \
                        1, 16 at 2, and 65 536 at 8. Each is a soft blob a little larger than its cell, lighter or \
                        darker at random, and the finer the weaker (0.6^depth). Add sums them over the gray of the \
                        background: clouds, made as noise is made of octaves.",
                program: "# A splat at every depth, each finer one weaker: clouds.\nsize = 1.4\nvalue = (rand(1) - 0.5) * 0.6^depth",
                count: 1,
                depths: (0, 8),
                pattern: 12,
                blend: ADD,
                background: Some(0.5),
                output: "clouds",
                usage: USAGE_HEIGHT,
            },
        ),
        (
            "Confetti",
            FxExample {
                title: "Confetti in colors",
                about: "600 splats in the one cell of depth 0, each a little strip placed, sized and turned at \
                        random. A program that gives color makes the image a color one; Stack lays each later \
                        splat over the earlier ones. The Output is a base color: the 3D view shows it as the \
                        model's colors.",
                program: "x = rand(1); y = rand(2)\nsize = 0.02 + 0.03 * rand(3), 0.01 + 0.01 * rand(4)\nangle = 360 * rand(5)\ncolor = rand(6), rand(7), rand(8)",
                count: 600,
                depths: (0, 0),
                pattern: 1,
                blend: STACK,
                background: None,
                output: "confetti",
                usage: USAGE_BASE_COLOR,
            },
        ),
        (
            "Dots",
            FxExample {
                title: "A dot in every cell, some left out",
                about: "Depth 5 splits the image into 32 x 32 cells, each with one dot in its middle (x and y are 0.5 \
                        when the program does not say). Its size and brightness are random; draw = rand(3) < 0.8 \
                        leaves about one in five out: draw is 0 where the comparison does not hold.",
                program: "# A dot in every cell of a 32 x 32 grid, one in five left out.\nsize = 0.9 * rand(1)^0.5\nvalue = 0.3 + 0.7 * rand(2)\ndraw = rand(3) < 0.8",
                count: 1,
                depths: (5, 5),
                pattern: 0,
                blend: MAX,
                background: None,
                output: "dots",
                usage: USAGE_HEIGHT,
            },
        ),
    ];
    let mut graphs = examples.iter().map(|(name, example)| (*name, example.graph()));
    let (name, (first, preview)) = graphs.next().expect("graphs");
    let mut package = Package::new(name, first);
    for (name, (graph, _)) in graphs {
        package.add_graph(name, graph);
    }
    (package, Some(preview))
}

fn pixel_math() -> (Package, Option<NodeId>) {
    // A normal from a height.
    let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
    let height = g.add_node(NodeKind::NOISE, [0.0, 0.0]);
    let normal = g.add_node(NodeKind::PIXEL_MATH, [300.0, 0.0]);
    set(
        &mut g,
        normal,
        "expression",
        ParamValue::Text(
            "# How steeply the height h rises across and down, in its pixels.\n\
             dx = sample(h, u + 1 / width, v) - sample(h, u - 1 / width, v)\n\
             dy = sample(h, u, v + 1 / height) - sample(h, u, v - 1 / height)\n\
             n = normalize(vec3(-dx, -dy, 0.02))\n\
             # From -1..1 to the colors of a normal map.\n\
             n * 0.5 + 0.5"
                .into(),
        ),
    );
    let _ = g.connect(height, "o:output", normal, &crate::graph::var_socket_key("h"));
    let out = output(&mut g, "normal", USAGE_NORMAL, [600.0, 0.0]);
    let _ = g.connect(normal, "o:output", out, "i:input");
    let heights = output(&mut g, "height", USAGE_HEIGHT, [600.0, ROW]);
    let _ = g.connect(height, "o:output", heights, "i:input");
    frame(
        &mut g,
        "A normal worked out from how the height slopes",
        "Pixel Math works out every pixel of its image from its expression. Here it reads the height h a pixel \
         to either side: dx and dy are how much it rises across and down. The normal leans away from the rise: \
         vec3(-dx, -dy, 0.02), made one long by normalize, the small z saying how steep a slope looks. n * 0.5 + \
         0.5 turns it from -1..1 into the colors of a normal map. The normal lights the model in the 3D view; the \
         height raises it.",
        [-30.0, -130.0],
        [828.0, 130.0 + ROW + TALL + 30.0],
        HOW,
    );
    let mut package = Package::new("Normal from Height", g);

    // Three maps in the channels of one.
    let mut g = Graph { resolution: [1024, 1024], ..Graph::default() };
    let mut maps = Vec::new();
    for (row, (kind, name)) in [(crate::nodes::NOISE_CELLS, "ao"), (0, "rough"), (crate::nodes::NOISE_WEAVE, "metal")].into_iter().enumerate() {
        let noise = g.add_node(NodeKind::NOISE, [0.0, row as f32 * ROW]);
        set(&mut g, noise, "type", ParamValue::Enum(kind));
        maps.push((noise, name));
    }
    let packed = g.add_node(NodeKind::PIXEL_MATH, [300.0, ROW]);
    set(&mut g, packed, "expression", ParamValue::Text("# Occlusion in red, roughness in green, metallic in blue.\nvec3(ao, rough, metal)".into()));
    for (noise, name) in maps {
        let _ = g.connect(noise, "o:output", packed, &crate::graph::var_socket_key(name));
    }
    let out = output(&mut g, "orm", crate::nodes::USAGE_BASE_COLOR, [600.0, ROW]);
    let _ = g.connect(packed, "o:output", out, "i:input");
    frame(
        &mut g,
        "Three maps in the channels of one",
        "Game engines read several gray maps from the channels of one picture: fewer files, less memory. Each \
         Noise here is one map: occlusion, roughness, metallic. vec3(ao, rough, metal) makes one color of the \
         three, occlusion in red, roughness in green, metallic in blue: ORM, as Unreal Engine packs them. The \
         Output shows it as the model's base color, to see all three at once.",
        [-30.0, -130.0],
        [828.0, 130.0 + 2.0 * ROW + TALL + 30.0],
        HOW,
    );
    package.add_graph("Channel Packing", g);
    (package, Some(normal))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every template and example is whole: its links lead somewhere, nothing in it says
    /// anything is wrong, and a template's outputs are those it says, in its category.
    #[test]
    fn templates_and_examples_are_whole() {
        let pbr = Template::Pbr.graph([512, 256], Precision::Full);
        assert_eq!((pbr.resolution, pbr.precision), ([512, 256], Precision::Full));
        // A panorama: square, as wide as asked, its light in 32-bit floats.
        let panorama = Template::StudioPanorama.graph([1024, 512], Precision::Half);
        assert_eq!((panorama.resolution, panorama.precision), ([1024, 1024], Precision::Full));
        assert!(Template::STARTERS.iter().all(|t| Template::ALL.contains(t)));
        assert!(Template::Empty.graph([2048, 2048], Precision::Half).nodes.is_empty());
        let mut graphs = Vec::new();
        for template in Template::ALL {
            let g = template.graph([512, 512], Precision::Half);
            let outputs: Vec<&str> = g.nodes_of(NodeKind::OUTPUT).map(|n| n.text("name")).collect();
            assert_eq!(outputs, template.outputs(), "{template:?}");
            assert_eq!(Template::from_id(template.id()), Some(template));
            assert!(Category::ALL.contains(&template.category()));
            graphs.push(g);
        }
        assert!(Category::ALL.iter().all(|c| Template::ALL.iter().any(|t| t.category() == *c)), "no category is empty");
        for example in Example::ALL {
            let (package, preview) = example.package();
            assert!(preview.is_some_and(|id| package.graphs[0].graph.node(id).is_some()), "{example:?} shows one of its nodes");
            graphs.extend(package.graphs.iter().map(|g| Graph::clone(&g.graph)));
        }
        for g in &graphs {
            let ev = crate::eval::evaluate(g);
            for n in &g.nodes {
                let e = ev.node(n.id).unwrap();
                assert!(e.errors.is_empty() && e.warnings.is_empty(), "{}: {:?} {:?}", n.def().name, e.errors, e.warnings);
            }
            for out in g.nodes_of(NodeKind::OUTPUT) {
                assert!(g.link_into(out.id, "i:input").is_some(), "output {} is not connected", out.text("name"));
            }
        }
    }
}
