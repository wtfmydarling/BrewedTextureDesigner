//! Undo / redo based on serialized snapshots of the graph.
//!
//! The graph is small (parameters, positions, links), so a JSON snapshot per step is
//! cheap and makes undo exact for every kind of edit. Rapid bursts of changes (slider
//! drags, typing) are merged into one step.

use crate::graph::Graph;

const MAX_STEPS: usize = 200;
const MERGE_SECONDS: f64 = 0.6;

pub struct History {
    undo: Vec<String>,
    redo: Vec<String>,
    current: String,
    last_change: f64,
}

impl History {
    pub fn new(graph: &Graph) -> History {
        History { undo: Vec::new(), redo: Vec::new(), current: snapshot(graph), last_change: f64::NEG_INFINITY }
    }

    /// Record the graph state. Call once per frame; skipped while the user is still
    /// dragging so a drag becomes a single undo step.
    pub fn track(&mut self, graph: &Graph, now: f64, interacting: bool) {
        if interacting {
            return;
        }
        let snap = snapshot(graph);
        if snap == self.current {
            return;
        }
        if now - self.last_change > MERGE_SECONDS {
            self.undo.push(std::mem::replace(&mut self.current, snap));
            if self.undo.len() > MAX_STEPS {
                self.undo.remove(0);
            }
        } else {
            self.current = snap;
        }
        self.redo.clear();
        self.last_change = now;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Option<Graph> {
        let prev = self.undo.pop()?;
        self.redo.push(std::mem::replace(&mut self.current, prev));
        self.last_change = f64::NEG_INFINITY;
        restore(&self.current)
    }

    pub fn redo(&mut self) -> Option<Graph> {
        let next = self.redo.pop()?;
        self.undo.push(std::mem::replace(&mut self.current, next));
        self.last_change = f64::NEG_INFINITY;
        restore(&self.current)
    }

    /// Start over from `graph` (new or loaded document).
    pub fn reset(&mut self, graph: &Graph) {
        *self = History::new(graph);
    }
}

fn snapshot(graph: &Graph) -> String {
    serde_json::to_string(graph).unwrap_or_default()
}

fn restore(json: &str) -> Option<Graph> {
    let mut g: Graph = serde_json::from_str(json).ok()?;
    g.normalize();
    Some(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::NodeKind;

    #[test]
    fn undo_redo_and_merging() {
        let mut g = Graph::default();
        let mut h = History::new(&g);
        g.add_node(NodeKind::Noise, [0.0, 0.0]);
        h.track(&g, 1.0, false);
        g.add_node(NodeKind::Blur, [0.0, 0.0]);
        h.track(&g, 2.0, false);
        // Two quick edits merge into one step.
        g.nodes[1].pos = [5.0, 5.0];
        h.track(&g, 2.1, false);
        g.nodes[1].pos = [9.0, 9.0];
        h.track(&g, 2.2, false);
        let back = h.undo().unwrap();
        assert_eq!(back.nodes.len(), 1);
        let back = h.undo().unwrap();
        assert!(back.nodes.is_empty());
        assert!(h.undo().is_none());
        let fwd = h.redo().unwrap();
        assert_eq!(fwd.nodes.len(), 1);
        let fwd = h.redo().unwrap();
        assert_eq!(fwd.nodes[1].pos, [9.0, 9.0]);
    }
}
