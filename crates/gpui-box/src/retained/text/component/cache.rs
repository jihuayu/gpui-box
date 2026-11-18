//! Bounded owner-local LRU of immutable prepared resources, not a layout cache.
use super::*;
use std::collections::{BTreeMap, HashMap};

/// Budget for the cache's accounted preparation payload. Accounting includes
/// source/family bytes, shaped arrays and glyph masks/records, but excludes
/// allocator/map overhead, shared GPUI caches, lazy wrap/chunk/GPU data and
/// resources still held by callers. This is not a process-memory byte limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextCacheLimits {
    pub max_entries: usize,
    pub max_payload_bytes: usize,
}
impl Default for TextCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 128,
            max_payload_bytes: 8 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCacheStats {
    pub hits: u64,
    /// Lookup misses, including requests that subsequently fail preparation.
    pub misses: u64,
    pub evictions: u64,
    pub bypassed: u64,
    pub font_invalidations: u64,
    pub entries: usize,
    pub payload_bytes: usize,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Group {
    family: Arc<str>,
    weight: u16,
    slant: u8,
    size: u32,
    line: u32,
    scale: u32,
    revision: u64,
}
impl Group {
    fn new(t: &TextTypography, scale: f32, revision: u64) -> Self {
        Self {
            family: t.font.family.clone(),
            weight: t.font.weight,
            slant: match t.font.slant {
                FontSlant::Normal => 0,
                FontSlant::Italic => 1,
                FontSlant::Oblique => 2,
            },
            size: t.font_size.to_bits(),
            line: t.line_height.to_bits(),
            scale: scale.to_bits(),
            revision,
        }
    }
}
struct Entry {
    resource: TextResource,
    age: u64,
    bytes: usize,
}
pub(super) struct PreparationCache {
    groups: BTreeMap<Group, HashMap<Arc<str>, Entry>>,
    // One recency record per entry; no growing hit log or O(capacity) scan.
    ages: BTreeMap<u64, (Group, Arc<str>)>,
    next_age: u64,
    epoch: usize,
    limits: TextCacheLimits,
    stats: TextCacheStats,
}
impl PreparationCache {
    pub(super) fn new(epoch: usize) -> Self {
        Self {
            groups: BTreeMap::new(),
            ages: BTreeMap::new(),
            next_age: 0,
            epoch,
            limits: TextCacheLimits::default(),
            stats: TextCacheStats::default(),
        }
    }
    pub(super) fn stats(&self) -> TextCacheStats {
        self.stats
    }
    pub(super) fn clear(&mut self) {
        self.groups.clear();
        self.ages.clear();
        self.next_age = 0;
        self.stats.entries = 0;
        self.stats.payload_bytes = 0;
    }
    pub(super) fn set_limits(&mut self, limits: TextCacheLimits) {
        self.limits = limits;
        while self.stats.entries > limits.max_entries
            || self.stats.payload_bytes > limits.max_payload_bytes
        {
            self.evict();
        }
    }
    fn age(&mut self) -> u64 {
        if self.next_age == u64::MAX {
            self.clear();
        }
        let age = self.next_age;
        self.next_age += 1;
        age
    }
    pub(super) fn get(
        &mut self,
        text: &str,
        typography: &TextTypography,
        scale: f32,
        revision: u64,
        epoch: usize,
    ) -> Option<TextResource> {
        if self.epoch != epoch {
            self.clear();
            self.epoch = epoch;
            self.stats.font_invalidations = self.stats.font_invalidations.saturating_add(1);
        }
        if self.limits.max_entries == 0 || self.limits.max_payload_bytes == 0 {
            self.stats.misses = self.stats.misses.saturating_add(1);
            return None;
        }
        let age = self.age();
        let group = Group::new(typography, scale, revision);
        if let Some((key, entry)) = self
            .groups
            .get(&group)
            .and_then(|texts| texts.get_key_value(text))
        {
            let key = key.clone();
            let resource = entry.resource.clone();
            let old = entry.age;
            self.ages.remove(&old);
            self.ages.insert(age, (group.clone(), key.clone()));
            self.groups
                .get_mut(&group)
                .unwrap()
                .get_mut(key.as_ref())
                .unwrap()
                .age = age;
            self.stats.hits = self.stats.hits.saturating_add(1);
            Some(resource)
        } else {
            self.stats.misses = self.stats.misses.saturating_add(1);
            None
        }
    }
    fn evict(&mut self) {
        let (_, (group, text)) = self
            .ages
            .pop_first()
            .expect("nonempty cache exceeds limits");
        let entries = self.groups.get_mut(&group).unwrap();
        let removed = entries.remove(text.as_ref()).unwrap();
        if entries.is_empty() {
            self.groups.remove(&group);
        }
        self.stats.entries -= 1;
        self.stats.payload_bytes -= removed.bytes;
        self.stats.evictions = self.stats.evictions.saturating_add(1);
    }
    pub(super) fn insert(
        &mut self,
        text: &str,
        typography: &TextTypography,
        scale: f32,
        revision: u64,
        epoch: usize,
        resource: TextResource,
    ) {
        // A font mutation during shaping must not enter another epoch's cache.
        if self.epoch != epoch || resource.paragraph().font_epoch() != epoch {
            return;
        }
        if self.limits.max_entries == 0 || self.limits.max_payload_bytes == 0 {
            self.stats.bypassed = self.stats.bypassed.saturating_add(1);
            return;
        }
        let bytes = resource
            .preparation_payload_bytes()
            .saturating_add(text.len())
            .saturating_add(typography.font.family.len());
        if bytes > self.limits.max_payload_bytes {
            self.stats.bypassed = self.stats.bypassed.saturating_add(1);
            return;
        }
        while self.stats.entries >= self.limits.max_entries
            || self.stats.payload_bytes > self.limits.max_payload_bytes - bytes
        {
            self.evict();
        }
        let age = self.age();
        let group = Group::new(typography, scale, revision);
        let text: Arc<str> = text.into();
        self.groups.entry(group.clone()).or_default().insert(
            text.clone(),
            Entry {
                resource,
                age,
                bytes,
            },
        );
        self.ages.insert(age, (group, text));
        self.stats.entries += 1;
        self.stats.payload_bytes += bytes;
    }
}
