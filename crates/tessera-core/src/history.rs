//! Undo / redo with snapshots of the graph.
//!
//! The graph is small (parameters, positions, links), so storing the state before each
//! step is cheap and makes undo exact for every kind of edit. Consecutive edits with the
//! same merge key (a slider drag, typing into one field) collapse into one step until
//! [`History::seal`] is called or the user pauses.

use std::time::{Duration, Instant};

use crate::edit::MergeKey;
use crate::graph::Graph;

const MAX_STEPS: usize = 256;
const MERGE_WINDOW: Duration = Duration::from_millis(1500);

struct Step {
    /// The state to go back to.
    state: Graph,
    label: &'static str,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    last_key: Option<MergeKey>,
    last_time: Option<Instant>,
    sealed: bool,
}

impl History {
    /// Would an edit with `key` be merged into the current step?
    pub fn would_merge(&self, key: Option<&MergeKey>, now: Instant) -> bool {
        !self.sealed
            && key.is_some()
            && key == self.last_key.as_ref()
            && !self.undo.is_empty()
            && self.last_time.is_some_and(|t| now.duration_since(t) < MERGE_WINDOW)
    }

    /// Record a new step whose undo restores `before`.
    pub fn push(&mut self, before: Graph, label: &'static str, key: Option<MergeKey>, now: Instant) {
        self.undo.push(Step { state: before, label });
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_key = key;
        self.last_time = Some(now);
        self.sealed = false;
    }

    /// An edit was merged into the current step.
    pub fn touch(&mut self, now: Instant) {
        self.redo.clear();
        self.last_time = Some(now);
    }

    /// End the current step: the next edit starts a new one (end of a drag, focus
    /// change, selection change).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|s| s.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|s| s.label)
    }

    /// Step back: `current` is kept for redo, the previous state is returned.
    pub fn undo(&mut self, current: &Graph) -> Option<Graph> {
        let step = self.undo.pop()?;
        self.redo.push(Step { state: current.clone(), label: step.label });
        self.sealed = true;
        Some(step.state)
    }

    pub fn redo(&mut self, current: &Graph) -> Option<Graph> {
        let step = self.redo.pop()?;
        self.undo.push(Step { state: current.clone(), label: step.label });
        self.sealed = true;
        Some(step.state)
    }

    pub fn clear(&mut self) {
        *self = History::default();
    }
}
