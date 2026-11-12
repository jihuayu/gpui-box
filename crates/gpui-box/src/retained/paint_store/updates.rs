//! Owned deltas can share their authoritative immutable pages without item copies.
use super::*;

#[derive(Clone, Debug)]
enum Storage {
    Indexed {
        items: Arc<PaintItems>,
        indices: Vec<usize>,
    },
    Owned(Vec<PaintItem>),
}

/// Immutable indexed view of an owned paint snapshot, or explicitly supplied
/// items. Index mutation materializes a private COW list and loses view identity.
#[derive(Clone, Debug)]
pub struct PaintUpdates {
    storage: Storage,
}

impl From<Vec<PaintItem>> for PaintUpdates {
    fn from(items: Vec<PaintItem>) -> Self {
        Self {
            storage: Storage::Owned(items),
        }
    }
}
impl<const N: usize> From<[PaintItem; N]> for PaintUpdates {
    fn from(items: [PaintItem; N]) -> Self {
        Vec::from(items).into()
    }
}
impl PaintUpdates {
    pub(in crate::retained) fn indexed(items: Arc<PaintItems>, indices: Vec<usize>) -> Arc<Self> {
        assert!(
            indices.windows(2).all(|p| p[0] < p[1])
                && indices.last().is_none_or(|i| *i < items.len())
        );
        Arc::new(Self {
            storage: Storage::Indexed { items, indices },
        })
    }
    pub fn len(&self) -> usize {
        match &self.storage {
            Storage::Indexed { indices, .. } => indices.len(),
            Storage::Owned(items) => items.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, index: usize) -> Option<&PaintItem> {
        match &self.storage {
            Storage::Indexed { items, indices } => indices.get(index).map(|i| &items[*i]),
            Storage::Owned(items) => items.get(index),
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &PaintItem> + ExactSizeIterator {
        (0..self.len()).map(|index| &self[index])
    }
    pub fn to_vec(&self) -> Vec<PaintItem> {
        self.iter().cloned().collect()
    }
    #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
    pub(in crate::retained) fn matches_items(&self, items: &Arc<PaintItems>) -> bool {
        matches!(&self.storage,Storage::Indexed {items: source,..} if Arc::ptr_eq(source,items))
    }
}
impl Index<usize> for PaintUpdates {
    type Output = PaintItem;
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("paint update index")
    }
}
impl IndexMut<usize> for PaintUpdates {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        if matches!(self.storage, Storage::Indexed { .. }) {
            self.storage = Storage::Owned(self.to_vec());
        }
        match &mut self.storage {
            Storage::Owned(items) => &mut items[index],
            Storage::Indexed { .. } => unreachable!(),
        }
    }
}
