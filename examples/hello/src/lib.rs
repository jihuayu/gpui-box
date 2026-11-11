//! One Rust page shared by the desktop executable and Android native library.
#[cfg(target_os = "android")]
mod permissions;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_box_components::input;
use gpui_box_components::{
    query::{QueryClient, QueryRequest},
    reactive::{Binding, Memo, ReactiveScope, Signal, Store},
};

#[derive(Clone, PartialEq)]
struct Preferences {
    dark: bool,
    checked: bool,
    large_text: bool,
}

struct Hello {
    count: usize,
    input: Entity<input::TextInput>,
    secondary_input: Entity<input::TextInput>,
    gallery_open: bool,
    preferences: Store<Preferences>,
    committed: Signal<String>,
    summary: Memo<String>,
    scope: ReactiveScope,
    queries: QueryClient<&'static str, Vec<String>>,
    request: Option<QueryRequest>,
    render_count: u64,
    activation_count: usize,
    modal_open: bool,
    modal_focus: FocusHandle,
    modal_trigger: FocusHandle,
    #[cfg(target_os = "android")]
    permissions: Entity<permissions::PermissionPanel>,
}
impl Render for Hello {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_count += 1;
        if self.gallery_open {
            return self.render_gallery(window, cx).into_any_element();
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .tab_group()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .gap_4()
            .p_6()
            .bg(rgb(0x171d28))
            .text_color(rgb(0xe8edf5))
            .font_family("Noto Sans CJK SC")
            .when(self.count > 0, |element| {
                element.on_back_gesture(cx.listener(|this, event: &BackGestureEvent, _, cx| {
                    if event.phase == BackGesturePhase::Committed {
                        this.count = this.count.saturating_sub(1);
                        cx.notify();
                    }
                }))
            })
            .child(div().text_3xl().child("gpui-box"))
            .child("Desktop and Android · One Rust view")
            .child("你好，GPUI！中文渲染与输入")
            .child(
                div()
                    .id("counter")
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(0x245fc7))
                    .cursor_pointer()
                    .child(format!("Count: {} · Tap to increment", self.count))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        this.count += 1;
                        cx.notify();
                    })),
            )
            .child("Text input")
            .child(self.input.clone())
            .child("Second input · Check focus isolation")
            .child(self.secondary_input.clone())
            .child(gpui_box_components::button(
                "open-gallery",
                "Open component gallery",
                gpui_box_components::Theme::DARK,
                {
                    let entity = cx.entity().downgrade();
                    move |window, cx| {
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        let _ = entity.update(cx, |this, cx| {
                            this.gallery_open = true;
                            cx.notify();
                        });
                    }
                },
            ))
            .child(
                div().text_sm().child(
                    "Rotate to test surface replacement. Back decrements a positive counter.",
                ),
            )
            .into_any_element()
    }
}

impl Hello {
    fn render_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_box_components::{Layout, Status, Theme, button, checkbox, label, status, switch};
        let preferences = self.preferences.snapshot(cx);
        let mut theme = if preferences.dark {
            Theme::DARK
        } else {
            Theme::LIGHT
        };
        theme.text_scale = if preferences.large_text { 1.5 } else { 1.0 };
        if self.modal_open {
            return div()
                .id("modal-layer")
                .size_full()
                .p_6()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(theme.background))
                .font_family("Noto Sans CJK SC")
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.close_modal(window, cx);
                    }
                    if matches!(event.keystroke.key.as_str(), "escape" | "tab") {
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                }))
                .on_back_gesture(cx.listener(|this, event: &BackGestureEvent, window, cx| {
                    if event.phase == BackGesturePhase::Committed {
                        this.close_modal(window, cx);
                    }
                }))
                .child(
                    div()
                        .id("settings-dialog")
                        .role(accesskit::Role::Dialog)
                        .aria_label("Review settings")
                        .p_6()
                        .gap_4()
                        .flex()
                        .flex_col()
                        .bg(rgb(theme.surface))
                        .rounded_lg()
                        .child(label(
                            "review-message",
                            "Drafts and preferences remain in the screen model.",
                            theme,
                        ))
                        .child(
                            button("dismiss", "Close", theme, {
                                let entity = cx.entity().downgrade();
                                move |window, cx| {
                                    let _ =
                                        entity.update(cx, |this, cx| this.close_modal(window, cx));
                                }
                            })
                            .track_focus(&self.modal_focus),
                        ),
                )
                .into_any_element();
        }
        let wide = Layout::for_width(f32::from(window.viewport_size().width)) == Layout::Wide;
        let toggle = |field: fn(&mut Preferences) -> &mut bool| {
            let entity = cx.entity().downgrade();
            move |_: &mut Window, cx: &mut App| {
                let _ = entity.update(cx, |this, cx| {
                    this.preferences.command(
                        |state| {
                            let value = field(state);
                            *value = !*value;
                        },
                        cx,
                    );
                    let preferences = this.preferences.snapshot(cx);
                    let mut theme = if preferences.dark {
                        gpui_box_components::Theme::DARK
                    } else {
                        gpui_box_components::Theme::LIGHT
                    };
                    theme.text_scale = if preferences.large_text { 1.5 } else { 1.0 };
                    this.input
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                    this.secondary_input
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                });
            }
        };
        div()
            .id("component-gallery")
            .tab_group()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .on_back_gesture(cx.listener(|this, event: &BackGestureEvent, window, cx| {
                if event.phase == BackGesturePhase::Committed {
                    window.blur(cx);
                    window.dismiss_virtual_keyboard();
                    this.gallery_open = false;
                    cx.notify();
                }
            }))
            .size_full()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.text))
            .font_family("Noto Sans CJK SC")
            .child(label("gallery-title", "Component workbench", theme).text_3xl())
            .child(label(
                "activation-status",
                format!("Activation events {}", self.activation_count),
                theme,
            ))
            .child(label(
                "gallery-layout",
                if wide {
                    "Wide workspace · Controllers retained"
                } else {
                    "Compact workspace · Controllers retained"
                },
                theme,
            ))
            .child(button("gallery-home", "Back to platform checks", theme, {
                let entity = cx.entity().downgrade();
                move |window, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        this.gallery_open = false;
                        cx.notify();
                    });
                }
            }))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .when(!wide, |element| element.flex_col())
                    .child(switch(
                        "theme",
                        "Dark appearance",
                        preferences.dark,
                        theme,
                        toggle(|this| &mut this.dark),
                    ))
                    .child(checkbox(
                        "large-text",
                        "Large text",
                        preferences.large_text,
                        theme,
                        toggle(|this| &mut this.large_text),
                    ))
                    .child(checkbox(
                        "consent",
                        "Enable notifications",
                        preferences.checked,
                        theme,
                        toggle(|this| &mut this.checked),
                    )),
            )
            .child(
                button("show-dialog", "Review settings", theme, {
                    let entity = cx.entity().downgrade();
                    move |window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            this.modal_open = true;
                            window.dismiss_virtual_keyboard();
                            this.modal_focus.focus(window, cx);
                            cx.notify();
                        });
                    }
                })
                .track_focus(&self.modal_trigger),
            )
            .child(label(
                "form-label",
                "Drafts stay with their Rust controllers",
                theme,
            ))
            .child(self.input.clone())
            .child(self.secondary_input.clone())
            .child(button("commit-draft", "Commit primary draft", theme, {
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        let value = this.input.read(cx).value().to_owned();
                        Binding::signal(this.committed.clone()).set(value, cx);
                    });
                }
            }))
            .child(label("committed-summary", self.summary.get(cx), theme))
            .child(label(
                "reactive-metrics",
                format!(
                    "View renders {} · Scoped notifications {}",
                    self.render_count,
                    self.scope.notification_count()
                ),
                theme,
            ))
            .child(button("load-items", "Start item request", theme, {
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.request = Some(this.queries.start("items"));
                        cx.notify();
                    });
                }
            }))
            .child(button("finish-items", "Complete item request", theme, {
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        if let Some(request) = this.request.take() {
                            this.queries.complete(
                                &"items",
                                &request,
                                Ok(vec!["Saved item 你好".into(), "Responsive detail".into()]),
                            );
                            cx.notify();
                        }
                    });
                }
            }))
            .child(button("clear-account", "Clear account data", theme, {
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.queries.clear();
                        this.request = None;
                        cx.notify();
                    });
                }
            }))
            .child(button("fail-items", "Fail item refresh", theme, {
                let entity = cx.entity().downgrade();
                move |_, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        let request = this.queries.start("items");
                        this.queries.complete(
                            &"items",
                            &request,
                            Err(gpui_system::SystemError::new(
                                gpui_system::ErrorCode::Unavailable,
                                "The fake transport is offline",
                            )),
                        );
                        this.request = None;
                        cx.notify();
                    });
                }
            }))
            .child(
                status(
                    "query-status",
                    match self.queries.state(&"items") {
                        Some(state) if state.pending() => Status::Loading,
                        Some(state) if state.error.is_some() && state.value.is_some() => {
                            Status::Offline
                        }
                        Some(state) if state.error.is_some() => Status::Error,
                        Some(state) if state.value.is_some() => Status::Ready,
                        _ => Status::Empty,
                    },
                    theme,
                )
                .aria_label("Item query status"),
            )
            .children(
                self.queries
                    .state(&"items")
                    .and_then(|state| state.value.as_ref())
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, item)| label(("query-item", index), item.clone(), theme)),
            )
            .children(
                [
                    Status::Loading,
                    Status::Empty,
                    Status::Error,
                    Status::Offline,
                    Status::Ready,
                ]
                .into_iter()
                .enumerate()
                .map(|(index, state)| status(("status", index), state, theme)),
            )
            .child(
                div()
                    .id("sample-list")
                    .role(accesskit::Role::List)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(
                        [
                            "Inspect the native host",
                            "Edit a Chinese draft",
                            "Resize without losing state",
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, text)| {
                            label(("list-item", index), text, theme)
                                .role(accesskit::Role::ListItem)
                                .p_3()
                                .bg(rgb(theme.surface))
                        }),
                    ),
            )
            .children(self.permission_panel())
            .into_any_element()
    }

    fn permission_panel(&self) -> Option<AnyElement> {
        #[cfg(target_os = "android")]
        {
            Some(self.permissions.clone().into_any_element())
        }
        #[cfg(not(target_os = "android"))]
        {
            None
        }
    }

    fn close_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal_open = false;
        self.modal_trigger.focus(window, cx);
        cx.notify();
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled OFL font");
    input::bind_keys(cx);
    let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                let input = cx.new(input::TextInput::new);
                let secondary_input = cx.new(input::TextInput::new);
                input.update(cx, |input, cx| input.set_label("Primary draft", cx));
                secondary_input.update(cx, |input, cx| input.set_label("Secondary draft", cx));
                cx.new(|cx| {
                    let preferences = Store::new(
                        Preferences {
                            dark: true,
                            checked: false,
                            large_text: false,
                        },
                        cx,
                    );
                    let committed = Signal::new(String::new(), cx);
                    let summary = Memo::new(
                        &committed,
                        |value| format!("Committed primary draft · {value}"),
                        cx,
                    );
                    let mut scope = ReactiveScope::default();
                    scope.observe(&preferences.signal(), cx);
                    scope.observe(&summary.signal(), cx);
                    Hello {
                        count: 0,
                        input,
                        secondary_input,
                        gallery_open: false,
                        preferences,
                        committed,
                        summary,
                        scope,
                        queries: QueryClient::default(),
                        request: None,
                        render_count: 0,
                        activation_count: 0,
                        modal_open: false,
                        modal_focus: cx.focus_handle(),
                        modal_trigger: cx.focus_handle(),
                        #[cfg(target_os = "android")]
                        permissions: cx.new(permissions::PermissionPanel::new),
                    }
                })
            },
        )
        .expect("open shared example");
    #[cfg(target_os = "android")]
    {
        let mut save_app = cx.to_async();
        gpui_android::on_saved_state(move || {
            window
                .update(&mut save_app, |view, window, cx| view.snapshot(window, cx))
                .unwrap_or_default()
        });
        if let Some(bytes) = gpui_android::take_saved_state() {
            let _ = window.update(cx, |view, window, cx| view.restore(&bytes, window, cx));
        }
        let mut app = cx.to_async();
        gpui_android::on_activation(move |_, _| {
            window
                .update(&mut app, |view, _, cx| {
                    view.activation_count += 1;
                    view.gallery_open = true;
                    cx.notify();
                })
                .is_ok()
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = window;
    cx.activate(true);
}

#[cfg(target_os = "android")]
impl Hello {
    fn snapshot(&self, window: &Window, cx: &App) -> Vec<u8> {
        use gpui_lifecycle::*;
        let preferences = self.preferences.snapshot(cx);
        let route = RouteIdentity::new(if self.gallery_open {
            "gallery"
        } else {
            "platform"
        })
        .with_param("count", self.count.to_string())
        .with_param("dark", preferences.dark.to_string())
        .with_param("checked", preferences.checked.to_string())
        .with_param("large_text", preferences.large_text.to_string());
        let generation = gpui_android::lifecycle_service()
            .surface(SurfaceId(1))
            .map_or(0, |s| s.generation);
        let mut snapshot = VersionedSnapshot::new(SchemaVersion(1), generation, route);
        for (name, controller, committed) in [
            ("primary", &self.input, self.committed.get(cx)),
            ("secondary", &self.secondary_input, String::new()),
        ] {
            let input = controller.read(cx);
            let (anchor, focus) = input.selection();
            let text = input.value();
            let utf16 = |offset| text[..offset].encode_utf16().count() as u32;
            let draft = FieldDraft::new(name)
                .with_text(committed, text)
                .with_selection(
                    SelectionRange::new(utf16(anchor), utf16(focus)),
                    SelectionRange::new(anchor as u32, focus as u32),
                );
            snapshot.drafts.insert(name.into(), draft);
            if input.focus_handle(cx).is_focused(window) {
                snapshot.focus = Some(FocusTarget::new(name));
            }
        }
        snapshot.encode()
    }

    fn restore(&mut self, bytes: &[u8], window: &mut Window, cx: &mut Context<Self>) {
        use gpui_lifecycle::*;
        let Ok(snapshot) = VersionedSnapshot::decode(bytes) else {
            return;
        };
        if snapshot.schema != SchemaVersion(1) {
            return;
        }
        self.gallery_open = snapshot.route.name == "gallery";
        self.count = snapshot
            .route
            .param("count")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        self.preferences.command(
            |preferences| {
                preferences.dark = snapshot.route.param("dark") != Some("false");
                preferences.checked = snapshot.route.param("checked") == Some("true");
                preferences.large_text = snapshot.route.param("large_text") == Some("true");
            },
            cx,
        );
        let preferences = self.preferences.snapshot(cx);
        let mut theme = if preferences.dark {
            gpui_box_components::Theme::DARK
        } else {
            gpui_box_components::Theme::LIGHT
        };
        theme.text_scale = if preferences.large_text { 1.5 } else { 1.0 };
        for (name, controller) in [
            ("primary", self.input.clone()),
            ("secondary", self.secondary_input.clone()),
        ] {
            if let Some(draft) = snapshot.drafts.get(name) {
                controller.update(cx, |input, cx| {
                    input.set_theme(theme, cx);
                    input.restore_draft(
                        &draft.draft,
                        draft.selection_bytes.anchor as usize,
                        draft.selection_bytes.focus as usize,
                        window,
                        cx,
                    );
                    if snapshot.focus.as_ref().is_some_and(|f| f.as_str() == name) {
                        input.focus_handle(cx).focus(window, cx);
                    }
                });
                if name == "primary" {
                    self.committed.set(draft.committed.clone(), cx);
                }
            }
        }
        cx.notify();
    }
}
