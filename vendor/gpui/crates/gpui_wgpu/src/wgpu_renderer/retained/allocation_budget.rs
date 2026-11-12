//! Allocation payload accounting. A shared lease counts once across all flights.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct Ledger {
    bytes: AtomicU64,
    peak: AtomicU64,
    count: AtomicU64,
}

#[derive(Clone, Default)]
pub(super) struct AllocationBudget(Arc<Ledger>);

pub(super) struct Allocation {
    bytes: u64,
    ledger: Arc<Ledger>,
}

impl AllocationBudget {
    pub fn reserve(&self, bytes: u64, limit: u64) -> Option<Arc<Allocation>> {
        if bytes == 0 {
            return None;
        }
        let previous = self
            .0
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= limit)
            })
            .ok()?;
        self.0.count.fetch_add(1, Ordering::Relaxed);
        self.0.peak.fetch_max(previous + bytes, Ordering::Relaxed);
        Some(Arc::new(Allocation {
            bytes,
            ledger: self.0.clone(),
        }))
    }
    pub fn bytes(&self) -> u64 {
        self.0.bytes.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> u64 {
        self.0.peak.load(Ordering::Acquire)
    }
    pub fn count(&self) -> u64 {
        self.0.count.load(Ordering::Acquire)
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.ledger.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.ledger.count.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_flights_count_unique_allocations_and_retired_epochs_release_at_last_drop() {
        let budget = AllocationBudget::default();
        let first = budget.reserve(64, 128).unwrap();
        let flights = (0..8).map(|_| first.clone()).collect::<Vec<_>>();
        assert_eq!((budget.bytes(), budget.count()), (64, 1));
        let second = budget.reserve(64, 128).unwrap();
        assert!(budget.reserve(1, 128).is_none());
        assert_eq!(
            (budget.bytes(), budget.peak(), budget.count()),
            (128, 128, 2)
        );
        drop(first);
        assert_eq!(budget.bytes(), 128);
        drop(flights);
        assert_eq!((budget.bytes(), budget.count()), (64, 1));
        let third = budget.reserve(32, 96).unwrap();
        assert_eq!(budget.bytes(), 96);
        drop(second);
        drop(third);
        assert_eq!((budget.bytes(), budget.peak(), budget.count()), (0, 128, 0));
    }
    #[test]
    fn rejection_and_overflow_leave_ledger_unchanged_and_lease_outlives_owner() {
        let budget = AllocationBudget::default();
        assert!(budget.reserve(0, u64::MAX).is_none());
        assert!(budget.reserve(1, 0).is_none());
        let lease = budget.reserve(u64::MAX - 1, u64::MAX).unwrap();
        assert!(budget.reserve(2, u64::MAX).is_none());
        assert_eq!((budget.bytes(), budget.count()), (u64::MAX - 1, 1));
        let observer = budget.clone();
        drop(budget);
        drop(lease);
        assert_eq!((observer.bytes(), observer.count()), (0, 0));
    }
}
