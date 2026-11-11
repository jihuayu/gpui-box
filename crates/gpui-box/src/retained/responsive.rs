//! Explicit Flex subset and bounded style queries over persistent geometry.
use super::{
    logical::LogicalUi,
    reactive::{Property, Signal},
    *,
};

/// Layout, application zoom and device scale are separate host inputs. Unknown
/// input capabilities stay unknown; this is not a mobile/desktop classifier.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEnvironment {
    pub layout_viewport: [f32; 2],
    pub device_scale: f32,
    pub application_zoom: f32,
    pub text_scale: f32,
    pub direction: taffy::Direction,
    pub touch: Option<bool>,
    pub pointer: Option<bool>,
    pub reduced_motion: Option<bool>,
}

impl LayoutEnvironment {
    pub fn new(layout_viewport: [f32; 2]) -> Self {
        Self {
            layout_viewport,
            device_scale: 1.0,
            application_zoom: 1.0,
            text_scale: 1.0,
            direction: taffy::Direction::Ltr,
            touch: None,
            pointer: None,
            reduced_motion: None,
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self
            .layout_viewport
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || [self.device_scale, self.application_zoom, self.text_scale]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err(Error::InvalidEnvironment);
        }
        Ok(())
    }
}

/// Opt-in public contract. Raw Taffy Style remains available as a lower-level API.
#[derive(Clone, Debug, PartialEq)]
pub struct FlexStyle(Style);

impl FlexStyle {
    pub fn row() -> Self {
        Self(Style::default())
    }
    pub fn column() -> Self {
        Self(Style {
            flex_direction: taffy::FlexDirection::Column,
            ..Default::default()
        })
    }
    pub fn try_from_style(style: Style) -> Result<Self, Error> {
        validate_flex(&style)?;
        Ok(Self(style))
    }
    pub fn style(&self) -> &Style {
        &self.0
    }
    pub fn into_style(self) -> Style {
        self.0
    }
}

fn validate_flex(style: &Style) -> Result<(), Error> {
    if !matches!(style.display, taffy::Display::Flex | taffy::Display::None)
        || !matches!(
            style.position,
            taffy::Position::Static | taffy::Position::Relative | taffy::Position::Absolute
        )
        || style.flex_wrap == taffy::FlexWrap::WrapReverse
        || style.float != taffy::Float::None
        || style.clear != taffy::Clear::None
        || style.contain != taffy::Contain::NONE
        || style.item_is_table
        || [style.overflow.x, style.overflow.y]
            .iter()
            .any(|value| *value != taffy::Overflow::Visible)
    {
        return Err(Error::UnsupportedLayout);
    }
    let valid = |value: f32| value.is_finite() && value >= 0.0;
    if ![style.flex_grow, style.flex_shrink, style.scrollbar_width]
        .into_iter()
        .all(valid)
        || style
            .aspect_ratio
            .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        return Err(Error::InvalidStyle);
    }
    let lp = |value: taffy::LengthPercentage| match value.expand() {
        taffy::style::ExpandedLengthPercentage::Length(v)
        | taffy::style::ExpandedLengthPercentage::Percent(v) => valid(v),
    };
    let lpa = |value: taffy::LengthPercentageAuto| match value.expand() {
        taffy::style::ExpandedLengthPercentageAuto::Length(v)
        | taffy::style::ExpandedLengthPercentageAuto::Percent(v) => valid(v),
        taffy::style::ExpandedLengthPercentageAuto::Auto => true,
    };
    let dimension = |value: taffy::Dimension| match value.expand() {
        taffy::style::ExpandedDimension::Length(v)
        | taffy::style::ExpandedDimension::Percent(v) => valid(v),
        taffy::style::ExpandedDimension::Auto => true,
        _ => false,
    };
    if ![style.size.width, style.size.height, style.flex_basis]
        .into_iter()
        .all(dimension)
        || ![
            style.min_size.width,
            style.min_size.height,
            style.max_size.width,
            style.max_size.height,
        ]
        .into_iter()
        .all(lpa)
        || ![
            style.padding.left,
            style.padding.right,
            style.padding.top,
            style.padding.bottom,
            style.border.left,
            style.border.right,
            style.border.top,
            style.border.bottom,
            style.gap.width,
            style.gap.height,
        ]
        .into_iter()
        .all(lp)
    {
        return Err(Error::InvalidStyle);
    }
    // Negative margins/insets are valid. They must still be finite.
    for value in [
        style.margin.left,
        style.margin.right,
        style.margin.top,
        style.margin.bottom,
        style.inset.left,
        style.inset.right,
        style.inset.top,
        style.inset.bottom,
    ] {
        match value.expand() {
            taffy::style::ExpandedLengthPercentageAuto::Length(v)
            | taffy::style::ExpandedLengthPercentageAuto::Percent(v)
                if !v.is_finite() =>
            {
                return Err(Error::InvalidStyle);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Each threshold selects the next complete style at width >= threshold.
/// Thresholds are caller data, not global device categories.
#[derive(Clone, Debug)]
pub struct BreakpointStyles {
    thresholds: Vec<f32>,
    styles: Vec<FlexStyle>,
}
impl BreakpointStyles {
    pub fn new(base: FlexStyle, wider: Vec<(f32, FlexStyle)>) -> Result<Self, Error> {
        let mut thresholds = vec![];
        let mut styles = vec![base];
        for (threshold, style) in wider {
            if !threshold.is_finite()
                || threshold < 0.0
                || thresholds.last().is_some_and(|last| *last >= threshold)
            {
                return Err(Error::InvalidBreakpoint);
            }
            thresholds.push(threshold);
            styles.push(style);
        }
        Ok(Self { thresholds, styles })
    }
    fn select(&self, width: f32, direction: taffy::Direction) -> Style {
        let mut style = self.styles[self
            .thresholds
            .partition_point(|threshold| *threshold <= width)]
        .0
        .clone();
        style.direction = direction;
        style
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleQuery {
    ViewportInline,
    /// Fixed exterior width and height; target must be a render descendant.
    ContainerInline(NodeId),
}

struct Binding {
    source: Signal<Style>,
    target: NodeId,
    query: StyleQuery,
    styles: BreakpointStyles,
}

#[derive(Default)]
pub struct ResponsiveBindings {
    bindings: Vec<Binding>,
}

pub struct ResponsiveFrame {
    pub frame: FramePacket,
    pub rules_examined: usize,
    pub changed_styles: usize,
    /// Total layout entries across the bounded pre-query/final passes.
    pub layout_runs: u64,
    pub layout_stats: LayoutStats,
}

impl ResponsiveBindings {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Register an existing scoped Style source and synchronize its render sink.
    /// This engine becomes the source's sole producer; callers retain ownership.
    pub fn bind(
        &mut self,
        ui: &mut LogicalUi,
        source: Signal<Style>,
        target: NodeId,
        query: StyleQuery,
        styles: BreakpointStyles,
    ) -> Result<(), Error> {
        if self
            .bindings
            .iter()
            .any(|binding| binding.source == source || binding.target == target)
        {
            return Err(Error::BindingConflict);
        }
        validate_flex(ui.read(source)?)?;
        self.validate_query(ui.reactive().tree(), target, query)?;
        let tree = ui.reactive().tree();
        for binding in &self.bindings {
            if let StyleQuery::ContainerInline(host) = binding.query
                && (target == host
                    || matches!(query, StyleQuery::ContainerInline(_))
                        && Self::is_ancestor(tree, target, host)?)
            {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        ui.bind(source, Property::style(target))?;
        self.bindings.push(Binding {
            source,
            target,
            query,
            styles,
        });
        Ok(())
    }

    fn is_ancestor(tree: &UiTree, ancestor: NodeId, descendant: NodeId) -> Result<bool, Error> {
        let mut current = Some(descendant);
        while let Some(id) = current {
            if id == ancestor {
                return Ok(true);
            }
            current = tree.node(id)?.parent;
        }
        Ok(false)
    }

    fn validate_query(
        &self,
        tree: &UiTree,
        target: NodeId,
        query: StyleQuery,
    ) -> Result<(), Error> {
        tree.node(target)?;
        let StyleQuery::ContainerInline(container) = query else {
            return Ok(());
        };
        let host = tree.node(container)?;
        let definite = |value: taffy::Dimension| matches!(value.expand(), taffy::style::ExpandedDimension::Length(v) if v.is_finite() && v >= 0.0);
        let fixed_axis = |size: taffy::Dimension,
                          min: taffy::LengthPercentageAuto,
                          max: taffy::LengthPercentageAuto| {
            match (size.expand(), min.expand(), max.expand()) {
                (
                    taffy::style::ExpandedDimension::Length(size),
                    taffy::style::ExpandedLengthPercentageAuto::Length(min),
                    taffy::style::ExpandedLengthPercentageAuto::Length(max),
                ) => size == min && size == max,
                _ => false,
            }
        };
        let length = |value: taffy::LengthPercentage| match value.expand() {
            taffy::style::ExpandedLengthPercentage::Length(value)
                if value.is_finite() && value >= 0.0 =>
            {
                Some(value)
            }
            _ => None,
        };
        let insets = [
            host.style.padding.left,
            host.style.padding.right,
            host.style.padding.top,
            host.style.padding.bottom,
            host.style.border.left,
            host.style.border.right,
            host.style.border.top,
            host.style.border.bottom,
        ]
        .into_iter()
        .map(length)
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::UnsafeContainerQuery)?;
        let width = match host.style.size.width.expand() {
            taffy::style::ExpandedDimension::Length(v) => v,
            _ => return Err(Error::UnsafeContainerQuery),
        };
        let height = match host.style.size.height.expand() {
            taffy::style::ExpandedDimension::Length(v) => v,
            _ => return Err(Error::UnsafeContainerQuery),
        };
        if host.style.display != taffy::Display::Flex
            || host.style.box_sizing != taffy::BoxSizing::BorderBox
            || insets[0] + insets[1] + insets[4] + insets[5] > width
            || insets[2] + insets[3] + insets[6] + insets[7] > height
            || !definite(host.style.size.width)
            || !definite(host.style.size.height)
            || host.style.flex_grow != 0.0
            || host.style.flex_shrink != 0.0
            || host.style.flex_basis != taffy::Dimension::auto()
            || !fixed_axis(
                host.style.size.width,
                host.style.min_size.width,
                host.style.max_size.width,
            )
            || !fixed_axis(
                host.style.size.height,
                host.style.min_size.height,
                host.style.max_size.height,
            )
            || self
                .bindings
                .iter()
                .any(|binding| binding.target == container)
        {
            return Err(Error::UnsafeContainerQuery);
        }
        for binding in &self.bindings {
            if matches!(binding.query, StyleQuery::ContainerInline(_))
                && Self::is_ancestor(tree, binding.target, container)?
            {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        let mut ancestor = tree.node(target)?.parent;
        while let Some(id) = ancestor {
            if id == container {
                return Ok(());
            }
            ancestor = tree.node(id)?.parent;
        }
        Err(Error::UnsafeContainerQuery)
    }

    /// Resolve viewport rules, lay out fixed query containers, then resolve
    /// container rules in one further bounded pass. Never iterate to convergence.
    pub fn frame(
        &mut self,
        ui: &mut LogicalUi,
        environment: &LayoutEnvironment,
    ) -> Result<ResponsiveFrame, Error> {
        environment.validate()?;
        self.bindings.retain(|binding| {
            ui.read(binding.source).is_ok() && ui.reactive().tree().node(binding.target).is_ok()
        });
        for binding in &self.bindings {
            self.validate_query(ui.reactive().tree(), binding.target, binding.query)?;
        }
        let viewport = self
            .bindings
            .iter()
            .filter(|binding| matches!(binding.query, StyleQuery::ViewportInline))
            .map(|binding| {
                (
                    binding.source,
                    binding
                        .styles
                        .select(environment.layout_viewport[0], environment.direction),
                )
            })
            .collect::<Vec<_>>();
        let first = Self::apply(ui, viewport)?;
        let before = ui.frame(environment.layout_viewport)?;
        let tree = ui.reactive().tree();
        let mut containers = vec![];
        for binding in &self.bindings {
            if let StyleQuery::ContainerInline(container) = binding.query {
                let layout = tree.layout.layout(tree.node(container)?.layout)?;
                // Read actual resolved content width, including padding, border
                // and scrollbar allocation. Never substitute parent max-width.
                let width = (layout.size.width
                    - layout.padding.left
                    - layout.padding.right
                    - layout.border.left
                    - layout.border.right
                    - layout.scrollbar_size.width)
                    .max(0.0);
                containers.push((
                    binding.source,
                    binding.styles.select(width, environment.direction),
                ));
            }
        }
        let second = Self::apply(ui, containers)?;
        let frame = if second > 0 {
            ui.frame(environment.layout_viewport)?
        } else {
            before.clone()
        };
        let layout_stats = LayoutStats {
            layout_writes: before.layout_stats.layout_writes
                + if second > 0 {
                    frame.layout_stats.layout_writes
                } else {
                    0
                },
            changed_layout_writes: before.layout_stats.changed_layout_writes
                + if second > 0 {
                    frame.layout_stats.changed_layout_writes
                } else {
                    0
                },
            net_changed_nodes: before.layout_stats.net_changed_nodes
                + if second > 0 {
                    frame.layout_stats.net_changed_nodes
                } else {
                    0
                },
            leaf_layouts: before.layout_stats.leaf_layouts
                + if second > 0 {
                    frame.layout_stats.leaf_layouts
                } else {
                    0
                },
            measure_calls: before.layout_stats.measure_calls
                + if second > 0 {
                    frame.layout_stats.measure_calls
                } else {
                    0
                },
        };
        Ok(ResponsiveFrame {
            layout_runs: before.layout_runs + if second > 0 { frame.layout_runs } else { 0 },
            frame,
            rules_examined: self.bindings.len(),
            changed_styles: first + second,
            layout_stats,
        })
    }

    fn apply(ui: &mut LogicalUi, values: Vec<(Signal<Style>, Style)>) -> Result<usize, Error> {
        let mut changes = vec![];
        for (source, value) in values {
            if *ui.read(source)? != value {
                changes.push((source, value));
            }
        }
        if changes.is_empty() {
            return Ok(0);
        }
        Ok(ui
            .transaction(|tx| {
                for (source, value) in changes {
                    tx.set(source, value)?;
                }
                Ok(())
            })?
            .changed_sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> LogicalUi {
        LogicalUi::new(
            Style {
                size: Size {
                    width: percent(1.0),
                    height: percent(1.0),
                },
                ..Default::default()
            },
            0,
            BackendCapabilities::default(),
        )
        .unwrap()
    }
    fn bounds(frame: &FramePacket, node: NodeId) -> Rect {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    }
    fn rules(threshold: f32) -> BreakpointStyles {
        BreakpointStyles::new(FlexStyle::column(), vec![(threshold, FlexStyle::row())]).unwrap()
    }
    fn host_style() -> Style {
        Style {
            size: Size {
                width: length(140.0),
                height: length(80.0),
            },
            min_size: Size {
                width: length(140.0),
                height: length(80.0),
            },
            max_size: Size {
                width: length(140.0),
                height: length(80.0),
            },
            padding: taffy::Rect {
                left: length(10.0),
                right: length(10.0),
                ..taffy::Rect::zero()
            },
            flex_shrink: 0.0,
            ..Default::default()
        }
    }

    #[test]
    fn viewport_rules_change_only_at_threshold_and_preserve_ids_and_chunks() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let target = ui
            .append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap();
        let source = ui.state(root, 0, Style::default).unwrap();
        let a = ui
            .append_render(
                root,
                target,
                Style {
                    size: Size {
                        width: length(30.0),
                        height: length(20.0),
                    },
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        let b = ui
            .append_render(
                root,
                target,
                Style {
                    size: Size {
                        width: length(30.0),
                        height: length(20.0),
                    },
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                source,
                target,
                StyleQuery::ViewportInline,
                rules(100.0),
            )
            .unwrap();
        let before = responsive
            .frame(&mut ui, &LayoutEnvironment::new([99.0, 100.0]))
            .unwrap();
        assert_eq!(
            (before.changed_styles, bounds(&before.frame, b).y),
            (1, 20.0)
        );
        let wider = responsive
            .frame(&mut ui, &LayoutEnvironment::new([100.0, 100.0]))
            .unwrap();
        assert_eq!((wider.changed_styles, bounds(&wider.frame, b).x), (1, 30.0));
        assert!(Arc::ptr_eq(
            &before.frame.items[2].commands,
            &wider.frame.items[2].commands
        ));
        assert_eq!(wider.frame.hit_test(35.0, 5.0), Some(b));
        let resized = responsive
            .frame(&mut ui, &LayoutEnvironment::new([101.0, 100.0]))
            .unwrap();
        assert_eq!(
            (resized.changed_styles, resized.frame.recorded_chunks),
            (0, 0)
        );
        let idle = responsive
            .frame(&mut ui, &LayoutEnvironment::new([101.0, 100.0]))
            .unwrap();
        assert_eq!((idle.layout_runs, idle.changed_styles), (0, 0));
        assert!(Arc::ptr_eq(&resized.frame.items, &idle.frame.items));
        let mut environment = LayoutEnvironment::new([101.0, 100.0]);
        environment.device_scale = 1.25;
        environment.application_zoom = 2.0;
        environment.text_scale = 1.5;
        let scale_only = responsive.frame(&mut ui, &environment).unwrap();
        assert_eq!((scale_only.changed_styles, scale_only.layout_runs), (0, 0));
        environment.direction = taffy::Direction::Rtl;
        let rtl = responsive.frame(&mut ui, &environment).unwrap();
        assert_eq!(rtl.changed_styles, 1);
        assert!(bounds(&rtl.frame, a).x > bounds(&rtl.frame, b).x);
    }

    #[test]
    fn fixed_container_reads_resolved_content_width_and_never_parent_constraint() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let host = ui
            .append_render(root, ui.root_render(), host_style(), 0)
            .unwrap();
        let target = ui.append_render(root, host, Style::default(), 0).unwrap();
        let source = ui.state(root, 0, Style::default).unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                source,
                target,
                StyleQuery::ContainerInline(host),
                rules(130.0),
            )
            .unwrap();
        let result = responsive
            .frame(&mut ui, &LayoutEnvironment::new([600.0, 300.0]))
            .unwrap();
        // Border box is 140; resolved content box is 120. A parent-max-width
        // approximation would wrongly select the wide row style.
        assert_eq!(
            ui.read(source).unwrap().flex_direction,
            taffy::FlexDirection::Column
        );
        assert_eq!(result.layout_runs, 2);
        assert_eq!(result.changed_styles, 1);
        assert_eq!(
            responsive
                .frame(&mut ui, &LayoutEnvironment::new([600.0, 300.0]))
                .unwrap()
                .layout_runs,
            0
        );
    }

    #[test]
    fn unsafe_container_dependencies_reject_before_binding_or_frame_mutation() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap();
        let target = ui.append_render(root, host, Style::default(), 0).unwrap();
        let source = ui.state(root, 0, Style::default).unwrap();
        let mut responsive = ResponsiveBindings::new();
        assert_eq!(
            responsive.bind(
                &mut ui,
                source,
                target,
                StyleQuery::ContainerInline(host),
                rules(100.0)
            ),
            Err(Error::UnsafeContainerQuery)
        );
        assert_eq!(ui.reactive().binding_count(), 0);
        assert!(responsive.is_empty());
        let foreign = fixture().root_render();
        assert_eq!(
            responsive.bind(
                &mut ui,
                source,
                target,
                StyleQuery::ContainerInline(foreign),
                rules(100.0)
            ),
            Err(Error::InvalidNode)
        );
        let host = ui
            .append_render(root, ui.root_render(), host_style(), 0)
            .unwrap();
        assert_eq!(
            responsive.bind(
                &mut ui,
                source,
                host,
                StyleQuery::ContainerInline(host),
                rules(100.0)
            ),
            Err(Error::UnsafeContainerQuery)
        );
        let before = ui.frame([600.0, 300.0]).unwrap();
        let mut invalid = LayoutEnvironment::new([600.0, 300.0]);
        invalid.text_scale = f32::NAN;
        assert!(matches!(
            responsive.frame(&mut ui, &invalid),
            Err(Error::InvalidEnvironment)
        ));
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    }

    #[test]
    fn retired_scope_sources_remove_rules_and_new_handles_do_not_inherit_them() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let target = ui
            .append_render(scope, ui.root_render(), Style::default(), 0)
            .unwrap();
        let source = ui.state(scope, 0, Style::default).unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                source,
                target,
                StyleQuery::ViewportInline,
                rules(100.0),
            )
            .unwrap();
        responsive
            .frame(&mut ui, &LayoutEnvironment::new([120.0, 100.0]))
            .unwrap();
        ui.reconcile_keys(root, &[]).unwrap();
        let result = responsive
            .frame(&mut ui, &LayoutEnvironment::new([120.0, 100.0]))
            .unwrap();
        assert_eq!((responsive.len(), result.rules_examined), (0, 0));
        assert_eq!(result.frame.items.len(), 1);
    }

    #[test]
    fn public_flex_subset_rejects_unimplemented_layout_and_invalid_thresholds() {
        assert_eq!(
            (
                FlexStyle::row().style().flex_grow,
                FlexStyle::row().style().flex_shrink
            ),
            (0.0, 1.0)
        );
        assert_eq!(
            FlexStyle::row().style().box_sizing,
            taffy::BoxSizing::BorderBox
        );
        for style in [
            Style {
                display: taffy::Display::Grid,
                ..Default::default()
            },
            Style {
                flex_wrap: taffy::FlexWrap::WrapReverse,
                ..Default::default()
            },
            Style {
                position: taffy::Position::Sticky,
                ..Default::default()
            },
            Style {
                float: taffy::Float::Left,
                ..Default::default()
            },
        ] {
            assert!(matches!(
                FlexStyle::try_from_style(style),
                Err(Error::UnsupportedLayout)
            ));
        }
        for style in [
            Style {
                flex_grow: f32::NAN,
                ..Default::default()
            },
            Style {
                gap: Size {
                    width: length(-1.0),
                    height: length(0.0),
                },
                ..Default::default()
            },
            Style {
                aspect_ratio: Some(0.0),
                ..Default::default()
            },
        ] {
            assert!(matches!(
                FlexStyle::try_from_style(style),
                Err(Error::InvalidStyle)
            ));
        }
        assert!(matches!(
            BreakpointStyles::new(
                FlexStyle::row(),
                vec![(10.0, FlexStyle::column()), (10.0, FlexStyle::row())]
            ),
            Err(Error::InvalidBreakpoint)
        ));
        assert!(matches!(
            BreakpointStyles::new(FlexStyle::row(), vec![(f32::NAN, FlexStyle::column())]),
            Err(Error::InvalidBreakpoint)
        ));
    }

    #[test]
    fn source_outlives_removed_target_and_nested_query_edges_are_rejected() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let outer = ui
            .append_render(scope, ui.root_render(), host_style(), 0)
            .unwrap();
        let inner = ui.append_render(scope, outer, host_style(), 0).unwrap();
        let leaf = ui.append_render(scope, inner, Style::default(), 0).unwrap();
        let a = ui.state(root, 0, Style::default).unwrap();
        let b = ui.state(root, 1, Style::default).unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                a,
                leaf,
                StyleQuery::ContainerInline(inner),
                rules(100.0),
            )
            .unwrap();
        assert_eq!(
            responsive.bind(
                &mut ui,
                b,
                inner,
                StyleQuery::ContainerInline(outer),
                rules(100.0)
            ),
            Err(Error::UnsafeContainerQuery)
        );
        assert_eq!(
            responsive.bind(&mut ui, b, inner, StyleQuery::ViewportInline, rules(100.0)),
            Err(Error::UnsafeContainerQuery)
        );
        assert_eq!(ui.reactive().binding_count(), 1);
        ui.reconcile_keys(root, &[]).unwrap();
        assert!(ui.read(a).is_ok());
        responsive
            .frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
            .unwrap();
        assert!(responsive.is_empty());
    }

    #[test]
    fn responsive_visibility_removes_whole_subtree_and_restores_same_paint_chunks() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let parent = ui
            .append_render(root, ui.root_render(), Style::default(), 0x334455ff)
            .unwrap();
        let child = ui
            .append_render(
                root,
                parent,
                Style {
                    size: Size {
                        width: length(20.0),
                        height: length(20.0),
                    },
                    ..Default::default()
                },
                0xff8844ff,
            )
            .unwrap();
        let source = ui.state(root, 0, Style::default).unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                source,
                parent,
                StyleQuery::ViewportInline,
                BreakpointStyles::new(
                    FlexStyle::row(),
                    vec![(
                        100.0,
                        FlexStyle::try_from_style(Style {
                            display: taffy::Display::None,
                            ..Default::default()
                        })
                        .unwrap(),
                    )],
                )
                .unwrap(),
            )
            .unwrap();
        let before = responsive
            .frame(&mut ui, &LayoutEnvironment::new([99.0, 80.0]))
            .unwrap()
            .frame;
        assert_eq!(before.hit_test(5.0, 5.0), Some(child));
        let hidden = responsive
            .frame(&mut ui, &LayoutEnvironment::new([100.0, 80.0]))
            .unwrap()
            .frame;
        assert_eq!(hidden.items.len(), 1);
        assert!(hidden.removed.contains(&child));
        assert_ne!(hidden.hit_test(5.0, 5.0), Some(child));
        let restored = responsive
            .frame(&mut ui, &LayoutEnvironment::new([99.0, 80.0]))
            .unwrap()
            .frame;
        assert_eq!(restored.recorded_chunks, 0);
        assert!(Arc::ptr_eq(
            &before.items[2].commands,
            &restored.items[2].commands
        ));
        assert_eq!(restored.hit_test(5.0, 5.0), Some(child));
    }
}
