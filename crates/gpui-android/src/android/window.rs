use super::{CallbackSlot, bridge::post};
use crate::lifecycle::SurfaceSession;
use gpui::*;
use gpui_wgpu::{GpuContext, WgpuContext, WgpuRenderer, WgpuSurfaceConfig};
use ndk::native_window::NativeWindow;
use raw_window_handle::WindowHandle;
use raw_window_handle::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

#[derive(Clone, Debug)]
pub(super) struct NativeSurface(pub NativeWindow);

#[derive(Debug)]
struct AndroidDisplay;

impl HasDisplayHandle for AndroidDisplay {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}
impl HasWindowHandle for NativeSurface {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let handle = AndroidNdkWindowHandle::new(self.0.ptr().cast());
        // SAFETY: NativeWindow owns an acquired reference for this borrow.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(handle)) })
    }
}
impl HasDisplayHandle for NativeSurface {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

type ResizeCallback = Box<dyn FnMut(Size<Pixels>, f32)>;

pub(super) struct WindowState {
    pub session: RefCell<SurfaceSession>,
    pub surface: RefCell<Option<NativeSurface>>,
    pub renderer: RefCell<WgpuRenderer>,
    pub gpu: GpuContext,
    pub size: Cell<Size<Pixels>>,
    pub scale: Cell<f32>,
    pub insets: RefCell<WindowInsets>,
    pub insets_changed: CallbackSlot<Box<dyn FnMut(WindowInsets)>>,
    pub handle: Cell<Option<AnyWindowHandle>>,
    pub visible: Cell<bool>,
    pub appearance: Cell<WindowAppearance>,
    pub appearance_changed: CallbackSlot<Box<dyn FnMut()>>,
    pub input_handler: CallbackSlot<PlatformInputHandler>,
    input_focus: Cell<Option<FocusId>>,
    pub input_epoch: Cell<u64>,
    pub accessibility: RefCell<super::accessibility::Accessibility>,
    pub text_configuration: RefCell<TextInputConfiguration>,
    pub back_enabled: Cell<bool>,
    pub back_gesture: RefCell<Option<(BackGestureEvent, bool)>>,
    pub back: CallbackSlot<Box<dyn FnMut()>>,
    pub request_frame: CallbackSlot<Box<dyn FnMut(RequestFrameOptions)>>,
    pub input: CallbackSlot<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    pub resized: CallbackSlot<ResizeCallback>,
    pub active: CallbackSlot<Box<dyn FnMut(bool)>>,
    pub visibility: CallbackSlot<Box<dyn FnMut(WindowVisibility)>>,
}

impl Drop for WindowState {
    fn drop(&mut self) {
        self.renderer.get_mut().destroy();
        self.surface.get_mut().take();
    }
}

impl std::fmt::Debug for WindowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidDisplay")
            .field("size", &self.size.get())
            .finish()
    }
}

impl WindowState {
    pub fn new(
        surface: NativeSurface,
        generation: u64,
        width: i32,
        height: i32,
        scale: f32,
    ) -> anyhow::Result<Rc<Self>> {
        // WGPU retains its InstanceDescriptor display provider. Supply a
        // display-only object so the first ANativeWindow cannot be retained by
        // the process-wide GPU context after surfaceDestroyed.
        let instance = WgpuContext::instance(Some(Box::new(AndroidDisplay)));
        let target = gpui_wgpu::wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(DisplayHandle::android().as_raw()),
            raw_window_handle: surface.window_handle()?.as_raw(),
        };
        // SAFETY: surface owns the native reference until this temporary WGPU
        // surface has been dropped. Both are used only on Android's main thread.
        let initial_surface = unsafe { instance.create_surface_unsafe(target)? };
        let context = WgpuContext::new(instance, &initial_surface, &config(width, height), None)?;
        drop(initial_surface);
        let gpu = Rc::new(RefCell::new(Some(context)));
        let renderer = WgpuRenderer::new(gpu.clone(), &surface, config(width, height), None)?;
        let mut session = SurfaceSession::default();
        session.attach(generation);
        log::info!("Android renderer: {:?}", renderer.gpu_specs());
        Ok(Rc::new(Self {
            session: RefCell::new(session),
            surface: RefCell::new(Some(surface)),
            renderer: RefCell::new(renderer),
            gpu,
            size: Cell::new(size(px(width as f32 / scale), px(height as f32 / scale))),
            scale: Cell::new(scale),
            insets: RefCell::default(),
            insets_changed: CallbackSlot::default(),
            handle: Cell::new(None),
            visible: Cell::new(true),
            appearance: Cell::new(WindowAppearance::Dark),
            appearance_changed: CallbackSlot::default(),
            input_handler: CallbackSlot::default(),
            input_focus: Cell::new(None),
            input_epoch: Cell::new(1),
            accessibility: RefCell::default(),
            text_configuration: RefCell::default(),
            back_enabled: Cell::new(false),
            back_gesture: RefCell::default(),
            back: CallbackSlot::default(),
            request_frame: CallbackSlot::default(),
            input: CallbackSlot::default(),
            resized: CallbackSlot::default(),
            active: CallbackSlot::default(),
            visibility: CallbackSlot::default(),
        }))
    }
    pub fn attach(
        &self,
        surface: NativeSurface,
        generation: u64,
        width: i32,
        height: i32,
        scale: f32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.session.borrow_mut().attach(generation),
            "Stale surface generation {generation}"
        );
        self.renderer.borrow_mut().unconfigure_surface();
        self.surface.borrow_mut().take();
        let instance = self
            .gpu
            .borrow()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing GPU context"))?
            .instance
            .clone();
        let result =
            self.renderer
                .borrow_mut()
                .replace_surface(&surface, config(width, height), &instance);
        if let Err(error) = result {
            self.session.borrow_mut().detach(generation);
            self.set_visible(false);
            return Err(error);
        }
        self.surface.replace(Some(surface));
        self.scale.set(scale);
        self.size
            .set(size(px(width as f32 / scale), px(height as f32 / scale)));
        self.resized.with(|cb| cb(self.size.get(), scale));
        self.set_visible(true);
        Ok(())
    }
    pub fn detach(&self, generation: u64) {
        if !self.session.borrow_mut().detach(generation) {
            return;
        }
        // Synchronous barrier. Surface and in-flight GPU work are released before
        // Android is allowed to destroy its ANativeWindow.
        self.renderer.borrow_mut().unconfigure_surface();
        if let Some(gpu) = self.gpu.borrow().as_ref() {
            let _ = gpu
                .device
                .poll(gpui_wgpu::wgpu::PollType::wait_indefinitely());
        }
        self.surface.borrow_mut().take();
        self.set_visible(false);
    }
    pub fn resize(&self, generation: u64, width: i32, height: i32, scale: f32) {
        if !self.session.borrow().accepts(generation)
            || width <= 0
            || height <= 0
            || !scale.is_finite()
            || scale <= 0.0
        {
            return;
        }
        self.renderer
            .borrow_mut()
            .update_drawable_size(config(width, height).size);
        self.scale.set(scale);
        self.size
            .set(size(px(width as f32 / scale), px(height as f32 / scale)));
        self.resized.with(|cb| cb(self.size.get(), scale));
        post(0);
    }
    pub fn set_visible(&self, visible: bool) {
        if self.visible.replace(visible) != visible {
            if !visible {
                self.invalidate_input();
            }
            self.visibility.with(|cb| {
                cb(if visible {
                    WindowVisibility::Visible
                } else {
                    WindowVisibility::Hidden
                })
            });
            self.active.with(|cb| cb(visible));
        }
    }
    pub fn invalidate_input(&self) {
        self.input_epoch
            .set(self.input_epoch.get().wrapping_add(1).max(1));
        post(5);
    }
    pub fn frame(&self, generation: u64) {
        if self.visible.get() && self.session.borrow().accepts(generation) {
            self.request_frame.with(|cb| {
                cb(RequestFrameOptions {
                    require_presentation: true,
                    ..Default::default()
                })
            });
        }
    }
}

fn config(width: i32, height: i32) -> WgpuSurfaceConfig {
    WgpuSurfaceConfig {
        size: size(DevicePixels(width.max(1)), DevicePixels(height.max(1))),
        transparent: false,
        preferred_present_mode: None,
    }
}

impl PlatformDisplay for WindowState {
    fn id(&self) -> DisplayId {
        DisplayId::new(1)
    }
    fn uuid(&self) -> anyhow::Result<uuid::Uuid> {
        Ok(uuid::Uuid::from_u128(1))
    }
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(Point::default(), self.size.get())
    }
    fn refresh_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(1.0 / 60.0))
    }
}

pub(super) struct AndroidWindow(pub Rc<WindowState>);
impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> std::result::Result<WindowHandle<'_>, HandleError> {
        let surface = self.0.surface.borrow();
        let handle = surface
            .as_ref()
            .ok_or(HandleError::Unavailable)?
            .window_handle()?
            .as_raw();
        // SAFETY: this main-thread window retains the surface until detach. No
        // native callback may run while the borrowed handle is in use.
        Ok(unsafe { WindowHandle::borrow_raw(handle) })
    }
}
impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> std::result::Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}
impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        PlatformDisplay::bounds(self.0.as_ref())
    }
    fn is_maximized(&self) -> bool {
        true
    }
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds())
    }
    fn content_size(&self) -> Size<Pixels> {
        self.0.size.get()
    }
    fn resize(&mut self, _: Size<Pixels>) {}
    fn scale_factor(&self) -> f32 {
        self.0.scale.get()
    }
    fn appearance(&self) -> WindowAppearance {
        self.0.appearance.get()
    }
    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.clone())
    }
    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }
    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }
    fn capslock(&self) -> Capslock {
        Capslock::default()
    }
    fn set_input_handler(&mut self, handler: PlatformInputHandler) {
        if self.0.input_focus.replace(handler.focus_id()) != handler.focus_id() {
            self.0.invalidate_input();
        }
        self.0.input_handler.set(handler);
    }
    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.input_handler.take()
    }
    fn prompt(
        &self,
        _: PromptLevel,
        _: &str,
        _: Option<&str>,
        _: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        None
    }
    fn activate(&self) {}
    fn is_active(&self) -> bool {
        self.0.visible.get()
    }
    fn visibility(&self) -> WindowVisibility {
        if self.0.visible.get() {
            WindowVisibility::Visible
        } else {
            WindowVisibility::Hidden
        }
    }
    fn is_hovered(&self) -> bool {
        false
    }
    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }
    fn set_title(&mut self, _: &str) {}
    fn set_background_appearance(&self, _: WindowBackgroundAppearance) {}
    fn minimize(&self) {}
    fn zoom(&self) {}
    fn toggle_fullscreen(&self) {}
    fn is_fullscreen(&self) -> bool {
        true
    }
    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        Some(Rc::new(|| post(0)))
    }
    fn on_request_frame(&self, cb: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.request_frame.set(cb);
        post(0);
    }
    fn on_input(&self, cb: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.input.set(cb);
    }
    fn on_active_status_change(&self, cb: Box<dyn FnMut(bool)>) {
        self.0.active.set(cb);
    }
    fn on_visibility_change(&self, cb: Box<dyn FnMut(WindowVisibility)>) {
        self.0.visibility.set(cb);
    }
    fn on_hover_status_change(&self, _: Box<dyn FnMut(bool)>) {}
    fn on_resize(&self, cb: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.resized.set(cb);
    }
    fn on_moved(&self, _: Box<dyn FnMut()>) {}
    fn on_display_changed(&self, _: Box<dyn FnMut()>) {}
    fn on_should_close(&self, _: Box<dyn FnMut() -> bool>) {}
    fn on_hit_test_window_control(&self, _: Box<dyn FnMut() -> Option<WindowControlArea>>) {}
    fn on_close(&self, _: Box<dyn FnOnce()>) {}
    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.appearance_changed.set(callback);
    }
    fn draw(&self, scene: &Scene) {
        if self.0.surface.borrow().is_some() {
            self.0.renderer.borrow_mut().draw(scene);
        }
    }
    fn schedule_frame(&self) {
        post(0);
    }
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.renderer.borrow().sprite_atlas().clone()
    }
    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }
    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.0.renderer.borrow().gpu_specs()
    }
    fn update_ime_position(&self, _: Bounds<Pixels>) {}
    fn show_soft_keyboard(&self) {
        post(1);
    }
    fn hide_soft_keyboard(&self) {
        post(2);
    }
    fn text_input_state_changed(&self, change: TextInputStateChange) {
        if change == TextInputStateChange::FocusLost {
            self.0.input_focus.set(None);
            self.0.invalidate_input();
        }
        post(4);
    }
    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        self.0.text_configuration.replace(configuration);
        self.0.invalidate_input();
    }
    fn a11y_init(&self, callbacks: A11yCallbacks) {
        self.0.accessibility.borrow_mut().callbacks = Some(Rc::new(callbacks));
        post(7);
    }
    fn a11y_tree_update(&self, update: accesskit::TreeUpdate) {
        self.0.accessibility.borrow_mut().update(update);
        post(7);
    }
    fn insets(&self) -> WindowInsets {
        self.0.insets.borrow().clone()
    }
    fn on_insets_changed(&self, callback: Box<dyn FnMut(WindowInsets)>) {
        self.0.insets_changed.set(callback);
    }
    fn set_back_handler(&self, callback: Box<dyn FnMut()>) {
        self.0.back.set(callback);
    }
    fn set_back_enabled(&self, enabled: bool) {
        self.0.back_enabled.set(enabled);
        post(6);
    }
}
