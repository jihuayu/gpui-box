use super::*;
fn style(w: f32, h: f32) -> Style {
    Style {
        size: Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}
fn fixture() -> UiTree {
    UiTree::new(style(160.0, 80.0), 0x112233ff).unwrap()
}
const PATCH: BackendCapabilities = BackendCapabilities {
    solid_color_patch: true,
};
#[test]
fn construction_moves_removal_and_properties_publish_one_revision_with_stable_layout() {
    let mut tree = fixture();
    let root = tree.root();
    let host = tree.append(root, style(40.0, 40.0), 0).unwrap();
    let moved = tree.append(host, style(16.0, 16.0), 0xff0000ff).unwrap();
    let retired = tree.append(root, style(20.0, 20.0), 0x00ff00ff).unwrap();
    let before = tree.frame([160.0, 80.0]).unwrap();
    let old_layout = tree.node(moved).unwrap().layout;
    let result = tree
        .edit(PATCH, |edit| {
            let host = edit.append(root, style(48.0, 48.0), 0x223344ff)?;
            let child = edit.append(host, style(12.0, 12.0), 0xffffffff)?;
            edit.reparent(moved, host)?;
            edit.remove(retired)?;
            edit.properties().color(moved, 0x4488ffff);
            edit.properties().translation(host, [4.0, 2.0]);
            Ok((host, child))
        })
        .unwrap();
    let (new_host, child) = result.value;
    assert_eq!(result.commit.revision, before.revision + 1);
    assert_eq!(result.inserted.len(), 2);
    assert_eq!(result.removed, [retired]);
    assert_eq!(tree.node(moved).unwrap().layout, old_layout);
    assert_eq!(tree.node(moved).unwrap().parent, Some(new_host));
    assert_eq!(tree.node(new_host).unwrap().children, vec![child, moved]);
    let after = tree.frame([160.0, 80.0]).unwrap();
    assert_eq!(after.recorded_chunks, 2);
    let old = before.items.iter().find(|i| i.node == moved).unwrap();
    let next = after.items.iter().find(|i| i.node == moved).unwrap();
    assert!(Arc::ptr_eq(&old.commands, &next.commands));
    assert_eq!(next.fill_color, 0x4488ffff);
    assert!(tree.node(retired).is_err());
    let mut fresh = fixture();
    let r = fresh.root();
    fresh.append(r, style(40.0, 40.0), 0).unwrap();
    let h = fresh.append(r, style(48.0, 48.0), 0x223344ff).unwrap();
    fresh.append(h, style(12.0, 12.0), 0xffffffff).unwrap();
    fresh.append(h, style(16.0, 16.0), 0x4488ffff).unwrap();
    let mut tx = Transaction::default();
    tx.translation(h, [4.0, 2.0]);
    fresh.commit(tx, PATCH).unwrap();
    let expected = fresh.frame([160.0, 80.0]).unwrap();
    assert_eq!(
        after
            .items
            .iter()
            .map(|i| (i.bounds, i.fill_color))
            .collect::<Vec<_>>(),
        expected
            .items
            .iter()
            .map(|i| (i.bounds, i.fill_color))
            .collect::<Vec<_>>()
    );
}
#[test]
fn invalid_draft_never_publishes_and_reserved_ids_never_revive() {
    let mut tree = fixture();
    let root = tree.root();
    let child = tree.append(root, style(12.0, 12.0), 1).unwrap();
    let before = tree.frame([160.0, 80.0]).unwrap();
    let mut leaked = None;
    let result = tree.edit(PATCH, |edit| {
        leaked = Some(edit.append(root, style(10.0, 10.0), 2)?);
        edit.remove(child)?;
        edit.properties().color(root, 3);
        edit.properties().translation(root, [f32::NAN, 0.0]);
        Ok(())
    });
    assert!(matches!(result, Err(Error::InvalidTranslation)));
    let leaked = leaked.unwrap();
    assert!(tree.node(leaked).is_err());
    assert!(tree.node(child).is_ok());
    assert_eq!(tree.node(root).unwrap().color, 0x112233ff);
    let current = tree.frame([160.0, 80.0]).unwrap();
    assert_eq!(current.revision, before.revision);
    assert!(Arc::ptr_eq(&current.items, &before.items));
    let published = tree.append(root, style(10.0, 10.0), 2).unwrap();
    assert_eq!(published.slot, leaked.slot);
    assert_ne!(published, leaked);
    let mut thrown = None;
    assert!(
        tree.edit(PATCH, |edit| {
            thrown = Some(edit.append(root, style(1.0, 1.0), 0)?);
            Err::<(), _>(Error::InvalidFrame)
        })
        .is_err()
    );
    let new = tree.append(root, style(1.0, 1.0), 0).unwrap();
    assert_ne!(new, thrown.unwrap());
}
#[test]
fn final_leaf_contract_allows_image_container_swaps_and_rejects_invalid_topology() {
    let mut tree = fixture();
    let root = tree.root();
    let image = tree.append(root, Style::default(), 0).unwrap();
    let resource = ImageResource::rgba8([8, 4], vec![255; 128]).unwrap();
    let mut tx = Transaction::default();
    tx.image(image, Some(resource.clone()));
    tree.commit(tx, PATCH).unwrap();
    let old = tree.frame([160.0, 80.0]).unwrap();
    assert!(matches!(
        tree.edit(PATCH, |edit| edit.append(image, style(12.0, 12.0), 0)),
        Err(Error::MeasuredNodeHasChildren)
    ));
    let swap = tree
        .edit(PATCH, |edit| {
            edit.properties().image(image, None);
            edit.append(image, style(12.0, 12.0), 0x4488ffff)
        })
        .unwrap();
    let child = swap.value;
    assert_eq!(swap.commit.revision, old.revision + 1);
    let container = tree.frame([160.0, 80.0]).unwrap();
    assert!(container.items.iter().any(|i| i.node == child));
    let next = tree
        .edit(PATCH, |edit| {
            edit.remove(child)?;
            edit.properties().image(image, Some(resource));
            Ok(())
        })
        .unwrap();
    assert_eq!(next.commit.revision, container.revision + 1);
    assert!(tree.node(child).is_err());
    assert!(tree.node(image).unwrap().image.is_some());
    let before = tree.frame([160.0, 80.0]).unwrap();
    assert!(
        tree.edit(PATCH, |edit| {
            edit.reparent(image, image)?;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(tree.frame([160.0, 80.0]).unwrap().revision, before.revision);
}
#[test]
fn cancelled_construction_equal_properties_and_revision_exhaustion_preserve_published_output() {
    let mut tree = fixture();
    let root = tree.root();
    let before = tree.frame([160.0, 80.0]).unwrap();
    let mut cancelled = None;
    let result = tree
        .edit(PATCH, |edit| {
            let id = edit.append(root, style(10.0, 10.0), 1)?;
            cancelled = Some(id);
            edit.remove(id)?;
            edit.properties().color(root, 2);
            edit.properties().color(root, 0x112233ff);
            Ok(())
        })
        .unwrap();
    assert!(!result.commit.request_frame);
    assert!(result.inserted.is_empty() && result.removed.is_empty());
    assert_eq!(result.commit.revision, before.revision);
    assert!(Arc::ptr_eq(
        &tree.frame([160.0, 80.0]).unwrap().items,
        &before.items
    ));
    let live = tree.append(root, style(10.0, 10.0), 1).unwrap();
    assert_ne!(live, cancelled.unwrap());
    tree.revision = u64::MAX;
    assert!(
        !tree
            .edit(PATCH, |edit| {
                let node = edit.append(root, style(1.0, 1.0), 0)?;
                edit.remove(node)?;
                Ok(())
            })
            .unwrap()
            .commit
            .request_frame
    );
    assert!(
        !tree
            .edit(PATCH, |edit| {
                edit.properties().color(root, 0x112233ff);
                Ok(())
            })
            .unwrap()
            .commit
            .request_frame
    );
    assert!(matches!(
        tree.edit(PATCH, |edit| {
            edit.properties().color(root, 5);
            Ok(())
        }),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(tree.node(root).unwrap().color, 0x112233ff);
}

#[test]
fn child_lists_then_reparent_resolve_draft_edges_and_allow_host_evacuation() {
    let mut tree = fixture();
    let root = tree.root();
    let host = tree.append(root, style(20.0, 20.0), 1).unwrap();
    let child = tree.append(host, style(4.0, 4.0), 2).unwrap();
    tree.edit(PATCH, |edit| {
        edit.set_children(host, vec![])?;
        edit.set_children(root, vec![host, child])?;
        edit.reparent(child, host)?;
        edit.reparent(child, root)?;
        edit.remove(host)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(tree.node(root).unwrap().children, [child]);
    assert_eq!(tree.node(child).unwrap().parent, Some(root));
}

#[test]
fn property_only_edit_keeps_layout_and_chunk_fast_paths() {
    let mut tree = fixture();
    let root = tree.root();
    let before = tree.frame([160.0, 80.0]).unwrap();
    let result = tree
        .edit(PATCH, |edit| {
            edit.properties().color(root, 0x4488ffff);
            Ok(())
        })
        .unwrap();
    assert!(result.commit.request_frame);
    let after = tree.frame([160.0, 80.0]).unwrap();
    assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.items[0].fill_color, 0x4488ffff);
}

#[test]
fn cancelled_holes_and_free_slots_do_not_lose_generation_or_layout_inputs() {
    let mut tree = fixture();
    let root = tree.root();
    let removed = tree.append(root, Style::default(), 0).unwrap();
    tree.remove(removed).unwrap();
    let result = tree
        .edit(PATCH, |edit| {
            let cancelled = edit.append(root, Style::default(), 0)?;
            edit.remove(cancelled)?;
            let leaf = edit.append(root, Style::default(), 0x4488ffff)?;
            edit.properties().measure(
                leaf,
                Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                    width: 12.0,
                    height: 8.0,
                    baseline: None,
                })),
            );
            edit.properties().style(
                leaf,
                Style {
                    flex_grow: 0.0,
                    flex_shrink: 0.0,
                    align_self: Some(taffy::AlignSelf::START),
                    ..Default::default()
                },
            );
            Ok((cancelled, leaf))
        })
        .unwrap();
    assert!(tree.node(result.value.0).is_err());
    assert_eq!(result.inserted, [result.value.1]);
    let frame = tree.frame([160.0, 80.0]).unwrap();
    let item = frame
        .items
        .iter()
        .find(|i| i.node == result.value.1)
        .unwrap();
    assert_eq!((item.bounds.width, item.bounds.height), (12.0, 8.0));
    let later = tree.append(root, Style::default(), 0).unwrap();
    assert_eq!(later.slot, result.value.0.slot);
    assert_ne!(later, result.value.0);
    assert_ne!(later, removed);
}
