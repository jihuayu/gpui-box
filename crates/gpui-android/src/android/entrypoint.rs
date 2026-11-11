/// Defines the typed Android host exports for an application's GPUI launch function.
#[macro_export]
macro_rules! android_entrypoint {
    ($launch:path) => {
        use $crate::jni::{
            JNIEnv,
            objects::{JClass, JObject, JString},
            sys::{jboolean, jfloatArray, jint, jintArray, jlong, jstring},
        };

        fn report(env: &mut JNIEnv, result: Result<(), String>) {
            if let Err(message) = result {
                let _ = env.throw_new("java/lang/IllegalStateException", message);
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_initialize(
            mut env: JNIEnv,
            class: JClass,
        ) {
            boundary(&mut env, |env| {
                let result = $crate::initialize(env, &class, $launch).map_err(|e| format!("{e:#}"));
                report(env, result);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_attach(
            mut env: JNIEnv,
            _: JClass,
            surface: JObject,
            generation: jlong,
            width: jint,
            height: jint,
            density: f32,
        ) {
            boundary(&mut env, |env| {
                let result =
                    $crate::attach(env, &surface, generation as u64, width, height, density)
                        .map_err(|e| format!("{e:#}"));
                report(env, result);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_detach(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) {
            boundary(&mut env, |_env| {
                $crate::detach(generation as u64);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_resize(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            width: jint,
            height: jint,
            density: f32,
        ) {
            boundary(&mut env, |_env| {
                $crate::resize(generation as u64, width, height, density);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_frame(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) {
            boundary(&mut env, |_env| {
                $crate::frame(generation as u64);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_lifecycle(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            resumed: jboolean,
        ) {
            boundary(&mut env, |_env| {
                $crate::lifecycle(generation as u64, resumed != 0);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_touch(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            id: jlong,
            phase: jint,
            x: f32,
            y: f32,
        ) {
            boundary(&mut env, |_env| {
                $crate::touch(generation as u64, id as u64, phase, x, y);
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_text(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jstring {
            boundary(&mut env, |env| {
                match env.new_string($crate::text(generation as u64)) {
                    Ok(value) => value.into_raw(),
                    Err(_) => std::ptr::null_mut(),
                }
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_textState(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jintArray {
            boundary(&mut env, |env| {
                let Ok(array) = env.new_int_array(4) else {
                    return std::ptr::null_mut();
                };
                if env
                    .set_int_array_region(&array, 0, &$crate::text_state(generation as u64))
                    .is_err()
                {
                    return std::ptr::null_mut();
                }
                array.into_raw()
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_edit(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            session: jlong,
            operation: jint,
            value: JString,
            a: jint,
            b: jint,
        ) -> jboolean {
            boundary(&mut env, |env| match env.get_string(&value) {
                Ok(value) => $crate::edit(
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
            })
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_inputSession(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jlong {
            boundary(&mut env, |_env| {
                $crate::input_session(generation as u64) as jlong
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_backEnabled(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jboolean {
            boundary(&mut env, |_env| {
                $crate::back_enabled(generation as u64) as jboolean
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_back(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jboolean {
            boundary(&mut env, |env| $crate::back(generation as u64) as jboolean)
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_memoryWarning(
            mut env: JNIEnv,
            _: JClass,
        ) {
            boundary(&mut env, |_env| {
                $crate::memory_warning();
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_clipboardAction(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            session: jlong,
            operation: jint,
        ) -> jboolean {
            boundary(&mut env, |_env| {
                $crate::clipboard_action(generation as u64, session as u64, operation) as jboolean
            })
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_editorInfo(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) -> jintArray {
            boundary(&mut env, |env| {
                let array = env.new_int_array(2).expect("allocate EditorInfo");
                env.set_int_array_region(&array, 0, &$crate::editor_info(generation as u64))
                    .expect("write EditorInfo");
                array.into_raw()
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_editorAction(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            session: jlong,
        ) -> jboolean {
            boundary(&mut env, |_env| {
                $crate::editor_action(generation as u64, session as u64) as jboolean
            })
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_accessibilitySnapshot(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            enabled: jboolean,
        ) -> jstring {
            boundary(&mut env, |env| {
                env.new_string($crate::accessibility_snapshot(
                    generation as u64,
                    enabled != 0,
                ))
                .map(|s| s.into_raw())
                .unwrap_or_default()
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_accessibilityAction(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            id: jlong,
            action: jint,
            text: JString,
        ) -> jboolean {
            boundary(&mut env, |env| {
                let Ok(value) = env.get_string(&text) else {
                    return 0;
                };
                $crate::accessibility_action(
                    generation as u64,
                    id as u64,
                    action,
                    &String::from(value),
                ) as jboolean
            })
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_insets(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
        ) {
            boundary(&mut env, |_env| $crate::insets(generation as u64, [0; 8]));
        }
        static NATIVE_FAILED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_openUrlEvent(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            id: JString,
            url: JString,
        ) -> jboolean {
            boundary(&mut env, |env| {
                let Ok(id) = env.get_string(&id).map(String::from) else {
                    return 0;
                };
                let Ok(url) = env.get_string(&url).map(String::from) else {
                    return 0;
                };
                $crate::open_url_event(generation as u64, &id, &url) as jboolean
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_caretBounds(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            session: jlong,
        ) -> jfloatArray {
            boundary(&mut env, |env| {
                let Some(bounds) = $crate::caret_bounds(generation as u64, session as u64) else {
                    return std::ptr::null_mut();
                };
                let Ok(array) = env.new_float_array(4) else {
                    return std::ptr::null_mut();
                };
                if env.set_float_array_region(&array, 0, &bounds).is_err() {
                    return std::ptr::null_mut();
                }
                array.into_raw()
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_key(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            key: JString,
            modifiers: jint,
            down: jboolean,
            held: jboolean,
        ) -> jboolean {
            boundary(&mut env, |env| {
                let Ok(value) = env.get_string(&key) else {
                    return 0;
                };
                $crate::key(
                    generation as u64,
                    &String::from(value),
                    modifiers,
                    down != 0,
                    held != 0,
                ) as jboolean
            })
        }
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn Java_dev_gpui_box_NativeBridge_backGesture(
            mut env: JNIEnv,
            _: JClass,
            generation: jlong,
            phase: jint,
            x: f32,
            y: f32,
            progress: f32,
        ) -> jboolean {
            boundary(&mut env, |_env| {
                $crate::back_gesture(generation as u64, phase, x, y, progress) as jboolean
            })
        }
        fn boundary<T: Default>(env: &mut JNIEnv, operation: impl FnOnce(&mut JNIEnv) -> T) -> T {
            if NATIVE_FAILED.load(std::sync::atomic::Ordering::Acquire) {
                let _ = env.throw_new(
                    "java/lang/IllegalStateException",
                    "GPUI runtime failed; restart the application process",
                );
                return T::default();
            }
            // GPUI state is thread-local and must never be entered from an IME binder thread.
            if env
                .call_static_method("dev/gpui/box/NativeBridge", "checkMainThread", "()V", &[])
                .is_err()
            {
                return T::default();
            }
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(env))) {
                Ok(value) => value,
                Err(_) => {
                    NATIVE_FAILED.store(true, std::sync::atomic::Ordering::Release);
                    if !env.exception_check().unwrap_or(true) {
                        let _ = env.throw_new(
                            "java/lang/IllegalStateException",
                            "GPUI native callback panicked; this runtime must be restarted",
                        );
                    }
                    T::default()
                }
            }
        }
    };
}
