//! Real GPUI window events and image installation over the common owner runtime.
use gpui::*;
use gpui_box::retained::{
    input::{gpui_host::GpuiInputAdapter, *},
    local::LocalFrame,
    logical::*,
    paragraph::ParagraphSnapshot,
    reactive::{Property, Signal},
    responsive::LayoutEnvironment,
    software::{SoftwareJob, SoftwareOptions},
    style::*,
    text::{GlyphRasterizer, TextPaint},
    ui_ir::UiProperties,
    *,
};
use std::sync::Arc;

#[derive(Clone)]
enum Raster {
    Image(Arc<RenderImage>),
    #[cfg(all(feature = "retained-wgpu", target_os = "windows"))]
    Gpu(WindowsGpuSurface),
}
#[derive(Clone)]
struct Presentation {
    raster: Raster,
    frame: LocalFrame,
    scene: UiInputScene,
    size: [u32; 2],
    scale: f32,
    viewport: [f32; 2],
    gpu_epoch: Option<u64>,
}
struct Request {
    frame: LocalFrame,
    scene: UiInputScene,
    size: [u32; 2],
    scale: f32,
    viewport: [f32; 2],
}

pub(super) struct OwnerDemo {
    ui: LogicalUi,
    router: UiInputRouter,
    count: Signal<u32>,
    scroll_host: NodeId,
    #[cfg(feature = "native-check")]
    buttons: [NodeId; 2],
    focus: FocusHandle,
    bounds: Option<Bounds<Pixels>>,
    shown: Option<Presentation>,
    ready: Option<Presentation>,
    cpu_task: Option<Task<()>>,
    pending: Option<Request>,
    last_requested: Option<(UiInputSceneId, [u32; 2], u32)>,
    cpu: bool,
    error: Option<String>,
    events: usize,
    installs: usize,
    _activation: Subscription,
    _blur: Subscription,
    #[cfg(feature = "retained-wgpu")]
    gpu: Option<gpui_box::retained::wgpu::WgpuFrameRenderer>,
    #[cfg(feature = "retained-wgpu")]
    _completion: Option<Task<()>>,
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn prepare_text(window: &Window, cx: &App, text: &str) -> TextPaint {
    let paragraph = ParagraphSnapshot::shape(
        window.text_system(),
        text,
        font("Noto Sans CJK SC"),
        16.0,
        32.0,
    )
    .unwrap();
    let resource = GlyphRasterizer::new(cx.text_system().clone())
        .prepare(paragraph, window.scale_factor())
        .unwrap();
    TextPaint {
        resource,
        color: 0xe8edf5ff,
    }
}
impl OwnerDemo {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>, cpu: bool) -> Self {
        #[cfg(feature = "retained-wgpu")]
        let mut cpu = cpu;
        #[cfg(not(feature = "retained-wgpu"))]
        let cpu = cpu || !cfg!(feature = "retained-wgpu");
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::column()
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
                .padding(LayoutEdges::all(LayoutLength::Px(20.0)))
                .gap(LayoutLength::Px(16.0), LayoutLength::Px(0.0)),
            0x17202fff,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let count = ui.state(scope, 0, || 0u32).unwrap();
        let title = prepare_text(
            window,
            cx,
            "Retained owner runtime · 原生输入 / Flex / 文字",
        );
        let button_texts = [
            prepare_text(window, cx, "点击或 Enter / Space：计数 +1"),
            prepare_text(window, cx, "Tab 到这里：计数 +10"),
        ];
        let long = prepare_text(
            window,
            cx,
            "滚轮与焦点滚入\n这段文字使用持久 Flex 布局。\nCPU 模式在后台完成整张画面后才替换。\n鼠标移入按钮显示 hover，按下显示 pressed。\nTab / Shift+Tab 切换焦点，Space 在释放时激活。\n调整窗口大小会重新布局。\n焦点和命中沿实际安装的画面更新。\n这一行在滚动容器下方。\n继续滚动可以看到底部按钮。",
        );
        let (scroll_host, _buttons) = ui
            .edit_local_frame(
                LayoutConstraints::viewport(640.0, 400.0),
                [640.0, 400.0],
                |_| Ok(()),
                |(), edit| {
                    let heading =
                        edit.append_layout(scope, root, LayoutStyle::column().flex_shrink(0.0), 0)?;
                    edit.properties().text(heading, Some(title));
                    let button = edit.append_layout(scope, root, fixed(320.0, 48.0), 0)?;
                    Self::button(edit, scope, button, 1, count, button_texts[0].clone(), 1)?;
                    let host = edit.append_layout(
                        scope,
                        root,
                        LayoutStyle::column()
                            .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(200.0))
                            .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(80.0))
                            .flex_grow(1.0)
                            .overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
                        0x223149ff,
                    )?;
                    let text = edit.append_layout(
                        scope,
                        host,
                        LayoutStyle::column()
                            .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                            .flex_shrink(0.0),
                        0,
                    )?;
                    edit.properties().text(text, Some(long));
                    edit.append_layout(scope, host, fixed(100.0, 80.0), 0)?;
                    let bottom = edit.append_layout(scope, host, fixed(300.0, 48.0), 0)?;
                    Self::button(edit, scope, bottom, 4, count, button_texts[1].clone(), 10)?;
                    Ok((host, [button, bottom]))
                },
            )
            .unwrap()
            .0
            .result
            .value;
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.input(UiInput::WindowFocusLost, cx);
            }
        });
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.input(UiInput::WindowFocusLost, cx)
        });
        #[cfg(feature = "retained-wgpu")]
        let (gpu, error) = if cpu {
            (None, None)
        } else {
            match gpui_box::retained::wgpu::WgpuFrameRenderer::new_for_window(window, 3) {
                Ok(gpu) => (Some(gpu), None),
                Err(error) => {
                    cpu = true;
                    (
                        None,
                        Some(format!("GPU unavailable; CPU fallback: {error}")),
                    )
                }
            }
        };
        #[cfg(not(feature = "retained-wgpu"))]
        let error = None;
        #[cfg(feature = "retained-wgpu")]
        let mut gpu = gpu;
        #[cfg(feature = "retained-wgpu")]
        let completion = gpu.as_mut().map(|gpu| {
            let mut receiver = gpu.open_completions().unwrap();
            cx.spawn(async move |view, cx| {
                while let Ok(Some(_)) = receiver.ready().await {
                    if view
                        .update(cx, |this, cx| {
                            if let Some(gpu) = &mut this.gpu
                                && let Err(error) = gpu.poll()
                            {
                                this.error = Some(error.to_string());
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        Self {
            ui,
            router: UiInputRouter::new(),
            count,
            scroll_host,
            #[cfg(feature = "native-check")]
            buttons: _buttons,
            focus,
            bounds: None,
            shown: None,
            ready: None,
            cpu_task: None,
            pending: None,
            last_requested: None,
            cpu,
            error,
            events: 0,
            installs: 0,
            _activation: activation,
            _blur: blur,
            #[cfg(feature = "retained-wgpu")]
            gpu,
            #[cfg(feature = "retained-wgpu")]
            _completion: completion,
        }
    }
    fn button(
        edit: &mut LogicalEdit<'_, '_, '_>,
        scope: ScopeId,
        node: NodeId,
        key: u64,
        count: Signal<u32>,
        text: TextPaint,
        step: u32,
    ) -> Result<(), Error> {
        let state = edit.interaction_state(scope, key, node, false, false)?;
        let plan = StylePlan::new(&[StyleRef::new(
            "button",
            vec![
                StyleDeclaration::Color(
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
                ),
                StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(LayoutLength::Px(
                    12.0,
                )))),
            ],
        )
        .map_err(|_| Error::InvalidStyle)?]);
        let output = edit.computed(scope, key + 1, &[state.dependency()], move |i| {
            plan.resolve_values(
                &LayoutStyle::row()
                    .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(48.0))
                    .flex_shrink(0.0),
                &UiProperties::default(),
                &LayoutEnvironment::new([640.0, 400.0]),
                *i.read_prop(state)?,
            )
            .map_err(|_| Error::InvalidStyle)
        })?;
        edit.bind(output, Property::style_resolution_without_scroll(node))?;
        edit.properties().text(node, Some(text));
        let handler = edit.event_handler(
            scope,
            key,
            EventHandler::new(move |event, cx| {
                if event.kind == UiInputEventKind::Activate {
                    let value = *cx.read(count)?;
                    cx.set(count, value.saturating_add(step))?;
                }
                Ok(())
            }),
        )?;
        edit.interaction(
            scope,
            node,
            Some(UiInteraction {
                focus: UiFocusPolicy::Tab,
                activate_handler: Some(handler),
                ..Default::default()
            }),
        )
    }
    fn adapter(&self) -> Option<GpuiInputAdapter> {
        let bounds = self.bounds?;
        let shown = self.shown.as_ref()?;
        GpuiInputAdapter::new(
            Rect {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: f32::from(bounds.size.width).min(shown.viewport[0]),
                height: f32::from(bounds.size.height).min(shown.viewport[1]),
            },
            20.0,
        )
        .ok()
    }
    fn input(&mut self, input: UiInput, cx: &mut Context<Self>) {
        if self.router.scene_id().is_none()
            || self.router.scene_id() != self.shown.as_ref().map(|shown| shown.scene.id())
        {
            return;
        }
        match self.ui.handle_input(&mut self.router, input) {
            Ok(result) => {
                self.events += 1;
                if result.commit.commit.request_frame {
                    cx.notify();
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
        // The caption is diagnostic and changes only after real input.
        cx.notify();
    }
    fn request_cpu(&mut self, request: Request, cx: &mut Context<Self>) {
        self.pending = Some(request);
        if self.cpu_task.is_some() {
            return;
        }
        self.start_cpu(cx);
    }
    fn start_cpu(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.pending.take() else {
            return;
        };
        let frame = request.frame.clone();
        let size = request.size;
        let scale = request.scale;
        let work = cx.background_executor().spawn(async move {
            let mut job = SoftwareJob::new(
                frame.resolve(),
                SoftwareOptions {
                    size,
                    scale,
                    background: 0,
                    effects: pacing::SoftwareEffects::Exact,
                    max_bytes: 128 * 1024 * 1024,
                },
            )?;
            while !job.step(8192)? {}
            let frame = job.into_frame()?;
            let mut pixels = frame.pixels().to_vec();
            for p in pixels.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
            let image = image::RgbaImage::from_raw(size[0], size[1], pixels).unwrap();
            Ok::<_, software::SoftwareError>(Arc::new(RenderImage::new(vec![image::Frame::new(
                image,
            )])))
        });
        self.cpu_task = Some(cx.spawn(async move |view, cx| {
            let output = work.await;
            let _ = view.update(cx, |this, cx| {
                this.cpu_task = None;
                match output {
                    Ok(image) => {
                        this.ready = Some(Presentation {
                            raster: Raster::Image(image),
                            frame: request.frame,
                            scene: request.scene,
                            size,
                            scale,
                            viewport: request.viewport,
                            gpu_epoch: None,
                        });
                    }
                    Err(error) => this.error = Some(format!("CPU raster: {error:?}")),
                }
                this.start_cpu(cx);
                cx.notify();
            });
        }));
    }
    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Presentation> {
        self.bounds = Some(bounds);
        let viewport = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
        if viewport.iter().any(|v| *v <= 0.0) {
            return None;
        }
        let scale = window.scale_factor();
        let size = viewport.map(|v| (v * scale).ceil().max(1.0) as u32);
        let frame = match self.ui.local_frame(viewport) {
            Ok(frame) => frame,
            Err(error) => {
                self.error = Some(error.to_string());
                return self.shown.clone();
            }
        };
        let scene = self.ui.input_scene().unwrap();
        let request = (scene.id(), size, scale.to_bits());
        if self.cpu {
            if self.last_requested != Some(request) {
                self.last_requested = Some(request);
                self.request_cpu(
                    Request {
                        frame,
                        scene,
                        size,
                        scale,
                        viewport,
                    },
                    cx,
                );
            }
            if let Some(ready) = self.ready.take()
                && ready.size == size
                && ready.scale == scale
            {
                return Some(ready);
            }
            return self.shown.clone();
        }
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut self.gpu {
            #[cfg(target_os = "windows")]
            match gpu.ensure_window_device(window) {
                Ok(true) => {
                    self.last_requested = None;
                    // Rebind notifications to this replacement epoch.
                    let mut receiver = gpu.open_completions().unwrap();
                    self._completion = Some(cx.spawn(async move |view, cx| {
                        while let Ok(Some(_)) = receiver.ready().await {
                            if view
                                .update(cx, |this, cx| {
                                    if let Some(gpu) = &mut this.gpu
                                        && let Err(error) = gpu.poll()
                                    {
                                        this.error = Some(error.to_string());
                                    }
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }));
                }
                Ok(false) => {}
                Err(error) => {
                    self.error = Some(error.to_string());
                    return self.shown.clone();
                }
            }
            if self.last_requested == Some(request) {
                return self.shown.clone();
            }
            match gpu.poll_frame_capacity() {
                Ok(capacity) if capacity.available() => {}
                Ok(_) => return self.shown.clone(),
                Err(error) => {
                    self.error = Some(error.to_string());
                    return self.shown.clone();
                }
            }
            match gpu.submit_local_for_window(window, &frame, size, scale) {
                Ok(_) => {}
                Err(error) => {
                    self.error = Some(error.to_string());
                    return self.shown.clone();
                }
            }
            #[cfg(target_os = "windows")]
            let raster = match gpu.window_surface() {
                Ok(Some(surface)) => Raster::Gpu(surface),
                _ => match gpu.window_image() {
                    Ok(image) => Raster::Image(image),
                    Err(error) => {
                        self.error = Some(error.to_string());
                        return self.shown.clone();
                    }
                },
            };
            #[cfg(not(target_os = "windows"))]
            let raster = match gpu.window_image() {
                Ok(image) => Raster::Image(image),
                Err(error) => {
                    self.error = Some(error.to_string());
                    return self.shown.clone();
                }
            };
            self.last_requested = Some(request);
            return Some(Presentation {
                raster,
                frame,
                scene,
                size,
                scale,
                viewport,
                gpu_epoch: Some(gpu.device_epoch()),
            });
        }
        None
    }
    fn installed(
        &mut self,
        presentation: Presentation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = self.shown.as_ref().is_some_and(|shown| {
            shown.scene.id() == presentation.scene.id()
                && shown.frame.tree() == presentation.frame.tree()
                && shown.frame.revision() == presentation.frame.revision()
                && shown.size == presentation.size
                && shown.scale == presentation.scale
                && shown.gpu_epoch == presentation.gpu_epoch
        });
        if same && self.router.scene_id() == Some(presentation.scene.id()) {
            return;
        }
        if !same {
            if let Some(Presentation {
                raster: Raster::Image(previous),
                ..
            }) = self.shown.take()
            {
                window.drop_image(previous).ok();
            }
            self.installs += 1;
        }
        let scene = presentation.scene.clone();
        self.shown = Some(presentation);
        match self
            .ui
            .adopt_input_scene(&mut self.router, scene, UiInputEvidence::ImageInstalled)
        {
            Ok(result) => {
                if result.commit.commit.request_frame {
                    cx.notify();
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
            }
        }
    }
}
impl Render for OwnerDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity();
        let paint_owner = owner.clone();
        let scroll_y = self
            .ui
            .input_scene()
            .ok()
            .and_then(|scene| scene.scroll_state(scene.node_id(self.scroll_host)?))
            .map(|state| state.offset[1])
            .unwrap_or(0.0);
        let caption = format!(
            "{} · Count {} · scroll {:.0} · inputs {} · installed {}{}",
            if self.cpu { "CPU worker" } else { "GPU" },
            self.ui.read(self.count).unwrap(),
            scroll_y,
            self.events,
            self.installs,
            self.error
                .as_ref()
                .map(|e| format!(" · {e}"))
                .unwrap_or_default()
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x101827))
            .text_color(rgb(0xe8edf5))
            .font_family("Noto Sans CJK SC")
            .child(
                div()
                    .p_4()
                    .child("Owner runtime · 鼠标 / Tab / Enter / Space / 滚轮")
                    .child(div().text_sm().child(caption)),
            )
            .child(
                div()
                    .id("owner-runtime-host")
                    .debug_selector(|| "owner-runtime-host".into())
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if this.focus.is_focused(window)
                            && let Some(input) = GpuiInputAdapter::key_down(event)
                        {
                            this.input(input, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_key_up(cx.listener(|this, event: &KeyUpEvent, window, cx| {
                        if this.focus.is_focused(window)
                            && let Some(input) = GpuiInputAdapter::key_up(event)
                        {
                            this.input(input, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                owner.update(cx, |this, cx| this.prepare(bounds, window, cx))
                            },
                            move |bounds, presentation, window, cx| {
                                let entity = paint_owner.clone();
                                window.on_mouse_event(
                                    move |event: &MouseMoveEvent, phase, _, cx| {
                                        if phase == DispatchPhase::Bubble {
                                            entity.update(cx, |this, cx| {
                                                if let Some(adapter) = this.adapter() {
                                                    let captured = this.router.captured().is_some();
                                                    this.input(
                                                        adapter.mouse_move(event, captured),
                                                        cx,
                                                    );
                                                }
                                            });
                                        }
                                    },
                                );
                                let entity = paint_owner.clone();
                                window.on_mouse_event(
                                    move |event: &MouseDownEvent, phase, window, cx| {
                                        if phase == DispatchPhase::Bubble {
                                            entity.update(cx, |this, cx| {
                                                if let Some(input) = this
                                                    .adapter()
                                                    .and_then(|adapter| adapter.mouse_down(event))
                                                {
                                                    window.focus(&this.focus, cx);
                                                    this.input(input, cx);
                                                    cx.stop_propagation();
                                                }
                                            });
                                        }
                                    },
                                );
                                let entity = paint_owner.clone();
                                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        entity.update(cx, |this, cx| {
                                            let captured = this.router.captured().is_some();
                                            if let Some(input) =
                                                this.adapter().and_then(|adapter| {
                                                    adapter.mouse_up(event, captured)
                                                })
                                            {
                                                this.input(input, cx);
                                                cx.stop_propagation();
                                            }
                                        });
                                    }
                                });
                                let entity = paint_owner.clone();
                                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        entity.update(cx, |this, cx| {
                                            this.input(UiInput::PointerLeave, cx)
                                        });
                                    }
                                });
                                let entity = paint_owner.clone();
                                window.on_mouse_event(move |event: &ScrollEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        entity.update(cx, |this, cx| {
                                            if let Some(input) = this
                                                .adapter()
                                                .and_then(|adapter| adapter.wheel(event))
                                            {
                                                this.input(input, cx);
                                                cx.stop_propagation();
                                            }
                                        });
                                    }
                                });
                                if let Some(presentation) = presentation {
                                    // Keep old pixels and hit geometry at the same logical
                                    // scale while a resized complete image is pending.
                                    let image_bounds = Bounds::new(
                                        bounds.origin,
                                        size(
                                            px(presentation.viewport[0]),
                                            px(presentation.viewport[1]),
                                        ),
                                    );
                                    window.with_content_mask(
                                        Some(ContentMask { bounds }),
                                        |window| match &presentation.raster {
                                            Raster::Image(image) => {
                                                window
                                                    .paint_image(
                                                        bounds,
                                                        image_bounds,
                                                        Corners::default(),
                                                        image.clone(),
                                                        0,
                                                        false,
                                                    )
                                                    .unwrap();
                                            }
                                            #[cfg(all(
                                                feature = "retained-wgpu",
                                                target_os = "windows"
                                            ))]
                                            Raster::Gpu(surface) => window
                                                .paint_gpu_surface(image_bounds, surface.clone()),
                                        },
                                    );
                                    paint_owner.update(cx, |this, cx| {
                                        this.installed(presentation, window, cx)
                                    });
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

pub(super) fn open(cx: &mut App, cpu: bool) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let bounds = Bounds::centered(None, size(px(720.0), px(560.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |window, cx| {
            window.set_window_title(if cpu {
                "gpui-box owner runtime · CPU"
            } else {
                "gpui-box owner runtime · GPU"
            });
            cx.new(|cx| OwnerDemo::new(window, cx, cpu))
        },
    )
    .unwrap();
    cx.activate(true);
}

#[cfg(feature = "native-check")]
pub(super) mod native_check;
#[cfg(test)]
mod tests;
