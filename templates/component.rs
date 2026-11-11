use gpui::{prelude::FluentBuilder, *};
use gpui_box_components::{
    Layout, TextInput, Theme, app_bar, button, checkbox, label, shell, switch,
};

struct ComponentPreview {
    input: Entity<TextInput>,
    dark: bool,
    high_contrast: bool,
    reduced_motion: bool,
    large_text: bool,
    checked: bool,
    activations: usize,
}

impl ComponentPreview {
    fn theme(&self) -> Theme {
        let theme = if self.dark { Theme::DARK } else { Theme::LIGHT };
        theme
            .with_high_contrast(self.high_contrast)
            .with_reduced_motion(self.reduced_motion)
            .with_text_scale(if self.large_text { 1.25 } else { 1.0 })
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = self.theme();
        self.input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        window.dismiss_virtual_keyboard();
        cx.notify();
    }
}

impl Render for ComponentPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let layout = Layout::for_width(f32::from(window.viewport_size().width));
        let dark = cx.entity().downgrade();
        let contrast = cx.entity().downgrade();
        let motion = cx.entity().downgrade();
        let scale = cx.entity().downgrade();
        let checked = cx.entity().downgrade();
        let action = cx.entity().downgrade();
        let controls = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(label("preview-settings", "Preview settings", theme))
            .child(switch(
                "dark-theme",
                "Dark theme",
                self.dark,
                theme,
                move |window, cx| {
                    let _ = dark.update(cx, |view, cx| {
                        view.dark = !view.dark;
                        view.refresh(window, cx);
                    });
                },
            ))
            .child(switch(
                "contrast",
                "High contrast",
                self.high_contrast,
                theme,
                move |window, cx| {
                    let _ = contrast.update(cx, |view, cx| {
                        view.high_contrast = !view.high_contrast;
                        view.refresh(window, cx);
                    });
                },
            ))
            .child(switch(
                "motion",
                "Reduced motion",
                self.reduced_motion,
                theme,
                move |window, cx| {
                    let _ = motion.update(cx, |view, cx| {
                        view.reduced_motion = !view.reduced_motion;
                        view.refresh(window, cx);
                    });
                },
            ))
            .child(switch(
                "text-scale",
                "Large text",
                self.large_text,
                theme,
                move |window, cx| {
                    let _ = scale.update(cx, |view, cx| {
                        view.large_text = !view.large_text;
                        view.refresh(window, cx);
                    });
                },
            ));
        // Replace this application-owned specimen with your component. Keep
        // shared text/focus/semantics behavior in the coordinated component crate.
        let specimen = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(label("specimen-title", "Your component specimen", theme))
            .child(self.input.clone())
            .child(checkbox(
                "checked",
                "Example checked state",
                self.checked,
                theme,
                move |window, cx| {
                    let _ = checked.update(cx, |view, cx| {
                        view.checked = !view.checked;
                        view.refresh(window, cx);
                    });
                },
            ))
            .child(button(
                "activate",
                "Activate specimen",
                theme,
                move |window, cx| {
                    let _ = action.update(cx, |view, cx| {
                        view.activations += 1;
                        view.refresh(window, cx);
                    });
                },
            ))
            .child(label(
                "activations",
                format!("Activations · {}", self.activations),
                theme,
            ))
            .child(label(
                "motion-policy",
                format!("Motion duration · {} ms", theme.transition_ms(200)),
                theme,
            ));
        let body = div()
            .id("preview-body")
            .flex()
            .gap_4()
            .p_4()
            .when(layout == Layout::Compact, |element| element.flex_col())
            .child(div().flex_1().min_w_0().child(controls))
            .child(div().flex_1().min_w_0().child(specimen))
            .overflow_y_scroll();
        shell::safe_area(window)
            .size_full()
            .flex()
            .flex_col()
            .tab_group()
            .font_family("Noto Sans CJK SC")
            .bg(rgb(theme.background))
            .text_color(rgb(theme.text))
            .on_key_down(|event, window, cx| match event.keystroke.key.as_str() {
                "tab" => {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
                "escape" => {
                    window.blur(cx);
                    window.dismiss_virtual_keyboard();
                    cx.stop_propagation();
                }
                _ => {}
            })
            .child(app_bar(
                "preview-header",
                "Component author preview",
                None,
                Vec::<AnyElement>::new(),
                theme,
            ))
            .child(label("layout-status", format!("Layout · {layout:?}"), theme).p_3())
            .child(shell::keyboard_avoidance(window, body).flex_1().min_h_0())
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled font");
    gpui_box_components::input::bind_keys(cx);
    cx.open_window(WindowOptions::default(), |_, cx| {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.set_theme(Theme::DARK, cx);
            input.set_label("Component preview input", cx);
            input
        });
        cx.new(|_| ComponentPreview {
            input,
            dark: true,
            high_contrast: false,
            reduced_motion: false,
            large_text: false,
            checked: false,
            activations: 0,
        })
    })
    .expect("open component preview");
    cx.activate(true);
}
