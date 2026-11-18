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
struct Props {
    width: u32,
    color: u32,
    mounts: Rc<Cell<usize>>,
    cleanup: Rc<Cell<usize>>,
    handles: Rc<RefCell<Vec<(ReadProp<Props>, Signal<u32>, NodeId)>>>,
}
impl PartialEq for Props {
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
    type Props = Props;
    fn mount(
        props: ReadProp<Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let p = cx.read_prop(props)?.clone();
        let state = cx.state(0, || 10u32)?;
        let style = cx.computed(1, &[props.dependency(), state.dependency()], move |i| {
            Ok(fixed(i.read_prop(props)?.width, *i.read(state)?))
        })?;
        let color = cx.computed(2, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(color, Property::color(node))?;
        p.handles.borrow_mut().push((props, state, node));
        cx.effect(0, &[], move |_| {
            p.mounts.set(p.mounts.get() + 1);
            let cleanup = p.cleanup.clone();
            Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
        })?;
        Ok(())
    }
}
struct Other;
impl Component for Other {
    type Props = Props;
    fn mount(
        props: ReadProp<Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        Item::mount(props, cx)
    }
}
#[test]
fn component_plans_automatic_branch_match_and_keyed_payloads_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_component_plans_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("component plan adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let mode = ui.state(owner, 0, || 0u32).unwrap();
        let mounts = Rc::new(Cell::new(0));
        let cleanup = Rc::new(Cell::new(0));
        let handles = Rc::new(RefCell::new(vec![]));
        let (m, c, h) = (mounts.clone(), cleanup.clone(), handles.clone());
        let mut sites = vec![];
        let register = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
            sites = edit.component_sites(owner, &[10, 20, 30])?;
            let regions = sites.clone();
            edit.component_producer(
                owner,
                0,
                host,
                &[],
                ComponentProducer::new(move |i| {
                    let phase = *i.read(mode)?;
                    let value = |width, color| Props {
                        width,
                        color,
                        mounts: m.clone(),
                        cleanup: c.clone(),
                        handles: h.clone(),
                    };
                    let mut plan = ComponentPlan::new();
                    plan.branch::<Item, Other>(
                        regions[0],
                        host,
                        phase < 2,
                        || Ok(value(if phase == 0 { 20 } else { 25 }, 0xff0000ff)),
                        || Ok(value(30, 0x00ff00ff)),
                    )?;
                    match phase {
                        0 | 1 => plan.select::<Item>(
                            regions[1],
                            host,
                            7,
                            value(40 + phase * 5, 0x0000ffff),
                        ),
                        2 => plan.select::<Item>(regions[1], host, 8, value(50, 0xff00ffff)),
                        _ => plan.empty(regions[1], host),
                    }
                    let rows = if phase == 0 {
                        vec![(1, value(15, 0x00ffffff)), (2, value(35, 0xffff00ff))]
                    } else {
                        vec![(2, value(35, 0xffff00ff)), (1, value(18, 0x00ffffff))]
                    };
                    plan.list::<Item>(regions[2], host, rows);
                    Ok(plan)
                }),
            )?;
            Ok(())
        };
        if local {
            ui.edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        } else {
            ui.edit_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        }
        let list_scope = ui.children(sites[2].scope()).unwrap()[0];
        let first_branch = ui.children(sites[0].scope()).unwrap()[0];
        let list_handles = handles.borrow()[2].clone();
        let mut old = None;
        for phase in 0..4 {
            if phase > 0 {
                ui.transaction(|tx| tx.set(mode, phase)).unwrap();
            }
            assert_eq!(mounts.get(), [4, 4, 6, 6][phase as usize]);
            assert_eq!(cleanup.get(), [0, 0, 2, 3][phase as usize]);
            assert_eq!(
                ui.children(sites[2].scope()).unwrap()[usize::from(phase > 0)],
                list_scope
            );
            assert_eq!(ui.renders(list_scope).unwrap(), [list_handles.2]);
            assert_eq!(*ui.read(list_handles.1).unwrap(), 10);
            assert_eq!(
                ui.read_prop(list_handles.0).unwrap().width,
                if phase == 0 { 15 } else { 18 }
            );
            if phase < 2 {
                assert_eq!(ui.children(sites[0].scope()).unwrap(), [first_branch]);
            } else {
                assert!(ui.children(first_branch).is_err());
            }
            let mut expected = vec![(
                if phase == 0 {
                    20
                } else if phase == 1 {
                    25
                } else {
                    30
                },
                if phase < 2 { 0xff0000ff } else { 0x00ff00ff },
            )];
            if phase < 3 {
                expected.push((
                    40 + phase * 5,
                    if phase < 2 { 0x0000ffff } else { 0xff00ffff },
                ));
            }
            expected.extend(if phase == 0 {
                vec![(15, 0x00ffffff), (35, 0xffff00ff)]
            } else {
                vec![(35, 0xffff00ff), (18, 0x00ffffff)]
            });
            let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            for (width, color) in expected {
                full.append_layout(full.root(), fixed(width, 10), color)
                    .unwrap();
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

struct Lower;
impl Component for Lower {
    type Props = u32;
    fn mount(_: ReadProp<u32>, _: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        Ok(())
    }
}
struct Upper;
impl Component for Upper {
    type Props = u64;
    fn mount(_: ReadProp<u64>, _: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        Ok(())
    }
}
struct ObserveFinal;
impl Component for ObserveFinal {
    type Props = (ReadProp<u32>, ReadProp<u64>);
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let (a, b) = *cx.read_prop(props)?;
        assert_eq!((*cx.read_prop(a)?, *cx.read_prop(b)?), (30, 50));
        Ok(())
    }
}
#[test]
fn component_plans_heterogeneous_sites_settle_related_inputs_once_before_mount() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let sites = ui
        .edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[1, 2]))
        .unwrap()
        .result
        .value;
    let mut initial = ComponentPlan::new();
    initial.select::<Lower>(sites[0], host, 0, 10u32);
    initial.select::<Upper>(sites[1], host, 0, 20u64);
    ui.edit(|_| Ok(()), |(), edit| edit.apply_components(initial))
        .unwrap();
    let a = ui.children(sites[0].scope()).unwrap()[0];
    let b = ui.children(sites[1].scope()).unwrap()[0];
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    let log = Rc::new(RefCell::new(vec![]));
    let l = log.clone();
    let (lower, upper, width) = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let lower = edit.prop(a, u64::MAX, 10u32)?;
                let upper = edit.prop(b, u64::MAX, 20u64)?;
                let width = edit.computed(
                    owner,
                    0,
                    &[lower.dependency(), upper.dependency()],
                    move |i| {
                        let (a, b) = (u64::from(*i.read_prop(lower)?), *i.read_prop(upper)?);
                        if a > b {
                            return Err(Error::InvalidStyle);
                        }
                        c.set(c.get() + 1);
                        Ok((b - a) as u32)
                    },
                )?;
                edit.effect(owner, 0, &[width.dependency()], move |i| {
                    l.borrow_mut().push(*i.read(width).unwrap());
                    None
                })?;
                Ok((lower, upper, width))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.components_at::<Lower>(sites[0], host, [(0, 30u32)])
        ),
        Err(Error::InvalidStyle)
    ));
    let mut plan = ComponentPlan::new();
    let observer = ui
        .edit(
            |_| Ok(()),
            |(), edit| edit.component_sites(owner, &[1, 2, 3]),
        )
        .unwrap()
        .result
        .value[2];
    // Declare the new mount first: it must still see both later final inputs.
    plan.select::<ObserveFinal>(observer, host, 0, (lower, upper));
    plan.select::<Lower>(sites[0], host, 0, 30u32);
    plan.select::<Upper>(sites[1], host, 0, 50u64);
    ui.edit(|_| Ok(()), |(), edit| edit.apply_components(plan))
        .unwrap();
    assert_eq!(
        (
            *ui.read_prop(lower).unwrap(),
            *ui.read_prop(upper).unwrap(),
            *ui.read(width).unwrap()
        ),
        (30, 50, 20)
    );
    assert_eq!(count.get(), 2);
    assert_eq!(*log.borrow(), [10, 20]);
}

struct ContextParent;
impl Component for ContextParent {
    type Props = u32;
    fn mount(props: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let sites = cx.component_sites(&[1, 2])?;
        let mut initial = ComponentPlan::new();
        initial.select::<Lower>(sites[0], host, 0, 10u32);
        cx.apply_components(initial)?;
        cx.component_producer(
            0,
            host,
            &[],
            ComponentProducer::new(move |i| {
                let mut plan = ComponentPlan::new();
                plan.select::<Upper>(sites[1], host, 0, u64::from(*i.read_prop(props)?));
                Ok(plan)
            }),
        )?;
        Ok(())
    }
}
#[test]
fn component_plans_context_registration_retires_with_selected_parent() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let site = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(owner, &[1])?[0];
                let mut plan = ComponentPlan::new();
                plan.select::<ContextParent>(site, host, 0, 10u32);
                edit.apply_components(plan)?;
                Ok(site)
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let parent = ui.children(site.scope()).unwrap()[0];
    assert_eq!(ui.layout_producers.len(), 1);
    let children = ui.children(parent).unwrap();
    assert_eq!(children.len(), 2);
    for &child in children {
        assert_eq!(ui.children(child).unwrap().len(), 1);
    }
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let mut plan = ComponentPlan::new();
            plan.empty(site, host);
            edit.apply_components(plan)
        },
    )
    .unwrap();
    assert!(ui.layout_producers.is_empty());
    assert!(ui.children(parent).is_err());
    assert!(ui.children(site.scope()).unwrap().is_empty());
}

#[test]
fn component_plans_selector_tracks_conditional_reads_and_final_values_only() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let choose = ui.state(owner, 0, || false).unwrap();
    let a = ui.state(owner, 1, || 10u32).unwrap();
    let b = ui.state(owner, 2, || 20u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let site = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(owner, &[0])?[0];
                edit.component_producer(
                    owner,
                    0,
                    host,
                    &[],
                    ComponentProducer::new(move |i| {
                        counter.set(counter.get() + 1);
                        let mut plan = ComponentPlan::new();
                        let selected = *i.read(choose)?;
                        plan.branch::<Lower, Upper>(
                            site,
                            host,
                            selected,
                            || Ok(*i.read(a)?),
                            || Ok(u64::from(*i.read(b)?)),
                        )?;
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
    let original = ui.children(site.scope()).unwrap()[0];
    ui.transaction(|tx| tx.set(a, 12)).unwrap();
    assert_eq!(calls.get(), 1);
    ui.transaction(|tx| {
        tx.set(choose, true)?;
        tx.set(choose, false)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(ui.children(site.scope()).unwrap(), [original]);
    ui.transaction(|tx| tx.set(choose, true)).unwrap();
    assert_eq!(calls.get(), 2);
    ui.transaction(|tx| tx.set(b, 22)).unwrap();
    assert_eq!(calls.get(), 2);
    ui.transaction(|tx| tx.set(a, 15)).unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn component_plans_no_else_skips_inactive_factory_and_failed_factory_keeps_old_state() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let shown = ui.state(owner, 0, || false).unwrap();
    let input = ui.state(owner, 1, || 10u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let site = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(owner, &[1])?[0];
                edit.component_producer(
                    owner,
                    0,
                    host,
                    &[],
                    ComponentProducer::new(move |i| {
                        let mut plan = ComponentPlan::new();
                        plan.show::<Lower>(site, host, *i.read(shown)?, || {
                            count.set(count.get() + 1);
                            let value = *i.read(input)?;
                            if value > 20 {
                                return Err(Error::InvalidStyle);
                            }
                            Ok(value)
                        })?;
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
    assert_eq!(calls.get(), 0);
    assert!(ui.children(site.scope()).unwrap().is_empty());
    ui.transaction(|tx| tx.set(shown, true)).unwrap();
    assert_eq!(calls.get(), 1);
    let first = ui.children(site.scope()).unwrap()[0];
    assert!(matches!(
        ui.transaction(|tx| tx.set(input, 30)),
        Err(Error::InvalidStyle)
    ));
    assert_eq!(*ui.read(input).unwrap(), 10);
    assert_eq!(ui.children(site.scope()).unwrap(), [first]);
    ui.transaction(|tx| tx.set(shown, false)).unwrap();
    assert_eq!(calls.get(), 2, "only the failed selected factory ran again");
    assert!(ui.children(site.scope()).unwrap().is_empty());
    ui.transaction(|tx| tx.set(input, 15)).unwrap();
    assert_eq!(calls.get(), 2);
    ui.transaction(|tx| tx.set(shown, true)).unwrap();
    assert_ne!(ui.children(site.scope()).unwrap()[0], first);
    assert_eq!(calls.get(), 3);
}

struct Fail;
impl Component for Fail {
    type Props = bool;
    fn mount(
        props: ReadProp<bool>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        cx.append_layout(fixed(5, 5), 0xff0000ff)?;
        if *cx.read_prop(props)? {
            panic!("expected planned mount panic");
        }
        Err(Error::InvalidStyle)
    }
}
#[test]
fn component_plans_mount_failure_or_panic_cannot_publish_prepared_siblings() {
    for panic in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let sites = ui
            .edit_local_frame(
                constraints(),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| edit.component_sites(owner, &[1, 2]),
            )
            .unwrap()
            .0
            .result
            .value;
        let before = ui.published_local_frame().unwrap().clone();
        assert!(matches!(
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    let mut plan = ComponentPlan::new();
                    plan.select::<Lower>(sites[0], host, 0, 30u32);
                    plan.select::<Fail>(sites[1], host, 0, panic);
                    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        edit.apply_components(plan)
                    }));
                    if panic {
                        assert!(failure.is_err());
                    } else {
                        assert!(matches!(failure.unwrap(), Err(Error::InvalidStyle)));
                    }
                    Ok(())
                }
            ),
            Err(Error::InvalidComponent)
        ));
        for site in &sites {
            assert!(ui.children(site.scope()).unwrap().is_empty());
        }
        assert!(before.same(ui.published_local_frame().unwrap()));
    }
}

#[test]
fn component_plans_duplicate_sites_and_nested_writers_reject_without_partial_adoption() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let (outer, inner) = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let outer = edit.component_sites(owner, &[0])?[0];
                let inner = edit.component_sites(outer.scope(), &[0])?[0];
                Ok((outer, inner))
            },
        )
        .unwrap()
        .result
        .value;
    for duplicate in [true, false] {
        let mut plan = ComponentPlan::new();
        plan.select::<Lower>(outer, host, 0, 10u32);
        plan.select::<Upper>(if duplicate { outer } else { inner }, host, 0, 20u64);
        let result = ui.edit(|_| Ok(()), |(), edit| edit.apply_components(plan));
        if duplicate {
            assert!(matches!(result, Err(Error::DuplicateKey)));
        } else {
            assert!(matches!(result, Err(Error::InvalidComponent)));
        }
        assert_eq!(ui.children(outer.scope()).unwrap(), [inner.scope()]);
        assert!(ui.children(inner.scope()).unwrap().is_empty());
    }
}

#[test]
fn component_plans_caught_payload_factory_error_rejects_the_whole_plan() {
    for (no_else, unwind) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let sites = ui
            .edit(|_| Ok(()), |(), edit| edit.component_sites(owner, &[1, 2]))
            .unwrap()
            .result
            .value;
        let mut plan = ComponentPlan::new();
        plan.select::<Lower>(sites[0], host, 0, 10u32);
        let selected = || {
            if unwind {
                panic!("expected planned factory panic");
            }
            Err(Error::InvalidStyle)
        };
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if no_else {
                plan.show::<Upper>(sites[1], host, true, selected)
            } else {
                plan.branch::<Upper, Lower>(sites[1], host, true, selected, || {
                    panic!("inactive factory executed")
                })
            }
        }));
        if unwind {
            assert!(failure.is_err());
        } else {
            assert!(matches!(failure.unwrap(), Err(Error::InvalidStyle)));
        }
        assert!(matches!(
            ui.edit(|_| Ok(()), |(), edit| edit.apply_components(plan)),
            Err(Error::InvalidComponent)
        ));
        for site in sites {
            assert!(ui.children(site.scope()).unwrap().is_empty());
        }
    }
}

#[test]
fn component_plans_final_validation_rolls_back_all_payloads_and_selector_failure_keeps_frame() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let input = ui.state(owner, 0, || 10u32).unwrap();
    let site = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(owner, &[0])?[0];
                edit.component_producer(
                    owner,
                    0,
                    host,
                    &[],
                    ComponentProducer::new(move |i| {
                        let value = *i.read(input)?;
                        if value == 30 {
                            return Err(Error::InvalidStyle);
                        }
                        let mut plan = ComponentPlan::new();
                        plan.select::<Lower>(site, host, 0, value);
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
    let child = ui.children(site.scope()).unwrap()[0];
    let payload = *ui.scope(child).unwrap().sources[&u64::MAX]
        .as_any()
        .downcast_ref::<ReadProp<u32>>()
        .unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    assert!(matches!(
        ui.transaction(|tx| tx.set(input, 30)),
        Err(Error::InvalidStyle)
    ));
    assert_eq!(*ui.read(input).unwrap(), 10);
    assert_eq!(*ui.read_prop(payload).unwrap(), 10);
    assert_eq!(ui.children(site.scope()).unwrap(), [child]);
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                let mut plan = ComponentPlan::new();
                plan.select::<Lower>(site, host, 0, 20u32);
                edit.apply_components(plan)?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert_eq!(*ui.read_prop(payload).unwrap(), 10);
}
