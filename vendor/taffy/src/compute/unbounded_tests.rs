use crate::prelude::*;

#[test]
fn unbounded_normal_flex_and_intrinsic_queries_remain_distinct_after_cache_reuse() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let child = tree
            .new_leaf(Style {
                size: Size { width: length(80.0), height: length(80.0) },
                min_size: Size::zero(),
                flex_basis: length(20.0),
                flex_grow: 1.0,
                ..Style::default()
            })
            .unwrap();
        let root = tree
            .new_with_children(
                Style { flex_direction: direction, align_items: AlignItems::START, ..Style::default() },
                &[child],
            )
            .unwrap();
        for space in [AvailableSpace::Unbounded, AvailableSpace::MaxContent, AvailableSpace::Unbounded] {
            tree.compute_layout(root, Size { width: space, height: space }).unwrap();
            let expected = if direction == FlexDirection::Row {
                Size { width: if space == AvailableSpace::Unbounded { 20.0 } else { 80.0 }, height: 80.0 }
            } else {
                Size { width: 80.0, height: if space == AvailableSpace::Unbounded { 20.0 } else { 80.0 } }
            };
            assert_eq!(tree.layout(root).unwrap().size, expected, "{direction:?}/{space:?}");
            assert_eq!(tree.layout(child).unwrap().size, expected);
        }
    }
}

#[test]
fn unbounded_arithmetic_stays_tagged_until_a_real_upper_bound() {
    use crate::util::MaybeMath;
    let free = AvailableSpace::Unbounded;
    assert!(!free.is_definite());
    assert!(!free.is_intrinsic());
    assert_eq!(free.into_option(), None);
    assert_eq!(free.maybe_sub(10.0), free);
    assert_eq!(free.maybe_add(10.0), free);
    assert_eq!(free.maybe_max(20.0), free);
    assert_eq!(free.maybe_clamp(Some(20.0), None), free);
    assert_eq!(free.maybe_clamp(Some(20.0), Some(10.0)), AvailableSpace::Definite(20.0));
    assert_eq!(free.maybe_min(50.0), AvailableSpace::Definite(50.0));
    assert!(!free.is_roughly_equal(AvailableSpace::MaxContent));
}

#[test]
fn unbounded_grid_and_block_remain_finite_with_actual_descendants() {
    for display in [Display::Grid, Display::Block] {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let children = [30.0, 40.0].map(|width| {
            tree.new_leaf(Style { size: Size { width: length(width), height: length(10.0) }, ..Style::default() })
                .unwrap()
        });
        let root = tree.new_with_children(Style { display, ..Style::default() }, &children).unwrap();
        tree.compute_layout(root, Size { width: AvailableSpace::Unbounded, height: AvailableSpace::Unbounded })
            .unwrap();
        assert_eq!(tree.layout(root).unwrap().size, Size { width: 40.0, height: 20.0 }, "{display:?}");
        assert_eq!(tree.layout(children[1]).unwrap().location.y, 10.0);
    }
}

#[test]
fn unbounded_fractional_grid_tracks_expand_from_content_without_infinity() {
    let mut tree: TaffyTree<()> = TaffyTree::new();
    tree.disable_rounding();
    let children = [30.0, 40.0].map(|width| {
        tree.new_leaf(Style { size: Size { width: length(width), height: length(10.0) }, ..Style::default() }).unwrap()
    });
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                grid_template_columns: vec![fr(1.0), fr(2.0)],
                gap: Size { width: length(3.0), height: length(0.0) },
                ..Style::default()
            },
            &children,
        )
        .unwrap();
    for space in [AvailableSpace::Unbounded, AvailableSpace::MaxContent, AvailableSpace::Unbounded] {
        tree.compute_layout(root, Size { width: space, height: space }).unwrap();
        assert_eq!(tree.layout(root).unwrap().size, Size { width: 93.0, height: 10.0 });
        assert_eq!(tree.layout(children[1]).unwrap().location.x, 33.0);
    }
}
