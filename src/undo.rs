//! Snapshot-based undo/redo of a document's annotation list.
//!
//! Annotation lists are small (geometry only, never pixels), so whole-list
//! snapshots are simpler and more robust than per-command inverses. Callers
//! take a snapshot when an interaction *starts* and commit it once when it
//! ends, so a drag is a single undo step.

use crate::model::Annotation;

const LIMIT: usize = 200;

#[derive(Default, Debug)]
pub struct UndoStack {
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
}

impl UndoStack {
    /// Record `before` as the state preceding a change. No-op when nothing
    /// changed.
    pub fn commit(&mut self, before: Vec<Annotation>, after: &[Annotation]) {
        if before == after {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Returns the state to restore, given the current state.
    pub fn undo(&mut self, current: &[Annotation]) -> Option<Vec<Annotation>> {
        let prev = self.undo.pop()?;
        self.redo.push(current.to_vec());
        Some(prev)
    }

    pub fn redo(&mut self, current: &[Annotation]) -> Option<Vec<Annotation>> {
        let next = self.redo.pop()?;
        self.undo.push(current.to_vec());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pt;
    use crate::model::{Document, PALETTE, Shape, Style};

    fn arrow(x: f32) -> Shape {
        Shape::Arrow {
            start: Pt::ZERO,
            end: Pt::new(x, 0.0),
            style: Style {
                color: PALETTE[0].1,
                width: 2.0,
            },
        }
    }

    #[test]
    fn create_move_delete_undo_redo() {
        let mut d = Document::new(10, 10, "original.png", None);
        let mut u = UndoStack::default();

        let before = d.annotations.clone();
        let id = d.add(arrow(5.0));
        u.commit(before, &d.annotations);

        // A drag made of many moves is one step.
        let before = d.annotations.clone();
        for _ in 0..50 {
            d.get_mut(id).unwrap().shape.translate(Pt::new(1.0, 0.0));
        }
        u.commit(before, &d.annotations);

        let before = d.annotations.clone();
        d.remove(id);
        u.commit(before, &d.annotations);
        assert!(d.annotations.is_empty());

        d.annotations = u.undo(&d.annotations).unwrap();
        assert_eq!(d.annotations.len(), 1);
        d.annotations = u.undo(&d.annotations).unwrap();
        assert_eq!(d.annotations[0].shape, arrow(5.0));
        d.annotations = u.undo(&d.annotations).unwrap();
        assert!(d.annotations.is_empty());
        assert!(u.undo(&d.annotations).is_none());

        d.annotations = u.redo(&d.annotations).unwrap();
        d.annotations = u.redo(&d.annotations).unwrap();
        assert_ne!(d.annotations[0].shape, arrow(5.0));
        assert_eq!(d.annotations[0].id, id, "ids survive undo/redo");

        // A new edit clears the redo stack.
        let before = d.annotations.clone();
        d.add(arrow(1.0));
        u.commit(before, &d.annotations);
        assert!(!u.can_redo());
    }

    #[test]
    fn unchanged_commit_is_ignored() {
        let mut u = UndoStack::default();
        u.commit(Vec::new(), &[]);
        assert!(!u.can_undo());
    }
}
