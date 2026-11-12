//! Bounded message-ID deduplication with a deterministic time-to-live.
//!
//! The journal suppresses duplicates across in-memory repeats, simulated
//! restarts and `TerminatedColdLaunch` wakes. It is persisted by
//! [`crate::PushStore`] inside a versioned SHA-256 envelope; a corrupt payload is
//! quarantined and recovered as an empty journal rather than panicking.

use crate::model::{
    DEFAULT_DEDUP_CAPACITY, DEFAULT_DEDUP_TTL_MS, MAX_DEDUP_CAPACITY, MAX_DEDUP_TTL_MS,
    PushMessageId,
};
use crate::store::PUSH_FORMAT_VERSION;
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

/// One recorded message identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DedupEntry {
    pub message_id: PushMessageId,
    pub first_seen_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Bounded, versioned message-ID journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DedupJournal {
    pub format_version: u32,
    pub capacity: usize,
    pub ttl_ms: u64,
    pub entries: Vec<DedupEntry>,
}

impl DedupJournal {
    pub fn new(capacity: usize, ttl_ms: u64) -> Result<Self, SystemError> {
        if capacity == 0 || capacity > MAX_DEDUP_CAPACITY {
            return Err(invalid(
                "Message-ID journal capacity is outside its supported bounds",
            ));
        }
        if ttl_ms == 0 || ttl_ms > MAX_DEDUP_TTL_MS {
            return Err(invalid(
                "Message-ID journal TTL is outside its supported bounds",
            ));
        }
        Ok(Self {
            format_version: PUSH_FORMAT_VERSION,
            capacity,
            ttl_ms,
            entries: Vec::new(),
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn ttl_ms(&self) -> u64 {
        self.ttl_ms
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[DedupEntry] {
        &self.entries
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// First-seen timestamp of a live (unexpired) record.
    pub fn first_seen(&self, message_id: &PushMessageId, now_ms: u64) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| entry.message_id == *message_id && entry.expires_at_ms > now_ms)
            .map(|entry| entry.first_seen_at_ms)
    }

    /// True when a live record already exists.
    pub fn contains(&self, message_id: &PushMessageId, now_ms: u64) -> bool {
        self.first_seen(message_id, now_ms).is_some()
    }

    /// Record one identity. Returns `false` when a live duplicate already exists.
    ///
    /// Expired entries are pruned first, then the oldest entry is evicted when
    /// the journal is at capacity, so the operation is deterministic.
    pub fn record(&mut self, message_id: PushMessageId, now_ms: u64) -> bool {
        self.prune(now_ms);
        if self.contains(&message_id, now_ms) {
            return false;
        }
        self.entries.push(DedupEntry {
            message_id,
            first_seen_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(self.ttl_ms),
        });
        self.enforce_capacity();
        true
    }

    /// Drop every entry whose TTL elapsed at or before `now_ms`.
    pub fn prune(&mut self, now_ms: u64) {
        self.entries.retain(|entry| entry.expires_at_ms > now_ms);
    }

    /// Apply the bounded capacity using oldest-first, then identity, eviction.
    pub fn enforce_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            let Some(index) = self
                .entries
                .iter()
                .enumerate()
                .min_by(|(_, left), (_, right)| {
                    left.first_seen_at_ms
                        .cmp(&right.first_seen_at_ms)
                        .then_with(|| left.message_id.cmp(&right.message_id))
                })
                .map(|(index, _)| index)
            else {
                break;
            };
            self.entries.remove(index);
        }
    }

    /// Validate a journal loaded from durable storage or supplied by a host.
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.format_version != PUSH_FORMAT_VERSION {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Message-ID journal has an unsupported schema version",
            ));
        }
        if self.capacity == 0 || self.capacity > MAX_DEDUP_CAPACITY {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Message-ID journal capacity is invalid",
            ));
        }
        if self.ttl_ms == 0 || self.ttl_ms > MAX_DEDUP_TTL_MS {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Message-ID journal TTL is invalid",
            ));
        }
        if self.entries.len() > self.capacity {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Message-ID journal exceeds its capacity",
            ));
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.expires_at_ms <= entry.first_seen_at_ms {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Message-ID journal entry has an invalid lifetime",
                ));
            }
            if self.entries[..index]
                .iter()
                .any(|previous| previous.message_id == entry.message_id)
            {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Message-ID journal repeats a message identity",
                ));
            }
        }
        Ok(())
    }
}

impl Default for DedupJournal {
    fn default() -> Self {
        Self {
            format_version: PUSH_FORMAT_VERSION,
            capacity: DEFAULT_DEDUP_CAPACITY,
            ttl_ms: DEFAULT_DEDUP_TTL_MS,
            entries: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> PushMessageId {
        PushMessageId::new(value).expect("message id")
    }

    #[test]
    fn suppresses_live_duplicates_and_forgets_expired_ones() {
        let mut journal = DedupJournal::new(4, 1_000).expect("journal");
        assert!(journal.record(id("m-1"), 100));
        assert!(!journal.record(id("m-1"), 200));
        assert!(journal.contains(&id("m-1"), 200));
        assert!(journal.record(id("m-1"), 1_100));
        assert_eq!(journal.len(), 1);
    }

    #[test]
    fn evicts_the_oldest_entry_at_capacity() {
        let mut journal = DedupJournal::new(2, 10_000).expect("journal");
        assert!(journal.record(id("m-1"), 100));
        assert!(journal.record(id("m-2"), 200));
        assert!(journal.record(id("m-3"), 300));
        assert_eq!(journal.len(), 2);
        assert!(!journal.contains(&id("m-1"), 300));
        assert!(journal.contains(&id("m-3"), 300));
    }

    #[test]
    fn rejects_out_of_bounds_configuration_and_invalid_records() {
        assert!(DedupJournal::new(0, 1_000).is_err());
        assert!(DedupJournal::new(1, 0).is_err());
        let mut journal = DedupJournal::new(2, 1_000).expect("journal");
        journal.entries.push(DedupEntry {
            message_id: id("m-1"),
            first_seen_at_ms: 10,
            expires_at_ms: 10,
        });
        assert!(journal.validate().is_err());
    }
}
