//! One Rust page shared by the desktop executable and Android native library.
#[cfg(target_os = "android")]
mod permissions;
pub mod workflows;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_box_components::input;
use gpui_box_components::{
    Layout, TextInput, Theme, VirtualListSpec, bottom_navigation, button, checkbox, label,
    master_detail,
    query::{QueryClient, QueryRequest},
    reactive::{Binding, Memo, ReactiveScope, Signal, Store},
    shell::{self, BottomSheetController, NavItem, keyboard_avoidance},
    status, switch, virtual_list,
};
use gpui_local_auth::VerificationPolicy;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GallerySection {
    Overview,
    AdaptiveShell,
    SystemWorkflows,
    LocalUnlock,
}
impl GallerySection {
    const ALL: [Self; 4] = [
        Self::Overview,
        Self::AdaptiveShell,
        Self::SystemWorkflows,
        Self::LocalUnlock,
    ];
    const fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::AdaptiveShell => 1,
            Self::SystemWorkflows => 2,
            Self::LocalUnlock => 3,
        }
    }
    const fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Overview,
            1 => Self::AdaptiveShell,
            2 => Self::SystemWorkflows,
            _ => Self::LocalUnlock,
        }
    }
    const fn nav_label(self) -> &'static str {
        match self {
            Self::Overview => "Section: Overview",
            Self::AdaptiveShell => "Section: Adaptive shell",
            Self::SystemWorkflows => "Section: System workflows",
            Self::LocalUnlock => "Section: Local unlock",
        }
    }
}

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
    gallery_section: GallerySection,
    workflows: workflows::GalleryWorkbench,
    shell_high_contrast: bool,
    shell_reduced_motion: bool,
    bottom_sheet: BottomSheetController,
    catalog_filter: Entity<TextInput>,
    catalog_list: VirtualListSpec,
    catalog_query: String,
    catalog_selected: Option<usize>,
    catalog_detail_open: bool,
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
    fn render_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let preferences = self.preferences.snapshot(cx);
        let theme = Self::preference_theme(&preferences);
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
        if self.bottom_sheet.is_open() {
            return self.render_sheet(&preferences, theme, cx);
        }
        let wide = Layout::for_width(f32::from(window.viewport_size().width)) == Layout::Wide;
        let mut gallery = div()
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
                    if this.bottom_sheet.is_open() {
                        this.dismiss_sheet(window, cx);
                    } else if this.gallery_section != GallerySection::Overview {
                        this.set_section(GallerySection::Overview, cx);
                    } else {
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        this.gallery_open = false;
                        cx.notify();
                    }
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
            .font_family("Noto Sans CJK SC");
        gallery = match self.gallery_section {
            GallerySection::Overview => self.render_overview(gallery, theme, wide, cx),
            GallerySection::AdaptiveShell => {
                gallery.child(self.render_adaptive_shell(window, &preferences, cx))
            }
            GallerySection::SystemWorkflows => {
                gallery.child(self.render_system_workflows(theme, cx))
            }
            GallerySection::LocalUnlock => gallery.child(self.render_local_unlock(theme, cx)),
        };
        gallery
            .child(self.render_section_navigation(theme, cx))
            .into_any_element()
    }

    fn preference_theme(preferences: &Preferences) -> Theme {
        let mut theme = if preferences.dark {
            Theme::DARK
        } else {
            Theme::LIGHT
        };
        theme.text_scale = if preferences.large_text { 1.5 } else { 1.0 };
        theme
    }

    fn section_theme(&self, preferences: &Preferences) -> Theme {
        let mut theme = match (preferences.dark, self.shell_high_contrast) {
            (true, true) => Theme::HIGH_CONTRAST_DARK,
            (true, false) => Theme::DARK,
            (false, true) => Theme::HIGH_CONTRAST_LIGHT,
            (false, false) => Theme::LIGHT,
        };
        theme.text_scale = if preferences.large_text { 1.5 } else { 1.0 };
        theme.with_reduced_motion(self.shell_reduced_motion)
    }

    fn render_overview(
        &mut self,
        gallery: Stateful<Div>,
        theme: Theme,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use gpui_box_components::Status;
        let preferences = self.preferences.snapshot(cx);
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
                    let theme = Self::preference_theme(&preferences);
                    this.input
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                    this.secondary_input
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                });
            }
        };
        gallery
            .child(
                label("gallery-title", "Component workbench", theme)
                    .text_3xl()
                    .debug_selector(|| "overview-marker".into()),
            )
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
    }

    fn render_section_navigation(&self, theme: Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let items = GallerySection::ALL.map(|section| {
            NavItem::new(section.nav_label(), section.nav_label()).with_icon(match section {
                GallerySection::Overview => "▦",
                GallerySection::AdaptiveShell => "◫",
                GallerySection::SystemWorkflows => "⇄",
                GallerySection::LocalUnlock => "◇",
            })
        });
        let entity = cx.entity().downgrade();
        bottom_navigation(
            "gallery-sections",
            &items,
            self.gallery_section.index(),
            theme,
            move |index, _, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.set_section(GallerySection::from_index(index), cx);
                });
            },
        )
        .aria_label("Gallery sections")
        .debug_selector(|| "section-navigation".into())
    }

    fn render_sheet(
        &mut self,
        preferences: &Preferences,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let section_theme = self.section_theme(preferences);
        let duration = self.bottom_sheet.motion_duration_ms(section_theme);
        let entity = cx.entity().downgrade();
        div()
            .id("adaptive-sheet-layer")
            .tab_group()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .justify_end()
            .bg(rgb(theme.background))
            .font_family("Noto Sans CJK SC")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.dismiss_sheet(window, cx);
                    cx.stop_propagation();
                    window.prevent_default();
                } else if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                    window.prevent_default();
                }
            }))
            .on_back_gesture(cx.listener(|this, event: &BackGestureEvent, window, cx| {
                if event.phase == BackGesturePhase::Committed {
                    this.dismiss_sheet(window, cx);
                }
            }))
            .child(
                self.bottom_sheet
                    .sheet(
                        "adaptive-sheet",
                        "Adaptive shell sheet",
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(label(
                                "adaptive-sheet-isolation",
                                "Background gallery controls are isolated while this sheet is open.",
                                section_theme,
                            ))
                            .child(label(
                                "adaptive-sheet-motion",
                                format!("Sheet motion · {duration} ms"),
                                section_theme,
                            )),
                        section_theme,
                        move |window, cx| {
                            let _ = entity.update(cx, |this, cx| this.dismiss_sheet(window, cx));
                        },
                    )
                    .debug_selector(|| "adaptive-sheet".into()),
            )
            .into_any_element()
    }

    fn render_adaptive_shell(
        &mut self,
        window: &mut Window,
        preferences: &Preferences,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let section_theme = self.section_theme(preferences);
        let wide = Layout::for_width(f32::from(window.viewport_size().width)) == Layout::Wide;

        let back_entity = cx.entity().downgrade();
        let back = button(
            "adaptive-back",
            "Back to overview",
            section_theme,
            move |_, cx| {
                let _ = back_entity.update(cx, |this, cx| {
                    this.set_section(GallerySection::Overview, cx);
                });
            },
        );
        let contrast_entity = cx.entity().downgrade();
        let contrast = switch(
            "adaptive-high-contrast",
            "High contrast",
            self.shell_high_contrast,
            section_theme,
            move |_, cx| {
                let _ = contrast_entity.update(cx, |this, cx| {
                    this.shell_high_contrast = !this.shell_high_contrast;
                    let preferences = this.preferences.snapshot(cx);
                    let theme = this.section_theme(&preferences);
                    this.catalog_filter
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                    cx.notify();
                });
            },
        );
        let motion_entity = cx.entity().downgrade();
        let motion = switch(
            "adaptive-reduced-motion",
            "Reduced motion",
            self.shell_reduced_motion,
            section_theme,
            move |_, cx| {
                let _ = motion_entity.update(cx, |this, cx| {
                    this.shell_reduced_motion = !this.shell_reduced_motion;
                    let preferences = this.preferences.snapshot(cx);
                    let theme = this.section_theme(&preferences);
                    this.catalog_filter
                        .update(cx, |input, cx| input.set_theme(theme, cx));
                    cx.notify();
                });
            },
        );
        let sheet_entity = cx.entity().downgrade();
        let sheet_button = button(
            "adaptive-open-sheet",
            "Open adaptive sheet",
            section_theme,
            move |window, cx| {
                let _ = sheet_entity.update(cx, |this, cx| this.open_sheet(window, cx));
            },
        )
        .track_focus(self.bottom_sheet.trigger_focus());
        let bar = shell::app_bar(
            "adaptive-app-bar",
            "Adaptive shell",
            Some(back.into_any_element()),
            [
                contrast.into_any_element(),
                motion.into_any_element(),
                sheet_button.into_any_element(),
            ],
            section_theme,
        )
        .debug_selector(|| "adaptive-shell-marker".into());

        let apply_entity = cx.entity().downgrade();
        let apply = button(
            "catalog-apply-filter",
            "Apply catalog filter",
            section_theme,
            move |window, cx| {
                let _ = apply_entity.update(cx, |this, cx| this.apply_catalog_filter(window, cx));
            },
        );
        let clear_entity = cx.entity().downgrade();
        let clear = button(
            "catalog-clear-filter",
            "Clear catalog filter",
            section_theme,
            move |window, cx| {
                let _ = clear_entity.update(cx, |this, cx| this.clear_catalog_filter(window, cx));
            },
        );
        let up_entity = cx.entity().downgrade();
        let up = button(
            "catalog-scroll-up",
            "Scroll catalog up",
            section_theme,
            move |_, cx| {
                let _ = up_entity.update(cx, |this, cx| this.scroll_catalog_by(-120.0, cx));
            },
        );
        let down_entity = cx.entity().downgrade();
        let down = button(
            "catalog-scroll-down",
            "Scroll catalog down",
            section_theme,
            move |_, cx| {
                let _ = down_entity.update(cx, |this, cx| this.scroll_catalog_by(120.0, cx));
            },
        );
        let end_entity = cx.entity().downgrade();
        let end = button(
            "catalog-jump-end",
            "Jump to catalog end",
            section_theme,
            move |_, cx| {
                let _ = end_entity.update(cx, |this, cx| {
                    let count = this.catalog_list.item_count;
                    this.catalog_list.scroll_to_index(count.saturating_sub(1));
                    cx.notify();
                });
            },
        );
        let top_entity = cx.entity().downgrade();
        let top = button(
            "catalog-jump-top",
            "Scroll catalog to top",
            section_theme,
            move |_, cx| {
                let _ = top_entity.update(cx, |this, cx| {
                    this.catalog_list.scroll_offset_px = 0.0;
                    cx.notify();
                });
            },
        );
        let detail_entity = cx.entity().downgrade();
        let open_detail = button(
            "catalog-open-detail",
            "Open catalog detail",
            section_theme,
            move |_, cx| {
                let _ = detail_entity.update(cx, |this, cx| {
                    this.catalog_detail_open = true;
                    cx.notify();
                });
            },
        );
        let close_detail_entity = cx.entity().downgrade();
        let close_detail = button(
            "catalog-close-detail",
            "Back to catalog list",
            section_theme,
            move |_, cx| {
                let _ = close_detail_entity.update(cx, |this, cx| {
                    this.catalog_detail_open = false;
                    cx.notify();
                });
            },
        );

        let filtered = self.filtered_catalog();
        let count = filtered.len();
        let mut spec = self.catalog_list.clone();
        spec.set_item_count(count);
        let window_spec = spec.window();
        let results = label(
            "catalog-results",
            format!(
                "Catalog results · {count} items · visible rows {}",
                window_spec.visible_range.len()
            ),
            section_theme,
        );
        let items = Rc::new(filtered);
        let selected_index = self.catalog_selected;
        let entity = cx.entity().downgrade();
        let row_theme = section_theme;
        let list = virtual_list("catalog-list", window_spec.clone(), count, move |index| {
            let original = items.get(index).copied().unwrap_or(index);
            let text = catalog_item_text(original);
            let selected = selected_index == Some(original);
            let click_entity = entity.clone();
            let a11y_entity = entity.clone();
            let mut row = div()
                .id(("catalog-row", original))
                .role(accesskit::Role::ListItem)
                .aria_label(text.clone())
                .aria_selected(selected)
                .focusable()
                .tab_index(0)
                .min_h(px(40.))
                .px_3()
                .py_2()
                .rounded_sm()
                .text_size(px(14. * row_theme.text_scale))
                .focus_visible(|style| style.border_2().border_color(rgb(row_theme.text)))
                .cursor_pointer()
                .child(text)
                .on_click(move |_, _, cx| {
                    let _ = click_entity.update(cx, |this, cx| {
                        this.catalog_selected = Some(original);
                        cx.notify();
                    });
                })
                .on_a11y_action(accesskit::Action::Click, move |_, _, cx| {
                    let _ = a11y_entity.update(cx, |this, cx| {
                        this.catalog_selected = Some(original);
                        cx.notify();
                    });
                });
            row = if selected {
                row.bg(rgb(row_theme.accent)).text_color(rgb(0xffffff))
            } else {
                row.bg(rgb(row_theme.surface))
                    .text_color(rgb(row_theme.text))
            };
            row.into_any_element()
        });
        let master = div()
            .debug_selector(|| "catalog-master".into())
            .flex()
            .flex_col()
            .gap_2()
            .child(results)
            .child(
                div()
                    .h(px(self.catalog_list.viewport_height_px))
                    .overflow_y_hidden()
                    .child(list),
            );
        let selected_text = self
            .catalog_selected
            .map(catalog_item_text)
            .unwrap_or_else(|| "No catalog item selected".to_owned());
        let detail = div()
            .debug_selector(|| "catalog-detail".into())
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(section_theme.surface))
            .rounded_lg()
            .border_2()
            .border_color(rgb(section_theme.border))
            .child(label(
                "catalog-detail-title",
                "Catalog detail",
                section_theme,
            ))
            .child(label(
                "catalog-detail-value",
                format!("Selected catalog item · {selected_text}"),
                section_theme,
            ));
        let detail = if wide || self.catalog_detail_open {
            Some(detail.into_any_element())
        } else {
            None
        };
        let empty = div().debug_selector(|| "catalog-empty".into()).child(label(
            "catalog-empty-message",
            "Select a catalog item to inspect its detail.",
            section_theme,
        ));
        let panel = master_detail(
            "catalog-master-detail",
            Layout::for_width(f32::from(window.viewport_size().width)),
            master,
            detail,
            empty,
            section_theme,
        )
        .debug_selector(|| "catalog-master-detail".into());

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(bar)
            .child(
                div()
                    .flex()
                    .gap_4()
                    .when(!wide, |element| element.flex_col())
                    .child(label(
                        "adaptive-theme-label",
                        "Shell appearance",
                        section_theme,
                    ))
                    .child(label(
                        "adaptive-motion-status",
                        format!(
                            "Adaptive motion · {} ms",
                            self.bottom_sheet.motion_duration_ms(section_theme)
                        ),
                        section_theme,
                    )),
            )
            .child(label(
                "catalog-filter-label",
                "Catalog filter",
                section_theme,
            ))
            .child(keyboard_avoidance(
                window,
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.catalog_filter.clone())
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(apply)
                            .child(clear)
                            .child(up)
                            .child(down)
                            .child(top)
                            .child(end)
                            .child(open_detail)
                            .child(close_detail),
                    ),
            ))
            .child(panel)
            .into_any_element()
    }

    fn render_system_workflows(&mut self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let back_entity = cx.entity().downgrade();
        let back = button("workflows-back", "Back to overview", theme, move |_, cx| {
            let _ = back_entity.update(cx, |this, cx| {
                this.set_section(GallerySection::Overview, cx);
            });
        });
        let bar = shell::app_bar(
            "workflows-app-bar",
            "System workflows",
            Some(back.into_any_element()),
            [],
            theme,
        )
        .debug_selector(|| "system-workflows-marker".into());

        let open_entity = cx.entity().downgrade();
        let open = button(
            "workflow-open-edit",
            "Open & edit document",
            theme,
            move |_, cx| {
                let _ = open_entity.update(cx, |this, cx| {
                    let _ = this.workflows.open_and_edit_document();
                    cx.notify();
                });
            },
        );
        let save_entity = cx.entity().downgrade();
        let save = button(
            "workflow-save-share",
            "Atomic save & leased share",
            theme,
            move |_, cx| {
                let _ = save_entity.update(cx, |this, cx| {
                    let _ = this.workflows.save_atomically();
                    let _ = this.workflows.stage_and_share();
                    cx.notify();
                });
            },
        );
        let toggle_entity = cx.entity().downgrade();
        let toggle = button(
            "workflow-toggle-share",
            "Toggle share unsupported mode",
            theme,
            move |_, cx| {
                let _ = toggle_entity.update(cx, |this, cx| {
                    this.workflows.toggle_share_unsupported();
                    cx.notify();
                });
            },
        );
        let copy_entity = cx.entity().downgrade();
        let copy = button(
            "workflow-copy-fallback",
            "Explicit copy export fallback",
            theme,
            move |_, cx| {
                let _ = copy_entity.update(cx, |this, cx| {
                    let _ = this.workflows.copy_export_fallback();
                    cx.notify();
                });
            },
        );

        let post_entity = cx.entity().downgrade();
        let post = button(
            "workflow-post-notification",
            "Export & post notification",
            theme,
            move |_, cx| {
                let _ = post_entity.update(cx, |this, cx| {
                    let _ = this.workflows.export_and_post_notification();
                    cx.notify();
                });
            },
        );
        let replace_entity = cx.entity().downgrade();
        let replace = button(
            "workflow-replace-notification",
            "Replace notification",
            theme,
            move |_, cx| {
                let _ = replace_entity.update(cx, |this, cx| {
                    let _ = this.workflows.replace_notification();
                    cx.notify();
                });
            },
        );
        let warm_entity = cx.entity().downgrade();
        let warm = button(
            "workflow-warm-activation",
            "Trigger warm activation",
            theme,
            move |_, cx| {
                let _ = warm_entity.update(cx, |this, cx| {
                    let _ = this.workflows.trigger_warm_activation();
                    cx.notify();
                });
            },
        );
        let cold_entity = cx.entity().downgrade();
        let cold = button(
            "workflow-cold-activation",
            "Simulate cold-launch activation",
            theme,
            move |_, cx| {
                let _ = cold_entity.update(cx, |this, cx| {
                    let _ = this.workflows.simulate_cold_launch_activation();
                    cx.notify();
                });
            },
        );
        let root_entity = cx.entity().downgrade();
        let reset = button(
            "workflow-reset-route",
            "Reset route to root",
            theme,
            move |_, cx| {
                let _ = root_entity.update(cx, |this, cx| {
                    this.workflows.reset_route_to_root();
                    cx.notify();
                });
            },
        );

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(bar)
            .child(label(
                "workflow-one-heading",
                "Workflow 1 · Open, edit, atomic save & leased share",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(open)
                    .child(save)
                    .child(toggle)
                    .child(copy),
            )
            .child(label(
                "workflow-document-status",
                self.workflows.document_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-save-status",
                self.workflows.save_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-share-status",
                self.workflows.share_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-fallback-status",
                self.workflows.fallback_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-two-heading",
                "Workflow 2 · Export, notification replace & activation routing",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(post)
                    .child(replace)
                    .child(warm)
                    .child(cold)
                    .child(reset),
            )
            .child(label(
                "workflow-notification-status",
                self.workflows.notification_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-activation-status",
                self.workflows.activation_status().to_owned(),
                theme,
            ))
            .child(label(
                "workflow-route-status",
                format!("Screen route · {:?}", self.workflows.current_route()),
                theme,
            ))
            .into_any_element()
    }

    fn render_local_unlock(&mut self, theme: Theme, cx: &mut Context<Self>) -> AnyElement {
        let back_entity = cx.entity().downgrade();
        let back = button("unlock-back", "Back to overview", theme, move |_, cx| {
            let _ = back_entity.update(cx, |this, cx| {
                this.set_section(GallerySection::Overview, cx);
            });
        });
        let bar = shell::app_bar(
            "unlock-app-bar",
            "Local unlock",
            Some(back.into_any_element()),
            [],
            theme,
        )
        .debug_selector(|| "local-unlock-marker".into());

        let probe_entity = cx.entity().downgrade();
        let probe = button(
            "unlock-probe",
            "Probe local auth readiness",
            theme,
            move |_, cx| {
                let _ = probe_entity.update(cx, |this, cx| {
                    let _ = this.workflows.probe_local_auth();
                    cx.notify();
                });
            },
        );
        let biometric_entity = cx.entity().downgrade();
        let biometric = button(
            "unlock-biometric-only",
            "Use biometric-only policy",
            theme,
            move |_, cx| {
                let _ = biometric_entity.update(cx, |this, cx| {
                    let _ = this
                        .workflows
                        .set_verification_policy(VerificationPolicy::BiometricOnly);
                    cx.notify();
                });
            },
        );
        let fallback_entity = cx.entity().downgrade();
        let fallback = button(
            "unlock-credential-fallback",
            "Use biometric or device credential",
            theme,
            move |_, cx| {
                let _ = fallback_entity.update(cx, |this, cx| {
                    let _ = this
                        .workflows
                        .set_verification_policy(VerificationPolicy::BiometricOrDeviceCredential);
                    cx.notify();
                });
            },
        );
        let verify_entity = cx.entity().downgrade();
        let verify = button(
            "unlock-verify",
            "Verify & unlock session",
            theme,
            move |_, cx| {
                let _ = verify_entity.update(cx, |this, cx| {
                    let _ = this.workflows.verify_and_unlock_session();
                    cx.notify();
                });
            },
        );
        let secret_entity = cx.entity().downgrade();
        let secret = button(
            "unlock-release-secret",
            "Release protected secret (redacted)",
            theme,
            move |_, cx| {
                let _ = secret_entity.update(cx, |this, cx| {
                    let _ = this.workflows.release_protected_secret();
                    cx.notify();
                });
            },
        );
        let background_entity = cx.entity().downgrade();
        let background = button(
            "unlock-background-relock",
            "Simulate background relock",
            theme,
            move |_, cx| {
                let _ = background_entity.update(cx, |this, cx| {
                    let _ = this.workflows.simulate_background_relock();
                    cx.notify();
                });
            },
        );
        let session_entity = cx.entity().downgrade();
        let session = button(
            "unlock-session-lock",
            "Simulate session lock",
            theme,
            move |_, cx| {
                let _ = session_entity.update(cx, |this, cx| {
                    let _ = this.workflows.simulate_session_lock();
                    cx.notify();
                });
            },
        );
        let cancelled_entity = cx.entity().downgrade();
        let cancelled = button(
            "unlock-simulate-cancel",
            "Simulate cancelled prompt",
            theme,
            move |_, cx| {
                let _ = cancelled_entity.update(cx, |this, cx| {
                    let _ = this.workflows.simulate_cancelled_verification();
                    cx.notify();
                });
            },
        );

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(bar)
            .child(label(
                "unlock-heading",
                "Workflow 3 · Local verification, relock & protected secret release",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(probe)
                    .child(biometric)
                    .child(fallback)
                    .child(verify)
                    .child(secret)
                    .child(background)
                    .child(session)
                    .child(cancelled),
            )
            .child(label(
                "unlock-local-auth-status",
                self.workflows.local_auth_status().to_owned(),
                theme,
            ))
            .child(label(
                "unlock-session-status",
                self.workflows.unlock_status().to_owned(),
                theme,
            ))
            .child(label(
                "unlock-secret-status",
                self.workflows.secret_status().to_owned(),
                theme,
            ))
            .child(label(
                "unlock-relock-status",
                self.workflows.relock_status().to_owned(),
                theme,
            ))
            .into_any_element()
    }

    fn filtered_catalog(&self) -> Vec<usize> {
        let query = self.catalog_query.to_lowercase();
        (0..500)
            .filter(|index| {
                query.is_empty() || catalog_item_text(*index).to_lowercase().contains(&query)
            })
            .collect()
    }

    fn sync_catalog_count(&mut self) {
        let filtered = self.filtered_catalog();
        self.catalog_list.set_item_count(filtered.len());
        if self
            .catalog_selected
            .is_some_and(|selected| !filtered.contains(&selected))
        {
            self.catalog_selected = filtered.first().copied();
        }
    }

    fn apply_catalog_filter(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.catalog_query = self.catalog_filter.read(cx).value().trim().to_lowercase();
        self.sync_catalog_count();
        cx.notify();
    }

    fn clear_catalog_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.catalog_query.clear();
        self.catalog_filter
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.sync_catalog_count();
        cx.notify();
    }

    fn scroll_catalog_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.catalog_list.scroll_offset_px = (self.catalog_list.scroll_offset_px + delta).max(0.0);
        self.catalog_list.scroll_offset_px = self.catalog_list.window().scroll_offset_px;
        cx.notify();
    }

    fn set_section(&mut self, section: GallerySection, cx: &mut Context<Self>) {
        if self.gallery_section == section {
            return;
        }
        self.gallery_section = section;
        self.catalog_detail_open = false;
        cx.notify();
    }

    fn open_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_sheet.present(window, cx);
        cx.notify();
    }

    fn dismiss_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_sheet.dismiss(window, cx);
        cx.notify();
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

impl Hello {
    fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(input::TextInput::new);
        let secondary_input = cx.new(input::TextInput::new);
        input.update(cx, |input, cx| input.set_label("Primary draft", cx));
        secondary_input.update(cx, |input, cx| input.set_label("Secondary draft", cx));
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
        let catalog_filter = cx.new(input::TextInput::new);
        catalog_filter.update(cx, |input, cx| {
            input.set_label("Catalog filter", cx);
            input.set_theme(Theme::DARK, cx);
        });
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
            gallery_section: GallerySection::Overview,
            workflows: workflows::GalleryWorkbench::new()
                .expect("create deterministic workflow workbench"),
            shell_high_contrast: false,
            shell_reduced_motion: false,
            bottom_sheet: BottomSheetController::new(cx),
            catalog_filter,
            catalog_list: VirtualListSpec {
                item_count: 500,
                row_height_px: 40.0,
                viewport_height_px: 320.0,
                scroll_offset_px: 0.0,
                overscan: 2,
            },
            catalog_query: String::new(),
            catalog_selected: Some(0),
            catalog_detail_open: false,
            #[cfg(target_os = "android")]
            permissions: cx.new(permissions::PermissionPanel::new),
        }
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
            |_, cx| cx.new(Hello::new),
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

fn catalog_item_text(index: usize) -> String {
    let category = match index % 5 {
        0 => "Documents",
        1 => "Notes",
        2 => "Signals",
        3 => "纪要 ✅",
        _ => "Archive",
    };
    format!("Catalog item {index:03} · {category}")
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
        let section_theme = self.section_theme(&preferences);
        self.catalog_filter
            .update(cx, |input, cx| input.set_theme(section_theme, cx));
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

#[cfg(test)]
mod gallery_tests {
    use super::{GallerySection, Hello};
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px, size};

    fn open_gallery(cx: &mut TestAppContext) -> (Entity<Hello>, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(|_, cx| Hello::new(cx));
        cx.simulate_resize(size(px(900.), px(2000.)));
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.gallery_open = true;
                cx.notify();
            });
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn select_section(view: &Entity<Hello>, cx: &mut VisualTestContext, section: GallerySection) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| view.set_section(section, cx));
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn gallery_defaults_to_overview_and_switches_sections_through_bottom_navigation(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = open_gallery(cx);
        assert_eq!(
            cx.read(|cx| view.read(cx).gallery_section),
            GallerySection::Overview
        );
        assert!(cx.debug_bounds("overview-marker").is_some());
        assert!(cx.debug_bounds("section-navigation").is_some());
        assert_eq!(
            cx.read(|cx| view.read(cx).input.read(cx).value().to_owned()),
            ""
        );

        let navigation = cx
            .debug_bounds("section-navigation")
            .expect("bottom navigation is rendered");
        let height = navigation.size.height;
        assert!(height > px(0.));
        let adaptive_position = point(
            navigation.left() + navigation.size.width * (1.5 / 4.0),
            navigation.center().y,
        );
        cx.simulate_click(adaptive_position, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).gallery_section),
            GallerySection::AdaptiveShell
        );
        assert!(cx.debug_bounds("adaptive-shell-marker").is_some());
        assert!(cx.debug_bounds("overview-marker").is_none());
        assert!(cx.debug_bounds("section-navigation").is_some());

        let navigation = cx
            .debug_bounds("section-navigation")
            .expect("bottom navigation remains rendered");
        let workflows_position = point(
            navigation.left() + navigation.size.width * (2.5 / 4.0),
            navigation.center().y,
        );
        cx.simulate_click(workflows_position, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).gallery_section),
            GallerySection::SystemWorkflows
        );
        assert!(cx.debug_bounds("system-workflows-marker").is_some());

        select_section(&view, cx, GallerySection::LocalUnlock);
        assert!(cx.debug_bounds("local-unlock-marker").is_some());
        select_section(&view, cx, GallerySection::Overview);
        assert!(cx.debug_bounds("overview-marker").is_some());
    }

    #[gpui::test]
    fn adaptive_shell_filters_scrubs_and_retains_draft_and_selection(cx: &mut TestAppContext) {
        let (view, cx) = open_gallery(cx);
        select_section(&view, cx, GallerySection::AdaptiveShell);
        assert!(cx.debug_bounds("catalog-master").is_some());
        assert!(cx.debug_bounds("catalog-detail").is_some());

        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.catalog_list.scroll_to_index(499);
                cx.notify();
            });
        });
        cx.run_until_parked();
        let offset_before_filter = cx.read(|cx| view.read(cx).catalog_list.scroll_offset_px);
        assert!(offset_before_filter > 0.0);

        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.catalog_filter.update(cx, |filter, cx| {
                    filter.set_value("纪要", window, cx);
                });
                view.apply_catalog_filter(window, cx);
                view.catalog_selected = Some(3);
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| view.read(cx).catalog_query.clone()), "纪要");
        assert_eq!(cx.read(|cx| view.read(cx).catalog_list.item_count), 100);
        let window = cx.read(|cx| view.read(cx).catalog_list.window());
        assert!(window.visible_range.len() <= 12);
        assert!(window.scroll_offset_px <= 100.0 * 40.0 - 320.0);
        assert_eq!(cx.read(|cx| view.read(cx).catalog_selected), Some(3));

        cx.simulate_resize(size(px(500.), px(900.)));
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| view.read(cx).catalog_filter.read(cx).value().to_owned()),
            "纪要"
        );
        assert_eq!(cx.read(|cx| view.read(cx).catalog_selected), Some(3));
        assert!(cx.debug_bounds("catalog-master").is_some());
        assert!(cx.debug_bounds("catalog-detail").is_none());

        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.catalog_detail_open = true;
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("catalog-detail").is_some());
        assert!(cx.debug_bounds("catalog-master").is_none());

        cx.simulate_resize(size(px(900.), px(2000.)));
        cx.run_until_parked();
        assert!(cx.debug_bounds("catalog-master").is_some());
        assert!(cx.debug_bounds("catalog-detail").is_some());
        assert_eq!(
            cx.read(|cx| view.read(cx).catalog_filter.read(cx).value().to_owned()),
            "纪要"
        );
    }

    #[gpui::test]
    fn adaptive_sheet_isolates_background_and_restores_trigger_focus(cx: &mut TestAppContext) {
        let (view, cx) = open_gallery(cx);
        select_section(&view, cx, GallerySection::AdaptiveShell);
        let (trigger, sheet_focus) = cx.read(|cx| {
            let view = view.read(cx);
            (
                view.bottom_sheet.trigger_focus().clone(),
                view.bottom_sheet.sheet_focus().clone(),
            )
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.open_sheet(window, cx));
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("adaptive-sheet").is_some());
        assert!(cx.debug_bounds("adaptive-shell-marker").is_none());
        assert!(cx.debug_bounds("section-navigation").is_none());
        assert!(cx.debug_bounds("overview-marker").is_none());
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(sheet_focus)
        );

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.debug_bounds("adaptive-sheet").is_none());
        assert!(cx.debug_bounds("adaptive-shell-marker").is_some());
        assert_eq!(cx.update(|window, cx| window.focused(cx)), Some(trigger));

        let duration = cx.read(|cx| {
            let view = view.read(cx);
            let preferences = view.preferences.snapshot(cx);
            view.bottom_sheet
                .motion_duration_ms(view.section_theme(&preferences))
        });
        assert_eq!(duration, 220);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.shell_reduced_motion = true;
                let preferences = view.preferences.snapshot(cx);
                assert_eq!(
                    view.bottom_sheet
                        .motion_duration_ms(view.section_theme(&preferences)),
                    0
                );
                cx.notify();
            });
        });
        cx.run_until_parked();
    }
}
