//! Typed complete theme groups. Group construction is owner-independent data;
//! logical providers/selectors use the ordinary owner-local source engine.
use super::{Error, logical::*};
use std::{fmt, marker::PhantomData, sync::Arc};

/// An explicit group identity, independent of its value type. Defaults must
/// define the complete group. Validation runs on construction, before any source
/// adoption. Values and validation must not depend on mutable external state.
/// This Rust schema is not a portable token-expression or serialization format.
///
/// ```compile_fail
/// use gpui_box::retained::{logical::LogicalEdit,theme::{ThemeKey,ThemeGroup},reactive::Signal};
/// struct A; impl ThemeKey for A {type Values=u32;}
/// struct B; impl ThemeKey for B {type Values=u32;}
/// fn wrong(edit:&mut LogicalEdit<'_, '_, '_>,scope:gpui_box::retained::logical::ScopeId,input:Signal<ThemeGroup<A>>) {
///     edit.provide_theme::<B>(scope,input).unwrap();
/// }
/// ```
pub trait ThemeKey: 'static {
    type Values: Default + Clone + PartialEq + Send + Sync + 'static;
    fn validate(_values: &Self::Values) -> Result<(), Error> {
        Ok(())
    }
}

/// Immutable whole-group replacement. Missing overrides start from this group's
/// defaults, never from an outer provider. Clone retains snapshot identity;
/// independently constructed equal groups compare by value for source equality.
pub struct ThemeGroup<K: ThemeKey> {
    values: Arc<K::Values>,
    key: PhantomData<fn() -> K>,
}
impl<K: ThemeKey> Clone for ThemeGroup<K> {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            key: PhantomData,
        }
    }
}
impl<K: ThemeKey> PartialEq for ThemeGroup<K> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.values, &other.values) || self.values == other.values
    }
}
impl<K: ThemeKey> fmt::Debug for ThemeGroup<K>
where
    K::Values: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ThemeGroup")
            .field("key", &std::any::type_name::<K>())
            .field("values", &self.values)
            .finish()
    }
}
impl<K: ThemeKey> ThemeGroup<K> {
    pub fn defaults() -> Result<Self, Error> {
        Self::from_values(K::Values::default())
    }
    /// Apply construction-time overrides to fresh complete defaults. This
    /// callback is not retained or executed by a renderer or theme consumer.
    pub fn create(overrides: impl FnOnce(&mut K::Values)) -> Result<Self, Error> {
        let mut values = K::Values::default();
        overrides(&mut values);
        Self::from_values(values)
    }
    pub fn from_values(values: K::Values) -> Result<Self, Error> {
        if !PartialEq::eq(&values, &values) {
            return Err(Error::InvalidTheme);
        }
        K::validate(&values)?;
        Ok(Self {
            values: Arc::new(values),
            key: PhantomData,
        })
    }
    pub fn values(&self) -> &K::Values {
        &self.values
    }
}

/// An ordinary logical provider component without an extra render/layout box.
/// The single keyed child receives live Props and retains state across complete
/// theme replacements. Use a new component key to change its identity/type.
pub struct ThemeScope<K, C>(PhantomData<fn() -> (K, C)>);
pub struct ThemeScopeProps<K: ThemeKey, C: Component> {
    pub theme: ThemeGroup<K>,
    pub child: C::Props,
}
impl<K: ThemeKey, C: Component> PartialEq for ThemeScopeProps<K, C> {
    fn eq(&self, other: &Self) -> bool {
        self.theme == other.theme && self.child == other.child
    }
}
impl<K: ThemeKey, C: Component> Clone for ThemeScopeProps<K, C>
where
    C::Props: Clone,
{
    fn clone(&self) -> Self {
        Self {
            theme: self.theme.clone(),
            child: self.child.clone(),
        }
    }
}
impl<K: ThemeKey, C: Component> Component for ThemeScope<K, C>
where
    C::Props: Clone,
{
    type Props = ThemeScopeProps<K, C>;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let theme = cx.computed(0, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.theme.clone())
        })?;
        cx.provide_theme(theme)?;
        let child = cx.computed(1, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.child.clone())
        })?;
        cx.forward_child::<C>(ReadProp {
            signal: child,
            owner_local: PhantomData,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
