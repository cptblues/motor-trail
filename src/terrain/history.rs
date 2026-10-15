//! One sparse operation per completed stroke; runtime republishes geometry after undo/redo.
use std::collections::{BTreeMap, VecDeque};
use std::mem::size_of;

use super::brush::HeightChange;
use super::config::MAX_VERTICES;
use super::obstacles::Obstacle;
use super::scene::{TerrainScene, validate_obstacles};

const MAX_OPERATIONS: usize = 32;
const MAX_MEMORY_BYTES: usize = 96 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum EditOperation {
    Sculpt(Vec<HeightChange>),
    Objects {
        before: Vec<Obstacle>,
        after: Vec<Obstacle>,
    },
}

#[derive(Default, Debug)]
pub struct EditHistory {
    undo: VecDeque<EditOperation>,
    redo: Vec<EditOperation>,
}

impl EditHistory {
    pub fn push(&mut self, operation: EditOperation) {
        let Some(operation) = operation.normalized() else {
            return;
        };
        if operation.payload_bytes()
            + size_of::<Self>()
            + MAX_OPERATIONS * 2 * size_of::<EditOperation>()
            > MAX_MEMORY_BYTES
        {
            return;
        }
        self.redo.clear();
        self.redo.shrink_to_fit();
        // Evict before pushing to keep the deque capacity bounded too.
        if self.undo.len() == MAX_OPERATIONS {
            self.undo.pop_front();
        }
        self.undo.push_back(operation);
        while self.memory_bytes() > MAX_MEMORY_BYTES {
            if self.undo.pop_front().is_none() {
                break;
            }
        }
    }

    pub fn undo(&mut self, scene: &mut TerrainScene) -> bool {
        let Some(operation) = self.undo.back() else {
            return false;
        };
        if !operation.apply(scene, false) {
            return false;
        }
        if let Some(operation) = self.undo.pop_back() {
            self.redo.push(operation);
        }
        true
    }

    pub fn redo(&mut self, scene: &mut TerrainScene) -> bool {
        let Some(operation) = self.redo.last() else {
            return false;
        };
        if !operation.apply(scene, true) {
            return false;
        }
        if let Some(operation) = self.redo.pop() {
            self.undo.push_back(operation);
        }
        true
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn memory_bytes(&self) -> usize {
        size_of::<Self>()
            + (self.undo.capacity() + self.redo.capacity()) * size_of::<EditOperation>()
            + self
                .undo
                .iter()
                .chain(self.redo.iter())
                .map(EditOperation::payload_bytes)
                .sum::<usize>()
    }
}

impl EditOperation {
    fn normalized(self) -> Option<Self> {
        match self {
            Self::Sculpt(changes) => {
                let mut unique: BTreeMap<usize, HeightChange> = BTreeMap::new();
                for change in changes {
                    if !change.before.is_finite() || !change.after.is_finite() {
                        return None;
                    }
                    unique
                        .entry(change.index)
                        .and_modify(|existing| existing.after = change.after)
                        .or_insert(change);
                    if unique.len() > MAX_VERTICES {
                        return None;
                    }
                }
                let mut changes: Vec<_> = unique
                    .into_values()
                    .filter(|c| c.before != c.after)
                    .collect();
                if changes.is_empty() {
                    return None;
                }
                changes.shrink_to_fit();
                Some(Self::Sculpt(changes))
            }
            Self::Objects {
                mut before,
                mut after,
            } => {
                if before == after
                    || validate_obstacles(&before).is_err()
                    || validate_obstacles(&after).is_err()
                {
                    return None;
                }
                before.shrink_to_fit();
                after.shrink_to_fit();
                Some(Self::Objects { before, after })
            }
        }
    }

    fn payload_bytes(&self) -> usize {
        match self {
            Self::Sculpt(changes) => changes.capacity() * size_of::<HeightChange>(),
            Self::Objects { before, after } => {
                (before.capacity() + after.capacity()) * size_of::<Obstacle>()
            }
        }
    }

    fn apply(&self, scene: &mut TerrainScene, forward: bool) -> bool {
        match self {
            Self::Sculpt(changes) => {
                let values = |c: &HeightChange| {
                    if forward {
                        (c.before, c.after)
                    } else {
                        (c.after, c.before)
                    }
                };
                // Check every index and expected value before changing any sample.
                if changes
                    .iter()
                    .any(|c| scene.terrain.heights.get(c.index).copied() != Some(values(c).0))
                {
                    return false;
                }
                for change in changes {
                    scene.terrain.heights[change.index] = values(change).1;
                }
                if scene.terrain.validate().is_err() {
                    for change in changes {
                        scene.terrain.heights[change.index] = values(change).0;
                    }
                    return false;
                }
                true
            }
            Self::Objects { before, after } => {
                let (expected, replacement) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                if &scene.obstacles != expected || validate_obstacles(replacement).is_err() {
                    return false;
                }
                scene.obstacles.clone_from(replacement);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::scene::ReferenceScene;
    use super::*;

    #[test]
    fn whole_stroke_roundtrip_and_branch_invalidation() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let mut history = EditHistory::default();
        let before = scene.terrain.heights[0];
        scene.terrain.heights[0] = before + 1.0;
        history.push(EditOperation::Sculpt(vec![HeightChange {
            index: 0,
            before,
            after: before + 1.0,
        }]));
        assert!(history.undo(&mut scene));
        assert_eq!(scene.terrain.heights[0], before);
        assert!(history.redo(&mut scene));
        assert_eq!(scene.terrain.heights[0], before + 1.0);
        assert!(history.undo(&mut scene));
        history.push(EditOperation::Sculpt(vec![HeightChange {
            index: 1,
            before: 0.0,
            after: 2.0,
        }]));
        assert_eq!(history.redo_len(), 0);
    }

    #[test]
    fn object_placement_transform_and_deletion_roundtrip() {
        let mut scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        let before = scene.obstacles.clone();
        let mut after = before.clone();
        after[0].rotation_degrees = [10.0, 35.0, 5.0];
        after.pop();
        scene.obstacles = after.clone();
        let mut history = EditHistory::default();
        history.push(EditOperation::Objects {
            before: before.clone(),
            after: after.clone(),
        });
        assert!(history.undo(&mut scene));
        assert_eq!(scene.obstacles, before);
        assert!(history.redo(&mut scene));
        assert_eq!(scene.obstacles, after);
    }

    #[test]
    fn noop_strokes_preserve_redo_and_repeated_indices_merge() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let mut history = EditHistory::default();
        scene.terrain.heights[0] = 2.0;
        history.push(EditOperation::Sculpt(vec![
            HeightChange {
                index: 0,
                before: 0.0,
                after: 1.0,
            },
            HeightChange {
                index: 0,
                before: 1.0,
                after: 2.0,
            },
        ]));
        assert!(history.undo(&mut scene));
        assert_eq!(scene.terrain.heights[0], 0.0);
        history.push(EditOperation::Sculpt(vec![]));
        history.push(EditOperation::Sculpt(vec![HeightChange {
            index: 0,
            before: 1.0,
            after: 1.0,
        }]));
        history.push(EditOperation::Objects {
            before: vec![],
            after: vec![],
        });
        assert_eq!(history.redo_len(), 1);
        assert!(history.redo(&mut scene));
        assert_eq!(scene.terrain.heights[0], 2.0);
    }

    #[test]
    fn stale_or_invalid_delta_does_not_partially_modify_scene() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        let before = scene.clone();
        let mut history = EditHistory::default();
        history.push(EditOperation::Sculpt(vec![HeightChange {
            index: usize::MAX,
            before: 0.0,
            after: 1.0,
        }]));
        assert!(!history.undo(&mut scene));
        assert_eq!(scene, before);
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn retains_32_operations_and_releases_memory_when_evicted() {
        let mut history = EditHistory::default();
        for index in 0..100 {
            history.push(EditOperation::Sculpt(vec![HeightChange {
                index,
                before: 0.0,
                after: 1.0,
            }]));
        }
        assert_eq!(history.undo_len(), 32);
        assert!(history.memory_bytes() < 96 * 1024 * 1024);
    }

    #[test]
    fn a_stroke_larger_than_any_valid_terrain_does_not_displace_history() {
        let mut history = EditHistory::default();
        history.push(EditOperation::Sculpt(vec![HeightChange {
            index: 0,
            before: 0.0,
            after: 1.0,
        }]));
        let changes = (0..=super::super::config::MAX_VERTICES)
            .map(|index| HeightChange {
                index,
                before: 0.0,
                after: 1.0,
            })
            .collect();
        history.push(EditOperation::Sculpt(changes));
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn validation_failure_rolls_back_every_sample() {
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        scene.terrain.heights[0] = 1.0;
        scene.terrain.heights[1] = 2.0;
        let before = scene.clone();
        let mut history = EditHistory::default();
        history.push(EditOperation::Sculpt(vec![
            HeightChange {
                index: 0,
                before: 0.0,
                after: 1.0,
            },
            HeightChange {
                index: 1,
                before: 1000.0,
                after: 2.0,
            },
        ]));
        assert!(!history.undo(&mut scene));
        assert_eq!(scene, before);
    }
}
