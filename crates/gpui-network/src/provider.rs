//! Explicit provider boundary plus deterministic fake and unavailable
//! providers.

use crate::{
    EndpointDiagnostic, EndpointOrigin, NetworkOperation, NetworkSnapshot, ProxyCandidate,
    ProxyConfig,
};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode, Evidence, PermissionState,
    ProviderManifest, Support, SystemError,
};
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

fn unavailable() -> SystemError {
    SystemError::new(
        ErrorCode::Unavailable,
        "No native network provider is installed",
    )
}

/// Provider-to-service pushes. They are delivered synchronously and must never
/// be emitted while the provider holds its own state lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkProviderNotice {
    SnapshotUpdated(NetworkSnapshot),
    ProxyConfigurationInvalidated,
    ProxyCredentialsRotated,
}

/// The service's weak provider listener.
pub trait NetworkProviderListener: Send + Sync + 'static {
    fn on_network_provider_notice(&self, notice: NetworkProviderNotice);
}

/// Explicit native/provider boundary.
///
/// Every method is safe to call from a background thread. Capability snapshots
/// are advisory; the service rechecks platform constraints and the execution
/// context immediately before and after each external call.
pub trait NetworkProvider: Send + Sync + 'static {
    fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation>;

    fn capabilities(&self) -> Vec<Capability<NetworkOperation>> {
        NetworkOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn snapshot(&self) -> Result<NetworkSnapshot, SystemError>;

    fn resolve_proxies(
        &self,
        target: &EndpointOrigin,
        config: &ProxyConfig,
    ) -> Result<Vec<ProxyCandidate>, SystemError>;

    fn diagnose_endpoint(&self, target: &EndpointOrigin)
    -> Result<EndpointDiagnostic, SystemError>;

    fn attach_listener(&self, listener: Weak<dyn NetworkProviderListener>);

    fn as_any(&self) -> &dyn Any;
}

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// Contract-only provider used when no native host adapter is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableNetworkProvider;

impl UnavailableNetworkProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl NetworkProvider for UnavailableNetworkProvider {
    fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation> {
        Capability {
            operation,
            support: Support::Conditional,
            availability: Availability::Unavailable,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 0,
        }
    }

    fn snapshot(&self) -> Result<NetworkSnapshot, SystemError> {
        Ok(NetworkSnapshot::unknown(crate::SignalSource::Unavailable))
    }

    fn resolve_proxies(
        &self,
        _target: &EndpointOrigin,
        _config: &ProxyConfig,
    ) -> Result<Vec<ProxyCandidate>, SystemError> {
        Err(unavailable())
    }

    fn diagnose_endpoint(
        &self,
        _target: &EndpointOrigin,
    ) -> Result<EndpointDiagnostic, SystemError> {
        Err(unavailable())
    }

    fn attach_listener(&self, _listener: Weak<dyn NetworkProviderListener>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeState {
    snapshot: NetworkSnapshot,
    capabilities: BTreeMap<NetworkOperation, Capability<NetworkOperation>>,
    proxy_results: BTreeMap<EndpointOrigin, Vec<ProxyCandidate>>,
    diagnostics: BTreeMap<EndpointOrigin, EndpointDiagnostic>,
    listeners: Vec<Weak<dyn NetworkProviderListener>>,
    snapshot_calls: u64,
    resolve_calls: u64,
    diagnose_calls: u64,
}

/// Deterministic provider for tests and headless product integration.
#[derive(Clone)]
pub struct FakeNetworkProvider {
    state: Arc<Mutex<FakeState>>,
}

impl FakeNetworkProvider {
    pub fn new() -> Self {
        let capabilities = NetworkOperation::ALL
            .into_iter()
            .map(|operation| {
                (
                    operation,
                    Capability {
                        operation,
                        support: Support::Native,
                        availability: Availability::Available,
                        configuration: Configuration::Ready,
                        permission: PermissionState::NotRequired,
                        evidence: Evidence::ContractOnly,
                        constraints: Vec::new(),
                        observed_revision: 1,
                    },
                )
            })
            .collect();
        Self {
            state: Arc::new(Mutex::new(FakeState {
                snapshot: NetworkSnapshot::unknown(crate::SignalSource::Unknown),
                capabilities,
                proxy_results: BTreeMap::new(),
                diagnostics: BTreeMap::new(),
                listeners: Vec::new(),
                snapshot_calls: 0,
                resolve_calls: 0,
                diagnose_calls: 0,
            })),
        }
    }

    pub fn with_snapshot(snapshot: NetworkSnapshot) -> Self {
        let provider = Self::new();
        provider.set_snapshot(snapshot);
        provider
    }

    pub fn snapshot(&self) -> NetworkSnapshot {
        lock(&self.state).snapshot.clone()
    }

    pub fn set_snapshot(&self, snapshot: NetworkSnapshot) {
        let notice = {
            let mut state = lock(&self.state);
            state.snapshot = snapshot.clone();
            NetworkProviderNotice::SnapshotUpdated(snapshot)
        };
        self.notify(notice);
    }

    /// Simulate an OS proxy/PAC configuration change.
    pub fn invalidate_proxy_configuration(&self) {
        self.notify(NetworkProviderNotice::ProxyConfigurationInvalidated);
    }

    /// Simulate rotated proxy credentials, such as a Keychain/Keystore update.
    pub fn rotate_proxy_credentials(&self) {
        self.notify(NetworkProviderNotice::ProxyCredentialsRotated);
    }

    pub fn mutate_snapshot(&self, update: impl FnOnce(&mut NetworkSnapshot)) {
        let notice = {
            let mut state = lock(&self.state);
            let previous = state.snapshot.clone();
            update(&mut state.snapshot);
            if state.snapshot == previous {
                return;
            }
            state.snapshot.revision = previous.revision.saturating_add(1);
            NetworkProviderNotice::SnapshotUpdated(state.snapshot.clone())
        };
        self.notify(notice);
    }

    pub fn set_capability(
        &self,
        operation: NetworkOperation,
        capability: Capability<NetworkOperation>,
    ) {
        lock(&self.state).capabilities.insert(operation, capability);
    }

    pub fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation> {
        lock(&self.state)
            .capabilities
            .get(&operation)
            .cloned()
            .expect("the fake provider contains every network operation")
    }

    pub fn set_proxy_result(&self, target: EndpointOrigin, candidates: Vec<ProxyCandidate>) {
        lock(&self.state).proxy_results.insert(target, candidates);
    }

    pub fn set_diagnostic(&self, target: EndpointOrigin, diagnostic: EndpointDiagnostic) {
        lock(&self.state).diagnostics.insert(target, diagnostic);
    }

    pub fn snapshot_call_count(&self) -> u64 {
        lock(&self.state).snapshot_calls
    }

    pub fn resolve_call_count(&self) -> u64 {
        lock(&self.state).resolve_calls
    }

    pub fn diagnose_call_count(&self) -> u64 {
        lock(&self.state).diagnose_calls
    }

    pub fn listener_count(&self) -> usize {
        let mut state = lock(&self.state);
        state
            .listeners
            .retain(|listener| listener.strong_count() > 0);
        state.listeners.len()
    }

    fn notify(&self, notice: NetworkProviderNotice) {
        let listeners = {
            let mut state = lock(&self.state);
            state
                .listeners
                .retain(|listener| listener.strong_count() > 0);
            state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for listener in listeners {
            listener.on_network_provider_notice(notice.clone());
        }
    }
}

impl Default for FakeNetworkProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkProvider for FakeNetworkProvider {
    fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation> {
        Self::capability(self, operation)
    }

    fn snapshot(&self) -> Result<NetworkSnapshot, SystemError> {
        let mut state = lock(&self.state);
        state.snapshot_calls = state.snapshot_calls.saturating_add(1);
        Ok(state.snapshot.clone())
    }

    fn resolve_proxies(
        &self,
        target: &EndpointOrigin,
        config: &ProxyConfig,
    ) -> Result<Vec<ProxyCandidate>, SystemError> {
        let mut state = lock(&self.state);
        state.resolve_calls = state.resolve_calls.saturating_add(1);
        Ok(state
            .proxy_results
            .get(target)
            .cloned()
            .unwrap_or_else(|| config.resolve_proxies(target)))
    }

    fn diagnose_endpoint(
        &self,
        target: &EndpointOrigin,
    ) -> Result<EndpointDiagnostic, SystemError> {
        let mut state = lock(&self.state);
        state.diagnose_calls = state.diagnose_calls.saturating_add(1);
        state
            .diagnostics
            .get(target)
            .cloned()
            .ok_or_else(unavailable)
    }

    fn attach_listener(&self, listener: Weak<dyn NetworkProviderListener>) {
        let mut state = lock(&self.state);
        state.listeners.retain(|current| current.strong_count() > 0);
        state.listeners.push(listener);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
