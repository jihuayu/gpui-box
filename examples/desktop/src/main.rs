fn main() {
    let retained = std::env::args().any(|argument| argument == "--retained");
    gpui_box::application().run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if retained {
            gpui_box_example_hello::open_retained(cx);
        } else {
            gpui_box_example_hello::open(cx);
        }
    });
}
