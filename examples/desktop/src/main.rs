use gpui_box::gpui::{
    App, AppContext, Bounds, Context, IntoElement, Render, Styled, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};

struct Welcome;

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .items_center()
            .gap_4()
            .bg(rgb(0x171d28))
            .text_color(rgb(0xe8edf5))
            .child(div().text_3xl().child("gpui-box"))
            .child("One GPUI distribution for desktop and mobile.")
            .child("Windows / macOS / Linux / Android / iOS")
    }
}

fn main() {
    gpui_box::application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(720.0), px(480.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Welcome),
        )
        .expect("desktop window should open");
        cx.activate(true);
    });
}
