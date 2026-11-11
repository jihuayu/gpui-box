use gpui::{
    Context, Entity, IntoElement, KeyDownEvent, Modifiers, Render, TestAppContext,
    VisualTestContext, Window, div, point, prelude::*, px, size,
};
use gpui_box_components::{
    Layout, TextInput, Theme, button, label,
    shell::{BottomSheetController, NavItem, app_bar, bottom_navigation, master_detail},
};

struct ShellView {
    layout: Layout,
    theme: Theme,
    filter: Entity<TextInput>,
    sheet: BottomSheetController,
    selected: usize,
    detail_open: bool,
}

impl Render for ShellView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let back = button("shell-back", "Back", theme, |_, _| {});
        let action = button("shell-action", "Action", theme, |_, _| {});
        let detail = if self.detail_open || self.layout == Layout::Wide {
            Some(
                div()
                    .debug_selector(|| "detail-pane".into())
                    .child("Retained detail")
                    .into_any_element(),
            )
        } else {
            None
        };
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                app_bar(
                    "shell-app-bar",
                    "Shell test",
                    Some(back.into_any_element()),
                    [action.into_any_element()],
                    theme,
                )
                .debug_selector(|| "test-app-bar".into()),
            )
            .child(
                master_detail(
                    "shell-master-detail",
                    self.layout,
                    div()
                        .debug_selector(|| "master-pane".into())
                        .child(self.filter.clone()),
                    detail,
                    div()
                        .debug_selector(|| "empty-detail".into())
                        .child("Nothing selected"),
                    theme,
                )
                .debug_selector(|| "test-master-detail".into()),
            );

        let items = [
            NavItem::new("one", "One").with_icon("1"),
            NavItem::new("two", "Two").with_icon("2"),
            NavItem::new("three", "Three").with_icon("3"),
        ];
        let entity = cx.entity().downgrade();
        root = root.child(
            bottom_navigation(
                "shell-nav",
                &items,
                self.selected,
                theme,
                move |index, _, cx| {
                    let _ = entity.update(cx, |view, cx| {
                        view.selected = index;
                        cx.notify();
                    });
                },
            )
            .debug_selector(|| "test-nav".into()),
        );

        if self.sheet.is_open() {
            let entity = cx.entity().downgrade();
            let sheet = self
                .sheet
                .sheet(
                    "shell-sheet",
                    "Sheet test",
                    label("shell-sheet-message", "Sheet content", theme),
                    theme,
                    move |window, cx| {
                        let _ = entity.update(cx, |view, cx| {
                            view.sheet.dismiss(window, cx);
                            cx.notify();
                        });
                    },
                )
                .debug_selector(|| "test-sheet".into());
            root = root.child(sheet);
        }
        let entity = cx.entity().downgrade();
        root.id("shell-root")
            .tab_group()
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    let _ = entity.update(cx, |view, cx| {
                        view.sheet.dismiss(window, cx);
                        cx.notify();
                    });
                    cx.stop_propagation();
                    window.prevent_default();
                }
            })
    }
}

fn new_view(cx: &mut TestAppContext) -> (Entity<ShellView>, &mut VisualTestContext) {
    cx.add_window_view(|_, cx| {
        let filter = cx.new(TextInput::new);
        filter.update(cx, |filter, cx| {
            filter.set_label("Catalog filter", cx);
            filter.set_theme(Theme::DARK, cx);
        });
        ShellView {
            layout: Layout::Compact,
            theme: Theme::DARK,
            filter,
            sheet: BottomSheetController::new(cx),
            selected: 0,
            detail_open: false,
        }
    })
}

#[gpui::test]
fn app_bar_master_detail_and_bottom_navigation_render_and_retain_state(cx: &mut TestAppContext) {
    let (view, cx) = new_view(cx);
    cx.simulate_resize(size(px(600.), px(800.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("test-app-bar").is_some());
    assert!(cx.debug_bounds("test-master-detail").is_some());
    assert!(cx.debug_bounds("test-nav").is_some());
    assert!(cx.debug_bounds("master-pane").is_some());
    assert!(cx.debug_bounds("detail-pane").is_none());

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.filter.update(cx, |filter, cx| {
                filter.set_value("retained draft", window, cx);
            });
            view.layout = Layout::Wide;
            cx.notify();
        });
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("master-pane").is_some());
    assert!(cx.debug_bounds("detail-pane").is_some());
    assert_eq!(
        cx.read(|cx| view.read(cx).filter.read(cx).value().to_owned()),
        "retained draft"
    );

    let nav = cx.debug_bounds("test-nav").expect("navigation is rendered");
    let position = point(nav.left() + nav.size.width * (2.5 / 3.0), nav.center().y);
    cx.simulate_click(position, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| view.read(cx).selected), 2);

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.layout = Layout::Compact;
            view.detail_open = true;
            cx.notify();
        });
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("detail-pane").is_some());
    assert!(cx.debug_bounds("master-pane").is_none());
    assert_eq!(
        cx.read(|cx| view.read(cx).filter.read(cx).value().to_owned()),
        "retained draft"
    );
}

#[gpui::test]
fn bottom_sheet_controller_restores_trigger_and_sheet_focus(cx: &mut TestAppContext) {
    let (view, cx) = new_view(cx);
    cx.simulate_resize(size(px(600.), px(800.)));
    cx.run_until_parked();
    let (trigger, sheet_focus) = cx.read(|cx| {
        let view = view.read(cx);
        (
            view.sheet.trigger_focus().clone(),
            view.sheet.sheet_focus().clone(),
        )
    });
    assert_eq!(
        cx.read(|cx| view.read(cx).sheet.motion_duration_ms(Theme::DARK)),
        220
    );
    assert_eq!(
        cx.read(|cx| view
            .read(cx)
            .sheet
            .motion_duration_ms(Theme::DARK.with_reduced_motion(true))),
        0
    );

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.sheet.open(window, cx);
            cx.notify();
        });
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| view.read(cx).sheet.is_open()));
    assert!(cx.debug_bounds("test-sheet").is_some());
    assert_eq!(
        cx.update(|window, cx| window.focused(cx)),
        Some(sheet_focus.clone())
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!cx.read(|cx| view.read(cx).sheet.is_open()));
    assert!(cx.debug_bounds("test-sheet").is_none());
    assert_eq!(cx.update(|window, cx| window.focused(cx)), Some(trigger));
}
