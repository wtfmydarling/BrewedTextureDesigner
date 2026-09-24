//! An open document: a package (see [`crate::package`]), the graph of it being edited, its
//! undo history and its save state.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::edit::{Edit, EditResult};
use crate::graph::Graph;
use crate::history::History;
use crate::instance::GraphId;
use crate::package::{GraphEntry, Library, Package, PackageEdit, PackageResult, sync_instances};

pub struct Document {
    package: Package,
    /// The graph being edited.
    current: GraphId,
    /// Each step goes back to the package as it was, and to the graph that was being edited.
    history: History<(Package, GraphId)>,
    /// Incremented by every change (edits, undo, redo, loading, another graph edited).
    revision: u64,
    saved_revision: u64,
    path: Option<PathBuf>,
}

impl Document {
    pub fn new(package: Package, path: Option<PathBuf>) -> Document {
        let current = package.first();
        Document { package, current, history: History::default(), revision: 1, saved_revision: 1, path }
    }

    /// A document of one graph.
    pub fn of_graph(graph: Graph, path: Option<PathBuf>) -> Document {
        Document::new(Package::new("Graph", graph), path)
    }

    /// The graph being edited.
    pub fn graph(&self) -> &Graph {
        &self.entry().graph
    }

    /// The graph being edited, with its id and name.
    pub fn entry(&self) -> &GraphEntry {
        self.package.graph(self.current).expect("the graph being edited is in the package")
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The id of the graph being edited.
    pub fn current(&self) -> GraphId {
        self.current
    }

    /// Edit another graph of the package. Not a change of the document, but a new revision:
    /// what is shown changes.
    pub fn open_graph(&mut self, id: GraphId) -> bool {
        if id == self.current || self.package.graph(id).is_none() {
            return false;
        }
        self.current = id;
        self.history.seal();
        self.touch();
        true
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// The path of the package file as the graphs that use this package's graphs name it.
    pub fn key(&self) -> Option<String> {
        self.path.as_ref().map(|p| p.display().to_string())
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

    /// A new revision that is not a change of the document: a clean document stays clean.
    fn touch(&mut self) {
        if !self.is_dirty() {
            self.saved_revision += 1;
        }
        self.revision += 1;
    }

    /// Apply an edit to the graph being edited and record it for undo. No-op edits do not
    /// create undo steps.
    pub fn apply(&mut self, edit: Edit) -> Result<EditResult, String> {
        self.apply_at(edit, Instant::now())
    }

    pub fn apply_at(&mut self, edit: Edit, now: Instant) -> Result<EditResult, String> {
        let key = edit.merge_key();
        let label = edit.label();
        let merge = self.history.would_merge(key.as_ref(), now);
        // (A copy of the package shares its graphs: the one the edit changes is copied then.)
        let before = if merge { None } else { Some((self.package.clone(), self.current)) };
        let entry = self.package.graph_mut(self.current).expect("the graph being edited is in the package");
        let result = edit.apply(Arc::make_mut(&mut entry.graph))?;
        if result.changed {
            self.package.sync_resources();
            self.revision += 1;
            match before {
                Some(before) => self.history.push(before, label, key, now),
                None => self.history.touch(now),
            }
        }
        Ok(result)
    }

    /// Apply a change of the package as a whole, as one undo step.
    pub fn apply_package(&mut self, edit: PackageEdit) -> Result<PackageResult, String> {
        let label = edit.label();
        let mut package = self.package.clone();
        let result = edit.apply(&mut package)?;
        if result.changed {
            let before = std::mem::replace(&mut self.package, package);
            self.history.push((before, self.current), label, None, Instant::now());
            self.history.seal();
            self.package.sync_resources();
            if self.package.graph(self.current).is_none() {
                self.current = self.package.first();
            }
            self.revision += 1;
        }
        Ok(result)
    }

    /// Bring the nodes that use graphs in line with those graphs (see
    /// [`crate::package::sync_instances`]). What the graphs are, not a change of the
    /// document: no undo step, and a clean document stays clean. True if anything changed.
    pub fn sync_instances(&mut self, lib: &impl Library) -> bool {
        let own = self.key();
        let changed = sync_instances(&mut self.package, own.as_deref(), lib);
        if changed {
            self.touch();
        }
        changed
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

    /// Step back: to the package as it was, in the graph that was being edited then.
    pub fn undo(&mut self) -> bool {
        match self.history.undo(&(self.package.clone(), self.current)) {
            Some(state) => {
                self.restore(state);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo(&(self.package.clone(), self.current)) {
            Some(state) => {
                self.restore(state);
                true
            }
            None => false,
        }
    }

    fn restore(&mut self, (package, current): (Package, GraphId)) {
        self.package = package;
        self.current = if self.package.graph(current).is_some() { current } else { self.package.first() };
        self.revision += 1;
    }

    /// Replace the whole document (new / open). History starts over.
    pub fn replace(&mut self, package: Package, path: Option<PathBuf>) {
        self.current = package.first();
        self.package = package;
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
        let mut doc = Document::of_graph(Graph::default(), None);
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
        doc.mark_saved("x.brewtex".into());
        assert!(!doc.is_dirty());
        assert_eq!(doc.title(), "x.brewtex");
    }

    #[test]
    fn failed_edits_change_nothing() {
        let mut doc = Document::of_graph(Graph::default(), None);
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

    #[test]
    fn graphs_of_a_package_and_their_undo() {
        let mut doc = Document::of_graph(Graph::default(), None);
        let first = doc.current();
        let blur = doc.apply(Edit::AddNode { kind: NodeKind::BLUR, pos: [0.0, 0.0] }).unwrap().created[0];
        let second = doc.apply_package(PackageEdit::AddGraph { name: "Bricks".into(), graph: Graph::default() }).unwrap().graph.unwrap();
        // Editing another graph is not a change; the undo steps stay.
        let saved = doc.revision();
        doc.mark_saved("p.brewtex".into());
        assert!(doc.open_graph(second));
        assert!(doc.revision() > saved && !doc.is_dirty());
        assert!(doc.graph().nodes.is_empty());
        doc.apply(Edit::AddNode { kind: NodeKind::NOISE, pos: [0.0, 0.0] }).unwrap();
        // Undo goes back through both graphs, each in its own graph.
        assert!(doc.undo());
        assert_eq!(doc.current(), second);
        assert!(doc.graph().nodes.is_empty());
        assert!(doc.undo());
        assert_eq!(doc.package().graphs.len(), 1);
        assert_eq!(doc.current(), first);
        assert!(doc.graph().node(blur).is_some());
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.current(), second);
        assert_eq!(doc.graph().nodes.len(), 1);
        // A graph that is not changed is not copied for undo.
        let other = &doc.package().graph(first).unwrap().graph;
        assert!(Arc::strong_count(other) > 1, "shared with the undo steps");
    }
}
