//! Read-only component inputs backed by the ordinary generation-checked source
//! graph. Only the keyed logical draft can replace their values.
use super::*;

/// A live component input, distinct from a mount-time value and writable state.
/// Read/bind through the owner or current draft; no `Signal` conversion or
/// transaction setter is exposed. Retirement invalidates the same generation
/// checked by ordinary sources. This handle does not own the source lifetime.
/// It stays owner-local, including when its value type is Send.
///
/// ```compile_fail
/// use gpui_box::retained::reactive::{ReadProp, ReactiveTransaction};
/// fn write(tx: &mut ReactiveTransaction<'_>, prop: ReadProp<u32>) {
///     tx.set(prop, 1).unwrap(); // only writable Signal has this capability
/// }
/// ```
///
/// ```compile_fail
/// use gpui_box::retained::reactive::ReadProp;
/// fn requires_send<T: Send>() {}
/// requires_send::<ReadProp<u32>>();
/// ```
pub struct ReadProp<T> {
    pub(in crate::retained) signal: Signal<T>,
    pub(in crate::retained) owner_local: PhantomData<Rc<()>>,
}
impl<T> Copy for ReadProp<T> {}
impl<T> Clone for ReadProp<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> std::fmt::Debug for ReadProp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ReadProp").field(&self.signal.id).finish()
    }
}
impl<T> PartialEq for ReadProp<T> {
    fn eq(&self, other: &Self) -> bool {
        self.signal.id == other.signal.id
    }
}
impl<T> Eq for ReadProp<T> {}
impl<T> ReadProp<T> {
    pub fn dependency(self) -> Dependency {
        self.signal.dependency()
    }
}
impl<T: 'static> ScopedSource for ReadProp<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn dependency(&self) -> Dependency {
        self.signal.dependency()
    }
}
impl ReactiveUi {
    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.read(prop.signal)
    }
    pub fn bind_prop<T: 'static>(
        &mut self,
        prop: ReadProp<T>,
        property: Property<T>,
    ) -> Result<(BindingId, Commit), Error> {
        self.bind(prop.signal, property)
    }
}
impl ReactiveTransaction<'_> {
    pub fn read_prop<T: 'static>(&mut self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.read(prop.signal)
    }
}
impl ComputedInputs<'_> {
    /// Includes EffectInputs, which is the same declared-dependency view.
    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.read(prop.signal)
    }
}
