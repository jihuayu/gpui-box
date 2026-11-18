//! Actual GPUI event dispatch into the backend-independent UiDocument router.
use core::prelude::v1::test;
use gpui::*;
use gpui_box::retained::{input::*, style::*, ui_ir::*, *};
use std::{cell::RefCell, rc::Rc};
#[path = "retained_focus_reveal_tests.rs"]
mod focus_reveal_tests;
#[path = "retained_wheel_input_tests.rs"]
mod wheel_tests;

const ROOT: UiNodeId = UiNodeId::new(0, 1);
const BUTTON: UiNodeId = UiNodeId::new(1, 1);
struct State {
    document: UiDocument,
    resources: UiResources,
    router: UiInputRouter,
    frame: local::LocalFrame,
    activations: usize,
    events: Vec<UiInputEventKind>,
}
impl State {
    fn new() -> Self {
        let mut root = UiNode::new(
            ROOT,
            LayoutStyle::row().size(LayoutDimension::Px(140.0), LayoutDimension::Px(60.0)),
        );
        root.children = vec![BUTTON];
        let mut button = UiNode::new(
            BUTTON,
            LayoutStyle::row().size(LayoutDimension::Px(100.0), LayoutDimension::Px(40.0)),
        );
        button.interaction = UiInteraction {
            focus: UiFocusPolicy::Tab,
            pointer_handler: Some(UiHandlerId(1)),
            activate_handler: Some(UiHandlerId(2)),
            ..Default::default()
        };
        button.styles = vec![
            StyleRef::new(
                "feedback",
                vec![StyleDeclaration::Color(
                    StyleRule::value(0xff0000ff)
                        .when(
                            StyleCondition {
                                hovered: Some(true),
                                ..Default::default()
                            },
                            0x0000ffff,
                        )
                        .when(
                            StyleCondition {
                                pressed: Some(true),
                                ..Default::default()
                            },
                            0x00ff00ff,
                        ),
                )],
            )
            .unwrap(),
        ];
        let resources = UiResources::new();
        let (document, publication) = UiDocument::from_snapshot(
            UiSnapshot::new(700, 1, ROOT, vec![root, button]),
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 60.0),
            [140.0, 60.0],
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
            activations: 0,
            events: vec![],
        }
    }
    fn event(&mut self, event: UiInput) {
        let effects = self.router.route(event).unwrap();
        for dispatch in &effects.dispatches {
            if self.document.accepts_dispatch(dispatch) {
                self.events.push(dispatch.kind);
                if dispatch.kind == UiInputEventKind::Activate {
                    self.activations += 1;
                }
            }
        }
        let batch = UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: 700,
            epoch: 1,
            base_revision: self.document.revision(),
            revision: self.document.revision() + 1,
            sequence: self.document.next_sequence(),
            operations: effects.style_operations(&self.document),
        };
        let publication = self
            .document
            .apply(
                batch,
                &self.resources,
                BackendCapabilities::default(),
                LayoutConstraints::viewport(140.0, 60.0),
                [140.0, 60.0],
            )
            .unwrap();
        self.frame = publication.frame;
        // Test host adopts CPU-published complete geometry. This label is not
        // native display evidence; canvas paint below runs through GPUI normally.
        self.router
            .adopt(publication.input, UiInputEvidence::Published)
            .unwrap();
    }
}
struct View {
    state: Rc<RefCell<State>>,
}
impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .id("owned-input-host")
            .debug_selector(|| "owned-input-host".into())
            .w(px(140.0))
            .h(px(60.0))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.state.borrow_mut().event(UiInput::PointerMove([
                    f32::from(event.position.x),
                    f32::from(event.position.y),
                ]));
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.state.borrow_mut().event(UiInput::PrimaryDown([
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    ]));
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                    this.state.borrow_mut().event(UiInput::PrimaryUp([
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    ]));
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    move |_, _, _| state.borrow().frame.resolve(),
                    |bounds, frame, window, _| {
                        for item in frame.items.iter() {
                            if let Some(DisplayCommand::Fill(color)) = item.commands.first() {
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: bounds.origin
                                            + point(px(item.bounds.x), px(item.bounds.y)),
                                        size: size(px(item.bounds.width), px(item.bounds.height)),
                                    },
                                    rgba(*color),
                                ));
                            }
                        }
                    },
                )
                .w(px(140.0))
                .h(px(60.0)),
            )
    }
}
#[gpui::test]
fn normal_mouse_click_routes_through_owned_input_and_complete_feedback_frame(
    cx: &mut TestAppContext,
) {
    let state = Rc::new(RefCell::new(State::new()));
    let shared = state.clone();
    let (_view, cx) = cx.add_window_view(move |_, _| View { state: shared });
    let host = cx.debug_bounds("owned-input-host").unwrap();
    cx.simulate_click(
        host.origin + point(px(20.0), px(20.0)),
        Modifiers::default(),
    );
    cx.run_until_parked();
    let state = state.borrow();
    assert_eq!(state.activations, 1);
    assert!(state.events.contains(&UiInputEventKind::PointerDown));
    assert!(state.events.contains(&UiInputEventKind::PointerUp));
    assert_eq!(state.router.focused(), Some(BUTTON));
    assert_eq!(
        state
            .document
            .resolved_node(BUTTON)
            .unwrap()
            .properties
            .color,
        0x0000ffff
    );
    assert_eq!(
        state
            .frame
            .resolve()
            .items
            .iter()
            .find(|item| item.fill_color == 0x0000ffff)
            .unwrap()
            .bounds
            .width,
        100.0
    );
}
