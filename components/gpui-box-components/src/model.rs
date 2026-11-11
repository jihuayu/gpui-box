//! Small state prototypes owned by the screen model across layout changes.
use gpui_system::{SystemError, SystemServices};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Navigation<T> {
    root: T,
    routes: Vec<T>,
}
impl<T> Navigation<T> {
    pub fn new(root: T) -> Self {
        Self {
            root,
            routes: Vec::new(),
        }
    }
    pub fn current(&self) -> &T {
        self.routes.last().unwrap_or(&self.root)
    }
    pub fn root(&self) -> &T {
        &self.root
    }
    pub fn routes(&self) -> &[T] {
        &self.routes
    }
    pub fn push(&mut self, route: T) {
        self.routes.push(route);
    }
    pub fn pop(&mut self) -> bool {
        self.routes.pop().is_some()
    }
    pub fn depth(&self) -> usize {
        self.routes.len()
    }
    pub fn reset_to_root(&mut self) {
        self.routes.clear();
    }
}

/// A screen-owned navigation stack plus explicitly registered services.
///
/// Services are typed `gpui_system::SystemServices` entries, not process
/// globals. Tests and applications can install deterministic providers on one
/// screen without leaking them into another screen or the process.
pub struct ScreenModel<R> {
    navigation: Navigation<R>,
    services: SystemServices,
}
impl<R> ScreenModel<R> {
    pub fn new(root: R) -> Self {
        Self::with_services(root, SystemServices::new())
    }
    pub fn with_services(root: R, services: SystemServices) -> Self {
        Self {
            navigation: Navigation::new(root),
            services,
        }
    }
    /// Register one service, rejecting a duplicate concrete type.
    pub fn with_service<S: 'static>(mut self, service: S) -> Result<Self, SystemError> {
        self.insert_service(service)?;
        Ok(self)
    }
    pub fn insert_service<S: 'static>(&mut self, service: S) -> Result<(), SystemError> {
        self.services.register(service)
    }
    /// Replace an existing service or install the first one. Install overrides
    /// before starting work; existing handles retain their original provider.
    pub fn replace_service<S: 'static>(&mut self, service: S) {
        self.services.override_provider(service);
    }
    pub fn service<S: 'static>(&self) -> Result<Rc<S>, SystemError> {
        self.services.service::<S>()
    }
    pub fn services(&self) -> &SystemServices {
        &self.services
    }
    pub fn services_mut(&mut self) -> &mut SystemServices {
        &mut self.services
    }
    pub fn navigation(&self) -> &Navigation<R> {
        &self.navigation
    }
    pub fn navigation_mut(&mut self) -> &mut Navigation<R> {
        &mut self.navigation
    }
}
/// Request epochs reject responses from an obsolete screen or query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resource<T> {
    epoch: u64,
    value: Option<T>,
    pending: bool,
    error: Option<gpui_system::SystemError>,
}
impl<T> Resource<T> {
    pub fn start(&mut self) -> u64 {
        self.epoch += 1;
        self.pending = true;
        self.error = None;
        self.epoch
    }
    pub fn complete(&mut self, epoch: u64, result: Result<T, gpui_system::SystemError>) -> bool {
        if epoch != self.epoch || !self.pending {
            return false;
        }
        self.pending = false;
        match result {
            Ok(value) => {
                self.value = Some(value);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }
    pub fn cancel(&mut self) {
        self.epoch += 1;
        self.pending = false;
    }
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn error(&self) -> Option<&gpui_system::SystemError> {
        self.error.as_ref()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_keeps_a_root_and_restores_previous_route() {
        let mut routes = Navigation::new("catalog");
        assert!(!routes.pop());
        routes.push("settings");
        routes.push("details");
        assert_eq!(routes.depth(), 2);
        assert!(routes.pop());
        assert_eq!(routes.current(), &"settings");
    }
    #[test]
    fn resources_reject_stale_duplicate_and_cancelled_completions() {
        let mut resource = Resource::default();
        let old = resource.start();
        let current = resource.start();
        assert!(!resource.complete(old, Ok(1)));
        assert!(resource.complete(current, Ok(2)));
        assert!(!resource.complete(current, Ok(3)));
        assert_eq!(resource.value(), Some(&2));
        let cancelled = resource.start();
        resource.cancel();
        assert!(!resource.complete(cancelled, Ok(4)));
        let failed = resource.start();
        assert!(resource.complete(
            failed,
            Err(gpui_system::SystemError::new(
                gpui_system::ErrorCode::PlatformFailure,
                "offline"
            ))
        ));
        assert_eq!(
            resource.error().unwrap().code,
            gpui_system::ErrorCode::PlatformFailure
        );
        assert_eq!(resource.value(), Some(&2));
    }
}
