//! Current committed component geometry is queried through normal Flex/Taffy.
//! Feedback is one explicit bounded owner transaction, never a layout callback.
use super::*;

#[derive(Default)]
pub struct VirtualListMeasureReport {
    pub measured_rows: usize,
    pub changed_rows: usize,
    /// Complete source/frame adoption. None when all measured extents match.
    pub commit: Option<ReactiveCommit<()>>,
}
type Apply = dyn FnOnce(&mut ReactiveTransaction<'_>) -> Result<(), Error>;
pub(in crate::retained) struct Update {
    pub measured: usize,
    pub changed: usize,
    pub apply: Box<Apply>,
}
pub(in crate::retained) trait Measure {
    fn validate(&self, edit: &LogicalEdit<'_, '_, '_>) -> Result<[Dependency; 2], Error>;
    fn collect(&self, ui: &LogicalUi, budget: usize) -> Result<Update, Error>;
}
/// An opt-in auto-height list. Items is a keyed dataset source holding estimates
/// for unmeasured rows. Viewport is the explicit allocated content box.
pub struct MeasuredVirtualListProps<C: Component> {
    pub items: Signal<VirtualListItems<C>>,
    pub scroll: Signal<[f32; 2]>,
    pub viewport: [f32; 2],
    pub overscan: f32,
    pub max_rows: usize,
    /// Positive floor for empty content, retaining a navigable metric interval.
    pub minimum_height: f32,
    pub scroll_policy: VirtualListScrollPolicy,
}
impl<C: Component> Clone for MeasuredVirtualListProps<C> {
    fn clone(&self) -> Self {
        Self {
            items: self.items,
            scroll: self.scroll,
            viewport: self.viewport,
            overscan: self.overscan,
            max_rows: self.max_rows,
            minimum_height: self.minimum_height,
            scroll_policy: self.scroll_policy,
        }
    }
}
impl<C: Component> PartialEq for MeasuredVirtualListProps<C> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
            && self.scroll == other.scroll
            && self.viewport == other.viewport
            && self.overscan == other.overscan
            && self.max_rows == other.max_rows
            && self.minimum_height == other.minimum_height
            && self.scroll_policy == other.scroll_policy
    }
}
impl<C: Component> MeasuredVirtualListProps<C> {
    pub fn new(
        items: Signal<VirtualListItems<C>>,
        scroll: Signal<[f32; 2]>,
        viewport: [f32; 2],
    ) -> Self {
        Self {
            items,
            scroll,
            viewport,
            overscan: 0.0,
            max_rows: 512,
            minimum_height: 1.0,
            scroll_policy: VirtualListScrollPolicy::KeepVisible,
        }
    }
    fn validate(&self) -> Result<(), Error> {
        if self.viewport.iter().any(|v| !v.is_finite() || *v < 0.0)
            || !self.overscan.is_finite()
            || self.overscan < 0.0
            || self.max_rows == 0
            || !self.minimum_height.is_finite()
            || self.minimum_height <= 0.0
        {
            return Err(Error::InvalidVirtualList);
        }
        if let VirtualListScrollPolicy::FollowEnd { tolerance } = self.scroll_policy
            && (!tolerance.is_finite() || tolerance < 0.0)
        {
            return Err(Error::InvalidVirtualList);
        }
        Ok(())
    }
}
struct Typed<C: Component> {
    props: ReadProp<MeasuredVirtualListProps<C>>,
    site: ComponentSite,
    items: Signal<VirtualListItems<C>>,
    scroll: Signal<[f32; 2]>,
}
impl<C: Component> Measure for Typed<C> {
    fn validate(&self, edit: &LogicalEdit<'_, '_, '_>) -> Result<[Dependency; 2], Error> {
        let p = edit.read_prop(self.props)?;
        p.validate()?;
        if p.items != self.items || p.scroll != self.scroll || edit.read(p.items)?.keys.is_none() {
            return Err(Error::InvalidVirtualList);
        }
        let offset = edit.read(p.scroll)?;
        if offset.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(Error::InvalidScroll);
        }
        Ok([p.items.dependency(), p.scroll.dependency()])
    }
    fn collect(&self, ui: &LogicalUi, budget: usize) -> Result<Update, Error> {
        let p = ui.read_prop(self.props)?;
        p.validate()?;
        let items = ui.read(p.items)?.clone();
        let keys = items.keys.as_ref().ok_or(Error::InvalidVirtualList)?;
        let list = *ui
            .children(self.site.scope())?
            .first()
            .ok_or(Error::InvalidVirtualList)?;
        let host = *ui.renders(list)?.first().ok_or(Error::InvalidVirtualList)?;
        let tree = ui.reactive().tree();
        let mut ancestor = Some(host);
        while let Some(id) = ancestor {
            let node = tree.node(id)?;
            if node.style.display == taffy::Display::None {
                return Ok(Update {
                    measured: 0,
                    changed: 0,
                    apply: Box::new(|_| Ok(())),
                });
            }
            ancestor = node.parent;
        }
        let viewport = ui.layout_result(host)?.size;
        if viewport[0] <= 0.0 || viewport[1] <= 0.0 {
            return Ok(Update {
                measured: 0,
                changed: 0,
                apply: Box::new(|_| Ok(())),
            });
        }
        let sites = ui.children(list)?;
        if sites.len() != 3 {
            return Err(Error::InvalidVirtualList);
        }
        let rows = ui.children(sites[1])?;
        if rows.len() > budget {
            return Err(Error::VirtualListLimit);
        }
        let current = ui
            .published_local_frame()
            .is_some_and(|f| f.revision() == tree.revision)
            || tree
                .published_frame()
                .is_some_and(|f| f.revision == tree.revision);
        if !current || tree.layout_dirty {
            return Err(Error::InvalidFrame);
        }
        let input = ChildLayoutInput {
            sizing_mode: LayoutSizingMode::Content,
            known_size: LayoutSize::new(Some(viewport[0]), None),
            percentage_basis: LayoutSize::new(Some(viewport[0]), None),
            ..ChildLayoutInput::new(
                LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(viewport[0]),
                    LayoutAvailableSpace::Unbounded,
                )
                .with_bounds(BoxConstraints::new(
                    [viewport[0], 0.0],
                    [viewport[0], f32::INFINITY],
                )),
            )
        };
        let mut indices = vec![];
        let mut queries = vec![];
        for row in rows {
            let key = ui.key(*row)?;
            let index = *keys.positions.get(&key).ok_or(Error::InvalidVirtualList)?;
            let nodes = ui.renders(*row)?;
            if nodes.len() != 1 {
                return Err(Error::InvalidVirtualList);
            }
            indices.push(index);
            queries.push((nodes[0], input));
        }
        let results = ui.reactive().tree().dry_measure_nodes(&queries)?;
        let mut changes = vec![];
        for (index, result) in indices.into_iter().zip(results) {
            if !result.size[1].is_finite() || result.size[1] < 0.0 {
                return Err(Error::InvalidVirtualList);
            }
            let height = result.size[1].max(p.minimum_height);
            if !height.is_finite() || height <= 0.0 {
                return Err(Error::InvalidVirtualList);
            }
            if height != items.metrics.height(index)? {
                changes.push((index, height));
            }
        }
        let changed = changes.len();
        let next = items.with_heights(changes)?;
        let source = p.items;
        let scroll = p.scroll;
        let height = p.viewport[1];
        let policy = p.scroll_policy;
        Ok(Update {
            measured: rows.len(),
            changed,
            apply: Box::new(move |tx| {
                if *tx.read(source)? != items {
                    return Err(Error::InvalidVirtualList);
                }
                if changed > 0 {
                    tx.rebase_virtual_list(source, scroll, next, height, height, policy)?;
                }
                Ok(())
            }),
        })
    }
}
/// Mounted rows are measured with normal layout; unmounted rows retain estimates.
/// Host calls settle_virtual_list_measurements once per preparation turn.
pub struct MeasuredVirtualList<C>(PhantomData<C>);
impl<C: Component> Component for MeasuredVirtualList<C>
where
    C::Props: Clone,
{
    type Props = MeasuredVirtualListProps<C>;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let p = cx.read_prop(props)?;
        p.validate()?;
        let items = p.items;
        let scroll = p.scroll;
        let host = cx.append_layout(LayoutStyle::column(), 0)?;
        let style = cx.computed(0, &[props.dependency()], move |i| {
            let p = i.read_prop(props)?;
            p.validate()?;
            Ok(LayoutStyle::column()
                .size(
                    LayoutDimension::Px(p.viewport[0]),
                    LayoutDimension::Px(p.viewport[1]),
                )
                .flex_shrink(0.0))
        })?;
        cx.bind(style, Property::layout_style(host))?;
        let site = cx.component_sites(&[0])?[0];
        cx.component_producer(
            0,
            host,
            &[],
            ComponentProducer::new(move |i| {
                let p = i.read_prop(props)?;
                p.validate()?;
                if p.items != items || p.scroll != scroll {
                    return Err(Error::InvalidVirtualList);
                }
                let mut child = VirtualListProps::new(i.read(items)?.clone(), scroll, p.viewport);
                child.overscan = p.overscan;
                child.max_rows = p.max_rows;
                let mut plan = ComponentPlan::new();
                plan.select::<VirtualList<C>>(site, host, 0, child);
                Ok(plan)
            }),
        )?;
        cx.register_list_measurement(Rc::new(Typed {
            props,
            site,
            items,
            scroll,
        }))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
