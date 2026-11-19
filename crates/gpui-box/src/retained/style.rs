//! Owned, typed style composition. Select a complete rule per atomic property
//! first, then evaluate its ordered conditions. No callbacks or CSS parser.
use super::tokens::{TokenExpr, TokenLimits, TokenProjection, TokenSnapshot, TokenType};
use super::*;
use super::{responsive::LayoutEnvironment, ui_ir::UiProperties};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleState {
    pub hovered: bool,
    pub pressed: bool,
    pub focus_visible: bool,
    pub disabled: bool,
    pub selected: bool,
}

/// Flat conjunction. Width is the supplied environment's logical viewport,
/// independent of layout constraints/paint viewport. max_width is exclusive.
/// Unknown optional capabilities do not match either true or false requirements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleCondition {
    pub hovered: Option<bool>,
    pub pressed: Option<bool>,
    pub focus_visible: Option<bool>,
    pub disabled: Option<bool>,
    pub selected: Option<bool>,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub direction: Option<LayoutDirection>,
    pub reduced_motion: Option<bool>,
    pub touch: Option<bool>,
    pub pointer: Option<bool>,
}
impl StyleCondition {
    fn dependencies(&self) -> StyleDependencies {
        StyleDependencies {
            hovered: self.hovered.is_some(),
            pressed: self.pressed.is_some(),
            focus_visible: self.focus_visible.is_some(),
            disabled: self.disabled.is_some(),
            selected: self.selected.is_some(),
            viewport_width: self.min_width.is_some() || self.max_width.is_some(),
            direction: self.direction.is_some(),
            reduced_motion: self.reduced_motion.is_some(),
            touch: self.touch.is_some(),
            pointer: self.pointer.is_some(),
        }
    }
    fn validate(&self) -> Result<(), StyleError> {
        if [self.min_width, self.max_width]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0)
            || self
                .min_width
                .zip(self.max_width)
                .is_some_and(|(min, max)| min >= max)
        {
            return Err(StyleError::InvalidCondition);
        }
        Ok(())
    }
    fn matches(&self, environment: &LayoutEnvironment, state: StyleState) -> bool {
        let state_pairs = [
            (self.hovered, state.hovered),
            (self.pressed, state.pressed),
            (self.focus_visible, state.focus_visible),
            (self.disabled, state.disabled),
            (self.selected, state.selected),
        ];
        state_pairs
            .iter()
            .all(|(wanted, actual)| wanted.is_none_or(|v| v == *actual))
            && self
                .min_width
                .is_none_or(|min| environment.layout_viewport[0] >= min)
            && self
                .max_width
                .is_none_or(|max| environment.layout_viewport[0] < max)
            && self.direction.is_none_or(|v| v == environment.direction)
            && [
                (self.reduced_motion, environment.reduced_motion),
                (self.touch, environment.touch),
                (self.pointer, environment.pointer),
            ]
            .iter()
            .all(|(wanted, actual)| wanted.is_none_or(|v| *actual == Some(v)))
    }
}

/// Inputs read by the winning complete property rules. Overridden rules do not
/// contribute dependencies. These describe pure style evaluation, not layout
/// dependencies, event subscriptions or presentation-thread safety.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleDependencies {
    pub hovered: bool,
    pub pressed: bool,
    pub focus_visible: bool,
    pub disabled: bool,
    pub selected: bool,
    pub viewport_width: bool,
    pub direction: bool,
    pub reduced_motion: bool,
    pub touch: bool,
    pub pointer: bool,
}
impl StyleDependencies {
    fn include(&mut self, other: Self) {
        self.hovered |= other.hovered;
        self.pressed |= other.pressed;
        self.focus_visible |= other.focus_visible;
        self.disabled |= other.disabled;
        self.selected |= other.selected;
        self.viewport_width |= other.viewport_width;
        self.direction |= other.direction;
        self.reduced_motion |= other.reduced_motion;
        self.touch |= other.touch;
        self.pointer |= other.pointer;
    }
    pub(crate) fn state_changed(self, before: StyleState, after: StyleState) -> bool {
        (self.hovered && before.hovered != after.hovered)
            || (self.pressed && before.pressed != after.pressed)
            || (self.focus_visible && before.focus_visible != after.focus_visible)
            || (self.disabled && before.disabled != after.disabled)
            || (self.selected && before.selected != after.selected)
    }
    pub(crate) fn environment_inputs(self) -> [bool; 5] {
        [
            self.viewport_width,
            self.direction,
            self.reduced_motion,
            self.touch,
            self.pointer,
        ]
    }
}

/// Initial resets to the framework default, not the earlier composed value or
/// the node's authored base. No declaration means leave that base untouched.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleValue<T> {
    Value(T),
    Initial,
    Expression(TokenExpr<T>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct StyleCase<T> {
    pub condition: StyleCondition,
    pub value: StyleValue<T>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule<T> {
    pub default: StyleValue<T>,
    pub cases: Vec<StyleCase<T>>,
}
impl<T> StyleRule<T> {
    pub fn value(value: T) -> Self {
        Self {
            default: StyleValue::Value(value),
            cases: vec![],
        }
    }
    pub fn initial() -> Self {
        Self {
            default: StyleValue::Initial,
            cases: vec![],
        }
    }
    pub fn when(mut self, condition: StyleCondition, value: T) -> Self {
        self.cases.push(StyleCase {
            condition,
            value: StyleValue::Value(value),
        });
        self
    }
    pub fn when_initial(mut self, condition: StyleCondition) -> Self {
        self.cases.push(StyleCase {
            condition,
            value: StyleValue::Initial,
        });
        self
    }
    fn selected(
        &self,
        environment: &LayoutEnvironment,
        state: StyleState,
    ) -> (&StyleValue<T>, Option<usize>) {
        self.cases
            .iter()
            .enumerate()
            .rev()
            .find(|(_, case)| case.condition.matches(environment, state))
            .map_or((&self.default, None), |(i, case)| (&case.value, Some(i)))
    }
}
impl<T: TokenType> StyleRule<T> {
    pub fn expression(expression: TokenExpr<T>) -> Self {
        Self {
            default: StyleValue::Expression(expression),
            cases: vec![],
        }
    }
    pub fn when_expression(mut self, condition: StyleCondition, expression: TokenExpr<T>) -> Self {
        self.cases.push(StyleCase {
            condition,
            value: StyleValue::Expression(expression),
        });
        self
    }
    fn map<U: TokenType>(
        &self,
        map: impl Fn(T) -> U,
        field: TokenProjection,
    ) -> Result<StyleRule<U>, StyleError> {
        let value = |v: &StyleValue<T>| -> Result<StyleValue<U>, StyleError> {
            match v {
                StyleValue::Value(v) => Ok(StyleValue::Value(map(v.clone()))),
                StyleValue::Initial => Ok(StyleValue::Initial),
                StyleValue::Expression(v) => Ok(StyleValue::Expression(TokenExpr::from_program(
                    tokens::TokenProgram::projected(v.program(), field)?,
                )?)),
            }
        };
        Ok(StyleRule {
            default: value(&self.default)?,
            cases: self
                .cases
                .iter()
                .map(|c| {
                    Ok(StyleCase {
                        condition: c.condition.clone(),
                        value: value(&c.value)?,
                    })
                })
                .collect::<Result<_, StyleError>>()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleLimits {
    pub max_declarations: usize,
    pub max_cases_per_declaration: usize,
    pub max_grid_track_entries_per_value: usize,
    pub max_shadows_per_value: usize,
    pub max_name_bytes: usize,
}
impl Default for StyleLimits {
    fn default() -> Self {
        Self {
            max_declarations: 256,
            max_cases_per_declaration: 64,
            max_grid_track_entries_per_value: 4_096,
            max_shadows_per_value: 64,
            max_name_bytes: 1_024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleError {
    DuplicateProperty(StyleProperty),
    InvalidCondition,
    LimitExceeded,
    NonReflexiveValue,
    InvalidValue(Error),
    Token(tokens::TokenError),
}
impl From<tokens::TokenError> for StyleError {
    fn from(e: tokens::TokenError) -> Self {
        Self::Token(e)
    }
}
impl From<Error> for StyleError {
    fn from(e: Error) -> Self {
        Self::InvalidValue(e)
    }
}
impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "style error: {self:?}")
    }
}
impl std::error::Error for StyleError {}

/// Generated typed atoms keep key, value, assignment and validation in one
/// definition. Shorthands expand before duplicate checks/composition.
macro_rules! properties {
    ($($name:ident: $ty:ty => $target:ident.$($field:ident).+),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum StyleProperty { $($name),+ }
        #[derive(Clone, Debug, PartialEq)]
        pub enum StyleDeclaration {
            $($name(StyleRule<$ty>)),+,
            Padding(StyleRule<LayoutEdges>), Margin(StyleRule<LayoutEdges>),
            Border(StyleRule<LayoutEdges>), Inset(StyleRule<LayoutEdges>),
            Size(StyleRule<LayoutSize<LayoutDimension>>),
            MinSize(StyleRule<LayoutSize<LayoutDimension>>),
            MaxSize(StyleRule<LayoutSize<LayoutDimension>>),
            Gap(StyleRule<LayoutSize<LayoutLength>>),
        }
        impl StyleDeclaration {
            fn token_dependencies(&self) -> std::collections::BTreeSet<tokens::TokenId> {
                match self {$(Self::$name(rule)=>std::iter::once(&rule.default).chain(rule.cases.iter().map(|c|&c.value))
                    .filter_map(|v|if let StyleValue::Expression(e)=v {Some(e.program().references())}else {None}).flatten().collect()),+,_=>unreachable!("expanded shorthand")}
            }
            fn expression_work(&self)->usize {
                match self {$(Self::$name(rule)=>std::iter::once(&rule.default).chain(rule.cases.iter().map(|c|&c.value))
                    .map(|v|if let StyleValue::Expression(e)=v {e.program().operations().len()}else {0}).sum()),+,_=>unreachable!("expanded shorthand")}
            }
            fn validate_resolved_bounds(&self,output:&StyleResolution,limits:StyleLimits)->Result<(),StyleError> {
                match self {
                    Self::Grid(_) if output.layout.grid.as_ref().is_some_and(|g|grid_entries(g)>limits.max_grid_track_entries_per_value)=>return Err(StyleError::LimitExceeded),
                    Self::Shadows(_) if output.properties.shadows.len()>limits.max_shadows_per_value=>return Err(StyleError::LimitExceeded),
                    _=>{},
                }
                Ok(())
            }
            fn dependencies(&self) -> StyleDependencies {
                match self { $(Self::$name(rule) => {
                    let mut dependencies = StyleDependencies::default();
                    for case in &rule.cases { dependencies.include(case.condition.dependencies()); }
                    dependencies
                }),+, _ => unreachable!("expanded shorthand") }
            }
            fn case_count(&self) -> usize { match self {
                $(Self::$name(r) => r.cases.len()),+,
                Self::Padding(r) | Self::Margin(r) | Self::Border(r) | Self::Inset(r) => r.cases.len(),
                Self::Size(r) | Self::MinSize(r) | Self::MaxSize(r) => r.cases.len(),
                Self::Gap(r) => r.cases.len(),
            } }
            fn value_work(&self) -> (usize, usize) {
                match self {
                    Self::Grid(rule) => (std::iter::once(&rule.default).chain(rule.cases.iter().map(|c| &c.value))
                        .map(|v| match v { StyleValue::Value(Some(grid)) => grid_entries(grid), _ => 0 }).max().unwrap_or(0), 0),
                    Self::Shadows(rule) => (0, std::iter::once(&rule.default).chain(rule.cases.iter().map(|c| &c.value))
                        .map(|v| match v { StyleValue::Value(shadows) => shadows.len(), _ => 0 }).max().unwrap_or(0)),
                    _ => (0, 0),
                }
            }
            fn key(&self) -> StyleProperty { match self { $(Self::$name(_) => StyleProperty::$name),+, _ => unreachable!("expanded shorthand") } }
            fn validate(&self, limits: StyleLimits) -> Result<(), StyleError> {
                match self { $(Self::$name(rule) => {
                    if rule.cases.len() > limits.max_cases_per_declaration { return Err(StyleError::LimitExceeded); }
                    if !PartialEq::eq(rule, rule) { return Err(StyleError::NonReflexiveValue); }
                    for value in std::iter::once(&rule.default).chain(rule.cases.iter().map(|c| &c.value)) {
                        let mut output = StyleResolution::default();
                        if let StyleValue::Value(value) = value { output.$target.$($field).+ = value.clone(); }
                        if let StyleValue::Expression(expression)=value {expression.program().validate(TokenLimits::default())?;}
                        validate_output(&output)?;
                    }
                    for case in &rule.cases { case.condition.validate()?; }
                }),+, _ => unreachable!("expanded shorthand") }
                Ok(())
            }
            fn apply(&self, output: &mut StyleResolution, environment: &LayoutEnvironment, state: StyleState, tokens:&TokenSnapshot) -> Result<(Option<usize>, bool),StyleError> {
                match self { $(Self::$name(rule) => {
                    let (value, case) = rule.selected(environment, state);
                    let initial = matches!(value, StyleValue::Initial);
                    output.$target.$($field).+ = match value {
                        StyleValue::Value(value) => value.clone(),
                        StyleValue::Initial => StyleResolution::default().$target.$($field).+,
                        StyleValue::Expression(expression) => expression.evaluate(tokens)?,
                    };
                    Ok((case, initial))
                }),+, _ => unreachable!("expanded shorthand") }
            }
        }
    }
}
properties! {
    Order: i32 => layout.order,
    Display: LayoutDisplay => layout.display,
    ContainerType: LayoutContainerType => layout.container_type,
    ContainIntrinsicSize: LayoutSize<Option<f32>> => layout.contain_intrinsic_size,
    Direction: LayoutDirection => layout.direction,
    Position: LayoutPosition => layout.position,
    InsetLeft: LayoutLength => layout.inset.left, InsetRight: LayoutLength => layout.inset.right,
    InsetTop: LayoutLength => layout.inset.top, InsetBottom: LayoutLength => layout.inset.bottom,
    Width: LayoutDimension => layout.size.width, Height: LayoutDimension => layout.size.height,
    MinWidth: LayoutDimension => layout.min_size.width, MinHeight: LayoutDimension => layout.min_size.height,
    MaxWidth: LayoutDimension => layout.max_size.width, MaxHeight: LayoutDimension => layout.max_size.height,
    AspectRatio: Option<f32> => layout.aspect_ratio,
    MarginLeft: LayoutLength => layout.margin.left, MarginRight: LayoutLength => layout.margin.right,
    MarginTop: LayoutLength => layout.margin.top, MarginBottom: LayoutLength => layout.margin.bottom,
    PaddingLeft: LayoutLength => layout.padding.left, PaddingRight: LayoutLength => layout.padding.right,
    PaddingTop: LayoutLength => layout.padding.top, PaddingBottom: LayoutLength => layout.padding.bottom,
    BorderLeft: LayoutLength => layout.border.left, BorderRight: LayoutLength => layout.border.right,
    BorderTop: LayoutLength => layout.border.top, BorderBottom: LayoutLength => layout.border.bottom,
    FlexDirection: LayoutFlexDirection => layout.flex_direction,
    FlexWrap: LayoutFlexWrap => layout.flex_wrap,
    FlexBasis: LayoutDimension => layout.flex_basis,
    FlexGrow: f32 => layout.flex_grow, FlexShrink: f32 => layout.flex_shrink,
    AlignItems: LayoutAlignment<LayoutAlignItems> => layout.align_items,
    AlignSelf: LayoutAlignment<LayoutAlignSelf> => layout.align_self,
    JustifyItems: LayoutAlignment<LayoutAlignItems> => layout.justify_items,
    JustifySelf: LayoutAlignment<LayoutAlignSelf> => layout.justify_self,
    Grid: Option<Arc<GridStyle>> => layout.grid,
    GridColumn: GridAxisPlacement => layout.grid_column, GridRow: GridAxisPlacement => layout.grid_row,
    AlignContent: LayoutAlignment<LayoutAlignContent> => layout.align_content,
    JustifyContent: LayoutAlignment<LayoutJustifyContent> => layout.justify_content,
    ColumnGap: LayoutLength => layout.gap.width, RowGap: LayoutLength => layout.gap.height,
    MainCrossGap: Option<LayoutSize<LayoutLength>> => layout.main_cross_gap,
    Overflow: LayoutSize<LayoutOverflow> => layout.overflow,
    ScrollbarWidth: f32 => layout.scrollbar_width,
    BoxSizing: LayoutBoxSizing => layout.box_sizing,
    Color: u32 => properties.color, Translation: [f32; 2] => properties.translation,
    Clip: Option<Rect> => properties.clip, Scroll: [f32; 2] => properties.scroll,
    Opacity: f32 => properties.opacity, Decoration: QuadDecoration => properties.decoration,
    Shadows: Arc<[BoxShadow]> => properties.shadows,
}

impl StyleDeclaration {
    fn expand(&self) -> Result<Vec<Self>, StyleError> {
        use StyleDeclaration::*;
        Ok(match self {
            Padding(r) => vec![
                PaddingLeft(r.map(|v| v.left, TokenProjection::Left)?),
                PaddingRight(r.map(|v| v.right, TokenProjection::Right)?),
                PaddingTop(r.map(|v| v.top, TokenProjection::Top)?),
                PaddingBottom(r.map(|v| v.bottom, TokenProjection::Bottom)?),
            ],
            Margin(r) => vec![
                MarginLeft(r.map(|v| v.left, TokenProjection::Left)?),
                MarginRight(r.map(|v| v.right, TokenProjection::Right)?),
                MarginTop(r.map(|v| v.top, TokenProjection::Top)?),
                MarginBottom(r.map(|v| v.bottom, TokenProjection::Bottom)?),
            ],
            Border(r) => vec![
                BorderLeft(r.map(|v| v.left, TokenProjection::Left)?),
                BorderRight(r.map(|v| v.right, TokenProjection::Right)?),
                BorderTop(r.map(|v| v.top, TokenProjection::Top)?),
                BorderBottom(r.map(|v| v.bottom, TokenProjection::Bottom)?),
            ],
            Inset(r) => vec![
                InsetLeft(r.map(|v| v.left, TokenProjection::Left)?),
                InsetRight(r.map(|v| v.right, TokenProjection::Right)?),
                InsetTop(r.map(|v| v.top, TokenProjection::Top)?),
                InsetBottom(r.map(|v| v.bottom, TokenProjection::Bottom)?),
            ],
            Size(r) => vec![
                Width(r.map(|v| v.width, TokenProjection::Width)?),
                Height(r.map(|v| v.height, TokenProjection::Height)?),
            ],
            MinSize(r) => vec![
                MinWidth(r.map(|v| v.width, TokenProjection::Width)?),
                MinHeight(r.map(|v| v.height, TokenProjection::Height)?),
            ],
            MaxSize(r) => vec![
                MaxWidth(r.map(|v| v.width, TokenProjection::Width)?),
                MaxHeight(r.map(|v| v.height, TokenProjection::Height)?),
            ],
            Gap(r) => vec![
                ColumnGap(r.map(|v| v.width, TokenProjection::Width)?),
                RowGap(r.map(|v| v.height, TokenProjection::Height)?),
            ],
            atom => vec![atom.clone()],
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Sheet {
    name: String,
    declarations: Vec<StyleDeclaration>,
    work: (usize, usize, usize, usize),
    expression_work: usize,
    limits: StyleLimits,
}
/// Owned validated immutable definition. Clone shares declarations; equal new
/// definitions compare by value. No global registry, callback or lifetime leak.
#[derive(Clone, Debug)]
pub struct StyleRef(Arc<Sheet>);
impl PartialEq for StyleRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
impl StyleRef {
    pub fn new(
        name: impl Into<String>,
        declarations: Vec<StyleDeclaration>,
    ) -> Result<Self, StyleError> {
        Self::with_limits(name, declarations, StyleLimits::default())
    }
    pub fn with_limits(
        name: impl Into<String>,
        declarations: Vec<StyleDeclaration>,
        limits: StyleLimits,
    ) -> Result<Self, StyleError> {
        let name = name.into();
        if name.len() > limits.max_name_bytes {
            return Err(StyleError::LimitExceeded);
        }
        if declarations.len() > limits.max_declarations {
            return Err(StyleError::LimitExceeded);
        }
        let mut expanded = Vec::new();
        let mut keys = std::collections::BTreeSet::new();
        for declaration in declarations {
            if declaration.case_count() > limits.max_cases_per_declaration {
                return Err(StyleError::LimitExceeded);
            }
            let (tracks, shadows) = declaration.value_work();
            if tracks > limits.max_grid_track_entries_per_value
                || shadows > limits.max_shadows_per_value
            {
                return Err(StyleError::LimitExceeded);
            }
            for atom in declaration.expand()? {
                if expanded.len() >= limits.max_declarations {
                    return Err(StyleError::LimitExceeded);
                }
                atom.validate(limits)?;
                if !keys.insert(atom.key()) {
                    return Err(StyleError::DuplicateProperty(atom.key()));
                }
                expanded.push(atom);
            }
        }
        let mut work = (expanded.len(), 0usize, 0usize, 0usize);
        let mut expression_work = 0usize;
        for atom in &expanded {
            expression_work = expression_work.saturating_add(atom.expression_work());
            if expression_work > TokenLimits::default().max_operations {
                return Err(StyleError::LimitExceeded);
            }
            work.1 += atom.case_count();
            let values = atom.value_work();
            work.2 = work.2.max(values.0);
            work.3 = work.3.max(values.1);
        }
        Ok(Self(Arc::new(Sheet {
            name,
            declarations: expanded,
            work,
            expression_work,
            limits,
        })))
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    /// Validated atomic declarations, in expansion order, for diagnostics and
    /// frontend adapters. Mutation requires constructing a new StyleRef.
    pub fn declarations(&self) -> &[StyleDeclaration] {
        &self.0.declarations
    }
    pub(crate) fn work(&self) -> (usize, usize, usize, usize) {
        self.0.work
    }
    pub(crate) fn expression_work(&self) -> usize {
        self.0.expression_work
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleWinner {
    pub style_index: usize,
    /// Index in the expanded atomic declaration list, not shorthand input index.
    pub declaration_index: usize,
    pub case_index: Option<usize>,
    pub initial: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleResolution {
    pub layout: LayoutStyle,
    pub properties: UiProperties,
    pub winners: BTreeMap<StyleProperty, StyleWinner>,
}

fn validate_output(output: &StyleResolution) -> Result<(), StyleError> {
    if !PartialEq::eq(output, output) {
        return Err(StyleError::NonReflexiveValue);
    }
    output.layout.validate()?;
    super::ui_ir::validate_properties(&output.properties).map_err(|e| match e {
        super::ui_ir::UiError::Retained(e) => StyleError::InvalidValue(e),
        _ => unreachable!("property validation returns retained errors"),
    })
}

fn grid_entries(grid: &GridStyle) -> usize {
    grid.columns.iter().chain(&grid.rows).fold(
        grid.auto_columns.len().saturating_add(grid.auto_rows.len()),
        |count, track| {
            count.saturating_add(match track {
                GridTemplate::Track(_) => 1,
                GridTemplate::Repeat { tracks, .. } => tracks.len().saturating_add(1),
            })
        },
    )
}

/// Composition chooses the last declaration for each property, including its
/// entire default/case rule. Only that rule is then evaluated; earlier states
/// never leak through an external replacement. Unmentioned fields keep base.
pub fn resolve(
    base_layout: &LayoutStyle,
    base_properties: &UiProperties,
    styles: &[StyleRef],
    environment: &LayoutEnvironment,
    state: StyleState,
) -> Result<StyleResolution, StyleError> {
    StylePlan::new(styles).resolve(base_layout, base_properties, environment, state)
}

/// Owned compiled selection of one complete rule per atomic property. Retains
/// StyleRef storage rather than cloning rule values. Reuse across state and
/// environment changes; constructing a new composition produces a new plan.
#[derive(Clone, Debug)]
pub struct StylePlan {
    styles: Vec<StyleRef>,
    selected: Vec<(StyleProperty, usize, usize)>,
    dependencies: StyleDependencies,
    token_dependencies: std::collections::BTreeSet<tokens::TokenId>,
}
impl StylePlan {
    pub fn new(styles: &[StyleRef]) -> Self {
        let mut selected = BTreeMap::new();
        for (style_index, style) in styles.iter().enumerate() {
            for (declaration_index, declaration) in style.0.declarations.iter().enumerate() {
                selected.insert(declaration.key(), (style_index, declaration_index));
            }
        }
        let mut dependencies = StyleDependencies::default();
        let mut token_dependencies = std::collections::BTreeSet::new();
        let selected = selected
            .into_iter()
            .map(|(key, (s, d))| {
                dependencies.include(styles[s].0.declarations[d].dependencies());
                token_dependencies.extend(styles[s].0.declarations[d].token_dependencies());
                (key, s, d)
            })
            .collect();
        Self {
            styles: styles.to_vec(),
            selected,
            dependencies,
            token_dependencies,
        }
    }
    pub fn dependencies(&self) -> StyleDependencies {
        self.dependencies
    }
    /// References from winning complete property rules, including inactive cases.
    /// This is a conservative invalidation set, not the active condition trace.
    pub fn token_dependencies(&self) -> &std::collections::BTreeSet<tokens::TokenId> {
        &self.token_dependencies
    }
    pub(crate) fn token_inputs_changed(
        &self,
        before: &TokenSnapshot,
        after: &TokenSnapshot,
    ) -> bool {
        self.token_dependencies
            .iter()
            .any(|id| before.values().get(id) != after.values().get(id))
    }
    pub fn resolve(
        &self,
        base_layout: &LayoutStyle,
        base_properties: &UiProperties,
        environment: &LayoutEnvironment,
        state: StyleState,
    ) -> Result<StyleResolution, StyleError> {
        self.evaluate(
            base_layout,
            base_properties,
            environment,
            state,
            true,
            &TokenSnapshot::default(),
        )
    }
    /// Resolve effective values without allocating winner diagnostics. Suitable
    /// for ordinary computed bindings that only need visual equality.
    pub fn resolve_values(
        &self,
        base_layout: &LayoutStyle,
        base_properties: &UiProperties,
        environment: &LayoutEnvironment,
        state: StyleState,
    ) -> Result<StyleResolution, StyleError> {
        self.evaluate(
            base_layout,
            base_properties,
            environment,
            state,
            false,
            &TokenSnapshot::default(),
        )
    }
    pub fn resolve_tokens(
        &self,
        base_layout: &LayoutStyle,
        base_properties: &UiProperties,
        environment: &LayoutEnvironment,
        state: StyleState,
        tokens: &TokenSnapshot,
    ) -> Result<StyleResolution, StyleError> {
        self.evaluate(
            base_layout,
            base_properties,
            environment,
            state,
            true,
            tokens,
        )
    }
    pub fn resolve_token_values(
        &self,
        base_layout: &LayoutStyle,
        base_properties: &UiProperties,
        environment: &LayoutEnvironment,
        state: StyleState,
        tokens: &TokenSnapshot,
    ) -> Result<StyleResolution, StyleError> {
        self.evaluate(
            base_layout,
            base_properties,
            environment,
            state,
            false,
            tokens,
        )
    }
    fn evaluate(
        &self,
        base_layout: &LayoutStyle,
        base_properties: &UiProperties,
        environment: &LayoutEnvironment,
        state: StyleState,
        diagnostics: bool,
        tokens: &TokenSnapshot,
    ) -> Result<StyleResolution, StyleError> {
        environment.validate()?;
        let mut output = StyleResolution {
            layout: base_layout.clone(),
            properties: base_properties.clone(),
            winners: BTreeMap::new(),
        };
        for &(key, style_index, declaration_index) in &self.selected {
            let declaration = &self.styles[style_index].0.declarations[declaration_index];
            let (case_index, initial) =
                declaration.apply(&mut output, environment, state, tokens)?;
            declaration.validate_resolved_bounds(&output, self.styles[style_index].0.limits)?;
            if diagnostics {
                output.winners.insert(
                    key,
                    StyleWinner {
                        style_index,
                        declaration_index,
                        case_index,
                        initial,
                    },
                );
            }
        }
        validate_output(&output)?;
        Ok(output)
    }
}
