use gpui_system::{DiagnosticPhase, SurfaceId};
use std::collections::HashMap;

/// The application lifecycle phase reported by the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppPhase {
    /// The process is starting before the first scene attaches.
    #[default]
    Launching,
    /// Visible and receiving input.
    ForegroundActive,
    /// Visible but not receiving input.
    ForegroundInactive,
    /// Not visible; the process may be reclaimed.
    Background,
    /// Execution is suspended without terminating the process.
    Suspended,
    /// The host is terminating the process.
    Terminating,
}

impl AppPhase {
    /// Whether the application is in an active foreground phase.
    ///
    /// `Launching` is deliberately excluded: visible surfaces may be reported
    /// before the first foreground transition, but they must not accrue
    /// foreground time yet.
    pub fn is_foreground(self) -> bool {
        matches!(self, Self::ForegroundActive | Self::ForegroundInactive)
    }

    /// Whether the application may receive user input.
    pub fn accepts_input(self) -> bool {
        self == Self::ForegroundActive
    }
}

/// Visibility and focus of one logical window or scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceVisibility {
    /// Visible, with an explicit focus state.
    Visible { focused: bool },
    /// Hidden by the host without being minimized.
    Hidden,
    /// Minimized to the platform task surface.
    Minimized,
    /// Covered by another window or system surface.
    Occluded,
}

impl SurfaceVisibility {
    /// Whether the surface is visible, focused or not.
    pub fn is_visible(self) -> bool {
        matches!(self, Self::Visible { .. })
    }

    /// Whether the surface is visible and focused.
    pub fn is_focused(self) -> bool {
        matches!(self, Self::Visible { focused: true })
    }
}

/// One host snapshot for a logical surface.
///
/// The generation must equal the currently registered generation. Older
/// callbacks are rejected and never revive a detached surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSnapshot {
    /// Current visibility and focus.
    pub visibility: SurfaceVisibility,
    /// Monotonic host attachment generation.
    pub generation: u64,
}

/// Device lock state, which is separate from application foregrounding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SessionLockState {
    /// The device is unlocked.
    #[default]
    Unlocked,
    /// The device is locked; rendering should be conservative.
    Locked,
}

/// Coarse memory-pressure signal from the platform.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MemoryPressure {
    /// Ordinary operation.
    #[default]
    Normal,
    /// The host asks the application to release caches soon.
    Warning,
    /// The host asks the application to release caches immediately.
    Critical,
}

/// A redacted record of a rejected host callback.
///
/// Only numeric identities and the diagnostic phase are retained; native
/// payloads and user data are never copied into diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectedCallback {
    /// The surface named by the callback.
    pub surface: SurfaceId,
    /// The generation carried by the callback.
    pub received_generation: u64,
    /// The currently registered generation, if the surface is attached.
    pub current_generation: Option<u64>,
    /// Always [`DiagnosticPhase::LateCallbackDiscarded`].
    pub phase: DiagnosticPhase,
}

impl RejectedCallback {
    pub(crate) fn late(
        surface: SurfaceId,
        received_generation: u64,
        current_generation: Option<u64>,
    ) -> Self {
        Self {
            surface,
            received_generation,
            current_generation,
            phase: DiagnosticPhase::LateCallbackDiscarded,
        }
    }
}

/// Coherent lifecycle state for one process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifecycleState {
    phase: AppPhase,
    surfaces: HashMap<SurfaceId, SurfaceSnapshot>,
    session_lock: SessionLockState,
    memory_pressure: MemoryPressure,
    revision: u64,
}

impl LifecycleState {
    /// The current application phase.
    pub fn phase(&self) -> AppPhase {
        self.phase
    }

    /// The registered snapshot for one surface, if attached.
    pub fn surface(&self, surface: SurfaceId) -> Option<SurfaceSnapshot> {
        self.surfaces.get(&surface).copied()
    }

    /// Whether the surface is attached and visible.
    pub fn is_surface_visible(&self, surface: SurfaceId) -> bool {
        self.surface(surface)
            .is_some_and(|snapshot| snapshot.visibility.is_visible())
    }

    /// All attached surfaces in unspecified order.
    pub fn surfaces(&self) -> impl Iterator<Item = (SurfaceId, SurfaceSnapshot)> + '_ {
        self.surfaces
            .iter()
            .map(|(surface, snapshot)| (*surface, *snapshot))
    }

    /// Attached surfaces that are currently visible.
    pub fn visible_surfaces(&self) -> impl Iterator<Item = (SurfaceId, SurfaceSnapshot)> + '_ {
        self.surfaces()
            .filter(|(_, snapshot)| snapshot.visibility.is_visible())
    }

    /// The current device lock state.
    pub fn session_lock(&self) -> SessionLockState {
        self.session_lock
    }

    /// The last memory-pressure signal.
    pub fn memory_pressure(&self) -> MemoryPressure {
        self.memory_pressure
    }

    /// Monotonic revision of accepted lifecycle state.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn set_phase(&mut self, phase: AppPhase) {
        self.phase = phase;
        self.bump();
    }

    pub(crate) fn set_session_lock(&mut self, session_lock: SessionLockState) {
        self.session_lock = session_lock;
        self.bump();
    }

    pub(crate) fn set_memory_pressure(&mut self, memory_pressure: MemoryPressure) {
        self.memory_pressure = memory_pressure;
        self.bump();
    }

    pub(crate) fn insert_surface(&mut self, surface: SurfaceId, snapshot: SurfaceSnapshot) {
        self.surfaces.insert(surface, snapshot);
        self.bump();
    }

    pub(crate) fn remove_surface(&mut self, surface: SurfaceId) -> Option<SurfaceSnapshot> {
        let removed = self.surfaces.remove(&surface);
        if removed.is_some() {
            self.bump();
        }
        removed
    }

    fn bump(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}
