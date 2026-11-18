use super::*;
use std::cell::Cell;

#[derive(Clone, PartialEq)]
struct Height(f32);
struct Item;
impl Component for Item {
    type Props = Height;
    fn mount(p: ReadProp<Height>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let node = cx.append_layout(LayoutStyle::column(), 0x336699ff)?;
        let style = cx.computed(0, &[p.dependency()], move |i| {
            Ok(LayoutStyle::column()
                .size(
                    LayoutDimension::Percent(1.0),
                    LayoutDimension::Px(i.read_prop(p)?.0),
                )
                .flex_shrink(0.0))
        })?;
        cx.bind(style, Property::layout_style(node))?;
        Ok(())
    }
}
struct Fixture {
    ui: LogicalUi,
    items: Signal<VirtualListItems<Item>>,
    scroll: Signal<[f32; 2]>,
    props: MeasuredVirtualListProps<Item>,
    wrapper: NodeId,
    calls: Rc<Cell<usize>>,
}
fn fixture(count: usize, estimate: f32, actual: f32, offset: f32, overscan: f32) -> Fixture {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(100.0), LayoutDimension::Px(80.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let c = calls.clone();
    let data = VirtualListItems::<Item>::keyed(
        VirtualListMetrics::fixed(count, estimate).unwrap(),
        0..count as u64,
        move |_| {
            c.set(c.get() + 1);
            Ok(Height(actual))
        },
    )
    .unwrap();
    let items = ui.state(root, 0, || data).unwrap();
    let scroll = ui.state(root, 1, || [0.0, offset]).unwrap();
    let mut props = MeasuredVirtualListProps::new(items, scroll, [100.0, 80.0]);
    props.overscan = overscan;
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<Item>>(root, host, [(0, props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    let scope = ui.children(root).unwrap()[0];
    let wrapper = ui.renders(scope).unwrap()[0];
    Fixture {
        ui,
        items,
        scroll,
        props,
        wrapper,
        calls,
    }
}
#[test]
fn mounted_only_height_feedback_preserves_reading_position_and_is_noop_when_stable() {
    let mut f = fixture(100_000, 20.0, 30.0, 45.0, 20.0);
    let old = f.ui.published_local_frame().unwrap().clone();
    let first = f.ui.settle_virtual_list_measurements(10).unwrap();
    assert_eq!((first.measured_rows, first.changed_rows), (7, 7));
    assert!(first.commit.is_some());
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 55.0]);
    let data = f.ui.read(f.items).unwrap();
    assert_eq!(data.metrics.height(0).unwrap(), 20.0);
    assert_eq!(data.metrics.height(1).unwrap(), 30.0);
    assert_eq!(data.metrics.height(8).unwrap(), 20.0);
    assert!(f.calls.get() < 20);
    assert!(f.ui.reactive().source_count() < 100);
    let revision = f.ui.published_local_frame().unwrap().revision();
    let next = f.ui.settle_virtual_list_measurements(10).unwrap();
    assert_eq!(next.changed_rows, 0);
    assert!(next.commit.is_none());
    assert_eq!(f.ui.published_local_frame().unwrap().revision(), revision);
    assert!(old.revision() < revision);
}
#[test]
fn bounded_feedback_can_need_another_turn_and_budget_failure_adopts_nothing() {
    let mut f = fixture(100, 40.0, 20.0, 0.0, 0.0);
    let before = f.ui.read(f.items).unwrap().clone();
    let revision = f.ui.published_local_frame().unwrap().revision();
    assert!(matches!(
        f.ui.settle_virtual_list_measurements(1),
        Err(Error::VirtualListLimit)
    ));
    assert!(*f.ui.read(f.items).unwrap() == before);
    assert_eq!(f.ui.published_local_frame().unwrap().revision(), revision);
    let first = f.ui.settle_virtual_list_measurements(10).unwrap();
    assert_eq!((first.measured_rows, first.changed_rows), (2, 2));
    // New rows exposed by the smaller extents wait for the next host turn.
    assert_eq!(f.ui.read(f.items).unwrap().metrics.height(2).unwrap(), 40.0);
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        1
    );
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        1
    );
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        0
    );
}
#[test]
fn hidden_lists_retain_estimates_and_retirement_removes_feedback() {
    let mut f = fixture(100, 20.0, 30.0, 0.0, 0.0);
    f.ui.edit(
        |_| Ok(()),
        |(), e| {
            e.properties().layout_style(
                f.wrapper,
                LayoutStyle::column().display(LayoutDisplay::None),
            );
            Ok(())
        },
    )
    .unwrap();
    let result = f.ui.settle_virtual_list_measurements(10).unwrap();
    assert_eq!((result.measured_rows, result.changed_rows), (0, 0));
    assert_eq!(f.ui.read(f.items).unwrap().metrics.height(0).unwrap(), 20.0);
    let root = f.ui.root_scope();
    f.ui.reconcile_keys(root, &[]).unwrap();
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .measured_rows,
        0
    );
}
#[test]
fn source_ownership_is_exclusive_and_invalid_replacement_rolls_back() {
    let mut f = fixture(100, 20.0, 30.0, 0.0, 0.0);
    let root = f.ui.root_scope();
    let host = f.ui.root_render();
    let result = f.ui.edit(
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<Item>>(
                root,
                host,
                [(0, f.props.clone()), (1, f.props.clone())],
            )?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidVirtualList)));
    assert_eq!(f.ui.children(root).unwrap().len(), 1);
    let bad = VirtualListItems::<Item>::new(VirtualListMetrics::fixed(100, 20.0).unwrap(), |i| {
        Ok((i as u64, Height(30.0)))
    });
    assert!(matches!(
        f.ui.transaction(|tx| tx.set(f.items, bad)),
        Err(Error::InvalidVirtualList)
    ));
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        4
    );
}
#[test]
fn sparse_extents_coalesce_validate_and_share_same_value_snapshots() {
    let metrics = VirtualListMetrics::variable([10.0, 20.0, 30.0]).unwrap();
    let same = metrics.with_heights([(0, 12.0), (0, 10.0)]).unwrap();
    assert_eq!(same, metrics);
    let (Extents::Variable(a), Extents::Variable(b)) = (&metrics.0, &same.0) else {
        panic!()
    };
    assert!(Arc::ptr_eq(a, b));
    assert_eq!(
        metrics
            .with_heights([(0, 15.0), (2, 35.0)])
            .unwrap()
            .total(),
        70.0
    );
    for update in [(3, 10.0), (1, 0.0), (1, f32::NAN)] {
        assert!(metrics.with_heights([update]).is_err());
    }
}

#[test]
fn measurement_feedback_failure_preserves_dataset_scroll_and_complete_frame() {
    let mut f = fixture(100, 20.0, 30.0, 45.0, 20.0);
    let source = f.items;
    let opacity =
        f.ui.computed(f.ui.root_scope(), 99, &[source.dependency()], move |i| {
            Ok(if i.read(source)?.metrics().height(1)? == 20.0 {
                1.0
            } else {
                2.0
            })
        })
        .unwrap();
    f.ui.bind(opacity, Property::opacity(f.wrapper)).unwrap();
    f.ui.local_frame([100.0, 80.0]).unwrap();
    let before = f.ui.read(source).unwrap().clone();
    let revision = f.ui.published_local_frame().unwrap().revision();
    assert!(f.ui.settle_virtual_list_measurements(10).is_err());
    assert!(*f.ui.read(source).unwrap() == before);
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 45.0]);
    assert_eq!(f.ui.published_local_frame().unwrap().revision(), revision);
}

#[test]
fn follow_end_feedback_uses_current_source_and_new_data_is_remeasured() {
    let mut f = fixture(10, 20.0, 30.0, 120.0, 0.0);
    f.props.scroll_policy = VirtualListScrollPolicy::FollowEnd { tolerance: 1.0 };
    let root = f.ui.root_scope();
    let host = f.ui.root_render();
    f.ui.edit(
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<Item>>(root, host, [(0, f.props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    let report = f.ui.settle_virtual_list_measurements(10).unwrap();
    assert_eq!(report.changed_rows, 4);
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 160.0]);
    // A newer user scroll takes precedence over any earlier near-end offset.
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 0.0])).unwrap();
    let replacement = VirtualListItems::<Item>::keyed(
        VirtualListMetrics::fixed(10, 20.0).unwrap(),
        0..10,
        |_| Ok(Height(35.0)),
    )
    .unwrap();
    f.ui.transaction(|tx| tx.set(f.items, replacement)).unwrap();
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        4
    );
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 0.0]);
    assert_eq!(
        f.ui.read(f.items).unwrap().metrics().height(0).unwrap(),
        35.0
    );
}

#[test]
fn zero_width_defers_feedback_and_resolved_frame_entry_is_supported() {
    let mut f = fixture(100, 20.0, 30.0, 0.0, 0.0);
    let root = f.ui.root_scope();
    let host = f.ui.root_render();
    f.props.viewport[0] = 0.0;
    f.ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<Item>>(root, host, [(0, f.props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .measured_rows,
        0
    );
    f.props.viewport[0] = 100.0;
    f.ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<Item>>(root, host, [(0, f.props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        f.ui.settle_virtual_list_measurements(10)
            .unwrap()
            .changed_rows,
        4
    );
}
