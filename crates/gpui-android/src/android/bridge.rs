use super::{
    dispatcher::Dispatcher,
    platform::AndroidPlatform,
    window::{NativeSurface, WindowState},
};
use gpui::*;
use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JClass, JObject, JString, JValue},
};
use ndk::native_window::NativeWindow;
use std::{cell::RefCell, rc::Rc, sync::OnceLock};

struct Host {
    vm: JavaVM,
    class: GlobalRef,
}
static HOST: OnceLock<Host> = OnceLock::new();
struct Runtime {
    platform: Rc<AndroidPlatform>,
    _application: ApplicationHandle,
    queue: RefCell<PriorityQueueReceiver<RunnableVariant>>,
    delivered_urls: RefCell<std::collections::VecDeque<String>>,
}
type ActivationCallback = Box<dyn FnMut(&str, &str) -> bool>;
thread_local! {
    static RUNTIME: RefCell<Option<Rc<Runtime>>> = const { RefCell::new(None) };
    static LAUNCH: RefCell<Option<fn(&mut App)>> = const { RefCell::new(None) };
    static ACTIVATION: super::CallbackSlot<ActivationCallback> = super::CallbackSlot::default();
    static SAVE_STATE: super::CallbackSlot<Box<dyn FnMut() -> Vec<u8>>> = super::CallbackSlot::default();
    static RESTORED_STATE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Register application-owned versioned UI state. Android may reclaim a stopped
/// process, so persist durable business data separately through private storage.
/// The host bounds this transient Activity state to 128 KiB.
pub fn on_saved_state(save: impl FnMut() -> Vec<u8> + 'static) {
    SAVE_STATE.with(|slot| slot.set(Box::new(save)));
}

/// Consume state through the launch callback's App context after constructing
/// the application model, preserving GPUI's existing mutable-borrow ownership.
pub fn take_saved_state() -> Option<Vec<u8>> {
    RESTORED_STATE.with(|slot| slot.borrow_mut().take())
}

pub fn save_state() -> Vec<u8> {
    let value = SAVE_STATE
        .with(|slot| slot.with(|save| save()))
        .unwrap_or_default();
    if value.len() > 128 * 1024 {
        log::warn!("Application UI snapshot exceeds the 128 KiB Activity limit");
        Vec::new()
    } else {
        value
    }
}

pub fn restore_state(value: Vec<u8>) {
    // Activity recreation retains the authoritative live Rust model. A saved
    // Bundle is consumed only when a new process has not launched its model yet.
    if runtime().is_none() && value.len() <= 128 * 1024 {
        RESTORED_STATE.with(|slot| slot.replace(Some(value)));
    }
}

/// Install an application-owned activation handoff. Return true only after
/// accepting the stable ID and URL into the application's routing workflow.
/// Persist on a worker using gpui-activation when durable delivery is required.
pub fn on_activation(callback: impl FnMut(&str, &str) -> bool + 'static) {
    ACTIVATION.with(|slot| slot.set(Box::new(callback)));
    post(0);
}

/// Installs the Java bridge on Android's main thread. The class must provide
/// static request(int), which posts work instead of reentering Rust.
pub fn initialize(env: &mut JNIEnv, class: &JClass, launch: fn(&mut App)) -> anyhow::Result<()> {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("gpui-box")
            .with_max_level(log::LevelFilter::Info),
    );
    if HOST.get().is_none() {
        let host = Host {
            vm: env.get_java_vm()?,
            class: env.new_global_ref(class)?,
        };
        let _ = HOST.set(host);
    }
    LAUNCH.with(|slot| slot.replace(Some(launch)));
    Ok(())
}

pub(super) fn post(command: i32) {
    let Some(host) = HOST.get() else {
        return;
    };
    let result = (|| -> jni::errors::Result<()> {
        let mut env = host.vm.attach_current_thread()?;
        let class: &JClass = host.class.as_obj().into();
        env.call_static_method(class, "request", "(I)V", &[JValue::Int(command)])?;
        Ok(())
    })();
    if let Err(error) = result {
        log::error!("Android host request failed: {error}");
    }
}

pub(super) fn read_clipboard() -> Option<String> {
    let host = HOST.get()?;
    let result = (|| -> jni::errors::Result<Option<String>> {
        let mut env = host.vm.attach_current_thread()?;
        let class: &JClass = host.class.as_obj().into();
        let result = env.call_static_method(class, "readClipboard", "()Ljava/lang/String;", &[]);
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        let value = result?.l()?;
        if value.is_null() {
            return Ok(None);
        }
        let value = JString::from(value);
        Ok(Some(env.get_string(&value)?.into()))
    })();
    match result {
        Ok(value) => value,
        Err(error) => {
            log::error!("Android clipboard read failed: {error}");
            None
        }
    }
}

pub(super) fn write_clipboard(text: &str) {
    let Some(host) = HOST.get() else { return };
    let result = (|| -> jni::errors::Result<()> {
        let mut env = host.vm.attach_current_thread()?;
        let class: &JClass = host.class.as_obj().into();
        let text = env.new_string(text)?;
        let result = env.call_static_method(
            class,
            "writeClipboard",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&text)],
        );
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        result?;
        Ok(())
    })();
    if let Err(error) = result {
        log::error!("Android clipboard write failed: {error}");
    }
}

fn runtime() -> Option<Rc<Runtime>> {
    RUNTIME.with(|slot| slot.borrow().clone())
}
pub(super) fn window(generation: u64) -> Option<Rc<WindowState>> {
    let window = runtime()?.platform.window.clone();
    let valid = window.session.borrow().accepts(generation);
    valid.then_some(window)
}

/// Acquires an ANativeWindow reference. All entry points must run on the system
/// main Looper, including the synchronous surface-destruction barrier.
pub fn attach(
    env: &mut JNIEnv,
    surface: &JObject,
    generation: u64,
    width: i32,
    height: i32,
    scale: f32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        generation > 0 && width > 0 && height > 0 && scale.is_finite() && scale > 0.0,
        "Invalid surface geometry"
    );
    // SAFETY: Surface is a live android.view.Surface local reference and env is
    // valid for this thread. NativeWindow acquires and releases its own ref.
    let native =
        unsafe { NativeWindow::from_surface(env.get_native_interface(), surface.as_raw()) }
            .ok_or_else(|| anyhow::anyhow!("ANativeWindow_fromSurface failed"))?;
    let surface = NativeSurface(native);
    if let Some(runtime) = runtime() {
        runtime
            .platform
            .window
            .attach(surface, generation, width, height, scale)?;
    } else {
        let window = WindowState::new(surface, generation, width, height, scale)?;
        let (dispatcher, queue) = Dispatcher::new();
        let platform = Rc::new(AndroidPlatform::new(window, dispatcher));
        let launch = LAUNCH
            .with(|slot| *slot.borrow())
            .ok_or_else(|| anyhow::anyhow!("initialize must precede attach"))?;
        let application = Application::with_platform(platform.clone()).run_embedded(move |cx| {
            gpui_lifecycle::gpui_adapter::install_lifecycle_service(super::lifecycle_service(), cx)
                .expect("install process-owned Android lifecycle service");
            launch(cx);
        });
        RUNTIME.with(|slot| {
            slot.replace(Some(Rc::new(Runtime {
                platform,
                _application: application,
                queue: RefCell::new(queue),
                delivered_urls: RefCell::default(),
            })))
        });
    }
    super::host::attach(generation);
    log::info!("Surface attached generation={generation} {width}x{height} density={scale}");
    post(0);
    Ok(())
}

pub fn detach(generation: u64) {
    back_gesture(generation, 3, 0., 0., 0.);
    if let Some(runtime) = runtime() {
        runtime.platform.window.detach(generation);
    }
    super::host::detach(generation);
    log::info!("Surface detached generation={generation}");
}

pub fn resize(generation: u64, width: i32, height: i32, scale: f32) {
    if let Some(window) = window(generation) {
        window.resize(generation, width, height, scale);
    }
}

pub fn frame(generation: u64) {
    let Some(runtime) = runtime() else {
        return;
    };
    // Bound work per frame so a self-waking future cannot starve Android.
    for _ in 0..256 {
        let task = runtime.queue.borrow_mut().try_pop().ok().flatten();
        let Some(task) = task else {
            break;
        };
        task.run();
    }
    if !runtime.queue.borrow().is_empty() {
        post(0);
    }
    runtime.platform.window.frame(generation);
}

pub fn lifecycle(generation: u64, resumed: bool) {
    if !resumed {
        back_gesture(generation, 3, 0., 0., 0.);
    }
    if let Some(window) = window(generation) {
        window.set_visible(resumed);
        if let Some(runtime) = runtime() {
            runtime.platform.lifecycle.with(|cb| {
                cb(if resumed {
                    AppLifecyclePhase::Active
                } else {
                    AppLifecyclePhase::Background
                })
            });
        }
        if resumed {
            post(0);
        }
    }
}

/// Returns zero when the current window has no eligible focused editor.
pub fn input_session(generation: u64) -> u64 {
    let Some(window) = window(generation).filter(|w| w.visible.get()) else {
        return 0;
    };
    window
        .input_handler
        .with(|h| {
            if h.query_accepts_focused_text_input() {
                window.input_epoch.get()
            } else {
                0
            }
        })
        .unwrap_or(0)
}

pub fn back_enabled(generation: u64) -> bool {
    window(generation).is_some_and(|w| w.visible.get())
}

pub fn back(generation: u64) -> bool {
    let Some(window) = window(generation).filter(|w| w.visible.get()) else {
        return false;
    };
    let handled = if window.back_enabled.get() {
        window.back.with(|callback| callback()).is_some()
    } else {
        // Android's committed button/gesture can also use GPUI's portable
        // back listeners. Progress animation is a separate host capability.
        let event = BackGestureEvent {
            phase: BackGesturePhase::Started,
            start_position: point(window.size.get().width / 2., window.size.get().height / 2.),
            progress: 0.,
        };
        let claimed = window
            .input
            .with(|callback| callback(PlatformInput::BackGesture(event.clone())))
            .is_some_and(|result| result.default_prevented);
        if claimed {
            window.input.with(|callback| {
                callback(PlatformInput::BackGesture(BackGestureEvent {
                    phase: BackGesturePhase::Committed,
                    progress: 1.,
                    ..event
                }))
            });
        }
        claimed
    };
    post(0);
    handled
}

/// Predictive gestures keep their original claim and coordinates through completion.
pub fn back_gesture(generation: u64, phase: i32, x: f32, y: f32, progress: f32) -> bool {
    let Some(window) = window(generation).filter(|w| w.visible.get()) else {
        return false;
    };
    if !x.is_finite() || !y.is_finite() || !progress.is_finite() {
        return false;
    }
    if phase == 0 {
        back_gesture(generation, 3, x, y, 0.);
        let event = BackGestureEvent {
            phase: BackGesturePhase::Started,
            start_position: point(px(x / window.scale.get()), px(y / window.scale.get())),
            progress: 0.,
        };
        let platform_claim = window.back_enabled.get();
        let claimed = platform_claim
            || window
                .input
                .with(|callback| callback(PlatformInput::BackGesture(event.clone())))
                .is_some_and(|result| result.default_prevented);
        if claimed {
            window.back_gesture.replace(Some((event, platform_claim)));
        }
        post(0);
        return claimed;
    }
    if !(1..=3).contains(&phase) {
        return false;
    }
    let Some((mut event, platform_claim)) = window.back_gesture.borrow().clone() else {
        return false;
    };
    event.phase = match phase {
        1 => BackGesturePhase::Progressed,
        2 => BackGesturePhase::Committed,
        _ => BackGesturePhase::Cancelled,
    };
    event.progress = if phase == 2 {
        1.
    } else {
        progress.clamp(0., 1.)
    };
    if phase != 1 {
        window.back_gesture.take();
    }
    if platform_claim {
        if phase == 2 {
            window.back.with(|callback| callback());
        }
    } else {
        window
            .input
            .with(|callback| callback(PlatformInput::BackGesture(event)));
    }
    post(0);
    true
}

pub fn memory_warning() {
    super::host::memory_warning();
    if let Some(runtime) = runtime() {
        runtime.platform.memory_warning.with(|callback| callback());
        post(0);
    }
}

/// Best-effort GPUI URL handoff. Android reports only whether startActivity accepted it.
pub(super) fn open_url(url: &str) {
    let Some(host) = HOST.get() else {
        return;
    };
    let result = (|| -> jni::errors::Result<bool> {
        let mut env = host.vm.attach_current_thread()?;
        let class: &JClass = host.class.as_obj().into();
        let value = env.new_string(url)?;
        env.call_static_method(
            class,
            "openUrl",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&value)],
        )?
        .z()
    })();
    if !matches!(result, Ok(true)) {
        log::warn!("Android URL handoff was unavailable");
    }
}

/// Delivery requires a live foreground surface and an application URL listener.
/// The bounded in-process deduplication complements the application durable inbox.
pub fn open_url_event(generation: u64, id: &str, url: &str) -> bool {
    if window(generation).is_none_or(|window| !window.visible.get())
        || id.is_empty()
        || id.len() > 128
        || url.len() > 8192
        || url.chars().any(char::is_control)
    {
        return false;
    }
    let Some(runtime) = runtime() else {
        return false;
    };
    if runtime
        .delivered_urls
        .borrow()
        .iter()
        .any(|delivered| delivered == id)
    {
        return true;
    }
    let accepted = ACTIVATION.with(|slot| slot.with(|callback| callback(id, url)));
    if !accepted.unwrap_or_else(|| {
        runtime
            .platform
            .open_urls
            .with(|callback| callback(vec![url.to_owned()]))
            .is_some()
    }) {
        return false;
    }
    let mut delivered = runtime.delivered_urls.borrow_mut();
    delivered.push_back(id.to_owned());
    if delivered.len() > 256 {
        delivered.pop_front();
    }
    true
}

/// Hardware navigation and shortcuts use GPUI's normal key dispatch path.
pub fn key(generation: u64, key: &str, modifiers: i32, down: bool, held: bool) -> bool {
    let Some(window) = window(generation).filter(|w| w.visible.get()) else {
        return false;
    };
    super::host::input();
    let keystroke = Keystroke {
        key: key.into(),
        modifiers: Modifiers {
            shift: modifiers & 1 != 0,
            control: modifiers & 2 != 0,
            alt: modifiers & 4 != 0,
            platform: modifiers & 8 != 0,
            ..Default::default()
        },
        key_char: None,
    };
    let event = if down {
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: held,
            prefer_character_input: false,
        })
    } else {
        PlatformInput::KeyUp(KeyUpEvent { keystroke })
    };
    let handled = window.input.with(|callback| callback(event)).is_some();
    post(0);
    handled
}

/// Android context-menu commands share the platform clipboard implementation
/// used by ordinary GPUI copy/paste actions. Operations are select-all/copy/cut/paste.
pub fn clipboard_action(generation: u64, session: u64, operation: i32) -> bool {
    if session == 0 || input_session(generation) != session {
        return false;
    }
    let Some(window) = window(generation) else {
        return false;
    };
    let paste = if operation == 3 {
        read_clipboard()
    } else {
        None
    };
    let accepted = window
        .input_handler
        .with(|handler| {
            let Some(selection) = handler.selected_text_range(true) else {
                return false;
            };
            match operation {
                0 => {
                    let length = handler.text_length_utf16().unwrap_or(0);
                    handler.set_selected_text_range(0..length);
                }
                1 | 2 => {
                    if selection.range.is_empty() {
                        return false;
                    }
                    let Some(text) = handler.text_for_range(selection.range.clone(), &mut None)
                    else {
                        return false;
                    };
                    write_clipboard(&text);
                    if operation == 2 {
                        handler.replace_text_in_range(Some(selection.range), "");
                    }
                }
                3 => {
                    let Some(text) = paste else { return false };
                    handler.replace_text_in_range(Some(selection.range), &text);
                }
                _ => return false,
            }
            true
        })
        .unwrap_or(false);
    post(4);
    post(0);
    accepted
}

pub fn touch(generation: u64, id: u64, phase: i32, x: f32, y: f32) {
    let Some(window) = window(generation) else {
        return;
    };
    if window.visible.get() && phase == 0 {
        super::host::input();
    }
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Cancelled,
    };
    let event = TouchEvent {
        id: TouchId(id),
        phase,
        position: point(px(x / window.scale.get()), px(y / window.scale.get())),
        ..Default::default()
    };
    window.input.with(|cb| cb(PlatformInput::Touch(event)));
    post(0);
}

pub fn text(generation: u64) -> String {
    window(generation)
        .and_then(|w| {
            w.input_handler.with(|h| {
                let length = h.text_length_utf16().unwrap_or(0);
                h.text_for_range(0..length, &mut None).unwrap_or_default()
            })
        })
        .unwrap_or_default()
}

pub fn text_state(generation: u64) -> [i32; 4] {
    window(generation)
        .and_then(|w| {
            w.input_handler.with(|h| {
                let selection = h.selected_text_range(true).map(|s| s.range).unwrap_or(0..0);
                let marked = h.marked_text_range();
                [
                    selection.start as i32,
                    selection.end as i32,
                    marked.as_ref().map_or(-1, |r| r.start as i32),
                    marked.as_ref().map_or(-1, |r| r.end as i32),
                ]
            })
        })
        .unwrap_or([0, 0, -1, -1])
}

/// Caret geometry uses physical pixels relative to the fitted SurfaceView.
pub fn caret_bounds(generation: u64, session: u64) -> Option<[f32; 4]> {
    if session == 0 || input_session(generation) != session {
        return None;
    }
    let window = window(generation)?;
    window
        .input_handler
        .with(|handler| {
            let selection = handler.selected_text_range(true)?;
            let cursor = if selection.reversed {
                selection.range.start
            } else {
                selection.range.end
            };
            let bounds = handler.bounds_for_range(cursor..cursor)?;
            let scale = window.scale.get();
            Some([
                f32::from(bounds.origin.x) * scale,
                f32::from(bounds.origin.y) * scale,
                f32::from(bounds.size.width) * scale,
                f32::from(bounds.size.height) * scale,
            ])
        })
        .flatten()
}

/// IME offsets and cursor arguments use UTF-16 code units, as Android does.
/// Operations are commit, compose, finish, selection, delete, composing-region.
pub fn edit(generation: u64, session: u64, operation: i32, value: &str, a: i32, b: i32) -> bool {
    if !(0..=5).contains(&operation) || session == 0 || input_session(generation) != session {
        return false;
    }
    let Some(window) = window(generation) else {
        return false;
    };
    window.input_handler.with(|h| {
        let Some(selection) = h.selected_text_range(true) else {
            return;
        };
        match operation {
            0 | 1 => {
                let range = h.marked_text_range().unwrap_or(selection.range);
                let start = range.start;
                let inserted = value.encode_utf16().count();
                if operation == 1 {
                    h.replace_and_mark_text_in_range(Some(range), value, None);
                } else {
                    h.replace_text_in_range(Some(range), value);
                }
                let cursor = if a > 0 {
                    (start + inserted) as i64 + a as i64 - 1
                } else {
                    start as i64 + a as i64
                };
                let length = h.text_length_utf16().unwrap_or(0);
                let cursor = cursor.clamp(0, length as i64) as usize;
                h.set_selected_text_range(cursor..cursor);
            }
            2 => h.unmark_text(),
            3 => {
                let length = h.text_length_utf16().unwrap_or(0);
                let start = (a.max(0) as usize).min(length);
                let end = (b.max(0) as usize).min(length);
                h.set_selected_text_range(start.min(end)..start.max(end));
            }
            4 => {
                let length = h.text_length_utf16().unwrap_or(0);
                // Delete surrounding text while preserving the selected text.
                let range = selection.range;
                let end = range.end.saturating_add(b.max(0) as usize).min(length);
                h.replace_text_in_range(Some(range.end..end), "");
                let start = range.start.saturating_sub(a.max(0) as usize);
                h.replace_text_in_range(Some(start..range.start), "");
                h.set_selected_text_range(start..start + range.len());
            }
            5 => {
                let length = h.text_length_utf16().unwrap_or(0);
                let start = (a.max(0) as usize).min(length);
                let end = (b.max(0) as usize).min(length);
                let range = start.min(end)..start.max(end);
                let value = h
                    .text_for_range(range.clone(), &mut None)
                    .unwrap_or_default();
                h.replace_and_mark_text_in_range(Some(range), &value, None);
                h.set_selected_text_range(selection.range);
            }
            _ => {}
        }
    });
    post(4);
    post(0);
    true
}

/// Android EditorInfo flags derived from the coordinated GPUI configuration.
pub fn editor_info(generation: u64) -> [i32; 2] {
    let Some(window) = window(generation) else {
        return [1, 1];
    };
    let config = window.text_configuration.borrow();
    let mut input_type = 1;
    if config.autocorrect {
        input_type |= 0x8000;
    }
    if !config.suggestions {
        input_type |= 0x80000;
    }
    input_type |= match config.autocapitalize {
        Autocapitalize::None => 0,
        Autocapitalize::Characters => 0x1000,
        Autocapitalize::Words => 0x2000,
        Autocapitalize::Sentences => 0x4000,
    };
    let action = match config.input_action {
        TextInputAction::Unspecified => 0,
        TextInputAction::Enter => 1,
        TextInputAction::Go => 2,
        TextInputAction::Search => 3,
        TextInputAction::Send => 4,
        TextInputAction::Next => 5,
        TextInputAction::Done => 6,
        TextInputAction::Previous => 7,
    };
    [input_type, action]
}

pub fn editor_action(generation: u64, session: u64) -> bool {
    if session == 0 || input_session(generation) != session {
        return false;
    }
    let Some(window) = window(generation) else {
        return false;
    };
    window.input_handler.with(|handler| handler.unmark_text());
    let action = window.text_configuration.borrow().input_action;
    if matches!(action, TextInputAction::Done) {
        post(2);
    }
    let (key, shift) = match action {
        TextInputAction::Next => ("tab", false),
        TextInputAction::Previous => ("tab", true),
        _ => ("enter", false),
    };
    window.input.with(|callback| {
        callback(PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke {
                key: key.into(),
                modifiers: Modifiers {
                    shift,
                    ..Default::default()
                },
                ..Default::default()
            },
            is_held: false,
            prefer_character_input: false,
        }))
    });
    post(0);
    true
}

/// Insets are relative to the already fitted SurfaceView viewport. The host
/// applies system/IME geometry once, and forwards zero residual edges here.
pub fn insets(generation: u64, values: [i32; 8]) {
    let Some(window) = window(generation) else {
        return;
    };
    let scale = window.scale.get();
    let edges = |offset| Edges {
        left: px(values[offset] as f32 / scale),
        top: px(values[offset + 1] as f32 / scale),
        right: px(values[offset + 2] as f32 / scale),
        bottom: px(values[offset + 3] as f32 / scale),
    };
    let next = WindowInsets {
        safe_area: edges(0),
        ime: edges(4),
        ..Default::default()
    };
    if *window.insets.borrow() != next {
        window.insets.replace(next.clone());
        window.insets_changed.with(|callback| callback(next));
        post(0);
    }
}
