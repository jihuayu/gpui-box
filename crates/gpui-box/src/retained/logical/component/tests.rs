use super::*;
use std::cell::RefCell;

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
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
    cleanups: Rc<Cell<usize>>,
    handles: Rc<RefCell<Vec<(ReadProp<Row>, Signal<u32>, NodeId)>>>,
}
impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.color == other.color
            && Rc::ptr_eq(&self.mounts, &other.mounts)
            && Rc::ptr_eq(&self.cleanups, &other.cleanups)
            && Rc::ptr_eq(&self.handles, &other.handles)
    }
}
struct RowComponent;
impl Component for RowComponent {
    type Props = Row;
    fn mount(props: ReadProp<Row>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let value = cx.read_prop(props)?.clone();
        let height = cx.state(0, || 10u32)?;
        let style = cx.computed(1, &[props.dependency(), height.dependency()], move |i| {
            Ok(fixed(
                i.read_prop(props)?.width as f32,
                *i.read(height)? as f32,
            ))
        })?;
        let color = cx.computed(2, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(color, Property::color(node))?;
        value.handles.borrow_mut().push((props, height, node));
        let mounts = value.mounts.clone();
        let cleanups = value.cleanups.clone();
        cx.effect(0, &[], move |_| {
            mounts.set(mounts.get() + 1);
            let cleanups = cleanups.clone();
            Some(Box::new(move || cleanups.set(cleanups.get() + 1)))
        })?;
        Ok(())
    }
}
fn row(
    width: u32,
    color: u32,
    mounts: &Rc<Cell<usize>>,
    cleanups: &Rc<Cell<usize>>,
    handles: &Rc<RefCell<Vec<(ReadProp<Row>, Signal<u32>, NodeId)>>>,
) -> Row {
    Row {
        width,
        color,
        mounts: mounts.clone(),
        cleanups: cleanups.clone(),
        handles: handles.clone(),
    }
}

#[test]
fn typed_components_update_payload_reorder_and_retire_without_remounting() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_typed_components_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("typed component adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let mounts = Rc::new(Cell::new(0));
        let cleanups = Rc::new(Cell::new(0));
        let handles = Rc::new(RefCell::new(vec![]));
        let initial = vec![(1u64, 20u32, 0xff0000ff), (2, 30, 0x00ff00ff)];
        let model = ui.state(owner, 0, || initial.clone()).unwrap();
        let (m, c, h) = (mounts.clone(), cleanups.clone(), handles.clone());
        let parent = LayoutProducer::new(move |_, edit| {
            let rows = edit.read(model)?.clone();
            edit.components::<RowComponent>(
                owner,
                host,
                rows.into_iter()
                    .map(|(key, width, color)| (key, row(width, color, &m, &c, &h))),
            )?;
            Ok(())
        });
        let register = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], parent)?;
            Ok(())
        };
        if local {
            ui.edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        } else {
            ui.edit_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        }
        let scopes = ui.children(owner).unwrap().to_vec();
        let (prop, state, node) = handles.borrow()[0];
        let mut old = None;
        for (index, rows) in [
            initial,
            vec![(2, 40, 0x0000ffff), (1, 25, 0xffff00ff)],
            vec![(1, 15, 0xff00ffff), (3, 35, 0x00ffffff)],
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                ui.transaction(|tx| tx.set(model, rows.clone())).unwrap();
            }
            assert_eq!(mounts.get(), if index < 2 { 2 } else { 3 });
            assert_eq!(cleanups.get(), usize::from(index == 2));
            let scope = ui.children(owner).unwrap()[if index == 1 { 1 } else { 0 }];
            assert_eq!(scope, scopes[0]);
            assert_eq!(ui.renders(scope).unwrap(), [node]);
            assert_eq!(*ui.read(state).unwrap(), 10);
            assert_eq!(
                ui.read_prop(prop).unwrap().width,
                rows.iter().find(|r| r.0 == 1).unwrap().1
            );
            let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            for &(_, width, color) in &rows {
                full.append_layout(full.root(), fixed(width as f32, 10.0), color)
                    .unwrap();
            }
            let reference = full.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            let frame = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
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
        let retired = handles.borrow()[1].0;
        assert!(ui.read_prop(retired).is_err());
        drop(ui);
        assert_eq!(cleanups.get(), 3);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[derive(Clone, PartialEq)]
struct EmptyProps {
    mounts: Rc<Cell<usize>>,
    cleanups: Rc<Cell<usize>>,
}
struct Empty;
impl Component for Empty {
    type Props = EmptyProps;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let p = cx.read_prop(props)?.clone();
        cx.state(0, || 7u32)?;
        cx.effect(0, &[], move |_| {
            p.mounts.set(p.mounts.get() + 1);
            let cleanup = p.cleanups.clone();
            Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
        })?;
        Ok(())
    }
}
struct Other;
impl Component for Other {
    type Props = EmptyProps;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        Empty::mount(props, cx)
    }
}
#[test]
fn typed_components_zero_output_mounts_once_and_type_or_host_change_remounts() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let other_host = ui
        .append_render_layout(owner, host, LayoutStyle::column(), 0)
        .unwrap();
    let mounts = Rc::new(Cell::new(0));
    let cleanups = Rc::new(Cell::new(0));
    let p = EmptyProps {
        mounts: mounts.clone(),
        cleanups: cleanups.clone(),
    };
    let first = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.components::<Empty>(owner, host, [(1, p.clone())]),
        )
        .unwrap()
        .result
        .value[0];
    assert!(ui.renders(first).unwrap().is_empty());
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<Empty>(owner, host, [(1, p.clone())]),
    )
    .unwrap();
    assert_eq!(mounts.get(), 1);
    let changed = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.components::<Other>(owner, host, [(1, p.clone())]),
        )
        .unwrap()
        .result
        .value[0];
    assert_ne!(first, changed);
    assert_eq!((mounts.get(), cleanups.get()), (2, 1));
    let moved = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.components::<Other>(owner, other_host, [(1, p.clone())]),
        )
        .unwrap()
        .result
        .value[0];
    assert_ne!(changed, moved);
    assert_eq!((mounts.get(), cleanups.get()), (3, 2));
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<Other>(owner, other_host, []),
    )
    .unwrap();
    assert_eq!(cleanups.get(), 3);
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<Other>(owner, other_host, [(1, p)]),
    )
    .unwrap();
    assert_eq!(mounts.get(), 4);
}

struct Multi;
impl Component for Multi {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let a = cx.append_layout(fixed(20.0, 10.0), 0)?;
        let b = cx.append_layout(fixed(10.0, 5.0), 0)?;
        cx.bind_prop(props, Property::color(a))?;
        cx.bind_prop(props, Property::color(b))?;
        Ok(())
    }
}
struct Nested;
impl Component for Nested {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let scope = cx.scope();
        cx.layout_producer(
            0,
            host,
            LayoutBuildKind::Content,
            &[],
            LayoutProducer::new(move |_, edit| {
                let color = *edit.read_prop(props)?;
                edit.components::<Multi>(scope, host, [(1, color)])?;
                Ok(())
            }),
        )?;
        Ok(())
    }
}
#[test]
fn typed_components_nested_producers_and_multiple_outputs_use_ordinary_lifetimes() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let source = ui.state(owner, 0, || 0xff0000ffu32).unwrap();
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
                    let color = *edit.read(source)?;
                    edit.components::<Nested>(owner, host, [(9, color)])?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let outer = ui.children(owner).unwrap()[0];
    let inner = ui.children(outer).unwrap()[0];
    let nodes = ui.renders(inner).unwrap().to_vec();
    assert_eq!(nodes.len(), 2);
    ui.transaction(|tx| tx.set(source, 0x00ff00ff)).unwrap();
    assert_eq!(ui.children(outer).unwrap(), [inner]);
    assert_eq!(ui.renders(inner).unwrap(), nodes);
    for node in nodes {
        assert_eq!(ui.reactive().tree().node(node).unwrap().color, 0x00ff00ff);
    }
    assert_eq!(ui.layout_result(host).unwrap().size, [20.0, 15.0]);
}

struct Fail;
impl Component for Fail {
    type Props = bool;
    fn mount(
        props: ReadProp<bool>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        cx.append_layout(fixed(10.0, 10.0), 0xff0000ff)?;
        if *cx.read_prop(props)? {
            panic!("expected component mount panic")
        }
        Err(Error::InvalidStyle)
    }
}
#[test]
fn typed_components_caught_mount_error_or_unwind_poison_outer_edit() {
    for panic in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let source = ui.state(owner, 0, || 0u32).unwrap();
        let before = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
        let stats = ui.stats();
        let result = ui.edit_frame(
            constraints(),
            [180.0; 2],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                if panic {
                    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        edit.components::<Fail>(owner, host, [(1, true)])
                    }));
                    assert!(caught.is_err());
                } else {
                    assert!(matches!(
                        edit.components::<Fail>(owner, host, [(1, false)]),
                        Err(Error::InvalidStyle)
                    ));
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(Error::InvalidComponent)));
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert!(ui.children(owner).unwrap().is_empty());
        assert_eq!(ui.stats(), stats);
        assert_eq!(
            pixels(before),
            pixels(ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap())
        );
    }
}

#[test]
fn typed_components_duplicate_keys_validate_before_mount_and_final_failure_keeps_old_instance() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let mounts = Rc::new(Cell::new(0));
    let cleanup = Rc::new(Cell::new(0));
    let p = EmptyProps {
        mounts: mounts.clone(),
        cleanups: cleanup.clone(),
    };
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.components::<Empty>(owner, host, [(1, p.clone()), (1, p.clone())])
        ),
        Err(Error::DuplicateKey)
    ));
    assert_eq!(mounts.get(), 0);
    let old = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| edit.components::<Empty>(owner, host, [(1, p.clone())]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let before = ui.published_local_frame().unwrap().clone();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.components::<Other>(owner, host, [(1, p.clone())])?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(ui.children(owner).unwrap(), [old]);
    assert_eq!((mounts.get(), cleanup.get()), (1, 0));
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn typed_components_replace_nonempty_type_and_clear_descendant_producers() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let first = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| edit.components::<Nested>(owner, host, [(1, 0xff0000ffu32)]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let inner = ui.children(first).unwrap()[0];
    let old_nodes = ui.renders(inner).unwrap().to_vec();
    let old = ui.published_local_frame().unwrap().clone();
    assert_eq!(ui.layout_producers.len(), 1);
    let next = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.components::<Multi>(owner, host, [(1, 0x00ff00ffu32)]),
        )
        .unwrap()
        .result
        .value[0];
    assert_ne!(first, next);
    assert!(ui.children(first).is_err());
    assert!(ui.renders(inner).is_err());
    assert!(ui.layout_producers.is_empty());
    for node in old_nodes {
        assert!(old.resolve_geometry(node).is_some());
        assert!(
            ui.published_local_frame()
                .unwrap()
                .resolve_geometry(node)
                .is_none()
        );
    }
    assert_eq!(ui.renders(next).unwrap().len(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size, [20.0, 15.0]);
}

struct StaticNested;
impl Component for StaticNested {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        assert!(matches!(
            cx.state(u64::MAX, || 0u32),
            Err(Error::InvalidComponent)
        ));
        let color = *cx.read_prop(props)?;
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.components::<Multi>(host, [(1, color)])?;
        Ok(())
    }
}
#[test]
fn typed_components_context_nested_mount_and_invalid_scope_or_host_preflight() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let scope = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| edit.components::<StaticNested>(owner, host, [(1, 0xff0000ff)]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let child = ui.children(scope).unwrap()[0];
    assert_eq!(ui.renders(child).unwrap().len(), 2);
    let foreign = fixture();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.components::<Multi>(foreign.root_scope(), host, [(1, 0u32)])
        ),
        Err(Error::InvalidScope)
    ));
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.components::<Multi>(owner, foreign.root_render(), [(1, 0u32)])
        ),
        Err(Error::InvalidNode)
    ));
    let removed = ui
        .append_render_layout(owner, host, LayoutStyle::column(), 0)
        .unwrap();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.remove(removed)?;
                edit.components::<Multi>(owner, removed, [(1, 0u32)])
            }
        ),
        Err(Error::InvalidNode)
    ));
    assert_eq!(ui.children(owner).unwrap(), [scope]);
}
