//! Layout types owned by the framework rather than by a layout engine.
//!
//! The retained implementation currently lowers these values to Taffy, but
//! callers do not need to depend on Taffy's representation.  This is the
//! boundary used by the future UI IR and by non-GPUI hosts.

use super::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
    Auto,
}

impl Length {
    pub const fn px(value: f32) -> Self {
        Self::Px(value)
    }
    pub const fn percent(value: f32) -> Self {
        Self::Percent(value)
    }
    pub const fn auto() -> Self {
        Self::Auto
    }

    fn to_taffy(self) -> taffy::LengthPercentageAuto {
        match self {
            Self::Px(value) => taffy::LengthPercentageAuto::length(value),
            Self::Percent(value) => taffy::LengthPercentageAuto::percent(value),
            Self::Auto => taffy::LengthPercentageAuto::auto(),
        }
    }

    fn to_taffy_percentage(self) -> Result<taffy::LengthPercentage, Error> {
        match self {
            Self::Px(value) => Ok(taffy::LengthPercentage::length(value)),
            Self::Percent(value) => Ok(taffy::LengthPercentage::percent(value)),
            Self::Auto => Err(Error::InvalidStyle),
        }
    }

    fn validate(self, allow_auto: bool, allow_negative: bool) -> Result<(), Error> {
        match self {
            Self::Px(value) | Self::Percent(value) if !value.is_finite() => {
                Err(Error::InvalidStyle)
            }
            Self::Px(value) | Self::Percent(value) if !allow_negative && value < 0.0 => {
                Err(Error::InvalidStyle)
            }
            Self::Auto if !allow_auto => Err(Error::InvalidStyle),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dimension {
    Px(f32),
    Percent(f32),
    Auto,
}

impl Dimension {
    pub const fn px(value: f32) -> Self {
        Self::Px(value)
    }
    pub const fn percent(value: f32) -> Self {
        Self::Percent(value)
    }
    pub const fn auto() -> Self {
        Self::Auto
    }

    fn to_taffy(self) -> taffy::Dimension {
        match self {
            Self::Px(value) => taffy::Dimension::length(value),
            Self::Percent(value) => taffy::Dimension::percent(value),
            Self::Auto => taffy::Dimension::auto(),
        }
    }

    fn validate(self) -> Result<(), Error> {
        match self {
            Self::Px(value) | Self::Percent(value) if !value.is_finite() || value < 0.0 => {
                Err(Error::InvalidStyle)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    pub left: Length,
    pub right: Length,
    pub top: Length,
    pub bottom: Length,
}

impl Edges {
    pub const fn zero() -> Self {
        let zero = Length::Px(0.0);
        Self {
            left: zero,
            right: zero,
            top: zero,
            bottom: zero,
        }
    }
    pub const fn all(value: Length) -> Self {
        Self {
            left: value,
            right: value,
            top: value,
            bottom: value,
        }
    }
    fn to_taffy(self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        taffy::Rect {
            left: self.left.to_taffy(),
            right: self.right.to_taffy(),
            top: self.top.to_taffy(),
            bottom: self.bottom.to_taffy(),
        }
    }
    fn to_taffy_padding(self) -> Result<taffy::Rect<taffy::LengthPercentage>, Error> {
        Ok(taffy::Rect {
            left: self.left.to_taffy_percentage()?,
            right: self.right.to_taffy_percentage()?,
            top: self.top.to_taffy_percentage()?,
            bottom: self.bottom.to_taffy_percentage()?,
        })
    }
    fn validate(self, allow_auto: bool, allow_negative: bool) -> Result<(), Error> {
        for value in [self.left, self.right, self.top, self.bottom] {
            value.validate(allow_auto, allow_negative)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignItems {
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

/// Per-item cross-axis alignment. `Auto` inherits the flex container's
/// `align_items` value, matching the CSS/Taffy contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignSelf {
    Auto,
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignContent {
    Start,
    End,
    Center,
    Stretch,
    SpaceBetween,
    SpaceEvenly,
    SpaceAround,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JustifyContent {
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceEvenly,
    SpaceAround,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutPosition {
    Static,
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDisplay {
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDirection {
    Ltr,
    Rtl,
}

/// Layout-affecting overflow behavior. Painting and scrolling remain owned by
/// the property/scroll stages; the value here controls flex automatic minimums
/// and Taffy's overflow-aware sizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Clip,
    Hidden,
    Scroll,
}

impl From<taffy::Direction> for LayoutDirection {
    fn from(value: taffy::Direction) -> Self {
        match value {
            taffy::Direction::Ltr => Self::Ltr,
            taffy::Direction::Rtl => Self::Rtl,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSizing {
    BorderBox,
    ContentBox,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSize<T> {
    pub width: T,
    pub height: T,
}

/// A finite available space or an intrinsic sizing query. This mirrors the
/// concepts needed by Flex without leaking a layout engine's enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableSpace {
    Definite(f32),
    MinContent,
    MaxContent,
    Unbounded,
}

impl AvailableSpace {
    pub const fn definite(value: f32) -> Self {
        Self::Definite(value)
    }

    pub fn validate(self) -> Result<(), Error> {
        match self {
            Self::Definite(value) if !value.is_finite() || value < 0.0 => {
                Err(Error::InvalidViewport)
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn to_taffy(self) -> Result<taffy::AvailableSpace, Error> {
        self.validate()?;
        match self {
            Self::Definite(value) => Ok(taffy::AvailableSpace::Definite(value)),
            Self::MinContent => Ok(taffy::AvailableSpace::MinContent),
            Self::MaxContent => Ok(taffy::AvailableSpace::MaxContent),
            // Taffy 0.15 has no separate unbounded variant. Do not silently
            // equate it with max-content; callers must use the explicit
            // measure contract once the backend can represent this distinction.
            Self::Unbounded => Err(Error::UnsupportedLayout),
        }
    }

    pub(crate) fn resolve_definite(self) -> Result<f32, Error> {
        match self {
            Self::Definite(value) => Ok(value),
            // A paint frame needs a finite clip/viewport. Intrinsic queries are
            // accepted by the layout contract but must be resolved by a host
            // before producing a renderable frame.
            Self::MinContent | Self::MaxContent | Self::Unbounded => Err(Error::UnsupportedLayout),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutConstraints {
    pub width: AvailableSpace,
    pub height: AvailableSpace,
}

impl LayoutConstraints {
    pub const fn new(width: AvailableSpace, height: AvailableSpace) -> Self {
        Self { width, height }
    }

    pub const fn viewport(width: f32, height: f32) -> Self {
        Self::new(
            AvailableSpace::Definite(width),
            AvailableSpace::Definite(height),
        )
    }

    pub fn validate(self) -> Result<(), Error> {
        self.width.validate()?;
        self.height.validate()?;
        Ok(())
    }
}

/// Engine-independent local layout output. Coordinates are relative to the
/// layout parent; world transforms and device-pixel rounding happen later.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutResult {
    pub location: [f32; 2],
    pub size: [f32; 2],
    pub content_size: [f32; 2],
    pub border: [f32; 4],
    pub padding: [f32; 4],
    pub margin: [f32; 4],
    /// Scrollable overflow in the queried node's scroll-origin coordinates.
    pub scrollable_overflow: [f32; 4],
    pub baseline: Option<f32>,
}

impl LayoutResult {
    pub(crate) fn from_taffy_and_baseline(layout: &taffy::Layout, baseline: Option<f32>) -> Self {
        Self {
            location: [layout.location.x, layout.location.y],
            size: [layout.size.width, layout.size.height],
            content_size: [
                (layout.size.width
                    - layout.border.left
                    - layout.border.right
                    - layout.padding.left
                    - layout.padding.right)
                    .max(0.0),
                (layout.size.height
                    - layout.border.top
                    - layout.border.bottom
                    - layout.padding.top
                    - layout.padding.bottom)
                    .max(0.0),
            ],
            border: [
                layout.border.left,
                layout.border.right,
                layout.border.top,
                layout.border.bottom,
            ],
            padding: [
                layout.padding.left,
                layout.padding.right,
                layout.padding.top,
                layout.padding.bottom,
            ],
            margin: [
                layout.margin.left,
                layout.margin.right,
                layout.margin.top,
                layout.margin.bottom,
            ],
            scrollable_overflow: [
                layout.scrollable_overflow_rect.left,
                layout.scrollable_overflow_rect.right,
                layout.scrollable_overflow_rect.top,
                layout.scrollable_overflow_rect.bottom,
            ],
            // Taffy 0.15 stores baselines during layout computation but does
            // not expose them on the published `Layout`; keep this optional
            // until the framework owns a stable baseline contract.
            baseline,
        }
    }
}

impl<T> LayoutSize<T> {
    pub const fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

/// Engine-independent public layout contract for the supported Flex model.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutStyle {
    /// Stable flex-item ordering. Equal values preserve logical child order.
    pub order: i32,
    pub display: LayoutDisplay,
    pub direction: LayoutDirection,
    pub position: LayoutPosition,
    pub inset: Edges,
    pub size: LayoutSize<Dimension>,
    pub min_size: LayoutSize<Length>,
    pub max_size: LayoutSize<Length>,
    pub aspect_ratio: Option<f32>,
    pub margin: Edges,
    pub padding: Edges,
    pub border: Edges,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_basis: Dimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub align_items: AlignItems,
    pub align_self: AlignSelf,
    pub align_content: AlignContent,
    pub justify_content: JustifyContent,
    pub gap: LayoutSize<Length>,
    pub main_cross_gap: Option<LayoutSize<Length>>,
    pub overflow: LayoutSize<Overflow>,
    pub box_sizing: BoxSizing,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            order: 0,
            display: LayoutDisplay::Flex,
            direction: LayoutDirection::Ltr,
            position: LayoutPosition::Static,
            inset: Edges::all(Length::Auto),
            size: LayoutSize::new(Dimension::Auto, Dimension::Auto),
            min_size: LayoutSize::new(Length::Auto, Length::Auto),
            max_size: LayoutSize::new(Length::Auto, Length::Auto),
            aspect_ratio: None,
            margin: Edges::zero(),
            padding: Edges::zero(),
            border: Edges::zero(),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_basis: Dimension::Auto,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            align_items: AlignItems::Stretch,
            align_self: AlignSelf::Auto,
            align_content: AlignContent::Stretch,
            justify_content: JustifyContent::Start,
            gap: LayoutSize::new(Length::Px(0.0), Length::Px(0.0)),
            main_cross_gap: None,
            overflow: LayoutSize::new(Overflow::Visible, Overflow::Visible),
            box_sizing: BoxSizing::BorderBox,
        }
    }
}

impl LayoutStyle {
    pub fn row() -> Self {
        Self::default()
    }
    pub fn column() -> Self {
        Self {
            flex_direction: FlexDirection::Column,
            ..Self::default()
        }
    }

    pub fn display(mut self, display: LayoutDisplay) -> Self {
        self.display = display;
        self
    }
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
    pub fn size(mut self, width: Dimension, height: Dimension) -> Self {
        self.size = LayoutSize::new(width, height);
        self
    }
    pub fn min_size(mut self, width: Length, height: Length) -> Self {
        self.min_size = LayoutSize::new(width, height);
        self
    }
    pub fn max_size(mut self, width: Length, height: Length) -> Self {
        self.max_size = LayoutSize::new(width, height);
        self
    }
    pub fn direction(mut self, direction: LayoutDirection) -> Self {
        self.direction = direction;
        self
    }
    pub fn flex_direction(mut self, direction: FlexDirection) -> Self {
        self.flex_direction = direction;
        self
    }
    pub fn flex_wrap(mut self, wrap: FlexWrap) -> Self {
        self.flex_wrap = wrap;
        self
    }
    pub fn flex_basis(mut self, basis: Dimension) -> Self {
        self.flex_basis = basis;
        self
    }
    pub fn flex_grow(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }
    pub fn flex_shrink(mut self, shrink: f32) -> Self {
        self.flex_shrink = shrink;
        self
    }
    pub fn gap(mut self, main: Length, cross: Length) -> Self {
        // `gap` is expressed as physical row/column gaps, like CSS. Use
        // `main_cross_gap` when authoring a direction-independent flex helper.
        self.gap = LayoutSize::new(main, cross);
        self.main_cross_gap = None;
        self
    }
    pub fn row_gap(mut self, value: Length) -> Self {
        self.gap.width = value;
        self.main_cross_gap = None;
        self
    }
    pub fn column_gap(mut self, value: Length) -> Self {
        self.gap.height = value;
        self.main_cross_gap = None;
        self
    }
    pub fn main_cross_gap(mut self, main: Length, cross: Length) -> Self {
        self.main_cross_gap = Some(LayoutSize::new(main, cross));
        self
    }
    pub fn padding(mut self, edges: Edges) -> Self {
        self.padding = edges;
        self
    }
    pub fn margin(mut self, edges: Edges) -> Self {
        self.margin = edges;
        self
    }
    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.align_items = value;
        self
    }
    pub fn align_self(mut self, value: AlignSelf) -> Self {
        self.align_self = value;
        self
    }
    pub fn overflow(mut self, x: Overflow, y: Overflow) -> Self {
        self.overflow = LayoutSize::new(x, y);
        self
    }
    pub fn align_content(mut self, value: AlignContent) -> Self {
        self.align_content = value;
        self
    }
    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.justify_content = value;
        self
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.inset.validate(true, true)?;
        self.margin.validate(true, true)?;
        self.padding.validate(false, false)?;
        self.border.validate(false, false)?;
        self.min_size.width.validate(true, false)?;
        self.min_size.height.validate(true, false)?;
        self.max_size.width.validate(true, false)?;
        self.max_size.height.validate(true, false)?;
        self.size.width.validate()?;
        self.size.height.validate()?;
        self.flex_basis.validate()?;
        let gap = self.main_cross_gap.unwrap_or(self.gap);
        gap.width.validate(false, false)?;
        gap.height.validate(false, false)?;
        if !self.flex_grow.is_finite()
            || self.flex_grow < 0.0
            || !self.flex_shrink.is_finite()
            || self.flex_shrink < 0.0
            || self
                .aspect_ratio
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(Error::InvalidStyle);
        }
        Ok(())
    }

    pub(crate) fn to_taffy(&self) -> Result<taffy::Style, Error> {
        self.validate()?;
        Ok(taffy::Style {
            display: match self.display {
                LayoutDisplay::Flex => taffy::Display::Flex,
                LayoutDisplay::None => taffy::Display::None,
            },
            direction: match self.direction {
                LayoutDirection::Ltr => taffy::Direction::Ltr,
                LayoutDirection::Rtl => taffy::Direction::Rtl,
            },
            position: match self.position {
                LayoutPosition::Static => taffy::Position::Static,
                LayoutPosition::Relative => taffy::Position::Relative,
                LayoutPosition::Absolute => taffy::Position::Absolute,
            },
            inset: self.inset.to_taffy(),
            size: taffy::Size {
                width: self.size.width.to_taffy(),
                height: self.size.height.to_taffy(),
            },
            min_size: taffy::Size {
                width: self.min_size.width.to_taffy(),
                height: self.min_size.height.to_taffy(),
            },
            max_size: taffy::Size {
                width: self.max_size.width.to_taffy(),
                height: self.max_size.height.to_taffy(),
            },
            aspect_ratio: self.aspect_ratio,
            margin: self.margin.to_taffy(),
            padding: self.padding.to_taffy_padding()?,
            border: self.border.to_taffy_padding()?,
            flex_direction: match self.flex_direction {
                FlexDirection::Row => taffy::FlexDirection::Row,
                FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
                FlexDirection::Column => taffy::FlexDirection::Column,
                FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
            },
            flex_wrap: match self.flex_wrap {
                FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
                FlexWrap::Wrap => taffy::FlexWrap::Wrap,
                FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
            },
            flex_basis: self.flex_basis.to_taffy(),
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            align_items: match self.align_items {
                AlignItems::Start => taffy::AlignItems::START,
                AlignItems::End => taffy::AlignItems::END,
                AlignItems::Center => taffy::AlignItems::CENTER,
                AlignItems::Stretch => taffy::AlignItems::STRETCH,
                AlignItems::Baseline => taffy::AlignItems::BASELINE,
            },
            align_self: match self.align_self {
                AlignSelf::Auto => None,
                AlignSelf::Start => Some(taffy::AlignSelf::START),
                AlignSelf::End => Some(taffy::AlignSelf::END),
                AlignSelf::Center => Some(taffy::AlignSelf::CENTER),
                AlignSelf::Stretch => Some(taffy::AlignSelf::STRETCH),
                AlignSelf::Baseline => Some(taffy::AlignSelf::BASELINE),
            },
            align_content: match self.align_content {
                AlignContent::Start => taffy::AlignContent::START,
                AlignContent::End => taffy::AlignContent::END,
                AlignContent::Center => taffy::AlignContent::CENTER,
                AlignContent::Stretch => taffy::AlignContent::STRETCH,
                AlignContent::SpaceBetween => taffy::AlignContent::SPACE_BETWEEN,
                AlignContent::SpaceEvenly => taffy::AlignContent::SPACE_EVENLY,
                AlignContent::SpaceAround => taffy::AlignContent::SPACE_AROUND,
            },
            justify_content: match self.justify_content {
                JustifyContent::Start => taffy::JustifyContent::START,
                JustifyContent::End => taffy::JustifyContent::END,
                JustifyContent::Center => taffy::JustifyContent::CENTER,
                JustifyContent::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
                JustifyContent::SpaceEvenly => taffy::JustifyContent::SPACE_EVENLY,
                JustifyContent::SpaceAround => taffy::JustifyContent::SPACE_AROUND,
            },
            gap: {
                let gap = self.main_cross_gap.unwrap_or(self.gap);
                let physical = match self.flex_direction {
                    FlexDirection::Row | FlexDirection::RowReverse => gap,
                    FlexDirection::Column | FlexDirection::ColumnReverse => {
                        LayoutSize::new(gap.height, gap.width)
                    }
                };
                taffy::Size {
                    width: physical.width.to_taffy_percentage()?,
                    height: physical.height.to_taffy_percentage()?,
                }
            },
            box_sizing: match self.box_sizing {
                BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
                BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
            },
            overflow: taffy::Point {
                x: match self.overflow.width {
                    Overflow::Visible => taffy::Overflow::Visible,
                    Overflow::Clip => taffy::Overflow::Clip,
                    Overflow::Hidden => taffy::Overflow::Hidden,
                    Overflow::Scroll => taffy::Overflow::Scroll,
                },
                y: match self.overflow.height {
                    Overflow::Visible => taffy::Overflow::Visible,
                    Overflow::Clip => taffy::Overflow::Clip,
                    Overflow::Hidden => taffy::Overflow::Hidden,
                    Overflow::Scroll => taffy::Overflow::Scroll,
                },
            },
            ..taffy::Style::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retained::{
        BackendCapabilities, CustomMeasureSnapshot, MeasureSnapshot, Transaction, UiTree,
    };
    use crate::retained::{Style, length};

    #[test]
    fn public_flex_contract_lowers_without_exposing_engine_style() {
        let root_style = LayoutStyle::row()
            .size(Dimension::px(120.0), Dimension::px(40.0))
            .gap(Length::px(10.0), Length::px(0.0))
            .justify_content(JustifyContent::SpaceBetween);
        let mut tree = UiTree::new_layout(root_style, 0).unwrap();
        let first = tree
            .append_layout(
                tree.root(),
                LayoutStyle::default().size(Dimension::px(20.0), Dimension::px(20.0)),
                1,
            )
            .unwrap();
        let second = tree
            .append_layout(
                tree.root(),
                LayoutStyle::default().size(Dimension::px(20.0), Dimension::px(20.0)),
                2,
            )
            .unwrap();
        let frame = tree.frame([120.0, 40.0]).unwrap();
        let first_item = frame.items.iter().find(|item| item.node == first).unwrap();
        let second_item = frame.items.iter().find(|item| item.node == second).unwrap();
        assert_eq!((first_item.bounds.x, second_item.bounds.x), (0.0, 100.0));
        assert_eq!(first_item.bounds.height, 20.0);
        assert_eq!(second_item.bounds.height, 20.0);
        let _ = BackendCapabilities::default();
    }

    #[test]
    fn invalid_public_flex_values_are_rejected_before_engine_creation() {
        let invalid = LayoutStyle::row().size(Dimension::px(f32::NAN), Dimension::px(10.0));
        assert_eq!(invalid.validate(), Err(Error::InvalidStyle));
        let invalid_gap = LayoutStyle::row().gap(Length::px(-1.0), Length::px(0.0));
        assert_eq!(invalid_gap.validate(), Err(Error::InvalidStyle));
    }

    #[test]
    fn flex_item_alignment_and_directional_gap_lower_without_engine_types() {
        let style = LayoutStyle::column()
            .main_cross_gap(Length::px(12.0), Length::px(4.0))
            .align_self(AlignSelf::End)
            .overflow(Overflow::Clip, Overflow::Scroll);
        let lowered = style.to_taffy().unwrap();
        assert_eq!(lowered.gap.width, taffy::LengthPercentage::length(4.0));
        assert_eq!(lowered.gap.height, taffy::LengthPercentage::length(12.0));
        assert_eq!(lowered.align_self, Some(taffy::AlignSelf::END));
        assert_eq!(lowered.overflow.x, taffy::Overflow::Clip);
        assert_eq!(lowered.overflow.y, taffy::Overflow::Scroll);
    }

    #[test]
    fn logical_gap_resolution_is_independent_of_builder_call_order() {
        let before_direction = LayoutStyle::row()
            .main_cross_gap(Length::px(12.0), Length::px(4.0))
            .flex_direction(FlexDirection::Column);
        let after_direction = LayoutStyle::column()
            .flex_direction(FlexDirection::Column)
            .main_cross_gap(Length::px(12.0), Length::px(4.0));
        assert_eq!(
            before_direction.to_taffy().unwrap().gap,
            after_direction.to_taffy().unwrap().gap
        );
    }

    #[test]
    fn invalid_layout_patch_is_atomic_and_valid_patch_reflows_flex_children() {
        let mut ui = crate::retained::logical::LogicalUi::new_layout(
            LayoutStyle::row().size(Dimension::px(100.0), Dimension::px(20.0)),
            0,
            crate::retained::BackendCapabilities::default(),
        )
        .unwrap();
        let scope = ui.root_scope();
        let node = ui
            .append_render_layout(
                scope,
                ui.root_render(),
                LayoutStyle::default().size(Dimension::px(20.0), Dimension::px(20.0)),
                1,
            )
            .unwrap();
        let style = ui.state(scope, 0, || LayoutStyle::default()).unwrap();
        ui.bind(
            style,
            crate::retained::reactive::Property::layout_style(node),
        )
        .unwrap();
        let before = ui.frame([100.0, 20.0]).unwrap();
        let invalid = LayoutStyle::row().size(Dimension::px(f32::NAN), Dimension::px(20.0));
        let error = ui.transaction(|tx| tx.set(style, invalid));
        assert!(matches!(error, Err(Error::InvalidStyle)));
        assert_eq!(ui.read(style).unwrap(), &LayoutStyle::default());
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);

        let next = LayoutStyle::row()
            .size(Dimension::px(40.0), Dimension::px(20.0))
            .flex_grow(0.0);
        ui.transaction(|tx| tx.set(style, next)).unwrap();
        let after = ui.frame([100.0, 20.0]).unwrap();
        assert_eq!(
            after
                .items
                .iter()
                .find(|item| item.node == node)
                .unwrap()
                .bounds
                .width,
            40.0
        );
    }

    #[test]
    fn constraints_and_layout_result_are_engine_independent() {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row().size(Dimension::percent(1.0), Dimension::percent(1.0)),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::default().size(Dimension::px(24.0), Dimension::px(12.0)),
                1,
            )
            .unwrap();
        let frame = tree
            .frame_with_constraints(LayoutConstraints::new(
                AvailableSpace::definite(80.0),
                AvailableSpace::definite(40.0),
            ))
            .unwrap();
        assert_eq!(frame.items.len(), 2);
        let result = tree.layout_result(child).unwrap();
        assert_eq!(result.location, [0.0, 0.0]);
        assert_eq!(result.size, [24.0, 12.0]);
        assert_eq!(result.content_size, [24.0, 12.0]);
        assert_eq!(result.margin, [0.0; 4]);
        // Taffy 0.15 only populates this rectangle when its `content_size`
        // feature is enabled; the retained default keeps a zero rectangle.
        assert_eq!(result.scrollable_overflow, [0.0; 4]);
        assert_eq!(result.baseline, None);
        assert!(matches!(
            tree.frame_with_constraints(LayoutConstraints::new(
                AvailableSpace::MinContent,
                AvailableSpace::definite(40.0),
            )),
            Err(Error::UnsupportedLayout)
        ));
    }

    #[test]
    fn intrinsic_measure_query_is_separate_from_paint_publication() {
        let mut tree = UiTree::new_layout(LayoutStyle::row(), 0).unwrap();
        tree.append_layout(
            tree.root(),
            LayoutStyle::default().size(Dimension::px(24.0), Dimension::px(12.0)),
            1,
        )
        .unwrap();
        let measured = tree
            .measure_with_constraints(LayoutConstraints::new(
                AvailableSpace::MinContent,
                AvailableSpace::MaxContent,
            ))
            .unwrap();
        assert!(measured.size[0] >= 24.0);
        assert!(measured.size[1] >= 12.0);
        assert!(tree.published_frame().is_none());
        let frame = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(frame.items.len(), 2);
        assert!(matches!(
            tree.measure_with_constraints(LayoutConstraints::new(
                AvailableSpace::Unbounded,
                AvailableSpace::MaxContent,
            )),
            Err(Error::UnsupportedLayout)
        ));
    }

    #[test]
    fn intrinsic_measure_can_target_a_live_subtree_without_publishing() {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row().size(Dimension::px(100.0), Dimension::px(40.0)),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::column().size(Dimension::auto(), Dimension::auto()),
                1,
            )
            .unwrap();
        tree.append_layout(
            child,
            LayoutStyle::default().size(Dimension::px(18.0), Dimension::px(9.0)),
            2,
        )
        .unwrap();
        let measured = tree
            .measure_node_with_constraints(
                child,
                LayoutConstraints::new(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .unwrap();
        assert!(measured.size[0] >= 18.0);
        assert!(measured.size[1] >= 9.0);
        assert!(tree.published_frame().is_none());
    }

    #[test]
    fn measure_only_does_not_replace_published_geometry() {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row().size(Dimension::px(100.0), Dimension::px(40.0)),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::default().size(Dimension::px(20.0), Dimension::px(10.0)),
                1,
            )
            .unwrap();
        let frame = tree.frame([100.0, 40.0]).unwrap();
        let before = tree.layout_result(child).unwrap();
        let measured = tree
            .measure_node_with_constraints(
                child,
                LayoutConstraints::new(
                    AvailableSpace::definite(80.0),
                    AvailableSpace::definite(25.0),
                ),
            )
            .unwrap();
        assert_eq!(measured.size, [20.0, 10.0]);
        assert_eq!(tree.layout_result(child).unwrap(), before);
        assert_eq!(tree.published_frame().unwrap().revision, frame.revision);
    }

    #[test]
    fn published_layout_result_exposes_leaf_measure_baseline() {
        let mut tree = UiTree::new(
            Style {
                padding: taffy::Rect {
                    top: length(3.0),
                    ..taffy::Rect::zero()
                },
                ..Style::default()
            },
            0,
        )
        .unwrap();
        let child = tree.append(tree.root(), Style::default(), 1).unwrap();
        let mut transaction = Transaction::default();
        transaction.measure(
            child,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 20.0,
                height: 10.0,
                baseline: Some(6.0),
            })),
        );
        tree.commit(transaction, BackendCapabilities::default())
            .unwrap();
        tree.frame([100.0, 40.0]).unwrap();
        assert_eq!(tree.layout_result(child).unwrap().baseline, Some(6.0));
    }
}
