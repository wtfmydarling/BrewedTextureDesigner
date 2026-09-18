//! An open document: the graph, its undo history and its save state.

use std::path::PathBuf;
use std::time::Instant;

use crate::edit::{Edit, EditResult};
use crate::graph::Graph;
use crate::history::History;

pub struct Document {
    graph: Graph,
    history: History,
    /// Incremented by every change (edits, undo, redo, loading).
    revision: u64,
    saved_revision: u64,
    path: Option<PathBuf>,
}

impl Document {
    pub fn new(graph: Graph, path: Option<PathBuf>) -> Document {
        Document { graph, history: History::default(), revision: 1, saved_revision: 1, path }
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// File name for the title bar.
    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().to_string())
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_revision = self.revision;
    }

    /// Apply an edit and record it for undo. No-op edits do not create undo steps.
    pub fn apply(&mut self, edit: Edit) -> Result<EditResult, String> {
        self.apply_at(edit, Instant::now())
    }

    pub fn apply_at(&mut self, edit: Edit, now: Instant) -> Result<EditResult, String> {
        let key = edit.merge_key();
        let label = edit.label();
        let merge = self.history.would_merge(key.as_ref(), now);
        let before = if merge { None } else { Some(self.graph.clone()) };
        let result = edit.apply(&mut self.graph)?;
        if result.changed {
            self.revision += 1;
            match before {
                Some(before) => self.history.push(before, label, key, now),
                None => self.history.touch(now),
            }
        }
        Ok(result)
    }

    /// End the current undo step (see [`History::seal`]).
    pub fn seal(&mut self) {
        self.history.seal();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    pub fn undo(&mut self) -> bool {
        match self.history.undo(&self.graph) {
            Some(g) => {
                self.graph = g;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(&self.graph) {
            Some(g) => {
                self.graph = g;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    /// Replace the whole document (new / open). History starts over.
    pub fn replace(&mut self, graph: Graph, path: Option<PathBuf>) {
        self.graph = graph;
        self.history.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.path = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, ParamValue};
    use std::time::Duration;

    #[test]
    fn undo_redo_merging_and_dirty_state() {
        let mut doc = Document::new(Graph::default(), None);
        assert!(!doc.is_dirty());
        let t0 = Instant::now();
        let id = doc.apply_at(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }, t0).unwrap().created[0];
        assert!(doc.is_dirty());
        // A drag: many values, one step.
        for (i, r) in [1.0, 2.0, 3.0, 4.0].iter().enumerate() {
            let t = t0 + Duration::from_millis(100 * i as u64);
            let edit = Edit::SetParam { node: id, key: "radius".into(), value: ParamValue::Float(*r) };
            doc.apply_at(edit, t).unwrap();
        }
        doc.seal();
        let edit = Edit::SetParam { node: id, key: "radius".into(), value: ParamValue::Float(9.0) };
        doc.apply_at(edit, t0 + Duration::from_millis(500)).unwrap();
        assert_eq!(doc.undo_label(), Some("Change Parameter"));
        assert!(doc.undo());
        assert_eq!(doc.graph().node(id).unwrap().value("radius"), Some(&ParamValue::Float(4.0)));
        assert!(doc.undo());
        assert_eq!(doc.graph().node(id).unwrap().value("radius"), Some(&ParamValue::Float(8.0)));
        assert!(doc.undo());
        assert!(doc.graph().nodes.is_empty());
        assert!(!doc.undo());
        assert!(doc.redo());
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.graph().node(id).unwrap().value("radius"), Some(&ParamValue::Float(9.0)));
        // No-op edits leave no trace.
        let rev = doc.revision();
        let edit = Edit::SetParam { node: id, key: "radius".into(), value: ParamValue::Float(9.0) };
        assert!(!doc.apply(edit).unwrap().changed);
        assert_eq!(doc.revision(), rev);
        doc.mark_saved("x.tessera".into());
        assert!(!doc.is_dirty());
        assert_eq!(doc.title(), "x.tessera");
    }

    #[test]
    fn failed_edits_change_nothing() {
        let mut doc = Document::new(Graph::default(), None);
        let rev = doc.revision();
        assert!(
            doc.apply(Edit::RemoveLink(crate::Link {
                from: crate::NodeId(1),
                from_socket: "o:output".into(),
                to: crate::NodeId(2),
                to_socket: "i:input".into()
            }))
            .is_ok()
        );
        assert!(doc.apply(Edit::ResetVariable("missing".into())).is_err());
        assert_eq!(doc.revision(), rev);
        assert!(!doc.can_undo());
    }
}
