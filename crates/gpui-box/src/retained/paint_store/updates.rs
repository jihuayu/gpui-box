//! Owned deltas can share their authoritative immutable pages without item copies.
use super::*;

#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
use std::sync::Weak;

#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
#[derive(Clone, Debug)]
struct Base {
    tree: u64,
    revision: u64,
    items: Weak<PaintItems>,
    properties: Weak<PropertyTrees>,
}
#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
#[derive(Clone, Debug)]
struct Provenance {
    tree: u64,
    revision: u64,
    base: Option<Base>,
    items: Weak<PaintItems>,
    properties: Weak<PropertyTrees>,
    removed: Arc<[NodeId]>,
    order: Option<Arc<[NodeId]>>,
    damage: Damage,
}

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
    #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
    provenance: Option<Provenance>,
}

impl From<Vec<PaintItem>> for PaintUpdates {
    fn from(items: Vec<PaintItem>) -> Self {
        Self {
            storage: Storage::Owned(items),
            #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
            provenance: None,
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
            #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
            provenance: None,
        })
    }
    #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
    pub(in crate::retained) fn indices_for(&self, items: &Arc<PaintItems>) -> Option<&[usize]> {
        match &self.storage {
            Storage::Indexed {
                items: root,
                indices,
            } if Arc::ptr_eq(root, items) => Some(indices),
            _ => None,
        }
    }
    #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
    pub(in crate::retained) fn follows_snapshot(
        &self,
        frame: &FramePacket,
        tree: u64,
        revision: u64,
        items: &Arc<PaintItems>,
        properties: &Arc<PropertyTrees>,
    ) -> bool {
        let Some(p) = &self.provenance else {
            return false;
        };
        let Some(base) = &p.base else {
            return false;
        };
        p.tree == frame.tree
            && p.revision == frame.revision
            && base.tree == tree
            && base.revision == revision
            && frame.base_revision == Some(revision)
            && p.items.ptr_eq(&Arc::downgrade(&frame.items))
            && p.properties.ptr_eq(&Arc::downgrade(&frame.properties))
            && base.items.ptr_eq(&Arc::downgrade(items))
            && base.properties.ptr_eq(&Arc::downgrade(properties))
            && Arc::ptr_eq(&p.removed, &frame.removed)
            && p.damage == frame.damage
            && match (&p.order, &frame.order) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
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
        #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
        {
            self.provenance = None;
        }
        if matches!(self.storage, Storage::Indexed { .. }) {
            self.storage = Storage::Owned(self.to_vec());
        }
        match &mut self.storage {
            Storage::Owned(items) => &mut items[index],
            Storage::Indexed { .. } => unreachable!(),
        }
    }
}

impl FramePacket {
    /// Only the normal producer can attest a complete transition. Weak roots do
    /// not retain old page/resource payloads or create snapshot chains.
    pub(in crate::retained) fn seal_updates(&mut self, base: Option<&FramePacket>) {
        #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
        {
            let provenance = Provenance {
                tree: self.tree,
                revision: self.revision,
                base: base.map(|base| Base {
                    tree: base.tree,
                    revision: base.revision,
                    items: Arc::downgrade(&base.items),
                    properties: Arc::downgrade(&base.properties),
                }),
                items: Arc::downgrade(&self.items),
                properties: Arc::downgrade(&self.properties),
                removed: self.removed.clone(),
                order: self.order.clone(),
                damage: self.damage,
            };
            Arc::make_mut(&mut self.updates).provenance = Some(provenance);
        }
        #[cfg(not(all(feature = "retained-wgpu", not(target_family = "wasm"))))]
        let _ = base;
    }
}
