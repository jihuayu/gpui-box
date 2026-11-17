//! Translate the item input domain without leaking engine types to clients.
use super::*;

impl ChildLayoutStyle {
    pub(super) fn from_taffy(value: taffy::tree::CustomChildStyle) -> Result<Self, Error> {
        use taffy::AlignItemsKeyword as K;
        let align_self = match value.align_self {
            None => LayoutAlignSelf::Auto.into(),
            Some(alignment) => LayoutAlignment {
                position: match alignment.keyword {
                    K::Normal => LayoutAlignSelf::Normal,
                    K::Start => LayoutAlignSelf::Start,
                    K::End => LayoutAlignSelf::End,
                    K::FlexStart => LayoutAlignSelf::FlexStart,
                    K::FlexEnd => LayoutAlignSelf::FlexEnd,
                    K::SelfStart => LayoutAlignSelf::SelfStart,
                    K::SelfEnd => LayoutAlignSelf::SelfEnd,
                    K::Center => LayoutAlignSelf::Center,
                    K::Stretch => LayoutAlignSelf::Stretch,
                    K::Baseline => LayoutAlignSelf::Baseline,
                    K::LastBaseline => LayoutAlignSelf::LastBaseline,
                },
                overflow: match alignment.safety {
                    taffy::AlignmentSafety::Default => LayoutAlignmentOverflow::Default,
                    taffy::AlignmentSafety::Safe => LayoutAlignmentOverflow::Safe,
                    taffy::AlignmentSafety::Unsafe => LayoutAlignmentOverflow::Unsafe,
                },
            },
        };
        let containment = value.contain.size_containment();
        Ok(Self {
            hidden: value.hidden,
            direction: value.direction.into(),
            position: match value.position {
                taffy::Position::Static => LayoutPosition::Static,
                taffy::Position::Relative => LayoutPosition::Relative,
                taffy::Position::Absolute => LayoutPosition::Absolute,
                // Raw legacy styles outside the framework's public model are
                // rejected, rather than silently mapped to a different policy.
                _ => return Err(Error::InvalidParentLayout),
            },
            inset: auto_edges(value.inset)?,
            size: dimensions(value.size)?,
            min_size: dimensions(value.min_size)?,
            max_size: dimensions(value.max_size)?,
            aspect_ratio: value.aspect_ratio,
            margin: auto_edges(value.margin)?,
            padding: edges(value.padding)?,
            border: edges(value.border)?,
            box_sizing: match value.box_sizing {
                taffy::BoxSizing::BorderBox => LayoutBoxSizing::BorderBox,
                taffy::BoxSizing::ContentBox => LayoutBoxSizing::ContentBox,
            },
            overflow: LayoutSize::new(overflow(value.overflow.x)?, overflow(value.overflow.y)?),
            scrollbar_width: value.scrollbar_width,
            size_containment: from_size(containment),
            contain_intrinsic_size: from_size(value.contain_intrinsic_size),
            layout_containment: value.contain.contains(taffy::Contain::LAYOUT),
            paint_containment: value.contain.contains(taffy::Contain::PAINT),
            is_replaced: value.is_replaced,
            is_compressible_replaced: value.is_compressible_replaced,
            flex_basis: dimension(value.flex_basis)?,
            flex_grow: value.flex_grow,
            flex_shrink: value.flex_shrink,
            align_self,
        })
    }
}

#[allow(unreachable_patterns)] // calc may be enabled by a downstream dependency
fn dimension(value: taffy::Dimension) -> Result<LayoutDimension, Error> {
    use taffy::style::ExpandedDimension as D;
    Ok(match value.expand() {
        D::Length(v) => LayoutDimension::Px(v),
        D::Percent(v) => LayoutDimension::Percent(v),
        D::Auto => LayoutDimension::Auto,
        D::MinContent => LayoutDimension::MinContent,
        D::MaxContent => LayoutDimension::MaxContent,
        D::FitContent => LayoutDimension::FitContent,
        D::FitContentPx(v) => LayoutDimension::FitContentPx(v),
        D::FitContentPercent(v) => LayoutDimension::FitContentPercent(v),
        D::Stretch => LayoutDimension::Stretch,
        D::Content => LayoutDimension::Content,
        _ => return Err(Error::InvalidParentLayout),
    })
}
fn dimensions(value: Size<taffy::Dimension>) -> Result<LayoutSize<LayoutDimension>, Error> {
    Ok(LayoutSize::new(
        dimension(value.width)?,
        dimension(value.height)?,
    ))
}
#[allow(unreachable_patterns)]
fn auto_length(value: taffy::LengthPercentageAuto) -> Result<LayoutLength, Error> {
    use taffy::style::ExpandedLengthPercentageAuto as L;
    Ok(match value.expand() {
        L::Length(v) => LayoutLength::Px(v),
        L::Percent(v) => LayoutLength::Percent(v),
        L::Auto => LayoutLength::Auto,
        _ => return Err(Error::InvalidParentLayout),
    })
}
#[allow(unreachable_patterns)]
fn length_value(value: taffy::LengthPercentage) -> Result<LayoutLength, Error> {
    use taffy::style::ExpandedLengthPercentage as L;
    Ok(match value.expand() {
        L::Length(v) => LayoutLength::Px(v),
        L::Percent(v) => LayoutLength::Percent(v),
        _ => return Err(Error::InvalidParentLayout),
    })
}
fn auto_edges(value: taffy::Rect<taffy::LengthPercentageAuto>) -> Result<LayoutEdges, Error> {
    Ok(LayoutEdges {
        left: auto_length(value.left)?,
        right: auto_length(value.right)?,
        top: auto_length(value.top)?,
        bottom: auto_length(value.bottom)?,
    })
}
fn edges(value: taffy::Rect<taffy::LengthPercentage>) -> Result<LayoutEdges, Error> {
    Ok(LayoutEdges {
        left: length_value(value.left)?,
        right: length_value(value.right)?,
        top: length_value(value.top)?,
        bottom: length_value(value.bottom)?,
    })
}
fn overflow(value: taffy::Overflow) -> Result<LayoutOverflow, Error> {
    Ok(match value {
        taffy::Overflow::Visible => LayoutOverflow::Visible,
        taffy::Overflow::Clip => LayoutOverflow::Clip,
        taffy::Overflow::Hidden => LayoutOverflow::Hidden,
        taffy::Overflow::Scroll => LayoutOverflow::Scroll,
    })
}
