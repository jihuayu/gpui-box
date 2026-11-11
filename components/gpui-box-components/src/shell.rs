use crate::{Theme, button};
use gpui::{prelude::*, *};

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
