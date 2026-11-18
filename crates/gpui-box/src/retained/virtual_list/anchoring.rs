//! Pure key-based scroll rebasing plus atomic ordinary source adoption.
use super::*;
use std::collections::BTreeMap;

pub(super) struct Keys {
    pub(super) order: Vec<u64>,
    pub(super) positions: BTreeMap<u64, usize>,
}
impl Keys {
    pub(super) fn new(keys: impl IntoIterator<Item = u64>, length: usize) -> Result<Self, Error> {
        let order: Vec<_> = keys.into_iter().collect();
        if order.len() != length {
            return Err(Error::InvalidVirtualList);
        }
        let mut positions = BTreeMap::new();
        for (index, key) in order.iter().copied().enumerate() {
            if positions.insert(key, index).is_some() {
                return Err(Error::DuplicateKey);
            }
        }
        Ok(Self { order, positions })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VirtualListScrollPolicy {
    /// Preserve the requested numerical offset, clamped to new content.
    KeepOffset,
    /// Preserve the first intersecting row and its visible pixel position.
    /// If removed, prefer the following surviving old key, then preceding.
    #[default]
    KeepVisible,
    /// Follow new end only when old effective offset was within tolerance of
    /// old end. Otherwise use KeepVisible; appending never pulls a reader down.
    FollowEnd { tolerance: f32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualListRebaseReason {
    KeptOffset,
    KeptKey,
    FollowingKey,
    PrecedingKey,
    NoSurvivingKey,
    FollowedEnd,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualListRebase {
    pub offset: [f32; 2],
    pub key: Option<u64>,
    pub reason: VirtualListRebaseReason,
    /// Bounds may prevent preserving an anchor's previous pixel position.
    pub clamped: bool,
}
impl<C: Component> VirtualListItems<C> {
    /// Plan from current source values, never from an earlier queued frame.
    /// KeepVisible/FollowEnd require keyed datasets, including empty datasets.
    /// No payload factory, layout or side effect runs here.
    pub fn rebase(
        &self,
        new: &Self,
        offset: [f32; 2],
        old_viewport: f32,
        new_viewport: f32,
        policy: VirtualListScrollPolicy,
    ) -> Result<VirtualListRebase, Error> {
        if offset.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(Error::InvalidScroll);
        }
        let old_max = self.metrics.maximum(old_viewport)?;
        let new_max = new.metrics.maximum(new_viewport)?;
        let used = offset[1].min(old_max);
        let result = |desired: f64, key, reason| {
            let bounded = desired.clamp(0.0, f64::from(new_max));
            VirtualListRebase {
                offset: [0.0, bounded as f32],
                key,
                reason,
                clamped: desired != bounded,
            }
        };
        if policy == VirtualListScrollPolicy::KeepOffset {
            return Ok(result(
                f64::from(offset[1]),
                None,
                VirtualListRebaseReason::KeptOffset,
            ));
        }
        let before = self.keys.as_ref().ok_or(Error::InvalidVirtualList)?;
        let after = new.keys.as_ref().ok_or(Error::InvalidVirtualList)?;
        if let VirtualListScrollPolicy::FollowEnd { tolerance } = policy {
            if !tolerance.is_finite() || tolerance < 0.0 {
                return Err(Error::InvalidVirtualList);
            }
            if old_max - used <= tolerance {
                return Ok(result(
                    f64::from(new_max),
                    None,
                    VirtualListRebaseReason::FollowedEnd,
                ));
            }
        }
        if before.order.is_empty() || after.order.is_empty() || old_viewport == 0.0 {
            return Ok(result(
                f64::from(used),
                None,
                VirtualListRebaseReason::NoSurvivingKey,
            ));
        }
        let first = self
            .metrics
            .lower_row(f64::from(used))
            .min(self.metrics.len() - 1);
        let anchor = before.order[first];
        if let Some(&index) = after.positions.get(&anchor) {
            // A shorter anchor retains as much of its original inset as fits,
            // keeping this key visible rather than silently switching to next.
            let height = new.metrics.edge(index + 1) - new.metrics.edge(index);
            let previous_inset = f64::from(used) - self.metrics.edge(first);
            let inset = previous_inset.min(f64::from((height as f32).next_down().max(0.0)));
            let mut output = result(
                new.metrics.edge(index) + inset,
                Some(anchor),
                VirtualListRebaseReason::KeptKey,
            );
            let end = new.metrics.edge(index + 1) as f32;
            if inset > 0.0 && output.offset[1] >= end && end > 0.0 {
                output.offset[1] = end.next_down().min(new_max);
                output.clamped = true;
            }
            output.clamped |= inset != previous_inset;
            return Ok(output);
        }
        let fallback = (first + 1..before.order.len())
            .map(|i| (i, VirtualListRebaseReason::FollowingKey))
            .chain(
                (0..first)
                    .rev()
                    .map(|i| (i, VirtualListRebaseReason::PrecedingKey)),
            );
        for (old_index, reason) in fallback {
            let key = before.order[old_index];
            if let Some(&index) = after.positions.get(&key) {
                let previous_screen = self.metrics.edge(old_index) - f64::from(used);
                return Ok(result(
                    new.metrics.edge(index) - previous_screen,
                    Some(key),
                    reason,
                ));
            }
        }
        Ok(result(
            f64::from(used),
            None,
            VirtualListRebaseReason::NoSurvivingKey,
        ))
    }
}
impl ReactiveTransaction<'_> {
    /// Replace dataset and source scroll as one existing source transaction.
    /// Derived readers/producers see the complete new tuple; failure rolls back
    /// both writes. A newer wheel/programmatic source value is read at invocation.
    /// Viewport arguments describe the old/new committed data transition; update
    /// any viewport source in this same outer transaction when it also changes.
    pub fn rebase_virtual_list<C: Component>(
        &mut self,
        items: Signal<VirtualListItems<C>>,
        scroll: Signal<[f32; 2]>,
        new: VirtualListItems<C>,
        old_viewport: f32,
        new_viewport: f32,
        policy: VirtualListScrollPolicy,
    ) -> Result<VirtualListRebase, Error> {
        self.transaction(|tx| {
            let before = tx.read(items)?.clone();
            let offset = *tx.read(scroll)?;
            let rebase = before.rebase(&new, offset, old_viewport, new_viewport, policy)?;
            tx.set(items, new)?;
            tx.set(scroll, rebase.offset)?;
            Ok(rebase)
        })
    }
}

#[cfg(test)]
mod tests;
