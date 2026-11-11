//! Immutable decoded straight-alpha RGBA8 resources, independent of render nodes.
use super::*;

#[derive(Debug)]
struct Pixels {
    id: u64,
    size: [u32; 2],
    rgba: Arc<[u8]>,
}

/// Cloning preserves resource identity. Replacing pixels creates a new identity;
/// old packets keep their exact bytes alive without borrowing an application.
#[derive(Clone, Debug)]
pub struct ImageResource(Arc<Pixels>);
impl PartialEq for ImageResource {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
impl Eq for ImageResource {}
impl ImageResource {
    /// Decoding and orientation must be resolved by the caller before publication.
    /// One texel initially maps to one logical pixel for intrinsic sizing.
    pub fn rgba8(size: [u32; 2], rgba: impl Into<Arc<[u8]>>) -> Result<Self, Error> {
        let rgba = rgba.into();
        let bytes = u64::from(size[0])
            .checked_mul(u64::from(size[1]))
            .and_then(|n| n.checked_mul(4));
        if size.contains(&0)
            || bytes.is_none_or(|bytes| bytes > 64 * 1024 * 1024 || bytes != rgba.len() as u64)
        {
            return Err(Error::InvalidImage);
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        Ok(Self(Arc::new(Pixels { id, size, rgba })))
    }
    pub fn id(&self) -> u64 {
        self.0.id
    }
    pub fn size(&self) -> [u32; 2] {
        self.0.size
    }
    pub fn pixels(&self) -> &Arc<[u8]> {
        &self.0.rgba
    }
    pub(super) fn measure(&self) -> MeasureSnapshot {
        MeasureSnapshot::Image(ImageIntrinsicSize {
            width: self.size()[0] as f32,
            height: self.size()[1] as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(size: [u32; 2], color: [u8; 4]) -> ImageResource {
        ImageResource::rgba8(size, color.repeat((size[0] * size[1]) as usize)).unwrap()
    }
    #[test]
    fn replacement_routes_resource_and_intrinsic_inputs_independently() {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let a = image([8, 4], [255, 0, 0, 255]);
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(a.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let old = tree.frame([100.0; 2]).unwrap();
        assert_eq!(
            (old.items[0].bounds.width, old.items[0].bounds.height),
            (8.0, 4.0)
        );
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(a.clone()));
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
        let b = image([8, 4], [0, 255, 0, 128]);
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(b));
        let commit = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(commit.routes, [(tree.root(), UpdatePath::ResourcePatch)]);
        let next = tree.frame([100.0; 2]).unwrap();
        assert_eq!(
            (next.layout_runs, next.recorded_chunks, next.updates.len()),
            (0, 0, 1)
        );
        assert!(Arc::ptr_eq(&old.items[0].commands, &next.items[0].commands));
        assert_eq!(old.items[0].image.as_ref().unwrap(), &a);
        assert!(next.damage.bounds.is_some());
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(image([12, 6], [0, 0, 255, 255])));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let resized = tree.frame([100.0; 2]).unwrap();
        assert_eq!((resized.layout_runs, resized.recorded_chunks), (1, 0));
        assert_eq!(
            (
                resized.items[0].bounds.width,
                resized.items[0].bounds.height
            ),
            (12.0, 6.0)
        );
        let mut tx = Transaction::default();
        tx.image(tree.root(), None);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let removed = tree.frame([100.0; 2]).unwrap();
        assert!(removed.items[0].image.is_none());
        assert!(removed.damage.bounds.is_some());
    }
    #[test]
    fn owned_resources_validate_and_reject_ambiguous_measurement_atomically() {
        assert!(ImageResource::rgba8([0, 1], vec![]).is_err());
        assert!(ImageResource::rgba8([2, 2], vec![0; 15]).is_err());
        assert!(ImageResource::rgba8([u32::MAX; 2], vec![]).is_err());
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let before = tree.frame([100.0; 2]).unwrap();
        let resource = image([2, 2], [255; 4]);
        let weak = Arc::downgrade(resource.pixels());
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0xffffffff);
        tx.image(tree.root(), Some(resource.clone()));
        tx.measure(tree.root(), None);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidMeasure
        );
        assert_eq!(before.revision, tree.frame([100.0; 2]).unwrap().revision);
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(resource.clone()));
        tx.image(tree.root(), None);
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
        let mut tx = Transaction::default();
        tx.image(tree.root(), Some(resource.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let packet = tree.frame([100.0; 2]).unwrap();
        drop(tree);
        drop(resource);
        assert!(weak.upgrade().is_some());
        drop(packet);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn removing_image_nodes_retires_logical_bindings_without_reviving_reused_slots() {
        use logical::LogicalUi;
        use reactive::Property;
        let mut ui = LogicalUi::from_tree(
            UiTree::new(Style::default(), 0).unwrap(),
            BackendCapabilities::default(),
        )
        .unwrap();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let resource = image([4, 2], [255; 4]);
        let weak = Arc::downgrade(resource.pixels());
        let signal = ui.state(scope, 0, || Some(resource.clone())).unwrap();
        let node = ui
            .append_render(scope, ui.root_render(), Style::default(), 0)
            .unwrap();
        ui.bind(signal, Property::image(node)).unwrap();
        let old = ui.frame([100.0; 2]).unwrap();
        ui.reconcile_keys(root, &[]).unwrap();
        let removed = ui.frame([100.0; 2]).unwrap();
        assert!(removed.removed.contains(&node));
        assert!(ui.read(signal).is_err());
        drop(resource);
        drop(removed);
        drop(ui);
        assert!(
            weak.upgrade().is_some(),
            "published old snapshot owns the resource"
        );
        drop(old);
        assert!(weak.upgrade().is_none());
    }
}
