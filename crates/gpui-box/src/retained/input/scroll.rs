//! Wheel default scrolling uses adopted targeting and current atomic source
//! offsets. No prediction is silently substituted for installed hit geometry.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiScrollState {
    /// Effective clamped layout/render offset of this complete publication.
    pub offset: [f32; 2],
    pub maximum: [f32; 2],
    /// Only Scroll overflow and an unowned authored scroll accept wheel input.
    pub wheel_axes: [bool; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiWheelScroll {
    pub scene: UiInputSceneId,
    pub evidence: UiInputEvidence,
    pub target: Option<UiNodeId>,
    /// Adopted render ancestry, target first. Reparent/retirement invalidates it.
    pub route: Arc<[UiNodeId]>,
    pub position: [f32; 2],
    pub delta: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiScrollOutcome {
    pub scene: UiInputSceneId,
    pub target: Option<UiNodeId>,
    pub consumed: [f32; 2],
    /// Unconsumed logical pixels for host-level chaining/platform behavior.
    pub remaining: [f32; 2],
}

/// Exact-base ordinary UI batch plus consumption diagnostics. Apply the batch
/// once using UiDocument::apply; a stale base rejects instead of overwriting
/// newer offsets. Nothing changes until that complete publication succeeds.
#[derive(Clone, Debug, PartialEq)]
pub struct UiScrollPlan {
    pub batch: super::super::ui_ir::UiBatch,
    pub outcomes: Vec<UiScrollOutcome>,
}

pub(in crate::retained) fn states(
    tree: &UiTree,
    mapping: &BTreeMap<UiNodeId, NodeId>,
    effective: &super::super::ui_ir::NodeMap,
    authored: &super::super::ui_ir::NodeMap,
) -> Arc<BTreeMap<UiNodeId, UiScrollState>> {
    let mut states = BTreeMap::new();
    for (&id, source) in effective {
        let overflow = source.layout.overflow;
        if overflow.width != LayoutOverflow::Scroll && overflow.height != LayoutOverflow::Scroll {
            continue;
        }
        let node = tree.node(mapping[&id]).expect("adopted mapped scroll host");
        let maximum = [
            node.published_layout.scroll_width(),
            node.published_layout.scroll_height(),
        ];
        let owned = authored[&id].styles.iter().any(|style| {
            style.declarations().iter().any(|declaration| {
                matches!(
                    declaration,
                    super::super::style::StyleDeclaration::Scroll(_)
                )
            })
        });
        states.insert(
            id,
            UiScrollState {
                offset: [
                    node.scroll[0].min(maximum[0]),
                    node.scroll[1].min(maximum[1]),
                ],
                maximum,
                wheel_axes: [
                    !owned && overflow.width == LayoutOverflow::Scroll,
                    !owned && overflow.height == LayoutOverflow::Scroll,
                ],
            },
        );
    }
    Arc::new(states)
}

/// Shared wheel math over one complete current scene. Owners publish these
/// offsets only with their ordinary atomic candidate.
pub(in crate::retained) struct WheelPlan {
    pub offsets: BTreeMap<UiNodeId, [f32; 2]>,
    pub outcomes: Vec<UiScrollOutcome>,
}
pub(in crate::retained) fn plan_wheels(
    current: &UiInputScene,
    events: &[UiWheelScroll],
) -> Result<WheelPlan, UiInputError> {
    let mut offsets = BTreeMap::<UiNodeId, [f32; 2]>::new();
    let mut outcomes = vec![];
    for event in events {
        if event.position.iter().any(|v| !v.is_finite()) {
            return Err(UiInputError::InvalidPosition);
        }
        if event.delta.iter().any(|v| !v.is_finite()) {
            return Err(UiInputError::InvalidScrollDelta);
        }
        if event.scene.runtime != current.id.runtime {
            return Err(UiInputError::WrongRuntime);
        }
        if event.scene.epoch != current.id.epoch {
            return Err(UiInputError::WrongEpoch);
        }
        if event.scene.tree != current.id.tree
            || event.scene.source_revision > current.id.source_revision
            || event.scene.source_revision == current.id.source_revision
                && event.scene != current.id
        {
            return Err(UiInputError::StaleScrollRoute);
        }
        let valid = match event.target {
            Some(target) => {
                current.graph.eligible(target)
                    && current.graph.path(target).as_slice() == event.route.as_ref()
            }
            None => event.route.is_empty(),
        };
        if !valid {
            return Err(UiInputError::StaleScrollRoute);
        }
        let mut remaining = event.delta.map(f64::from);
        let mut consumed = [0.0f64; 2];
        for &id in event.route.iter() {
            let Some(state) = current.scroll_state(id) else {
                continue;
            };
            if !current.graph.eligible(id) {
                continue;
            }
            let before = offsets.get(&id).copied().unwrap_or(state.offset);
            let mut next = before;
            for axis in 0..2 {
                if !state.wheel_axes[axis] || remaining[axis] == 0.0 {
                    continue;
                }
                // f64 addition avoids finite f32 deltas overflowing before
                // clamping. Only representable consumed distance is removed.
                next[axis] = ((before[axis] as f64 + remaining[axis])
                    .clamp(0.0, state.maximum[axis] as f64)) as f32;
                // Rounding must not consume beyond the requested distance
                // and reverse the residual handed to an ancestor.
                let rounded = f64::from(next[axis]) - f64::from(before[axis]);
                if remaining[axis] > 0.0 && rounded > remaining[axis] {
                    next[axis] = next[axis].next_down().max(before[axis]);
                } else if remaining[axis] < 0.0 && rounded < remaining[axis] {
                    next[axis] = next[axis].next_up().min(before[axis]);
                }
                let used = f64::from(next[axis]) - f64::from(before[axis]);
                consumed[axis] += used;
                remaining[axis] -= used;
            }
            if next != before {
                offsets.insert(id, next);
            }
        }
        outcomes.push(UiScrollOutcome {
            scene: event.scene,
            target: event.target,
            consumed: consumed.map(|value| value as f32),
            remaining: remaining.map(|value| value as f32),
        });
    }
    Ok(WheelPlan { offsets, outcomes })
}

pub(in crate::retained) fn validate_reveals(
    current: &UiInputScene,
    requests: &[UiFocusReveal],
) -> Result<Vec<UiNodeId>, UiInputError> {
    let mut targets = vec![];
    for request in requests {
        if request.scene.runtime != current.id.runtime {
            return Err(UiInputError::WrongRuntime);
        }
        if request.scene.epoch != current.id.epoch {
            return Err(UiInputError::WrongEpoch);
        }
        if request.scene.tree != current.id.tree
            || request.scene.source_revision > current.id.source_revision
            || request.scene.source_revision == current.id.source_revision
                && request.scene != current.id
            || !current.graph.focusable(request.target)
            || current.graph.path(request.target).as_slice() != request.route.as_ref()
        {
            return Err(UiInputError::StaleScrollRoute);
        }
        targets.push(request.target);
    }
    Ok(targets)
}

impl UiDocument {
    /// Plan wheel deltas, focus reveal and style feedback together, preserving
    /// unrelated authored fields. Wheel deltas use current committed offsets;
    /// focus reveal runs after this batch's final style/layout settlement.
    pub fn plan_input_scroll(
        &self,
        effects: &UiInputEffects,
    ) -> Result<UiScrollPlan, UiInputError> {
        let current = self.input_scene();
        let WheelPlan { offsets, outcomes } = plan_wheels(&current, &effects.wheel_scrolls)?;
        let mut operations = effects.style_operations(self);
        operations.extend(offsets.into_iter().filter_map(|(node, value)| {
            (self.node(node)?.properties.scroll != value)
                .then_some(UiOperation::Scroll { node, value })
        }));
        // Focus reveal is a post-layout intent, not an absolute offset computed
        // from adopted (possibly old) geometry. The exact-base batch protects
        // this validation until apply; final draft policy is checked there too.
        for node in validate_reveals(&current, &effects.focus_reveals)? {
            operations.push(UiOperation::ScrollIntoView { node });
        }
        let revision = self
            .revision()
            .checked_add(1)
            .ok_or(UiInputError::RevisionExhausted)?;
        Ok(UiScrollPlan {
            batch: super::super::ui_ir::UiBatch {
                schema: super::super::ui_ir::UI_SCHEMA_VERSION,
                runtime: current.id.runtime,
                epoch: current.id.epoch,
                base_revision: self.revision(),
                revision,
                sequence: self.next_sequence(),
                operations,
            },
            outcomes,
        })
    }
}
