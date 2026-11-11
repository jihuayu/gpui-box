#![cfg(target_os = "android")]
use gpui_box::android as gpui_android;
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JString},
    sys::{jboolean, jint, jintArray, jlong, jstring},
};

fn report(env: &mut JNIEnv, result: Result<(), String>) {
    if let Err(message) = result {
        let _ = env.throw_new("java/lang/IllegalStateException", message);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_initialize(mut env: JNIEnv, class: JClass) {
    let result = gpui_android::initialize(&mut env, &class, gpui_box_example_hello::open)
        .map_err(|e| format!("{e:#}"));
    report(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_attach(
    mut env: JNIEnv,
    _: JClass,
    surface: JObject,
    generation: jlong,
    width: jint,
    height: jint,
    density: f32,
) {
    let result = gpui_android::attach(
        &mut env,
        &surface,
        generation as u64,
        width,
        height,
        density,
    )
    .map_err(|e| format!("{e:#}"));
    report(&mut env, result);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_detach(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) {
    gpui_android::detach(generation as u64);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_resize(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
    width: jint,
    height: jint,
    density: f32,
) {
    gpui_android::resize(generation as u64, width, height, density);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_frame(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) {
    gpui_android::frame(generation as u64);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_lifecycle(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
    resumed: jboolean,
) {
    gpui_android::lifecycle(generation as u64, resumed != 0);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_touch(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
    id: jlong,
    phase: jint,
    x: f32,
    y: f32,
) {
    gpui_android::touch(generation as u64, id as u64, phase, x, y);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_text(
    env: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jstring {
    match env.new_string(gpui_android::text(generation as u64)) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_textState(
    env: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jintArray {
    let Ok(array) = env.new_int_array(4) else {
        return std::ptr::null_mut();
    };
    if env
        .set_int_array_region(&array, 0, &gpui_android::text_state(generation as u64))
        .is_err()
    {
        return std::ptr::null_mut();
    }
    array.into_raw()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_edit(
    mut env: JNIEnv,
    _: JClass,
    generation: jlong,
    session: jlong,
    operation: jint,
    value: JString,
    a: jint,
    b: jint,
) -> jboolean {
    match env.get_string(&value) {
        Ok(value) => gpui_android::edit(
            generation as u64,
            session as u64,
            operation,
            &String::from(value),
            a,
            b,
        ) as jboolean,
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error.to_string());
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_inputSession(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jlong {
    gpui_android::input_session(generation as u64) as jlong
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_backEnabled(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jboolean {
    gpui_android::back_enabled(generation as u64) as jboolean
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_back(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
) -> jboolean {
    gpui_android::back(generation as u64) as jboolean
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_memoryWarning(_: JNIEnv, _: JClass) {
    gpui_android::memory_warning();
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_gpui_box_NativeBridge_clipboardAction(
    _: JNIEnv,
    _: JClass,
    generation: jlong,
    session: jlong,
    operation: jint,
) -> jboolean {
    gpui_android::clipboard_action(generation as u64, session as u64, operation) as jboolean
}
