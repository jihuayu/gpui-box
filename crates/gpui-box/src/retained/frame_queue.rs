//! Latest-visual-frame mailbox. Lifecycle/resource commands belong elsewhere.
use super::*;
use std::sync::Mutex;

/// A single pending immutable local snapshot. Same source/revision ordering as
/// the resolved mailbox, without forcing world-geometry materialization.
#[derive(Default)]
pub struct LocalFrameQueue {
    state: Mutex<LocalState>,
}
#[derive(Default)]
struct LocalState {
    source: Option<u64>,
    revision: Option<u64>,
    pending: Option<local::LocalFrame>,
    stats: FrameQueueStats,
}
impl LocalFrameQueue {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn publish(&self, frame: local::LocalFrame) -> Result<bool, Error> {
        let mut state = self.state.lock().map_err(|_| Error::FrameQueuePoisoned)?;
        if state.source.is_some_and(|tree| tree != frame.tree()) {
            return Err(Error::InvalidFrame);
        }
        if state
            .revision
            .is_some_and(|revision| revision > frame.revision())
        {
            return Err(Error::StaleFrame);
        }
        if state.revision == Some(frame.revision()) {
            state.stats.equal += 1;
            return Ok(false);
        }
        let wake = state.pending.is_none();
        state.source = Some(frame.tree());
        state.revision = Some(frame.revision());
        state.pending = Some(frame);
        state.stats.published += 1;
        state.stats.coalesced += u64::from(!wake);
        state.stats.pending = 1;
        Ok(wake)
    }
    pub fn take_latest(&self) -> Result<Option<local::LocalFrame>, Error> {
        let mut state = self.state.lock().map_err(|_| Error::FrameQueuePoisoned)?;
        let frame = state.pending.take();
        if frame.is_some() {
            state.stats.consumed += 1;
            state.stats.pending = 0;
        }
        Ok(frame)
    }
    pub fn stats(&self) -> Result<FrameQueueStats, Error> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::FrameQueuePoisoned)?
            .stats)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameQueueStats {
    pub published: u64,
    pub coalesced: u64,
    pub consumed: u64,
    pub equal: u64,
    pub pending: usize,
}

#[derive(Default)]
struct State {
    source: Option<u64>,
    latest_revision: Option<u64>,
    pending: Option<FramePacket>,
    stats: FrameQueueStats,
}

/// One immutable UI tree source, at most one pending complete visual packet.
/// Shared across producer/consumer threads; it never exposes a mutable UiTree.
/// A new tree/window uses a new queue. GPU completion still owns in-flight data.
#[derive(Default)]
pub struct FrameQueue {
    state: Mutex<State>,
}

impl FrameQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return true only for empty -> pending: host should wake the consumer once.
    /// Later frames replace pending output. They are complete snapshots, allowing
    /// the renderer to detect a skipped delta base and safely perform full resync.
    pub fn publish(&self, frame: FramePacket) -> Result<bool, Error> {
        if frame
            .base_revision
            .is_some_and(|base| base >= frame.revision)
        {
            return Err(Error::InvalidFrame);
        }
        let mut state = self.state.lock().map_err(|_| Error::FrameQueuePoisoned)?;
        if state.source.is_some_and(|source| source != frame.tree) {
            return Err(Error::InvalidFrame);
        }
        if state
            .latest_revision
            .is_some_and(|revision| revision > frame.revision)
        {
            return Err(Error::StaleFrame);
        }
        if state.latest_revision == Some(frame.revision) {
            state.stats.equal += 1;
            return Ok(false);
        }
        let wake = state.pending.is_none();
        state.source = Some(frame.tree);
        state.latest_revision = Some(frame.revision);
        state.stats.published += 1;
        state.stats.coalesced += u64::from(!wake);
        state.stats.pending = 1;
        state.pending = Some(frame);
        Ok(wake)
    }

    /// Empty queues do not block. Call in response to host wake/present demand.
    pub fn take_latest(&self) -> Result<Option<FramePacket>, Error> {
        let mut state = self.state.lock().map_err(|_| Error::FrameQueuePoisoned)?;
        let frame = state.pending.take();
        if frame.is_some() {
            state.stats.consumed += 1;
            state.stats.pending = 0;
        }
        Ok(frame)
    }

    pub fn stats(&self) -> Result<FrameQueueStats, Error> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::FrameQueuePoisoned)?
            .stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_coalesces_complete_snapshots_without_reviving_stale_sources() {
        let queue = FrameQueue::new();
        let mut tree = UiTree::new(Style::default(), 0xff8844ff).unwrap();
        let before = tree.frame([80.0, 40.0]).unwrap();
        assert!(queue.publish(before.clone()).unwrap());
        for color in [0x44bb88ff, 0x4488ffff] {
            let mut tx = Transaction::default();
            tx.color(tree.root(), color);
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )
            .unwrap();
            assert!(!queue.publish(tree.frame([80.0, 40.0]).unwrap()).unwrap());
        }
        assert_eq!(
            queue.stats().unwrap(),
            FrameQueueStats {
                published: 3,
                coalesced: 2,
                pending: 1,
                ..Default::default()
            }
        );
        let latest = queue.take_latest().unwrap().unwrap();
        assert_eq!(latest.items[0].fill_color, 0x4488ffff);
        assert_eq!(before.items[0].fill_color, 0xff8844ff);
        assert_eq!(latest.base_revision, Some(before.revision + 1));
        assert!(queue.take_latest().unwrap().is_none());
        assert!(!queue.publish(latest.clone()).unwrap());
        assert_eq!(queue.publish(before), Err(Error::StaleFrame));
        let mut other = UiTree::new(Style::default(), 0).unwrap();
        assert_eq!(
            queue.publish(other.frame([80.0, 40.0]).unwrap()),
            Err(Error::InvalidFrame)
        );
        assert_eq!(queue.stats().unwrap().pending, 0);
    }

    #[test]
    fn owner_thread_publishes_without_sharing_mutable_ui_and_consumer_gets_final_frame() {
        let queue = Arc::new(FrameQueue::new());
        let publisher = queue.clone();
        let task = std::thread::spawn(move || {
            let mut tree = UiTree::new(Style::default(), 0).unwrap();
            for i in 1..=100 {
                let mut tx = Transaction::default();
                tx.color(tree.root(), i);
                tree.commit(
                    tx,
                    BackendCapabilities {
                        solid_color_patch: true,
                    },
                )
                .unwrap();
                publisher
                    .publish(tree.frame([80.0, 40.0]).unwrap())
                    .unwrap();
            }
        });
        task.join().unwrap();
        let frame = queue.take_latest().unwrap().unwrap();
        assert_eq!(frame.items[0].fill_color, 100);
        assert_eq!(
            (
                queue.stats().unwrap().published,
                queue.stats().unwrap().coalesced
            ),
            (100, 99)
        );
        assert_eq!(frame.items.len(), 1);
    }
}
