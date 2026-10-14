//! What an export writes of a graph (File → Export…): its Outputs as they are, or the files a
//! game engine or a format expects of a material, made of the Outputs by their usages (Base
//! Color, Normal, Roughness…), some packed into the channels of one file. What each preset
//! writes is the `texture-export` crate's, which the programs of the set share; this says
//! which maps a graph has: its Outputs, in the order a node of the graph has them, each with
//! its usage.

use crate::graph::{Graph, NodeId};
use crate::nodes::{NodeKind, Usage};

pub use texture_export::Preset;

/// A channel of a file packed of several, of a graph: of an Output's image, or a value.
pub type Source = texture_export::Source<NodeId>;
/// A file an export writes of a graph.
pub type Planned = texture_export::Planned<NodeId>;
pub type PlannedFile = texture_export::PlannedFile<NodeId>;

/// What an Output's usage is to an export, where it is one the presets read.
fn usage(index: u32) -> Option<texture_export::Usage> {
    use texture_export::Usage as U;
    [
        (Usage::BaseColor, U::BaseColor),
        (Usage::Normal, U::Normal),
        (Usage::Height, U::Height),
        (Usage::Roughness, U::Roughness),
        (Usage::Metallic, U::Metallic),
        (Usage::AmbientOcclusion, U::AmbientOcclusion),
        (Usage::Emissive, U::Emissive),
        (Usage::Opacity, U::Opacity),
        (Usage::Diffuse, U::Diffuse),
        (Usage::Glossiness, U::Glossiness),
        (Usage::Displacement, U::Displacement),
    ]
    .into_iter()
    .find_map(|(ours, theirs)| (ours.index() == index).then_some(theirs))
}

/// The maps of `graph` to an export: its Outputs, the upper first (as a node of the graph has
/// them), each by its name and usage.
pub fn maps(graph: &Graph) -> Vec<texture_export::Map<NodeId>> {
    crate::instance::port_nodes(graph)
        .into_iter()
        .map(|n| texture_export::Map { reference: n.id, name: n.text("name").to_string(), usage: usage(n.enum_value("usage")) })
        .collect()
}

/// Whether the normal maps of `graph` are DirectX's (Y−): as its Normal nodes make them (the
/// first says), OpenGL's where it has none.
pub fn graph_normals_directx(graph: &Graph) -> bool {
    graph.nodes_of(NodeKind::NORMAL).next().is_some_and(|n| n.enum_value("format") == 1)
}

/// The files `preset` writes of `graph` (called `name`), its normal maps being DirectX's
/// where `directx`.
pub fn plan(graph: &Graph, name: &str, preset: Preset, directx: bool) -> Vec<PlannedFile> {
    texture_export::plan(&maps(graph), name, preset, directx)
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
}
