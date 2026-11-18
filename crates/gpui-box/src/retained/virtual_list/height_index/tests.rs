use super::*;
use std::collections::BTreeSet;

fn oracle_window(heights: &[f32], offset: f32, viewport: f32, overscan: f32) -> VirtualListWindow {
    let mut prefix = vec![0.0];
    for &h in heights {
        prefix.push(prefix.last().unwrap() + f64::from(h));
    }
    let total = *prefix.last().unwrap();
    let maximum = (total - f64::from(viewport)).max(0.0) as f32;
    let offset = offset.min(maximum);
    let start = (f64::from(offset) - f64::from(overscan)).max(0.0);
    let end = (f64::from(offset) + f64::from(viewport) + f64::from(overscan)).min(total);
    let range = if viewport == 0.0 || heights.is_empty() {
        0..0
    } else {
        prefix.partition_point(|v| *v <= start).saturating_sub(1)
            ..prefix.partition_point(|v| *v < end).min(heights.len())
    };
    VirtualListWindow {
        leading: prefix[range.start] as f32,
        trailing: (total - prefix[range.end]) as f32,
        range,
        offset,
        maximum,
        total: total as f32,
    }
}
fn check(metrics: &VirtualListMetrics, heights: &[f32]) {
    let mut sum = 0.0;
    for (i, &h) in heights.iter().enumerate() {
        assert_eq!(metrics.edge(i), sum);
        assert_eq!(metrics.height(i).unwrap(), h);
        // Both sides and the exact edge must select the same rows as the
        // independent former flat prefix implementation, including leaf splits.
        for offset in [
            (sum as f32).next_down().max(0.0),
            sum as f32,
            (sum as f32).next_up(),
        ] {
            for (view, over) in [(0.0, 10.0), (0.5, 0.0), (80.0, 20.0)] {
                assert_eq!(
                    metrics
                        .window(offset, view, over, heights.len() + 1)
                        .unwrap(),
                    oracle_window(heights, offset, view, over)
                );
            }
        }
        sum += f64::from(h);
    }
    assert_eq!(metrics.edge(heights.len()), sum);
    assert_eq!(metrics.total(), sum as f32);
}

#[test]
fn persistent_index_matches_flat_reference_across_leaf_boundaries_and_sparse_revisions() {
    for count in [0, 1, 2, 63, 64, 65, 127, 128, 129, 257] {
        let mut heights = (0..count)
            .map(|i| 1.0 + (i % 17) as f32 * 0.25)
            .collect::<Vec<_>>();
        let mut metrics = VirtualListMetrics::variable(heights.iter().copied()).unwrap();
        check(&metrics, &heights);
        if count == 0 {
            continue;
        }
        let original = metrics.clone();
        let old = heights.clone();
        for turn in 0..12 {
            let updates = (0..5)
                .map(|i| {
                    (
                        (turn * 37 + i * 23) % count,
                        1.5 + ((turn + i) % 13) as f32 * 0.5,
                    )
                })
                .collect::<Vec<_>>();
            for &(i, h) in &updates {
                heights[i] = h;
            }
            metrics = metrics.with_heights(updates).unwrap();
            check(&metrics, &heights);
            assert_eq!(
                metrics,
                VirtualListMetrics::variable(heights.iter().copied()).unwrap()
            );
        }
        check(&original, &old);
    }
}
fn nodes(index: &Arc<Index>, out: &mut BTreeSet<usize>) {
    out.insert(Arc::as_ptr(index) as usize);
    if let Kind::Branch(a, b) = &index.kind {
        nodes(a, out);
        nodes(b, out);
    }
}
#[test]
fn distant_height_updates_share_untouched_intervals_and_revert_to_uniform_storage() {
    let fixed = VirtualListMetrics::fixed(100_000, 20.0).unwrap();
    let one = fixed.with_heights([(50_000, 30.0)]).unwrap();
    let Extents::Variable(a) = &one.0 else {
        panic!()
    };
    let mut first = BTreeSet::new();
    nodes(a, &mut first);
    assert!(
        first.len() < 30,
        "implicit fixed range must not materialize all leaves"
    );
    let two = one.with_heights([(99_000, 25.0)]).unwrap();
    let Extents::Variable(b) = &two.0 else {
        panic!()
    };
    let mut second = BTreeSet::new();
    nodes(b, &mut second);
    assert!(second.difference(&first).count() < 30);
    assert!(second.intersection(&first).count() > 5);
    assert_eq!(one.height(99_000).unwrap(), 20.0);
    assert_eq!(two.height(50_000).unwrap(), 30.0);
    assert_eq!(
        two.with_heights([(99_000, 20.0), (50_000, 20.0)]).unwrap(),
        fixed
    );
    assert!(matches!(
        two.with_heights([(99_000, 20.0), (50_000, 20.0)])
            .unwrap()
            .0,
        Extents::Fixed { .. }
    ));
}
#[test]
fn variable_revision_copies_only_affected_paths_and_dense_updates_match_fresh_build() {
    let heights = (0..100_000)
        .map(|i| 20.0 + (i % 7) as f32)
        .collect::<Vec<_>>();
    let original = VirtualListMetrics::variable(heights.iter().copied()).unwrap();
    let next = original
        .with_heights([(49_999, 31.0), (50_000, 32.0), (50_001, 33.0)])
        .unwrap();
    let (Extents::Variable(a), Extents::Variable(b)) = (&original.0, &next.0) else {
        panic!()
    };
    let mut before = BTreeSet::new();
    let mut after = BTreeSet::new();
    nodes(a, &mut before);
    nodes(b, &mut after);
    assert!(after.difference(&before).count() < 40);
    assert!(after.intersection(&before).count() > before.len() - 40);
    assert!(Arc::ptr_eq(
        b,
        match &next.with_heights([(50_000, 32.0)]).unwrap().0 {
            Extents::Variable(p) => p,
            _ => panic!(),
        }
    ));
    let dense = next
        .with_heights((0..heights.len()).map(|i| (i, 10.0 + (i % 3) as f32)))
        .unwrap();
    assert_eq!(
        dense,
        VirtualListMetrics::variable((0..heights.len()).map(|i| 10.0 + (i % 3) as f32)).unwrap()
    );
    assert_eq!(original.height(50_000).unwrap(), heights[50_000]);
}
#[test]
fn updated_total_overflow_and_invalid_input_leave_old_snapshot_usable() {
    let metrics = VirtualListMetrics::variable([20.0, 25.0]).unwrap();
    assert!(
        metrics
            .with_heights([(0, f32::MAX), (1, f32::MAX)])
            .is_err()
    );
    assert!(metrics.with_heights([(0, 21.0), (5, 12.0)]).is_err());
    assert_eq!(metrics.total(), 45.0);
    assert_eq!(metrics.height(0).unwrap(), 20.0);
    assert_eq!(
        metrics.with_heights([(0, 21.0), (0, 20.0)]).unwrap(),
        metrics
    );
}

#[test]
fn fractional_heights_and_updates_share_public_window_and_reveal_semantics() {
    let heights = (0..1025)
        .map(|i| match i % 4 {
            0 => 16.7,
            1 => 23.1,
            2 => 37.35,
            _ => 12.25,
        })
        .collect::<Vec<_>>();
    let original = VirtualListMetrics::variable(heights.iter().copied()).unwrap();
    check(&original, &heights);
    let mut next = heights.clone();
    for (i, value) in next.iter_mut().enumerate().filter(|(i, _)| i % 43 == 0) {
        *value += i as f32 * 0.01;
    }
    let updated = original
        .with_heights(next.iter().copied().enumerate())
        .unwrap();
    check(&updated, &next);
    for (i, &height) in next.iter().enumerate() {
        let shown = updated.reveal(i, 0.0, 80.0).unwrap();
        let window = updated.window(shown, 80.0, 0.0, 1026).unwrap();
        assert!(window.range.contains(&i), "reveal {i}, height {height}");
    }
}

#[test]
fn sparse_height_index_rebases_a_far_reading_key_without_payload_or_old_snapshot_changes() {
    struct Item;
    impl Component for Item {
        type Props = ();
        fn mount(_: ReadProp<()>, _: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
            unreachable!()
        }
    }
    let old = VirtualListItems::<Item>::keyed(
        VirtualListMetrics::fixed(100_000, 20.0).unwrap(),
        0..100_000,
        |_| panic!("offscreen payload"),
    )
    .unwrap();
    let next = old
        .with_heights([(100, 30.0), (20_000, 35.0), (49_999, 25.0), (70_000, 40.0)])
        .unwrap();
    assert!(Rc::ptr_eq(&old.row, &next.row));
    assert!(Arc::ptr_eq(
        old.keys.as_ref().unwrap(),
        next.keys.as_ref().unwrap()
    ));
    let r = old
        .rebase(
            &next,
            [0.0, 1_000_005.0],
            80.0,
            80.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(r.key, Some(50_000));
    assert_eq!(r.offset, [0.0, 1_000_035.0]);
    assert_eq!(old.metrics.total(), 2_000_000.0);
    assert_eq!(next.metrics.total(), 2_000_050.0);
}
