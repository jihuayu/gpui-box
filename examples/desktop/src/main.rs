fn main() {
    gpui_box::application().run(|cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        gpui_box_example_hello::open(cx);
    });
}
