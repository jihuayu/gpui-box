//! Reusable controls built against the distribution's single GPUI identity.
pub mod input;
pub mod model;
pub mod query;
pub mod reactive;
pub mod shell;
use gpui::{prelude::*, *};
pub use input::TextInput;

/// Shared tokens with logical dimensions and 48px touch targets.
#[derive(Clone, Copy, PartialEq)]
pub struct Theme {
    pub background: u32,
    pub surface: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub border: u32,
    pub text_scale: f32,
}
impl Theme {
    pub const DARK: Self = Self {
        background: 0x171d28,
        surface: 0x243247,
        text: 0xe8edf5,
        muted: 0xa9bbd2,
        accent: 0x245fc7,
        border: 0x48607e,
        text_scale: 1.0,
    };
    pub const LIGHT: Self = Self {
        background: 0xeaf1fa,
        surface: 0xffffff,
        text: 0x172941,
        muted: 0x4e6380,
        accent: 0x245fc7,
        border: 0xb0c4de,
        text_scale: 1.0,
    };
}
/// GPUI supplies pointer, touch and keyboard click activation. AccessKit shares the action.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label = label.into();
    let action = std::rc::Rc::new(action);
    let click = action.clone();
    div()
        .id(id)
        .role(accesskit::Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_index(0)
        .min_h(px(48.))
        .text_size(px(16. * theme.text_scale))
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(rgb(theme.accent))
        .text_color(rgb(0xffffff))
        .focus_visible(|style| style.border_2().border_color(rgb(theme.text)))
        .cursor_pointer()
        .child(label)
        .on_click(move |_, window, cx| click(window, cx))
        .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
            action(window, cx)
        })
}
pub fn label(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(accesskit::Role::Label)
        .aria_value(text.clone())
        .text_size(px(16. * theme.text_scale))
        .text_color(rgb(theme.text))
        .child(text)
}
pub fn checkbox(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    checked: bool,
    theme: Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let text = text.into();
    button(
        id,
        format!("{}  {}", if checked { "☑" } else { "☐" }, text),
        theme,
        action,
    )
    .role(accesskit::Role::CheckBox)
    .aria_label(text)
    .aria_toggled(checked.into())
}
pub fn switch(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    checked: bool,
    theme: Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let text = text.into();
    button(
        id,
        format!("{}  {}", if checked { "● On" } else { "○ Off" }, text),
        theme,
        action,
    )
    .role(accesskit::Role::Switch)
    .aria_label(text)
    .aria_toggled(checked.into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    Empty,
    Error,
    Offline,
    Ready,
}
pub fn status(id: impl Into<ElementId>, state: Status, theme: Theme) -> Stateful<Div> {
    let message = match state {
        Status::Loading => "Loading items…",
        Status::Empty => "No items. Add an item to begin.",
        Status::Error => "Items could not be loaded. Try again.",
        Status::Offline => "Offline. Showing saved items.",
        Status::Ready => "Items are up to date.",
    };
    label(id, message, theme)
        .role(accesskit::Role::Status)
        .a11y_synthetic_children(|builder| builder.parent_node().set_live(accesskit::Live::Polite))
        .p_3()
        .bg(rgb(theme.surface))
        .rounded_lg()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Compact,
    Wide,
}

/// A semantic icon supplied by the application font or symbol set.
pub fn icon(
    id: impl Into<ElementId>,
    glyph: impl Into<SharedString>,
    name: impl Into<SharedString>,
    theme: Theme,
) -> Stateful<Div> {
    label(id, glyph, theme)
        .role(accesskit::Role::Image)
        .aria_label(name.into())
}

pub fn list(
    id: impl Into<ElementId>,
    items: impl IntoIterator<Item = impl IntoElement>,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(accesskit::Role::List)
        .flex()
        .flex_col()
        .gap_2()
        .children(items)
}

pub fn scroll_view(id: impl Into<ElementId>, content: impl IntoElement) -> Stateful<Div> {
    div().id(id).size_full().overflow_y_scroll().child(content)
}
impl Layout {
    pub fn for_width(width: f32) -> Self {
        if width >= 720. {
            Self::Wide
        } else {
            Self::Compact
        }
    }
}
