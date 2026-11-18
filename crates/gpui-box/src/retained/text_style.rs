//! Logical text inheritance, independent of font shaping and render backends.
use super::{
    Error,
    logical::*,
    reactive::{ComputedInputs, Signal},
    theme::*,
};
use std::{marker::PhantomData, sync::Arc};

/// Missing declarations inherit. Initial explicitly resets one property to the
/// framework default; it does not reset the other properties in the scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TextValue<T> {
    #[default]
    Inherit,
    Initial,
    Value(T),
}
impl<T: Clone> TextValue<T> {
    fn resolve(&self, inherited: &T, initial: &T) -> T {
        match self {
            Self::Inherit => inherited.clone(),
            Self::Initial => initial.clone(),
            Self::Value(value) => value.clone(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontSlant {
    #[default]
    Normal,
    Italic,
    Oblique,
}
/// Uniform-font input. Features/fallback lists and rich text are separate future
/// contracts; the owner font adapter retains its ordinary fallback policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontDescriptor {
    pub family: Arc<str>,
    pub weight: u16,
    pub slant: FontSlant,
}
impl Default for FontDescriptor {
    fn default() -> Self {
        Self {
            family: ".SystemUIFont".into(),
            weight: 400,
            slant: FontSlant::Normal,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextLineHeight {
    /// Logical pixels, inherited as an absolute length.
    Px(f32),
    /// Inherited as a multiplier; resolves against the consumer's font size.
    Multiple(f32),
}
impl Default for TextLineHeight {
    fn default() -> Self {
        Self::Multiple(2.0)
    }
}
/// Only these enumerated text properties inherit. Layout, backgrounds, borders,
/// opacity and interaction capabilities are deliberately absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextStyle {
    pub font_family: TextValue<Arc<str>>,
    pub font_weight: TextValue<u16>,
    pub font_slant: TextValue<FontSlant>,
    pub font_size: TextValue<f32>,
    pub line_height: TextValue<TextLineHeight>,
    pub color: TextValue<u32>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTextStyle {
    pub font: FontDescriptor,
    pub font_size: f32,
    pub line_height: TextLineHeight,
    pub color: u32,
}
impl Default for ResolvedTextStyle {
    fn default() -> Self {
        Self {
            font: FontDescriptor::default(),
            font_size: 16.0,
            line_height: TextLineHeight::default(),
            color: 0xffffffff,
        }
    }
}
/// Actual uniform shaping input. Color is independent, so a paint-only style
/// change cannot force new paragraph/glyph resources through this projection.
#[derive(Clone, Debug, PartialEq)]
pub struct TextTypography {
    pub font: FontDescriptor,
    pub font_size: f32,
    pub line_height: f32,
}
impl TextTypography {
    pub fn validate(&self) -> Result<(), Error> {
        if self.font.family.is_empty()
            || self.font.family.len() > 4096
            || self.font.family.contains('\0')
            || !(100..=900).contains(&self.font.weight)
            || !self.font_size.is_finite()
            || self.font_size <= 0.0
            || !self.line_height.is_finite()
            || self.line_height <= 0.0
        {
            return Err(Error::InvalidTextStyle);
        }
        Ok(())
    }
}
impl ResolvedTextStyle {
    pub fn typography(&self) -> Result<TextTypography, Error> {
        let line_height = match self.line_height {
            TextLineHeight::Px(px) => px,
            TextLineHeight::Multiple(factor) => factor * self.font_size,
        };
        let result = TextTypography {
            font: self.font.clone(),
            font_size: self.font_size,
            line_height,
        };
        result.validate()?;
        Ok(result)
    }
}
impl TextStyle {
    pub fn resolve(&self, inherited: &ResolvedTextStyle) -> Result<ResolvedTextStyle, Error> {
        // Invalid upstream data must not hide behind local overrides.
        inherited.typography()?;
        let initial = ResolvedTextStyle::default();
        let result = ResolvedTextStyle {
            font: FontDescriptor {
                family: self
                    .font_family
                    .resolve(&inherited.font.family, &initial.font.family),
                weight: self
                    .font_weight
                    .resolve(&inherited.font.weight, &initial.font.weight),
                slant: self
                    .font_slant
                    .resolve(&inherited.font.slant, &initial.font.slant),
            },
            font_size: self
                .font_size
                .resolve(&inherited.font_size, &initial.font_size),
            line_height: self
                .line_height
                .resolve(&inherited.line_height, &initial.line_height),
            color: self.color.resolve(&inherited.color, &initial.color),
        };
        result.typography()?;
        Ok(result)
    }
}
/// Built-in complete group for text defaults. Applications can provide a group
/// from ordinary theme selectors; partial TextStyleScope declarations merge only
/// the explicitly inheritable fields above, not arbitrary theme groups.
pub struct TextStyleKey;
impl ThemeKey for TextStyleKey {
    type Values = ResolvedTextStyle;
    fn validate(value: &Self::Values) -> Result<(), Error> {
        value.typography().map(|_| ())
    }
}
pub(super) fn prop<T>(signal: Signal<T>) -> ReadProp<T> {
    ReadProp {
        signal,
        owner_local: PhantomData,
    }
}
fn resolve_input(
    i: &ComputedInputs<'_>,
    input: ReadProp<TextStyle>,
    parent: Option<ReadProp<ThemeGroup<TextStyleKey>>>,
) -> Result<ResolvedTextStyle, Error> {
    let defaults = ResolvedTextStyle::default();
    let inherited = parent
        .map(|parent| i.read_prop(parent))
        .transpose()?
        .map_or(&defaults, ThemeGroup::values);
    i.read_prop(input)?.resolve(inherited)
}
impl LogicalEdit<'_, '_, '_> {
    pub fn resolve_text_style(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: ReadProp<TextStyle>,
    ) -> Result<ReadProp<ResolvedTextStyle>, Error> {
        let parent = self.try_use_theme::<TextStyleKey>(scope)?;
        let mut dependencies = vec![input.dependency()];
        dependencies.extend(parent.map(ReadProp::dependency));
        let value = self.computed(scope, key, &dependencies, move |i| {
            resolve_input(i, input, parent)
        })?;
        Ok(prop(value))
    }
}
impl ComponentContext<'_, '_, '_, '_> {
    pub fn resolve_text_style(
        &mut self,
        key: u64,
        input: ReadProp<TextStyle>,
    ) -> Result<ReadProp<ResolvedTextStyle>, Error> {
        if key == u64::MAX {
            return Err(Error::InvalidComponent);
        }
        let parent = self.try_use_theme::<TextStyleKey>()?;
        let mut dependencies = vec![input.dependency()];
        dependencies.extend(parent.map(ReadProp::dependency));
        let value = self.computed(key, &dependencies, move |i| resolve_input(i, input, parent))?;
        Ok(prop(value))
    }
}
/// Transparent partial text-style override, composed with the outer logical
/// text provider. Uses ordinary ThemeScope for the resolved complete group.
pub struct TextStyleScope<C>(PhantomData<fn() -> C>);
pub struct TextStyleScopeProps<C: Component> {
    pub style: TextStyle,
    pub child: C::Props,
}
impl<C: Component> Clone for TextStyleScopeProps<C>
where
    C::Props: Clone,
{
    fn clone(&self) -> Self {
        Self {
            style: self.style.clone(),
            child: self.child.clone(),
        }
    }
}
impl<C: Component> PartialEq for TextStyleScopeProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style && self.child == other.child
    }
}
impl<C: Component> Component for TextStyleScope<C>
where
    C::Props: Clone,
{
    type Props = TextStyleScopeProps<C>;
    fn mount(
        p: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let style = cx.computed(0, &[p.dependency()], move |i| {
            Ok(i.read_prop(p)?.style.clone())
        })?;
        // Resolve before entering the separate ThemeScope provider. Resolving
        // and declaring the same key on one scope would violate provider rules.
        let resolved = cx.resolve_text_style(1, prop(style))?;
        let child = cx.computed(2, &[p.dependency(), resolved.dependency()], move |i| {
            Ok(ThemeScopeProps::<TextStyleKey, C> {
                theme: ThemeGroup::from_values(i.read_prop(resolved)?.clone())?,
                child: i.read_prop(p)?.child.clone(),
            })
        })?;
        cx.forward_child::<ThemeScope<TextStyleKey, C>>(prop(child))
    }
}

#[cfg(test)]
mod tests;
