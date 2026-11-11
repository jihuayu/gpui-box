//! Contextual Android permission recovery using the installed shared service.
use gpui::*;
use gpui_box_components::{Theme, button, label};
use gpui_permissions::{PermissionKind, PermissionRequest, SettingsTarget};

pub struct PermissionPanel {
    status: String,
    camera_status: String,
    outcome: String,
    _observations: Task<()>,
}

impl PermissionPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let service = gpui_android::permission_service();
        let stream = service.subscribe();
        let observations = cx.spawn(async move |this, cx| {
            while let Ok(next) = stream.try_next() {
                if next.await.is_none() {
                    break;
                }
                if this
                    .update(cx, |this, cx| {
                        this.refresh(cx);
                        stream.acknowledge_snapshot(service.revision());
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut panel = Self {
            status: String::new(),
            camera_status: String::new(),
            outcome: "Request only when you enable notifications.".into(),
            _observations: observations,
        };
        panel.refresh(cx);
        panel
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = match gpui_android::permission_service()
            .provider()
            .query(PermissionKind::Notifications)
        {
            Ok(snapshot) => format!("Notification permission · {:?}", snapshot.status),
            Err(error) => error.to_string(),
        };
        self.camera_status = match gpui_android::permission_service()
            .provider()
            .query(PermissionKind::Camera)
        {
            Ok(snapshot) => format!("Camera permission · {:?}", snapshot.status),
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn request(&mut self, kind: PermissionKind, cx: &mut Context<Self>) {
        let context = match gpui_android::permission_context() {
            Ok(context) => context,
            Err(error) => {
                self.outcome = error.to_string();
                cx.notify();
                return;
            }
        };
        let request = gpui_android::permission_service()
            .request_permission(PermissionRequest::new(kind), context);
        let task = gpui_permissions::gpui_adapter::run_permission_request(request, cx);
        self.outcome = "Waiting for Android permission result…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.outcome = format!("Permission result · {result:?}");
                this.refresh(cx);
            });
        })
        .detach();
    }

    fn settings(&mut self, cx: &mut Context<Self>) {
        let context = match gpui_android::permission_context() {
            Ok(context) => context,
            Err(error) => {
                self.outcome = error.to_string();
                cx.notify();
                return;
            }
        };
        let request = gpui_android::permission_service()
            .open_settings(SettingsTarget::NotificationSettings, context);
        let task = gpui_permissions::gpui_adapter::run_permission_request(request, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.outcome = format!("Settings handoff · {result:?}");
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for PermissionPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                label("notification-permission", self.status.clone(), theme)
                    .aria_label("Notification permission status")
                    .role(accesskit::Role::Status)
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_live(accesskit::Live::Polite)
                    }),
            )
            .child(
                label("permission-result", self.outcome.clone(), theme)
                    .aria_label("Permission operation result")
                    .role(accesskit::Role::Status)
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_live(accesskit::Live::Polite)
                    }),
            )
            .child(button(
                "request-notifications",
                "Request notification permission",
                theme,
                {
                    let entity = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            this.request(PermissionKind::Notifications, cx)
                        });
                    }
                },
            ))
            .child(
                label("camera-permission", self.camera_status.clone(), theme)
                    .aria_label("Camera permission status")
                    .role(accesskit::Role::Status),
            )
            .child(button(
                "request-camera",
                "Request camera permission",
                theme,
                {
                    let entity = cx.entity().downgrade();
                    move |_, cx| {
                        let _ =
                            entity.update(cx, |this, cx| this.request(PermissionKind::Camera, cx));
                    }
                },
            ))
            .child(button(
                "notification-settings",
                "Open notification settings",
                theme,
                {
                    let entity = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = entity.update(cx, |this, cx| this.settings(cx));
                    }
                },
            ))
            .child(button(
                "refresh-permission",
                "Refresh permission status",
                theme,
                {
                    let entity = cx.entity().downgrade();
                    move |_, cx| {
                        let _ = entity.update(cx, |this, cx| this.refresh(cx));
                    }
                },
            ))
    }
}
