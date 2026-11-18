use super::*;
use crate::retained::input::*;
use std::cell::{Cell, RefCell};
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
#[derive(Clone)]
struct Props {
    key: u64,
    height: f32,
    color: u32,
    handles: Rc<RefCell<BTreeMap<u64, (ScopeId, NodeId, Signal<u32>)>>>,
    live: Rc<Cell<usize>>,
}
impl PartialEq for Props {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.height == other.height
            && self.color == other.color
            && Rc::ptr_eq(&self.handles, &other.handles)
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
        let state = cx.state(0, || 17u32)?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        let style = cx.computed(1, &[props.dependency()], move |i| {
            Ok(fixed(100.0, i.read_prop(props)?.height))
        })?;
        let color = cx.computed(2, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(color, Property::color(node))?;
        p.handles
            .borrow_mut()
            .insert(p.key, (cx.scope(), node, state));
        cx.effect(0, &[], move |_| {
            p.live.set(p.live.get() + 1);
            let live = p.live.clone();
            Some(Box::new(move || live.set(live.get() - 1)))
        })?;
        Ok(())
    }
}
type Data = Vec<(u64, f32, u32)>;
struct Fixture {
    ui: LogicalUi,
    items: Signal<VirtualListItems<Item>>,
    scroll: Signal<[f32; 2]>,
    viewport: Signal<[f32; 2]>,
    site: ComponentSite,
    handles: Rc<RefCell<BTreeMap<u64, (ScopeId, NodeId, Signal<u32>)>>>,
    live: Rc<Cell<usize>>,
}
fn items(
    data: &Data,
    handles: &Rc<RefCell<BTreeMap<u64, (ScopeId, NodeId, Signal<u32>)>>>,
    live: &Rc<Cell<usize>>,
) -> VirtualListItems<Item> {
    let rows = data.clone();
    let handles = handles.clone();
    let live = live.clone();
    VirtualListItems::keyed(
        VirtualListMetrics::variable(data.iter().map(|(_, h, _)| *h)).unwrap(),
        data.iter().map(|(key, _, _)| *key),
        move |index| {
            let (key, height, color) = rows[index];
            Ok(Props {
                key,
                height,
                color,
                handles: handles.clone(),
                live: live.clone(),
            })
        },
    )
    .unwrap()
}
fn data() -> Data {
    (0..50)
        .map(|i| (i, 20.0, ((i as u32 * 31) << 24) | 0x3355ff))
        .collect()
}
fn fixture(data: &Data) -> Fixture {
    let mut ui =
        LogicalUi::new_layout(fixed(120.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let handles = Rc::new(RefCell::new(BTreeMap::new()));
    let live = Rc::new(Cell::new(0));
    let items = ui.state(root, 0, || items(data, &handles, &live)).unwrap();
    let scroll = ui.state(root, 1, || [0.0, 65.0]).unwrap();
    let viewport = ui.state(root, 2, || [100.0, 80.0]).unwrap();
    let site = ui
        .edit_local_frame(
            LayoutConstraints::viewport(120.0, 100.0),
            [120.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(root, &[0])?[0];
                edit.component_producer(
                    root,
                    0,
                    host,
                    &[],
                    ComponentProducer::new(move |i| {
                        let mut plan = ComponentPlan::new();
                        let mut props = VirtualListProps::new(
                            i.read(items)?.clone(),
                            scroll,
                            *i.read(viewport)?,
                        );
                        props.overscan = 20.0;
                        plan.select::<VirtualList<Item>>(site, host, 0, props);
                        Ok(plan)
                    }),
                )?;
                Ok(site)
            },
        )
        .unwrap()
        .0
        .result
        .value;
    Fixture {
        ui,
        items,
        scroll,
        viewport,
        site,
        handles,
        live,
    }
}
fn keyed(keys: &[u64], height: f32) -> VirtualListItems<Item> {
    VirtualListItems::keyed(
        VirtualListMetrics::fixed(keys.len(), height).unwrap(),
        keys.iter().copied(),
        |_| panic!("anchor must not build payload"),
    )
    .unwrap()
}
#[test]
fn pure_anchor_insert_remove_reorder_height_change_and_fallback_are_explicit() {
    let old = keyed(&[1, 2, 3, 4, 5, 6], 20.0);
    let inserted = keyed(&[10, 11, 1, 2, 3, 4, 5, 6], 20.0);
    let r = old
        .rebase(
            &inserted,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(
        (r.offset, r.key, r.reason),
        ([0.0, 65.0], Some(2), VirtualListRebaseReason::KeptKey)
    );
    assert!(!r.clamped);
    let removed = keyed(&[1, 3, 4, 5, 6], 20.0);
    let r = old
        .rebase(
            &removed,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(
        (r.offset, r.key, r.reason),
        ([0.0, 5.0], Some(3), VirtualListRebaseReason::FollowingKey)
    );
    let preceding = keyed(&[1, 7, 8, 9, 10], 20.0);
    let r = old
        .rebase(
            &preceding,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(
        (r.offset, r.key, r.reason),
        ([0.0, 25.0], Some(1), VirtualListRebaseReason::PrecedingKey)
    );
    let tall = keyed(&[1, 2, 3, 4, 5, 6], 30.0);
    assert_eq!(
        old.rebase(
            &tall,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible
        )
        .unwrap()
        .offset,
        [0.0, 35.0]
    );
    let shrink = keyed(&[1, 2, 3, 4, 5, 6], 2.0);
    let r = old
        .rebase(
            &shrink,
            [0.0, 35.0],
            4.0,
            4.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(r.key, Some(2));
    assert!(r.offset[1] >= 2.0 && r.offset[1] < 4.0);
    assert!(
        VirtualListItems::<Item>::keyed(
            VirtualListMetrics::fixed(2, 20.0).unwrap(),
            [1, 1],
            |_| unreachable!()
        )
        .is_err()
    );
    assert!(
        VirtualListItems::<Item>::keyed(
            VirtualListMetrics::fixed(2, 20.0).unwrap(),
            [1],
            |_| unreachable!()
        )
        .is_err()
    );
}
#[test]
fn follow_end_only_follows_reader_at_end_and_handles_empty_or_missing_keys() {
    let old = keyed(&[1, 2, 3, 4, 5], 20.0);
    let new = keyed(&[1, 2, 3, 4, 5, 6], 20.0);
    let policy = VirtualListScrollPolicy::FollowEnd { tolerance: 1.0 };
    assert_eq!(
        old.rebase(&new, [0.0, 59.5], 40.0, 40.0, policy)
            .unwrap()
            .offset,
        [0.0, 80.0]
    );
    assert_eq!(
        old.rebase(&new, [0.0, 25.0], 40.0, 40.0, policy)
            .unwrap()
            .offset,
        [0.0, 25.0]
    );
    let empty = keyed(&[], 20.0);
    assert_eq!(
        empty
            .rebase(&new, [0.0; 2], 40.0, 40.0, policy)
            .unwrap()
            .reason,
        VirtualListRebaseReason::FollowedEnd
    );
    assert_eq!(
        old.rebase(
            &empty,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible
        )
        .unwrap()
        .offset,
        [0.0; 2]
    );
    let missing = keyed(&[8, 9, 10, 11, 12], 20.0);
    assert_eq!(
        old.rebase(
            &missing,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepVisible
        )
        .unwrap()
        .reason,
        VirtualListRebaseReason::NoSurvivingKey
    );
    let unindexed = VirtualListItems::<Item>::new(
        VirtualListMetrics::fixed(5, 20.0).unwrap(),
        |_| unreachable!(),
    );
    assert!(
        old.rebase(&unindexed, [0.0; 2], 40.0, 40.0, policy)
            .is_err()
    );
    assert_eq!(
        old.rebase(
            &unindexed,
            [0.0, 25.0],
            40.0,
            40.0,
            VirtualListScrollPolicy::KeepOffset
        )
        .unwrap()
        .offset,
        [0.0, 25.0]
    );
    assert!(
        old.rebase(
            &new,
            [0.0; 2],
            40.0,
            40.0,
            VirtualListScrollPolicy::FollowEnd {
                tolerance: f32::NAN
            }
        )
        .is_err()
    );
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [120, 100],
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
fn reference(data: &Data, offset: f32, height: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(120.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            fixed(100.0, height).overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    for (_, h, color) in data {
        tree.append_layout(host, fixed(100.0, *h), *color).unwrap();
    }
    let mut tx = Transaction::default();
    tx.scroll(host, [0.0, offset]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([120.0, 100.0]).unwrap()
}
#[test]
fn atomic_key_rebase_preserves_live_component_position_and_matches_complete_flex() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_virtual_list_anchor_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let mut rows = data();
    let mut f = fixture(&rows);
    let (_, anchor, state) = f.handles.borrow()[&3];
    f.ui.transaction(|tx| tx.set(state, 42)).unwrap();
    let old = f.ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for stage in 0..4 {
        let expected = match stage {
            0 => {
                rows.insert(0, (100, 30.0, 0x776633ff));
                95.0
            }
            1 => {
                rows[0].1 = 50.0;
                115.0
            }
            2 => {
                rows.remove(0);
                65.0
            }
            _ => {
                let row = rows.remove(3);
                rows.insert(5, row);
                105.0
            }
        };
        let next = items(&rows, &f.handles, &f.live);
        let rebase =
            f.ui.transaction(|tx| {
                tx.rebase_virtual_list(
                    f.items,
                    f.scroll,
                    next,
                    80.0,
                    80.0,
                    VirtualListScrollPolicy::KeepVisible,
                )
            })
            .unwrap()
            .value;
        assert_eq!(rebase.offset, [0.0, expected]);
        assert_eq!(rebase.key, Some(3));
        assert_eq!(*f.ui.read(state).unwrap(), 42);
        let frame = f.ui.published_local_frame().unwrap().clone();
        assert_eq!(frame.resolve_geometry(anchor).unwrap().bounds.y, -5.0);
        let full = reference(&rows, expected, 80.0);
        assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("anchor adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [120, 100], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [120, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    assert!(f.live.get() <= 7);
    drop(f);
    assert_eq!(pixels(old.resolve()), old_pixels);
}
#[test]
fn current_wheel_offset_wins_and_failed_candidate_or_inner_call_keeps_original_tuple() {
    let rows = data();
    let mut f = fixture(&rows);
    let mut router = UiInputRouter::new();
    router
        .adopt(f.ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    f.ui.handle_input(
        &mut router,
        UiInput::Wheel {
            position: [5.0; 2],
            delta: [0.0, 40.0],
        },
    )
    .unwrap();
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 105.0]);
    let mut changed = rows.clone();
    changed.insert(0, (100, 30.0, 0xff0000ff));
    let next = items(&changed, &f.handles, &f.live);
    let before = f.ui.published_local_frame().unwrap().clone();
    let live = f.live.get();
    let root_render = f.ui.root_render();
    assert!(
        f.ui.edit_local_frame(
            LayoutConstraints::viewport(120.0, 100.0),
            [120.0, 100.0],
            |tx| tx.rebase_virtual_list(
                f.items,
                f.scroll,
                next.clone(),
                80.0,
                80.0,
                VirtualListScrollPolicy::KeepVisible
            ),
            |_, edit| {
                edit.properties().opacity(root_render, 2.0);
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 105.0]);
    assert_eq!(f.live.get(), live);
    assert!(before.same(f.ui.published_local_frame().unwrap()));
    let rebase =
        f.ui.transaction(|tx| {
            tx.rebase_virtual_list(
                f.items,
                f.scroll,
                next,
                80.0,
                80.0,
                VirtualListScrollPolicy::KeepVisible,
            )
        })
        .unwrap()
        .value;
    assert_eq!(rebase.offset, [0.0, 135.0]);
    let other = items(&rows, &f.handles, &f.live);
    let readonly =
        f.ui.computed(f.ui.root_scope(), 90, &[f.items.dependency()], move |i| {
            Ok(i.read(f.items)?.clone())
        })
        .unwrap();
    f.ui.transaction(|tx| {
        assert!(matches!(
            tx.rebase_virtual_list(
                readonly,
                f.scroll,
                other,
                80.0,
                80.0,
                VirtualListScrollPolicy::KeepVisible
            ),
            Err(Error::ReadonlySignal)
        ));
        assert_eq!(*tx.read(f.scroll)?, [0.0, 135.0]);
        Ok(())
    })
    .unwrap();
    let other = items(&rows, &f.handles, &f.live);
    let previous = f.ui.read(f.items).unwrap().clone();
    let readonly_scroll =
        f.ui.computed(f.ui.root_scope(), 91, &[f.scroll.dependency()], move |i| {
            Ok(*i.read(f.scroll)?)
        })
        .unwrap();
    f.ui.transaction(|tx| {
        assert!(matches!(
            tx.rebase_virtual_list(
                f.items,
                readonly_scroll,
                other,
                80.0,
                80.0,
                VirtualListScrollPolicy::KeepVisible
            ),
            Err(Error::ReadonlySignal)
        ));
        assert!(*tx.read(f.items)? == previous);
        Ok(())
    })
    .unwrap();
}

#[test]
fn viewport_resize_and_follow_end_adopt_one_coherent_tuple_without_remount() {
    let mut rows = data();
    let mut f = fixture(&rows);
    let initial = f.ui.children(f.site.scope()).unwrap()[0];
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 920.0]))
        .unwrap();
    let mut appended = rows.clone();
    appended.push((100, 25.0, 0x88aaffff));
    let next = items(&appended, &f.handles, &f.live);
    let result =
        f.ui.transaction(|tx| {
            let r = tx.rebase_virtual_list(
                f.items,
                f.scroll,
                next,
                80.0,
                100.0,
                VirtualListScrollPolicy::FollowEnd { tolerance: 1.0 },
            )?;
            tx.set(f.viewport, [100.0, 100.0])?;
            Ok(r)
        })
        .unwrap()
        .value;
    assert_eq!(result.reason, VirtualListRebaseReason::FollowedEnd);
    assert_eq!(result.offset, [0.0, 925.0]);
    assert_eq!(f.ui.children(f.site.scope()).unwrap()[0], initial);
    let scope = f.ui.children(f.site.scope()).unwrap()[0];
    let host = f.ui.renders(scope).unwrap()[0];
    assert_eq!(f.ui.layout_result(host).unwrap().size, [100.0, 100.0]);
    assert_eq!(
        pixels(f.ui.published_local_frame().unwrap().resolve()),
        pixels(reference(&appended, 925.0, 100.0))
    );
    // Reader moves away before the next append. FollowEnd must keep that view.
    f.ui.transaction(|tx| tx.set(f.scroll, [0.0, 45.0]))
        .unwrap();
    rows = appended;
    rows.push((101, 30.0, 0x665544ff));
    let next = items(&rows, &f.handles, &f.live);
    let result =
        f.ui.transaction(|tx| {
            tx.rebase_virtual_list(
                f.items,
                f.scroll,
                next,
                100.0,
                100.0,
                VirtualListScrollPolicy::FollowEnd { tolerance: 1.0 },
            )
        })
        .unwrap()
        .value;
    assert_eq!(result.offset, [0.0, 45.0]);
    assert_eq!(result.key, Some(2));
}

#[test]
fn indexed_hundred_thousand_rows_rebase_never_invokes_offscreen_factories() {
    let old = VirtualListItems::<Item>::keyed(
        VirtualListMetrics::fixed(100_000, 20.0).unwrap(),
        0..100_000,
        |_| panic!("no payload during rebase"),
    )
    .unwrap();
    let new = VirtualListItems::<Item>::keyed(
        VirtualListMetrics::fixed(100_001, 20.0).unwrap(),
        std::iter::once(100_001).chain(0..100_000),
        |_| panic!("no payload during rebase"),
    )
    .unwrap();
    let r = old
        .rebase(
            &new,
            [0.0, 1_000_005.0],
            80.0,
            80.0,
            VirtualListScrollPolicy::KeepVisible,
        )
        .unwrap();
    assert_eq!(r.key, Some(50_000));
    assert_eq!(r.offset, [0.0, 1_000_025.0]);
    assert_eq!(new.key(50_001).unwrap(), 50_000);
}

#[test]
fn derived_tuple_only_sees_final_dataset_and_scroll_and_failure_rolls_back_both() {
    let rows = data();
    let mut f = fixture(&rows);
    let root = f.ui.root_scope();
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    let tuple =
        f.ui.computed(
            root,
            70,
            &[f.items.dependency(), f.scroll.dependency()],
            move |i| {
                let length = i.read(f.items)?.metrics().len();
                let scroll = i.read(f.scroll)?[1];
                if (length == 50 && scroll != 65.0) || (length == 51 && scroll != 95.0) {
                    return Err(Error::InvalidVirtualList);
                }
                calls.set(calls.get() + 1);
                Ok((length, scroll))
            },
        )
        .unwrap();
    let old = f.ui.published_local_frame().unwrap().clone();
    let mut changed = rows.clone();
    changed.insert(0, (100, 30.0, 0x556677ff));
    let next = items(&changed, &f.handles, &f.live);
    let original = f.ui.read(f.items).unwrap().clone();
    assert!(
        f.ui.transaction(|tx| {
            tx.rebase_virtual_list(
                f.items,
                f.scroll,
                next.clone(),
                80.0,
                80.0,
                VirtualListScrollPolicy::KeepOffset,
            )?;
            Ok(())
        })
        .is_err()
    );
    assert!(*f.ui.read(f.items).unwrap() == original);
    assert_eq!(*f.ui.read(tuple).unwrap(), (50, 65.0));
    assert!(old.same(f.ui.published_local_frame().unwrap()));
    let before = count.get();
    f.ui.transaction(|tx| {
        tx.rebase_virtual_list(
            f.items,
            f.scroll,
            next,
            80.0,
            80.0,
            VirtualListScrollPolicy::KeepVisible,
        )
    })
    .unwrap();
    assert_eq!(*f.ui.read(tuple).unwrap(), (51, 95.0));
    assert_eq!(count.get(), before + 1);
}

#[test]
fn ordinary_activation_rebases_data_in_the_same_input_candidate() {
    let rows = data();
    let mut f = fixture(&rows);
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    let mut appended = rows.clone();
    appended.insert(0, (100, 30.0, 0xffbb77ff));
    let next = items(&appended, &f.handles, &f.live);
    let data = f.items;
    let scroll = f.scroll;
    f.ui.edit(
        |_| Ok(()),
        |(), edit| {
            let handler = edit.event_handler(
                root,
                0,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::Activate {
                        cx.rebase_virtual_list(
                            data,
                            scroll,
                            next.clone(),
                            80.0,
                            80.0,
                            VirtualListScrollPolicy::KeepVisible,
                        )?;
                    }
                    Ok(())
                }),
            )?;
            edit.interaction(
                root,
                render,
                Some(UiInteraction {
                    activate_handler: Some(handler),
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
    let result =
        f.ui.handle_input(&mut router, UiInput::PrimaryUp([5.0; 2]))
            .unwrap();
    assert_eq!(*f.ui.read(f.scroll).unwrap(), [0.0, 95.0]);
    assert_eq!(
        pixels(result.frame.resolve()),
        pixels(reference(&appended, 95.0, 80.0))
    );
}
