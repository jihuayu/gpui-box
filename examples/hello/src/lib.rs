//! One Rust page shared by the desktop executable and Android native library.
mod input;
mod retained_demo;
use gpui::prelude::FluentBuilder;
use gpui::*;

struct Hello {
    count: usize,
    input: Entity<input::TextInput>,
    secondary_input: Entity<input::TextInput>,
}
impl Render for Hello {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(rgb(0x171d28))
            .text_color(rgb(0xe8edf5))
            .font_family("Noto Sans CJK SC")
            .when(self.count > 0, |element| {
                element.on_back_gesture(cx.listener(|this, event: &BackGestureEvent, _, cx| {
                    if event.phase == BackGesturePhase::Committed {
                        this.count = this.count.saturating_sub(1);
                        cx.notify();
                    }
                }))
            })
            .child(div().text_3xl().child("gpui-box"))
            .child("Desktop and Android · One Rust view")
            .child("你好，GPUI！中文渲染与输入")
            .child(
                div()
                    .id("counter")
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(0x245fc7))
                    .cursor_pointer()
                    .child(format!("Count: {} · Tap to increment", self.count))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        this.count += 1;
                        cx.notify();
                    })),
            )
            .child("Text input")
            .child(self.input.clone())
            .child("Second input · Check focus isolation")
            .child(self.secondary_input.clone())
            .child(
                div().text_sm().child(
                    "Rotate to test surface replacement. Back decrements a positive counter.",
                ),
            )
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled OFL font");
    input::bind_keys(cx);
    let bounds = Bounds::centered(None, size(px(720.), px(480.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| {
            let input = cx.new(input::TextInput::new);
            let secondary_input = cx.new(input::TextInput::new);
            cx.new(|_| Hello {
                count: 0,
                input,
                secondary_input,
            })
        },
    )
    .expect("open shared example");
    cx.activate(true);
}

/// Opens the retained-state integration gallery through the normal GPUI runtime.
pub fn open_retained(cx: &mut App) {
    cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(retained_demo::RetainedDemo::new)
    })
    .expect("open retained UI example");
    cx.activate(true);
}

/// Opens the normal gallery with offscreen WGPU delta consumption enabled.
#[cfg(feature = "retained-wgpu")]
pub fn open_retained_gpu(cx: &mut App) {
    cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(retained_demo::RetainedDemo::with_gpu)
    })
    .expect("open retained GPU integration example");
    cx.activate(true);
}
