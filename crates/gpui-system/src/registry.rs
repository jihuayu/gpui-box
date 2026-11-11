use crate::{ErrorCode, SystemError};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    rc::Rc,
};

/// Static typed registration on the UI thread. Register a concrete service
/// wrapper (which may contain an Rc<dyn Provider>) rather than untyped strings.
/// Rc deliberately keeps platform providers and their native UI objects local.
#[derive(Default)]
pub struct SystemServices {
    providers: HashMap<TypeId, Rc<dyn Any>>,
}

impl SystemServices {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T: 'static>(&mut self, provider: T) -> Result<(), SystemError> {
        if self.providers.contains_key(&TypeId::of::<T>()) {
            return Err(SystemError::new(
                ErrorCode::InvalidConfiguration,
                "Service is already registered",
            ));
        }
        self.providers.insert(TypeId::of::<T>(), Rc::new(provider));
        Ok(())
    }

    /// Explicit replacement enables scoped test/app overrides. Existing handles
    /// retain their provider; install overrides before starting work.
    pub fn override_provider<T: 'static>(&mut self, provider: T) {
        self.providers.insert(TypeId::of::<T>(), Rc::new(provider));
    }

    pub fn service<T: 'static>(&self) -> Result<Rc<T>, SystemError> {
        self.providers
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|provider| provider.downcast::<T>().ok())
            .ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Service provider is not installed")
            })
    }
}
