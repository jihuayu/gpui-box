//! Dense subtrees remain contiguous; sparse point edits retain duplicate guards.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Candidates {
    copying: Option<BTreeMap<usize, PaintItem>>,
    owned: Vec<(usize, PaintItem)>,
    ranges: Vec<std::ops::Range<usize>>,
    points: BTreeSet<usize>,
}
impl Candidates {
    pub fn new(copying: bool, ranges: Vec<std::ops::Range<usize>>) -> Self {
        Self {
            copying: copying.then(BTreeMap::new),
            owned: vec![],
            ranges,
            points: BTreeSet::new(),
        }
    }
    pub fn contains(&self, index: usize) -> bool {
        if let Some(items) = &self.copying {
            return items.contains_key(&index);
        }
        let next = self.ranges.partition_point(|r| r.end <= index);
        self.ranges.get(next).is_some_and(|r| r.contains(&index)) || self.points.contains(&index)
    }
    pub fn insert(&mut self, index: usize, item: PaintItem, point: bool) {
        if let Some(items) = &mut self.copying {
            items.insert(index, item);
        } else {
            if point {
                assert!(self.points.insert(index));
            }
            self.owned.push((index, item));
        }
    }
    pub fn into_sorted(self) -> Box<dyn Iterator<Item = (usize, PaintItem)>> {
        if let Some(items) = self.copying {
            return Box::new(items.into_iter());
        }
        let mut items = self.owned;
        items.sort_unstable_by_key(|(index, _)| *index);
        debug_assert!(items.windows(2).all(|p| p[0].0 < p[1].0));
        Box::new(items.into_iter())
    }
}
