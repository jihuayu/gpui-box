//! Reusable controls built against the distribution's single GPUI identity.
pub mod input;
pub mod model;
pub mod query;
pub mod reactive;
pub mod shell;
pub mod virtual_list;
use gpui::{prelude::*, *};
pub use input::TextInput;
pub use model::{Navigation, ScreenModel};
pub use shell::{
    BottomSheetController, NavItem, app_bar, bottom_navigation, keyboard_avoidance, master_detail,
};
pub use virtual_list::{VirtualListSpec, VirtualWindow, virtual_list};

/// Shared tokens with logical dimensions and 48px touch targets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub background: u32,
    pub surface: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub border: u32,
    pub text_scale: f32,
    /// Suppresses non-essential animation while preserving functionality.
    pub reduced_motion: bool,
    /// Requests stronger foreground/surface/border separation.
    pub high_contrast: bool,
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
        reduced_motion: false,
        high_contrast: false,
    };
    pub const LIGHT: Self = Self {
        background: 0xeaf1fa,
        surface: 0xffffff,
        text: 0x172941,
        muted: 0x4e6380,
        accent: 0x245fc7,
        border: 0xb0c4de,
        text_scale: 1.0,
        reduced_motion: false,
        high_contrast: false,
    };
    pub const HIGH_CONTRAST_DARK: Self = Self {
        background: 0x0b0f16,
        surface: 0x121924,
        text: 0xffffff,
        muted: 0xd6e2f0,
        accent: 0x3b82f6,
        border: 0xf2f6fc,
        text_scale: 1.0,
        reduced_motion: false,
        high_contrast: true,
    };
    pub const HIGH_CONTRAST_LIGHT: Self = Self {
        background: 0xffffff,
        surface: 0xf4f7fb,
        text: 0x000000,
        muted: 0x2b3a4d,
        accent: 0x0b4fc4,
        border: 0x101820,
        text_scale: 1.0,
        reduced_motion: false,
        high_contrast: true,
    };
    pub const fn with_text_scale(mut self, scale: f32) -> Self {
        self.text_scale = scale;
        self
    }
    pub const fn with_reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = reduced;
        self
    }
    /// Set the high-contrast flag. Enabling it also strengthens the text,
    /// muted and border tokens against the theme's current background while
    /// preserving the application's background, surface and accent palette.
    pub const fn with_high_contrast(mut self, high_contrast: bool) -> Self {
        self.high_contrast = high_contrast;
        if dark_color(self.background) {
            if high_contrast {
                self.text = 0xffffff;
                self.muted = 0xd6e2f0;
                self.border = 0xf2f6fc;
            } else {
                self.text = Self::DARK.text;
                self.muted = Self::DARK.muted;
                self.border = Self::DARK.border;
            }
        } else if high_contrast {
            self.text = 0x000000;
            self.muted = 0x2b3a4d;
            self.border = 0x101820;
        } else {
            self.text = Self::LIGHT.text;
            self.muted = Self::LIGHT.muted;
            self.border = Self::LIGHT.border;
        }
        self
    }
    /// Standard motion duration, suppressed entirely when reduced motion is on.
    pub const fn transition_ms(&self, standard_ms: u32) -> u32 {
        if self.reduced_motion { 0 } else { standard_ms }
    }
}
const fn dark_color(color: u32) -> bool {
    let red = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let blue = color & 0xff;
    red * 299 + green * 587 + blue * 114 < 128 * 1000
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
    let element = div()
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
        });
    if theme.high_contrast {
        element.border_2().border_color(rgb(theme.border))
    } else {
        element
    }
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
