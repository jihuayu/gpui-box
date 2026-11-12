fn main() {
    let retained = std::env::args().any(|argument| argument == "--retained");
    #[cfg(feature = "retained-wgpu")]
    let delivery_check = std::env::args()
        .skip_while(|argument| argument != "--retained-delivery-check")
        .nth(1)
        .map(std::path::PathBuf::from);
    #[cfg(all(feature = "native-check", target_os = "windows"))]
    let native_check = std::env::args()
        .skip_while(|arg| arg != "--retained-native-check")
        .nth(1)
        .map(std::path::PathBuf::from);
    gpui_box::application().run(move |cx| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        #[cfg(all(feature = "native-check", target_os = "windows"))]
        if let Some(output) = native_check {
            gpui_box_example_hello::check_retained_native(cx, output);
            return;
        }
        #[cfg(feature = "retained-wgpu")]
        if let Some(output) = delivery_check {
            gpui_box_example_hello::check_retained_delivery(cx, output);
            return;
        }
        #[cfg(feature = "retained-wgpu")]
        if retained {
            gpui_box_example_hello::open_retained_gpu(cx);
            return;
        }
        if retained {
            gpui_box_example_hello::open_retained(cx);
        } else {
            gpui_box_example_hello::open(cx);
        }
    });
}
