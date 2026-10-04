//! What an Output's map is for: its usage, as Substance Designer's outputs have one. A usage
//! is how the 3D view knows where a map goes on the model (and the export presets which file
//! a game engine wants it in): every parameter of OpenPBR, the standard material the templates
//! make, has one; so has each map older material models read (specular and glossiness, a
//! bump, a mask …), and the panorama, an environment that lights the scene.
//!
//! Files say a usage by its id, which never changes; the order here is that of the "usage"
//! parameter's options (the first eight are those it always had), the menu shows them by
//! [`UsageGroup`].

/// The parts of a material the usages are grouped by in their menu: OpenPBR's, then those
/// of older material models, then the environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UsageGroup {
    Base,
    Specular,
    Transmission,
    Subsurface,
    Coat,
    Fuzz,
    Emission,
    ThinFilm,
    Geometry,
    /// Maps of the material models before OpenPBR (specular and glossiness, …).
    Other,
    Environment,
    /// A map of no usage: written, not shown on the model.
    Custom,
}

impl UsageGroup {
    pub const ALL: [UsageGroup; 12] = [
        UsageGroup::Base,
        UsageGroup::Specular,
        UsageGroup::Transmission,
        UsageGroup::Subsurface,
        UsageGroup::Coat,
        UsageGroup::Fuzz,
        UsageGroup::Emission,
        UsageGroup::ThinFilm,
        UsageGroup::Geometry,
        UsageGroup::Other,
        UsageGroup::Environment,
        UsageGroup::Custom,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            UsageGroup::Base => "Base",
            UsageGroup::Specular => "Specular",
            UsageGroup::Transmission => "Transmission",
            UsageGroup::Subsurface => "Subsurface",
            UsageGroup::Coat => "Coat",
            UsageGroup::Fuzz => "Fuzz",
            UsageGroup::Emission => "Emission",
            UsageGroup::ThinFilm => "Thin Film",
            UsageGroup::Geometry => "Geometry",
            UsageGroup::Other => "Other Workflows",
            UsageGroup::Environment => "Environment",
            UsageGroup::Custom => "None",
        }
    }
}

macro_rules! usages {
    ($($variant:ident = $index:literal, $id:literal, $name:literal, $group:ident, $words:literal, $tip:literal;)*) => {
        /// What an Output's map is for (see the module's documentation). The value is the
        /// option of the Output's "usage" parameter.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u32)]
        pub enum Usage {
            $($variant = $index,)*
        }

        impl Usage {
            /// Every usage, in the order of the parameter's options.
            pub const ALL: [Usage; [$($index),*].len()] = [$(Usage::$variant),*];

            /// What a file calls it.
            pub const fn id(self) -> &'static str {
                match self { $(Usage::$variant => $id,)* }
            }

            /// What the menu calls it.
            pub const fn name(self) -> &'static str {
                match self { $(Usage::$variant => $name,)* }
            }

            pub const fn group(self) -> UsageGroup {
                match self { $(Usage::$variant => UsageGroup::$group,)* }
            }

            /// Words it is found by besides its name: the names Substance Designer gives the
            /// same usage, and OpenPBR's name of its parameter.
            pub const fn words(self) -> &'static str {
                match self { $(Usage::$variant => $words,)* }
            }

            /// What the map is, said in its menu.
            pub const fn tip(self) -> &'static str {
                match self { $(Usage::$variant => $tip,)* }
            }
        }
    };
}

usages! {
    Custom = 0, "custom", "Custom", Custom, "none other",
        "No usage: the map is written, the 3D view does not show it.";
    BaseColor = 1, "base_color", "Base Color", Base, "basecolor basecolour albedo base_color",
        "OpenPBR base_color: the color of the surface (its diffuse color, or a metal's reflection).";
    Normal = 2, "normal", "Normal", Geometry, "geometry_normal normal map",
        "OpenPBR geometry_normal: the surface's normals, as a tangent-space normal map (OpenGL).";
    Height = 3, "height", "Height", Geometry, "heightmap",
        "How high each point is: the 3D view raises the model by it, and lights the relief it makes where no normal map is given.";
    Roughness = 4, "roughness", "Roughness", Specular, "specularroughness specular_roughness",
        "OpenPBR specular_roughness: how rough the surface is, from a mirror (0) to matte (1).";
    Metallic = 5, "metallic", "Metallic", Base, "metalness base_metalness metal",
        "OpenPBR base_metalness: metal (1) or not (0).";
    AmbientOcclusion = 6, "ambient_occlusion", "Ambient Occlusion", Geometry, "ambientocclusion ao occlusion",
        "How much of the light around reaches each point: crevices darker.";
    Emissive = 7, "emissive", "Emission Color", Emission, "emissive emissioncolor emission_color glow",
        "OpenPBR emission_color: the color of the light the surface gives off. Without an Emission Luminance, it glows as bright as it is.";
    BaseWeight = 8, "base_weight", "Base Weight", Base, "baseweight",
        "OpenPBR base_weight: how much of the base color shows (a multiplier of it).";
    BaseDiffuseRoughness = 9, "base_diffuse_roughness", "Diffuse Roughness", Base, "basediffuseroughness oren nayar",
        "OpenPBR base_diffuse_roughness: how rough the matte part is: rougher, flatter, as clay or the moon look.";
    SpecularWeight = 10, "specular_weight", "Specular Weight", Specular, "specularweight",
        "OpenPBR specular_weight: how much the surface reflects (a multiplier of the reflection of its IOR).";
    SpecularColor = 11, "specular_color", "Specular Color", Specular, "specularcolor specularedgecolor edge tint",
        "OpenPBR specular_color: the tint of the reflection; of a metal, the color at its edges.";
    SpecularRoughnessAnisotropy = 12, "specular_roughness_anisotropy", "Anisotropy", Specular,
        "specularroughnessanisotropy anisotropylevel anisotropy_level brushed",
        "OpenPBR specular_roughness_anisotropy: how much rougher the surface is across its tangent than along it: brushed metal.";
    AnisotropyAngle = 13, "anisotropy_angle", "Anisotropy Angle", Specular, "anisotropyangle rotation",
        "Which way the anisotropy runs: the tangent turned by this much, black to white once round.";
    SpecularIor = 14, "specular_ior", "Specular IOR", Specular, "specularior ior index refraction",
        "OpenPBR specular_ior: the index of refraction of the surface (1.5 for glass and plastics): how much it reflects, and bends light through it.";
    TransmissionWeight = 15, "transmission_weight", "Transmission Weight", Transmission, "transmissionweight transmissive glass",
        "OpenPBR transmission_weight: how much light goes through the surface rather than being scattered by it: glass, water.";
    TransmissionColor = 16, "transmission_color", "Transmission Color", Transmission, "transmissioncolor absorptioncolor",
        "OpenPBR transmission_color: the color light takes going through.";
    TransmissionDepth = 17, "transmission_depth", "Transmission Depth", Transmission, "transmissiondepth absorptiondistance",
        "OpenPBR transmission_depth: how deep into the material light gets the Transmission Color (0: at the surface).";
    TransmissionScatter = 18, "transmission_scatter", "Transmission Scatter", Transmission, "transmissionscatter milky",
        "OpenPBR transmission_scatter: the color light is scattered with inside: milky, cloudy glass.";
    TransmissionScatterAnisotropy = 19, "transmission_scatter_anisotropy", "Transmission Anisotropy", Transmission,
        "transmissionscatteranisotropy",
        "OpenPBR transmission_scatter_anisotropy: whether light scattered inside goes on forward (1: the glass clouds less, with what is behind it), evenly (0) or back (-1: milkier). Below 0, its Output writes floats.";
    TransmissionDispersionScale = 20, "transmission_dispersion_scale", "Dispersion Scale", Transmission,
        "transmissiondispersionscale rainbow prism",
        "OpenPBR transmission_dispersion_scale: how much light going through splits into its colors.";
    TransmissionDispersionAbbeNumber = 21, "transmission_dispersion_abbe_number", "Abbe Number", Transmission,
        "transmissiondispersionabbenumber abbe",
        "OpenPBR transmission_dispersion_abbe_number: the material's Abbe number: the lower, the more it splits light.";
    SubsurfaceWeight = 22, "subsurface_weight", "Subsurface Weight", Subsurface, "subsurfaceweight translucency scattering sss skin wax",
        "OpenPBR subsurface_weight: how much light goes in under the surface and comes out elsewhere: skin, wax, marble.";
    SubsurfaceColor = 23, "subsurface_color", "Subsurface Color", Subsurface, "subsurfacecolor scatteringcolor",
        "OpenPBR subsurface_color: the color of the light scattered under the surface.";
    SubsurfaceRadius = 24, "subsurface_radius", "Subsurface Radius", Subsurface, "subsurfaceradius scatteringdistance",
        "OpenPBR subsurface_radius: how far light goes under the surface.";
    SubsurfaceRadiusScale = 25, "subsurface_radius_scale", "Subsurface Radius Scale", Subsurface,
        "subsurfaceradiusscale scatteringdistancescale",
        "OpenPBR subsurface_radius_scale: how far each of red, green and blue goes, of the radius (skin: red the farthest).";
    SubsurfaceScatterAnisotropy = 26, "subsurface_scatter_anisotropy", "Subsurface Anisotropy", Subsurface,
        "subsurfacescatteranisotropy",
        "OpenPBR subsurface_scatter_anisotropy: whether light scattered under the surface goes on forward (1: deeper, glowing through where lit from behind), evenly (0) or back (-1). Below 0, its Output writes floats.";
    CoatWeight = 27, "coat_weight", "Coat Weight", Coat, "coatweight coatopacity clearcoat clear coat varnish",
        "OpenPBR coat_weight: how much of a clear coat lies over the surface: varnish, car paint.";
    CoatColor = 28, "coat_color", "Coat Color", Coat, "coatcolor",
        "OpenPBR coat_color: the tint of the coat, which colors what is under it.";
    CoatRoughness = 29, "coat_roughness", "Coat Roughness", Coat, "coatroughness",
        "OpenPBR coat_roughness: how rough the coat is.";
    CoatRoughnessAnisotropy = 30, "coat_roughness_anisotropy", "Coat Anisotropy", Coat, "coatroughnessanisotropy",
        "OpenPBR coat_roughness_anisotropy: the anisotropy of the coat, along the Coat Tangent.";
    CoatIor = 31, "coat_ior", "Coat IOR", Coat, "coatior",
        "OpenPBR coat_ior: the index of refraction of the coat (1.6 by default): how much it reflects.";
    CoatDarkening = 32, "coat_darkening", "Coat Darkening", Coat, "coatdarkening",
        "OpenPBR coat_darkening: how much darker and more saturated the coat makes what is under it (light caught inside it), from none (0) to all (1).";
    CoatNormal = 33, "coat_normal", "Coat Normal", Coat, "coatnormal geometry_coat_normal",
        "OpenPBR geometry_coat_normal: the coat's own normals (without them, the coat is as smooth as the model).";
    CoatTangent = 34, "coat_tangent", "Coat Tangent", Coat, "coattangent geometry_coat_tangent",
        "OpenPBR geometry_coat_tangent: which way the coat's anisotropy runs.";
    FuzzWeight = 35, "fuzz_weight", "Fuzz Weight", Fuzz, "fuzzweight sheenopacity sheen velvet",
        "OpenPBR fuzz_weight: how much of a fine fuzz lies over everything: velvet, dust, peach skin.";
    FuzzColor = 36, "fuzz_color", "Fuzz Color", Fuzz, "fuzzcolor sheencolor",
        "OpenPBR fuzz_color: the color of the fuzz.";
    FuzzRoughness = 37, "fuzz_roughness", "Fuzz Roughness", Fuzz, "fuzzroughness sheenroughness",
        "OpenPBR fuzz_roughness: how the fuzz's fibers lie: low, close to the surface, shining at its edges; high, spread.";
    EmissionLuminance = 38, "emission_luminance", "Emission Luminance", Emission, "emissionluminance emissive intensity nits",
        "OpenPBR emission_luminance: how bright the surface glows, in nits (candelas a square meter): 1000 glows as bright as its Emission Color.";
    ThinFilmWeight = 39, "thin_film_weight", "Thin Film Weight", ThinFilm, "thinfilmweight iridescence soap oil",
        "OpenPBR thin_film_weight: how much of a thin film lies on the surface, coloring its reflection: soap bubbles, oil, heated metal.";
    ThinFilmThickness = 40, "thin_film_thickness", "Thin Film Thickness", ThinFilm, "thinfilmthickness",
        "OpenPBR thin_film_thickness: how thick the film is, in micrometers (0.5 by default): which colors it shows.";
    ThinFilmIor = 41, "thin_film_ior", "Thin Film IOR", ThinFilm, "thinfilmior",
        "OpenPBR thin_film_ior: the index of refraction of the film (1.4 by default).";
    Opacity = 42, "opacity", "Opacity", Geometry, "geometry_opacity alpha transparency cutout",
        "OpenPBR geometry_opacity: how much of the surface is there: 0 none (a hole), 1 all of it.";
    ThinWalled = 43, "thin_walled", "Thin Walled", Geometry, "thinwalled geometry_thin_walled leaf paper",
        "OpenPBR geometry_thin_walled: whether the surface is a thin sheet (a leaf, paper) rather than the outside of a solid: light goes through it unbent.";
    Tangent = 44, "tangent", "Tangent", Geometry, "geometry_tangent direction anisotropy",
        "OpenPBR geometry_tangent: which way the anisotropy runs, as a tangent-space vector (red right, green up).";
    Displacement = 45, "displacement", "Displacement", Geometry, "displace",
        "How far each point is moved out: the 3D view raises the model by it as by Height.";
    Bump = 46, "bump", "Bump", Geometry, "bumpmap",
        "A height the 3D view lights the surface by, without moving it.";
    DetailNormal = 47, "detail_normal", "Detail Normal", Geometry, "detailnormal",
        "A second normal map, of finer detail, laid over the Normal.";
    Diffuse = 48, "diffuse", "Diffuse", Other, "diffuse albedo specular glossiness",
        "The diffuse color of the specular-and-glossiness workflow: the 3D view shows it as the base color of a material that is not metal.";
    Specular = 49, "specular", "Specular", Other, "specular f0 reflectance glossiness",
        "The specular color of the specular-and-glossiness workflow: how much and in which color the surface reflects head on.";
    Glossiness = 50, "glossiness", "Glossiness", Other, "glossiness smoothness gloss",
        "How smooth the surface is, from matte (0) to a mirror (1): roughness turned over.";
    SpecularLevel = 51, "specular_level", "Specular Level", Other, "specularlevel reflectance",
        "How much a surface that is not metal reflects: 0.5, as most materials (4% head on).";
    Mask = 52, "mask", "Mask", Other, "mask",
        "A mask a game engine or another program reads; the 3D view does not show it.";
    Ambient = 53, "ambient", "Ambient", Other, "ambient",
        "An ambient map of older engines; the 3D view does not show it.";
    Reflection = 54, "reflection", "Reflection", Other, "reflection",
        "A reflection map of older engines; the 3D view does not show it.";
    Refraction = 55, "refraction", "Refraction", Other, "refraction",
        "A refraction map of older engines; the 3D view does not show it.";
    Panorama = 56, "panorama", "Panorama", Environment, "panorama hdri hdr environment equirectangular latlong latitude longitude light",
        "An environment: a panorama of all around (across, once round; down, from straight up to straight down). The 3D view is lit by it when its Output node or the lighting menu asks for it, as by an environment image.";
}

impl Usage {
    /// The usage of option `index` of the "usage" parameter.
    pub fn of(index: u32) -> Option<Usage> {
        Usage::ALL.get(index as usize).copied()
    }

    /// Its option of the "usage" parameter.
    pub const fn index(self) -> u32 {
        self as u32
    }

    /// Whether the 3D view does something with the map: puts it on the model, or lights the
    /// model with it.
    pub const fn shown_in_3d(self) -> bool {
        !matches!(self, Usage::Custom | Usage::Mask | Usage::Ambient | Usage::Reflection | Usage::Refraction)
    }
}

const fn names() -> [&'static str; Usage::ALL.len()] {
    let mut out = [""; Usage::ALL.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = Usage::ALL[i].name();
        i += 1;
    }
    out
}

const fn ids() -> [&'static str; Usage::ALL.len()] {
    let mut out = [""; Usage::ALL.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = Usage::ALL[i].id();
        i += 1;
    }
    out
}

const fn tips() -> [&'static str; Usage::ALL.len()] {
    let mut out = [""; Usage::ALL.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = Usage::ALL[i].tip();
        i += 1;
    }
    out
}

const fn words() -> [&'static str; Usage::ALL.len()] {
    let mut out = [""; Usage::ALL.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = Usage::ALL[i].words();
        i += 1;
    }
    out
}

/// The options of the "usage" parameter: their names, ids and words, in [`Usage::ALL`]'s order.
pub const USAGE_NAMES: [&str; Usage::ALL.len()] = names();
pub const USAGE_IDS: [&str; Usage::ALL.len()] = ids();
pub const USAGE_WORDS: [&str; Usage::ALL.len()] = words();
pub const USAGE_TIPS: [&str; Usage::ALL.len()] = tips();

/// The options of each group, in the order its menu shows them: OpenPBR's in the order
/// Substance Designer's OpenPBR shader lists its parameters.
pub const USAGE_GROUPS: &[(&str, &[u32])] = &[
    (UsageGroup::Base.name(), &[Usage::BaseWeight as u32, Usage::BaseColor as u32, Usage::Metallic as u32, Usage::BaseDiffuseRoughness as u32]),
    (
        UsageGroup::Specular.name(),
        &[
            Usage::SpecularWeight as u32,
            Usage::SpecularColor as u32,
            Usage::Roughness as u32,
            Usage::SpecularRoughnessAnisotropy as u32,
            Usage::AnisotropyAngle as u32,
            Usage::SpecularIor as u32,
        ],
    ),
    (
        UsageGroup::Transmission.name(),
        &[
            Usage::TransmissionWeight as u32,
            Usage::TransmissionColor as u32,
            Usage::TransmissionDepth as u32,
            Usage::TransmissionScatter as u32,
            Usage::TransmissionScatterAnisotropy as u32,
            Usage::TransmissionDispersionScale as u32,
            Usage::TransmissionDispersionAbbeNumber as u32,
        ],
    ),
    (
        UsageGroup::Subsurface.name(),
        &[
            Usage::SubsurfaceWeight as u32,
            Usage::SubsurfaceColor as u32,
            Usage::SubsurfaceRadius as u32,
            Usage::SubsurfaceRadiusScale as u32,
            Usage::SubsurfaceScatterAnisotropy as u32,
        ],
    ),
    (
        UsageGroup::Coat.name(),
        &[
            Usage::CoatWeight as u32,
            Usage::CoatColor as u32,
            Usage::CoatRoughness as u32,
            Usage::CoatRoughnessAnisotropy as u32,
            Usage::CoatIor as u32,
            Usage::CoatDarkening as u32,
            Usage::CoatNormal as u32,
            Usage::CoatTangent as u32,
        ],
    ),
    (UsageGroup::Fuzz.name(), &[Usage::FuzzWeight as u32, Usage::FuzzColor as u32, Usage::FuzzRoughness as u32]),
    (UsageGroup::Emission.name(), &[Usage::EmissionLuminance as u32, Usage::Emissive as u32]),
    (UsageGroup::ThinFilm.name(), &[Usage::ThinFilmWeight as u32, Usage::ThinFilmThickness as u32, Usage::ThinFilmIor as u32]),
    (
        UsageGroup::Geometry.name(),
        &[
            Usage::Opacity as u32,
            Usage::ThinWalled as u32,
            Usage::Normal as u32,
            Usage::Tangent as u32,
            Usage::AmbientOcclusion as u32,
            Usage::Height as u32,
            Usage::Displacement as u32,
            Usage::Bump as u32,
            Usage::DetailNormal as u32,
        ],
    ),
    (
        UsageGroup::Other.name(),
        &[
            Usage::Diffuse as u32,
            Usage::Specular as u32,
            Usage::Glossiness as u32,
            Usage::SpecularLevel as u32,
            Usage::Mask as u32,
            Usage::Ambient as u32,
            Usage::Reflection as u32,
            Usage::Refraction as u32,
        ],
    ),
    (UsageGroup::Environment.name(), &[Usage::Panorama as u32]),
    (UsageGroup::Custom.name(), &[Usage::Custom as u32]),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every usage is an option, in its place, and in its menu once, under its group; the
    /// ids are those files always had for the first eight.
    #[test]
    fn every_usage_is_in_its_menu_once() {
        for (i, u) in Usage::ALL.iter().enumerate() {
            assert_eq!(u.index() as usize, i, "{u:?}");
            assert_eq!(Usage::of(i as u32), Some(*u));
            assert!(!u.tip().is_empty() && !u.words().is_empty(), "{u:?}");
            let places: Vec<&str> =
                USAGE_GROUPS.iter().filter(|(_, of)| of.contains(&u.index())).map(|(name, _)| *name).collect();
            assert_eq!(places, [u.group().name()], "{u:?}");
        }
        let listed: usize = USAGE_GROUPS.iter().map(|(_, of)| of.len()).sum();
        assert_eq!(listed, Usage::ALL.len());
        assert_eq!(USAGE_GROUPS.len(), UsageGroup::ALL.len());
        assert_eq!(
            USAGE_IDS[..8],
            ["custom", "base_color", "normal", "height", "roughness", "metallic", "ambient_occlusion", "emissive"],
            "what files have always said"
        );
        assert_eq!(Usage::of(Usage::ALL.len() as u32), None);
    }
}
