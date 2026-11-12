//! AVL paths are copied only while shared with another immutable snapshot.
use super::*;
use std::ops::Index;

type Link<T> = Option<Arc<Entry<T>>>;

#[derive(Clone, Debug)]
struct Entry<T> {
    key: NodeId,
    value: PropertyNode<T>,
    left: Link<T>,
    right: Link<T>,
    height: u8,
}

/// Ordered sparse property storage with O(1) snapshot cloning and O(log N)
/// lookup/update. Mutating a snapshot copies shared search/rotation paths;
/// unchanged subtrees and values remain owned by their existing Arcs.
#[derive(Clone, Debug)]
pub struct PropertyMap<T> {
    root: Link<T>,
    len: usize,
}

impl<T> Default for PropertyMap<T> {
    fn default() -> Self {
        Self { root: None, len: 0 }
    }
}

impl<T> PropertyMap<T> {
    /// Full extraction emits each live identity once. Sort once, then build a
    /// balanced snapshot without repeated lookups, rotations or COW checks.
    pub(super) fn from_entries(mut entries: Vec<(NodeId, PropertyNode<T>)>) -> Self {
        entries.sort_unstable_by_key(|(id, _)| *id);
        let len = entries.len();
        fn build<T>(
            iter: &mut impl Iterator<Item = (NodeId, PropertyNode<T>)>,
            len: usize,
        ) -> Link<T> {
            if len == 0 {
                return None;
            }
            let left = build(iter, len / 2);
            let (key, value) = iter.next().expect("full property entries");
            let right = build(iter, len - len / 2 - 1);
            let height = 1 + height(&left).max(height(&right));
            Some(Arc::new(Entry {
                key,
                value,
                left,
                right,
                height,
            }))
        }
        Self {
            root: build(&mut entries.into_iter(), len),
            len,
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn clear(&mut self) {
        self.root = None;
        self.len = 0;
    }
    pub fn get(&self, key: &NodeId) -> Option<&PropertyNode<T>> {
        let mut cursor = self.root.as_deref();
        while let Some(entry) = cursor {
            match key.cmp(&entry.key) {
                std::cmp::Ordering::Less => cursor = entry.left.as_deref(),
                std::cmp::Ordering::Greater => cursor = entry.right.as_deref(),
                std::cmp::Ordering::Equal => return Some(&entry.value),
            }
        }
        None
    }
    pub fn contains_key(&self, key: &NodeId) -> bool {
        self.get(key).is_some()
    }
    /// Shared identity is sufficient for equality, including empty maps. This
    /// checks the actual owned root, never a caller-provided revision counter.
    pub fn shares_root(&self, other: &Self) -> bool {
        match (&self.root, &other.root) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
    pub fn iter(&self) -> PropertyIter<'_, T> {
        let mut iter = PropertyIter {
            stack: vec![],
            remaining: self.len,
        };
        iter.descend(self.root.as_deref());
        iter
    }
}

impl<T: Clone + PartialEq> PropertyMap<T> {
    /// Sparse edits preserve shared paths; dense edits merge once and rebuild a
    /// balanced root. No-op batches preserve snapshot identity in both cases.
    pub(super) fn apply_edits(&mut self, mut edits: Vec<(NodeId, Option<PropertyNode<T>>)>) {
        edits.sort_unstable_by_key(|(id, _)| *id);
        debug_assert!(edits.windows(2).all(|pair| pair[0].0 != pair[1].0));
        edits.retain(|(id, value)| self.get(id) != value.as_ref());
        if edits.len() <= 64 || edits.len() <= self.len / 4 {
            for (id, value) in edits {
                if let Some(value) = value {
                    self.insert(id, value);
                } else {
                    self.remove(&id);
                }
            }
            return;
        }
        let mut merged = Vec::with_capacity(self.len + edits.len());
        let mut old = self.iter().peekable();
        for (id, value) in edits {
            while old.peek().is_some_and(|(key, _)| **key < id) {
                let (key, value) = old.next().unwrap();
                merged.push((*key, value.clone()));
            }
            if old.peek().is_some_and(|(key, _)| **key == id) {
                old.next();
            }
            if let Some(value) = value {
                merged.push((id, value));
            }
        }
        merged.extend(old.map(|(key, value)| (*key, value.clone())));
        *self = Self::from_entries(merged);
    }
    /// Equal writes retain root identity. This also avoids copying unchanged
    /// chains when an ancestor's dirty subtree is extracted again.
    pub fn insert(&mut self, key: NodeId, value: PropertyNode<T>) -> Option<PropertyNode<T>> {
        let old = self.get(&key).cloned();
        if old.as_ref() == Some(&value) {
            return old;
        }
        insert(&mut self.root, key, value);
        self.len += usize::from(old.is_none());
        old
    }
}

impl<T: Clone> PropertyMap<T> {
    pub fn remove(&mut self, key: &NodeId) -> Option<PropertyNode<T>> {
        let old = self.get(key)?.clone();
        remove(&mut self.root, key);
        self.len -= 1;
        Some(old)
    }
}

impl<T: PartialEq> PartialEq for PropertyMap<T> {
    fn eq(&self, other: &Self) -> bool {
        self.shares_root(other) || (self.len == other.len && self.iter().eq(other.iter()))
    }
}

impl<T> Index<&NodeId> for PropertyMap<T> {
    type Output = PropertyNode<T>;
    fn index(&self, key: &NodeId) -> &Self::Output {
        self.get(key).expect("property node")
    }
}

pub struct PropertyIter<'a, T> {
    stack: Vec<&'a Entry<T>>,
    remaining: usize,
}
impl<'a, T> PropertyIter<'a, T> {
    fn descend(&mut self, mut cursor: Option<&'a Entry<T>>) {
        while let Some(entry) = cursor {
            self.stack.push(entry);
            cursor = entry.left.as_deref();
        }
    }
}
impl<'a, T> Iterator for PropertyIter<'a, T> {
    type Item = (&'a NodeId, &'a PropertyNode<T>);
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.stack.pop()?;
        self.descend(entry.right.as_deref());
        self.remaining -= 1;
        Some((&entry.key, &entry.value))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}
impl<T> ExactSizeIterator for PropertyIter<'_, T> {}
impl<T> std::iter::FusedIterator for PropertyIter<'_, T> {}
impl<'a, T> IntoIterator for &'a PropertyMap<T> {
    type Item = (&'a NodeId, &'a PropertyNode<T>);
    type IntoIter = PropertyIter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

fn height<T>(link: &Link<T>) -> u8 {
    link.as_ref().map_or(0, |entry| entry.height)
}
fn refresh<T>(entry: &mut Entry<T>) {
    entry.height = 1 + height(&entry.left).max(height(&entry.right));
}
fn skew<T>(entry: &Entry<T>) -> i16 {
    i16::from(height(&entry.left)) - i16::from(height(&entry.right))
}
fn rotate_left<T: Clone>(link: &mut Link<T>) {
    let mut root = link.take().expect("rotation root");
    let root_mut = Arc::make_mut(&mut root);
    let mut pivot = root_mut.right.take().expect("right-heavy rotation");
    root_mut.right = Arc::make_mut(&mut pivot).left.take();
    refresh(root_mut);
    let pivot_mut = Arc::make_mut(&mut pivot);
    pivot_mut.left = Some(root);
    refresh(pivot_mut);
    *link = Some(pivot);
}
fn rotate_right<T: Clone>(link: &mut Link<T>) {
    let mut root = link.take().expect("rotation root");
    let root_mut = Arc::make_mut(&mut root);
    let mut pivot = root_mut.left.take().expect("left-heavy rotation");
    root_mut.left = Arc::make_mut(&mut pivot).right.take();
    refresh(root_mut);
    let pivot_mut = Arc::make_mut(&mut pivot);
    pivot_mut.right = Some(root);
    refresh(pivot_mut);
    *link = Some(pivot);
}
fn balance<T: Clone>(link: &mut Link<T>) {
    let Some(root) = link.as_mut() else { return };
    let root = Arc::make_mut(root);
    refresh(root);
    if skew(root) > 1 {
        if skew(root.left.as_ref().unwrap()) < 0 {
            rotate_left(&mut root.left);
        }
        rotate_right(link);
    } else if skew(root) < -1 {
        if skew(root.right.as_ref().unwrap()) > 0 {
            rotate_right(&mut root.right);
        }
        rotate_left(link);
    }
}
fn insert<T: Clone>(link: &mut Link<T>, key: NodeId, value: PropertyNode<T>) {
    let Some(root) = link.as_mut() else {
        *link = Some(Arc::new(Entry {
            key,
            value,
            left: None,
            right: None,
            height: 1,
        }));
        return;
    };
    let root = Arc::make_mut(root);
    match key.cmp(&root.key) {
        std::cmp::Ordering::Less => insert(&mut root.left, key, value),
        std::cmp::Ordering::Greater => insert(&mut root.right, key, value),
        std::cmp::Ordering::Equal => root.value = value,
    }
    balance(link);
}
fn remove<T: Clone>(link: &mut Link<T>, key: &NodeId) {
    let root = Arc::make_mut(link.as_mut().expect("existing removal key"));
    match key.cmp(&root.key) {
        std::cmp::Ordering::Less => remove(&mut root.left, key),
        std::cmp::Ordering::Greater => remove(&mut root.right, key),
        std::cmp::Ordering::Equal => {
            if root.left.is_none() {
                *link = root.right.take();
                return;
            }
            if root.right.is_none() {
                *link = root.left.take();
                return;
            }
            let mut next = root.right.as_deref().unwrap();
            while let Some(left) = next.left.as_deref() {
                next = left;
            }
            root.key = next.key;
            root.value = next.value.clone();
            remove(&mut root.right, &root.key);
        }
    }
    balance(link);
}

#[cfg(test)]
mod tests;
