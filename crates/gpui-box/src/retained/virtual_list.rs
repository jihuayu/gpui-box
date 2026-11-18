//! Bounded vertical lists over ordinary Flex, keyed components and sources.
//! Extents are declared immutable data, independent of fonts or a render backend.
use super::{logical::*, reactive::*, *};
use std::{marker::PhantomData, ops::Range, rc::Rc};
mod anchoring;
pub(in crate::retained) mod measurement;
pub use anchoring::{VirtualListRebase, VirtualListRebaseReason, VirtualListScrollPolicy};
pub use measurement::{MeasuredVirtualList, MeasuredVirtualListProps, VirtualListMeasureReport};

#[derive(Clone, Debug)]
enum Extents {
    Fixed { count: usize, height: f64 },
    Variable(Arc<[f64]>),
}
impl PartialEq for Extents {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Fixed {
                    count: a,
                    height: x,
                },
                Self::Fixed {
                    count: b,
                    height: y,
                },
            ) => a == b && x == y,
            (Self::Variable(a), Self::Variable(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}
/// Declared positive border-box row heights, with no inter-row margins/gap.
/// Fixed lists store O(1) data; variable lists store one immutable prefix table.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListMetrics(Extents);
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListWindow {
    pub range: Range<usize>,
    pub offset: f32,
    pub maximum: f32,
    pub leading: f32,
    pub trailing: f32,
    pub total: f32,
}
impl VirtualListMetrics {
    pub fn fixed(count: usize, height: f32) -> Result<Self, Error> {
        if !height.is_finite() || height <= 0.0 {
            return Err(Error::InvalidVirtualList);
        }
        let total = count as f64 * f64::from(height);
        if !total.is_finite() || total > f64::from(f32::MAX) {
            return Err(Error::InvalidVirtualList);
        }
        Ok(Self(Extents::Fixed {
            count,
            height: f64::from(height),
        }))
    }
    pub fn variable(heights: impl IntoIterator<Item = f32>) -> Result<Self, Error> {
        let mut prefix = vec![0.0];
        for height in heights {
            if !height.is_finite() || height <= 0.0 {
                return Err(Error::InvalidVirtualList);
            }
            let previous = *prefix.last().unwrap();
            let next = previous + f64::from(height);
            if next <= previous || next > f64::from(f32::MAX) {
                return Err(Error::InvalidVirtualList);
            }
            prefix.push(next);
        }
        Ok(Self(Extents::Variable(prefix.into())))
    }
    pub fn len(&self) -> usize {
        match &self.0 {
            Extents::Fixed { count, .. } => *count,
            Extents::Variable(p) => p.len() - 1,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn edge(&self, index: usize) -> f64 {
        match &self.0 {
            Extents::Fixed { height, .. } => index as f64 * height,
            Extents::Variable(p) => p[index],
        }
    }
    pub fn total(&self) -> f32 {
        self.edge(self.len()) as f32
    }
    pub fn maximum(&self, viewport: f32) -> Result<f32, Error> {
        if !viewport.is_finite() || viewport < 0.0 {
            return Err(Error::InvalidVirtualList);
        }
        Ok((self.edge(self.len()) - f64::from(viewport)).max(0.0) as f32)
    }
    pub fn height(&self, index: usize) -> Result<f32, Error> {
        if index >= self.len() {
            return Err(Error::InvalidVirtualList);
        }
        Ok((self.edge(index + 1) - self.edge(index)) as f32)
    }
    /// Replace declared/measured extents together. Same values retain the shared
    /// metrics; changed snapshots rebuild one prefix table, without row payloads.
    pub fn with_heights(
        &self,
        updates: impl IntoIterator<Item = (usize, f32)>,
    ) -> Result<Self, Error> {
        let mut changes = BTreeMap::new();
        for (index, height) in updates {
            if index >= self.len() || !height.is_finite() || height <= 0.0 {
                return Err(Error::InvalidVirtualList);
            }
            changes.insert(index, height);
        }
        changes.retain(|index, height| self.height(*index).is_ok_and(|old| old != *height));
        if changes.is_empty() {
            return Ok(self.clone());
        }
        let mut changes = changes.into_iter().peekable();
        Self::variable((0..self.len()).map(|i| {
            if changes.peek().is_some_and(|(index, _)| *index == i) {
                changes.next().unwrap().1
            } else {
                self.height(i).expect("validated metric index")
            }
        }))
    }
    /// O(log N) selection, O(1) for fixed extents. No per-row UI allocation.
    pub fn window(
        &self,
        offset: f32,
        viewport: f32,
        overscan: f32,
        max_rows: usize,
    ) -> Result<VirtualListWindow, Error> {
        if [offset, viewport, overscan]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || max_rows == 0
        {
            return Err(Error::InvalidVirtualList);
        }
        let total = self.edge(self.len());
        let maximum = self.maximum(viewport)?;
        let offset = offset.min(maximum);
        let start = (f64::from(offset) - f64::from(overscan)).max(0.0);
        let end = (f64::from(offset) + f64::from(viewport) + f64::from(overscan)).min(total);
        let range = if viewport == 0.0 || self.is_empty() {
            0..0
        } else {
            match &self.0 {
                Extents::Fixed { height, count } => {
                    ((start / height).floor() as usize).min(*count)
                        ..((end / height).ceil() as usize).min(*count)
                }
                Extents::Variable(p) => {
                    p.partition_point(|v| *v <= start).saturating_sub(1)
                        ..p.partition_point(|v| *v < end).min(self.len())
                }
            }
        };
        if range.len() > max_rows {
            return Err(Error::VirtualListLimit);
        }
        Ok(VirtualListWindow {
            leading: self.edge(range.start) as f32,
            trailing: (total - self.edge(range.end)) as f32,
            range,
            offset,
            maximum,
            total: total as f32,
        })
    }
    /// Explicit programmatic reveal for an off-window row. Focus remains a
    /// separate request after the host installs the newly mounted row's scene.
    pub fn reveal(&self, index: usize, offset: f32, viewport: f32) -> Result<f32, Error> {
        if index >= self.len()
            || [offset, viewport]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidVirtualList);
        }
        let start = self.edge(index);
        let end = self.edge(index + 1);
        let offset = f64::from(offset).min((self.edge(self.len()) - f64::from(viewport)).max(0.0));
        let next = if start < offset {
            start
        } else if end > offset + f64::from(viewport) {
            start.min(end - f64::from(viewport))
        } else {
            offset
        };
        Ok(next.min((self.edge(self.len()) - f64::from(viewport)).max(0.0)) as f32)
    }
}

type RowFactory<C> = dyn Fn(usize) -> Result<(u64, <C as Component>::Props), Error>;
/// Stable dataset identity. Replace this handle when rows/order/payload change;
/// clone to retain it. Factories must be pure; only selected indices are called.
pub struct VirtualListItems<C: Component> {
    metrics: VirtualListMetrics,
    row: Rc<RowFactory<C>>,
    keys: Option<Arc<anchoring::Keys>>,
}
impl<C: Component> Clone for VirtualListItems<C> {
    fn clone(&self) -> Self {
        Self {
            metrics: self.metrics.clone(),
            row: self.row.clone(),
            keys: self.keys.clone(),
        }
    }
}
impl<C: Component> PartialEq for VirtualListItems<C> {
    fn eq(&self, other: &Self) -> bool {
        self.metrics == other.metrics && Rc::ptr_eq(&self.row, &other.row)
    }
}
impl<C: Component> VirtualListItems<C> {
    pub fn new(
        metrics: VirtualListMetrics,
        row: impl Fn(usize) -> Result<(u64, C::Props), Error> + 'static,
    ) -> Self {
        Self {
            metrics,
            row: Rc::new(row),
            keys: None,
        }
    }
    /// Build an immutable ordered key/inverse index without constructing rows.
    /// Whole dataset uniqueness and metric length are validated once. Replacing
    /// data still uses a new handle; cloning shares its index and factory.
    pub fn keyed(
        metrics: VirtualListMetrics,
        keys: impl IntoIterator<Item = u64>,
        row: impl Fn(usize) -> Result<C::Props, Error> + 'static,
    ) -> Result<Self, Error> {
        let keys = Arc::new(anchoring::Keys::new(keys, metrics.len())?);
        let lookup = keys.clone();
        Ok(Self {
            metrics,
            row: Rc::new(move |index| Ok((lookup.order[index], row(index)?))),
            keys: Some(keys),
        })
    }
    /// Read an indexed stable key without invoking the payload factory.
    pub fn key(&self, index: usize) -> Result<u64, Error> {
        self.keys
            .as_ref()
            .and_then(|keys| keys.order.get(index))
            .copied()
            .ok_or(Error::InvalidVirtualList)
    }
    pub fn metrics(&self) -> &VirtualListMetrics {
        &self.metrics
    }
    pub fn with_heights(
        &self,
        updates: impl IntoIterator<Item = (usize, f32)>,
    ) -> Result<Self, Error> {
        Ok(Self {
            metrics: self.metrics.with_heights(updates)?,
            row: self.row.clone(),
            keys: self.keys.clone(),
        })
    }
}
pub struct VirtualListProps<C: Component> {
    pub items: VirtualListItems<C>,
    /// Source-owned offset, shared with wheel/programmatic controls.
    pub scroll: Signal<[f32; 2]>,
    /// Explicit logical content-box viewport. May come from a proven Flex
    /// allocation builder; it never guesses container width from window size.
    pub viewport: [f32; 2],
    pub overscan: f32,
    pub max_rows: usize,
}
impl<C: Component> Clone for VirtualListProps<C> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            scroll: self.scroll,
            viewport: self.viewport,
            overscan: self.overscan,
            max_rows: self.max_rows,
        }
    }
}
impl<C: Component> PartialEq for VirtualListProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
            && self.scroll == other.scroll
            && self.viewport == other.viewport
            && self.overscan == other.overscan
            && self.max_rows == other.max_rows
    }
}
impl<C: Component> VirtualListProps<C> {
    pub fn new(items: VirtualListItems<C>, scroll: Signal<[f32; 2]>, viewport: [f32; 2]) -> Self {
        Self {
            items,
            scroll,
            viewport,
            overscan: 0.0,
            max_rows: 512,
        }
    }
    fn validate(&self) -> Result<(), Error> {
        if self.viewport.iter().any(|v| !v.is_finite() || *v < 0.0)
            || !self.overscan.is_finite()
            || self.overscan < 0.0
            || self.max_rows == 0
        {
            return Err(Error::InvalidVirtualList);
        }
        Ok(())
    }
}

fn spacer(height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(height))
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
struct RowProps<C: Component> {
    height: f32,
    payload: C::Props,
}
impl<C: Component> PartialEq for RowProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.height == other.height && self.payload == other.payload
    }
}
struct Row<C>(PhantomData<C>);
impl<C: Component> Component for Row<C>
where
    C::Props: Clone,
{
    type Props = RowProps<C>;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let style = cx.computed(0, &[props.dependency()], move |i| {
            Ok(spacer(i.read_prop(props)?.height)
                .overflow(LayoutOverflow::Clip, LayoutOverflow::Clip))
        })?;
        cx.bind(style, Property::layout_style(host))?;
        // A row gets an ordinary bounded layout box. Its content can use normal
        // Flex/text/images and cannot paint/hit outside the declared extent.
        let site = cx.component_sites(&[0])?[0];
        cx.layout_producer(
            0,
            host,
            LayoutBuildKind::Content,
            &[],
            LayoutProducer::new(move |_, edit| {
                let payload = edit.read_prop(props)?.payload.clone();
                edit.components_at::<C>(site, host, [(0, payload)])?;
                Ok(())
            }),
        )?;
        Ok(())
    }
}

struct Spacer;
impl Component for Spacer {
    type Props = f32;
    fn mount(props: ReadProp<f32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        let style = cx.computed(0, &[props.dependency()], move |i| {
            Ok(spacer(*i.read_prop(props)?))
        })?;
        cx.bind(style, Property::layout_style(node))?;
        Ok(())
    }
}

/// Vertical declared-extent list. Off-window components unmount normally;
/// persistent drafts/selection belong in parent/business state keyed by row ID.
/// Row Props must be Clone; use compact immutable handles for large data.
///
/// ```
/// use gpui_box::retained::{*, logical::*, virtual_list::*};
/// struct Row;
/// impl Component for Row {
///     type Props = u32;
///     fn mount(p: ReadProp<u32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
///         let node = cx.append_layout(LayoutStyle::row()
///             .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(20.0)), 0)?;
///         cx.bind_prop(p, reactive::Property::color(node))?;
///         Ok(())
///     }
/// }
/// let mut ui = LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default())?;
/// let root = ui.root_scope(); let host = ui.root_render();
/// let scroll = ui.state(root, 0, || [0.0; 2])?;
/// let items = VirtualListItems::<Row>::new(VirtualListMetrics::fixed(100_000, 20.0)?,
///     |index| Ok((index as u64, 0x223344ff)));
/// ui.edit_local_frame(LayoutConstraints::viewport(300.0, 200.0), [300.0, 200.0],
///     |_| Ok(()), |(), edit| edit.components::<VirtualList<Row>>(root, host,
///         [(1, VirtualListProps::new(items, scroll, [300.0, 200.0]))]))?;
/// # Ok::<(), Error>(())
/// ```
pub struct VirtualList<C>(PhantomData<C>);
impl<C: Component> Component for VirtualList<C>
where
    C::Props: Clone,
{
    type Props = VirtualListProps<C>;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        cx.read_prop(props)?.validate()?;
        let scroll = cx.read_prop(props)?.scroll;
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let sites = cx.component_sites(&[0, 1, 2])?;
        let placement = cx.computed(0, &[props.dependency(), scroll.dependency()], move |i| {
            let p = i.read_prop(props)?;
            p.validate()?;
            if p.scroll != scroll {
                return Err(Error::InvalidVirtualList);
            }
            let offset = *i.read(scroll)?;
            if offset.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(Error::InvalidScroll);
            }
            let maximum = p.items.metrics.maximum(p.viewport[1])?;
            Ok([0.0, offset[1].min(maximum)])
        })?;
        cx.bind(placement, Property::scroll(host))?;
        cx.layout_producer(
            0,
            host,
            LayoutBuildKind::Content,
            &[],
            LayoutProducer::new(move |_, edit| {
                let p = edit.read_prop(props)?;
                p.validate()?;
                let offset = *edit.read(p.scroll)?;
                if offset.iter().any(|v| !v.is_finite() || *v < 0.0) {
                    return Err(Error::InvalidScroll);
                }
                let window =
                    p.items
                        .metrics
                        .window(offset[1], p.viewport[1], p.overscan, p.max_rows)?;
                let viewport = p.viewport;
                let mut selected = Vec::with_capacity(window.range.len());
                let mut keys = std::collections::BTreeSet::new();
                for index in window.range.clone() {
                    let (key, payload) = (p.items.row)(index)?;
                    if !keys.insert(key) {
                        return Err(Error::DuplicateKey);
                    }
                    selected.push((
                        key,
                        RowProps::<C> {
                            height: p.items.metrics.height(index)?,
                            payload,
                        },
                    ));
                }
                edit.properties().layout_style(
                    host,
                    LayoutStyle::column()
                        .size(
                            LayoutDimension::Px(viewport[0]),
                            LayoutDimension::Px(viewport[1]),
                        )
                        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
                        .flex_shrink(0.0)
                        .overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
                );
                let mut plan = ComponentPlan::new();
                plan.select::<Spacer>(sites[0], host, 0, window.leading);
                plan.list::<Row<C>>(sites[1], host, selected);
                plan.select::<Spacer>(sites[2], host, 0, window.trailing);
                edit.apply_components(plan)?;
                Ok(())
            }),
        )?;
        let handler = cx.event_handler(
            0,
            EventHandler::new(move |event, cx| {
                if event.kind == input::UiInputEventKind::Wheel
                    && !cx.default_prevented()
                    && cx.remaining_wheel_delta().is_some()
                {
                    let p = cx.read_prop(props)?;
                    p.validate()?;
                    let maximum = p.items.metrics.maximum(p.viewport[1])?;
                    let scroll = p.scroll;
                    cx.scroll_wheel(scroll, [0.0, maximum], [false, true])?;
                }
                Ok(())
            }),
        )?;
        cx.interaction(
            host,
            Some(input::UiInteraction {
                pointer_handler: Some(handler),
                ..Default::default()
            }),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
