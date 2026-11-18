//! Owned UI input records. These are not paint commands or a stable wire ABI.
//! All messages use framework-owned values and resource IDs; the owner adapter
//! validates final state and publishes through the ordinary retained tree.
use super::input::UiInteraction;
use super::responsive::LayoutEnvironment;
use super::style::{StyleError, StyleRef, StyleState};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod planning;
mod runtime;
pub use planning::UiPlanningStats;
pub use runtime::{UiDocument, UiPublication, UiResources};
#[cfg(test)]
mod tests;

pub const UI_SCHEMA_VERSION: u32 = 4;

pub(super) type NodeMap = BTreeMap<UiNodeId, Arc<UiNode>>;

/// Client identity within one runtime epoch. Slots may be reused only with a
/// strictly greater generation, including after snapshot recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UiNodeId {
    pub slot: u64,
    pub generation: u64,
}
impl UiNodeId {
    pub const fn new(slot: u64, generation: u64) -> Self {
        Self { slot, generation }
    }
}

/// Immutable resource-table key, scoped to the table supplied by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UiResourceId(pub u64);

/// Primitive content, separate from the node's layout and common properties.
/// Text references already prepared text resources; no platform shaping callback
/// or Window borrow can be carried inside a message.
#[derive(Clone, Debug, PartialEq)]
pub enum UiContent {
    Box,
    Fixed {
        size: [f32; 2],
        first_baseline: Option<f32>,
        last_baseline: Option<f32>,
    },
    Image(UiResourceId),
    Text {
        resource: UiResourceId,
        color: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiProperties {
    pub color: u32,
    pub translation: [f32; 2],
    pub clip: Option<Rect>,
    pub scroll: [f32; 2],
    pub opacity: f32,
    pub decoration: QuadDecoration,
    pub shadows: Arc<[BoxShadow]>,
}
impl Default for UiProperties {
    fn default() -> Self {
        Self {
            color: 0,
            translation: [0.0; 2],
            clip: None,
            scroll: [0.0; 2],
            opacity: 1.0,
            decoration: QuadDecoration::default(),
            shadows: Arc::from([]),
        }
    }
}

/// A primitive UI node. Child order is logical order; normal LayoutStyle order
/// remains the independent layout/paint ordering input. No Taffy Style appears.
#[derive(Clone, Debug, PartialEq)]
pub struct UiNode {
    pub id: UiNodeId,
    pub children: Vec<UiNodeId>,
    pub layout: LayoutStyle,
    pub properties: UiProperties,
    pub content: UiContent,
    /// Applied after the authored base, in order. An empty list keeps the base.
    pub styles: Vec<StyleRef>,
    pub style_state: StyleState,
    pub interaction: UiInteraction,
}
impl UiNode {
    pub fn new(id: UiNodeId, layout: LayoutStyle) -> Self {
        Self {
            id,
            children: vec![],
            layout,
            properties: UiProperties::default(),
            content: UiContent::Box,
            styles: vec![],
            style_state: StyleState::default(),
            interaction: UiInteraction::default(),
        }
    }
}

/// Full recoverable source state, including tombstones for retired slots.
/// Resource bytes/handlers remain outside this record and must be installed by
/// the receiving host before accepting it. Schema 4 is experimental Rust data.
#[derive(Clone, Debug, PartialEq)]
pub struct UiSnapshot {
    pub schema: u32,
    pub runtime: u64,
    pub epoch: u64,
    pub revision: u64,
    pub next_sequence: u64,
    pub root: UiNodeId,
    pub nodes: Vec<UiNode>,
    pub generations: Vec<(u64, u64)>,
    /// Pure condition input; distinct from layout constraints and paint viewport.
    pub style_environment: LayoutEnvironment,
}
impl UiSnapshot {
    /// Initial revision zero, next sequence one. Duplicate IDs/slots are
    /// rejected by the receiver rather than silently deduplicated here.
    pub fn new(runtime: u64, epoch: u64, root: UiNodeId, nodes: Vec<UiNode>) -> Self {
        let generations = nodes.iter().map(|n| (n.id.slot, n.id.generation)).collect();
        Self {
            schema: UI_SCHEMA_VERSION,
            runtime,
            epoch,
            revision: 0,
            next_sequence: 1,
            root,
            nodes,
            generations,
            style_environment: LayoutEnvironment::new([0.0; 2]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiOperation {
    Create(Box<UiNode>),
    Layout {
        node: UiNodeId,
        value: Box<LayoutStyle>,
    },
    Properties {
        node: UiNodeId,
        value: UiProperties,
    },
    /// Replace only authored scroll, preserving all other common properties.
    /// Styles can still own scroll; input planning skips those competing writers.
    Scroll {
        node: UiNodeId,
        value: [f32; 2],
    },
    Content {
        node: UiNodeId,
        value: UiContent,
    },
    Children {
        node: UiNodeId,
        value: Vec<UiNodeId>,
    },
    Styles {
        node: UiNodeId,
        value: Vec<StyleRef>,
    },
    StyleState {
        node: UiNodeId,
        value: StyleState,
    },
    StyleEnvironment(LayoutEnvironment),
    Interaction {
        node: UiNodeId,
        value: UiInteraction,
    },
    /// Remove the subtree in the final draft child graph. Move a surviving
    /// child out of that graph before removing its old host.
    RemoveSubtree(UiNodeId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiBatch {
    pub schema: u32,
    pub runtime: u64,
    pub epoch: u64,
    pub base_revision: u64,
    pub revision: u64,
    pub sequence: u64,
    pub operations: Vec<UiOperation>,
}

/// Receiver-side work bounds. This does not measure resource-table bytes or
/// promise a hard wall-time deadline. Source graphs and caches still clone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiLimits {
    pub max_nodes: usize,
    pub max_operations: usize,
    pub max_depth: usize,
    pub max_shadows_per_node: usize,
    pub max_generation_slots: usize,
    pub max_children_per_node: usize,
    /// Authored explicit/implicit track entries and flat repeat fragments in
    /// one style. Expanded solver work retains the kernel's own finite limits.
    pub max_grid_track_entries: usize,
    pub max_styles_per_node: usize,
    pub max_style_declarations_per_node: usize,
    pub max_style_cases_per_node: usize,
}
impl Default for UiLimits {
    fn default() -> Self {
        Self {
            max_nodes: 100_000,
            max_operations: 100_000,
            max_depth: 1_024,
            max_shadows_per_node: 64,
            max_generation_slots: 200_000,
            max_children_per_node: 100_000,
            max_grid_track_entries: 4_096,
            max_styles_per_node: 64,
            max_style_declarations_per_node: 1_024,
            max_style_cases_per_node: 4_096,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiApplyStatus {
    Applied,
    /// Only the most recent exactly equal batch can be replayed. Older packets
    /// require recovery and never execute operations a second time.
    Replayed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiError {
    UnsupportedSchema(u32),
    WrongRuntime,
    WrongEpoch,
    WrongResourceTable,
    ResyncRequired {
        expected_revision: u64,
        expected_sequence: u64,
    },
    ReplayConflict,
    InvalidRevision,
    NonReflexiveValue,
    InvalidIdentity(UiNodeId),
    DuplicateSlot(u64),
    InvalidTopology,
    LimitExceeded,
    MissingResource(UiResourceId),
    ResourceConflict(UiResourceId),
    UnsupportedContent,
    InvalidHandler,
    Style(StyleError),
    Retained(Error),
}
impl From<Error> for UiError {
    fn from(error: Error) -> Self {
        Self::Retained(error)
    }
}
impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UI input error: {self:?}")
    }
}
impl std::error::Error for UiError {}

fn validate_header(schema: u32, runtime: u64, epoch: u64) -> Result<(), UiError> {
    if schema != UI_SCHEMA_VERSION {
        return Err(UiError::UnsupportedSchema(schema));
    }
    if runtime == 0 {
        return Err(UiError::WrongRuntime);
    }
    if epoch == 0 {
        return Err(UiError::WrongEpoch);
    }
    Ok(())
}

fn validate_graph(root: UiNodeId, nodes: &NodeMap, limits: UiLimits) -> Result<(), UiError> {
    if nodes.len() > limits.max_nodes {
        return Err(UiError::LimitExceeded);
    }
    if !nodes.contains_key(&root) {
        return Err(UiError::InvalidTopology);
    }
    let mut slots = BTreeSet::new();
    let mut parents = BTreeSet::new();
    for (&id, node) in nodes {
        if id != node.id || id.generation == 0 {
            return Err(UiError::InvalidIdentity(id));
        }
        if !slots.insert(id.slot) {
            return Err(UiError::DuplicateSlot(id.slot));
        }
        if node.children.len() > limits.max_children_per_node {
            return Err(UiError::LimitExceeded);
        }
        validate_style_bounds(&node.layout, limits)?;
        for child in &node.children {
            if *child == root || !nodes.contains_key(child) || !parents.insert(*child) {
                return Err(UiError::InvalidTopology);
            }
        }
        if !node.children.is_empty() && node.content != UiContent::Box {
            return Err(UiError::Retained(Error::MeasuredNodeHasChildren));
        }
        validate_payload_bounds(node, limits)?;
    }
    let mut seen = BTreeSet::new();
    let mut stack = vec![(root, 1)];
    while let Some((id, depth)) = stack.pop() {
        if depth > limits.max_depth {
            return Err(UiError::LimitExceeded);
        }
        if !seen.insert(id) {
            return Err(UiError::InvalidTopology);
        }
        stack.extend(nodes[&id].children.iter().map(|&id| (id, depth + 1)));
    }
    if seen.len() != nodes.len() {
        return Err(UiError::InvalidTopology);
    }
    Ok(())
}

fn validate_style_bounds(style: &LayoutStyle, limits: UiLimits) -> Result<(), UiError> {
    if let Some(grid) = &style.grid {
        let mut entries = grid
            .auto_columns
            .len()
            .checked_add(grid.auto_rows.len())
            .ok_or(UiError::LimitExceeded)?;
        for track in grid.columns.iter().chain(&grid.rows) {
            let count = match track {
                GridTemplate::Track(_) => 1,
                GridTemplate::Repeat { tracks, .. } => tracks.len().saturating_add(1),
            };
            entries = entries.checked_add(count).ok_or(UiError::LimitExceeded)?;
            if entries > limits.max_grid_track_entries {
                return Err(UiError::LimitExceeded);
            }
        }
        if entries > limits.max_grid_track_entries {
            return Err(UiError::LimitExceeded);
        }
    }
    Ok(())
}

pub(in crate::retained) fn validate_properties(p: &UiProperties) -> Result<(), UiError> {
    if p.translation.iter().any(|v| !v.is_finite()) {
        return Err(Error::InvalidTranslation.into());
    }
    if p.scroll.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err(Error::InvalidScroll.into());
    }
    if p.clip.is_some_and(|r| !r.valid()) {
        return Err(Error::InvalidClip.into());
    }
    if !p.opacity.is_finite() || !(0.0..=1.0).contains(&p.opacity) {
        return Err(Error::InvalidOpacity.into());
    }
    p.decoration.validate()?;
    for shadow in p.shadows.iter() {
        shadow.validate()?;
    }
    Ok(())
}
fn validate_content(content: &UiContent) -> Result<(), UiError> {
    if let UiContent::Fixed {
        size,
        first_baseline,
        last_baseline,
    } = *content
    {
        MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
            width: size[0],
            height: size[1],
            first_baseline,
            last_baseline,
        })
        .validate()?;
    }
    Ok(())
}
fn validate_payload_bounds(node: &UiNode, limits: UiLimits) -> Result<(), UiError> {
    node.interaction.validate()?;
    if node.children.len() > limits.max_children_per_node
        || node.properties.shadows.len() > limits.max_shadows_per_node
    {
        return Err(UiError::LimitExceeded);
    }
    validate_style_refs(&node.styles, limits)?;
    validate_style_bounds(&node.layout, limits)?;
    node.layout.validate()?;
    validate_properties(&node.properties)?;
    validate_content(&node.content)
}

fn validate_style_refs(styles: &[StyleRef], limits: UiLimits) -> Result<(), UiError> {
    if styles.len() > limits.max_styles_per_node {
        return Err(UiError::LimitExceeded);
    }
    let (mut declarations, mut cases) = (0usize, 0usize);
    for style in styles {
        let work = style.work();
        declarations = declarations
            .checked_add(work.0)
            .ok_or(UiError::LimitExceeded)?;
        cases = cases.checked_add(work.1).ok_or(UiError::LimitExceeded)?;
        if work.2 > limits.max_grid_track_entries || work.3 > limits.max_shadows_per_node {
            return Err(UiError::LimitExceeded);
        }
        if declarations > limits.max_style_declarations_per_node
            || cases > limits.max_style_cases_per_node
        {
            return Err(UiError::LimitExceeded);
        }
    }
    Ok(())
}
