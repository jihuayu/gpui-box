use super::*;
use crate::{
    command::{CommandRejectReason, command_channel},
    store::StoreWriter,
};
use std::cell::RefCell;

struct Document;
impl StoreKey for Document {
    type State = u32;
}
struct OtherDocument;
impl StoreKey for OtherDocument {
    type State = u32;
}
#[derive(Debug)]
struct Save(u32);
impl Command for Save {
    type Output = ();
    type Error = ();
}
fn fixture() -> LogicalUi {
    LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap()
}
#[derive(Default)]
struct Observed {
    stores: Vec<ReadProp<StoreSnapshot<u32>>>,
    commands: Vec<CommandSender<Save>>,
}
struct Consumer;
impl Component for Consumer {
    type Props = Rc<RefCell<Observed>>;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let log = cx.read_prop(props)?.clone();
        let input = cx.use_store::<Document>()?;
        let command = cx.use_command::<Save>()?;
        let width = cx.select_store(0, input, |n| {
            Ok(LayoutStyle::column()
                .size(LayoutDimension::Px(*n as f32), LayoutDimension::Px(10.0)))
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0xff0000ff)?;
        cx.bind_selector(width, Property::layout_style(node))?;
        log.borrow_mut().stores.push(input);
        log.borrow_mut().commands.push(command);
        Ok(())
    }
}
impl PartialEq for Observed {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

#[test]
fn scoped_providers_nearest_override_and_explicit_keys_keep_store_identity() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let a = StoreWriter::new(20u32).unwrap();
    let b = StoreWriter::new(35u32).unwrap();
    let input_a = ui.attach_store(owner, 0, &a.read_only()).unwrap();
    let input_b = ui.attach_store(owner, 1, &b.read_only()).unwrap();
    let (send_a, mut recv_a) = command_channel::<Save>(4).unwrap();
    let (send_b, mut recv_b) = command_channel::<Save>(4).unwrap();
    let logs = Rc::new(RefCell::new(Observed::default()));
    let scopes = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.provide_store::<Document>(owner, input_a.prop())?;
                edit.provide_store::<OtherDocument>(owner, input_b.prop())?;
                edit.provide_command(owner, send_a.clone())?;
                let scopes = edit.reconcile_keys(owner, &[1, 2])?;
                edit.provide_store::<Document>(scopes[1], input_b.prop())?;
                edit.provide_command(scopes[1], send_b.clone())?;
                for &scope in &scopes {
                    edit.components::<Consumer>(scope, host, [(1, logs.clone())])?;
                }
                assert_eq!(edit.use_store::<OtherDocument>(scopes[0])?, input_b.prop());
                Ok(scopes)
            },
        )
        .unwrap()
        .result
        .value;
    assert_eq!(logs.borrow().stores, [input_a.prop(), input_b.prop()]);
    for (index, sender) in logs.borrow().commands.iter().enumerate() {
        sender.send(Save(index as u32)).unwrap();
    }
    assert_eq!(recv_a.try_recv().unwrap().take_command().unwrap().0, 0);
    assert_eq!(recv_b.try_recv().unwrap().take_command().unwrap().0, 1);
    ui.frame_in_viewport(LayoutConstraints::viewport(180.0, 180.0), [180.0; 2])
        .unwrap();
    for (scope, width) in scopes.iter().zip([20.0, 35.0]) {
        let consumer = ui.children(*scope).unwrap()[0];
        assert_eq!(
            ui.layout_result(ui.renders(consumer).unwrap()[0])
                .unwrap()
                .size,
            [width, 10.0]
        );
    }
}

#[test]
fn scoped_providers_same_handle_redeclaration_and_changed_identity_rejection() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let a = StoreWriter::new(1u32).unwrap();
    let b = StoreWriter::new(2u32).unwrap();
    let input_a = ui.attach_store(owner, 0, &a.read_only()).unwrap();
    let input_b = ui.attach_store(owner, 1, &b.read_only()).unwrap();
    let (send, _receiver) = command_channel::<Save>(1).unwrap();
    let (other, _other_receiver) = command_channel::<Save>(1).unwrap();
    ui.provide_store::<Document>(owner, input_a.prop()).unwrap();
    ui.provide_store::<Document>(owner, input_a.prop()).unwrap();
    ui.provide_command(owner, send.clone()).unwrap();
    ui.provide_command(owner, send).unwrap();
    assert!(matches!(
        ui.provide_store::<Document>(owner, input_b.prop()),
        Err(Error::InvalidProvider)
    ));
    assert!(matches!(
        ui.provide_command(owner, other),
        Err(Error::InvalidProvider)
    ));
    let selected = ui
        .edit(|_| Ok(()), |(), edit| edit.use_store::<Document>(owner))
        .unwrap()
        .result
        .value;
    assert_eq!(selected, input_a.prop());
}

#[test]
fn scoped_providers_missing_required_dependency_has_path_and_caught_error_poison() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let child = ui.reconcile_keys(owner, &[7]).unwrap().order[0];
    let result = ui.edit(|_| Ok(()), |(), edit| edit.use_store::<Document>(child));
    assert!(
        matches!(result,Err(Error::MissingProvider{path,dependency}) if path==[0,7] && dependency.ends_with("Document"))
    );
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                assert!(edit.use_command::<Save>(child).is_err());
                Ok(())
            }
        ),
        Err(Error::InvalidComponent)
    ));
    assert!(ui.providers.is_empty());
}

#[test]
fn scoped_providers_optional_absence_and_inherited_resolution_remain_fixed() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    assert!(
        ui.edit(|_| Ok(()), |(), edit| edit.try_use_store::<Document>(child))
            .unwrap()
            .result
            .value
            .is_none()
    );
    let writer = StoreWriter::new(1u32).unwrap();
    let input = ui.attach_store(owner, 0, &writer.read_only()).unwrap();
    ui.provide_store::<Document>(owner, input.prop()).unwrap();
    assert!(
        ui.edit(|_| Ok(()), |(), edit| edit.try_use_store::<Document>(child))
            .unwrap()
            .result
            .value
            .is_none()
    );
    let next = ui.reconcile_keys(owner, &[1, 2]).unwrap().order[1];
    assert_eq!(
        ui.edit(|_| Ok(()), |(), edit| edit.use_store::<Document>(next))
            .unwrap()
            .result
            .value,
        input.prop()
    );
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.provide_store::<Document>(next, input.prop())
        ),
        Err(Error::InvalidProvider)
    ));
}

#[test]
fn scoped_providers_final_frame_failure_rolls_back_registry_and_command_references() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let (sender, mut receiver) = command_channel::<Save>(1).unwrap();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.provide_command(owner, sender)?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert!(ui.providers.is_empty());
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    assert!(matches!(
        std::future::Future::poll(std::pin::pin!(receiver.ready()).as_mut(), &mut cx),
        std::task::Poll::Ready(false)
    ));
}

#[test]
fn scoped_providers_retirement_releases_capability_and_keeps_accepted_business_work() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let (sender, mut receiver) = command_channel::<Save>(2).unwrap();
    ui.provide_command(child, sender).unwrap();
    let sender = ui
        .edit(|_| Ok(()), |(), edit| edit.use_command::<Save>(child))
        .unwrap()
        .result
        .value;
    let mut ticket = sender.send(Save(42)).unwrap();
    drop(sender);
    ui.reconcile_keys(owner, &[]).unwrap();
    assert!(ui.providers.is_empty());
    let mut request = receiver.try_recv().unwrap();
    assert_eq!(request.take_command().unwrap().0, 42);
    request.succeed((), None);
    assert!(ticket.try_result().unwrap().is_ok());
    assert!(receiver.try_recv().is_none());
}

#[test]
fn scoped_providers_reject_surviving_alias_to_retired_store_source() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let writer = StoreWriter::new(1u32).unwrap();
    let input = ui.attach_store(child, 0, &writer.read_only()).unwrap();
    ui.provide_store::<Document>(owner, input.prop()).unwrap();
    assert!(matches!(
        ui.reconcile_keys(owner, &[]),
        Err(Error::InvalidProvider)
    ));
    assert_eq!(ui.children(owner).unwrap(), [child]);
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.reconcile_keys(owner, &[])),
        Err(Error::InvalidProvider)
    ));
    assert!(ui.read_prop(input.prop()).is_ok());
}

#[test]
fn scoped_providers_wrong_runtime_and_retired_scope_reject_before_publish() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let mut other = fixture();
    let root = other.root_scope();
    let writer = StoreWriter::new(1u32).unwrap();
    let foreign = other.attach_store(root, 0, &writer.read_only()).unwrap();
    assert!(matches!(
        ui.provide_store::<Document>(owner, foreign.prop()),
        Err(Error::InvalidSignal)
    ));
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    ui.reconcile_keys(owner, &[]).unwrap();
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.try_use_store::<Document>(child)),
        Err(Error::InvalidScope)
    ));
    let (sender, receiver) = command_channel::<Save>(1).unwrap();
    drop(receiver);
    ui.provide_command(owner, sender).unwrap();
    let sender = ui
        .edit(|_| Ok(()), |(), edit| edit.use_command::<Save>(owner))
        .unwrap()
        .result
        .value;
    assert!(matches!(
        sender.send(Save(1)),
        Err(crate::command::CommandRejection {
            reason: CommandRejectReason::Closed,
            ..
        })
    ));
}

struct ContextProvider;
impl Component for ContextProvider {
    type Props = (ReadProp<StoreSnapshot<u32>>, Rc<RefCell<Observed>>);
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let (input, log) = cx.read_prop(props)?.clone();
        assert!(cx.try_use_command::<Save>()?.is_some());
        cx.provide_store::<Document>(input)?;
        let command = cx.use_command::<Save>()?;
        // Providing a different key uses the same captured source, never a
        // new business subscription or current-state sampling during mount.
        cx.provide_store::<OtherDocument>(input)?;
        assert_eq!(cx.try_use_store::<OtherDocument>()?, Some(input));
        let host = cx.host();
        cx.components::<Consumer>(host, [(1, log)])?;
        drop(command);
        Ok(())
    }
}
#[test]
fn scoped_providers_context_override_is_inherited_by_nested_component() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let writer = StoreWriter::new(30u32).unwrap();
    let input = ui.attach_store(owner, 0, &writer.read_only()).unwrap();
    let (sender, _receiver) = command_channel::<Save>(1).unwrap();
    ui.provide_command(owner, sender).unwrap();
    let log = Rc::new(RefCell::new(Observed::default()));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.components::<ContextProvider>(owner, host, [(1, (input.prop(), log.clone()))])
        },
    )
    .unwrap();
    assert_eq!(log.borrow().stores, [input.prop()]);
    let parent = ui.children(owner).unwrap()[0];
    let child = ui.children(parent).unwrap()[0];
    assert!(ui.providers.contains_key(&parent));
    assert!(ui.providers.contains_key(&child));
}
