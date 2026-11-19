//! Owned typed token data. No user callbacks, renderer handles or global registry.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};
mod program;
pub use program::{TokenExpr, TokenOp, TokenProgram, TokenProjection};
#[cfg(test)]
mod tests;

/// Stable symbolic identity, independent of Rust TypeId or process addresses.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenId {
    pub group: Arc<str>,
    pub name: Arc<str>,
    pub kind: TokenKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    InvalidName,
    LimitExceeded,
    InvalidProgram,
    InvalidValue,
    TypeMismatch {
        expected: TokenKind,
        actual: TokenKind,
    },
    Duplicate(TokenId),
    Missing(TokenId),
    UnknownOverride(TokenId),
    Cycle(Vec<TokenId>),
}
impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "token error: {self:?}")
    }
}
impl std::error::Error for TokenError {}
impl From<TokenError> for Error {
    fn from(e: TokenError) -> Self {
        Self::InvalidTokens(e)
    }
}
mod sealed {
    pub trait Sealed {}
}
pub trait TokenType: sealed::Sealed + Clone + PartialEq + Send + Sync + 'static {
    const KIND: TokenKind;
    fn into_token(self) -> TokenValue;
    fn from_token(value: &TokenValue) -> Result<Self, TokenError>;
}
macro_rules! types {
    ($($variant:ident:$ty:ty),+ $(,)?)=>{
        #[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord,Hash)]
        pub enum TokenKind {$($variant),+}
        #[derive(Clone,Debug,PartialEq)]
        pub enum TokenValue {$($variant($ty)),+}
        impl TokenValue {pub fn kind(&self)->TokenKind {match self {$(Self::$variant(_)=>TokenKind::$variant),+}}}
        $(impl sealed::Sealed for $ty {}
        impl TokenType for $ty {
            const KIND:TokenKind=TokenKind::$variant;
            fn into_token(self)->TokenValue {TokenValue::$variant(self)}
            fn from_token(value:&TokenValue)->Result<Self,TokenError> {
                if let TokenValue::$variant(v)=value {Ok(v.clone())} else {Err(TokenError::TypeMismatch {expected:Self::KIND,actual:value.kind()})}
            }
        })+
    }
}
types! {
    Number:f32, Color:u32, Integer:i32, Length:LayoutLength, Dimension:LayoutDimension,
    Edges:LayoutEdges, Dimensions:LayoutSize<LayoutDimension>, Lengths:LayoutSize<LayoutLength>,
    Display:LayoutDisplay, Container:LayoutContainerType, Direction:LayoutDirection,
    Position:LayoutPosition, Intrinsic:LayoutSize<Option<f32>>, OptionalNumber:Option<f32>,
    FlexDirection:LayoutFlexDirection, FlexWrap:LayoutFlexWrap,
    ItemsAlignment:LayoutAlignment<LayoutAlignItems>, SelfAlignment:LayoutAlignment<LayoutAlignSelf>,
    ContentAlignment:LayoutAlignment<LayoutAlignContent>, Justification:LayoutAlignment<LayoutJustifyContent>,
    Grid:Option<Arc<GridStyle>>, GridPlacement:GridAxisPlacement,
    OptionalLengths:Option<LayoutSize<LayoutLength>>, Overflow:LayoutSize<LayoutOverflow>, BoxSizing:LayoutBoxSizing,
    Point:[f32;2], Clip:Option<Rect>, Decoration:QuadDecoration, Shadows:Arc<[BoxShadow]>,
    Font:text_style::FontDescriptor, Duration:std::time::Duration,
}
impl TokenValue {
    fn validate(&self) -> Result<(), TokenError> {
        if !PartialEq::eq(self, self) {
            return Err(TokenError::InvalidValue);
        }
        let finite = |v: f32| v.is_finite();
        let length = |v: &LayoutLength| match v {
            LayoutLength::Px(v) | LayoutLength::Percent(v) => finite(*v),
            LayoutLength::Calc { px, percent } => finite(*px) && finite(*percent),
            LayoutLength::Auto => true,
        };
        let mut layout = LayoutStyle::default();
        match self {
            // Content is a valid generic dimension token for FlexBasis. The
            // final property rejects it when used as width/height instead.
            Self::Dimension(v) => layout.flex_basis = *v,
            Self::Edges(v) => layout.margin = *v,
            Self::Dimensions(v) => layout.size = *v,
            Self::Lengths(v) => layout.gap = *v,
            Self::OptionalNumber(v) => layout.aspect_ratio = *v,
            Self::Intrinsic(v) => layout.contain_intrinsic_size = *v,
            Self::Grid(v) => {
                if v.as_ref().is_some_and(|grid| {
                    grid.columns.iter().chain(&grid.rows).fold(
                        grid.auto_columns.len().saturating_add(grid.auto_rows.len()),
                        |n, t| {
                            n.saturating_add(match t {
                                GridTemplate::Track(_) => 1,
                                GridTemplate::Repeat { tracks, .. } => {
                                    tracks.len().saturating_add(1)
                                }
                            })
                        },
                    ) > 4096
                }) {
                    return Err(TokenError::LimitExceeded);
                }
                layout.grid = v.clone();
            }
            Self::GridPlacement(v) => layout.grid_column = *v,
            Self::OptionalLengths(v) => layout.main_cross_gap = *v,
            Self::ItemsAlignment(v) => layout.align_items = *v,
            Self::SelfAlignment(v) => layout.align_self = *v,
            Self::ContentAlignment(v) => layout.align_content = *v,
            Self::Justification(v) => layout.justify_content = *v,
            _ => {}
        }
        layout.validate().map_err(|_| TokenError::InvalidValue)?;
        let mut properties = ui_ir::UiProperties::default();
        match self {
            Self::Clip(v) => properties.clip = *v,
            Self::Decoration(v) => properties.decoration = *v,
            Self::Shadows(v) => {
                if v.len() > 64 {
                    return Err(TokenError::LimitExceeded);
                }
                properties.shadows = v.clone()
            }
            _ => {}
        }
        ui_ir::validate_properties(&properties).map_err(|_| TokenError::InvalidValue)?;
        match self {
            Self::Number(v) if !finite(*v) => Err(TokenError::InvalidValue),
            Self::Length(v) if !length(v) => Err(TokenError::InvalidValue),
            Self::Font(f)
                if f.family.is_empty()
                    || f.family.len() > 1024
                    || f.family.contains('\0')
                    || !(100..=900).contains(&f.weight) =>
            {
                Err(TokenError::InvalidValue)
            }
            Self::Point(v) if !v.iter().all(|v| finite(*v)) => Err(TokenError::InvalidValue),
            _ => Ok(()),
        }
    }
}
/// Token input types remain distinct even when their physical values overlap.
/// ```compile_fail
/// use gpui_box::retained::{tokens::TokenKey,style::{StyleDeclaration,StyleRule}};
/// let color=TokenKey::<u32>::new("app","color").unwrap();
/// let width=StyleDeclaration::Width(StyleRule::expression(color.expression()));
/// ```
pub struct TokenKey<T> {
    id: TokenId,
    marker: PhantomData<fn() -> T>,
}
impl<T> Clone for TokenKey<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            marker: PhantomData,
        }
    }
}
impl<T> std::fmt::Debug for TokenKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id.fmt(f)
    }
}
impl<T> PartialEq for TokenKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<T> Eq for TokenKey<T> {}
impl<T: TokenType> TokenKey<T> {
    pub fn new(group: impl Into<Arc<str>>, name: impl Into<Arc<str>>) -> Result<Self, TokenError> {
        let id = TokenId {
            group: group.into(),
            name: name.into(),
            kind: T::KIND,
        };
        id.validate()?;
        Ok(Self {
            id,
            marker: PhantomData,
        })
    }
    pub fn id(&self) -> &TokenId {
        &self.id
    }
    pub fn expression(&self) -> TokenExpr<T> {
        TokenExpr::reference(self.clone())
    }
}
impl TokenId {
    fn validate(&self) -> Result<(), TokenError> {
        if [&self.group, &self.name]
            .iter()
            .any(|n| n.is_empty() || n.len() > 1024 || n.contains('\0'))
        {
            Err(TokenError::InvalidName)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TokenDefinition {
    pub id: TokenId,
    pub expression: TokenProgram,
}
impl TokenDefinition {
    pub fn new<T: TokenType>(key: TokenKey<T>, expression: TokenExpr<T>) -> Self {
        Self {
            id: key.id,
            expression: expression.program().clone(),
        }
    }
    pub fn value<T: TokenType>(key: TokenKey<T>, value: T) -> Self {
        Self::new(key, TokenExpr::value(value))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenLimits {
    pub max_tokens: usize,
    pub max_operations: usize,
    pub max_operations_per_expression: usize,
    pub max_expression_depth: usize,
}
impl Default for TokenLimits {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            max_operations: 65536,
            max_operations_per_expression: 4096,
            max_expression_depth: 64,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TokenSchema {
    defaults: Arc<BTreeMap<TokenId, TokenProgram>>,
    limits: TokenLimits,
}
#[derive(Clone, Debug)]
pub struct TokenSnapshot(Arc<BTreeMap<TokenId, TokenValue>>);
impl Default for TokenSnapshot {
    fn default() -> Self {
        static EMPTY: std::sync::OnceLock<Arc<BTreeMap<TokenId, TokenValue>>> =
            std::sync::OnceLock::new();
        Self(EMPTY.get_or_init(|| Arc::new(BTreeMap::new())).clone())
    }
}
impl PartialEq for TokenSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
impl TokenSnapshot {
    pub fn select(&self, ids: &BTreeSet<TokenId>) -> Result<Self, TokenError> {
        let mut values = BTreeMap::new();
        for id in ids {
            values.insert(id.clone(), self.get(id)?.clone());
        }
        Ok(Self(Arc::new(values)))
    }
    pub fn read<T: TokenType>(&self, key: &TokenKey<T>) -> Result<T, TokenError> {
        T::from_token(self.get(&key.id)?)
    }
    pub fn values(&self) -> &BTreeMap<TokenId, TokenValue> {
        &self.0
    }
    fn get(&self, id: &TokenId) -> Result<&TokenValue, TokenError> {
        self.0
            .get(id)
            .ok_or_else(|| TokenError::Missing(id.clone()))
    }
}
impl TokenSchema {
    pub fn new(defaults: Vec<TokenDefinition>) -> Result<Self, TokenError> {
        Self::with_limits(defaults, TokenLimits::default())
    }
    pub fn with_limits(
        defaults: Vec<TokenDefinition>,
        limits: TokenLimits,
    ) -> Result<Self, TokenError> {
        if defaults.len() > limits.max_tokens {
            return Err(TokenError::LimitExceeded);
        }
        let mut map = BTreeMap::new();
        for definition in defaults {
            definition.id.validate()?;
            let id = definition.id;
            if map.insert(id.clone(), definition.expression).is_some() {
                return Err(TokenError::Duplicate(id));
            }
        }
        let schema = Self {
            defaults: Arc::new(map),
            limits,
        };
        schema.resolve(vec![])?;
        Ok(schema)
    }
    /// Replaces explicitly listed definitions against fresh complete defaults.
    /// Omitted overrides do not retain a preceding theme's values.
    pub fn resolve(&self, overrides: Vec<TokenDefinition>) -> Result<TokenSnapshot, TokenError> {
        if overrides.len() > self.limits.max_tokens {
            return Err(TokenError::LimitExceeded);
        }
        let mut definitions = (*self.defaults).clone();
        let mut seen = BTreeSet::new();
        for d in overrides {
            if !definitions.contains_key(&d.id) {
                return Err(TokenError::UnknownOverride(d.id));
            }
            if !seen.insert(d.id.clone()) {
                return Err(TokenError::Duplicate(d.id));
            }
            definitions.insert(d.id, d.expression);
        }
        let mut deps = BTreeMap::new();
        let mut dependents: BTreeMap<TokenId, Vec<TokenId>> = BTreeMap::new();
        let mut work = 0usize;
        for (id, p) in &definitions {
            let kind = p.validate(self.limits)?;
            if kind != id.kind {
                return Err(TokenError::TypeMismatch {
                    expected: id.kind,
                    actual: kind,
                });
            }
            work = work.saturating_add(p.operations().len());
            if work > self.limits.max_operations {
                return Err(TokenError::LimitExceeded);
            }
            let references = p.references();
            for r in &references {
                if !definitions.contains_key(r) {
                    return Err(TokenError::Missing(r.clone()));
                }
                dependents.entry(r.clone()).or_default().push(id.clone());
            }
            deps.insert(id.clone(), references.len());
        }
        let mut ready: BTreeSet<_> = deps
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut values = TokenSnapshot::default();
        while let Some(id) = ready.pop_first() {
            let v = definitions[&id].evaluate(&values)?;
            Arc::make_mut(&mut values.0).insert(id.clone(), v);
            for next in dependents.get(&id).into_iter().flatten() {
                let count = deps.get_mut(next).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(next.clone());
                }
            }
        }
        if values.0.len() != definitions.len() {
            // Follow unresolved dependency edges until an actual cycle repeats.
            let mut current = deps.iter().find(|(_, n)| **n > 0).unwrap().0.clone();
            let mut path = vec![];
            let mut positions = BTreeMap::new();
            loop {
                if let Some(start) = positions.insert(current.clone(), path.len()) {
                    let mut cycle = path[start..].to_vec();
                    cycle.push(current);
                    return Err(TokenError::Cycle(cycle));
                }
                path.push(current.clone());
                current = definitions[&current]
                    .references()
                    .into_iter()
                    .find(|r| deps[r] > 0)
                    .unwrap();
            }
        }
        Ok(values)
    }
}
/// Complete logical token environment; ordinary ThemeScope/provider rules apply.
pub struct TokenThemeKey;
impl theme::ThemeKey for TokenThemeKey {
    type Values = TokenSnapshot;
}
impl logical::ComponentContext<'_, '_, '_, '_> {
    pub fn use_tokens(
        &mut self,
    ) -> Result<logical::ReadProp<theme::ThemeGroup<TokenThemeKey>>, Error> {
        self.use_theme::<TokenThemeKey>()
    }
    pub fn select_token<T: TokenType>(
        &mut self,
        key: u64,
        input: logical::ReadProp<theme::ThemeGroup<TokenThemeKey>>,
        token: TokenKey<T>,
    ) -> Result<logical::ReadProp<T>, Error> {
        self.select_theme(key, input, move |values| {
            values.read(&token).map_err(Error::from)
        })
    }
}
