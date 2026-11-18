use super::*;
use crate::retained::input::*;
use crate::retained::ui_ir::UiNodeId;
use std::cell::{Cell, RefCell};
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
#[test]
fn metrics_fixed_variable_boundaries_limits_and_reveal() {
    let fixed = VirtualListMetrics::fixed(100_000, 20.0).unwrap();
    assert_eq!(fixed.window(1000.0, 100.0, 20.0, 8).unwrap().range, 49..56);
    assert_eq!(
        fixed.window(f32::MAX, 100.0, 0.0, 8).unwrap().range,
        99_995..100_000
    );
    assert_eq!(fixed.reveal(50, 0.0, 100.0).unwrap(), 920.0);
    assert_eq!(fixed.reveal(50, 1100.0, 100.0).unwrap(), 1000.0);
    assert!(fixed.window(0.0, 100.0, 0.0, 4).is_err());
    let variable = VirtualListMetrics::variable([10.0, 20.0, 30.0, 40.0]).unwrap();
    assert_eq!(variable.window(10.0, 20.0, 0.0, 2).unwrap().range, 1..2);
    let window = variable.window(29.0, 2.0, 0.0, 2).unwrap();
    assert_eq!(window.range, 1..3);
    assert_eq!((window.leading, window.trailing), (10.0, 40.0));
    assert!(VirtualListMetrics::variable([1.0, 0.0]).is_err());
    assert!(VirtualListMetrics::fixed(100, f32::MAX).is_err());
    assert_eq!(
        VirtualListMetrics::fixed(0, 20.0)
            .unwrap()
            .window(0.0, 80.0, 20.0, 3)
            .unwrap()
            .range,
        0..0
    );
    assert_eq!(fixed.window(120.0, 0.0, 100.0, 1).unwrap().range, 0..0);
}

#[derive(Clone)]
struct Props {
    key: u64,
    height: f32,
    color: u32,
    live: Rc<Cell<usize>>,
    mounts: Rc<Cell<usize>>,
    handles: Rc<RefCell<BTreeMap<u64, (ScopeId, NodeId, Signal<u32>)>>>,
}
impl PartialEq for Props {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.height == other.height
            && self.color == other.color
            && Rc::ptr_eq(&self.live, &other.live)
    }
}
struct Item;
impl Component for Item {
    type Props = Props;
    fn mount(
        props: ReadProp<Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let p = cx.read_prop(props)?.clone();
        let state = cx.state(0, || 7u32)?;
        let color = cx.computed(1, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        let style = cx.computed(2, &[props.dependency()], move |i| {
            Ok(fixed(100.0, i.read_prop(props)?.height))
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(color, Property::color(node))?;
        cx.bind(style, Property::layout_style(node))?;
        p.handles
            .borrow_mut()
            .insert(p.key, (cx.scope(), node, state));
        cx.effect(0, &[], move |_| {
            p.live.set(p.live.get() + 1);
            p.mounts.set(p.mounts.get() + 1);
            let live = p.live.clone();
            Some(Box::new(move || live.set(live.get() - 1)))
        })?;
        Ok(())
    }
}
struct Fixture {
    ui: LogicalUi,
    scope: ScopeId,
    host: NodeId,
    scroll: Signal<[f32; 2]>,
    props: VirtualListProps<Item>,
    live: Rc<Cell<usize>>,
    mounts: Rc<Cell<usize>>,
    calls: Rc<Cell<usize>>,
    handles: Rc<RefCell<BTreeMap<u64, (ScopeId, NodeId, Signal<u32>)>>>,
}
fn fixture(metrics: VirtualListMetrics) -> Fixture {
    let mut ui =
        LogicalUi::new_layout(fixed(140.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scroll = ui.state(root, 0, || [0.0; 2]).unwrap();
    let live = Rc::new(Cell::new(0));
    let mounts = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let handles = Rc::new(RefCell::new(BTreeMap::new()));
    let (a, b, c, d, m) = (
        live.clone(),
        mounts.clone(),
        calls.clone(),
        handles.clone(),
        metrics.clone(),
    );
    let items = VirtualListItems::<Item>::new(metrics, move |index| {
        c.set(c.get() + 1);
        Ok((
            index as u64,
            Props {
                key: index as u64,
                height: m.height(index)?,
                color: color(index),
                live: a.clone(),
                mounts: b.clone(),
                handles: d.clone(),
            },
        ))
    });
    let mut props = VirtualListProps::new(items, scroll, [100.0, 80.0]);
    props.overscan = 20.0;
    let scope = ui
        .edit_local_frame(
            LayoutConstraints::viewport(140.0, 100.0),
            [140.0, 100.0],
            |_| Ok(()),
            |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, props.clone())]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let host = ui.renders(scope).unwrap()[0];
    Fixture {
        ui,
        scope,
        host,
        scroll,
        props,
        live,
        mounts,
        calls,
        handles,
    }
}
fn color(index: usize) -> u32 {
    if index % 2 == 0 {
        0x225577ff
    } else {
        0x773322ff
    }
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [140, 100],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn reference(metrics: &VirtualListMetrics, offset: f32) -> FramePacket {
    // Independent full data reference, including offscreen rows; no window math.
    let mut tree = UiTree::new_layout(fixed(140.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            fixed(100.0, 80.0).overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    for i in 0..metrics.len() {
        tree.append_layout(host, fixed(100.0, metrics.height(i).unwrap()), color(i))
            .unwrap();
    }
    let mut tx = Transaction::default();
    tx.scroll(host, [0.0, offset]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([140.0, 100.0]).unwrap()
}
#[test]
fn hundred_thousand_rows_keep_live_nodes_sources_and_callbacks_bounded() {
    let mut f = fixture(VirtualListMetrics::fixed(100_000, 20.0).unwrap());
    assert_eq!(f.live.get(), 5);
    assert_eq!(f.calls.get(), 5);
    let retained = f.handles.borrow()[&3];
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 40.0]))
        .unwrap();
    assert_eq!(f.handles.borrow()[&3], retained);
    assert_eq!(f.live.get(), 6);
    let calls = f.calls.get();
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 40.0]))
        .unwrap();
    assert_eq!(f.calls.get(), calls);
    for index in [10_000, 50_000, 99_996, 4, 99_996] {
        f.ui.transaction(|tx| tx.set(f.scroll, [0.0, index as f32 * 20.0]))
            .unwrap();
        assert!(f.live.get() <= 6);
        assert!(f.ui.reactive().source_count() < 100);
        let frame = f.ui.published_local_frame().unwrap().resolve();
        assert!(frame.items.len() < 20);
        assert_eq!(
            f.ui.input_scene()
                .unwrap()
                .scroll_state(UiNodeId::new(f.host.slot as u64, f.host.generation))
                .unwrap()
                .maximum[1],
            1_999_920.0
        );
    }
    assert!(f.ui.read(retained.2).is_err());
    f.ui.reconcile_keys(f.ui.root_scope(), &[]).unwrap();
    assert_eq!(f.live.get(), 0);
    assert!(f.mounts.get() < 45);
    assert!(f.calls.get() < 50);
}
#[test]
fn variable_rows_wheel_and_programmatic_scroll_match_full_flex() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_virtual_list_matches_full_reference() {
    run(true);
}
fn run(hardware: bool) {
    let metrics =
        VirtualListMetrics::variable((0..80).map(|i| if i % 3 == 0 { 30.0 } else { 20.0 }))
            .unwrap();
    let mut f = fixture(metrics.clone());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = f.ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    for offset in [0.0, 25.0, 700.0, 1_780.0, 10.0] {
        f.ui.transaction(|tx| tx.set(f.scroll, [0.0, offset]))
            .unwrap();
        let frame = f.ui.published_local_frame().unwrap().clone();
        let full = reference(&metrics, offset);
        assert_eq!(
            pixels(frame.resolve()),
            pixels(full.clone()),
            "offset {offset}"
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("virtual list adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [140, 100], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [140, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    let mut router = UiInputRouter::new();
    router
        .adopt(f.ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    let result =
        f.ui.handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 100.0],
            },
        )
        .unwrap();
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 110.0]);
    assert_eq!(result.outcomes[0].consumed, [0.0, 100.0]);
    assert_eq!(result.outcomes[0].remaining, [0.0; 2]);
    assert_eq!(
        pixels(result.frame.resolve()),
        pixels(reference(&metrics, 110.0))
    );
    assert!(f.ui.children(f.scope).is_ok());
    drop(f);
    assert_eq!(pixels(old.resolve()), old_pixels);
}
#[test]
fn invalid_or_duplicate_selected_rows_rollback_sources_lifecycle_and_frame() {
    let mut f = fixture(VirtualListMetrics::fixed(100, 20.0).unwrap());
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    let before = f.ui.published_local_frame().unwrap().clone();
    let count = f.live.get();
    let mut bad = f.props.clone();
    bad.max_rows = 1;
    assert!(
        f.ui.edit(
            |_| Ok(()),
            |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, bad)])
        )
        .is_err()
    );
    assert!(before.same(f.ui.published_local_frame().unwrap()));
    assert_eq!(f.live.get(), count);
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0; 2]);
    let sample = f.props.items.clone();
    let mut duplicate = f.props.clone();
    duplicate.items = VirtualListItems::new(sample.metrics.clone(), move |i| {
        let (_, p) = (sample.row)(i)?;
        Ok((0, p))
    });
    assert!(matches!(
        f.ui.edit(
            |_| Ok(()),
            |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, duplicate)])
        ),
        Err(Error::DuplicateKey)
    ));
    assert!(before.same(f.ui.published_local_frame().unwrap()));
    assert_eq!(f.live.get(), count);
}

#[test]
fn resize_data_shrink_and_reorder_keep_current_keyed_state() {
    let mut f = fixture(VirtualListMetrics::fixed(100, 20.0).unwrap());
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 1000.0]))
        .unwrap();
    let retained = f.handles.borrow()[&50];
    f.ui.transaction(|tx| tx.set(retained.2, 91)).unwrap();
    let data = f.props.items.clone();
    let mut changed = f.props.clone();
    changed.viewport = [100.0, 100.0];
    changed.items = VirtualListItems::new(data.metrics.clone(), move |i| {
        let index = if i == 50 {
            51
        } else if i == 51 {
            50
        } else {
            i
        };
        (data.row)(index)
    });
    f.ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, changed.clone())]),
    )
    .unwrap();
    assert_eq!(f.handles.borrow()[&50], retained);
    assert_eq!(*f.ui.read(retained.2).unwrap(), 91);
    let (live, mounts, handles) = (f.live.clone(), f.mounts.clone(), f.handles.clone());
    changed.items =
        VirtualListItems::new(VirtualListMetrics::fixed(3, 20.0).unwrap(), move |index| {
            Ok((
                index as u64,
                Props {
                    key: index as u64,
                    height: 20.0,
                    color: color(index),
                    live: live.clone(),
                    mounts: mounts.clone(),
                    handles: handles.clone(),
                },
            ))
        });
    f.ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, changed)]),
    )
    .unwrap();
    assert_eq!(f.live.get(), 3);
    assert_eq!(
        f.ui.input_scene()
            .unwrap()
            .scroll_state(UiNodeId::new(f.host.slot as u64, f.host.generation))
            .unwrap()
            .offset,
        [0.0; 2]
    );
    assert!(f.ui.read(retained.2).is_err());
    // The source is the caller's requested offset; used offset is clamped. Its
    // application policy can explicitly normalize before later appending data.
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 1000.0]);
}

#[test]
fn virtual_row_retirement_preserves_wheel_residual_for_outer_default_and_rollback() {
    let mut f = fixture(VirtualListMetrics::fixed(100, 20.0).unwrap());
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    // Outer normal Scroll owns no source and keeps its existing default path.
    let outer =
        f.ui.edit(
            |_| Ok(()),
            |(), edit| {
                let outer = edit.append_layout(
                    root,
                    render,
                    fixed(110.0, 60.0).overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
                    0,
                )?;
                edit.reparent(f.host, outer)?;
                let tail = edit.reconcile_keys(root, &[0, 9])?[1];
                edit.append_layout(tail, outer, fixed(100.0, 100.0), 0)?;
                Ok(outer)
            },
        )
        .unwrap()
        .result
        .value;
    let mut router = UiInputRouter::new();
    router
        .adopt(f.ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    let old_row = f.handles.borrow()[&0].1;
    let result =
        f.ui.handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 1950.0],
            },
        )
        .unwrap();
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 1920.0]);
    assert!(f.ui.reactive().tree().node(old_row).is_err());
    assert_eq!(
        f.ui.reactive().tree().node(outer).unwrap().scroll,
        [0.0, 30.0]
    );
    assert_eq!(result.outcomes[0].consumed, [0.0, 1950.0]);
    assert_eq!(result.outcomes[0].remaining, [0.0; 2]);
    f.ui.adopt_input_scene(
        &mut router,
        f.ui.input_scene().unwrap(),
        UiInputEvidence::ImageInstalled,
    )
    .unwrap();
    let opacity =
        f.ui.computed(root, 77, &[f.scroll.dependency()], move |i| {
            Ok(if i.read(f.scroll)?[1] < 1900.0 {
                2.0
            } else {
                1.0
            })
        })
        .unwrap();
    f.ui.bind(opacity, Property::opacity(f.host)).unwrap();
    let before = f.ui.published_local_frame().unwrap().clone();
    let scene = router.scene_id();
    let live = f.live.get();
    assert!(
        f.ui.handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, -1000.0]
            }
        )
        .is_err()
    );
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 1920.0]);
    assert_eq!(f.live.get(), live);
    assert_eq!(router.scene_id(), scene);
    assert!(before.same(f.ui.published_local_frame().unwrap()));
}

#[test]
fn controlled_scroll_steps_validate_and_never_reverse_residual_at_float_boundaries() {
    let state = UiScrollState {
        offset: [16_777_216.0, 9.0],
        maximum: [20_000_000.0, 10.0],
        wheel_axes: [true, true],
    };
    let step = state.consume([1.0, 5.0]).unwrap();
    assert_eq!(step.offset, [16_777_216.0, 10.0]);
    assert_eq!(step.remaining, [1.0, 4.0]);
    assert_eq!(step.consumed, [0.0, 1.0]);
    assert!(state.consume([0.0, f32::NAN]).is_err());
    assert!(
        UiScrollState {
            offset: [-1.0, 0.0],
            ..state
        }
        .consume([0.0; 2])
        .is_err()
    );
}

#[test]
fn mounted_row_focus_reveal_capture_cancel_and_task_retirement_follow_installed_scene() {
    let mut f = fixture(VirtualListMetrics::fixed(100, 20.0).unwrap());
    let (scope, node, _) = f.handles.borrow()[&0];
    let workers = Rc::new(RefCell::new(None));
    let slot = workers.clone();
    let task =
        f.ui.start_task(scope, 0, move |context: UiTaskContext<u32>| {
            *slot.borrow_mut() = Some(context)
        })
        .unwrap();
    f.ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.interaction(
                scope,
                node,
                Some(UiInteraction {
                    focus: UiFocusPolicy::Tab,
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(f.ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    f.ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2]))
        .unwrap();
    assert!(router.focused().is_some() && router.captured().is_some());
    // Explicit reveal selects the off-window row through ordinary source work.
    let next = f.props.items.metrics.reveal(80, 0.0, 80.0).unwrap();
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, next]))
        .unwrap();
    assert_eq!(task.status(), UiTaskStatus::Cancelled);
    assert_eq!(workers.borrow_mut().take().unwrap().complete(9), Err(9));
    // Until image installation router still carries the old scene; adoption
    // clears retired focus/capture without making old row IDs address new rows.
    assert!(router.focused().is_some());
    f.ui.adopt_input_scene(
        &mut router,
        f.ui.input_scene().unwrap(),
        UiInputEvidence::ImageInstalled,
    )
    .unwrap();
    assert!(router.focused().is_none() && router.captured().is_none());
    let (new_scope, new_node, _) = f.handles.borrow()[&80];
    f.ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.interaction(
                new_scope,
                new_node,
                Some(UiInteraction {
                    focus: UiFocusPolicy::Tab,
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    let scene = f.ui.input_scene().unwrap();
    let id = scene.node_id(new_node).unwrap();
    f.ui.adopt_input_scene(&mut router, scene, UiInputEvidence::ImageInstalled)
        .unwrap();
    f.ui.handle_input(&mut router, UiInput::Focus(Some(id)))
        .unwrap();
    assert_eq!(router.focused(), Some(id));
}

#[test]
fn virtual_list_composes_with_proven_two_axis_flex_allocation_on_resize() {
    let mut f = fixture(VirtualListMetrics::fixed(100_000, 20.0).unwrap());
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    let props = f.props.clone();
    let host =
        f.ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(root, &[])?;
                edit.properties().layout_style(
                    render,
                    LayoutStyle::row()
                        .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
                );
                let item = LayoutStyle::column()
                    .flex_basis(LayoutDimension::Px(0.0))
                    .flex_grow(1.0)
                    .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0));
                let host = edit.append_layout(root, render, item.clone(), 0)?;
                let mut other = item;
                other.flex_grow = 2.0;
                edit.append_layout(root, render, other, 0)?;
                Ok(host)
            },
        )
        .unwrap()
        .result
        .value;
    for viewport in [[150.0, 100.0], [180.0, 140.0], [120.0, 80.0]] {
        f.ui.edit_local_frame(
            LayoutConstraints::viewport(viewport[0], viewport[1]),
            viewport,
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |input, edit| {
                    let size = [
                        input.known_size.width.unwrap(),
                        input.known_size.height.unwrap(),
                    ];
                    assert_eq!(size, [viewport[0] / 3.0, viewport[1]]);
                    let mut props = props.clone();
                    props.viewport = size;
                    edit.components::<VirtualList<Item>>(root, host, [(1, props)])?;
                    Ok(())
                })
            },
        )
        .unwrap();
        let list = f.ui.children(root).unwrap()[0];
        let list_host = f.ui.renders(list).unwrap()[0];
        assert_eq!(
            f.ui.layout_result(list_host).unwrap().size,
            [viewport[0] / 3.0, viewport[1]]
        );
        assert!(f.live.get() <= 9);
        assert!(f.ui.reactive().source_count() < 140);
    }
}
