use super::*;
use std::cell::RefCell;

fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixed(w: u32, h: u32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w as f32), LayoutDimension::Px(h as f32))
        .flex_shrink(0.0)
}
fn fixture() -> LogicalUi {
    LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap()
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
#[derive(Clone)]
struct Row {
    width: u32,
    color: u32,
    mounts: Rc<Cell<usize>>,
    cleanup: Rc<Cell<usize>>,
    handles: Rc<RefCell<Vec<(ReadProp<Row>, Signal<u32>, NodeId)>>>,
}
impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.color == other.color
            && Rc::ptr_eq(&self.mounts, &other.mounts)
            && Rc::ptr_eq(&self.cleanup, &other.cleanup)
            && Rc::ptr_eq(&self.handles, &other.handles)
    }
}
struct Item;
impl Component for Item {
    type Props = Row;
    fn mount(props: ReadProp<Row>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let p = cx.read_prop(props)?.clone();
        let height = cx.state(0, || 10u32)?;
        let style = cx.computed(1, &[props.dependency(), height.dependency()], move |i| {
            Ok(fixed(i.read_prop(props)?.width, *i.read(height)?))
        })?;
        let color = cx.computed(2, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(color, Property::color(node))?;
        p.handles.borrow_mut().push((props, height, node));
        cx.effect(0, &[], move |_| {
            p.mounts.set(p.mounts.get() + 1);
            let cleanup = p.cleanup.clone();
            Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
        })?;
        Ok(())
    }
}
// A different type with identical Props must never borrow Item's local state.
struct OtherItem;
impl Component for OtherItem {
    type Props = Row;
    fn mount(props: ReadProp<Row>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        Item::mount(props, cx)
    }
}
struct Multi;
impl Component for Multi {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        for (w, h) in [(8, 4), (12, 6)] {
            let node = cx.append_layout(fixed(w, h), 0)?;
            cx.bind_prop(props, Property::color(node))?;
        }
        Ok(())
    }
}
#[test]
fn component_sites_independent_lists_and_selected_types_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_component_sites_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("component sites adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let mounts = Rc::new(Cell::new(0));
        let cleanup = Rc::new(Cell::new(0));
        let handles = Rc::new(RefCell::new(vec![]));
        let row = |width, color| Row {
            width,
            color,
            mounts: mounts.clone(),
            cleanup: cleanup.clone(),
            handles: handles.clone(),
        };
        let mut sites = vec![];
        let mut old_site = None;
        let mut retained = None;
        let mut old = None;
        for phase in 0..5 {
            let declare = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                if phase == 0 {
                    sites = edit.component_sites(owner, &[10, 20, 30])?;
                    edit.components_at::<Multi>(sites[2], host, [(1, 0x0000ffff)])?;
                }
                if phase == 2 {
                    let reordered = edit.component_sites(owner, &[30, 20, 10])?;
                    assert_eq!(reordered, [sites[2], sites[1], sites[0]]);
                }
                if phase == 3 {
                    old_site = Some(sites[1]);
                    edit.component_sites(owner, &[30, 10])?;
                }
                if phase == 4 {
                    let next = edit.component_sites(owner, &[30, 20, 10])?;
                    assert_ne!(next[1], old_site.unwrap());
                    sites[1] = next[1];
                }
                let rows = if phase == 0 {
                    vec![(1, row(20, 0xff0000ff)), (2, row(30, 0x00ff00ff))]
                } else {
                    vec![(2, row(35, 0x00ffffff)), (1, row(25, 0xffff00ff))]
                };
                let list = edit.components_at::<Item>(sites[0], host, rows)?;
                if phase == 0 {
                    retained = Some(list[0]);
                } else {
                    assert_eq!(list[1], retained.unwrap());
                }
                if phase < 2 {
                    edit.components_at::<Item>(sites[1], host, [(1, row(40, 0xff00ffff))])?;
                } else if phase != 3 {
                    edit.components_at::<OtherItem>(sites[1], host, [(1, row(45, 0xff00ffff))])?;
                }
                Ok(())
            };
            if local {
                ui.edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), declare)
                    .unwrap();
            } else {
                ui.edit_frame(constraints(), [180.0; 2], |_| Ok(()), declare)
                    .unwrap();
            }
            let handle = handles.borrow()[0].clone();
            assert_eq!(ui.renders(retained.unwrap()).unwrap(), [handle.2]);
            assert_eq!(*ui.read(handle.1).unwrap(), 10);
            assert_eq!(
                ui.read_prop(handle.0).unwrap().width,
                if phase == 0 { 20 } else { 25 }
            );
            assert_eq!(mounts.get(), [3, 3, 4, 4, 5][phase]);
            assert_eq!(cleanup.get(), [0, 0, 1, 2, 2][phase]);
            for site in &sites {
                if Some(*site) != old_site || phase == 4 {
                    assert!(ui.renders(site.scope()).unwrap().is_empty());
                }
            }
            let list = if phase == 0 {
                vec![(20, 10, 0xff0000ff), (30, 10, 0x00ff00ff)]
            } else {
                vec![(35, 10, 0x00ffffff), (25, 10, 0xffff00ff)]
            };
            let mut expected = vec![];
            if phase >= 2 {
                expected.extend([(8, 4, 0x0000ffff), (12, 6, 0x0000ffff)]);
            }
            if (2..5).contains(&phase) && phase != 3 {
                expected.push((45, 10, 0xff00ffff));
            }
            expected.extend(list);
            if phase < 2 {
                expected.extend([
                    (40, 10, 0xff00ffff),
                    (8, 4, 0x0000ffff),
                    (12, 6, 0x0000ffff),
                ]);
            }
            let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            for (w, h, color) in expected {
                full.append_layout(full.root(), fixed(w, h), color).unwrap();
            }
            let reference = full.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            let frame = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            assert_eq!(
                ui.layout_result(host).unwrap(),
                full.layout_result(full.root()).unwrap()
            );
            assert_eq!(pixels(frame.clone()), pixels(reference.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit(&frame, [180, 180], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&reference, [180, 180], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            old.get_or_insert_with(|| (frame.clone(), pixels(frame)));
        }
        drop(ui);
        assert_eq!(mounts.get(), cleanup.get());
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

struct Nested;
impl Component for Nested {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let sites = cx.component_sites(&[10, 20])?;
        cx.components_at::<Multi>(sites[0], host, [(1, 0xff0000ff)])?;
        cx.layout_producer(
            0,
            host,
            LayoutBuildKind::Content,
            &[],
            LayoutProducer::new(move |_, edit| {
                let color = *edit.read_prop(props)?;
                edit.components_at::<Multi>(sites[1], host, [(1, color)])?;
                Ok(())
            }),
        )?;
        Ok(())
    }
}
#[test]
fn component_sites_context_and_content_producers_share_normal_retirement() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let source = ui.state(owner, 0, || true).unwrap();
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                owner,
                0,
                host,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    let keys = if *edit.read(source)? {
                        vec![10, 20]
                    } else {
                        vec![20]
                    };
                    let sites = edit.component_sites(owner, &keys)?;
                    if keys.len() == 2 {
                        edit.components_at::<Nested>(sites[0], host, [(1, 0x00ff00ff)])?;
                    }
                    edit.components_at::<Multi>(*sites.last().unwrap(), host, [(1, 0x0000ffff)])?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.layout_producers.len(), 2);
    let sites = ui.children(owner).unwrap().to_vec();
    let child = ui.children(sites[0]).unwrap()[0];
    let sibling = ui.children(sites[1]).unwrap()[0];
    let frame = ui.published_local_frame().unwrap().clone();
    assert_eq!(ui.layout_result(host).unwrap().size, [12.0, 30.0]);
    ui.transaction(|tx| tx.set(source, false)).unwrap();
    assert_eq!(ui.layout_producers.len(), 1);
    assert_eq!(ui.children(owner).unwrap(), [sites[1]]);
    assert_eq!(ui.children(sites[1]).unwrap(), [sibling]);
    assert!(ui.children(child).is_err());
    assert_eq!(ui.layout_result(host).unwrap().size, [12.0, 10.0]);
    assert_ne!(
        pixels(frame.resolve()),
        pixels(ui.published_local_frame().unwrap().resolve())
    );
}

#[test]
fn component_sites_stale_foreign_and_removed_parent_reject_even_when_caught() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let sites = ui
        .edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[1, 2]))
        .unwrap()
        .result
        .value;
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.component_sites(owner, &[2])?;
            Ok(())
        },
    )
    .unwrap();
    let mut foreign = fixture();
    let other = foreign.root_scope();
    let foreign_site = foreign
        .edit(|_| Ok(()), |(), edit| edit.component_sites(other, &[1]))
        .unwrap()
        .result
        .value[0];
    for site in [sites[0], foreign_site] {
        assert!(matches!(
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    assert!(
                        edit.components_at::<Multi>(site, host, [(1, 0u32)])
                            .is_err()
                    );
                    Ok(())
                }
            ),
            Err(Error::InvalidComponent)
        ));
    }
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.component_sites(owner, &[])?;
                assert!(matches!(
                    edit.components_at::<Multi>(sites[1], host, [(1, 0u32)]),
                    Err(Error::InvalidScope)
                ));
                Ok(())
            }
        ),
        Err(Error::InvalidComponent)
    ));
    assert_eq!(ui.children(owner).unwrap(), [sites[1].scope()]);
}

#[test]
fn component_sites_duplicate_and_final_failure_preserve_all_regions() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let sites = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let sites = edit.component_sites(owner, &[1, 2])?;
                for site in &sites {
                    edit.components_at::<Multi>(*site, host, [(1, 0xff0000ff)])?;
                }
                Ok(sites)
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let before = ui.published_local_frame().unwrap().clone();
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[2, 2])),
        Err(Error::DuplicateKey)
    ));
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.component_sites(owner, &[2])?;
                edit.components_at::<Multi>(sites[1], host, [(1, 0x00ff00ff)])?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(
        ui.children(owner).unwrap(),
        [sites[0].scope(), sites[1].scope()]
    );
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn component_sites_arm_keys_and_empty_selection_reset_state_after_retirement() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    // An ordinary scope with the same key is not silently adopted as a site.
    let manual = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let site = ui
        .edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[1]))
        .unwrap()
        .result
        .value[0];
    assert_ne!(manual, site.scope());
    let mounts = Rc::new(Cell::new(0));
    let cleanup = Rc::new(Cell::new(0));
    let handles = Rc::new(RefCell::new(vec![]));
    let props = Row {
        width: 20,
        color: 0xff0000ff,
        mounts: mounts.clone(),
        cleanup: cleanup.clone(),
        handles: handles.clone(),
    };
    let mut previous = None;
    for key in [10, 20, 10] {
        let scope = ui
            .edit(
                |_| Ok(()),
                |(), edit| edit.components_at::<Item>(site, host, [(key, props.clone())]),
            )
            .unwrap()
            .result
            .value[0];
        assert_ne!(Some(scope), previous);
        let height = handles.borrow().last().unwrap().1;
        assert_eq!(*ui.read(height).unwrap(), 10);
        ui.transaction(|tx| tx.set(height, 17)).unwrap();
        previous = Some(scope);
    }
    assert_eq!((mounts.get(), cleanup.get()), (3, 2));
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.components_at::<Item>(site, host, []),
    )
    .unwrap();
    assert!(ui.children(site.scope()).unwrap().is_empty());
    assert_eq!(cleanup.get(), 3);
    let next = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.components_at::<Item>(site, host, [(10, props)]),
        )
        .unwrap()
        .result
        .value[0];
    assert_ne!(Some(next), previous);
    assert_eq!(*ui.read(handles.borrow().last().unwrap().1).unwrap(), 10);
    assert_eq!((mounts.get(), cleanup.get()), (4, 3));
}

struct WrongContext;
impl Component for WrongContext {
    type Props = ComponentSite;
    fn mount(
        props: ReadProp<ComponentSite>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let site = *cx.read_prop(props)?;
        assert!(matches!(
            cx.components_at::<Multi>(site, cx.host(), [(1, 0u32)]),
            Err(Error::InvalidComponent)
        ));
        // A swallowed capability error must not adopt this partially mounted instance.
        Ok(())
    }
}
#[test]
fn component_sites_context_rejects_another_owners_region_and_poisons_edit() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let sites = ui
        .edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[1, 2]))
        .unwrap()
        .result
        .value;
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.components_at::<WrongContext>(sites[0], host, [(1, sites[1])])
        ),
        Err(Error::InvalidComponent)
    ));
    assert!(ui.children(sites[0].scope()).unwrap().is_empty());
    assert!(ui.children(sites[1].scope()).unwrap().is_empty());
}

#[test]
fn component_sites_equal_declarations_keep_outputs_and_do_not_request_visual_frame() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let declare = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
        let sites = edit.component_sites(owner, &[1, 2])?;
        edit.components_at::<Multi>(sites[0], host, [(1, 0xff0000ff)])?;
        edit.components_at::<Multi>(sites[1], host, [(1, 0x00ff00ff)])?;
        Ok(sites)
    };
    let first = ui
        .edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), declare)
        .unwrap()
        .0
        .result
        .value;
    let frame = ui.published_local_frame().unwrap().clone();
    let next = ui.edit(|_| Ok(()), declare).unwrap();
    assert_eq!(first, next.result.value);
    assert!(!next.result.commit.request_frame);
    assert!(frame.same(ui.published_local_frame().unwrap()));
}
