//! The graph the app opens with: a procedural brick wall that exercises variables, Math,
//! exposed parameters, Noise, Blend, Levels, Blur, Gradient Map, Histogram Scan, Ambient
//! Occlusion, Pixel Math, comments and several outputs for the 3D view. Every image in it
//! tiles.
//!
//! It is an ordinary project file, `assets/demo.brewtex`, built into the program: open it,
//! change it, save it over itself, and the next build opens with that.

use crate::graph::{Graph, NodeId};
use crate::package::Package;
use crate::project;

/// The demo package, as it is in `assets/demo.brewtex`.
const DEMO: &[u8] = include_bytes!("../assets/demo.brewtex");

/// What the graph of the demo is called.
pub const NAME: &str = "Brick Wall";

/// The demo as a package, and the node it shows in the 2D view first.
pub fn package() -> (Package, Option<NodeId>) {
    match project::from_bytes(DEMO, None) {
        Ok(loaded) => {
            let preview = loaded.view().preview;
            (loaded.package, preview)
        }
        Err(e) => {
            // The tests read the file; a build that cannot starts empty rather than not at all.
            log::error!("the demo graph cannot be read: {e}");
            (Package::new(NAME, Graph::default()), None)
        }
    }
}

/// The demo graph, and the node it shows in the 2D view first.
pub fn graph() -> (Graph, Option<NodeId>) {
    let (package, preview) = package();
    (Graph::clone(&package.graphs[0].graph), preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::NodeKind;
    use crate::types::ParamValue;

    #[test]
    fn demo_graph_is_valid() {
        let loaded = project::from_bytes(DEMO, None).expect("assets/demo.brewtex is a package");
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        let (g, preview) = graph();
        assert!(preview.is_some_and(|id| g.node(id).is_some()), "the demo shows one of its nodes");
        let (_, cyclic) = g.topo_order();
        assert!(cyclic.is_empty());
        for out in g.nodes_of(NodeKind::OUTPUT) {
            assert!(g.link_into(out.id, "i:input").is_some(), "output {} is not connected", out.text("name"));
        }
        assert_eq!(g.variables.len(), 5);
        assert_eq!(g.links.len(), 23);
        assert_eq!(g.comments.len(), 2);
        let ev = crate::eval::evaluate(&g);
        for n in &g.nodes {
            let e = ev.node(n.id).unwrap();
            assert!(e.errors.is_empty(), "{}: {:?}", n.def().name, e.errors);
            assert!(e.warnings.is_empty(), "{}: {:?}", n.def().name, e.warnings);
        }
        // The size is driven through Math: 1 - 2 * 0.06.
        let tiles = g.nodes_of(NodeKind::TILE_GENERATOR).next().unwrap().id;
        assert_eq!(ev.node(tiles).unwrap().driven.get("size"), Some(&ParamValue::Vec2([0.88, 0.88])));
    }
}
