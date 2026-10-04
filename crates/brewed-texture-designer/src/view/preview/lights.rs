//! The points of the nodes that make panoramas, in the 2D view (overlay.rs draws them and takes
//! the mouse), as Substance Designer's HDRI tools are placed: where a softbox or a panel is,
//! where a tube begins and ends, where a gradient's top color is. Each is dragged where it
//! should be. A point of a panorama is across (0 to 1, once round) and down (0 straight up, 1
//! straight down).

use brewed_texture_core::graph::NodeId;
use brewed_texture_core::{Edit, NodeKind, ParamValue};
use iced::Vector;

use crate::app::App;

/// The points of a node of panoramas, as the graph gives them.
#[derive(Clone, Debug)]
pub struct Points {
    node: NodeId,
    points: Vec<(&'static str, Vector)>,
    /// The points that take their values from sockets: they stay where they are.
    locked: Vec<&'static str>,
}

impl Points {
    /// The points of the node `id`, if it makes a panorama.
    pub fn of(app: &App, id: NodeId) -> Option<Points> {
        let node = app.graph().node(id)?;
        let keys: &[&'static str] = if node.kind == NodeKind::HDRI_GRADIENT {
            &["point"]
        } else if node.kind == NodeKind::HDRI_LINE_LIGHT {
            &["point_1", "point_2"]
        } else if node.kind == NodeKind::HDRI_SHAPE_LIGHT || node.kind == NodeKind::HDRI_PLANE_LIGHT {
            &["position"]
        } else {
            return None;
        };
        let params = app.eval.params(id)?;
        Some(Points {
            node: id,
            points: keys.iter().map(|k| (*k, Vector::from(params.v2(k)))).collect(),
            locked: keys.iter().copied().filter(|k| node.is_exposed(k)).collect(),
        })
    }

    /// Where the points are on the panorama.
    pub fn points(&self) -> impl Iterator<Item = Vector> + '_ {
        self.points.iter().map(|(_, at)| *at)
    }

    /// Whether the point `i` takes its value from a socket.
    pub fn locked(&self, i: usize) -> bool {
        self.points.get(i).is_some_and(|(k, _)| self.locked.contains(k))
    }

    /// What dragging the point `i` from `from` to `to` (points of the panorama) does: it moves as
    /// far, round and round across; Shift keeps it to one axis.
    pub fn edits(&self, i: usize, from: Vector, to: Vector, shift: bool) -> Vec<Edit> {
        let Some(&(key, at)) = self.points.get(i).filter(|_| !self.locked(i)) else { return Vec::new() };
        let mut by = to - from;
        if shift {
            if by.x.abs() > by.y.abs() {
                by.y = 0.0;
            } else {
                by.x = 0.0;
            }
        }
        let to = at + by;
        vec![Edit::SetParam { node: self.node, key: key.into(), value: ParamValue::Vec2([to.x.rem_euclid(1.0), to.y.clamp(0.0, 1.0)]) }]
    }
}
