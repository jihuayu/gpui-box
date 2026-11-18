//! Owned, typed style composition. Select a complete rule per atomic property
//! first, then evaluate its ordered conditions. No callbacks or CSS parser.
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

/// Initial resets to the framework default, not the earlier composed value or
/// the node's authored base. No declaration means leave that base untouched.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleValue<T> {
    Value(T),
    Initial,
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
impl<T: Clone> StyleRule<T> {
    fn map<U>(&self, map: impl Fn(T) -> U) -> StyleRule<U> {
        let value = |v: &StyleValue<T>| match v {
            StyleValue::Value(v) => StyleValue::Value(map(v.clone())),
            StyleValue::Initial => StyleValue::Initial,
        };
        StyleRule {
            default: value(&self.default),
            cases: self
                .cases
                .iter()
                .map(|c| StyleCase {
                    condition: c.condition.clone(),
                    value: value(&c.value),
                })
                .collect(),
        }
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
                        validate_output(&output)?;
                    }
                    for case in &rule.cases { case.condition.validate()?; }
                }),+, _ => unreachable!("expanded shorthand") }
                Ok(())
            }
            fn apply(&self, output: &mut StyleResolution, environment: &LayoutEnvironment, state: StyleState) -> (Option<usize>, bool) {
                match self { $(Self::$name(rule) => {
                    let (value, case) = rule.selected(environment, state);
                    let initial = matches!(value, StyleValue::Initial);
                    output.$target.$($field).+ = match value {
                        StyleValue::Value(value) => value.clone(),
                        StyleValue::Initial => StyleResolution::default().$target.$($field).+,
                    };
                    (case, initial)
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
    fn expand(&self) -> Vec<Self> {
        use StyleDeclaration::*;
        match self {
            Padding(r) => vec![
                PaddingLeft(r.map(|v| v.left)),
                PaddingRight(r.map(|v| v.right)),
                PaddingTop(r.map(|v| v.top)),
                PaddingBottom(r.map(|v| v.bottom)),
            ],
            Margin(r) => vec![
                MarginLeft(r.map(|v| v.left)),
                MarginRight(r.map(|v| v.right)),
                MarginTop(r.map(|v| v.top)),
                MarginBottom(r.map(|v| v.bottom)),
            ],
            Border(r) => vec![
                BorderLeft(r.map(|v| v.left)),
                BorderRight(r.map(|v| v.right)),
                BorderTop(r.map(|v| v.top)),
                BorderBottom(r.map(|v| v.bottom)),
            ],
            Inset(r) => vec![
                InsetLeft(r.map(|v| v.left)),
                InsetRight(r.map(|v| v.right)),
                InsetTop(r.map(|v| v.top)),
                InsetBottom(r.map(|v| v.bottom)),
            ],
            Size(r) => vec![Width(r.map(|v| v.width)), Height(r.map(|v| v.height))],
            MinSize(r) => vec![MinWidth(r.map(|v| v.width)), MinHeight(r.map(|v| v.height))],
            MaxSize(r) => vec![MaxWidth(r.map(|v| v.width)), MaxHeight(r.map(|v| v.height))],
            Gap(r) => vec![ColumnGap(r.map(|v| v.width)), RowGap(r.map(|v| v.height))],
            atom => vec![atom.clone()],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Sheet {
    name: String,
    declarations: Vec<StyleDeclaration>,
}
/// Owned validated immutable definition. Clone shares declarations; equal new
/// definitions compare by value. No global registry, callback or lifetime leak.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRef(Arc<Sheet>);
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
            for atom in declaration.expand() {
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
        Ok(Self(Arc::new(Sheet {
            name,
            declarations: expanded,
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
        let tracks = self
            .0
            .declarations
            .iter()
            .map(|d| d.value_work().0)
            .max()
            .unwrap_or(0);
        let shadows = self
            .0
            .declarations
            .iter()
            .map(|d| d.value_work().1)
            .max()
            .unwrap_or(0);
        (
            self.0.declarations.len(),
            self.0
                .declarations
                .iter()
                .map(StyleDeclaration::case_count)
                .sum(),
            tracks,
            shadows,
        )
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
    environment.validate()?;
    let mut selected = BTreeMap::new();
    for (style_index, style) in styles.iter().enumerate() {
        for (declaration_index, declaration) in style.0.declarations.iter().enumerate() {
            selected.insert(
                declaration.key(),
                (style_index, declaration_index, declaration),
            );
        }
    }
    let mut output = StyleResolution {
        layout: base_layout.clone(),
        properties: base_properties.clone(),
        winners: BTreeMap::new(),
    };
    for (key, (style_index, declaration_index, declaration)) in selected {
        let (case_index, initial) = declaration.apply(&mut output, environment, state);
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
    validate_output(&output)?;
    Ok(output)
}
