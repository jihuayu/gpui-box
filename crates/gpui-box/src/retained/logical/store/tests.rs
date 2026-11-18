use super::*;
use crate::command::{Command, CommandStatus, command_channel};
use crate::store::StoreWriter;
use std::cell::Cell;

#[derive(PartialEq)]
struct Model {
    width: u32,
    color: u32,
    unrelated: u32,
}
struct SaveModel {
    expected: crate::store::StoreRevision,
    next: Model,
}
impl Command for SaveModel {
    type Output = ();
    type Error = crate::store::StoreError;
}
struct DocumentStore;
impl crate::store::StoreKey for DocumentStore {
    type State = Model;
}
struct ProviderRow;
impl Component for ProviderRow {
    type Props = (u32, u32);
    fn mount(
        _: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let input = cx.use_store::<DocumentStore>()?;
        let _command = cx.use_command::<SaveModel>()?;
        let style = cx.select_store(0, input, |s| Ok(fixed(s.width)))?;
        let color = cx.select_store(1, input, |s| Ok(s.color))?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind_selector(style, Property::layout_style(node))?;
        cx.bind_selector(color, Property::color(node))?;
        Ok(())
    }
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
fn fixed(w: u32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w as f32), LayoutDimension::Px(10.0))
        .flex_shrink(0.0)
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
struct Row;
impl Component for Row {
    type Props = (u32, u32);
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let style = cx.computed(0, &[props.dependency()], move |i| {
            Ok(fixed(i.read_prop(props)?.0))
        })?;
        let color = cx.computed(1, &[props.dependency()], move |i| Ok(i.read_prop(props)?.1))?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(color, Property::color(node))?;
        Ok(())
    }
}
#[test]
fn business_store_component_bridge_selects_one_revision_and_matches_complete_reference() {
    run(false, false, false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_business_store_bridge_match_complete_reference() {
    run(true, false, false);
}
#[test]
fn typed_commands_store_ui_adoption_matches_complete_reference() {
    run(false, true, false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_typed_commands_store_ui_match_complete_reference() {
    run(true, true, false);
}
#[test]
fn scoped_providers_store_command_ui_matches_complete_reference() {
    run(false, true, true);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_scoped_providers_match_complete_reference() {
    run(true, true, true);
}
fn run(hardware: bool, commands: bool, providers: bool) {
    let mut writer = StoreWriter::new(Model {
        width: 20,
        color: 0xff0000ff,
        unrelated: 0,
    })
    .unwrap();
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let mut input = ui.attach_store(owner, 0, &writer.read_only()).unwrap();
    let (sender, mut receiver) = command_channel::<SaveModel>(2).unwrap();
    if providers {
        ui.provide_store::<DocumentStore>(owner, input.prop())
            .unwrap();
        ui.provide_command(owner, sender.clone()).unwrap();
    }
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let row = ui
        .select_store(owner, 1, input.prop(), |s| Ok((s.width, s.color)))
        .unwrap();
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
                        count.set(count.get() + 1);
                        let mut plan = ComponentPlan::new();
                        if providers {
                            plan.select::<ProviderRow>(site, host, 0, *i.read_selector(row)?);
                        } else {
                            plan.select::<Row>(site, host, 0, *i.read_selector(row)?);
                        }
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
    let scope = ui.children(site.scope()).unwrap()[0];
    let node = ui.renders(scope).unwrap()[0];
    let sender = if providers {
        ui.edit(|_| Ok(()), |(), edit| edit.use_command::<SaveModel>(scope))
            .unwrap()
            .result
            .value
    } else {
        sender
    };
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("business store adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (width, color)) in [(20, 0xff0000ff), (35, 0x00ff00ff), (15, 0x0000ffff)]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            let before = writer.read_only().snapshot().unwrap().revision();
            writer
                .commit(
                    before,
                    Model {
                        width: 100,
                        color: 0xffff00ff,
                        unrelated: 1,
                    },
                )
                .unwrap();
            let before = writer.read_only().snapshot().unwrap().revision();
            let next = Model {
                width,
                color,
                unrelated: 2,
            };
            let revision = if commands {
                let mut ticket = sender
                    .send(SaveModel {
                        expected: before,
                        next,
                    })
                    .unwrap_or_else(|_| panic!("command rejected"));
                assert_eq!(ticket.status(), CommandStatus::Accepted);
                let request = receiver.try_recv().unwrap();
                let worker = std::thread::spawn(move || {
                    let mut request = request;
                    let SaveModel { expected, next } = request.take_command().unwrap();
                    match writer.commit(expected, next) {
                        Ok(commit) => request.succeed((), Some(commit.revision)),
                        Err(error) => request.fail(error),
                    }
                    writer
                });
                writer = worker.join().unwrap();
                let result = ticket.try_result().unwrap().unwrap();
                assert_eq!(ticket.status(), CommandStatus::Succeeded);
                result.committed_revision.unwrap()
            } else {
                writer.commit(before, next).unwrap().revision
            };
            assert_ne!(input.applied_revision(), Some(revision));
            let applied = ui.apply_store(&mut input).unwrap().unwrap();
            assert_eq!(applied.revision, revision);
            assert_eq!(input.applied_revision(), Some(revision));
        }
        assert_eq!(calls.get(), index + 1);
        assert_eq!(ui.children(site.scope()).unwrap(), [scope]);
        assert_eq!(ui.renders(scope).unwrap(), [node]);
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        full.append_layout(full.root(), fixed(width), color)
            .unwrap();
        let reference = full.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [180, 180], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
    }
    let before = ui.published_local_frame().unwrap().clone();
    writer
        .commit(
            writer.read_only().snapshot().unwrap().revision(),
            Model {
                width: 15,
                color: 0x0000ffff,
                unrelated: 99,
            },
        )
        .unwrap();
    ui.apply_store(&mut input).unwrap();
    assert_eq!(
        calls.get(),
        3,
        "equal selector result skips component selection"
    );
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert!(ui.apply_store(&mut input).unwrap().is_none());
    drop((ui, input, writer));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
#[test]
fn business_store_bridge_failed_ui_candidate_remains_pending_and_newer_revision_recovers() {
    let mut writer = StoreWriter::new((10u32, 20u32)).unwrap();
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let mut input = ui.attach_store(owner, 0, &writer.read_only()).unwrap();
    let a = ui
        .select_store(owner, 1, input.prop(), |s| Ok(s.0))
        .unwrap();
    let b = ui
        .select_store(owner, 2, input.prop(), |s| Ok(s.1))
        .unwrap();
    let (width, node) = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let width =
                    edit.computed(owner, 3, &[a.dependency(), b.dependency()], move |i| {
                        let (a, b) = (*i.read_selector(a)?, *i.read_selector(b)?);
                        b.checked_sub(a).ok_or(Error::InvalidStyle)
                    })?;
                let style = edit.computed(owner, 4, &[width.dependency()], move |i| {
                    Ok(fixed(*i.read(width)?))
                })?;
                let node = edit.append_layout(owner, host, fixed(10), 0xff0000ff)?;
                edit.bind(style, Property::layout_style(node))?;
                Ok((width, node))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let initial = input.applied_revision();
    let before = ui.published_local_frame().unwrap().clone();
    writer
        .commit(writer.read_only().snapshot().unwrap().revision(), (30, 20))
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            ui.apply_store(&mut input),
            Err(Error::InvalidStyle)
        ));
    }
    assert_eq!(input.applied_revision(), initial);
    assert_eq!(*ui.read(width).unwrap(), 10);
    assert!(before.same(ui.published_local_frame().unwrap()));
    writer
        .commit(writer.read_only().snapshot().unwrap().revision(), (30, 50))
        .unwrap();
    let applied = ui.apply_store(&mut input).unwrap().unwrap();
    assert!(applied.commit.commit.request_frame);
    // Without an active producer/responsive session, the ordinary host still
    // requests the frame explicitly after source adoption.
    ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
    assert_eq!(
        (
            *ui.read_selector(a).unwrap(),
            *ui.read_selector(b).unwrap(),
            *ui.read(width).unwrap()
        ),
        (30, 50, 20)
    );
    assert_eq!(ui.layout_result(node).unwrap().size, [20.0, 10.0]);
}
#[test]
fn business_store_bridge_scope_retirement_does_not_close_business_store_or_rebind_keys() {
    let mut writer = StoreWriter::new(1u32).unwrap();
    let mut ui = fixture();
    let root = ui.root_scope();
    let child = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let mut input = ui.attach_store(child, 0, &writer.read_only()).unwrap();
    assert!(matches!(
        ui.attach_store(child, 0, &writer.read_only()),
        Err(Error::InvalidStore)
    ));
    ui.reconcile_keys(root, &[]).unwrap();
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    assert!(matches!(
        std::future::Future::poll(std::pin::pin!(input.ready()).as_mut(), &mut cx),
        std::task::Poll::Ready(Ok(false))
    ));
    writer
        .commit(writer.read_only().snapshot().unwrap().revision(), 2)
        .unwrap();
    assert!(matches!(
        ui.apply_store(&mut input),
        Err(Error::InvalidSignal)
    ));
    assert_eq!(*writer.read_only().snapshot().unwrap().state(), 2);
}

#[test]
fn business_store_rejected_scope_retirement_keeps_subscription_and_snapshot_live() {
    let mut writer = StoreWriter::new(1u32).unwrap();
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let mut input = ui.attach_store(child, 0, &writer.read_only()).unwrap();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(owner, &[])?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    writer
        .commit(writer.read_only().snapshot().unwrap().revision(), 2)
        .unwrap();
    ui.apply_store(&mut input).unwrap();
    assert_eq!(*ui.read_prop(input.prop()).unwrap().state(), 2);
    assert_eq!(ui.children(owner).unwrap(), [child]);
}
