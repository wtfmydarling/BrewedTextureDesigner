//! The presets, and the files each writes of the maps a program has.

/// What a map is, of what the presets read. (A program says what its maps are as it names
/// them; a map of no usage here is written by [`Preset::Maps`] alone.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Whether the map is gray: one channel of it is written.
    fn gray(self) -> bool {
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

    pub fn name(self) -> &'static str {
        match self {
            Preset::Maps => "Outputs as They Are",
            Preset::Unreal => "Unreal Engine",
            Preset::UnityUrp => "Unity (URP)",
            Preset::UnityHdrp => "Unity (HDRP)",
            Preset::Gltf => "glTF 2.0",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Preset::Maps => "Each Output, named and in the channels and format its node says.",
            Preset::Unreal => {
                "Base color (its alpha the opacity, where the graph has one), a normal map of DirectX (Y−), and occlusion, roughness and metallic packed \
                 in red, green and blue (ORM), named T_<graph>_…; height and emissive where the graph has them."
            }
            Preset::UnityUrp => {
                "Base map (its alpha the opacity, where the graph has one), a normal map of OpenGL (Y+), metallic with smoothness (1 − roughness) in \
                 alpha, and occlusion; height and emission where the graph has them."
            }
            Preset::UnityHdrp => {
                "Base map (its alpha the opacity, where the graph has one), a normal map of OpenGL (Y+), and the mask map: metallic, occlusion, detail \
                 mask and smoothness (1 − roughness); height and emission where the graph has them."
            }
            Preset::Gltf => {
                "Base color (its alpha the opacity, where the graph has one), a normal map of OpenGL (Y+), and occlusion, roughness and metallic in \
                 red, green and blue, which glTF's occlusion and metallic-roughness textures both read; emissive where it is."
            }
        }
    }

    /// The normal maps it writes: DirectX's (Y−), or OpenGL's (Y+). Unreal Engine reads only
    /// DirectX's (it has since 4.x), whatever the program makes.
    pub fn directx(self) -> bool {
        self == Preset::Unreal
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

/// What a packed channel of a usage is where the program has no map of it (no occlusion, half
/// rough, not metal), and the usage, said.
fn unmade(usage: Usage) -> (f32, &'static str) {
    match usage {
        Usage::AmbientOcclusion => (1.0, "occlusion"),
        Usage::Roughness => (0.5, "roughness"),
        _ => (0.0, "metallic"),
    }
}

/// The files `preset` writes of `maps` (of what is called `name`, a graph or a set of
/// textures), the program's normal maps being DirectX's where `directx`. Of several maps of a
/// usage, the first is taken: a program gives its maps in the order it would have them taken.
pub fn plan<R: Copy>(maps: &[Map<R>], name: &str, preset: Preset, directx: bool) -> Vec<PlannedFile<R>> {
    if preset == Preset::Maps {
        return maps.iter().map(|m| PlannedFile { name: m.name.clone(), what: Planned::AsItIs(m.reference), said: m.name.clone() }).collect();
    }
    // The map of a usage, or where the program has none, of one that stands for it, and whether
    // it is turned over to be it.
    let of = |usage: Usage| -> Option<(&Map<R>, bool)> {
        let find = |u: Usage| maps.iter().find(|m| m.usage == Some(u));
        find(usage).map(|m| (m, false)).or_else(|| match usage {
            Usage::BaseColor => find(Usage::Diffuse).map(|m| (m, false)),
            Usage::Roughness => find(Usage::Glossiness).map(|m| (m, true)),
            Usage::Height => find(Usage::Displacement).map(|m| (m, false)),
            _ => None,
        })
    };
    let label = |usage: Usage| of(usage).map_or_else(String::new, |(m, over)| format!("{}{}", if over { "1 − " } else { "" }, m.name));
    let channel = |usage: Usage, channel: u8, invert: bool| match of(usage) {
        Some((m, over)) => Source::Map { map: m.reference, channel, invert: invert != over },
        None => Source::Value(unmade(usage).0),
    };
    // A packed channel, said: the map's name (1 − it, turned over), or the value written where
    // the program has no map of it.
    let told = |usage: Usage, invert: bool| match of(usage) {
        Some((m, over)) => format!("{}{}", if invert != over { "1 − " } else { "" }, m.name),
        None => format!("{} (no {})", unmade(usage).0, unmade(usage).1),
    };
    // A map as it is: its red, green and blue, or its gray.
    let whole = |usage: Usage| {
        of(usage).map(|(m, over)| {
            let channels = if usage.gray() { 1 } else { 3 };
            Planned::Pack((0..channels).map(|channel| Source::Map { map: m.reference, channel, invert: over }).collect())
        })
    };
    let opacity = maps.iter().find(|m| m.usage == Some(Usage::Opacity));
    let base = crate::file_stem(name);
    let prefix = if preset == Preset::Unreal { format!("T_{base}_") } else { format!("{base}_") };
    let mut files = Vec::new();
    let mut add = |suffix: &str, what: Option<Planned<R>>, said: String| {
        if let Some(what) = what {
            files.push(PlannedFile { name: format!("{prefix}{suffix}"), what, said });
        }
    };
    // Base color: as it is, the opacity its alpha where the program has one (OpenPBR's base
    // color, 0.8, where it has only that).
    let (color, said) = match (whole(Usage::BaseColor), opacity) {
        (Some(Planned::Pack(mut rgb)), Some(alpha)) => {
            rgb.push(Source::Map { map: alpha.reference, channel: 0, invert: false });
            (Some(Planned::Pack(rgb)), format!("RGB {} · A {}", label(Usage::BaseColor), alpha.name))
        }
        (None, Some(alpha)) => (
            Some(Planned::Pack(vec![
                Source::Value(0.8),
                Source::Value(0.8),
                Source::Value(0.8),
                Source::Map { map: alpha.reference, channel: 0, invert: false },
            ])),
            format!("RGB 0.8 (no base color) · A {}", alpha.name),
        ),
        (color, _) => (color, label(Usage::BaseColor)),
    };
    add(
        match preset {
            Preset::UnityUrp | Preset::UnityHdrp => "BaseMap",
            Preset::Gltf => "baseColor",
            _ => "BaseColor",
        },
        color,
        said,
    );
    // Normal: green turned over where the program's are not what the preset writes.
    if let Some((normal, _)) = of(Usage::Normal) {
        let flip = directx != preset.directx();
        let what = Planned::Pack((0..3).map(|channel| Source::Map { map: normal.reference, channel, invert: flip && channel == 1 }).collect());
        let said = format!("{}{}", normal.name, if flip { " (green turned over)" } else { "" });
        add(if preset == Preset::Gltf { "normal" } else { "Normal" }, Some(what), said);
    }
    // What is packed: occlusion, roughness, metallic, as each preset has them (where the program
    // has any of them).
    let any = [Usage::AmbientOcclusion, Usage::Roughness, Usage::Metallic].into_iter().any(|u| of(u).is_some());
    match preset {
        _ if !any => {}
        Preset::Unreal | Preset::Gltf => {
            let pack = vec![channel(Usage::AmbientOcclusion, 0, false), channel(Usage::Roughness, 0, false), channel(Usage::Metallic, 0, false)];
            let said = format!("R {} · G {} · B {}", told(Usage::AmbientOcclusion, false), told(Usage::Roughness, false), told(Usage::Metallic, false));
            add(if preset == Preset::Unreal { "ORM" } else { "occlusionRoughnessMetallic" }, Some(Planned::Pack(pack)), said);
        }
        Preset::UnityUrp => {
            // Smoothness is 1 − roughness.
            let pack = vec![
                channel(Usage::Metallic, 0, false),
                channel(Usage::Metallic, 0, false),
                channel(Usage::Metallic, 0, false),
                channel(Usage::Roughness, 0, true),
            ];
            add("MetallicSmoothness", Some(Planned::Pack(pack)), format!("RGB {} · A {}", told(Usage::Metallic, false), told(Usage::Roughness, true)));
            add("Occlusion", whole(Usage::AmbientOcclusion), label(Usage::AmbientOcclusion));
        }
        Preset::UnityHdrp => {
            let pack = vec![
                channel(Usage::Metallic, 0, false),
                channel(Usage::AmbientOcclusion, 0, false),
                // The detail mask: all of it.
                Source::Value(1.0),
                channel(Usage::Roughness, 0, true),
            ];
            let said = format!(
                "R {} · G {} · B 1 (detail mask) · A {}",
                told(Usage::Metallic, false),
                told(Usage::AmbientOcclusion, false),
                told(Usage::Roughness, true)
            );
            add("MaskMap", Some(Planned::Pack(pack)), said);
        }
        Preset::Maps => {}
    }
    add(if preset == Preset::Gltf { "height" } else { "Height" }, if preset == Preset::Gltf { None } else { whole(Usage::Height) }, label(Usage::Height));
    add(
        match preset {
            Preset::UnityUrp | Preset::UnityHdrp => "Emission",
            Preset::Gltf => "emissive",
            _ => "Emissive",
        },
        whole(Usage::Emissive),
        label(Usage::Emissive),
    );
    files
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
