use gpui::*;
use gpui_box_components::{TextInput, Theme, button};
struct ApplicationView {
    count: usize,
    input: Entity<TextInput>,
    activation_count: usize,
}
impl Render for ApplicationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .tab_group()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .gap_4()
            .bg(rgb(0x171d28))
            .text_color(rgb(0xe8edf5))
            .font_family("Noto Sans CJK SC")
            .child(div().text_3xl().child("Your gpui-box application"))
            .child("Desktop and Android share this Rust view.")
            .child(format!("Activation events {}", self.activation_count))
            .child(button(
                "counter",
                format!("Count {}", self.count),
                Theme::DARK,
                move |_, cx| {
                    let _ = entity.update(cx, |view, cx| {
                        view.count += 1;
                        cx.notify();
                    });
                },
            ))
            .child(self.input.clone())
    }
}
pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled font");
    gpui_box_components::input::bind_keys(cx);
    let window = cx.open_window(WindowOptions::default(), |_, cx| {
        let input = cx.new(TextInput::new);
        cx.new(|_| ApplicationView { count: 0, input, activation_count: 0 })
    })
    .expect("open application");
    #[cfg(target_os = "android")]
    {
        let mut app = cx.to_async();
        gpui_box::android::on_activation(move |_, _| {
            window.update(&mut app, |view, _, cx| {
                view.activation_count += 1;
                cx.notify();
            }).is_ok()
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = window;
    cx.activate(true);
}
