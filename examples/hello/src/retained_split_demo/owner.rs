//! LogicalUi and all component/source/Flex work stay on this dedicated thread.
use super::*;
use gpui_box::{
    command::{Command, CommandReceiver},
    retained::{
        logical::*,
        reactive::{Property, Signal},
        responsive::LayoutEnvironment,
        style::*,
        ui_ir::UiProperties,
    },
};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) enum OwnerCommand {
    Viewport {
        viewport: [f32; 2],
        scale: f32,
        text: TextResource,
    },
    Input(UiInputEffects),
    /// Explicit diagnostic workload. Never runs on the platform/present thread.
    Stall {
        millis: u64,
    },
}
impl Command for OwnerCommand {
    type Output = OwnerUpdate;
    type Error = String;
}
pub(super) struct OwnerUpdate {
    pub scene: UiInputScene,
    pub count: u32,
    pub animated: NodeId,
    pub footer: NodeId,
    pub controls: Vec<AnimationControl>,
    pub turn: u64,
    pub viewport: [f32; 2],
    pub scale: f32,
}
#[derive(Default)]
pub(super) struct OwnerProbe {
    pub stalled: AtomicBool,
    pub stall_started: AtomicU64,
    pub stall_finished: AtomicU64,
    pub turns: AtomicU64,
}
struct Model {
    ui: LogicalUi,
    count: Signal<u32>,
    animated: NodeId,
    footer: NodeId,
    text: NodeId,
    viewport: [f32; 2],
    scale: f32,
    next_animation: u64,
    turn: u64,
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
impl Model {
    fn new(viewport: [f32; 2], scale: f32, resource: TextResource) -> Result<Self, Error> {
        if resource.scale() != scale {
            return Err(Error::InvalidTextStyle);
        }
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::column()
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
                .padding(LayoutEdges::all(LayoutLength::Px(20.0)))
                .gap(LayoutLength::Px(16.0), LayoutLength::Px(0.0)),
            0x17202fff,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let root = ui.root_render();
        let scope = ui.root_scope();
        let count = ui.state(scope, 0, || 0u32)?;
        let (animated, footer, text) = ui
            .edit_local_frame(
                LayoutConstraints::viewport(viewport[0], viewport[1]),
                viewport,
                |_| Ok(()),
                |(), edit| {
                    let animated =
                        edit.append_layout(scope, root, fixed(200.0, 48.0), 0x2862bcff)?;
                    let state = edit.interaction_state(scope, 1, animated, false, false)?;
                    let plan = StylePlan::new(&[StyleRef::new(
                        "split-button",
                        vec![StyleDeclaration::Color(
                            StyleRule::value(0x2862bcff)
                                .when(
                                    StyleCondition {
                                        hovered: Some(true),
                                        ..Default::default()
                                    },
                                    0x3979d9ff,
                                )
                                .when(
                                    StyleCondition {
                                        pressed: Some(true),
                                        ..Default::default()
                                    },
                                    0x17437fff,
                                )
                                .when(
                                    StyleCondition {
                                        focus_visible: Some(true),
                                        ..Default::default()
                                    },
                                    0x8152c7ff,
                                ),
                        )],
                    )
                    .map_err(|_| Error::InvalidStyle)?]);
                    let style = edit.computed(scope, 2, &[state.dependency()], move |i| {
                        plan.resolve_values(
                            &fixed(200.0, 48.0),
                            &UiProperties::default(),
                            &LayoutEnvironment::new([640.0, 400.0]),
                            *i.read_prop(state)?,
                        )
                        .map_err(|_| Error::InvalidStyle)
                    })?;
                    edit.bind(style, Property::style_resolution_without_scroll(animated))?;
                    let handler = edit.event_handler(
                        scope,
                        1,
                        EventHandler::new(move |event, cx| {
                            if event.kind == UiInputEventKind::Activate {
                                let n = *cx.read(count)?;
                                cx.set(count, n.saturating_add(1))?;
                            }
                            Ok(())
                        }),
                    )?;
                    edit.interaction(
                        scope,
                        animated,
                        Some(UiInteraction {
                            focus: UiFocusPolicy::Tab,
                            activate_handler: Some(handler),
                            ..Default::default()
                        }),
                    )?;
                    let scroll = edit.append_layout(
                        scope,
                        root,
                        LayoutStyle::column()
                            .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(180.0))
                            .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(100.0))
                            .flex_grow(1.0)
                            .overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
                        0x223149ff,
                    )?;
                    let text = edit.append_layout(
                        scope,
                        scroll,
                        LayoutStyle::column()
                            .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                            .flex_shrink(0.0),
                        0,
                    )?;
                    edit.properties().text(
                        text,
                        Some(TextPaint {
                            resource,
                            color: 0xe8edf5ff,
                        }),
                    );
                    edit.append_layout(scope, scroll, fixed(20.0, 140.0), 0)?;
                    let footer =
                        edit.append_layout(scope, scroll, fixed(200.0, 48.0), 0x7253a8ff)?;
                    edit.interaction(
                        scope,
                        footer,
                        Some(UiInteraction {
                            focus: UiFocusPolicy::Tab,
                            activate_handler: Some(handler),
                            ..Default::default()
                        }),
                    )?;
                    Ok((animated, footer, text))
                },
            )?
            .0
            .result
            .value;
        Ok(Self {
            ui,
            count,
            animated,
            footer,
            text,
            viewport,
            scale,
            next_animation: 1,
            turn: 0,
        })
    }
    fn process(&mut self, command: OwnerCommand) -> Result<OwnerUpdate, Error> {
        let before = *self.ui.read(self.count)?;
        match command {
            OwnerCommand::Viewport {
                viewport,
                scale,
                text,
            } => {
                if text.scale() != scale {
                    return Err(Error::InvalidTextStyle);
                }
                self.ui.edit_local_frame(
                    LayoutConstraints::viewport(viewport[0], viewport[1]),
                    viewport,
                    |_| Ok(()),
                    |(), e| {
                        e.properties().text(
                            self.text,
                            Some(TextPaint {
                                resource: text,
                                color: 0xe8edf5ff,
                            }),
                        );
                        Ok(())
                    },
                )?;
                self.viewport = viewport;
                self.scale = scale;
            }
            OwnerCommand::Input(effects) => {
                self.ui.process_input_effects(&effects)?;
            }
            OwnerCommand::Stall { .. } => {}
        }
        self.ui.local_frame(self.viewport)?;
        let count = *self.ui.read(self.count)?;
        let mut controls = vec![];
        if count != before {
            controls.push(AnimationControl::Animate(AnimationSpec {
                id: self.next_animation,
                node: self.animated,
                destination: AnimationValue::Translation([
                    if count.is_multiple_of(2) { 0.0 } else { 120.0 },
                    0.0,
                ]),
                curve: AnimationCurve::Spring(Spring {
                    mass: 1.0,
                    stiffness: 180.0,
                    damping: 24.0,
                }),
                duration: Duration::from_millis(1500),
                start: AnimationStart::Now,
                velocity: RetargetVelocity::Preserve,
            }));
            self.next_animation += 1;
        }
        self.turn += 1;
        Ok(OwnerUpdate {
            scene: self.ui.input_scene()?,
            count,
            animated: self.animated,
            footer: self.footer,
            controls,
            turn: self.turn,
            viewport: self.viewport,
            scale: self.scale,
        })
    }
}
pub(super) fn run(mut receiver: CommandReceiver<OwnerCommand>, probe: Arc<OwnerProbe>) {
    let mut model = None;
    while futures::executor::block_on(receiver.ready()) {
        let Some(mut request) = receiver.try_recv() else {
            continue;
        };
        let command = request.take_command().unwrap();
        if let OwnerCommand::Stall { millis } = &command {
            probe.stall_started.fetch_add(1, Ordering::SeqCst);
            probe.stalled.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis((*millis).min(2000)));
            probe.stalled.store(false, Ordering::SeqCst);
            probe.stall_finished.fetch_add(1, Ordering::SeqCst);
        }
        let result = match (&mut model, command) {
            (
                None,
                OwnerCommand::Viewport {
                    viewport,
                    scale,
                    text,
                },
            ) => Model::new(viewport, scale, text).and_then(|mut m| {
                m.ui.local_frame(viewport)?;
                m.turn = 1;
                let output = OwnerUpdate {
                    scene: m.ui.input_scene()?,
                    count: 0,
                    animated: m.animated,
                    footer: m.footer,
                    controls: vec![],
                    turn: 1,
                    viewport,
                    scale,
                };
                model = Some(m);
                Ok(output)
            }),
            (Some(model), command) => model.process(command),
            _ => Err(Error::InvalidFrame),
        };
        probe.turns.fetch_add(1, Ordering::SeqCst);
        match result {
            Ok(value) => request.succeed(value, None),
            Err(error) => request.fail(error.to_string()),
        }
    }
}
