//! Immutable aggregate index. Uniform intervals stay implicit; changed paths
//! and at most 64-height leaves are copied, never an N-entry prefix directory.
use super::*;
const LEAF: usize = 64;

#[derive(Debug)]
pub(super) struct Index {
    len: usize,
    total: f64,
    kind: Kind,
}
#[derive(Debug)]
enum Kind {
    Uniform(f32),
    Leaf {
        heights: Box<[f32]>,
        prefix: Box<[f64]>,
    },
    Branch(Arc<Index>, Arc<Index>),
}
impl PartialEq for Index {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        if self.len != other.len || self.total != other.total {
            return false;
        }
        match (&self.kind, &other.kind) {
            (Kind::Uniform(a), Kind::Uniform(b)) => a == b,
            (Kind::Leaf { heights: a, .. }, Kind::Leaf { heights: b, .. }) => a == b,
            (Kind::Branch(a, b), Kind::Branch(c, d)) => a == c && b == d,
            _ => false,
        }
    }
}
impl Index {
    pub(super) fn uniform(len: usize, height: f32) -> Arc<Self> {
        Arc::new(Self {
            len,
            total: len as f64 * f64::from(height),
            kind: Kind::Uniform(height),
        })
    }
    pub(super) fn build(heights: &[f32]) -> Arc<Self> {
        if heights.len() <= LEAF {
            return Self::leaf(heights.to_vec());
        }
        let mid = heights.len() / 2;
        Self::join(Self::build(&heights[..mid]), Self::build(&heights[mid..]))
    }
    fn leaf(heights: Vec<f32>) -> Arc<Self> {
        if let Some(&height) = heights.first()
            && heights.iter().all(|v| *v == height)
        {
            return Self::uniform(heights.len(), height);
        }
        let mut prefix = Vec::with_capacity(heights.len() + 1);
        prefix.push(0.0);
        for &height in &heights {
            prefix.push(prefix.last().unwrap() + f64::from(height));
        }
        Arc::new(Self {
            len: heights.len(),
            total: *prefix.last().unwrap(),
            kind: Kind::Leaf {
                heights: heights.into(),
                prefix: prefix.into(),
            },
        })
    }
    fn join(left: Arc<Self>, right: Arc<Self>) -> Arc<Self> {
        if let (Kind::Uniform(a), Kind::Uniform(b)) = (&left.kind, &right.kind)
            && a == b
        {
            return Self::uniform(left.len + right.len, *a);
        }
        Arc::new(Self {
            len: left.len + right.len,
            total: left.total + right.total,
            kind: Kind::Branch(left, right),
        })
    }
    pub(super) fn len(&self) -> usize {
        self.len
    }
    pub(super) fn total(&self) -> f64 {
        self.total
    }
    pub(super) fn uniform_height(&self) -> Option<f32> {
        if let Kind::Uniform(h) = self.kind {
            Some(h)
        } else {
            None
        }
    }
    pub(super) fn height(&self, index: usize) -> f32 {
        match &self.kind {
            Kind::Uniform(h) => *h,
            Kind::Leaf { heights, .. } => heights[index],
            Kind::Branch(a, b) => {
                if index < a.len {
                    a.height(index)
                } else {
                    b.height(index - a.len)
                }
            }
        }
    }
    pub(super) fn edge(&self, index: usize) -> f64 {
        if index == self.len {
            return self.total;
        }
        match &self.kind {
            Kind::Uniform(h) => index as f64 * f64::from(*h),
            Kind::Leaf { prefix, .. } => prefix[index],
            Kind::Branch(a, b) => {
                if index <= a.len {
                    a.edge(index)
                } else {
                    a.total + b.edge(index - a.len)
                }
            }
        }
    }
    /// Last intersecting interval at an exact edge selects the following row.
    /// End beyond total returns len, useful as the exclusive window boundary.
    pub(super) fn lower_row(&self, position: f64) -> usize {
        if position >= self.total {
            return self.len;
        }
        match &self.kind {
            Kind::Uniform(h) => (position / f64::from(*h)).floor() as usize,
            Kind::Leaf { prefix, .. } => {
                prefix.partition_point(|v| *v <= position).saturating_sub(1)
            }
            Kind::Branch(a, b) => {
                if position < a.total {
                    a.lower_row(position)
                } else {
                    a.len + b.lower_row(position - a.total)
                }
            }
        }
    }
    pub(super) fn upper_row(&self, position: f64) -> usize {
        if position >= self.total {
            return self.len;
        }
        match &self.kind {
            Kind::Uniform(h) => (position / f64::from(*h)).ceil() as usize,
            Kind::Leaf { prefix, .. } => prefix.partition_point(|v| *v < position),
            Kind::Branch(a, b) => {
                if position <= a.total {
                    a.upper_row(position)
                } else {
                    a.len + b.upper_row(position - a.total)
                }
            }
        }
    }
    /// Sorted unique changes share traversal of common ancestors. Untouched
    /// intervals retain exactly the old Arc, including uniform subranges.
    pub(super) fn updated(self: &Arc<Self>, offset: usize, changes: &[(usize, f32)]) -> Arc<Self> {
        if changes.is_empty() {
            return self.clone();
        }
        if self.len <= LEAF {
            let mut heights = (0..self.len).map(|i| self.height(i)).collect::<Vec<_>>();
            for &(index, height) in changes {
                heights[index - offset] = height;
            }
            return Self::leaf(heights);
        }
        let mid = self.len / 2;
        let split = changes.partition_point(|(i, _)| *i < offset + mid);
        let (a, b) = match &self.kind {
            Kind::Uniform(h) => (Self::uniform(mid, *h), Self::uniform(self.len - mid, *h)),
            Kind::Branch(a, b) => (a.clone(), b.clone()),
            Kind::Leaf { .. } => unreachable!("bounded leaf"),
        };
        Self::join(
            a.updated(offset, &changes[..split]),
            b.updated(offset + mid, &changes[split..]),
        )
    }
}

#[cfg(test)]
mod tests;
