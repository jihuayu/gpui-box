use super::*;
const CONTENT: UiNodeId = UiNodeId::new(2, 1);
struct ScrollState {
    document: UiDocument,
    resources: UiResources,
    router: UiInputRouter,
    frame: local::LocalFrame,
    events: usize,
}
impl ScrollState {
    fn new() -> Self {
        let mut root = UiNode::new(
            ROOT,
            LayoutStyle::column().size(LayoutDimension::Px(140.0), LayoutDimension::Px(80.0)),
        );
        root.children = vec![BUTTON];
        let mut host = UiNode::new(
            BUTTON,
            LayoutStyle::column()
                .size(LayoutDimension::Px(100.0), LayoutDimension::Px(50.0))
                .overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
        );
        host.children = vec![CONTENT];
        host.properties.clip = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        });
        let mut content = UiNode::new(
            CONTENT,
            LayoutStyle::column()
                .size(LayoutDimension::Px(100.0), LayoutDimension::Px(200.0))
                .flex_shrink(0.0),
        );
        content.properties.color = 0xff8844ff;
        let resources = UiResources::new();
        let (document, publication) = UiDocument::from_snapshot(
            UiSnapshot::new(701, 1, ROOT, vec![root, host, content]),
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
    fn scroll(&mut self, event: &ScrollEvent) {
        let delta = event.delta.pixel_delta(px(16.0));
        let effects = self
            .router
            .route(UiInput::Wheel {
                position: [event.position.x.into(), event.position.y.into()],
                delta: [-f32::from(delta.x), -f32::from(delta.y)],
            })
            .unwrap();
        let plan = self.document.plan_input_scroll(&effects).unwrap();
        let publication = self
            .document
            .apply(
                plan.batch,
                &self.resources,
                BackendCapabilities::default(),
                LayoutConstraints::viewport(140.0, 80.0),
                [140.0, 80.0],
            )
            .unwrap();
        self.frame = publication.frame;
        self.router
            .adopt(publication.input, UiInputEvidence::Published)
            .unwrap();
        self.events += 1;
    }
}
struct ScrollView {
    state: Rc<RefCell<ScrollState>>,
}
impl Render for ScrollView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .id("owned-wheel-host")
            .debug_selector(|| "owned-wheel-host".into())
            .w(px(140.0))
            .h(px(80.0))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                this.state.borrow_mut().scroll(event);
                cx.notify();
            }))
            .child(
                canvas(
                    move |_, _, _| state.borrow().frame.resolve(),
                    |origin, frame, window, _| {
                        for item in frame.items.iter() {
                            if let Some(DisplayCommand::Fill(color)) = item.commands.first() {
                                let bounds = if let Some(clip) = item.clip {
                                    item.bounds.intersect(clip)
                                } else {
                                    item.bounds
                                };
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: origin.origin + point(px(bounds.x), px(bounds.y)),
                                        size: size(px(bounds.width), px(bounds.height)),
                                    },
                                    rgba(*color),
                                ));
                            }
                        }
                    },
                )
                .w(px(140.0))
                .h(px(80.0)),
            )
    }
}
#[gpui::test]
fn actual_gpui_scroll_events_update_owned_offset_and_complete_frame(cx: &mut TestAppContext) {
    let state = Rc::new(RefCell::new(ScrollState::new()));
    let shared = state.clone();
    let (_, cx) = cx.add_window_view(move |_, _| ScrollView { state: shared });
    let bounds = cx.debug_bounds("owned-wheel-host").unwrap();
    for _ in 0..2 {
        cx.simulate_event(ScrollEvent {
            position: bounds.origin + point(px(10.0), px(10.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-25.0))),
            ..Default::default()
        });
        cx.run_until_parked();
    }
    let state = state.borrow();
    assert_eq!(state.events, 2);
    assert_eq!(
        state.document.node(BUTTON).unwrap().properties.scroll,
        [0.0, 50.0]
    );
    assert_eq!(
        state
            .frame
            .resolve()
            .items
            .iter()
            .find(|item| item.fill_color == 0xff8844ff)
            .unwrap()
            .bounds
            .y,
        -50.0
    );
    assert_eq!(state.router.focused(), None);
    assert_eq!(
        state.frame.hit_test(10.0, 10.0),
        state.document.published_frame().hit_test(10.0, 10.0)
    );
}
