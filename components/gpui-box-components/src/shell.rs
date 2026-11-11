use crate::{Layout, Theme, button, label};
use gpui::{prelude::*, *};
use std::rc::Rc;

/// Dialog presentation. The host must isolate background content and own focus restoration.
pub fn dialog(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    content: impl IntoElement,
    theme: Theme,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(accesskit::Role::Dialog)
        .aria_label(title.into())
        .p_6()
        .gap_4()
        .flex()
        .flex_col()
        .bg(rgb(theme.surface))
        .rounded_lg()
        .border_2()
        .border_color(rgb(theme.border))
        .child(content)
        .child(button("dismiss", "Close", theme, dismiss))
}
pub fn bottom_sheet(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    content: impl IntoElement,
    theme: Theme,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    dialog(id, title, content, theme, dismiss)
        .w_full()
        .rounded_t_lg()
}
pub fn toast(
    id: impl Into<ElementId>,
    message: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<Div> {
    crate::label(id, message, theme)
        .role(accesskit::Role::Status)
        .p_3()
        .bg(rgb(theme.surface))
        .rounded_lg()
}
/// Safe-area padding is relative to the current platform content viewport.
pub fn safe_area(window: &Window) -> Div {
    let edges = window.insets().effective();
    div()
        .pt(edges.top)
        .pr(edges.right)
        .pb(edges.bottom)
        .pl(edges.left)
}

/// Accessible application header with an optional leading slot and trailing
/// actions. The title keeps the caller's text scale.
pub fn app_bar(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    leading: Option<AnyElement>,
    trailing: impl IntoIterator<Item = AnyElement>,
    theme: Theme,
) -> Stateful<Div> {
    let title = title.into();
    let mut bar = div()
        .id(id)
        .role(accesskit::Role::Header)
        .aria_label(title.clone())
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .min_h(px(56.))
        .w_full()
        .px_4()
        .py_2()
        .bg(rgb(theme.surface))
        .border_b(px(if theme.high_contrast { 2. } else { 1. }))
        .border_color(rgb(theme.border));
    if let Some(leading) = leading {
        bar = bar.child(leading);
    }
    bar = bar
        .child(label((id_from(&title, "title"), 0usize), title, theme).flex_1())
        .children(trailing);
    bar
}

fn id_from(title: &SharedString, prefix: &'static str) -> SharedString {
    SharedString::from(format!("{prefix}-{title}"))
}

/// One bottom-navigation destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub id: SharedString,
    pub label: SharedString,
    pub icon_glyph: Option<SharedString>,
}
impl NavItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon_glyph: None,
        }
    }
    pub fn with_icon(mut self, glyph: impl Into<SharedString>) -> Self {
        self.icon_glyph = Some(glyph.into());
        self
    }
}

type NavCallback = dyn Fn(usize, &mut Window, &mut App);

/// Accessible tab bar. Each destination is a focusable Tab with a 48px touch
/// target and both pointer and AccessKit Click activation.
pub fn bottom_navigation(
    id: impl Into<ElementId>,
    items: &[NavItem],
    selected_index: usize,
    theme: Theme,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let container_id: ElementId = id.into();
    let on_select: Rc<NavCallback> = Rc::new(on_select);
    let mut bar = div()
        .id(container_id.clone())
        .role(accesskit::Role::Navigation)
        .aria_label("Primary navigation")
        .flex()
        .flex_row()
        .items_stretch()
        .w_full()
        .min_h(px(56.))
        .bg(rgb(theme.surface))
        .border_t(px(if theme.high_contrast { 2. } else { 1. }))
        .border_color(rgb(theme.border));
    for (index, item) in items.iter().enumerate() {
        let selected = index == selected_index;
        let background = if selected {
            theme.accent
        } else {
            theme.surface
        };
        let foreground = if selected { 0xffffff } else { theme.text };
        let click = on_select.clone();
        let a11y = on_select.clone();
        let mut tab = div()
            .id((container_id.clone(), item.id.clone()))
            .role(accesskit::Role::Tab)
            .aria_label(item.label.clone())
            .aria_selected(selected)
            .focusable()
            .tab_index(0)
            .flex()
            .flex_1()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .min_h(px(48.))
            .py_2()
            .bg(rgb(background))
            .text_color(rgb(foreground))
            .border_b(px(if selected { 3. } else { 0. }))
            .border_color(rgb(theme.border))
            .focus_visible(|style| style.border_2().border_color(rgb(theme.text)))
            .cursor_pointer()
            .on_click(move |_, window, cx| click(index, window, cx))
            .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                a11y(index, window, cx)
            });
        if let Some(glyph) = item.icon_glyph.clone() {
            tab = tab.child(
                div()
                    .text_size(px(18. * theme.text_scale))
                    .text_color(rgb(foreground))
                    .child(glyph),
            );
        }
        tab = tab.child(
            div()
                .text_size(px(12. * theme.text_scale))
                .text_color(rgb(foreground))
                .child(item.label.clone()),
        );
        bar = bar.child(tab);
    }
    bar
}

/// Retained open state and the focus pair used by a bottom sheet.
pub struct BottomSheetController {
    open: bool,
    trigger_focus: FocusHandle,
    sheet_focus: FocusHandle,
}
impl BottomSheetController {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            trigger_focus: cx.focus_handle(),
            sheet_focus: cx.focus_handle(),
        }
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn trigger_focus(&self) -> &FocusHandle {
        &self.trigger_focus
    }
    pub fn sheet_focus(&self) -> &FocusHandle {
        &self.sheet_focus
    }
    pub fn open(&mut self, window: &mut Window, cx: &mut App) {
        self.open = true;
        window.dismiss_virtual_keyboard();
        self.sheet_focus.focus(window, cx);
    }
    /// Alias used by hosts that describe presentation rather than state.
    pub fn present(&mut self, window: &mut Window, cx: &mut App) {
        self.open(window, cx);
    }
    /// Build a dialog-style bottom sheet whose Close button tracks the
    /// controller's sheet focus. Opening therefore lands on Close, so
    /// Enter/Space dismisses without a pointer and Escape remains available to
    /// the host's key handler. Focus restoration still belongs to `dismiss`.
    pub fn sheet(
        &self,
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        content: impl IntoElement,
        theme: Theme,
        dismiss: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(accesskit::Role::Dialog)
            .aria_label(title.into())
            .p_6()
            .gap_4()
            .flex()
            .flex_col()
            .bg(rgb(theme.surface))
            .rounded_lg()
            .border_2()
            .border_color(rgb(theme.border))
            .w_full()
            .rounded_t_lg()
            .child(content)
            .child(button("dismiss", "Close", theme, dismiss).track_focus(&self.sheet_focus))
    }
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut App) {
        if !self.open {
            return;
        }
        self.open = false;
        window.dismiss_virtual_keyboard();
        self.trigger_focus.focus(window, cx);
    }
    pub fn motion_duration_ms(&self, theme: Theme) -> u32 {
        theme.transition_ms(220)
    }
}

/// Bottom inset currently occupied by the soft keyboard or IME.
pub fn keyboard_bottom_inset(window: &Window) -> Pixels {
    let bottom = window.insets().ime.bottom;
    if bottom > px(0.) { bottom } else { px(0.) }
}

/// Apply keyboard avoidance as padding around content. This reads window insets
/// only; it never mutates focus, sheet or scroll controller state.
pub fn keyboard_avoidance(window: &Window, content: impl IntoElement) -> Div {
    div()
        .w_full()
        .pb(keyboard_bottom_inset(window))
        .child(content)
}

/// Compact/wide master-detail presentation. Both panes can be retained by the
/// caller; only the layout tree changes. Compact shows the detail when present,
/// otherwise the master. Wide always shows both, substituting `empty_detail`
/// when no detail is selected.
pub fn master_detail(
    id: impl Into<ElementId>,
    layout: Layout,
    master: impl IntoElement,
    detail: Option<AnyElement>,
    empty_detail: impl IntoElement,
    theme: Theme,
) -> Stateful<Div> {
    let master = master.into_any_element();
    let empty_detail = empty_detail.into_any_element();
    let mut container = div()
        .id(id)
        .flex()
        .size_full()
        .gap_4()
        .p_2()
        .bg(rgb(theme.background))
        .text_color(rgb(theme.text));
    match layout {
        Layout::Wide => {
            let detail = detail.unwrap_or(empty_detail);
            container = container
                .flex_row()
                .items_start()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(240.))
                        .border_r(px(if theme.high_contrast { 2. } else { 1. }))
                        .border_color(rgb(theme.border))
                        .pr_3()
                        .child(master),
                )
                .child(div().flex_1().min_w(px(240.)).child(detail));
        }
        Layout::Compact => {
            container = container.flex_col();
            container = match detail {
                Some(detail) => container.child(detail),
                None => container.child(master),
            };
        }
    }
    container
}
