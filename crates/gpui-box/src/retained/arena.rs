//! Stable CPU allocation table for GPU instance records. Retirement is driven by
//! completion serials from the backend that owns the corresponding allocations.

#[derive(Default)]
struct DirtySlots {
    words: Vec<u64>,
    listed: Vec<bool>,
    touched: Vec<usize>,
}
impl DirtySlots {
    fn insert(&mut self, slot: u32) {
        let word = slot as usize / 64;
        if self.words.len() <= word {
            self.words.resize(word + 1, 0);
            self.listed.resize(word + 1, false);
        }
        if !self.listed[word] {
            self.listed[word] = true;
            self.touched.push(word);
        }
        self.words[word] |= 1 << (slot % 64);
    }
    fn remove(&mut self, slot: u32) {
        if let Some(word) = self.words.get_mut(slot as usize / 64) {
            *word &= !(1 << (slot % 64));
        }
    }
    fn indices(&self) -> impl Iterator<Item = u32> + '_ {
        let mut words = self.touched.clone();
        words.sort_unstable();
        words.into_iter().flat_map(|index| {
            let mut bits = self.words[index];
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros();
                bits &= bits - 1;
                Some(index as u32 * 64 + bit)
            })
        })
    }
    fn clear(&mut self) {
        for index in self.touched.drain(..) {
            self.words[index] = 0;
            self.listed[index] = false;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstanceHandle {
    pub slot: u32,
    generation: u64,
    arena: u64,
}

struct Slot<T> {
    generation: u64,
    value: Option<T>,
    retired_at: Option<u64>,
}

pub struct InstanceArena<T> {
    identity: u64,
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    dirty: DirtySlots,
    retired: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct InstanceWrite<T> {
    pub start: u32,
    pub values: Vec<T>,
}

impl<T: Clone + PartialEq> Default for InstanceArena<T> {
    fn default() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("instance arena identities exhausted");
        Self {
            identity,
            slots: vec![],
            free: vec![],
            dirty: DirtySlots::default(),
            retired: vec![],
        }
    }
}

impl<T: Clone + PartialEq> InstanceArena<T> {
    pub fn allocate(&mut self, value: T) -> InstanceHandle {
        let slot = self.free.pop().unwrap_or_else(|| {
            let slot = u32::try_from(self.slots.len()).expect("instance arena capacity exhausted");
            self.slots.push(Slot {
                generation: 0,
                value: None,
                retired_at: None,
            });
            slot
        });
        let entry = &mut self.slots[slot as usize];
        entry.value = Some(value);
        self.dirty.insert(slot);
        InstanceHandle {
            slot,
            generation: entry.generation,
            arena: self.identity,
        }
    }

    fn validate(&self, handle: InstanceHandle) -> bool {
        handle.arena == self.identity
            && self.slots.get(handle.slot as usize).is_some_and(|slot| {
                slot.generation == handle.generation
                    && slot.retired_at.is_none()
                    && slot.value.is_some()
            })
    }

    pub fn update(&mut self, handle: InstanceHandle, value: T) -> Result<bool, &'static str> {
        if !self.validate(handle) {
            return Err("stale or foreign instance handle");
        }
        let entry = &mut self.slots[handle.slot as usize];
        if entry.value.as_ref() == Some(&value) {
            return Ok(false);
        }
        entry.value = Some(value);
        self.dirty.insert(handle.slot);
        Ok(true)
    }

    pub fn retire(
        &mut self,
        handle: InstanceHandle,
        last_submission: u64,
    ) -> Result<(), &'static str> {
        if !self.validate(handle) {
            return Err("stale or foreign instance handle");
        }
        self.slots[handle.slot as usize].retired_at = Some(last_submission);
        self.dirty.remove(handle.slot);
        self.retired.push(handle.slot);
        Ok(())
    }

    pub fn collect(&mut self, completed: u64) {
        self.retired.retain(|index| {
            let slot = &mut self.slots[*index as usize];
            if slot.retired_at.is_some_and(|serial| serial <= completed) {
                slot.value = None;
                if let Some(next) = slot.generation.checked_add(1) {
                    slot.generation = next;
                    self.free.push(*index);
                }
                // An exhausted slot stays retired permanently.
                slot.retired_at = None;
                false
            } else {
                true
            }
        });
    }

    pub fn capacity(&self) -> u32 {
        self.slots.len() as u32
    }

    /// Adjacent dirty slots are merged. Only successful submit acknowledges them.
    pub fn writes(&self) -> Vec<InstanceWrite<T>> {
        let mut writes: Vec<InstanceWrite<T>> = Vec::new();
        for index in self.dirty.indices() {
            let value = self.slots[index as usize]
                .value
                .as_ref()
                .expect("dirty live slot")
                .clone();
            if let Some(last) = writes
                .last_mut()
                .filter(|last| last.start + last.values.len() as u32 == index)
            {
                last.values.push(value);
            } else {
                writes.push(InstanceWrite {
                    start: index,
                    values: vec![value],
                });
            }
        }
        writes
    }

    pub fn acknowledge(&mut self) {
        self.dirty.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_dirty_ranges_match_ordered_selection_across_retirement_and_reuse() {
        let mut arena = InstanceArena::default();
        let handles: Vec<_> = (0..257).map(|i| arena.allocate(i)).collect();
        arena.acknowledge();
        let mut expected = std::collections::BTreeMap::new();
        for step in 0..800 {
            let i = (step * 73) % handles.len();
            let value = 1000 + step;
            arena.update(handles[i], value).unwrap();
            expected.insert(i as u32, value);
        }
        for &i in &[0, 63, 64, 127, 128, 256] {
            arena.retire(handles[i], 4).unwrap();
            expected.remove(&(i as u32));
        }
        let flattened = |arena: &InstanceArena<usize>| {
            arena
                .writes()
                .into_iter()
                .flat_map(|w| {
                    w.values
                        .into_iter()
                        .enumerate()
                        .map(move |(i, v)| (w.start + i as u32, v))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(flattened(&arena), expected);
        assert_eq!(flattened(&arena), expected); // unacknowledged retry
        arena.collect(4);
        for i in 0..6 {
            let handle = arena.allocate(2000 + i);
            expected.insert(handle.slot, 2000 + i);
        }
        assert_eq!(flattened(&arena), expected);
        arena.acknowledge();
        assert!(arena.writes().is_empty());
        // A cleared then reinserted word remains listed once until ack.
        arena.update(handles[65], 3000).unwrap();
        assert_eq!(arena.writes()[0].values, vec![3000]);
    }
    #[test]
    fn retired_slots_wait_for_completion_and_old_handles_never_revive() {
        let mut arena = InstanceArena::default();
        let old = arena.allocate(1);
        arena.acknowledge();
        arena.retire(old, 3).unwrap();
        arena.collect(2);
        let b = arena.allocate(2);
        assert_ne!(old.slot, b.slot);
        arena.collect(3);
        let replacement = arena.allocate(3);
        assert_eq!(old.slot, replacement.slot);
        assert!(arena.update(old, 4).is_err());
        assert!(InstanceArena::default().update(replacement, 4).is_err());
    }
    #[test]
    fn batch_ranges_merge_without_losing_unacknowledged_writes() {
        let mut arena = InstanceArena::default();
        let handles: Vec<_> = (0..4).map(|i| arena.allocate(i)).collect();
        assert_eq!(arena.writes()[0].values, vec![0, 1, 2, 3]);
        arena.acknowledge();
        assert!(!arena.update(handles[0], 0).unwrap());
        arena.update(handles[1], 10).unwrap();
        arena.update(handles[2], 20).unwrap();
        let batch = arena.writes();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].start, 1);
        assert_eq!(batch[0].values, vec![10, 20]);
        assert_eq!(arena.writes()[0].values, vec![10, 20]);
        arena.acknowledge();
        assert!(arena.writes().is_empty());
    }
}
