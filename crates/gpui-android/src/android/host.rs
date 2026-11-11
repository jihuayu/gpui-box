//! One UI-thread lifecycle service shared by the native host and application.
use gpui_lifecycle::*;
use std::{cell::Cell, time::Instant};

const SURFACE: SurfaceId = SurfaceId(1);
thread_local! {
    static LAST_GENERATION: Cell<u64> = const { Cell::new(0) };
    static SERVICE: LifecycleService = LifecycleService::new(
        ProcessId(std::process::id()), SessionId(1));
}

/// Clone the process-owned handle on Android's main Looper. The same handle is
/// installed in GPUI's SystemServices registry before the application launches.
pub fn lifecycle_service() -> LifecycleService {
    SERVICE.with(Clone::clone)
}

pub(super) fn attach(generation: u64) {
    if lifecycle_service()
        .register_surface(SURFACE, generation)
        .unwrap_or(false)
    {
        LAST_GENERATION.with(|current| current.set(generation));
    }
}

pub(super) fn detach(generation: u64) {
    let _ = lifecycle_service().detach_surface_at(SURFACE, generation, Instant::now());
}

fn apply_visibility(service: &LifecycleService, generation: u64, resumed: bool, focused: bool) {
    let now = Instant::now();
    let visibility = if resumed {
        SurfaceVisibility::Visible { focused }
    } else {
        SurfaceVisibility::Hidden
    };
    let snapshot = SurfaceSnapshot {
        visibility,
        generation,
    };
    if service.surface(SURFACE).is_some() {
        let _ = service.apply_surface_at(SURFACE, snapshot, now);
    }
    let phase = match (resumed, focused) {
        (true, true) => AppPhase::ForegroundActive,
        (true, false) => AppPhase::ForegroundInactive,
        (false, _) => AppPhase::Background,
    };
    let _ = service.set_phase_at(phase, now);
}

/// Apply host observations only for the current attached surface generation.
/// Missing platform preferences retain explicit Unavailable values.
#[allow(clippy::too_many_arguments)]
pub fn host_state(
    generation: u64,
    resumed: bool,
    focused: bool,
    locked: bool,
    dark: bool,
    text_scale: f32,
    locale: &str,
    reduced_motion: bool,
) {
    let service = lifecycle_service();
    if generation == 0 || !LAST_GENERATION.with(|current| current.get() == generation) {
        return;
    }
    apply_visibility(&service, generation, resumed, focused);
    let _ = service.set_session_lock(if locked {
        SessionLockState::Locked
    } else {
        SessionLockState::Unlocked
    });
    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(if dark {
            Appearance::Dark
        } else {
            Appearance::Light
        }),
        text_scale: TextScale::new(text_scale)
            .map(PreferenceValue::Known)
            .unwrap_or_default(),
        locale: LocaleTag::parse(locale)
            .map(PreferenceValue::Known)
            .unwrap_or_default(),
        reduced_motion: PreferenceValue::Known(reduced_motion),
        high_contrast: PreferenceValue::Unavailable,
    };
    let _ = service.set_preferences(preferences);
    if let Some(window) = super::bridge::window(generation) {
        let appearance = if dark {
            gpui::WindowAppearance::Dark
        } else {
            gpui::WindowAppearance::Light
        };
        if window.appearance.replace(appearance) != appearance {
            window.appearance_changed.with(|callback| callback());
            super::bridge::post(0);
        }
    }
}

pub(super) fn input() {
    let _ = lifecycle_service().record_input_at(Instant::now());
}

pub(super) fn memory_warning() {
    let _ = lifecycle_service().set_memory_pressure(MemoryPressure::Warning);
}

/// Redacted host diagnostics for emulator qualification and support reports.
pub fn host_snapshot() -> String {
    let service = lifecycle_service();
    let preferences = service.preferences();
    serde_json::json!({
        "phase": format!("{:?}", service.phase()),
        "generation": service.surface(SURFACE).map(|s| s.generation),
        "focused": service.surface(SURFACE).is_some_and(|s| s.visibility.is_focused()),
        "locked": service.session_lock() == SessionLockState::Locked,
        "memory": format!("{:?}", service.memory_pressure()),
        "textScale": preferences.text_scale.available().map(|s| s.get()),
        "locale": preferences.locale.available().map(|s| s.as_str()),
        "reducedMotion": preferences.reduced_motion.available(),
        "activeMillis": service.active_duration(Instant::now()).as_millis() as u64,
    })
    .to_string()
}
