//! Optional adapter over the distribution's single coordinated GPUI identity.
//!
//! Every function here must run on the owning GPUI UI thread.

use crate::{AppPhase, Appearance, LifecycleService, PreferenceValue, SystemPreferences};
use gpui::{App, AppLifecyclePhase, Context, Entity, Subscription, WindowAppearance};
use gpui_system::{ErrorCode, SystemError, SystemServices, UiOwner};
use std::rc::Rc;

/// Converts GPUI's native lifecycle vocabulary into the distribution phase.
pub fn phase_from_gpui(phase: AppLifecyclePhase) -> AppPhase {
    match phase {
        AppLifecyclePhase::Active => AppPhase::ForegroundActive,
        AppLifecyclePhase::Inactive => AppPhase::ForegroundInactive,
        AppLifecyclePhase::Foreground => AppPhase::ForegroundInactive,
        AppLifecyclePhase::Background => AppPhase::Background,
        AppLifecyclePhase::Disconnected => AppPhase::Suspended,
    }
}

/// Converts GPUI's window appearance into the distribution appearance.
pub fn appearance_from_gpui(appearance: WindowAppearance) -> Appearance {
    match appearance {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

/// Reads the current GPUI window appearance.
pub fn current_appearance(cx: &App) -> PreferenceValue<Appearance> {
    PreferenceValue::Known(appearance_from_gpui(cx.window_appearance()))
}

/// Reads the GPUI-known preferences, leaving unknown signals unavailable.
pub fn preferences_from_gpui(cx: &App) -> SystemPreferences {
    SystemPreferences {
        appearance: current_appearance(cx),
        ..SystemPreferences::default()
    }
}

/// Observes a selected lifecycle phase and notifies only on changes.
pub fn observe_phase<T: 'static>(
    entity: &Entity<T>,
    select: impl Fn(&T) -> AppPhase + 'static,
    changed: impl FnMut(AppPhase, &mut App) + 'static,
    cx: &mut App,
) -> Subscription {
    gpui_system::gpui_adapter::observe_selector(entity, select, changed, cx)
}

/// Observes selected system preferences and notifies only on changes.
pub fn observe_preferences<T: 'static>(
    entity: &Entity<T>,
    select: impl Fn(&T) -> SystemPreferences + 'static,
    changed: impl FnMut(SystemPreferences, &mut App) + 'static,
    cx: &mut App,
) -> Subscription {
    gpui_system::gpui_adapter::observe_selector(entity, select, changed, cx)
}

/// Creates an entity-owned native scope; keep the returned scope in the entity.
pub fn entity_scope<T: 'static>(
    owner: UiOwner,
    cx: &Context<T>,
) -> gpui_system::gpui_adapter::EntityScope {
    gpui_system::gpui_adapter::EntityScope::new(owner, cx)
}

/// Installs a lifecycle service in GPUI's global typed service registry.
pub fn install_lifecycle_service(
    service: LifecycleService,
    cx: &mut App,
) -> Result<(), SystemError> {
    cx.default_global::<SystemServices>().register(service)
}

/// Reads the shared lifecycle handle from the global registry.
///
/// The returned handle is cheap to clone and supports `&self` queries,
/// subscriptions and host transitions on the UI thread.
pub fn lifecycle_service(cx: &App) -> Result<Rc<LifecycleService>, SystemError> {
    let missing = SystemError::new(ErrorCode::Unavailable, "SystemServices is not installed");
    let services = cx.try_global::<SystemServices>().ok_or(missing)?;
    services.service::<LifecycleService>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_map_to_distinct_lifecycle_phases() {
        let active = phase_from_gpui(AppLifecyclePhase::Active);
        assert_eq!(active, AppPhase::ForegroundActive);
        let inactive = phase_from_gpui(AppLifecyclePhase::Inactive);
        assert_eq!(inactive, AppPhase::ForegroundInactive);
        let foreground = phase_from_gpui(AppLifecyclePhase::Foreground);
        assert_eq!(foreground, AppPhase::ForegroundInactive);
        let background = phase_from_gpui(AppLifecyclePhase::Background);
        assert_eq!(background, AppPhase::Background);
        let disconnected = phase_from_gpui(AppLifecyclePhase::Disconnected);
        assert_eq!(disconnected, AppPhase::Suspended);
    }

    #[test]
    fn appearances_map_to_light_and_dark() {
        let light = appearance_from_gpui(WindowAppearance::Light);
        assert_eq!(light, Appearance::Light);
        let vibrant_light = appearance_from_gpui(WindowAppearance::VibrantLight);
        assert_eq!(vibrant_light, Appearance::Light);
        let dark = appearance_from_gpui(WindowAppearance::Dark);
        assert_eq!(dark, Appearance::Dark);
        let vibrant_dark = appearance_from_gpui(WindowAppearance::VibrantDark);
        assert_eq!(vibrant_dark, Appearance::Dark);
    }
}
