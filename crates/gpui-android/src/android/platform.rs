use super::{
    CallbackSlot,
    dispatcher::Dispatcher,
    window::{AndroidWindow, WindowState},
};
use futures::channel::oneshot;
use gpui::*;
use gpui_wgpu::CosmicTextSystem;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

pub struct AndroidPlatform {
    pub(super) window: Rc<WindowState>,
    background: BackgroundExecutor,
    foreground: ForegroundExecutor,
    text: Arc<dyn PlatformTextSystem>,
    pub(super) lifecycle: CallbackSlot<Box<dyn FnMut(AppLifecyclePhase)>>,
    pub(super) memory_warning: CallbackSlot<Box<dyn FnMut()>>,
    pub(super) open_urls: CallbackSlot<Box<dyn FnMut(Vec<String>)>>,
}

impl AndroidPlatform {
    pub(super) fn new(window: Rc<WindowState>, dispatcher: Arc<Dispatcher>) -> Self {
        Self {
            window,
            background: BackgroundExecutor::new(dispatcher.clone()),
            foreground: ForegroundExecutor::new(dispatcher),
            text: Arc::new(CosmicTextSystem::new_without_system_fonts(
                "Noto Sans CJK SC",
            )),
            lifecycle: CallbackSlot::default(),
            memory_warning: CallbackSlot::default(),
            open_urls: CallbackSlot::default(),
        }
    }
}

fn unavailable<T>() -> oneshot::Receiver<anyhow::Result<T>> {
    let (tx, rx) = oneshot::channel();
    let _ = tx.send(Err(anyhow::anyhow!(
        "This Android platform service is not implemented yet"
    )));
    rx
}

struct KeyboardLayout;
impl PlatformKeyboardLayout for KeyboardLayout {
    fn id(&self) -> &str {
        "android-ime"
    }
    fn name(&self) -> &str {
        "Android IME"
    }
}

impl Platform for AndroidPlatform {
    fn platform_info(&self) -> PlatformInfo {
        PlatformInfo {
            os: OperatingSystem::Android,
            target: PlatformTarget::Native,
        }
    }
    fn background_executor(&self) -> BackgroundExecutor {
        self.background.clone()
    }
    fn foreground_executor(&self) -> ForegroundExecutor {
        self.foreground.clone()
    }
    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text.clone()
    }
    fn run(&self, launched: Box<dyn FnOnce()>) {
        launched();
    }
    fn quit(&self) {
        super::bridge::post(3);
    }
    fn restart(&self, _: Option<PathBuf>, _: Vec<OsString>) {}
    fn activate(&self, _: bool) {}
    fn hide(&self) {}
    fn hide_other_apps(&self) {}
    fn unhide_other_apps(&self) {}
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![self.window.clone()]
    }
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.window.clone())
    }
    fn on_displays_changed(&self, _: Box<dyn FnMut()>) {}
    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.window.handle.get()
    }
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        _: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        anyhow::ensure!(
            self.window.handle.get().is_none(),
            "M0 Android supports one window"
        );
        self.window.handle.set(Some(handle));
        Ok(Box::new(AndroidWindow(self.window.clone())))
    }
    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::Dark
    }
    fn open_url(&self, url: &str) {
        super::bridge::open_url(url);
    }
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.open_urls.set(callback);
        super::bridge::post(0);
    }
    fn register_url_scheme(&self, _: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow::anyhow!(
            "Declare URL schemes in AndroidManifest.xml"
        )))
    }
    fn prompt_for_paths(
        &self,
        _: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        unavailable()
    }
    fn prompt_for_new_path(
        &self,
        _: &Path,
        _: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        unavailable()
    }
    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }
    fn reveal_path(&self, _: &Path) {}
    fn open_with_system(&self, _: &Path) {}
    fn on_quit(&self, _: Box<dyn FnMut() -> bool>) {}
    fn on_reopen(&self, _: Box<dyn FnMut()>) {}
    fn on_system_sleep(&self, _: Box<dyn FnMut()>) {}
    fn on_system_wake(&self, _: Box<dyn FnMut()>) {}
    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        self.lifecycle.set(callback);
    }
    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        self.memory_warning.set(callback);
    }
    fn set_menus(&self, _: Vec<Menu>, _: &Keymap) {}
    fn set_dock_menu(&self, _: Vec<MenuItem>, _: &Keymap) {}
    fn on_app_menu_action(&self, _: Box<dyn FnMut(&dyn Action)>) {}
    fn on_will_open_app_menu(&self, _: Box<dyn FnMut()>) {}
    fn on_validate_app_menu_command(&self, _: Box<dyn FnMut(&dyn Action) -> bool>) {}
    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }
    fn on_thermal_state_change(&self, _: Box<dyn FnMut()>) {}
    fn prevent_idle_sleep(&self, _: &str) -> Task<Result<ActivityGuard>> {
        Task::ready(Err(anyhow::anyhow!(
            "Idle sleep prevention is not implemented"
        )))
    }
    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow::anyhow!("Android APK has no executable directory"))
    }
    fn path_for_auxiliary_executable(&self, _: &str) -> Result<PathBuf> {
        self.app_path()
    }
    fn set_cursor_style(&self, _: CursorStyle) {}
    fn hide_cursor_until_mouse_moves(&self) {}
    fn is_cursor_visible(&self) -> bool {
        false
    }
    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        super::bridge::read_clipboard().map(ClipboardItem::new_string)
    }
    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let Some(text) = item.text() {
            super::bridge::write_clipboard(&text);
        }
    }
    fn write_credentials(&self, _: &str, _: &str, _: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow::anyhow!(
            "Android credential storage is not implemented"
        )))
    }
    fn read_credentials(&self, _: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Err(anyhow::anyhow!(
            "Android credential storage is not implemented"
        )))
    }
    fn delete_credentials(&self, _: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow::anyhow!(
            "Android credential storage is not implemented"
        )))
    }
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(KeyboardLayout)
    }
    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }
    fn on_keyboard_layout_change(&self, _: Box<dyn FnMut()>) {}
}
