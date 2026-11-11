#![cfg(all(
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]

use gpui::TestAppContext;
use gpui_menus::gpui_adapter::{MenuContributionScope, MenusAppExt, run_command_request};
use gpui_menus::{
    CommandDescriptor, CommandId, ErrorCode, FakeMenuProvider, MenuCommandService, SystemServices,
    UiOwner,
};
use std::rc::Rc;

#[gpui::test]
fn app_global_system_services_and_scope_drop_are_coordinated(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let provider = Rc::new(FakeMenuProvider::new());
        let service = cx.init_menus(provider);
        let resolved = cx.menus_service().expect("global service");
        assert!(service.shares_state_with(&resolved));

        let mut services = SystemServices::new();
        service
            .clone()
            .install(&mut services)
            .expect("registry registration");
        gpui_system::gpui_adapter::install_system_services(services, cx);
        let from_registry = cx.menus_service().expect("registry service");
        assert!(service.shares_state_with(&from_registry));

        let owner = UiOwner::new(gpui_menus::SurfaceId(88));
        let mut scope =
            MenuContributionScope::new(owner.clone(), service.clone()).expect("contribution scope");
        let descriptor =
            CommandDescriptor::ui(CommandId::new("window.close").expect("id"), "Close Window")
                .expect("descriptor");
        let registration = service
            .register_ui_in(scope.scope(), descriptor, |_: (), _| Ok(()))
            .expect("registration");
        scope.retain_lease(registration.into_parts().1);
        assert!(service.is_registered(&CommandId::new("window.close").expect("id")));
        drop(scope);
        assert!(!service.is_registered(&CommandId::new("window.close").expect("id")));
        assert!(
            service
                .surface_generation(gpui_menus::SurfaceId(88))
                .is_none()
        );
        assert!(owner.context(None).is_err());

        let task = run_command_request(
            service.invoke_command(
                &CommandId::new("missing.command").expect("id"),
                gpui_menus::CommandPayload::Unit,
                gpui_menus::InvocationContext::default(),
            ),
            cx,
        );
        drop(task);
    });
}

#[gpui::test]
fn missing_global_is_a_typed_unavailable_error(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let error = cx.menus_service().expect_err("missing service");
        assert_eq!(error.code, ErrorCode::Unavailable);
        let _ = Rc::new(MenuCommandService::new(Rc::new(FakeMenuProvider::new())));
    });
}
