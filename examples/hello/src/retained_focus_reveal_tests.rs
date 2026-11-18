use super::*;
const HOST: UiNodeId = UiNodeId::new(2, 1);
const PREFIX: UiNodeId = UiNodeId::new(3, 1);
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
struct RevealState {
    document: UiDocument,
    resources: UiResources,
    router: UiInputRouter,
    frame: local::LocalFrame,
    events: usize,
}
impl RevealState {
    fn new() -> Self {
        let mut root = UiNode::new(ROOT, fixed(140.0, 80.0));
        root.children = vec![HOST];
        let mut host = UiNode::new(
            HOST,
            fixed(100.0, 50.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
        );
        host.children = vec![PREFIX, BUTTON];
        let prefix = UiNode::new(PREFIX, fixed(100.0, 90.0));
        let mut button = UiNode::new(BUTTON, fixed(100.0, 20.0));
        button.interaction.focus = UiFocusPolicy::Tab;
        button.properties.color = 0xff0000ff;
        let resources = UiResources::new();
        let (document, publication) = UiDocument::from_snapshot(
            UiSnapshot::new(703, 1, ROOT, vec![root, host, prefix, button]),
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 80.0),
            [140.0, 80.0],
        )
        .unwrap();
        let mut router = UiInputRouter::new();
        router
            .adopt(publication.input, UiInputEvidence::Published)
            .unwrap();
        Self {
            document,
            resources,
            router,
            frame: publication.frame,
            events: 0,
        }
    }
    fn tab(&mut self, backwards: bool) {
        let effects = self.router.route(UiInput::Tab { backwards }).unwrap();
        let plan = self.document.plan_input_scroll(&effects).unwrap();
        let output = self
            .document
            .apply(
                plan.batch,
                &self.resources,
                BackendCapabilities::default(),
                LayoutConstraints::viewport(140.0, 80.0),
                [140.0, 80.0],
            )
            .unwrap();
        self.frame = output.frame;
        self.router
            .adopt(output.input, UiInputEvidence::Published)
            .unwrap();
        self.events += 1;
    }
}
struct RevealView {
    state: Rc<RefCell<RevealState>>,
    focus: FocusHandle,
}
impl Render for RevealView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .id("focus-reveal-host")
            .debug_selector(|| "focus-reveal-host".into())
            .track_focus(&self.focus)
            .w(px(140.0))
            .h(px(80.0))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "tab" {
                    this.state.borrow_mut().tab(event.keystroke.modifiers.shift);
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |_, _, _| state.borrow().frame.resolve(),
                    |bounds, frame, window, _| {
                        for item in frame.items.iter() {
                            let rect = item
                                .clip
                                .map_or(item.bounds, |clip| item.bounds.intersect(clip));
                            window.paint_quad(fill(
                                Bounds {
                                    origin: bounds.origin + point(px(rect.x), px(rect.y)),
                                    size: size(px(rect.width), px(rect.height)),
                                },
                                rgba(item.fill_color),
                            ));
                        }
                    },
                )
                .w(px(140.0))
                .h(px(80.0)),
            )
    }
}
#[gpui::test]
fn actual_tab_key_reveals_offscreen_control_in_normal_complete_frame(cx: &mut TestAppContext) {
    let state = Rc::new(RefCell::new(RevealState::new()));
    let shared = state.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        RevealView {
            state: shared,
            focus,
        }
    });
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let state = state.borrow();
    assert_eq!(state.events, 1);
    assert_eq!(state.router.focused(), Some(BUTTON));
    assert_eq!(
        state.document.node(HOST).unwrap().properties.scroll,
        [0.0, 60.0]
    );
    assert_eq!(
        state
            .frame
            .resolve()
            .items
            .iter()
            .find(|item| item.fill_color == 0xff0000ff)
            .unwrap()
            .bounds
            .y,
        30.0
    );
    assert_eq!(
        state.document.input_scene().hit_test([10.0, 40.0]),
        Some(BUTTON)
    );
}
