use super::*;
use crate::tree::{
    CustomAllocationChildren, CustomLayout, CustomLayoutChildren, CustomLayoutInput, CustomLayoutOutput,
    CustomParentData, CustomPlacement, SizeBounds,
};
use crate::{AvailableSpace, Baselines, Point};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct Metadata(Arc<AtomicUsize>);
impl CustomLayout for Metadata {
    fn allocate(
        &self,
        _input: LayoutInput,
        children: &dyn CustomAllocationChildren,
    ) -> Result<Option<Vec<LayoutInput>>, TaffyError> {
        assert!(children.parent_data(children.len()).is_err());
        let mut slots = vec![];
        for i in 0..children.len() {
            let width = *children
                .parent_data(i)?
                .and_then(|d| d.downcast_ref::<u32>())
                .ok_or(TaffyError::InvalidCustomLayout)? as f32;
            let mut slot = LayoutInput::HIDDEN;
            slot.run_mode = RunMode::ComputeSize;
            slot.known_dimensions = Size {
                width: Some(width),
                height: Some(10.0),
            };
            slot.size_bounds = SizeBounds {
                min: Size { width, height: 10.0 },
                max: Size { width, height: 10.0 },
            };
            slots.push(slot);
        }
        Ok(Some(slots))
    }
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        assert!(children.parent_data(children.len()).is_err());
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            assert!(children.parent_data(child)?.unwrap().downcast_ref::<u32>().is_some());
            let result = children.measure(child, LayoutInput::HIDDEN)?;
            placements.push(CustomPlacement {
                child,
                input: LayoutInput::HIDDEN,
                origin: Point { x, y: 0.0 },
            });
            x += result.size.width;
            height = height.max(result.size.height);
        }
        Ok(CustomLayoutOutput {
            size: input.known_size.unwrap_or(Size { width: x, height }),
            baselines: Baselines::NONE,
            placements,
        })
    }
}

#[test]
fn parent_data_cache_clone_visibility_reparent_and_lifetime() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let first = tree.new_leaf(Style::DEFAULT).unwrap();
    let second = tree.new_leaf(Style::DEFAULT).unwrap();
    let root = tree.new_with_children(Style::DEFAULT, &[first, second]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    tree.set_custom_layout(root, Some(Arc::new(Metadata(calls.clone()))))
        .unwrap();
    let ten = CustomParentData::new(10u32);
    tree.set_parent_data(first, Some(ten.clone())).unwrap();
    tree.set_parent_data(second, Some(CustomParentData::new(20u32)))
        .unwrap();
    let space = Size {
        width: AvailableSpace::MaxContent,
        height: AvailableSpace::MaxContent,
    };
    tree.compute_layout(root, space).unwrap();
    assert_eq!(tree.layout(root).unwrap().size.width, 30.0);
    let before = calls.load(Ordering::Relaxed);
    tree.set_parent_data(first, Some(ten.clone())).unwrap();
    assert!(!tree.dirty(root).unwrap());
    tree.compute_layout(root, space).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), before);
    let old = tree.clone();
    tree.set_parent_data(first, Some(CustomParentData::new(40u32))).unwrap();
    assert!(tree.dirty(root).unwrap());
    tree.compute_layout(root, space).unwrap();
    assert_eq!(tree.layout(root).unwrap().size.width, 60.0);
    assert_eq!(old.parent_data(first).unwrap(), Some(&ten));
    assert_eq!(old.layout(root).unwrap().size.width, 30.0);
    tree.set_style(
        root,
        Style {
            display: Display::None,
            ..Style::DEFAULT
        },
    )
    .unwrap();
    tree.compute_layout(root, space).unwrap();
    assert_eq!(tree.layout(first).unwrap().size, Size::ZERO);
    assert!(tree.parent_data(first).unwrap().is_some());
    tree.set_style(root, Style::DEFAULT).unwrap();
    tree.compute_layout(root, space).unwrap();
    assert_eq!(tree.layout(root).unwrap().size.width, 60.0);
    let other = tree.new_leaf(Style::DEFAULT).unwrap();
    tree.set_custom_layout(other, Some(Arc::new(Metadata(calls)))).unwrap();
    tree.set_children(root, &[second]).unwrap();
    tree.set_children(other, &[first]).unwrap();
    tree.compute_layout(other, space).unwrap();
    assert_eq!(tree.layout(other).unwrap().size.width, 40.0);
    assert_eq!(
        tree.parent_data(first).unwrap().unwrap().downcast_ref::<u32>(),
        Some(&40)
    );
    tree.set_parent_data(first, None).unwrap();
    assert!(tree.compute_layout(other, space).is_err());
    tree.remove(first).unwrap();
    assert!(tree.parent_data(first).is_err());
    assert!(tree.set_parent_data(first, None).is_err());
    let reused = tree.new_leaf(Style::DEFAULT).unwrap();
    assert_eq!(tree.parent_data(reused).unwrap(), None);
    let token = Arc::new(());
    let weak = Arc::downgrade(&token);
    tree.set_parent_data(reused, Some(CustomParentData::new(token)))
        .unwrap();
    let mut clone = tree.clone();
    tree.clear();
    assert!(weak.upgrade().is_some());
    clone.clear();
    assert!(weak.upgrade().is_none());
    assert!(tree.parent_data.is_empty());
}
