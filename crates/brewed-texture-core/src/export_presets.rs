//! What an export writes of a graph (File → Export…): its Outputs as they are, or the files a
//! game engine or a format expects of a material, made of the Outputs by their usages (Base
//! Color, Normal, Roughness…), some packed into the channels of one file, as Substance
//! Designer's export presets do.

use crate::graph::{Graph, NodeId};
use crate::nodes::{NodeKind, USAGE_AO, USAGE_BASE_COLOR, USAGE_EMISSIVE, USAGE_HEIGHT, USAGE_METALLIC, USAGE_NORMAL, USAGE_ROUGHNESS};

/// What an export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    /// Each Output, as its node says (its name, channels and format).
    Outputs,
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
    pub const ALL: [Preset; 5] = [Preset::Outputs, Preset::Unreal, Preset::UnityUrp, Preset::UnityHdrp, Preset::Gltf];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Outputs => "Outputs as They Are",
            Preset::Unreal => "Unreal Engine",
            Preset::UnityUrp => "Unity (URP)",
            Preset::UnityHdrp => "Unity (HDRP)",
            Preset::Gltf => "glTF 2.0",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Preset::Outputs => "Each Output, named and in the channels and format its node says.",
            Preset::Unreal => {
                "Base color, a normal map of DirectX (Y−), and occlusion, roughness and metallic packed in red, green \
                 and blue (ORM), named T_<graph>_…; height and emissive where the graph has them."
            }
            Preset::UnityUrp => {
                "Base map, a normal map of OpenGL (Y+), metallic with smoothness (1 − roughness) in alpha, and \
                 occlusion; height and emission where the graph has them."
            }
            Preset::UnityHdrp => {
                "Base map, a normal map of OpenGL (Y+), and the mask map: metallic, occlusion, detail mask and \
                 smoothness (1 − roughness); height and emission where the graph has them."
            }
            Preset::Gltf => {
                "Base color, a normal map of OpenGL (Y+), and occlusion, roughness and metallic in red, green and \
                 blue, which glTF's occlusion and metallic-roughness textures both read; emissive where it is."
            }
        }
    }

    /// The normal maps it writes: DirectX's (Y−), or OpenGL's (Y+). Unreal Engine reads only
    /// DirectX's (it has since 4.x), whatever the graph makes.
    pub fn directx(self) -> bool {
        self == Preset::Unreal
    }
}

/// What one channel of a file packed of several is.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A channel of an Output's image (0 red … 3 alpha; a grayscale image is the same in the
    /// first three), turned over (1 − it) where `invert`.
    Output { node: NodeId, channel: u8, invert: bool },
    /// A value, where the graph has no Output of what it would be.
    Value(f32),
}

/// A file an export writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Planned {
    /// An Output, as its node says (its name, channels and format: the Outputs as they are).
    Output(NodeId),
    /// Channels of what arrives at Outputs, or values: gray of one; red, green and blue of
    /// three; and alpha of four. What a preset writes, in the channels and format it says.
    Pack(Vec<Source>),
}

/// A file an export writes: its name (without extension), what it is made of, and what that
/// is, said.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedFile {
    pub name: String,
    pub what: Planned,
    pub said: String,
}

/// `name` as the beginning of the name of a file: what a file name cannot have, and blanks,
/// as `_`.
fn file_stem(name: &str) -> String {
    let stem: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if stem.is_empty() { "graph".into() } else { stem }
}

/// What a packed channel of a usage is where the graph has no Output of it (no occlusion, half
/// rough, not metal), and the usage, said.
fn unmade(usage: u32) -> (f32, &'static str) {
    match usage {
        USAGE_AO => (1.0, "occlusion"),
        USAGE_ROUGHNESS => (0.5, "roughness"),
        _ => (0.0, "metallic"),
    }
}

/// Whether the normal maps of `graph` are DirectX's (Y−): as its Normal nodes make them (the
/// first says), OpenGL's where it has none.
pub fn graph_normals_directx(graph: &Graph) -> bool {
    graph.nodes_of(NodeKind::NORMAL).next().is_some_and(|n| n.enum_value("format") == 1)
}

/// The files `preset` writes of `graph` (called `name`), its normal maps being DirectX's
/// where `directx`.
pub fn plan(graph: &Graph, name: &str, preset: Preset, directx: bool) -> Vec<PlannedFile> {
    let outputs: Vec<&crate::graph::Node> = crate::instance::port_nodes(graph);
    if preset == Preset::Outputs {
        return outputs
            .iter()
            .map(|n| PlannedFile { name: n.text("name").to_string(), what: Planned::Output(n.id), said: n.text("name").to_string() })
            .collect();
    }
    // The Output of a usage (the upper one, where there are several).
    let of = |usage: u32| outputs.iter().find(|n| n.enum_value("usage") == usage).copied();
    let label = |usage: u32| of(usage).map_or_else(String::new, |n| n.text("name").to_string());
    let channel = |usage: u32, channel: u8, invert: bool| match of(usage) {
        Some(n) => Source::Output { node: n.id, channel, invert },
        None => Source::Value(unmade(usage).0),
    };
    // A packed channel, said: the Output's name (1 − it, turned over), or the value written where
    // the graph has no Output of it.
    let told = |usage: u32, invert: bool| match of(usage) {
        Some(n) => format!("{}{}", if invert { "1 − " } else { "" }, n.text("name")),
        None => format!("{} (no {})", unmade(usage).0, unmade(usage).1),
    };
    // A map as it is: its red, green and blue, or its gray.
    let whole = |usage: u32| {
        of(usage).map(|n| {
            let gray = [USAGE_HEIGHT, USAGE_AO, USAGE_ROUGHNESS, USAGE_METALLIC].contains(&usage);
            Planned::Pack((0..if gray { 1 } else { 3 }).map(|channel| Source::Output { node: n.id, channel, invert: false }).collect())
        })
    };
    let base = file_stem(name);
    let prefix = if preset == Preset::Unreal { format!("T_{base}_") } else { format!("{base}_") };
    let mut files = Vec::new();
    let mut add = |suffix: &str, what: Option<Planned>, said: String| {
        if let Some(what) = what {
            files.push(PlannedFile { name: format!("{prefix}{suffix}"), what, said });
        }
    };
    // Base color: as it is.
    add(
        match preset {
            Preset::UnityUrp | Preset::UnityHdrp => "BaseMap",
            Preset::Gltf => "baseColor",
            _ => "BaseColor",
        },
        whole(USAGE_BASE_COLOR),
        label(USAGE_BASE_COLOR),
    );
    // Normal: green turned over where the graph's are not what the preset writes.
    if let Some(normal) = of(USAGE_NORMAL) {
        let flip = directx != preset.directx();
        let what = Planned::Pack((0..3).map(|channel| Source::Output { node: normal.id, channel, invert: flip && channel == 1 }).collect());
        let said = format!("{}{}", normal.text("name"), if flip { " (green turned over)" } else { "" });
        add(if preset == Preset::Gltf { "normal" } else { "Normal" }, Some(what), said);
    }
    // What is packed: occlusion, roughness, metallic, as each preset has them (where the graph
    // has any of them).
    let any = [USAGE_AO, USAGE_ROUGHNESS, USAGE_METALLIC].into_iter().any(|u| of(u).is_some());
    match preset {
        _ if !any => {}
        Preset::Unreal | Preset::Gltf => {
            let pack = vec![channel(USAGE_AO, 0, false), channel(USAGE_ROUGHNESS, 0, false), channel(USAGE_METALLIC, 0, false)];
            let said = format!("R {} · G {} · B {}", told(USAGE_AO, false), told(USAGE_ROUGHNESS, false), told(USAGE_METALLIC, false));
            add(if preset == Preset::Unreal { "ORM" } else { "occlusionRoughnessMetallic" }, Some(Planned::Pack(pack)), said);
        }
        Preset::UnityUrp => {
            // Smoothness is 1 − roughness.
            let pack = vec![
                channel(USAGE_METALLIC, 0, false),
                channel(USAGE_METALLIC, 0, false),
                channel(USAGE_METALLIC, 0, false),
                channel(USAGE_ROUGHNESS, 0, true),
            ];
            add("MetallicSmoothness", Some(Planned::Pack(pack)), format!("RGB {} · A {}", told(USAGE_METALLIC, false), told(USAGE_ROUGHNESS, true)));
            add("Occlusion", whole(USAGE_AO), label(USAGE_AO));
        }
        Preset::UnityHdrp => {
            let pack = vec![
                channel(USAGE_METALLIC, 0, false),
                channel(USAGE_AO, 0, false),
                // The detail mask: all of it.
                Source::Value(1.0),
                channel(USAGE_ROUGHNESS, 0, true),
            ];
            let said = format!("R {} · G {} · B 1 (detail mask) · A {}", told(USAGE_METALLIC, false), told(USAGE_AO, false), told(USAGE_ROUGHNESS, true));
            add("MaskMap", Some(Planned::Pack(pack)), said);
        }
        Preset::Outputs => {}
    }
    add(
        match preset {
            Preset::UnityUrp | Preset::UnityHdrp => "Height",
            Preset::Gltf => "height",
            _ => "Height",
        },
        if preset == Preset::Gltf { None } else { whole(USAGE_HEIGHT) },
        label(USAGE_HEIGHT),
    );
    add(
        match preset {
            Preset::UnityUrp | Preset::UnityHdrp => "Emission",
            Preset::Gltf => "emissive",
            _ => "Emissive",
        },
        whole(USAGE_EMISSIVE),
        label(USAGE_EMISSIVE),
    );
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::Template;

    /// Each preset writes the files its engine expects of a material, named as it names them,
    /// the maps it packs where it packs them (a value where the graph has no such map), green
    /// turned over where the graph's normal maps are the other way.
    #[test]
    fn presets_make_the_files_engines_expect() {
        let g = Template::Pbr.graph([512, 512], crate::Precision::Half);
        let names = |preset: Preset, directx: bool| plan(&g, "Rusty Metal", preset, directx).into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(
            names(Preset::Outputs, false),
            ["basecolor", "normal", "roughness", "metallic", "height", "ambientocclusion"]
        );
        assert_eq!(names(Preset::Unreal, false), ["T_Rusty_Metal_BaseColor", "T_Rusty_Metal_Normal", "T_Rusty_Metal_ORM", "T_Rusty_Metal_Height"]);
        assert_eq!(
            names(Preset::UnityUrp, false),
            ["Rusty_Metal_BaseMap", "Rusty_Metal_Normal", "Rusty_Metal_MetallicSmoothness", "Rusty_Metal_Occlusion", "Rusty_Metal_Height"]
        );
        assert_eq!(names(Preset::UnityHdrp, false), ["Rusty_Metal_BaseMap", "Rusty_Metal_Normal", "Rusty_Metal_MaskMap", "Rusty_Metal_Height"]);
        assert_eq!(names(Preset::Gltf, false), ["Rusty_Metal_baseColor", "Rusty_Metal_normal", "Rusty_Metal_occlusionRoughnessMetallic"]);
        let unreal = plan(&g, "Rusty Metal", Preset::Unreal, false);
        // OpenGL normals for Unreal: green turned over.
        let Planned::Pack(normal) = &unreal[1].what else { panic!("{:?}", unreal[1]) };
        assert!(matches!(normal[1], Source::Output { channel: 1, invert: true, .. }));
        let Planned::Pack(directx) = &plan(&g, "Rusty Metal", Preset::Unreal, true)[1].what else { panic!() };
        assert!(directx.iter().all(|s| matches!(s, Source::Output { invert: false, .. })), "DirectX already");
        // Maps as they are: of three channels, of one where they are gray.
        let Planned::Pack(base) = &unreal[0].what else { panic!() };
        assert_eq!(base.len(), 3);
        let Planned::Pack(height) = &unreal[3].what else { panic!() };
        assert_eq!(height.len(), 1);
        let Planned::Pack(orm) = &unreal[2].what else { panic!() };
        let usage = |source: &Source| match source {
            Source::Output { node, .. } => g.node(*node).map(|n| n.text("name").to_string()),
            Source::Value(_) => None,
        };
        assert_eq!(orm.iter().map(usage).collect::<Vec<_>>(), [Some("ambientocclusion".into()), Some("roughness".into()), Some("metallic".into())]);
        let hdrp = plan(&g, "Rusty Metal", Preset::UnityHdrp, false);
        let Planned::Pack(mask) = &hdrp[2].what else { panic!() };
        assert_eq!(mask[2], Source::Value(1.0));
        assert!(matches!(mask[3], Source::Output { invert: true, .. }), "smoothness: 1 − roughness");
        // No such maps: values.
        let empty = Template::Empty.graph([512, 512], crate::Precision::Half);
        assert!(plan(&empty, "x", Preset::Unreal, false).is_empty(), "nothing to base on, nothing packed of nothing");
    }
}
