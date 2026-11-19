//! Lightweight GPUI platform/presentation service; LogicalUi lives elsewhere.
use gpui::*;
use gpui_box::{
    command::{CommandResult, CommandSender, command_channel},
    retained::{
        animation::*,
        input::{gpui_host::GpuiInputAdapter, *},
        paragraph::ParagraphSnapshot,
        software::{SoftwareJob, SoftwareOptions},
        text::{GlyphRasterizer, TextPaint, TextResource},
        *,
    },
};
#[cfg(any(test, feature = "native-check"))]
use std::sync::atomic::Ordering;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
mod owner;
#[cfg(test)]
mod tests;
use owner::*;
#[cfg(feature = "native-check")]
pub(super) mod native_check;

#[derive(Clone)]
enum Raster {
    Image(Arc<RenderImage>),
    #[cfg(all(feature = "retained-wgpu", target_os = "windows"))]
    Gpu(WindowsGpuSurface),
}
#[derive(Clone)]
struct Picture {
    raster: Raster,
    scene: UiInputScene,
    size: [u32; 2],
    scale: f32,
    viewport: [f32; 2],
}
struct SplitDemo {
    sender: Option<CommandSender<OwnerCommand>>,
    worker: Option<std::thread::JoinHandle<()>>,
    probe: Arc<OwnerProbe>,
    outstanding: usize,
    replies: BTreeMap<u64, CommandResult<OwnerCommand>>,
    next_reply: u64,
    requested: Option<([f32; 2], u32)>,
    viewport_request: Option<u64>,
    source: Option<UiInputScene>,
    source_viewport: [f32; 2],
    source_scale: f32,
    timeline: Option<AnimationRuntime>,
    router: UiInputRouter,
    shown: Option<Picture>,
    ready: Option<Picture>,
    cpu_task: Option<Task<()>>,
    last_requested: Option<(UiInputSceneId, [u32; 2], u32)>,
    bounds: Option<Bounds<Pixels>>,
    focus: FocusHandle,
    count: u32,
    animated: Option<NodeId>,
    footer: Option<NodeId>,
    owner_turn: u64,
    installs: u64,
    #[cfg(feature = "native-check")]
    installed_during_stall: Vec<(Instant, u64, [f32; 2])>,
    cpu: bool,
    error: Option<String>,
    _blur: Subscription,
    _activation: Subscription,
    #[cfg(feature = "retained-wgpu")]
    gpu: Option<wgpu::WgpuFrameRenderer>,
    #[cfg(feature = "retained-wgpu")]
    _completion: Option<Task<()>>,
}
impl Drop for SplitDemo {
    fn drop(&mut self) {
        self.sender.take();
        self.worker.take();
    }
}
impl SplitDemo {
    fn new(window: &mut Window, cx: &mut Context<Self>, cpu: bool) -> Self {
        #[cfg(not(feature = "retained-wgpu"))]
        let _ = cpu;
        let (sender, receiver) = command_channel(64).unwrap();
        let probe = Arc::new(OwnerProbe::default());
        let worker_probe = probe.clone();
        let worker = std::thread::Builder::new()
            .name("retained-ui-owner".into())
            .spawn(move || owner::run(receiver, worker_probe))
            .unwrap();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.input(UiInput::WindowFocusLost, cx)
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.input(UiInput::WindowFocusLost, cx);
            }
        });
        #[cfg(feature = "retained-wgpu")]
        let (gpu, error) = if cpu {
            (None, None)
        } else {
            match wgpu::WgpuFrameRenderer::new_for_window(window, 3) {
                Ok(g) => (Some(g), None),
                Err(e) => (None, Some(e.to_string())),
            }
        };
        #[cfg(feature = "retained-wgpu")]
        let cpu = cpu || gpu.is_none();
        #[cfg(not(feature = "retained-wgpu"))]
        let (cpu, error) = (true, None);
        let this = Self {
            sender: Some(sender),
            worker: Some(worker),
            probe,
            outstanding: 0,
            replies: BTreeMap::new(),
            next_reply: 1,
            requested: None,
            viewport_request: None,
            source: None,
            source_viewport: [0.0; 2],
            source_scale: 1.0,
            timeline: None,
            router: UiInputRouter::new(),
            shown: None,
            ready: None,
            cpu_task: None,
            last_requested: None,
            bounds: None,
            focus,
            count: 0,
            animated: None,
            footer: None,
            owner_turn: 0,
            installs: 0,
            #[cfg(feature = "native-check")]
            installed_during_stall: vec![],
            cpu,
            error,
            _blur: blur,
            _activation: activation,
            #[cfg(feature = "retained-wgpu")]
            gpu,
            #[cfg(feature = "retained-wgpu")]
            _completion: None,
        };
        #[cfg(feature = "retained-wgpu")]
        {
            let mut this = this;
            this.bind_completion(cx);
            this
        }
        #[cfg(not(feature = "retained-wgpu"))]
        this
    }
    #[cfg(feature = "retained-wgpu")]
    fn bind_completion(&mut self, cx: &mut Context<Self>) {
        if let Some(gpu) = &mut self.gpu {
            let mut receiver = gpu.open_completions().unwrap();
            self._completion = Some(cx.spawn(async move |view, cx| {
                while let Ok(Some(_)) = receiver.ready().await {
                    if view
                        .update(cx, |this, cx| {
                            if let Some(gpu) = &mut this.gpu
                                && let Err(e) = gpu.poll()
                            {
                                this.error = Some(e.to_string());
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
    }
    fn send(&mut self, command: OwnerCommand, cx: &mut Context<Self>) -> bool {
        // Includes completed-but-not-yet-adopted responses, beyond Command's
        // queued/running bound, so foreground scheduling cannot grow this table.
        if self.outstanding >= 64 {
            self.error = Some("owner event capacity reached".into());
            return false;
        }
        let Some(sender) = &self.sender else {
            return false;
        };
        let viewport_request = matches!(&command, OwnerCommand::Viewport { .. });
        let mut ticket = match sender.send(command) {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(format!("owner rejected: {:?}", e.reason));
                return false;
            }
        };
        let serial = ticket.id().sequence();
        if viewport_request {
            self.viewport_request = Some(serial);
        }
        self.outstanding += 1;
        cx.spawn(async move |view, cx| {
            let reply = ticket.result().await;
            let _ = view.update(cx, |this, cx| {
                this.replies.insert(serial, reply);
                this.receive(cx);
            });
        })
        .detach();
        true
    }
    fn receive(&mut self, cx: &mut Context<Self>) {
        while let Some(reply) = self.replies.remove(&self.next_reply) {
            if self.viewport_request == Some(self.next_reply) {
                self.viewport_request = None;
            }
            self.next_reply += 1;
            self.outstanding -= 1;
            match reply {
                Ok(reply) => {
                    let update = reply.output;
                    let now = Instant::now();
                    let result = (|| -> Result<(), AnimationError> {
                        if self.timeline.is_none() {
                            self.timeline = Some(AnimationRuntime::new(
                                update.scene.frame().clone(),
                                AnimationLimits::default(),
                            )?);
                        }
                        let timeline = self.timeline.as_mut().unwrap();
                        timeline.activate(
                            update.scene.frame().clone(),
                            AnimationBatch {
                                scene: AnimationScene::of(update.scene.frame()),
                                sequence: timeline.next_sequence(),
                                controls: update.controls,
                            },
                            now,
                        )?;
                        Ok(())
                    })();
                    match result {
                        Ok(()) => {
                            self.source = Some(update.scene);
                            self.count = update.count;
                            self.animated = Some(update.animated);
                            self.footer = Some(update.footer);
                            self.owner_turn = update.turn;
                            self.source_viewport = update.viewport;
                            self.source_scale = update.scale;
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                Err(e) => self.error = Some(format!("owner command failed: {e:?}")),
            }
        }
        cx.notify();
    }
    fn effects(&mut self, effects: UiInputEffects, cx: &mut Context<Self>) -> bool {
        if effects.dispatches.is_empty()
            && effects.feedback.is_empty()
            && effects.wheel_scrolls.is_empty()
            && effects.focus_reveals.is_empty()
        {
            return true;
        }
        self.send(OwnerCommand::Input(effects), cx)
    }
    fn input(&mut self, input: UiInput, cx: &mut Context<Self>) {
        if self.router.scene_id().is_none()
            || self.router.scene_id() != self.shown.as_ref().map(|p| p.scene.id())
        {
            return;
        }
        let mut candidate = self.router.clone();
        match candidate.route(input) {
            Ok(effects) => {
                if self.effects(effects, cx) {
                    self.router = candidate;
                }
            }
            Err(e) => self.error = Some(format!("input: {e:?}")),
        }
        cx.notify();
    }
    fn adapter(&self) -> Option<GpuiInputAdapter> {
        let b = self.bounds?;
        let shown = self.shown.as_ref()?;
        GpuiInputAdapter::new(
            Rect {
                x: b.origin.x.into(),
                y: b.origin.y.into(),
                width: f32::from(b.size.width).min(shown.viewport[0]),
                height: f32::from(b.size.height).min(shown.viewport[1]),
            },
            20.0,
        )
        .ok()
    }
    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        self.bounds = Some(bounds);
        let viewport = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
        if viewport.iter().any(|v| *v <= 0.0) {
            return self.shown.clone();
        }
        let scale = window.scale_factor();
        let request = (viewport, scale.to_bits());
        if self.requested != Some(request) && self.viewport_request.is_none() {
            let paragraph=ParagraphSnapshot::shape(window.text_system(),"独立 UI owner / Flex\n点击上方矩形或 Enter 启动动画。\nUI 构建线程停顿时，已接纳动画继续。\n鼠标命中来自实际安装的采样画面。\nTab 可将下方按钮滚入视口。\nCPU 工作完成整张图像后才替换。\n窗口大小改变仍走普通 Flex。",font("Noto Sans CJK SC"),16.0,32.0).unwrap();
            let text = GlyphRasterizer::new(cx.text_system().clone())
                .prepare(paragraph, scale)
                .unwrap();
            if self.send(
                OwnerCommand::Viewport {
                    viewport,
                    scale,
                    text,
                },
                cx,
            ) {
                self.requested = Some(request);
            }
        }
        let Some(source) = self.source.clone() else {
            return self.shown.clone();
        };
        let viewport = self.source_viewport;
        let scale = self.source_scale;
        let size = viewport.map(|v| (v * scale).ceil().max(1.0) as u32);
        if self.cpu {
            if let Some(ready) = self.ready.take() {
                return Some(ready);
            }
            if self.cpu_task.is_some() {
                return self.shown.clone();
            }
        }
        #[cfg(feature = "retained-wgpu")]
        if !self.cpu {
            #[cfg(target_os = "windows")]
            match self.gpu.as_mut().unwrap().ensure_window_device(window) {
                Ok(true) => {
                    self.last_requested = None;
                    self.bind_completion(cx);
                }
                Ok(false) => {}
                Err(e) => {
                    self.error = Some(e.to_string());
                    return self.shown.clone();
                }
            }
            match self.gpu.as_mut().unwrap().poll_frame_capacity() {
                Ok(c) if c.available() => {}
                Ok(_) => return self.shown.clone(),
                Err(e) => {
                    self.error = Some(e.to_string());
                    return self.shown.clone();
                }
            }
        }
        let timeline = self.timeline.as_mut().unwrap();
        let output = match timeline.sample(Instant::now()) {
            Ok(o) => o,
            Err(e) => {
                self.error = Some(e.to_string());
                return self.shown.clone();
            }
        };
        let scene = output.input_scene(&source).unwrap();
        let frame = output.frame().clone();
        if timeline.is_animating() && !self.cpu {
            window.request_animation_frame();
        }
        timeline.drain_notices().for_each(drop);
        let key = (scene.id(), size, scale.to_bits());
        if self.last_requested == Some(key) {
            if self.cpu && timeline.is_animating() {
                window.request_animation_frame();
            }
            return self.shown.clone();
        }
        if self.cpu {
            self.last_requested = Some(key);
            let pixels_frame = frame.clone();
            let work = cx.background_executor().spawn(async move {
                #[cfg(feature = "native-check")]
                let work_started = Instant::now();
                let mut job = SoftwareJob::new(
                    pixels_frame.resolve(),
                    SoftwareOptions {
                        size,
                        scale,
                        background: 0,
                        effects: pacing::SoftwareEffects::Exact,
                        max_bytes: 128 * 1024 * 1024,
                    },
                )?;
                while !job.step(8192)? {}
                #[cfg(feature = "native-check")]
                eprintln!(
                    "split_cpu_raster: revision={},size={}x{},elapsed_us={},work_units={},raster_samples={}",
                    pixels_frame.revision(),
                    size[0],
                    size[1],
                    work_started.elapsed().as_micros(),
                    job.stats().work_units,
                    job.stats().raster_samples
                );
                let mut bytes = job.into_frame()?.pixels().to_vec();
                for p in bytes.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                }
                Ok::<_, software::SoftwareError>(Arc::new(RenderImage::new(vec![
                    image::Frame::new(image::RgbaImage::from_raw(size[0], size[1], bytes).unwrap()),
                ])))
            });
            self.cpu_task = Some(cx.spawn(async move |view, cx| {
                let result = work.await;
                let _ = view.update(cx, |this, cx| {
                    this.cpu_task = None;
                    match result {
                        Ok(image) => {
                            this.ready = Some(Picture {
                                raster: Raster::Image(image),
                                scene,
                                size,
                                scale,
                                viewport,
                            })
                        }
                        Err(e) => this.error = Some(format!("CPU: {e:?}")),
                    }
                    cx.notify();
                });
            }));
            return self.shown.clone();
        }
        #[cfg(feature = "retained-wgpu")]
        {
            let gpu = self.gpu.as_mut().unwrap();
            if let Err(e) = gpu.submit_local_for_window(window, &frame, size, scale) {
                self.error = Some(e.to_string());
                return self.shown.clone();
            }
            #[cfg(target_os = "windows")]
            let raster = match gpu.window_surface() {
                Ok(Some(surface)) => Raster::Gpu(surface),
                _ => match gpu.window_image() {
                    Ok(image) => Raster::Image(image),
                    Err(e) => {
                        self.error = Some(e.to_string());
                        return self.shown.clone();
                    }
                },
            };
            #[cfg(not(target_os = "windows"))]
            let raster = match gpu.window_image() {
                Ok(image) => Raster::Image(image),
                Err(e) => {
                    self.error = Some(e.to_string());
                    return self.shown.clone();
                }
            };
            self.last_requested = Some(key);
            Some(Picture {
                raster,
                scene,
                size,
                scale,
                viewport,
            })
        }
        #[cfg(not(feature = "retained-wgpu"))]
        self.shown.clone()
    }
    fn installed(&mut self, picture: Picture, window: &mut Window, cx: &mut Context<Self>) {
        let same = self.shown.as_ref().is_some_and(|old| {
            old.scene.id() == picture.scene.id()
                && old.size == picture.size
                && old.scale == picture.scale
        });
        if same && self.router.scene_id() == Some(picture.scene.id()) {
            return;
        }
        if !same {
            if let Some(Picture {
                raster: Raster::Image(old),
                ..
            }) = self.shown.take()
            {
                window.drop_image(old).ok();
            }
            self.installs += 1;
            #[cfg(feature = "native-check")]
            if self.probe.stalled.load(Ordering::SeqCst) && self.installed_during_stall.len() < 4096
            {
                let translation = self
                    .animated
                    .and_then(|n| picture.scene.frame().resolve_geometry(n))
                    .map_or([0.0; 2], |p| p.spatial.translation);
                self.installed_during_stall.push((
                    Instant::now(),
                    picture.scene.frame().revision(),
                    translation,
                ));
            }
        }
        let mut candidate = self.router.clone();
        let result = candidate.adopt(picture.scene.clone(), UiInputEvidence::ImageInstalled);
        self.shown = Some(picture);
        match result {
            Ok(effects) => {
                if self.effects(effects, cx) {
                    self.router = candidate;
                }
            }
            Err(e) => self.error = Some(format!("adopt: {e:?}")),
        }
        if self.cpu
            && self
                .timeline
                .as_ref()
                .is_some_and(AnimationRuntime::is_animating)
        {
            window.request_animation_frame();
        }
    }
}
impl Render for SplitDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let paint = view.clone();
        let caption = format!(
            "{} · count {} · owner turns {} · installed {} · outstanding {} · owner stalled {}{}",
            if self.cpu { "CPU worker" } else { "GPU" },
            self.count,
            self.owner_turn,
            self.installs,
            self.outstanding,
            self.probe
                .stalled
                .load(std::sync::atomic::Ordering::Relaxed),
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
                    .child("Split runtime · 组件 / Flex 在独立 owner 线程")
                    .child(caption)
                    .child(
                        div()
                            .id("stall-owner")
                            .cursor_pointer()
                            .child("诊断：阻塞 UI owner 500ms")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.send(OwnerCommand::Stall { millis: 500 }, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("split-runtime-host")
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                        if let Some(input) = GpuiInputAdapter::key_down(e) {
                            this.input(input, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .on_key_up(cx.listener(|this, e: &KeyUpEvent, _, cx| {
                        if let Some(input) = GpuiInputAdapter::key_up(e) {
                            this.input(input, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        canvas(
                            move |b, w, cx| view.update(cx, |this, cx| this.prepare(b, w, cx)),
                            move |b, picture, w, cx| {
                                let e = paint.clone();
                                w.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        e.update(cx, |this, cx| {
                                            if let Some(a) = this.adapter() {
                                                let input = a.mouse_move(
                                                    event,
                                                    this.router.captured().is_some(),
                                                );
                                                this.input(input, cx);
                                            }
                                        });
                                    }
                                });
                                let e = paint.clone();
                                w.on_mouse_event(move |event: &MouseDownEvent, phase, w, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        e.update(cx, |this, cx| {
                                            if let Some(input) =
                                                this.adapter().and_then(|a| a.mouse_down(event))
                                            {
                                                w.focus(&this.focus, cx);
                                                this.input(input, cx);
                                                cx.stop_propagation();
                                            }
                                        });
                                    }
                                });
                                let e = paint.clone();
                                w.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        e.update(cx, |this, cx| {
                                            if let Some(input) = this.adapter().and_then(|a| {
                                                a.mouse_up(event, this.router.captured().is_some())
                                            }) {
                                                this.input(input, cx);
                                                cx.stop_propagation();
                                            }
                                        });
                                    }
                                });
                                let e = paint.clone();
                                w.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        e.update(cx, |this, cx| {
                                            this.input(UiInput::PointerLeave, cx)
                                        });
                                    }
                                });
                                let e = paint.clone();
                                w.on_mouse_event(move |event: &ScrollEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        e.update(cx, |this, cx| {
                                            if let Some(input) =
                                                this.adapter().and_then(|a| a.wheel(event))
                                            {
                                                this.input(input, cx);
                                                cx.stop_propagation();
                                            }
                                        });
                                    }
                                });
                                if let Some(picture) = picture {
                                    let target = Bounds::new(
                                        b.origin,
                                        size(px(picture.viewport[0]), px(picture.viewport[1])),
                                    );
                                    w.with_content_mask(Some(ContentMask { bounds: b }), |w| {
                                        match &picture.raster {
                                            Raster::Image(image) => w
                                                .paint_image(
                                                    b,
                                                    target,
                                                    Corners::default(),
                                                    image.clone(),
                                                    0,
                                                    false,
                                                )
                                                .unwrap(),
                                            #[cfg(all(
                                                feature = "retained-wgpu",
                                                target_os = "windows"
                                            ))]
                                            Raster::Gpu(surface) => {
                                                w.paint_gpu_surface(target, surface.clone())
                                            }
                                        }
                                    });
                                    paint.update(cx, |this, cx| this.installed(picture, w, cx));
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
        |w, cx| {
            w.set_window_title("gpui-box split owner / presentation");
            cx.new(|cx| SplitDemo::new(w, cx, cpu))
        },
    )
    .unwrap();
    cx.activate(true);
}
