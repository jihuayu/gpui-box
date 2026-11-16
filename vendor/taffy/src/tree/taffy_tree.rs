//! Contains [TaffyTree](crate::tree::TaffyTree): the default implementation of [LayoutTree](crate::tree::LayoutTree), and the error type for Taffy.
#[cfg(not(feature = "std"))]
use slotmap::SecondaryMap;
#[cfg(feature = "std")]
use slotmap::SparseSecondaryMap as SecondaryMap;
use slotmap::{DefaultKey, SlotMap};
#[cfg(feature = "std")]
use std::sync::Arc;
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;

#[cfg(feature = "block_layout")]
use crate::block::BlockContext;
use crate::geometry::Size;
use crate::style::{AvailableSpace, Display, Style};
use crate::sys::DefaultCheapStr;
use crate::tree::{
    Cache, ClearState, Layout, LayoutContainingBlock, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, PrintTree,
    RoundTree, RunMode, TraversePartialTree, TraverseTree,
};
use crate::util::debug::{debug_log, debug_log_node};
use crate::util::sys::{new_const_children_vec, new_vec_with_capacity, Box, ChildrenVec, Vec};

use crate::compute::{
    compute_cached_layout, compute_hidden_layout, compute_leaf_layout, compute_oof_layout, round_layout,
};
use crate::CacheTree;
use crate::tree::cache::ExactMeasureCache;

#[cfg(feature = "block_layout")]
use crate::{compute::compute_block_layout, LayoutBlockContainer};
#[cfg(feature = "flexbox")]
use crate::{compute::compute_flexbox_layout, LayoutFlexboxContainer};
#[cfg(feature = "grid")]
use crate::{compute::compute_grid_layout, LayoutGridContainer};

#[cfg(feature = "grid")]
use crate::compute::grid::DetailedGridInfo;
use crate::tree::layout::DetailedLayoutInfo;

/// The error Taffy generates on invalid operations
pub type TaffyResult<T> = Result<T, TaffyError>;

/// An error that occurs while trying to access or modify a node's children by index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaffyError {
    /// Hard host bounds must be ordered, nonnegative and have finite minima.
    InvalidSizeBounds,
    /// A ConstraintBox must have at most one direct, in-flow child.
    InvalidConstraintBox,
    /// A custom parent returned an invalid plan or has out-of-flow direct children.
    InvalidCustomLayout,
    /// The parent node does not have a child at `child_index`. It only has `child_count` children
    ChildIndexOutOfBounds {
        /// The parent node whose child was being looked up
        parent: NodeId,
        /// The index that was looked up
        child_index: usize,
        /// The total number of children the parent has
        child_count: usize,
    },
    /// The parent node was not found in the [`TaffyTree`](crate::TaffyTree) instance.
    InvalidParentNode(NodeId),
    /// The child node was not found in the [`TaffyTree`](crate::TaffyTree) instance.
    InvalidChildNode(NodeId),
    /// The supplied node was not found in the [`TaffyTree`](crate::TaffyTree) instance.
    InvalidInputNode(NodeId),
}

impl core::fmt::Display for TaffyError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            TaffyError::InvalidSizeBounds => write!(f, "Invalid host size bounds"),
            TaffyError::InvalidConstraintBox => write!(f, "Invalid single-child constraint box"),
            TaffyError::InvalidCustomLayout => write!(f, "Invalid custom parent layout"),
            TaffyError::ChildIndexOutOfBounds { parent, child_index, child_count } => {
                write!(f, "Index (is {child_index}) should be < child_count ({child_count}) for parent node {parent:?}")
            }
            TaffyError::InvalidParentNode(parent) => {
                write!(f, "Parent Node {parent:?} is not in the TaffyTree instance")
            }
            TaffyError::InvalidChildNode(child) => {
                write!(f, "Child Node {child:?} is not in the TaffyTree instance")
            }
            TaffyError::InvalidInputNode(node) => {
                write!(f, "Supplied Node {node:?} is not in the TaffyTree instance")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TaffyError {}

/// Global configuration values for a TaffyTree instance
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaffyConfig {
    /// Whether to round layout values
    pub(crate) use_rounding: bool,
}

impl Default for TaffyConfig {
    fn default() -> Self {
        Self { use_rounding: true }
    }
}

/// Layout information for a given [`Node`](crate::node::Node)
///
/// Stored in a [`TaffyTree`].
#[derive(Debug, Clone, PartialEq)]
struct NodeData {
    /// The layout strategy used by this node
    pub(crate) style: Style,

    /// The always unrounded results of the layout computation. We must store this separately from the rounded
    /// layout to avoid errors from rounding already-rounded values. See <https://github.com/DioxusLabs/taffy/issues/501>.
    pub(crate) unrounded_layout: Layout,

    /// The final results of the layout computation.
    /// These may be rounded or unrounded depending on what the `use_rounding` config setting is set to.
    pub(crate) final_layout: Layout,

    /// Whether the node has context data associated with it or not
    pub(crate) has_context: bool,

    /// Out-of-flow (absolute/fixed) boxes whose containing block is this node,
    /// as recorded by the out-of-flow positioning pass
    pub(crate) hoisted_children: ChildrenVec<NodeId>,

    /// The cached results of the layout computation
    pub(crate) cache: Cache,

    /// The computation result from layout algorithm
    pub(crate) detailed_layout_info: DetailedLayoutInfo,
}

impl NodeData {
    /// Create the data for a new node
    #[must_use]
    pub const fn new(style: Style) -> Self {
        Self {
            style,
            cache: Cache::new(),
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            has_context: false,
            hoisted_children: new_const_children_vec(),
            detailed_layout_info: DetailedLayoutInfo::None,
        }
    }

    /// Marks a node and all of its ancestors as requiring relayout
    ///
    /// This clears any cached data and signals that the data must be recomputed.
    /// If the node was already marked as dirty, returns true
    #[inline]
    pub fn mark_dirty(&mut self) -> ClearState {
        self.cache.clear()
    }
}

/// An entire tree of UI nodes. The entry point to Taffy's high-level API.
///
/// Allows you to build a tree of UI nodes, run Taffy's layout algorithms over that tree, and then access the resultant layout.]
#[derive(Debug, Clone)]
pub struct TaffyTree<NodeContext = ()> {
    /// The [`NodeData`] for each node stored in this tree
    nodes: SlotMap<DefaultKey, NodeData>,

    /// Functions/closures that compute the intrinsic size of leaf nodes
    node_context_data: SecondaryMap<DefaultKey, NodeContext>,

    /// Sparse index of non-CSS parent constraint protocols. Validation happens
    /// at layout entry, allowing valid multi-node edits in arbitrary order.
    constraint_boxes: SecondaryMap<DefaultKey, ()>,

    /// Last final child-slot inputs, retained across ancestor cache hits.
    constraint_box_inputs: SecondaryMap<DefaultKey, LayoutInput>,

    /// Owned immutable parent algorithms; ordinary nodes carry no delegate slot.
    custom_layouts: SecondaryMap<DefaultKey, Arc<dyn crate::tree::CustomLayout>>,

    /// Lazily allocated, bounded full-result measurement caches. Only visited
    /// nodes in exact-measure invocations have entries; ordinary nodes stay compact.
    exact_measure_caches: SecondaryMap<DefaultKey, Arc<ExactMeasureCache>>,

    /// The children of each node
    ///
    /// The indexes in the outer vector correspond to the position of the parent [`NodeData`]
    children: SlotMap<DefaultKey, ChildrenVec<NodeId>>,

    /// The parents of each node
    ///
    /// The indexes in the outer vector correspond to the position of the child [`NodeData`]
    parents: SlotMap<DefaultKey, Option<NodeId>>,

    /// Layout mode configuration
    config: TaffyConfig,
}

impl Default for TaffyTree {
    fn default() -> TaffyTree<()> {
        TaffyTree::new()
    }
}

/// Iterator that wraps a slice of nodes, lazily converting them to u64
pub struct TaffyTreeChildIter<'a>(core::slice::Iter<'a, NodeId>);
impl Iterator for TaffyTreeChildIter<'_> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().copied()
    }
}

// TraversePartialTree impl for TaffyTree
impl<NodeContext> TraversePartialTree for TaffyTree<NodeContext> {
    type ChildIter<'a>
        = TaffyTreeChildIter<'a>
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        TaffyTreeChildIter(self.children[parent_node_id.into()].iter())
    }

    #[inline(always)]
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.children[parent_node_id.into()].len()
    }

    #[inline(always)]
    fn get_child_id(&self, parent_node_id: NodeId, id: usize) -> NodeId {
        self.children[parent_node_id.into()][id]
    }
}

// TraverseTree impl for TaffyTree
impl<NodeContext> TraverseTree for TaffyTree<NodeContext> {}

// CacheTree impl for TaffyTree
impl<NodeContext> CacheTree for TaffyTree<NodeContext> {
    fn cache_get(&mut self, node_id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.cached_layout(node_id, input, !self.custom_layouts.is_empty())
    }

    fn cache_store(&mut self, node_id: NodeId, input: &LayoutInput, layout_output: LayoutOutput) {
        self.store_cached_layout(node_id, input, layout_output, !self.custom_layouts.is_empty())
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.nodes[node_id.into()].cache.clear();
        self.exact_measure_caches.remove(node_id.into());
    }
}

// PrintTree impl for TaffyTree
impl<NodeContext> PrintTree for TaffyTree<NodeContext> {
    #[inline(always)]
    fn get_debug_label(&self, node_id: NodeId) -> &'static str {
        let node = &self.nodes[node_id.into()];
        let display = node.style.display;
        let num_children = self.child_count(node_id);

        match (num_children, display) {
            (_, Display::None) => "NONE",
            (_, Display::ConstraintBox { .. }) => "CONSTRAINT-BOX",
            (0, _) => "LEAF",
            #[cfg(feature = "block_layout")]
            (_, Display::Block) => "BLOCK",
            #[cfg(feature = "block_layout")]
            (_, Display::FlowRoot) => "FLOW-ROOT",
            #[cfg(feature = "flexbox")]
            (_, Display::Flex) => {
                use crate::FlexDirection;
                match node.style.flex_direction {
                    FlexDirection::Row | FlexDirection::RowReverse => "FLEX ROW",
                    FlexDirection::Column | FlexDirection::ColumnReverse => "FLEX COL",
                }
            }
            #[cfg(feature = "grid")]
            (_, Display::Grid) => "GRID",
        }
    }

    #[inline(always)]
    fn get_final_layout(&self, node_id: NodeId) -> Layout {
        if self.config.use_rounding {
            self.nodes[node_id.into()].final_layout
        } else {
            self.nodes[node_id.into()].unrounded_layout
        }
    }
}

/// View over the Taffy tree that holds the tree itself along with a reference to the context
/// and implements LayoutTree. This allows the context to be stored outside of the TaffyTree struct
/// which makes the lifetimes of the context much more flexible.
pub(crate) struct TaffyView<'t, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    /// A reference to the TaffyTree
    pub(crate) taffy: &'t mut TaffyTree<NodeContext>,
    /// The context provided for passing to measure functions if layout is run over this struct
    pub(crate) measure_function: MeasureFunction,
    /// Reports every unrounded layout write; may include transient repeated writes.
    pub(crate) layout_observer: LayoutObserver,
    layout_error: Option<TaffyError>,
    exact_measure: bool,
}

impl<NodeContext, MeasureFunction, LayoutObserver> TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    #[inline(always)]
    /// Unified implementation that both `LayoutPartialTree::compute_child_layout`
    /// and `LayoutBlockContainer::compute_block_child_layout` delegate to.
    fn compute_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        #[cfg(feature = "block_layout")] block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        // If RunMode is PerformHiddenLayout then this indicates that an ancestor node is `Display::None`
        // and thus that we should lay out this node using hidden layout regardless of it's own display style.
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            debug_log!("HIDDEN");
            return compute_hidden_layout(self, node_id);
        }

        // We run the following wrapped in "compute_cached_layout", which will check the cache for an entry matching the node and inputs and:
        //   - Return that entry if exists
        //   - Else call the passed closure (below) to compute the result
        //
        // If there was no cache match and a new result needs to be computed then that result will be added to the cache
        compute_cached_layout(self, node_id, inputs, |tree, node_id, inputs| {
            let display_mode = tree.taffy.nodes[node_id.into()].style.display;
            let has_children = tree.child_count(node_id) > 0;

            debug_log!(display_mode);
            debug_log_node!(inputs);

            // Dispatch to a layout algorithm based on the node's display style and whether the node has children or not.
            let custom = tree.taffy.custom_layouts.get(node_id.into()).cloned();
            let mut output = if display_mode != Display::None && custom.is_some() {
                match crate::compute::compute_custom_layout(tree, node_id, inputs, custom.unwrap().as_ref()) {
                    Ok(output) => output,
                    Err(error) => {
                        tree.layout_error.get_or_insert(error);
                        LayoutOutput::HIDDEN
                    }
                }
            } else {
                match (display_mode, has_children) {
                    (Display::None, _) => compute_hidden_layout(tree, node_id),
                    (Display::ConstraintBox { width, height }, _) => {
                        crate::compute::compute_constraint_box_layout(tree, node_id, inputs, Size { width, height })
                    }
                    #[cfg(feature = "block_layout")]
                    (Display::Block, true) => compute_block_layout(tree, node_id, inputs, block_ctx),
                    #[cfg(feature = "block_layout")]
                    (Display::FlowRoot, true) => compute_block_layout(tree, node_id, inputs, None),
                    #[cfg(feature = "flexbox")]
                    (Display::Flex, true) => compute_flexbox_layout(tree, node_id, inputs),
                    #[cfg(feature = "grid")]
                    (Display::Grid, true) => compute_grid_layout(tree, node_id, inputs),
                    (_, false) => {
                        let node_key = node_id.into();
                        let style = &tree.taffy.nodes[node_key].style;
                        let has_context = tree.taffy.nodes[node_key].has_context;
                        let node_context = has_context.then(|| tree.taffy.node_context_data.get_mut(node_key)).flatten();
                        (tree.measure_function)(inputs, node_id, node_context, style)
                    }
                }
            };

            // Lay out any out-of-flow candidates for which this node is the containing block.
            // The rest bubble up via `output.oof_candidates`. This runs inside the cache-miss
            // closure so that cached outputs already contain the processed candidate list.
            // Only full layout passes run it: measure passes must not write hoisted box layouts
            // (which would not be rewritten if the final layout pass is a cache hit).
            if inputs.run_mode == RunMode::PerformLayout {
                compute_oof_layout(tree, node_id, &mut output);
            }

            output
        })
    }
}

// TraversePartialTree impl for TaffyView
impl<NodeContext, MeasureFunction, LayoutObserver> TraversePartialTree for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    type ChildIter<'a>
        = TaffyTreeChildIter<'a>
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        self.taffy.child_ids(parent_node_id)
    }

    #[inline(always)]
    fn child_count(&self, parent_node_id: NodeId) -> usize {
        self.taffy.child_count(parent_node_id)
    }

    #[inline(always)]
    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        self.taffy.get_child_id(parent_node_id, child_index)
    }
}

// TraverseTree impl for TaffyView
impl<NodeContext, MeasureFunction, LayoutObserver> TraverseTree for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver> where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
}

// LayoutPartialTree impl for TaffyView
impl<NodeContext, MeasureFunction, LayoutObserver> LayoutPartialTree for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    fn record_constraint_box_input(&mut self, node: NodeId, input: Option<LayoutInput>) {
        if let Some(input) = input {
            self.taffy.constraint_box_inputs.insert(node.into(), input);
        } else {
            self.taffy.constraint_box_inputs.remove(node.into());
        }
    }
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    type CustomIdent = DefaultCheapStr;

    #[inline(always)]
    fn get_core_container_style(&self, node_id: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.taffy.nodes[node_id.into()].style
    }

    #[inline(always)]
    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        let previous = &self.taffy.nodes[node_id.into()].unrounded_layout;
        (self.layout_observer)(node_id, previous, layout);
        self.taffy.nodes[node_id.into()].unrounded_layout = *layout;
    }

    #[inline(always)]
    fn resolve_calc_value(&self, _val: *const (), _basis: f32) -> f32 {
        0.0
    }

    #[inline(always)]
    fn compute_custom_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        let previous = self.exact_measure;
        self.exact_measure = true;
        let output = crate::compute::constraint_box::layout_child(self, node, inputs);
        self.exact_measure = previous;
        output
    }

    fn compute_child_layout(&mut self, node_id: NodeId, inputs: LayoutInput) -> LayoutOutput {
        self.compute_child_layout(
            node_id,
            inputs,
            #[cfg(feature = "block_layout")]
            None,
        )
    }
}

impl<NodeContext, MeasureFunction, LayoutObserver> LayoutContainingBlock for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    type OofItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    #[inline(always)]
    fn get_oof_item_style(&self, node_id: NodeId) -> Self::OofItemStyle<'_> {
        &self.taffy.nodes[node_id.into()].style
    }

    #[inline(always)]
    fn clear_hoisted_children(&mut self, node_id: NodeId) {
        self.taffy.nodes[node_id.into()].hoisted_children.clear();
    }

    #[inline(always)]
    fn add_hoisted_children(&mut self, node_id: NodeId, hoisted: &[NodeId]) {
        self.taffy.nodes[node_id.into()].hoisted_children.extend_from_slice(hoisted);
    }

    #[inline(always)]
    fn get_detailed_layout_info(&self, node_id: NodeId) -> &DetailedLayoutInfo {
        &self.taffy.nodes[node_id.into()].detailed_layout_info
    }
}

impl<NodeContext, MeasureFunction, LayoutObserver> CacheTree for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    fn cache_get(&mut self, node_id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.taffy.cached_layout(node_id, input, self.exact_measure)
    }

    fn cache_store(&mut self, node_id: NodeId, input: &LayoutInput, layout_output: LayoutOutput) {
        if self.layout_error.is_none() {
            self.taffy.store_cached_layout(node_id, input, layout_output, self.exact_measure);
        }
    }

    fn cache_clear(&mut self, node_id: NodeId) {
        self.taffy.nodes[node_id.into()].cache.clear();
        self.taffy.exact_measure_caches.remove(node_id.into());
    }
}

#[cfg(feature = "block_layout")]
impl<NodeContext, MeasureFunction, LayoutObserver> LayoutBlockContainer for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    #[inline(always)]
    fn get_block_container_style(&self, node_id: NodeId) -> Self::BlockContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    #[inline(always)]
    fn get_block_child_style(&self, child_node_id: NodeId) -> Self::BlockItemStyle<'_> {
        self.get_core_container_style(child_node_id)
    }

    #[inline(always)]
    fn compute_block_child_layout(
        &mut self,
        node_id: NodeId,
        inputs: LayoutInput,
        block_ctx: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        self.compute_child_layout(node_id, inputs, block_ctx)
    }
}

#[cfg(feature = "flexbox")]
impl<NodeContext, MeasureFunction, LayoutObserver> LayoutFlexboxContainer for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    #[inline(always)]
    fn get_flexbox_container_style(&self, node_id: NodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.taffy.nodes[node_id.into()].style
    }

    #[inline(always)]
    fn get_flexbox_child_style(&self, child_node_id: NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.taffy.nodes[child_node_id.into()].style
    }
}

#[cfg(feature = "grid")]
impl<NodeContext, MeasureFunction, LayoutObserver> LayoutGridContainer for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    #[inline(always)]
    fn get_grid_container_style(&self, node_id: NodeId) -> Self::GridContainerStyle<'_> {
        &self.taffy.nodes[node_id.into()].style
    }

    #[inline(always)]
    fn get_grid_child_style(&self, child_node_id: NodeId) -> Self::GridItemStyle<'_> {
        &self.taffy.nodes[child_node_id.into()].style
    }

    #[inline(always)]
    fn set_detailed_grid_info(&mut self, node_id: NodeId, detailed_grid_info: DetailedGridInfo) {
        self.taffy.nodes[node_id.into()].detailed_layout_info = DetailedLayoutInfo::Grid(Box::new(detailed_grid_info));
    }
}

// RoundTree impl for TaffyView
impl<NodeContext, MeasureFunction, LayoutObserver> RoundTree for TaffyView<'_, NodeContext, MeasureFunction, LayoutObserver>
where
    MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    LayoutObserver: FnMut(NodeId, &Layout, &Layout),
{
    #[inline(always)]
    fn get_unrounded_layout(&self, node: NodeId) -> Layout {
        self.taffy.nodes[node.into()].unrounded_layout
    }

    #[inline(always)]
    fn set_final_layout(&mut self, node_id: NodeId, layout: &Layout) {
        self.taffy.nodes[node_id.into()].final_layout = *layout;
    }

    #[inline(always)]
    fn is_out_of_flow(&self, node_id: NodeId) -> bool {
        let node = &self.taffy.nodes[node_id.into()];
        node.style.position.is_out_of_flow() && node.style.display != crate::style::Display::None
    }

    #[inline(always)]
    fn hoisted_child_count(&self, node_id: NodeId) -> usize {
        self.taffy.nodes[node_id.into()].hoisted_children.len()
    }

    #[inline(always)]
    fn get_hoisted_child_id(&self, node_id: NodeId, index: usize) -> NodeId {
        self.taffy.nodes[node_id.into()].hoisted_children[index]
    }
}

#[allow(clippy::iter_cloned_collect)] // due to no-std support, we need to use `iter_cloned` instead of `collect`
impl<NodeContext> TaffyTree<NodeContext> {
    fn cached_layout(&mut self, node: NodeId, input: &LayoutInput, exact: bool) -> Option<LayoutOutput> {
        if exact && input.run_mode == RunMode::ComputeSize {
            return self.exact_measure_caches.get(node.into()).and_then(|cache| cache.get(input));
        }
        self.nodes[node.into()].cache.get(input)
    }

    fn store_cached_layout(&mut self, node: NodeId, input: &LayoutInput, output: LayoutOutput, exact: bool) {
        if exact && input.run_mode == RunMode::ComputeSize {
            let cache = self.exact_measure_caches.entry(node.into()).unwrap().or_default();
            Arc::make_mut(cache).store(input, output);
        } else {
            self.nodes[node.into()].cache.store(input, output);
        }
    }

    /// Creates a new [`TaffyTree`]
    ///
    /// The default capacity of a [`TaffyTree`] is 16 nodes.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    /// Creates a new [`TaffyTree`] that can store `capacity` nodes before reallocation
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        TaffyTree {
            // TODO: make this method const upstream,
            // so constructors here can be const
            nodes: SlotMap::with_capacity(capacity),
            children: SlotMap::with_capacity(capacity),
            parents: SlotMap::with_capacity(capacity),
            node_context_data: SecondaryMap::with_capacity(capacity),
            constraint_boxes: SecondaryMap::new(),
            constraint_box_inputs: SecondaryMap::new(),
            custom_layouts: SecondaryMap::new(),
            exact_measure_caches: SecondaryMap::new(),
            config: TaffyConfig::default(),
        }
    }

    /// Install an immutable parent algorithm. Replacing it invalidates this node
    /// and ancestors. Display::None still hides the entire subtree. The delegate
    /// owns direct child placement; direct out-of-flow children are unsupported.
    pub fn set_custom_layout(
        &mut self,
        node: NodeId,
        delegate: Option<Arc<dyn crate::tree::CustomLayout>>,
    ) -> TaffyResult<()> {
        if !self.nodes.contains_key(node.into()) {
            return Err(TaffyError::InvalidInputNode(node));
        }
        if let Some(delegate) = delegate {
            self.custom_layouts.insert(node.into(), delegate);
        } else {
            self.custom_layouts.remove(node.into());
        }
        self.mark_dirty(node)
    }

    /// Enable rounding of layout values. Rounding is enabled by default.
    pub fn enable_rounding(&mut self) {
        self.config.use_rounding = true;
    }

    /// Disable rounding of layout values. Rounding is enabled by default.
    pub fn disable_rounding(&mut self) {
        self.config.use_rounding = false;
    }

    /// Final child-slot inputs recorded by the last successful layout. Cached
    /// ancestors retain the observation. Call layout before reading; a dirty
    /// tree may still contain observations from an earlier layout.
    pub fn constraint_box_input(&self, node: NodeId) -> TaffyResult<Option<LayoutInput>> {
        let key = node.into();
        let data = self.nodes.get(key).ok_or(TaffyError::InvalidInputNode(node))?;
        Ok(if matches!(data.style.display, Display::ConstraintBox { .. }) {
            self.constraint_box_inputs.get(key).copied()
        } else {
            None
        })
    }

    /// Creates and adds a new unattached leaf node to the tree, and returns the node of the new node
    pub fn new_leaf(&mut self, layout: Style) -> TaffyResult<NodeId> {
        let constraint_box = matches!(layout.display, Display::ConstraintBox { .. });
        let id = self.nodes.insert(NodeData::new(layout));
        if constraint_box { self.constraint_boxes.insert(id, ()); }
        let _ = self.children.insert(new_vec_with_capacity(0));
        let _ = self.parents.insert(None);

        Ok(id.into())
    }

    /// Creates and adds a new unattached leaf node to the tree, and returns the [`NodeId`] of the new node
    ///
    /// Creates and adds a new leaf node with a supplied context
    pub fn new_leaf_with_context(&mut self, layout: Style, context: NodeContext) -> TaffyResult<NodeId> {
        let constraint_box = matches!(layout.display, Display::ConstraintBox { .. });
        let mut data = NodeData::new(layout);
        data.has_context = true;

        let id = self.nodes.insert(data);
        if constraint_box { self.constraint_boxes.insert(id, ()); }
        self.node_context_data.insert(id, context);

        let _ = self.children.insert(new_vec_with_capacity(0));
        let _ = self.parents.insert(None);

        Ok(id.into())
    }

    /// Creates and adds a new node, which may have any number of `children`
    pub fn new_with_children(&mut self, layout: Style, children: &[NodeId]) -> TaffyResult<NodeId> {
        let constraint_box = matches!(layout.display, Display::ConstraintBox { .. });
        let id = NodeId::from(self.nodes.insert(NodeData::new(layout)));
        if constraint_box { self.constraint_boxes.insert(id.into(), ()); }

        for child in children {
            self.parents[(*child).into()] = Some(id);
        }

        let _ = self.children.insert(children.iter().copied().collect::<_>());
        let _ = self.parents.insert(None);

        Ok(id)
    }

    /// Drops all nodes in the tree
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.children.clear();
        self.parents.clear();
        // A context belongs to the node that owns it. Leaving it behind keeps arbitrary
        // user data -- for a measure function, a boxed closure and everything it
        // captures -- alive after the node it belonged to is gone.
        self.node_context_data.clear();
        self.constraint_boxes.clear();
        self.constraint_box_inputs.clear();
        self.custom_layouts.clear();
        self.exact_measure_caches.clear();
    }

    /// Remove a specific node from the tree and drop it
    ///
    /// Returns the id of the node removed.
    pub fn remove(&mut self, node: NodeId) -> TaffyResult<NodeId> {
        let key = node.into();
        if let Some(parent) = self.parents[key] {
            if let Some(children) = self.children.get_mut(parent.into()) {
                children.retain(|f| *f != node);
            }
            self.mark_dirty(parent)?;
        }

        // Remove "parent" references to a node when removing that node
        if let Some(children) = self.children.get(key) {
            for child in children.iter().copied() {
                self.parents[child.into()] = None;
            }
        }

        let _ = self.children.remove(key);
        let _ = self.parents.remove(key);
        let _ = self.nodes.remove(key);
        let _ = self.node_context_data.remove(key);
        let _ = self.constraint_boxes.remove(key);
        let _ = self.constraint_box_inputs.remove(key);
        let _ = self.custom_layouts.remove(key);
        let _ = self.exact_measure_caches.remove(key);

        Ok(node)
    }

    /// Sets the context data associated with the node
    #[inline]
    pub fn set_node_context(&mut self, node: NodeId, measure: Option<NodeContext>) -> TaffyResult<()> {
        let key = node.into();
        if let Some(measure) = measure {
            self.nodes[key].has_context = true;
            self.node_context_data.insert(key, measure);
        } else {
            self.nodes[key].has_context = false;
            self.node_context_data.remove(key);
        }

        self.mark_dirty(node)?;

        Ok(())
    }

    /// Gets a reference to the the context data associated with the node
    #[inline]
    pub fn get_node_context(&self, node: NodeId) -> Option<&NodeContext> {
        self.node_context_data.get(node.into())
    }

    /// Gets a mutable reference to the the context data associated with the node
    #[inline]
    pub fn get_node_context_mut(&mut self, node: NodeId) -> Option<&mut NodeContext> {
        self.node_context_data.get_mut(node.into())
    }

    /// Gets mutable references to the the context data associated with the nodes. All keys must be valid and disjoint, otherwise None is returned.
    pub fn get_disjoint_node_context_mut<const N: usize>(
        &mut self,
        keys: [NodeId; N],
    ) -> Option<[&mut NodeContext; N]> {
        self.node_context_data.get_disjoint_mut(keys.map(|k| k.into()))
    }

    /// Adds a `child` node under the supplied `parent`
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> TaffyResult<()> {
        let parent_key = parent.into();
        let child_key = child.into();
        self.parents[child_key] = Some(parent);
        self.children[parent_key].push(child);
        self.mark_dirty(parent)?;

        Ok(())
    }

    /// Inserts a `child` node at the given `child_index` under the supplied `parent`, shifting all children after it to the right.
    pub fn insert_child_at_index(&mut self, parent: NodeId, child_index: usize, child: NodeId) -> TaffyResult<()> {
        let parent_key = parent.into();

        let child_count = self.children[parent_key].len();
        if child_index > child_count {
            return Err(TaffyError::ChildIndexOutOfBounds { parent, child_index, child_count });
        }

        self.parents[child.into()] = Some(parent);
        self.children[parent_key].insert(child_index, child);
        self.mark_dirty(parent)?;

        Ok(())
    }

    /// Directly sets the `children` of the supplied `parent`
    pub fn set_children(&mut self, parent: NodeId, children: &[NodeId]) -> TaffyResult<()> {
        let parent_key = parent.into();

        // Remove node as parent from all its current children.
        for child in &self.children[parent_key] {
            self.parents[(*child).into()] = None;
        }

        // Build up relation node <-> child
        for &child in children {
            // Remove child from previous parent
            if let Some(previous_parent) = self.parents[child.into()] {
                self.remove_child(previous_parent, child).unwrap();
            }
            self.parents[child.into()] = Some(parent);
        }

        let parent_children = &mut self.children[parent_key];
        parent_children.clear();
        children.iter().for_each(|child| parent_children.push(*child));

        self.mark_dirty(parent)?;

        Ok(())
    }

    /// Removes the `child` of the parent `node`
    ///
    /// The child is not removed from the tree entirely, it is simply no longer attached to its previous parent.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> TaffyResult<NodeId> {
        let index = self.children[parent.into()].iter().position(|n| *n == child).unwrap();
        self.remove_child_at_index(parent, index)
    }

    /// Removes the child at the given `index` from the `parent`
    ///
    /// The child is not removed from the tree entirely, it is simply no longer attached to its previous parent.
    pub fn remove_child_at_index(&mut self, parent: NodeId, child_index: usize) -> TaffyResult<NodeId> {
        let parent_key = parent.into();
        let child_count = self.children[parent_key].len();
        if child_index >= child_count {
            return Err(TaffyError::ChildIndexOutOfBounds { parent, child_index, child_count });
        }

        let child = self.children[parent_key].remove(child_index);
        self.parents[child.into()] = None;

        self.mark_dirty(parent)?;

        Ok(child)
    }

    /// Removes children at the given range from the `parent`
    ///
    /// Children are not removed from the tree entirely, they are simply no longer attached to their previous parent.
    ///
    /// Function will panic if given range is invalid. See [`core::slice::range`]
    pub fn remove_children_range<R>(&mut self, parent: NodeId, range: R) -> TaffyResult<()>
    where
        R: core::ops::RangeBounds<usize>,
    {
        let parent_key = parent.into();
        for child in self.children[parent_key].drain(range) {
            self.parents[child.into()] = None;
        }

        self.mark_dirty(parent)?;
        Ok(())
    }

    /// Replaces the child at the given `child_index` from the `parent` node with the new `child` node
    ///
    /// The child is not removed from the tree entirely, it is simply no longer attached to its previous parent.
    pub fn replace_child_at_index(
        &mut self,
        parent: NodeId,
        child_index: usize,
        new_child: NodeId,
    ) -> TaffyResult<NodeId> {
        let parent_key = parent.into();

        let child_count = self.children[parent_key].len();
        if child_index >= child_count {
            return Err(TaffyError::ChildIndexOutOfBounds { parent, child_index, child_count });
        }

        self.parents[new_child.into()] = Some(parent);
        let old_child = core::mem::replace(&mut self.children[parent_key][child_index], new_child);
        self.parents[old_child.into()] = None;

        self.mark_dirty(parent)?;

        Ok(old_child)
    }

    /// Returns the child node of the parent `node` at the provided `child_index`
    #[inline]
    pub fn child_at_index(&self, parent: NodeId, child_index: usize) -> TaffyResult<NodeId> {
        let parent_key = parent.into();
        let child_count = self.children[parent_key].len();
        if child_index >= child_count {
            return Err(TaffyError::ChildIndexOutOfBounds { parent, child_index, child_count });
        }

        Ok(self.children[parent_key][child_index])
    }

    /// Returns the total number of nodes in the tree
    #[inline]
    pub fn total_node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the `NodeId` of the parent node of the specified node (if it exists)
    ///
    /// - Return None if the specified node has no parent
    /// - Panics if the specified node does not exist
    #[inline]
    pub fn parent(&self, child_id: NodeId) -> Option<NodeId> {
        self.parents[child_id.into()]
    }

    /// Returns a list of children that belong to the parent node
    pub fn children(&self, parent: NodeId) -> TaffyResult<Vec<NodeId>> {
        Ok(self.children[parent.into()].clone())
    }

    /// Returns the out-of-flow (absolute/fixed) boxes whose containing block is `node`, as
    /// recorded by the last layout. These boxes are laid out by `node` rather than by their
    /// parent, and their [`Layout::location`] is relative to `node`.
    pub fn hoisted_children(&self, node: NodeId) -> TaffyResult<&[NodeId]> {
        Ok(&self.nodes[node.into()].hoisted_children)
    }

    /// Sets the [`Style`] of the provided `node`
    #[inline]
    pub fn set_style(&mut self, node: NodeId, style: Style) -> TaffyResult<()> {
        if matches!(style.display, Display::ConstraintBox { .. }) {
            self.constraint_boxes.insert(node.into(), ());
        } else {
            self.constraint_boxes.remove(node.into());
            self.constraint_box_inputs.remove(node.into());
        }
        self.nodes[node.into()].style = style;
        self.mark_dirty(node)?;
        Ok(())
    }

    fn validate_constraint_children(&self, style: &Style, children: &[NodeId]) -> TaffyResult<()> {
        if matches!(style.display, Display::ConstraintBox { .. })
            && (children.len() > 1 || children.iter().any(|id| self.nodes[(*id).into()].style.position.is_out_of_flow())) {
            return Err(TaffyError::InvalidConstraintBox);
        }
        Ok(())
    }

    /// Gets the [`Style`] of the provided `node`
    #[inline]
    pub fn style(&self, node: NodeId) -> TaffyResult<&Style> {
        Ok(&self.nodes[node.into()].style)
    }

    /// Return this node layout relative to its parent
    #[inline]
    pub fn layout(&self, node: NodeId) -> TaffyResult<&Layout> {
        if self.config.use_rounding {
            Ok(&self.nodes[node.into()].final_layout)
        } else {
            Ok(&self.nodes[node.into()].unrounded_layout)
        }
    }

    /// Returns this node layout with unrounded values relative to its parent.
    #[inline]
    pub fn unrounded_layout(&self, node: NodeId) -> &Layout {
        &self.nodes[node.into()].unrounded_layout
    }

    /// Get the "detailed layout info" for a node.
    ///
    /// Currently this is only implemented for CSS Grid containers where it contains
    /// the computed size of each grid track and the computed placement of each grid item
    #[inline]
    pub fn detailed_layout_info(&self, node_id: NodeId) -> &DetailedLayoutInfo {
        &self.nodes[node_id.into()].detailed_layout_info
    }

    /// Marks the layout of this node and its ancestors as outdated
    pub fn mark_dirty(&mut self, node: NodeId) -> TaffyResult<()> {
        fn mark_dirty_recursive(
            nodes: &mut SlotMap<DefaultKey, NodeData>,
            parents: &SlotMap<DefaultKey, Option<NodeId>>,
            exact: &mut SecondaryMap<DefaultKey, Arc<ExactMeasureCache>>,
            node_key: DefaultKey,
        ) {
            let had_exact = exact.remove(node_key).is_some();
            let ordinary = nodes[node_key].mark_dirty();
            // An exact-only measured child still invalidates cached ancestors.
            // Stop only if both stores were already empty.
            if had_exact || matches!(ordinary, ClearState::Cleared) {
                if let Some(Some(node)) = parents.get(node_key) {
                    mark_dirty_recursive(nodes, parents, exact, (*node).into());
                }
            }
        }

        mark_dirty_recursive(&mut self.nodes, &self.parents, &mut self.exact_measure_caches, node.into());

        Ok(())
    }

    /// Indicates whether the layout of this node needs to be recomputed
    #[inline]
    pub fn dirty(&self, node: NodeId) -> TaffyResult<bool> {
        Ok(self.nodes[node.into()].cache.is_empty() && !self.exact_measure_caches.contains_key(node.into()))
    }

    /// Updates the stored layout of the provided `node` and its children
    pub fn compute_layout_with_measure<MeasureFunction>(
        &mut self,
        node_id: NodeId,
        available_space: Size<AvailableSpace>,
        measure_function: MeasureFunction,
    ) -> Result<(), TaffyError>
    where
        MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
    {
        self.compute_layout_with_measure_and_observer(node_id, available_space, measure_function, |_, _, _| {})
    }

    /// Compute layout and report each unrounded layout write, including equal or
    /// transient writes. The callback receives the node, previous value and new
    /// value. It runs synchronously and must not panic. Consumers must deduplicate
    /// identities and inspect final layouts after this call for net changes.
    /// With rounding enabled these observations are not final rounded layouts.
    pub fn compute_layout_with_measure_and_observer<MeasureFunction, LayoutObserver>(
        &mut self,
        node_id: NodeId,
        available_space: Size<AvailableSpace>,
        measure_function: MeasureFunction,
        layout_observer: LayoutObserver,
    ) -> Result<(), TaffyError>
    where
        MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
        LayoutObserver: FnMut(NodeId, &Layout, &Layout),
    {
        self.compute_layout_with_bounds_and_measure_and_observer(
            node_id,
            available_space,
            crate::tree::SizeBounds::NONE,
            measure_function,
            layout_observer,
        )
    }

    /// Root layout with hard border-box bounds, independent of the node's
    /// authored style. Invalid bounds are rejected before tree mutation. Child bounds are selected
    /// by their layout algorithm rather than inherited from this root.
    pub fn compute_layout_with_bounds_and_measure_and_observer<MeasureFunction, LayoutObserver>(
        &mut self,
        node_id: NodeId,
        available_space: Size<AvailableSpace>,
        bounds: crate::tree::SizeBounds,
        measure_function: MeasureFunction,
        layout_observer: LayoutObserver,
    ) -> Result<(), TaffyError>
    where
        MeasureFunction: FnMut(LayoutInput, NodeId, Option<&mut NodeContext>, &Style) -> LayoutOutput,
        LayoutObserver: FnMut(NodeId, &Layout, &Layout),
    {
        if !bounds.is_valid() {
            return Err(TaffyError::InvalidSizeBounds);
        }
        if !self.nodes.contains_key(node_id.into()) {
            return Err(TaffyError::InvalidInputNode(node_id));
        }
        // Validate only indexed boxes in this invocation's subtree, before
        // observer writes. Unrelated detached draft trees do not block layout.
        for (key, ()) in &self.constraint_boxes {
            let mut ancestor = Some(NodeId::from(key));
            while let Some(id) = ancestor {
                if id == node_id {
                    self.validate_constraint_children(&self.nodes[key].style, &self.children[key])?;
                    break;
                }
                ancestor = self.parents[id.into()];
            }
        }
        for (key, _) in &self.custom_layouts {
            let mut ancestor = Some(NodeId::from(key));
            while let Some(id) = ancestor {
                if id == node_id {
                    if self.children[key].iter().any(|child| self.nodes[(*child).into()].style.position.is_out_of_flow()) {
                        return Err(TaffyError::InvalidCustomLayout);
                    }
                    break;
                }
                ancestor = self.parents[id.into()];
            }
        }
        let use_rounding = self.config.use_rounding;
        // Custom descendants can observe vertical percentage bases and baseline
        // metadata omitted by ordinary CSS size caches, including through a
        // cached Flex ancestor. Keep final-layout reuse and use sparse exact
        // measurement entries throughout invocations containing custom algorithms.
        let exact_measure = !self.custom_layouts.is_empty();
        let mut taffy_view = TaffyView { taffy: self, measure_function, layout_observer, layout_error: None, exact_measure };
        crate::compute::compute_root_layout_with_bounds(&mut taffy_view, node_id, available_space, bounds);
        if let Some(error) = taffy_view.layout_error { return Err(error); }
        if use_rounding {
            round_layout(&mut taffy_view, node_id);
        }
        Ok(())
    }

    /// Updates the stored layout of the provided `node` and its children
    pub fn compute_layout(&mut self, node: NodeId, available_space: Size<AvailableSpace>) -> Result<(), TaffyError> {
        self.compute_layout_with_measure(node, available_space, |inputs, _, _, style| {
            compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size::ZERO)
        })
    }

    /// Prints a debug representation of the tree's layout
    #[cfg(feature = "std")]
    pub fn print_tree(&mut self, root: NodeId) {
        crate::util::print_tree(self, root)
    }

    /// Returns an instance of LayoutTree representing the TaffyTree
    #[cfg(test)]
    pub(crate) fn as_layout_tree(&mut self) -> impl LayoutPartialTree + CacheTree + '_ {
        TaffyView {
            taffy: self,
            measure_function: |inputs, _, _, style| compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size::ZERO),
            layout_observer: |_, _, _| {},
            layout_error: None,
            exact_measure: false,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::style::{Dimension, Display, FlexDirection};
    use crate::style_helpers::*;
    use crate::util::sys;

    #[test]
    fn exact_only_dirty_nodes_invalidate_ancestors_without_discarding_sibling_cache() {
        let mut tree = TaffyTree::<()>::new();
        let child = tree.new_leaf(Style::DEFAULT).unwrap();
        let sibling = tree.new_leaf(Style::DEFAULT).unwrap();
        let parent = tree.new_with_children(Style::DEFAULT, &[child, sibling]).unwrap();
        let query = LayoutInput { run_mode: RunMode::ComputeSize, ..LayoutInput::HIDDEN };
        let final_query = LayoutInput { run_mode: RunMode::PerformLayout, ..query };
        let output = LayoutOutput::from_outer_size(Size { width: 20.0, height: 10.0 });
        tree.store_cached_layout(child, &query, output.clone(), true);
        tree.store_cached_layout(sibling, &query, output.clone(), true);
        tree.store_cached_layout(parent, &query, output.clone(), true);
        tree.store_cached_layout(parent, &final_query, output.clone(), true);
        assert!(tree.nodes[child.into()].cache.is_empty());
        assert!(!tree.dirty(child).unwrap());
        let old = tree.clone();
        assert!(Arc::ptr_eq(tree.exact_measure_caches.get(child.into()).unwrap(), old.exact_measure_caches.get(child.into()).unwrap()));
        // A private preview that adds a query copies only that node's cache.
        let different = LayoutInput { parent_size: Size { width: Some(90.0), height: None }, ..query };
        tree.store_cached_layout(sibling, &different, output.clone(), true);
        assert!(!Arc::ptr_eq(tree.exact_measure_caches.get(sibling.into()).unwrap(), old.exact_measure_caches.get(sibling.into()).unwrap()));
        assert_eq!(old.exact_measure_caches.get(sibling.into()).unwrap().get(&different), None);
        tree.set_style(child, Style { size: Size { width: Dimension::length(50.0), height: Dimension::AUTO }, ..Style::DEFAULT }).unwrap();
        assert!(tree.dirty(child).unwrap());
        assert!(tree.dirty(parent).unwrap());
        assert_eq!(tree.cached_layout(child, &query, true), None);
        assert_eq!(tree.cached_layout(parent, &query, true), None);
        assert_eq!(tree.cached_layout(parent, &final_query, true), None);
        assert_eq!(tree.cached_layout(sibling, &query, true), Some(output.clone()));
        assert_eq!(old.exact_measure_caches.get(child.into()).unwrap().get(&query), Some(output.clone()));
        // Measurement resource/context epochs use the same invalidation path.
        tree.store_cached_layout(child, &query, output.clone(), true);
        tree.store_cached_layout(parent, &final_query, output, true);
        tree.set_node_context(child, Some(())).unwrap();
        assert!(tree.dirty(child).unwrap());
        assert!(tree.dirty(parent).unwrap());
    }

    #[test]
    fn exact_measure_side_tables_are_lazy_and_follow_hidden_clear_and_node_generations() {
        let mut tree = TaffyTree::<()>::new();
        let node = tree.new_leaf(Style::DEFAULT).unwrap();
        tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
        assert!(tree.exact_measure_caches.is_empty(), "ordinary layout allocates no exact entries");
        let query = LayoutInput { run_mode: RunMode::ComputeSize, ..LayoutInput::HIDDEN };
        tree.store_cached_layout(node, &query, LayoutOutput::HIDDEN, true);
        CacheTree::cache_clear(&mut tree, node);
        assert!(tree.exact_measure_caches.is_empty());
        tree.store_cached_layout(node, &query, LayoutOutput::HIDDEN, true);
        tree.set_style(node, Style { display: Display::None, ..Style::DEFAULT }).unwrap();
        tree.compute_layout(node, Size::MAX_CONTENT).unwrap();
        assert!(tree.exact_measure_caches.is_empty());
        tree.store_cached_layout(node, &query, LayoutOutput::HIDDEN, true);
        tree.remove(node).unwrap();
        assert!(tree.exact_measure_caches.is_empty());
        let next = tree.new_leaf(Style::DEFAULT).unwrap();
        assert_ne!(node, next);
        assert_eq!(tree.cached_layout(next, &query, true), None);
        tree.store_cached_layout(next, &query, LayoutOutput::HIDDEN, true);
        tree.clear();
        assert!(tree.exact_measure_caches.is_empty());
    }

    #[test]
    fn switching_back_to_ordinary_layout_keeps_exact_only_invalidation_provenance() {
        #[derive(Debug)]
        struct Unused;
        impl crate::tree::CustomLayout for Unused {
            fn layout(&self, _: crate::tree::CustomLayoutInput, _: &mut dyn crate::tree::CustomLayoutChildren) -> Result<crate::tree::CustomLayoutOutput, TaffyError> {
                unreachable!("this test exercises cache lifetime, not algorithm dispatch")
            }
        }
        let mut tree = TaffyTree::<()>::new();
        let child = tree.new_leaf(Style::DEFAULT).unwrap();
        let root = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
        tree.set_custom_layout(root, Some(Arc::new(Unused))).unwrap();
        let query = LayoutInput { run_mode: RunMode::ComputeSize, ..LayoutInput::HIDDEN };
        tree.store_cached_layout(child, &query, LayoutOutput::HIDDEN, true);
        tree.set_custom_layout(root, None).unwrap();
        assert!(!tree.dirty(child).unwrap());
        let final_query = LayoutInput { run_mode: RunMode::PerformLayout, ..query };
        tree.store_cached_layout(root, &final_query, LayoutOutput::HIDDEN, false);
        tree.mark_dirty(child).unwrap();
        assert!(tree.dirty(root).unwrap(), "removing the delegate cannot break later ancestor invalidation");
        assert!(tree.exact_measure_caches.is_empty());
    }

    fn size_measure_function(
        inputs: LayoutInput,
        _node_id: NodeId,
        node_context: Option<&mut Size<f32>>,
        style: &Style,
    ) -> LayoutOutput {
        compute_leaf_layout(
            inputs,
            style,
            |_, _| 0.0,
            |known_dimensions, _available_space| {
                known_dimensions.unwrap_or(node_context.as_deref().copied().unwrap_or(Size::ZERO))
            },
        )
    }

    #[test]
    fn intrinsic_limits_apply_to_container_and_item_algorithms() {
        for display in [Display::Flex, Display::Grid, Display::Block] {
            for nested in [false, true] {
                for (min, max, expected) in
                    [(Dimension::max_content(), auto(), 90.0), (auto(), Dimension::min_content(), 40.0)]
                {
                    let mut tree: TaffyTree<()> = TaffyTree::new();
                    tree.disable_rounding();
                    let children = [40.0, 50.0].map(|w| {
                        tree.new_leaf(Style {
                            size: Size { width: length(w), height: length(10.0) },
                            flex_shrink: 0.0,
                            ..Style::default()
                        })
                        .unwrap()
                    });
                    // The measured host uses Flex to provide distinct min/max
                    // contributions; its parent exercises all item algorithms.
                    let host = tree
                        .new_with_children(
                            Style {
                                size: Size { width: length(if min.is_auto() { 100.0 } else { 10.0 }), height: auto() },
                                min_size: Size { width: min, height: auto() },
                                max_size: Size { width: max, height: auto() },
                                flex_wrap: crate::FlexWrap::Wrap,
                                align_self: Some(crate::AlignSelf::START),
                                ..Style::default()
                            },
                            &children,
                        )
                        .unwrap();
                    let root = if nested {
                        tree.new_with_children(
                            Style {
                                display,
                                size: Size { width: length(100.0), height: length(100.0) },
                                ..Style::default()
                            },
                            &[host],
                        )
                        .unwrap()
                    } else {
                        host
                    };
                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                    let expected = if max.is_sizing_keyword() { 50.0 } else { expected };
                    assert_eq!(tree.layout(host).unwrap().size.width, expected, "{display:?}/{nested}/{min:?}/{max:?}");
                    let before = *tree.layout(host).unwrap();
                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                    assert_eq!(*tree.layout(host).unwrap(), before);
                }
            }
        }
    }

    #[test]
    fn intrinsic_limits_apply_inside_grid_and_block_hosts() {
        for display in [Display::Grid, Display::Block] {
            for nested in [false, true] {
                for minimum in [false, true] {
                    let mut tree: TaffyTree<()> = TaffyTree::new();
                    tree.disable_rounding();
                    let children = [40.0, 50.0].map(|width| {
                        tree.new_leaf(Style {
                            size: Size { width: length(width), height: length(10.0) },
                            ..Style::default()
                        })
                        .unwrap()
                    });
                    let host = tree
                        .new_with_children(
                            Style {
                                display,
                                size: Size { width: length(if minimum { 10.0 } else { 100.0 }), height: auto() },
                                min_size: Size {
                                    width: if minimum { Dimension::max_content() } else { auto() },
                                    height: auto(),
                                },
                                max_size: Size {
                                    width: if minimum { auto() } else { Dimension::max_content() },
                                    height: auto(),
                                },
                                align_self: Some(crate::AlignSelf::START),
                                flex_shrink: 0.0,
                                ..Style::default()
                            },
                            &children,
                        )
                        .unwrap();
                    let root = if nested {
                        tree.new_with_children(
                            Style { size: Size { width: length(100.0), height: length(100.0) }, ..Style::default() },
                            &[host],
                        )
                        .unwrap()
                    } else {
                        host
                    };
                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                    assert_eq!(
                        tree.layout(host).unwrap().size,
                        Size { width: 50.0, height: 20.0 },
                        "{display:?}/{nested}/{minimum}"
                    );
                    // A numeric maximum must not cap the intrinsic minimum's
                    // ContentSize query before minimum-wins-maximum clamping.
                    let mut style = tree.style(host).unwrap().clone();
                    style.min_size.width = Dimension::max_content();
                    style.max_size.width = length(30.0);
                    tree.set_style(host, style).unwrap();
                    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                    assert_eq!(tree.layout(host).unwrap().size.width, 50.0, "{display:?}/{nested}");
                }
            }
        }
    }

    #[test]
    fn intrinsic_leaf_height_limit_remeasures_after_flex_shrink() {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let child = tree
            .new_leaf(Style {
                size: Size { width: length(100.0), height: length(5.0) },
                min_size: Size { width: length(0.0), height: Dimension::max_content() },
                ..Style::default()
            })
            .unwrap();
        let root = tree
            .new_with_children(
                Style {
                    size: Size { width: length(50.0), height: length(100.0) },
                    align_items: crate::AlignItems::START,
                    ..Style::default()
                },
                &[child],
            )
            .unwrap();
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |inputs, _, _, style| {
            compute_leaf_layout(
                inputs,
                style,
                |_, _| 0.0,
                |known, space| {
                    let width = known.width.or(space.width.into_option()).unwrap_or(100.0);
                    Size { width: width.min(100.0), height: if width < 100.0 { 40.0 } else { 10.0 } }
                },
            )
        })
        .unwrap();
        assert_eq!(tree.layout(child).unwrap().size, Size { width: 50.0, height: 40.0 });
    }

    #[test]
    fn intrinsic_limits_size_absolutely_positioned_content() {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let children = [30.0, 40.0].map(|w| {
            tree.new_leaf(Style {
                size: Size { width: length(w), height: length(10.0) },
                flex_shrink: 0.0,
                ..Style::default()
            })
            .unwrap()
        });
        let host = tree
            .new_with_children(
                Style {
                    position: crate::Position::Absolute,
                    size: Size { width: length(100.0), height: auto() },
                    max_size: Size { width: Dimension::min_content(), height: auto() },
                    flex_wrap: crate::FlexWrap::Wrap,
                    ..Style::default()
                },
                &children,
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style { size: Size { width: length(200.0), height: length(100.0) }, ..Style::default() },
                &[host],
            )
            .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(host).unwrap().size, Size { width: 40.0, height: 20.0 });
    }
    #[test]
    fn size_containment_preserves_descendant_layout_for_all_container_algorithms() {
        for display in [Display::Flex, Display::Grid, Display::Block] {
            for contain in [crate::Contain::INLINE_SIZE, crate::Contain::SIZE] {
                let mut tree: TaffyTree<()> = TaffyTree::new();
                tree.disable_rounding();
                let child = tree
                    .new_leaf(Style {
                        size: Size { width: length(80.0), height: length(30.0) },
                        flex_shrink: 0.0,
                        ..Style::default()
                    })
                    .unwrap();
                let root = tree
                    .new_with_children(
                        Style {
                            display,
                            contain,
                            padding: crate::Rect {
                                left: length(2.0),
                                right: length(2.0),
                                top: length(2.0),
                                bottom: length(2.0),
                            },
                            ..Style::default()
                        },
                        &[child],
                    )
                    .unwrap();
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                assert_eq!(
                    tree.layout(root).unwrap().size,
                    Size { width: 4.0, height: if contain == crate::Contain::SIZE { 4.0 } else { 34.0 } },
                    "{display:?}/{contain:?}"
                );
                assert_eq!(tree.layout(child).unwrap().size, Size { width: 80.0, height: 30.0 });
                let before = *tree.layout(root).unwrap();
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                assert_eq!(*tree.layout(root).unwrap(), before);
            }
        }
    }

    #[test]
    fn inline_contained_leaf_measures_uncontained_height_against_its_used_width() {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let root = tree.new_leaf(Style { contain: crate::Contain::INLINE_SIZE, ..Style::default() }).unwrap();
        tree.compute_layout_with_measure(
            root,
            Size { width: AvailableSpace::Definite(300.0), height: AvailableSpace::MaxContent },
            |inputs, _, _, style| {
                compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |_, available| {
                        let width = available.width.into_option().unwrap_or(100.0);
                        Size { width: 100.0, height: if width < 100.0 { 40.0 } else { 10.0 } }
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(tree.layout(root).unwrap().size, Size { width: 0.0, height: 40.0 });
    }

    #[test]
    fn layout_containment_establishes_absolute_and_fixed_containing_block() {
        let mut tree: TaffyTree<()> = TaffyTree::new();
        tree.disable_rounding();
        let children = [crate::Position::Absolute, crate::Position::Fixed].map(|position| {
            tree.new_leaf(Style {
                position,
                size: Size { width: percent(0.5), height: percent(0.5) },
                inset: crate::Rect { left: length(5.0), right: auto(), top: length(6.0), bottom: auto() },
                ..Style::default()
            })
            .unwrap()
        });
        let host = tree
            .new_with_children(
                Style {
                    contain: crate::Contain::LAYOUT | crate::Contain::SIZE,
                    size: Size { width: length(100.0), height: length(80.0) },
                    margin: crate::Rect {
                        left: length(20.0),
                        right: length(0.0),
                        top: length(10.0),
                        bottom: length(0.0),
                    },
                    ..Style::default()
                },
                &children,
            )
            .unwrap();
        let root = tree
            .new_with_children(
                Style { size: Size { width: length(300.0), height: length(200.0) }, ..Style::default() },
                &[host],
            )
            .unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        for child in children {
            assert_eq!(tree.layout(child).unwrap().size, Size { width: 50.0, height: 40.0 });
            assert_eq!(tree.layout(child).unwrap().location, crate::Point { x: 5.0, y: 6.0 });
        }
    }

    #[test]
    fn new_should_allocate_default_capacity() {
        const DEFAULT_CAPACITY: usize = 16; // This is the capacity defined in the `impl Default`
        let taffy: TaffyTree<()> = TaffyTree::new();

        assert!(taffy.children.capacity() >= DEFAULT_CAPACITY);
        assert!(taffy.parents.capacity() >= DEFAULT_CAPACITY);
        assert!(taffy.nodes.capacity() >= DEFAULT_CAPACITY);
    }

    #[test]
    fn test_with_capacity() {
        const CAPACITY: usize = 8;
        let taffy: TaffyTree<()> = TaffyTree::with_capacity(CAPACITY);

        assert!(taffy.children.capacity() >= CAPACITY);
        assert!(taffy.parents.capacity() >= CAPACITY);
        assert!(taffy.nodes.capacity() >= CAPACITY);
    }

    #[test]
    fn test_new_leaf() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let res = taffy.new_leaf(Style::default());
        assert!(res.is_ok());
        let node = res.unwrap();

        // node should be in the taffy tree and have no children
        assert!(taffy.child_count(node) == 0);
    }

    #[test]
    fn new_leaf_with_context() {
        let mut taffy: TaffyTree<Size<f32>> = TaffyTree::new();

        let res = taffy.new_leaf_with_context(Style::default(), Size::ZERO);
        assert!(res.is_ok());
        let node = res.unwrap();

        // node should be in the taffy tree and have no children
        assert!(taffy.child_count(node) == 0);
    }

    /// Test that new_with_children works as expected
    #[test]
    fn test_new_with_children() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        // node should have two children
        assert_eq!(taffy.child_count(node), 2);
        assert_eq!(taffy.children(node).unwrap()[0], child0);
        assert_eq!(taffy.children(node).unwrap()[1], child1);
    }

    #[test]
    fn remove_node_should_remove() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let node = taffy.new_leaf(Style::default()).unwrap();

        let _ = taffy.remove(node).unwrap();
    }

    #[test]
    fn remove_node_should_detach_hierarchy() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        // Build a linear tree layout: <0> <- <1> <- <2>
        let node2 = taffy.new_leaf(Style::default()).unwrap();
        let node1 = taffy.new_with_children(Style::default(), &[node2]).unwrap();
        let node0 = taffy.new_with_children(Style::default(), &[node1]).unwrap();

        // Both node0 and node1 should have 1 child nodes
        assert_eq!(taffy.children(node0).unwrap().as_slice(), &[node1]);
        assert_eq!(taffy.children(node1).unwrap().as_slice(), &[node2]);

        // Disconnect the tree: <0> <2>
        let _ = taffy.remove(node1).unwrap();

        // Both remaining nodes should have no child nodes
        assert!(taffy.children(node0).unwrap().is_empty());
        assert!(taffy.children(node2).unwrap().is_empty());
    }

    #[test]
    fn remove_last_node() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let parent = taffy.new_leaf(Style::default()).unwrap();
        let child = taffy.new_leaf(Style::default()).unwrap();
        taffy.add_child(parent, child).unwrap();

        taffy.remove(child).unwrap();
        taffy.remove(parent).unwrap();
    }

    // Related to: https://github.com/DioxusLabs/taffy/issues/998
    #[test]
    fn remove_node_marks_parent_dirty() {
        use crate::prelude::*;

        let mut taffy: TaffyTree<()> = TaffyTree::new();

        // The root's height comes entirely from its child's 1px bottom border
        let child = taffy
            .new_leaf(Style { border: Rect { bottom: length(1f32), ..Rect::zero() }, ..Default::default() })
            .unwrap();
        let root = taffy.new_with_children(Style::default(), &[child]).unwrap();

        taffy.compute_layout(root, Size::MIN_CONTENT).unwrap();
        assert_eq!(taffy.layout(root).unwrap().size.height, 1f32);

        taffy.remove(child).unwrap();
        assert_eq!(taffy.dirty(root), Ok(true));

        // The root no longer has any children, so its height should shrink back to zero
        taffy.compute_layout(root, Size::MIN_CONTENT).unwrap();
        assert_eq!(taffy.layout(root).unwrap().size.height, 0f32);
    }

    #[test]
    fn set_measure() {
        let mut taffy: TaffyTree<Size<f32>> = TaffyTree::new();
        let node = taffy.new_leaf_with_context(Style::default(), Size { width: 200.0, height: 200.0 }).unwrap();
        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, size_measure_function).unwrap();
        assert_eq!(taffy.layout(node).unwrap().size.width, 200.0);

        taffy.set_node_context(node, Some(Size { width: 100.0, height: 100.0 })).unwrap();
        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, size_measure_function).unwrap();
        assert_eq!(taffy.layout(node).unwrap().size.width, 100.0);
    }

    #[test]
    fn set_measure_of_previously_unmeasured_node() {
        let mut taffy: TaffyTree<Size<f32>> = TaffyTree::new();
        let node = taffy.new_leaf(Style::default()).unwrap();
        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, size_measure_function).unwrap();
        assert_eq!(taffy.layout(node).unwrap().size.width, 0.0);

        taffy.set_node_context(node, Some(Size { width: 100.0, height: 100.0 })).unwrap();
        taffy.compute_layout_with_measure(node, Size::MAX_CONTENT, size_measure_function).unwrap();
        assert_eq!(taffy.layout(node).unwrap().size.width, 100.0);
    }

    /// Test that adding `add_child()` works
    #[test]
    fn add_child() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let node = taffy.new_leaf(Style::default()).unwrap();
        assert_eq!(taffy.child_count(node), 0);

        let child0 = taffy.new_leaf(Style::default()).unwrap();
        taffy.add_child(node, child0).unwrap();
        assert_eq!(taffy.child_count(node), 1);

        let child1 = taffy.new_leaf(Style::default()).unwrap();
        taffy.add_child(node, child1).unwrap();
        assert_eq!(taffy.child_count(node), 2);
    }

    #[test]
    fn insert_child_at_index() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let child2 = taffy.new_leaf(Style::default()).unwrap();

        let node = taffy.new_leaf(Style::default()).unwrap();
        assert_eq!(taffy.child_count(node), 0);

        taffy.insert_child_at_index(node, 0, child0).unwrap();
        assert_eq!(taffy.child_count(node), 1);
        assert_eq!(taffy.children(node).unwrap()[0], child0);

        taffy.insert_child_at_index(node, 0, child1).unwrap();
        assert_eq!(taffy.child_count(node), 2);
        assert_eq!(taffy.children(node).unwrap()[0], child1);
        assert_eq!(taffy.children(node).unwrap()[1], child0);

        taffy.insert_child_at_index(node, 1, child2).unwrap();
        assert_eq!(taffy.child_count(node), 3);
        assert_eq!(taffy.children(node).unwrap()[0], child1);
        assert_eq!(taffy.children(node).unwrap()[1], child2);
        assert_eq!(taffy.children(node).unwrap()[2], child0);
    }

    #[test]
    fn set_children() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        assert_eq!(taffy.child_count(node), 2);
        assert_eq!(taffy.children(node).unwrap()[0], child0);
        assert_eq!(taffy.children(node).unwrap()[1], child1);

        let child2 = taffy.new_leaf(Style::default()).unwrap();
        let child3 = taffy.new_leaf(Style::default()).unwrap();
        taffy.set_children(node, &[child2, child3]).unwrap();

        assert_eq!(taffy.child_count(node), 2);
        assert_eq!(taffy.children(node).unwrap()[0], child2);
        assert_eq!(taffy.children(node).unwrap()[1], child3);
    }

    /// Test that removing a child works
    #[test]
    fn remove_child() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        assert_eq!(taffy.child_count(node), 2);

        taffy.remove_child(node, child0).unwrap();
        assert_eq!(taffy.child_count(node), 1);
        assert_eq!(taffy.children(node).unwrap()[0], child1);

        taffy.remove_child(node, child1).unwrap();
        assert_eq!(taffy.child_count(node), 0);
    }

    #[test]
    fn remove_child_at_index() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        assert_eq!(taffy.child_count(node), 2);

        taffy.remove_child_at_index(node, 0).unwrap();
        assert_eq!(taffy.child_count(node), 1);
        assert_eq!(taffy.children(node).unwrap()[0], child1);

        taffy.remove_child_at_index(node, 0).unwrap();
        assert_eq!(taffy.child_count(node), 0);
    }

    #[test]
    fn remove_children_range() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let child2 = taffy.new_leaf(Style::default()).unwrap();
        let child3 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1, child2, child3]).unwrap();

        assert_eq!(taffy.child_count(node), 4);

        taffy.remove_children_range(node, 1..=2).unwrap();
        assert_eq!(taffy.child_count(node), 2);
        assert_eq!(taffy.children(node).unwrap(), [child0, child3]);
        for child in [child0, child3] {
            assert_eq!(taffy.parent(child), Some(node));
        }
        for child in [child1, child2] {
            assert_eq!(taffy.parent(child), None);
        }
    }

    // Related to: https://github.com/DioxusLabs/taffy/issues/510
    #[test]
    fn remove_child_updates_parents() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let parent = taffy.new_leaf(Style::default()).unwrap();
        let child = taffy.new_leaf(Style::default()).unwrap();

        taffy.add_child(parent, child).unwrap();

        taffy.remove(parent).unwrap();

        // Once the parent is removed this shouldn't panic.
        assert!(taffy.set_children(child, &[]).is_ok());
    }

    #[test]
    fn replace_child_at_index() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();

        let node = taffy.new_with_children(Style::default(), &[child0]).unwrap();
        assert_eq!(taffy.child_count(node), 1);
        assert_eq!(taffy.children(node).unwrap()[0], child0);

        taffy.replace_child_at_index(node, 0, child1).unwrap();
        assert_eq!(taffy.child_count(node), 1);
        assert_eq!(taffy.children(node).unwrap()[0], child1);
    }
    #[test]
    fn test_child_at_index() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let child2 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1, child2]).unwrap();

        assert!(if let Ok(result) = taffy.child_at_index(node, 0) { result == child0 } else { false });
        assert!(if let Ok(result) = taffy.child_at_index(node, 1) { result == child1 } else { false });
        assert!(if let Ok(result) = taffy.child_at_index(node, 2) { result == child2 } else { false });
    }
    #[test]
    fn test_child_count() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        assert!(taffy.child_count(node) == 2);
        assert!(taffy.child_count(child0) == 0);
        assert!(taffy.child_count(child1) == 0);
    }

    #[allow(clippy::vec_init_then_push)]
    #[test]
    fn test_children() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        let mut children = sys::Vec::new();
        children.push(child0);
        children.push(child1);

        let children_result = taffy.children(node).unwrap();
        assert_eq!(children_result, children);

        assert!(taffy.children(child0).unwrap().is_empty());
    }
    #[test]
    fn test_set_style() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let node = taffy.new_leaf(Style::default()).unwrap();
        assert_eq!(taffy.style(node).unwrap().display, Display::Flex);

        taffy.set_style(node, Style { display: Display::None, ..Style::default() }).unwrap();
        assert_eq!(taffy.style(node).unwrap().display, Display::None);
    }
    #[test]
    fn test_style() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let style = Style { display: Display::None, flex_direction: FlexDirection::RowReverse, ..Default::default() };

        let node = taffy.new_leaf(style.clone()).unwrap();

        let res = taffy.style(node);
        assert!(res.is_ok());
        assert!(res.unwrap() == &style);
    }
    #[test]
    fn test_layout() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let node = taffy.new_leaf(Style::default()).unwrap();

        // TODO: Improve this test?
        let res = taffy.layout(node);
        assert!(res.is_ok());
    }

    #[test]
    fn published_baselines_survive_cache_hits_dirtying_and_hidden_layout() {
        use crate::tree::Baselines;
        let mut taffy: TaffyTree<f32> = TaffyTree::new();
        taffy.disable_rounding();
        let child = taffy.new_leaf_with_context(Style::default(), 6.25).unwrap();
        let root = taffy.new_with_children(Style::default(), &[child]).unwrap();
        let mut measure = |inputs, _, baseline: Option<&mut f32>, style: &Style| {
            let mut output =
                compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size { width: 20.0, height: 12.0 });
            output.baselines = Baselines::from_first(baseline.map(|value| *value));
            output
        };
        taffy.compute_layout_with_measure(root, Size::MAX_CONTENT, &mut measure).unwrap();
        assert_eq!(taffy.layout(child).unwrap().baselines.first, Some(6.25));
        assert_eq!(taffy.layout(root).unwrap().baselines.first, Some(6.25));
        let mut writes = 0;
        taffy
            .compute_layout_with_measure_and_observer(root, Size::MAX_CONTENT, &mut measure, |_, _, _| writes += 1)
            .unwrap();
        // The parent still publishes its root position on a cached pass, while
        // a cached descendant retains its baseline without another write.
        assert_eq!(writes, 1);
        assert_eq!(taffy.layout(child).unwrap().baselines.first, Some(6.25));
        taffy.mark_dirty(child).unwrap();
        assert_eq!(taffy.layout(root).unwrap().baselines.first, Some(6.25));
        taffy.set_node_context(child, Some(8.5)).unwrap();
        let mut baseline_writes = 0;
        taffy
            .compute_layout_with_measure_and_observer(root, Size::MAX_CONTENT, &mut measure, |_, old, new| {
                if old.baselines != new.baselines {
                    assert_eq!(old.size, new.size);
                    assert_eq!(old.location, new.location);
                    baseline_writes += 1;
                }
            })
            .unwrap();
        assert_eq!(baseline_writes, 2);
        assert_eq!(taffy.layout(root).unwrap().baselines.first, Some(8.5));
        taffy.set_style(root, Style { display: Display::None, ..Style::default() }).unwrap();
        taffy.compute_layout_with_measure(root, Size::MAX_CONTENT, &mut measure).unwrap();
        assert_eq!(taffy.layout(root).unwrap().baselines, Baselines::NONE);
        assert_eq!(taffy.layout(child).unwrap().baselines, Baselines::NONE);
    }

    #[test]
    fn root_sizing_keywords_preserve_constraints_other_axis_and_final_children() {
        for display in [Display::Flex, Display::Grid, Display::Block] {
            let mut tree: TaffyTree<()> = TaffyTree::new();
            tree.disable_rounding();
            let children: Vec<_> = [30.0, 40.0]
                .into_iter()
                .map(|width| {
                    tree.new_leaf(Style {
                        size: Size {
                            width: length(width),
                            height: length(10.0),
                        },
                        flex_shrink: 0.0,
                        ..Style::default()
                    })
                    .unwrap()
                })
                .collect();
            let style = Style {
                display,
                size: Size {
                    width: Dimension::max_content(),
                    height: length(32.0),
                },
                max_size: Size {
                    width: length(55.0),
                    height: auto(),
                },
                flex_wrap: crate::FlexWrap::Wrap,
                grid_auto_flow: crate::GridAutoFlow::Column,
                padding: crate::Rect {
                    left: length(2.0),
                    right: length(2.0),
                    top: length(2.0),
                    bottom: length(2.0),
                },
                ..Style::default()
            };
            let root = tree.new_with_children(style, &children).unwrap();
            let viewport = Size {
                width: AvailableSpace::Definite(100.0),
                height: AvailableSpace::Definite(100.0),
            };
            tree.compute_layout(root, viewport).unwrap();
            let expected_width = if display == Display::Block { 44.0 } else { 55.0 };
            assert_eq!(
                tree.layout(root).unwrap().size,
                Size {
                    width: expected_width,
                    height: 32.0
                },
                "{display:?}"
            );
            // Cache reuse must preserve the final layout, not the preliminary query.
            let before = *tree.layout(root).unwrap();
            let child_before = *tree.layout(children[1]).unwrap();
            tree.compute_layout(root, viewport).unwrap();
            assert_eq!(*tree.layout(root).unwrap(), before);
            assert_eq!(*tree.layout(children[1]).unwrap(), child_before);
        }
    }

    #[test]
    fn intrinsic_column_used_height_wraps_without_resolving_percentage_height() {
        let wraps = [
            crate::FlexWrap::Wrap,
            #[cfg(feature = "flexbox_balance")]
            crate::FlexWrap::Balance,
        ];
        for wrap in wraps {
            let mut tree: TaffyTree<()> = TaffyTree::new();
            tree.disable_rounding();
            let children = [30.0, 40.0].map(|height| {
                tree.new_leaf(Style {
                    size: Size {
                        width: length(10.0),
                        height: length(height),
                    },
                    flex_shrink: 0.0,
                    ..Style::default()
                })
                .unwrap()
            });
            let host = tree
                .new_with_children(
                    Style {
                        size: Size {
                            width: length(50.0),
                            height: Dimension::max_content(),
                        },
                        min_size: Size {
                            width: auto(),
                            height: length(45.0),
                        },
                        max_size: Size {
                            width: auto(),
                            height: length(40.0),
                        },
                        flex_direction: crate::FlexDirection::Column,
                        flex_wrap: wrap,
                        align_items: crate::AlignItems::START,
                        align_self: Some(crate::AlignSelf::START),
                        ..Style::default()
                    },
                    &children,
                )
                .unwrap();
            let root = tree
                .new_with_children(
                    Style {
                        size: Size {
                            width: length(100.0),
                            height: length(100.0),
                        },
                        ..Style::default()
                    },
                    &[host],
                )
                .unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(host).unwrap().size.height, 45.0);
            assert_eq!(tree.layout(children[1]).unwrap().location, crate::Point { x: 25.0, y: 0.0 });

            // The same content-derived height is still indefinite for percentages.
            tree.set_style(
                children[0],
                Style {
                    size: Size {
                        width: length(10.0),
                        height: percent(0.5),
                    },
                    flex_shrink: 0.0,
                    ..Style::default()
                },
            )
            .unwrap();
            let grandchild = tree
                .new_leaf(Style {
                    size: Size {
                        width: length(10.0),
                        height: length(30.0),
                    },
                    ..Style::default()
                })
                .unwrap();
            tree.set_children(children[0], &[grandchild]).unwrap();
            tree.set_children(host, &[children[0]]).unwrap();
            tree.set_style(
                host,
                Style {
                    size: Size {
                        width: length(80.0),
                        height: Dimension::max_content(),
                    },
                    flex_direction: crate::FlexDirection::Column,
                    ..Style::default()
                },
            )
            .unwrap();
            tree.compute_layout(host, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(host).unwrap().size.height, 30.0);
            assert_eq!(tree.layout(children[0]).unwrap().size.height, 30.0);
        }
    }

    #[test]
    fn published_baselines_round_in_border_box_coordinates() {
        use crate::tree::Baselines;
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let root = taffy
            .new_leaf(Style {
                margin: crate::geometry::Rect { top: length(0.6), ..crate::geometry::Rect::zero() },
                ..Style::default()
            })
            .unwrap();
        taffy
            .compute_layout_with_measure(root, Size::MAX_CONTENT, |inputs, _, _, style| {
                let mut output =
                    compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size { width: 10.0, height: 12.0 });
                output.baselines = Baselines { first: Some(2.6), last: Some(8.8) };
                output
            })
            .unwrap();
        let unrounded = taffy.unrounded_layout(root);
        assert_eq!(unrounded.baselines, Baselines { first: Some(2.6), last: Some(8.8) });
        let rounded = taffy.layout(root).unwrap();
        assert_eq!(rounded.location.y, 1.0);
        // Round the absolute baseline, then subtract the rounded box origin.
        assert_eq!(rounded.baselines, Baselines { first: Some(2.0), last: Some(8.0) });
    }

    #[cfg(feature = "grid")]
    #[test]
    fn grid_propagates_final_baseline_with_one_or_multiple_aligned_items() {
        use crate::tree::Baselines;
        for alignment in [crate::AlignItems::START, crate::AlignItems::BASELINE] {
            for child_count in [1, 2] {
                let mut taffy: TaffyTree<f32> = TaffyTree::new();
                taffy.disable_rounding();
                let mut children = Vec::new();
                for index in 0..child_count {
                    children.push(
                        taffy
                            .new_leaf_with_context(
                                Style {
                                    size: Size { width: length(20.0), height: length(12.0) },
                                    margin: crate::geometry::Rect { top: length(2.0), ..crate::geometry::Rect::zero() },
                                    ..Style::default()
                                },
                                6.0 + index as f32 * 2.0,
                            )
                            .unwrap(),
                    );
                }
                let root = taffy
                    .new_with_children(
                        Style {
                            display: Display::Grid,
                            align_items: alignment,
                            grid_template_columns: vec![length(30.0), length(30.0)],
                            padding: crate::geometry::Rect { top: length(3.0), ..crate::geometry::Rect::zero() },
                            ..Style::default()
                        },
                        &children,
                    )
                    .unwrap();
                taffy
                    .compute_layout_with_measure(root, Size::MAX_CONTENT, |inputs, _, baseline, style| {
                        let mut output =
                            compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size { width: 20.0, height: 12.0 });
                        output.baselines = Baselines::from_first(baseline.copied());
                        output
                    })
                    .unwrap();
                let expected = if alignment == crate::AlignItems::BASELINE && child_count == 2 { 13.0 } else { 11.0 };
                let child = taffy.layout(children[0]).unwrap();
                assert_eq!(child.location.y + child.baselines.first.unwrap(), expected);
                // Child position already includes its margin; don't count it twice.
                assert_eq!(taffy.layout(root).unwrap().baselines.first, Some(expected));
            }
        }
    }

    #[test]
    fn test_mark_dirty() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child0 = taffy.new_leaf(Style::default()).unwrap();
        let child1 = taffy.new_leaf(Style::default()).unwrap();
        let node = taffy.new_with_children(Style::default(), &[child0, child1]).unwrap();

        taffy.compute_layout(node, Size::MAX_CONTENT).unwrap();

        assert_eq!(taffy.dirty(child0), Ok(false));
        assert_eq!(taffy.dirty(child1), Ok(false));
        assert_eq!(taffy.dirty(node), Ok(false));

        taffy.mark_dirty(node).unwrap();
        assert_eq!(taffy.dirty(child0), Ok(false));
        assert_eq!(taffy.dirty(child1), Ok(false));
        assert_eq!(taffy.dirty(node), Ok(true));

        taffy.compute_layout(node, Size::MAX_CONTENT).unwrap();
        taffy.mark_dirty(child0).unwrap();
        assert_eq!(taffy.dirty(child0), Ok(true));
        assert_eq!(taffy.dirty(child1), Ok(false));
        assert_eq!(taffy.dirty(node), Ok(true));
    }

    #[test]
    fn compute_layout_should_produce_valid_result() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let node_result = taffy.new_leaf(Style {
            size: Size { width: Dimension::from_length(10f32), height: Dimension::from_length(10f32) },
            ..Default::default()
        });
        assert!(node_result.is_ok());
        let node = node_result.unwrap();
        let layout_result = taffy.compute_layout(
            node,
            Size { width: AvailableSpace::Definite(100.), height: AvailableSpace::Definite(100.) },
        );
        assert!(layout_result.is_ok());
    }

    #[test]
    fn make_sure_layout_location_is_top_left() {
        use crate::prelude::*;

        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let node = taffy
            .new_leaf(Style {
                size: Size { width: Dimension::from_percent(1f32), height: Dimension::from_percent(1f32) },
                ..Default::default()
            })
            .unwrap();

        let root = taffy
            .new_with_children(
                Style {
                    size: Size { width: Dimension::from_length(100f32), height: Dimension::from_length(100f32) },
                    padding: Rect {
                        left: length(10f32),
                        right: length(20f32),
                        top: length(30f32),
                        bottom: length(40f32),
                    },
                    ..Default::default()
                },
                &[node],
            )
            .unwrap();

        taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

        // If Layout::location represents top-left coord, 'node' location
        // must be (due applied 'root' padding): {x: 10, y: 30}.
        //
        // It's important, since result will be different for each other
        // coordinate space:
        // - bottom-left:  {x: 10, y: 40}
        // - top-right:    {x: 20, y: 30}
        // - bottom-right: {x: 20, y: 40}
        let layout = taffy.layout(node).unwrap();
        assert_eq!(layout.location.x, 10f32);
        assert_eq!(layout.location.y, 30f32);
    }

    #[test]
    fn set_children_reparents() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let child = taffy.new_leaf(Style::default()).unwrap();
        let old_parent = taffy.new_with_children(Style::default(), &[child]).unwrap();

        let new_parent = taffy.new_leaf(Style::default()).unwrap();
        taffy.set_children(new_parent, &[child]).unwrap();

        assert!(taffy.children(old_parent).unwrap().is_empty());
    }

    /// A context that increments a counter when dropped. Each test owns its own static
    /// counter so the two can run in parallel.
    struct DropCounted(&'static core::sync::atomic::AtomicUsize);

    impl Drop for DropCounted {
        fn drop(&mut self) {
            self.0.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn remove_drops_the_nodes_context() {
        static DROPS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        let mut taffy: TaffyTree<DropCounted> = TaffyTree::new();
        let node = taffy.new_leaf_with_context(Style::DEFAULT, DropCounted(&DROPS)).unwrap();

        taffy.remove(node).unwrap();

        assert_eq!(
            DROPS.load(core::sync::atomic::Ordering::SeqCst),
            1,
            "removing a node must drop its context rather than strand it in node_context_data"
        );
    }

    #[test]
    fn clear_drops_every_nodes_context() {
        static DROPS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        let mut taffy: TaffyTree<DropCounted> = TaffyTree::new();
        for _ in 0..8 {
            taffy.new_leaf_with_context(Style::DEFAULT, DropCounted(&DROPS)).unwrap();
        }

        taffy.clear();

        assert_eq!(
            DROPS.load(core::sync::atomic::Ordering::SeqCst),
            8,
            "clear() drops all nodes, so it must drop their contexts too"
        );
    }
}
