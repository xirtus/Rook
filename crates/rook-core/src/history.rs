//! Undo/redo history — snapshot-based, from cutlass-engines.
//!
//! Every edit clones the project state before applying.  Undo restores the
//! previous clone; redo re-applies from the redo stack.  This is simpler
//! than inverse-command undo for a project with many interacting fields.

use crate::project::Project;

/// Undo/redo manager that snapshots the entire [`Project`] before each edit.
///
/// Cloning a project is dominated by the timeline + asset pool, which are
/// small relative to decoded frame data (which lives in the engine, not
/// the project).  A typical project with 50 clips and 200 assets fits in
/// a few hundred KB, so snapshotting at edit cadence is cheap.
/// An undo/redo entry: a snapshot of the project state plus a human label.
pub type HistoryEntry = (String, Project);

pub struct EditHistory {
    /// Undo stack: (label, project) pairs. Most recent last.
    undo_stack: Vec<HistoryEntry>,
    /// Redo stack: (label, project) pairs. Most recent last.
    redo_stack: Vec<HistoryEntry>,
    /// Maximum number of undo entries to retain.
    limit: usize,
}

impl EditHistory {
    pub fn new(limit: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Record `pre_edit` as the state to restore on undo.
    /// Clears the redo stack (a new edit invalidates redo).
    pub fn record(&mut self, pre_edit: Project) {
        self.record_labeled("Edit", pre_edit);
    }

    /// Record with an explicit label (for undo/redo dropdown menus).
    pub fn record_labeled(&mut self, label: &str, pre_edit: Project) {
        self.redo_stack.clear();
        self.undo_stack.push((label.to_string(), pre_edit));
        // Trim oldest entries if over limit
        if self.undo_stack.len() > self.limit {
            let excess = self.undo_stack.len() - self.limit;
            self.undo_stack.drain(0..excess);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Undo: pop the most recent pre-edit snapshot, banking `current` for redo.
    /// Returns the project restored and its label.
    pub fn undo(&mut self, current: Project) -> Option<Project> {
        let (label, previous) = self.undo_stack.pop()?;
        self.redo_stack.push((label, current));
        Some(previous)
    }

    /// Redo: pop the most recent redo snapshot, banking `current` for undo.
    /// Returns the project restored and its label.
    pub fn redo(&mut self, current: Project) -> Option<Project> {
        let (label, next) = self.redo_stack.pop()?;
        self.undo_stack.push((label, current));
        Some(next)
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Label of the next undo operation (most recent first).
    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|(label, _)| label.as_str())
    }

    /// Label of the next redo operation.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|(label, _)| label.as_str())
    }

    /// List undo labels (most recent first) for UI display.
    pub fn undo_labels(&self) -> Vec<String> {
        self.undo_stack.iter()
            .rev()
            .map(|(label, _)| label.clone())
            .collect()
    }

    /// List redo labels (most recent first) for UI display.
    pub fn redo_labels(&self) -> Vec<String> {
        self.redo_stack.iter()
            .rev()
            .map(|(label, _)| label.clone())
            .collect()
    }
}
