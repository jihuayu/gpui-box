use super::{
    dispatcher::Dispatcher,
    platform::AndroidPlatform,
    window::{NativeSurface, WindowState},
};
use gpui::*;
use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JClass, JObject, JValue},
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
}
thread_local! {
    static RUNTIME: RefCell<Option<Rc<Runtime>>> = const { RefCell::new(None) };
    static LAUNCH: RefCell<Option<fn(&mut App)>> = const { RefCell::new(None) };
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

fn runtime() -> Option<Rc<Runtime>> {
    RUNTIME.with(|slot| slot.borrow().clone())
}
fn window(generation: u64) -> Option<Rc<WindowState>> {
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
        let application = Application::with_platform(platform.clone()).run_embedded(launch);
        RUNTIME.with(|slot| {
            slot.replace(Some(Rc::new(Runtime {
                platform,
                _application: application,
                queue: RefCell::new(queue),
            })))
        });
    }
    log::info!("Surface attached generation={generation} {width}x{height} density={scale}");
    post(0);
    Ok(())
}

pub fn detach(generation: u64) {
    if let Some(runtime) = runtime() {
        runtime.platform.window.detach(generation);
    }
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

pub fn touch(generation: u64, id: u64, phase: i32, x: f32, y: f32) {
    let Some(window) = window(generation) else {
        return;
    };
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

/// IME offsets and cursor arguments use UTF-16 code units, as Android does.
/// Operations are commit, compose, finish, selection, delete, composing-region.
pub fn edit(generation: u64, operation: i32, value: &str, a: i32, b: i32) {
    let Some(window) = window(generation) else {
        return;
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
}
