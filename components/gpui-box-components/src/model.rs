//! Small state prototypes owned by the screen model across layout changes.
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
    pub fn push(&mut self, route: T) {
        self.routes.push(route);
    }
    pub fn pop(&mut self) -> bool {
        self.routes.pop().is_some()
    }
    pub fn depth(&self) -> usize {
        self.routes.len()
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
