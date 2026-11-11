use gpui::{prelude::FluentBuilder, *};
use gpui_box_components::{Layout, TextInput, Theme, app_bar, button, label, master_detail, shell};

const ITEMS: [&str; 3] = ["Welcome", "Shared components", "Platform services"];

struct ApplicationView {
    selected: Option<usize>,
    detail_open: bool,
    draft: Entity<TextInput>,
    committed: String,
    activation_count: usize,
    master_focus: FocusHandle,
}

impl ApplicationView {
    fn close_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detail_open = false;
        window.dismiss_virtual_keyboard();
        window.focus(&self.master_focus, cx);
        cx.notify();
    }
}

impl Render for ApplicationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let layout = Layout::for_width(f32::from(window.viewport_size().width));
        let compact_detail = layout == Layout::Compact && self.detail_open;
        let master = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(label("items-title", "Workspace items", theme))
            .children(ITEMS.into_iter().enumerate().map(|(index, title)| {
                let entity = cx.entity().downgrade();
                button(
                    ("item", index),
                    format!(
                        "{}{}",
                        if self.selected == Some(index) {
                            "Selected · "
                        } else {
                            ""
                        },
                        title
                    ),
                    theme,
                    move |window, cx| {
                        let _ = entity.update(cx, |view, cx| {
                            view.selected = Some(index);
                            view.detail_open = true;
                            window.blur(cx);
                            window.dismiss_virtual_keyboard();
                            cx.notify();
                        });
                    },
                )
                .when(self.selected.unwrap_or(0) == index, |element| {
                    element.track_focus(&self.master_focus)
                })
            }));
        let detail = self
            .selected
            .filter(|_| layout == Layout::Wide || self.detail_open)
            .map(|index| {
                let entity = cx.entity().downgrade();
                let back = cx.entity().downgrade();
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when(layout == Layout::Compact, |element| {
                        element.child(button(
                            "back-to-items",
                            "Back to items",
                            theme,
                            move |window, cx| {
                                let _ = back.update(cx, |view, cx| view.close_detail(window, cx));
                            },
                        ))
                    })
                    .child(label("selected-title", ITEMS[index], theme))
                    .child("A shared draft survives navigation and window resizing.")
                    .child(self.draft.clone())
                    .child(button(
                        "commit-draft",
                        "Commit draft",
                        theme,
                        move |window, cx| {
                            let _ = entity.update(cx, |view, cx| {
                                view.committed = view.draft.read(cx).value().to_owned();
                                window.blur(cx);
                                window.dismiss_virtual_keyboard();
                                cx.notify();
                            });
                        },
                    ))
                    .child(label(
                        "committed-draft",
                        format!("Committed draft · {}", self.committed),
                        theme,
                    ))
                    .into_any_element()
            });
        let body = master_detail(
            "workspace",
            layout,
            master,
            detail,
            label("empty-detail", "Choose an item to open its detail.", theme),
            theme,
        )
        .overflow_y_scroll();
        shell::safe_area(window)
            .size_full()
            .flex()
            .flex_col()
            .tab_group()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.text))
            .font_family("Noto Sans CJK SC")
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
                if compact_detail && event.keystroke.key == "escape" {
                    view.close_detail(window, cx);
                    cx.stop_propagation();
                }
            }))
            .when(compact_detail, |element| {
                element.on_back_gesture(cx.listener(
                    |view, event: &BackGestureEvent, window, cx| {
                        if event.phase == BackGesturePhase::Committed {
                            view.close_detail(window, cx);
                        }
                    },
                ))
            })
            .child(app_bar(
                "app-header",
                "Adaptive workspace",
                None,
                Vec::<AnyElement>::new(),
                theme,
            ))
            .child(
                label(
                    "layout-status",
                    format!(
                        "Layout · {layout:?} · Activation events {}",
                        self.activation_count
                    ),
                    theme,
                )
                .p_3(),
            )
            .child(shell::keyboard_avoidance(window, body).flex_1().min_h_0())
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled font");
    gpui_box_components::input::bind_keys(cx);
    let window = cx
        .open_window(WindowOptions::default(), |_, cx| {
            let draft = cx.new(|cx| {
                let mut input = TextInput::new(cx);
                input.set_label("Shared workspace draft", cx);
                input
            });
            cx.new(|cx| ApplicationView {
                selected: None,
                detail_open: false,
                draft,
                committed: String::new(),
                activation_count: 0,
                master_focus: cx.focus_handle(),
            })
        })
        .expect("open application");
    #[cfg(target_os = "android")]
    {
        let mut app = cx.to_async();
        gpui_box::android::on_activation(move |_, _| {
            window
                .update(&mut app, |view, _, cx| {
                    view.activation_count += 1;
                    cx.notify();
                })
                .is_ok()
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = window;
    cx.activate(true);
}
