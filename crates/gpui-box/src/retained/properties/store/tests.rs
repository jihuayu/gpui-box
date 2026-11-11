use super::*;

fn id(slot: usize, generation: u64) -> NodeId {
    NodeId {
        tree: 17,
        slot,
        generation,
    }
}
fn value(value: u32) -> PropertyNode<u32> {
    PropertyNode {
        parent: None,
        value,
    }
}
fn check<T>(link: &Link<T>, lower: Option<NodeId>, upper: Option<NodeId>) -> (u8, usize) {
    let Some(entry) = link else { return (0, 0) };
    assert!(lower.is_none_or(|key| key < entry.key));
    assert!(upper.is_none_or(|key| key > entry.key));
    let (left, nl) = check(&entry.left, lower, Some(entry.key));
    let (right, nr) = check(&entry.right, Some(entry.key), upper);
    assert!((i16::from(left) - i16::from(right)).abs() <= 1);
    assert_eq!(entry.height, 1 + left.max(right));
    (entry.height, 1 + nl + nr)
}

#[test]
fn ordered_mixed_edits_match_btree_with_independent_snapshots() {
    let mut map = PropertyMap::default();
    let mut reference = BTreeMap::new();
    let mut snapshots = vec![];
    let mut random = 1u64;
    for step in 0..8000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = id((random >> 32) as usize % 257, (random >> 25) % 3);
        match step % 7 {
            0 | 1 => assert_eq!(map.remove(&key), reference.remove(&key)),
            _ => assert_eq!(
                map.insert(key, value(step)),
                reference.insert(key, value(step))
            ),
        }
        assert_eq!(check(&map.root, None, None).1, reference.len());
        assert_eq!(map.len(), reference.len());
        assert!(map.iter().eq(reference.iter()));
        if step % 701 == 0 {
            snapshots.push((map.clone(), reference.clone()));
        }
    }
    let bulk = PropertyMap::from_entries(
        reference
            .iter()
            .rev()
            .map(|(key, value)| (*key, value.clone()))
            .collect(),
    );
    assert_eq!(
        bulk, map,
        "balanced bulk construction preserves ordered values"
    );
    check(&bulk.root, None, None);
    for (snapshot, expected) in snapshots {
        assert!(snapshot.iter().eq(expected.iter()));
        assert_eq!(snapshot.len(), expected.len());
        check(&snapshot.root, None, None);
    }
    // Deletion to empty exercises single-child replacement and final rotations.
    for key in reference.keys().copied().collect::<Vec<_>>() {
        assert_eq!(map.remove(&key), reference.remove(&key));
        check(&map.root, None, None);
    }
    assert!(map.is_empty());
    assert_eq!(map.iter().next(), None);
    assert_eq!(map.iter().size_hint(), (0, Some(0)));
}

#[test]
fn snapshot_update_copies_logarithmic_paths_and_equal_writes_copy_nothing() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CLONES: AtomicUsize = AtomicUsize::new(0);
    #[derive(Debug, PartialEq)]
    struct Counted(u32);
    impl Clone for Counted {
        fn clone(&self) -> Self {
            CLONES.fetch_add(1, Ordering::Relaxed);
            Self(self.0)
        }
    }
    let node = |n| PropertyNode {
        parent: None,
        value: Counted(n),
    };
    let mut original = PropertyMap::default();
    for slot in 0..10_000 {
        original.insert(id(slot, 1), node(slot as u32));
    }
    let weak = Arc::downgrade(original.root.as_ref().unwrap());
    CLONES.store(0, Ordering::Relaxed);
    let mut next = original.clone();
    assert_eq!(CLONES.load(Ordering::Relaxed), 0);
    next.insert(id(5000, 1), node(5000));
    next.remove(&id(5000, 2));
    assert!(next.shares_root(&original));
    // Return value is owned; the equal insertion clones just that old value.
    assert_eq!(CLONES.load(Ordering::Relaxed), 1);
    CLONES.store(0, Ordering::Relaxed);
    next.insert(id(5000, 1), node(99));
    assert!(CLONES.load(Ordering::Relaxed) < 32);
    assert_eq!(original[&id(5000, 1)].value.0, 5000);
    assert_eq!(next[&id(5000, 1)].value.0, 99);
    assert!(!next.shares_root(&original));
    // A new root owns children, never its previous root/history.
    drop(original);
    assert!(weak.upgrade().is_none());
    let weak = Arc::downgrade(next.root.as_ref().unwrap());
    next.clear();
    assert!(weak.upgrade().is_none());
    assert!(next.is_empty());
}
