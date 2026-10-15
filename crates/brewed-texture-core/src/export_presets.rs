//! What an export writes of a graph (File → Export…): its Outputs as they are, or the files a
//! game engine or a format expects of a material, made of the Outputs by their usages (Base
//! Color, Normal, Roughness…), some packed into the channels of one file. How a template is
//! written and what it writes is the `texture-export` crate's, which the programs of the set
//! share; this is what is this program's: its presets (in `export-templates/`), the parts of a
//! file's name it fills in (`$(graph)`, `$(package)`), and which maps a graph has: its Outputs,
//! in the order a node of the graph has them, each with its usage.

use crate::graph::{Graph, NodeId};
use crate::instance::GraphId;
use crate::nodes::{NodeKind, Usage};
use texture_export::naming::Token;
use texture_export::{FileFormat, Normals, Size, Template};

/// What the export window keeps with a package (its Save settings, see `docs/package-format.md`):
/// what the package says of its own for all its graphs, where not as the global settings say
/// (where the files go, what they are written with, as what and how large), and of each graph
/// whether it is exported and what it says of its own.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExportSettings {
    /// The folder the files go to (relative to the package's file where it can be; none: as the
    /// global settings say).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// The template, by its name: of a preset of the program's (`Raw Outputs`: each Output as it
    /// is), or of one of the user's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<FileFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graphs: Vec<GraphExportSettings>,
}

/// What a graph's export says of its own: whether it is exported, and where it is not as the
/// package's settings say, its folder, template, format, size, and the way its normal maps point.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphExportSettings {
    pub graph: GraphId,
    #[serde(default)]
    pub checked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<FileFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Normals>,
}

/// The parts of a file's name an export of a graph fills in.
pub const TOKENS: [Token; 2] = [
    Token { key: "graph", about: "The graph's name, as a file's (blanks and what a file name cannot have as _)" },
    Token { key: "package", about: "The package's name, as a file's" },
];

/// What an export writes, of the program's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    /// Each Output, as its node says (its name, channels and format): Raw Outputs.
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

/// What exports each Output as it is, as an Output says: its name, channels and format.
pub const RAW_OUTPUTS: &str = "Raw Outputs";

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Maps, Preset::Unreal, Preset::UnityUrp, Preset::UnityHdrp, Preset::Gltf];

    /// The template it is (all but the Outputs as they are): its own in `export-templates/`.
    pub fn template(self) -> Option<&'static Template> {
        static BUILT_IN: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
        let all = BUILT_IN.get_or_init(|| {
            [
                include_str!("../export-templates/unreal.json"),
                include_str!("../export-templates/unity-urp.json"),
                include_str!("../export-templates/unity-hdrp.json"),
                include_str!("../export-templates/gltf.json"),
            ]
            .into_iter()
            .map(|text| texture_export::template_of(text).expect("a template of the program's own"))
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

    /// What it is called in a file's name (the user's changes of it are kept as `<id>.json`).
    pub fn id(self) -> &'static str {
        match self {
            Preset::Maps => "raw",
            Preset::Unreal => "unreal",
            Preset::UnityUrp => "unity-urp",
            Preset::UnityHdrp => "unity-hdrp",
            Preset::Gltf => "gltf",
        }
    }

    pub fn name(self) -> &'static str {
        self.template().map_or(RAW_OUTPUTS, |t| t.name.as_str())
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

/// A channel of a file packed of several, of a graph: of an Output's image, or a value.
pub type Source = texture_export::Source<NodeId>;
/// A file an export writes of a graph.
pub type Planned = texture_export::Planned<NodeId>;
pub type PlannedFile = texture_export::PlannedFile<NodeId>;

/// What an Output's map is to an export: its usage's id; of an Output of no usage, its name
/// (`custom:` and the name).
fn map_of(node: &crate::graph::Node) -> texture_export::MapId {
    let index = node.enum_value("usage");
    match Usage::ALL.iter().find(|u| u.index() == index) {
        Some(Usage::Custom) | None => texture_export::MapId::custom(node.text("name")),
        Some(usage) => texture_export::MapId::from(usage.id()),
    }
}

/// The maps of `graph` to an export: its Outputs, the upper first (as a node of the graph has
/// them), each by its name and what it is.
pub fn maps(graph: &Graph) -> Vec<texture_export::Map<NodeId>> {
    crate::instance::port_nodes(graph)
        .into_iter()
        .map(|n| texture_export::Map { reference: n.id, name: n.text("name").to_string(), usage: Some(map_of(n)) })
        .collect()
}

/// The maps a template may be made of, as an Output's usage offers them, each with its name and
/// its group (OpenPBR's parts: Base, Specular…), by their groups; of no usage, the
/// maps are the graph's own (see [`maps`]).
pub fn input_maps() -> Vec<(texture_export::MapId, &'static str, &'static str)> {
    let mut usages: Vec<Usage> = Usage::ALL.into_iter().filter(|u| !matches!(u, Usage::Custom | Usage::Panorama)).collect();
    // (By their groups, in the order of the groups; within one, in the order of the usages.)
    usages.sort_by_key(|u| crate::nodes::UsageGroup::ALL.iter().position(|g| *g == u.group()));
    usages.into_iter().map(|u| (texture_export::MapId::from(u.id()), u.name(), u.group().name())).collect()
}

/// Whether the normal maps of `graph` are DirectX's (Y−): as its Normal nodes make them (the
/// first says), OpenGL's where it has none.
pub fn graph_normals_directx(graph: &Graph) -> bool {
    graph.nodes_of(NodeKind::NORMAL).next().is_some_and(|n| n.enum_value("format") == 1)
}

/// The files `preset` writes of `graph` (called `name`), its normal maps being DirectX's
/// where `directx`. Of several Outputs of a usage, the upper is taken.
pub fn plan(graph: &Graph, name: &str, preset: Preset, directx: bool) -> Vec<PlannedFile> {
    let maps = maps(graph);
    match preset.template() {
        Some(template) => texture_export::plan_template(template, &maps, &[("graph", name)], directx),
        None => texture_export::as_they_are(&maps),
    }
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
        assert_eq!(names(Preset::Maps, false), ["base_color", "base_metalness", "specular_roughness", "height", "normal", "ambient_occlusion"]);
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
        assert!(matches!(normal[1], Source::Map { channel: 1, invert: true, .. }));
        let Planned::Pack(directx) = &plan(&g, "Rusty Metal", Preset::Unreal, true)[1].what else { panic!() };
        assert!(directx.iter().all(|s| matches!(s, Source::Map { invert: false, .. })), "DirectX already");
        // Maps as they are: of three channels, of one where they are gray.
        let Planned::Pack(base) = &unreal[0].what else { panic!() };
        assert_eq!(base.len(), 3);
        let Planned::Pack(height) = &unreal[3].what else { panic!() };
        assert_eq!(height.len(), 1);
        let Planned::Pack(orm) = &unreal[2].what else { panic!() };
        let usage = |source: &Source| match source {
            Source::Map { map: node, .. } => g.node(*node).map(|n| n.text("name").to_string()),
            Source::Value(_) => None,
        };
        assert_eq!(
            orm.iter().map(usage).collect::<Vec<_>>(),
            [Some("ambient_occlusion".into()), Some("specular_roughness".into()), Some("base_metalness".into())]
        );
        let hdrp = plan(&g, "Rusty Metal", Preset::UnityHdrp, false);
        let Planned::Pack(mask) = &hdrp[2].what else { panic!() };
        assert_eq!(mask[2], Source::Value(1.0));
        assert!(matches!(mask[3], Source::Map { invert: true, .. }), "smoothness: 1 − roughness");
        // No such maps: values.
        let empty = Template::Empty.graph([512, 512], crate::Precision::Half);
        assert!(plan(&empty, "x", Preset::Unreal, false).is_empty(), "nothing to base on, nothing packed of nothing");
    }

    /// A graph of the specular-and-glossiness workflow, of a displacement and an opacity, is
    /// exported as the 3D view shows it: its diffuse color the base color, its opacity the base
    /// color's alpha, its glossiness turned over the roughness (and the smoothness as it is), its
    /// displacement the height.
    #[test]
    fn maps_that_stand_for_others_are_exported_as_them() {
        let mut g = crate::graph::Graph::default();
        let mut out = |usage: Usage| {
            let id = g.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
            let node = g.node_mut(id).expect("the output");
            node.param_mut("name").expect("a name").value = crate::types::ParamValue::Text(usage.id().into());
            node.param_mut("usage").expect("a usage").value = crate::types::ParamValue::Enum(usage.index());
            id
        };
        let [diffuse, gloss, displacement, opacity] = [Usage::Diffuse, Usage::Glossiness, Usage::Displacement, Usage::Opacity].map(&mut out);
        let unreal = plan(&g, "Leather", Preset::Unreal, false);
        let names: Vec<&str> = unreal.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["T_Leather_BaseColor", "T_Leather_ORM", "T_Leather_Height"]);
        let source = |map, channel, invert| Source::Map { map, channel, invert };
        assert_eq!(
            unreal[0].what,
            Planned::Pack(vec![source(diffuse, 0, false), source(diffuse, 1, false), source(diffuse, 2, false), source(opacity, 0, false)])
        );
        let Planned::Pack(orm) = &unreal[1].what else { panic!() };
        assert_eq!(orm[1], source(gloss, 0, true), "roughness: 1 − glossiness");
        assert_eq!(unreal[2].what, Planned::Pack(vec![source(displacement, 0, false)]));
        let urp = plan(&g, "Leather", Preset::UnityUrp, false);
        let Planned::Pack(metal) = &urp[1].what else { panic!() };
        assert_eq!(metal[3], source(gloss, 0, false), "smoothness: the glossiness as it is");
        assert_eq!(metal[0], Source::Value(0.0), "no metal");
        // An opacity and nothing else: OpenPBR's base color under it.
        let mut only = crate::graph::Graph::default();
        let id = only.add_node(NodeKind::OUTPUT, [0.0, 0.0]);
        only.node_mut(id).and_then(|n| n.param_mut("usage")).expect("a usage").value = crate::types::ParamValue::Enum(Usage::Opacity.index());
        let gltf = plan(&only, "Leaf", Preset::Gltf, false);
        assert_eq!(gltf.len(), 1);
        assert_eq!(gltf[0].what, Planned::Pack(vec![Source::Value(0.8), Source::Value(0.8), Source::Value(0.8), source(id, 0, false)]));
    }

    /// What a file of a preset is made of is said channel by channel: the gray of a map in three
    /// of them as RGB, a value with what it stands for.
    #[test]
    fn packed_files_say_what_each_channel_is() {
        let map = |reference: u32, name: &str, usage: &str| texture_export::Map { reference, name: name.into(), usage: Some(usage.into()) };
        let plan = |maps: &[texture_export::Map<u32>], preset: Preset| texture_export::plan_template(preset.template().expect("a template"), maps, &[], false);
        let maps = [map(1, "metal", "metallic"), map(2, "rough", "roughness")];
        let urp = plan(&maps, Preset::UnityUrp);
        assert_eq!(urp[0].said, "RGB metal · A 1 − rough");
        let hdrp = plan(&maps, Preset::UnityHdrp);
        assert_eq!(hdrp[0].said, "R metal · G 1 (no ambient occlusion) · B 1 (detail mask) · A 1 − rough");
        let base = plan(&[map(3, "alpha", "opacity")], Preset::Gltf);
        assert_eq!(base[0].said, "RGB 0.8 (no base color) · A alpha");
    }
}
