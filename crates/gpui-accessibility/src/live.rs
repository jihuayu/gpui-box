//! Bounded live-region announcements correlated with semantic scenes.
//!
//! Producers never block. The queue owns an explicit capacity and a dropped
//! counter instead of growing without bound. A surface disposal or generation
//! bump revokes announcements from the old native attachment, so a screen
//! reader can never speak content from a destroyed window.

use crate::LivePriority;
use crate::error::SemanticError;
use crate::scene::{DisplayedScene, SceneRevision, SemanticIdentity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// One live-region announcement. `scene` and `source` are retained so the queue
/// can reject stale revisions and surface disposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveAnnouncement {
    pub scene: DisplayedScene,
    pub source: SemanticIdentity,
    pub priority: LivePriority,
    pub message: String,
}

impl LiveAnnouncement {
    pub fn new(
        scene: DisplayedScene,
        source: SemanticIdentity,
        priority: LivePriority,
        message: impl Into<String>,
    ) -> Self {
        Self {
            scene,
            source,
            priority,
            message: message.into(),
        }
    }

    pub fn is_assertive(&self) -> bool {
        self.priority == LivePriority::Assertive
    }
}

/// The result of enqueueing an announcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveAnnouncementDisposition {
    Queued,
    Deduplicated,
}

/// Bounded announcement queue with explicit overflow accounting.
#[derive(Debug, Clone)]
pub struct LiveAnnouncementQueue {
    capacity: usize,
    queue: VecDeque<LiveAnnouncement>,
    dropped: u64,
    last: Option<LiveAnnouncement>,
    active_generations: BTreeMap<u64, u64>,
    last_revisions: BTreeMap<u64, (u64, SceneRevision)>,
    disposed: BTreeMap<u64, u64>,
}

impl Default for LiveAnnouncementQueue {
    fn default() -> Self {
        Self::new(32)
    }
}

impl LiveAnnouncementQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            queue: VecDeque::new(),
            dropped: 0,
            last: None,
            active_generations: BTreeMap::new(),
            last_revisions: BTreeMap::new(),
            disposed: BTreeMap::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped
    }

    pub fn dequeue(&mut self) -> Option<LiveAnnouncement> {
        self.queue.pop_front()
    }

    pub fn drain(&mut self) -> Vec<LiveAnnouncement> {
        self.queue.drain(..).collect()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.last = None;
    }

    pub fn queued_for_surface(&self, scene: DisplayedScene) -> usize {
        self.queue
            .iter()
            .filter(|announcement| {
                announcement.scene.surface.0 == scene.surface.0
                    && announcement.scene.generation == scene.generation
            })
            .count()
    }

    /// Queue one announcement. Equal/older revisions are stale; identical
    /// consecutive announcements in the same scene revision are deduplicated.
    /// At capacity the oldest item is dropped and counted so the newest state
    /// remains speakable and producers never block.
    pub fn enqueue(
        &mut self,
        announcement: LiveAnnouncement,
    ) -> Result<LiveAnnouncementDisposition, SemanticError> {
        if announcement.message.trim().is_empty() {
            return Err(SemanticError::InvalidAnnouncement);
        }
        let surface = announcement.scene.surface.0;
        let generation = announcement.scene.generation;
        if let Some(disposed) = self.disposed.get(&surface)
            && generation <= *disposed
        {
            return Err(SemanticError::SurfaceDisposed {
                surface: announcement.scene.surface,
            });
        }
        match self.active_generations.get(&surface).copied() {
            Some(active) if generation < active => {
                return Err(SemanticError::StaleGeneration {
                    displayed: generation,
                    active,
                });
            }
            Some(active) if generation > active => {
                self.clear_surface(surface);
                self.active_generations.insert(surface, generation);
            }
            Some(_) => {}
            None => {
                self.active_generations.insert(surface, generation);
            }
        }
        if let Some((revision_generation, revision)) = self.last_revisions.get(&surface).copied()
            && revision_generation == generation
            && announcement.scene.revision < revision
        {
            return Err(SemanticError::StaleRevision {
                displayed: announcement.scene.revision,
                current: revision,
            });
        }
        self.last_revisions
            .insert(surface, (generation, announcement.scene.revision));

        if self
            .last
            .as_ref()
            .is_some_and(|previous| previous == &announcement)
        {
            return Ok(LiveAnnouncementDisposition::Deduplicated);
        }
        if self.queue.len() >= self.capacity {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(announcement.clone());
        self.last = Some(announcement);
        Ok(LiveAnnouncementDisposition::Queued)
    }

    /// Clear announcements for a surface that is being disposed. Any scene at
    /// or below the disposed generation is rejected afterwards.
    pub fn dispose_surface(&mut self, surface: u64) -> usize {
        let removed = self.clear_surface(surface);
        let generation = self.active_generations.remove(&surface).unwrap_or(0);
        self.last_revisions.remove(&surface);
        self.disposed.insert(surface, generation);
        removed
    }

    /// Bump the active generation and revoke old announcements. Going backwards
    /// is rejected as stale.
    pub fn advance_generation(&mut self, scene: DisplayedScene) -> Result<usize, SemanticError> {
        let surface = scene.surface.0;
        if let Some(disposed) = self.disposed.get(&surface)
            && scene.generation <= *disposed
        {
            return Err(SemanticError::SurfaceDisposed {
                surface: scene.surface,
            });
        }
        if let Some(active) = self.active_generations.get(&surface).copied() {
            if scene.generation <= active {
                return Err(SemanticError::StaleGeneration {
                    displayed: scene.generation,
                    active,
                });
            }
            let removed = self.clear_surface(surface);
            self.active_generations.insert(surface, scene.generation);
            self.last_revisions
                .insert(surface, (scene.generation, scene.revision));
            Ok(removed)
        } else {
            self.active_generations.insert(surface, scene.generation);
            self.last_revisions
                .insert(surface, (scene.generation, scene.revision));
            Ok(0)
        }
    }

    fn clear_surface(&mut self, surface: u64) -> usize {
        let before = self.queue.len();
        self.queue
            .retain(|announcement| announcement.scene.surface.0 != surface);
        if self
            .last
            .as_ref()
            .is_some_and(|announcement| announcement.scene.surface.0 == surface)
        {
            self.last = None;
        }
        before - self.queue.len()
    }
}

/// Receipt for a queued announcement. `dropped` is the total dropped count for
/// the queue, not just this revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveAnnouncementReceipt {
    pub disposition: LiveAnnouncementDisposition,
    pub queue_len: usize,
    pub dropped: u64,
}
