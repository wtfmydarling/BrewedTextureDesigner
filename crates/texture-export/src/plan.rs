//! The presets, and the files each writes of the maps a program has.

use crate::template::{Normals, Template, plan_template};

/// What a map is, of what the presets read. (A program says what its maps are as it names
/// them; a map of no usage here is written by [`Preset::Maps`] alone.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Usage {
    BaseColor,
    Normal,
    Height,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Emissive,
    Opacity,
    /// Of the models before PBR: what stands for the base color, the roughness turned over,
    /// and the height.
    Diffuse,
    Glossiness,
    Displacement,
}

impl Usage {
    /// What it is called in a sentence: "no roughness".
    pub fn word(self) -> &'static str {
        match self {
            Usage::BaseColor => "base color",
            Usage::Normal => "normal map",
            Usage::Height => "height",
            Usage::Roughness => "roughness",
            Usage::Metallic => "metallic",
            Usage::AmbientOcclusion => "occlusion",
            Usage::Emissive => "emissive",
            Usage::Opacity => "opacity",
            Usage::Diffuse => "diffuse",
            Usage::Glossiness => "glossiness",
            Usage::Displacement => "displacement",
        }
    }

    /// Whether the map is gray: one channel of it is written.
    pub(crate) fn gray(self) -> bool {
        matches!(self, Usage::Height | Usage::AmbientOcclusion | Usage::Roughness | Usage::Metallic)
    }
}

/// A map a program has: its reference of its own to it (what [`Source`] and [`Planned`] give
/// back), its name, and what it is, where it is one of what the presets read.
#[derive(Clone, Debug, PartialEq)]
pub struct Map<R> {
    pub reference: R,
    pub name: String,
    pub usage: Option<Usage>,
}

/// What an export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    /// Each map, as the program has it (its name, channels and format).
    Maps,
    /// Unreal Engine: base color, a normal map of DirectX (Y−), and occlusion, roughness and
    /// metallic packed in red, green and blue (ORM).
    Unreal,
    /// Unity's Universal pipeline (URP Lit): base map, a normal map of OpenGL (Y+), metallic
    /// with smoothness (1 − roughness) in alpha, and occlusion.
    UnityUrp,
    /// Unity's High Definition pipeline (HDRP Lit): base map, normal map, and the mask map:
    /// metallic, occlusion, detail mask and smoothness.
    UnityHdrp,
    /// glTF 2.0: base color, normal map, and occlusion, roughness and metallic in red, green
    /// and blue (occlusionTexture and metallicRoughnessTexture read the one file).
    Gltf,
}

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Maps, Preset::Unreal, Preset::UnityUrp, Preset::UnityHdrp, Preset::Gltf];

    /// The template it is (all but the maps as they are): its own in `templates/`.
    pub fn template(self) -> Option<&'static Template> {
        static BUILT_IN: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
        let all = BUILT_IN.get_or_init(|| {
            [
                include_str!("../templates/unreal.json"),
                include_str!("../templates/unity-urp.json"),
                include_str!("../templates/unity-hdrp.json"),
                include_str!("../templates/gltf.json"),
            ]
            .into_iter()
            .map(|text| crate::template_of(text).expect("a template of the crate's own"))
            .collect()
        });
        match self {
            Preset::Maps => None,
            Preset::Unreal => all.first(),
            Preset::UnityUrp => all.get(1),
            Preset::UnityHdrp => all.get(2),
            Preset::Gltf => all.get(3),
        }
    }

    pub fn name(self) -> &'static str {
        self.template().map_or("Outputs as They Are", |t| t.name.as_str())
    }

    pub fn about(self) -> &'static str {
        self.template().map_or("Each Output, named and in the channels and format its node says.", |t| t.about.as_str())
    }

    /// The normal maps it writes: DirectX's (Y−), or OpenGL's (Y+). Unreal Engine reads only
    /// DirectX's (it has since 4.x), whatever the program makes.
    pub fn directx(self) -> bool {
        self.template().is_some_and(|t| t.normals == Normals::DirectX)
    }
}

/// What one channel of a file packed of several is.
#[derive(Clone, Debug, PartialEq)]
pub enum Source<R> {
    /// A channel of a map (0 red … 3 alpha; a grayscale map is the same in the first three),
    /// turned over (1 − it) where `invert`.
    Map { map: R, channel: u8, invert: bool },
    /// A value, where the program has no map of what it would be.
    Value(f32),
}

/// A file an export writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Planned<R> {
    /// A map, as the program has it (its name, channels and format: the maps as they are).
    AsItIs(R),
    /// Channels of maps, or values: gray of one; red, green and blue of three; and alpha of
    /// four. What a preset writes, in the channels and format it says.
    Pack(Vec<Source<R>>),
}

/// A file an export writes: its name (without extension), what it is made of, and what that
/// is, said.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedFile<R> {
    pub name: String,
    pub what: Planned<R>,
    pub said: String,
}

/// The files `preset` writes of `maps` (of what is called `name`, a graph or a set of
/// textures), the program's normal maps being DirectX's where `directx`. Of several maps of a
/// usage, the first is taken: a program gives its maps in the order it would have them taken.
pub fn plan<R: Copy>(maps: &[Map<R>], name: &str, preset: Preset, directx: bool) -> Vec<PlannedFile<R>> {
    match preset.template() {
        Some(template) => plan_template(template, maps, name, directx),
        None => maps.iter().map(|m| PlannedFile { name: m.name.clone(), what: Planned::AsItIs(m.reference), said: m.name.clone() }).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(reference: u32, name: &str, usage: Option<Usage>) -> Map<u32> {
        Map { reference, name: name.into(), usage }
    }

    /// The maps of a material of OpenPBR, in the order a program has them.
    fn pbr() -> Vec<Map<u32>> {
        vec![
            map(1, "base_color", Some(Usage::BaseColor)),
            map(2, "base_metalness", Some(Usage::Metallic)),
            map(3, "specular_roughness", Some(Usage::Roughness)),
            map(4, "height", Some(Usage::Height)),
            map(5, "normal", Some(Usage::Normal)),
            map(6, "ambient_occlusion", Some(Usage::AmbientOcclusion)),
            map(7, "mask", None),
        ]
    }

    /// Each preset writes the files its engine expects of a material, named as it names them,
    /// the maps it packs where it packs them (a value where the program has no such map), green
    /// turned over where the program's normal maps are the other way.
    #[test]
    fn presets_make_the_files_engines_expect() {
        let maps = pbr();
        let names = |preset: Preset| plan(&maps, "Rusty Metal", preset, false).into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(Preset::Maps), ["base_color", "base_metalness", "specular_roughness", "height", "normal", "ambient_occlusion", "mask"]);
        assert_eq!(names(Preset::Unreal), ["T_Rusty_Metal_BaseColor", "T_Rusty_Metal_Normal", "T_Rusty_Metal_ORM", "T_Rusty_Metal_Height"]);
        assert_eq!(
            names(Preset::UnityUrp),
            ["Rusty_Metal_BaseMap", "Rusty_Metal_Normal", "Rusty_Metal_MetallicSmoothness", "Rusty_Metal_Occlusion", "Rusty_Metal_Height"]
        );
        assert_eq!(names(Preset::UnityHdrp), ["Rusty_Metal_BaseMap", "Rusty_Metal_Normal", "Rusty_Metal_MaskMap", "Rusty_Metal_Height"]);
        assert_eq!(names(Preset::Gltf), ["Rusty_Metal_baseColor", "Rusty_Metal_normal", "Rusty_Metal_occlusionRoughnessMetallic"]);
        let unreal = plan(&maps, "Rusty Metal", Preset::Unreal, false);
        // OpenGL normals for Unreal: green turned over; DirectX ones as they are.
        assert_eq!(unreal[1].what, Planned::Pack((0..3).map(|channel| Source::Map { map: 5, channel, invert: channel == 1 }).collect()));
        let directx = plan(&maps, "Rusty Metal", Preset::Unreal, true);
        assert_eq!(directx[1].what, Planned::Pack((0..3).map(|channel| Source::Map { map: 5, channel, invert: false }).collect()));
        let orm = Planned::Pack(vec![
            Source::Map { map: 6, channel: 0, invert: false },
            Source::Map { map: 3, channel: 0, invert: false },
            Source::Map { map: 2, channel: 0, invert: false },
        ]);
        assert_eq!(unreal[2].what, orm);
        assert_eq!(unreal[3].what, Planned::Pack(vec![Source::Map { map: 4, channel: 0, invert: false }]), "gray: one channel");
        let hdrp = plan(&maps, "Rusty Metal", Preset::UnityHdrp, false);
        let Planned::Pack(mask) = &hdrp[2].what else { panic!("{:?}", hdrp[2]) };
        assert_eq!(mask[2], Source::Value(1.0));
        assert_eq!(mask[3], Source::Map { map: 3, channel: 0, invert: true }, "smoothness: 1 − roughness");
        assert!(plan(&[map(7, "mask", None)], "x", Preset::Unreal, false).is_empty(), "nothing to base on, nothing packed of nothing");
    }

    /// Maps of the specular-and-glossiness workflow, a displacement and an opacity are exported as
    /// what they stand for: the diffuse color the base color, the opacity its alpha, the
    /// glossiness turned over the roughness (and the smoothness as it is), the displacement the
    /// height. An opacity alone has OpenPBR's base color under it.
    #[test]
    fn maps_that_stand_for_others_are_exported_as_them() {
        let maps = [
            map(1, "diffuse", Some(Usage::Diffuse)),
            map(2, "glossiness", Some(Usage::Glossiness)),
            map(3, "displacement", Some(Usage::Displacement)),
            map(4, "opacity", Some(Usage::Opacity)),
        ];
        let unreal = plan(&maps, "Leather", Preset::Unreal, false);
        assert_eq!(unreal.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["T_Leather_BaseColor", "T_Leather_ORM", "T_Leather_Height"]);
        let source = |map, channel, invert| Source::Map { map, channel, invert };
        assert_eq!(unreal[0].what, Planned::Pack(vec![source(1, 0, false), source(1, 1, false), source(1, 2, false), source(4, 0, false)]));
        let Planned::Pack(orm) = &unreal[1].what else { panic!() };
        assert_eq!(orm[1], source(2, 0, true), "roughness: 1 − glossiness");
        assert_eq!(unreal[2].what, Planned::Pack(vec![source(3, 0, false)]));
        let urp = plan(&maps, "Leather", Preset::UnityUrp, false);
        let Planned::Pack(metal) = &urp[1].what else { panic!() };
        assert_eq!(metal[3], source(2, 0, false), "smoothness: the glossiness as it is");
        assert_eq!(metal[0], Source::Value(0.0), "no metal");
        let gltf = plan(&maps[3..], "Leaf", Preset::Gltf, false);
        assert_eq!(gltf.len(), 1);
        assert_eq!(gltf[0].what, Planned::Pack(vec![Source::Value(0.8), Source::Value(0.8), Source::Value(0.8), source(4, 0, false)]));
    }
}
