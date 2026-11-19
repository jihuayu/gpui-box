use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
struct Press {
    target: UiNodeId,
    owner: UiNodeId,
}

/// One primary mouse pointer and one keyboard focus. All methods return owned
/// dispatch/visual effects; they never invoke user code or mutate UiDocument.
/// Adopt a scene only when the host installs that complete frame for input.
#[derive(Clone, Default)]
pub struct UiInputRouter {
    scene: Option<(UiInputScene, UiInputEvidence)>,
    pointer: Option<[f32; 2]>,
    hover: Option<UiNodeId>,
    press: Option<Press>,
    focus: Option<UiNodeId>,
    focus_visible: bool,
    key: Option<(UiActivationKey, UiNodeId)>,
}
impl UiInputRouter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn focused(&self) -> Option<UiNodeId> {
        self.focus
    }
    pub fn captured(&self) -> Option<UiNodeId> {
        self.press.map(|p| p.target)
    }
    pub fn hovered(&self) -> Option<UiNodeId> {
        self.hover
    }
    pub fn scene_id(&self) -> Option<UiInputSceneId> {
        self.scene.as_ref().map(|(s, _)| s.id)
    }

    pub fn adopt(
        &mut self,
        scene: UiInputScene,
        evidence: UiInputEvidence,
    ) -> Result<UiInputEffects, UiInputError> {
        if let Some((old, _)) = &self.scene {
            if scene.id.runtime != old.id.runtime {
                return Err(UiInputError::WrongRuntime);
            }
            if scene.id.epoch != old.id.epoch {
                return Err(UiInputError::WrongEpoch);
            }
            let ordered_sample = match (old.id.presentation, scene.id.presentation) {
                (None, Some(_)) => true,
                (Some(a), Some(b)) => a.stream == b.stream && a.revision <= b.revision,
                (None, None) => true,
                (Some(_), None) => false,
            };
            let stale_presentation = matches!((old.id.presentation, scene.id.presentation),
                (Some(a), Some(b)) if a.stream != b.stream || a.revision > b.revision);
            if stale_presentation
                || scene.id.source_revision < old.id.source_revision
                || scene.id.source_revision == old.id.source_revision
                    && (scene.id.tree != old.id.tree
                        || scene.id.frame_revision != old.id.frame_revision
                        || !ordered_sample)
            {
                return Err(UiInputError::StaleScene);
            }
        }
        let mut effects = UiInputEffects::default();
        let before = self.feedback();
        if let Some((old, old_evidence)) = &self.scene {
            let stable = |id| {
                old.id.tree == scene.id.tree
                    && old.graph.path(id) == scene.graph.path(id)
                    && old
                        .graph
                        .path(id)
                        .iter()
                        .all(|id| old.graph.nodes.get(id) == scene.graph.nodes.get(id))
            };
            if self.press.is_some_and(|press| {
                !scene.graph.eligible(press.target)
                    || !scene.graph.eligible(press.owner)
                    || !stable(press.target)
            }) {
                let press = self.press.take().unwrap();
                old.dispatch(
                    *old_evidence,
                    press.target,
                    UiInputEventKind::PointerCancel,
                    self.pointer,
                    &mut effects,
                );
            }
            if self
                .focus
                .is_some_and(|id| old.id.tree != scene.id.tree || !scene.graph.focusable(id))
            {
                old.dispatch(
                    *old_evidence,
                    self.focus.take().unwrap(),
                    UiInputEventKind::Blur,
                    None,
                    &mut effects,
                );
                self.focus_visible = false;
                self.key = None;
            }
            if self.key.is_some_and(|(_, id)| !stable(id)) {
                self.key = None;
            }
            // Leave is delivered using the old policy/geometry before retiring it.
            let next = self
                .pointer
                .and_then(|position| scene.hit_test(position))
                .filter(|id| scene.graph.eligible(*id));
            let old_path = self.hover.map(|id| old.graph.path(id)).unwrap_or_default();
            let next_path = next.map(|id| scene.graph.path(id)).unwrap_or_default();
            for id in old_path
                .iter()
                .filter(|id| old.id.tree != scene.id.tree || !next_path.contains(id))
            {
                old.dispatch(
                    *old_evidence,
                    *id,
                    UiInputEventKind::PointerLeave,
                    self.pointer,
                    &mut effects,
                );
            }
            for id in next_path
                .iter()
                .rev()
                .filter(|id| old.id.tree != scene.id.tree || !old_path.contains(id))
            {
                scene.dispatch(
                    evidence,
                    *id,
                    UiInputEventKind::PointerEnter,
                    self.pointer,
                    &mut effects,
                );
            }
            self.hover = next;
        } else {
            self.hover = self
                .pointer
                .and_then(|position| scene.hit_test(position))
                .filter(|id| scene.graph.eligible(*id));
        }
        self.scene = Some((scene, evidence));
        // Recovery can retain external IDs and their authored feedback. Emit
        // clears using the newly adopted scene identity after cancelling state.
        // Removed/reused generations are filtered by style_operations.
        self.finish_feedback(before, &mut effects);
        Ok(effects)
    }

    /// Retire interaction state before switching runtime/epoch. The host must
    /// process returned cancellation/blur effects before dropping its registry.
    pub fn reset(&mut self) -> UiInputEffects {
        let mut effects = UiInputEffects::default();
        let before = self.feedback();
        self.cancel(&mut effects);
        self.set_focus(None, false, &mut effects);
        self.set_hover(None, &mut effects);
        self.pointer = None;
        self.finish_feedback(before, &mut effects);
        self.scene = None;
        effects
    }

    pub fn route(&mut self, event: UiInput) -> Result<UiInputEffects, UiInputError> {
        let (scene, _) = self.scene.as_ref().ok_or(UiInputError::NoScene)?;
        match event {
            UiInput::Wheel { position, .. } if position.iter().any(|v| !v.is_finite()) => {
                return Err(UiInputError::InvalidPosition);
            }
            UiInput::Wheel { delta, .. } if delta.iter().any(|v| !v.is_finite()) => {
                return Err(UiInputError::InvalidScrollDelta);
            }
            UiInput::PointerMove(position)
            | UiInput::PrimaryDown(position)
            | UiInput::PrimaryUp(position)
                if position.iter().any(|v| !v.is_finite()) =>
            {
                return Err(UiInputError::InvalidPosition);
            }
            UiInput::Focus(Some(id)) if !scene.graph.focusable(id) => {
                return Err(UiInputError::IneligibleFocus);
            }
            _ => {}
        }
        let before = self.feedback();
        let mut effects = UiInputEffects::default();
        match event {
            UiInput::Wheel { position, delta } => {
                // Wheel targets geometry under the pointer, independently of
                // primary capture/focus. No synthetic click or focus transfer.
                self.move_pointer(position, &mut effects);
                let first = effects.dispatches.len();
                if let Some(target) = self.hover {
                    self.dispatch(
                        target,
                        UiInputEventKind::Wheel,
                        Some(position),
                        &mut effects,
                    );
                }
                for dispatch in &mut effects.dispatches[first..] {
                    dispatch.wheel_delta = Some(delta);
                }
                let target = self.hover;
                let route = target.map(|id| self.graph().path(id)).unwrap_or_default();
                let (scene, evidence) = self.scene.as_ref().unwrap();
                effects.wheel_scrolls.push(UiWheelScroll {
                    scene: scene.id,
                    evidence: *evidence,
                    target,
                    route: route.into(),
                    position,
                    delta,
                });
            }
            UiInput::PointerMove(position) => {
                self.move_pointer(position, &mut effects);
                let target = self.press.map(|p| p.target).or(self.hover);
                if let Some(target) = target {
                    self.dispatch(
                        target,
                        UiInputEventKind::PointerMove,
                        Some(position),
                        &mut effects,
                    );
                }
            }
            UiInput::PrimaryDown(position) => {
                self.move_pointer(position, &mut effects);
                self.cancel(&mut effects);
                let target = self.hover;
                let focus = target.and_then(|id| {
                    self.graph()
                        .path(id)
                        .into_iter()
                        .find(|&id| self.graph().focusable(id))
                });
                self.set_focus(focus, false, &mut effects);
                if let Some(target) = target {
                    self.press = Some(Press {
                        target,
                        owner: self.graph().owner(target),
                    });
                    self.dispatch(
                        target,
                        UiInputEventKind::PointerDown,
                        Some(position),
                        &mut effects,
                    );
                }
            }
            UiInput::PrimaryUp(position) => {
                self.move_pointer(position, &mut effects);
                if let Some(press) = self.press.take() {
                    self.dispatch(
                        press.target,
                        UiInputEventKind::PointerUp,
                        Some(position),
                        &mut effects,
                    );
                    if self
                        .hover
                        .is_some_and(|id| self.graph().owner(id) == press.owner)
                        && self.within(press.owner)
                    {
                        self.dispatch(
                            press.owner,
                            UiInputEventKind::Activate,
                            Some(position),
                            &mut effects,
                        );
                    }
                }
            }
            UiInput::PointerLeave => {
                self.pointer = None;
                self.set_hover(None, &mut effects);
            }
            UiInput::Cancel => self.cancel(&mut effects),
            UiInput::WindowFocusLost => {
                self.cancel(&mut effects);
                self.set_focus(None, false, &mut effects);
                self.set_hover(None, &mut effects);
                self.pointer = None;
            }
            UiInput::Tab { backwards } => {
                self.cancel(&mut effects);
                let order = &self.graph().tab_order;
                let next = if order.is_empty() {
                    None
                } else {
                    let index = self
                        .focus
                        .and_then(|id| order.iter().position(|value| *value == id));
                    Some(
                        order[match (index, backwards) {
                            (Some(0), true) | (None, true) => order.len() - 1,
                            (Some(i), true) => i - 1,
                            (Some(i), false) => (i + 1) % order.len(),
                            (None, false) => 0,
                        }],
                    )
                };
                self.set_focus(next, true, &mut effects);
                self.reveal_focus(&mut effects);
            }
            UiInput::Focus(target) => {
                self.cancel(&mut effects);
                self.set_focus(target, true, &mut effects);
                self.reveal_focus(&mut effects);
            }
            UiInput::KeyDown { key, repeat } => {
                if !repeat
                    && self.key.is_none()
                    && let Some(target) = self.focus
                {
                    self.focus_visible = true;
                    if self.graph().nodes[&target]
                        .policy
                        .activate_handler
                        .is_some()
                    {
                        if key == UiActivationKey::Enter {
                            self.dispatch(target, UiInputEventKind::Activate, None, &mut effects);
                        }
                        self.key = Some((key, target));
                    }
                }
            }
            UiInput::KeyUp(key) => {
                if self.key.is_some_and(|(pressed, _)| pressed == key) {
                    let (_, target) = self.key.take().unwrap();
                    if key == UiActivationKey::Space && self.focus == Some(target) {
                        self.dispatch(target, UiInputEventKind::Activate, None, &mut effects);
                    }
                }
            }
        }
        self.finish_feedback(before, &mut effects);
        Ok(effects)
    }
    fn graph(&self) -> &InputGraph {
        &self.scene.as_ref().unwrap().0.graph
    }
    fn reveal_focus(&self, effects: &mut UiInputEffects) {
        if let Some(target) = self.focus {
            let (scene, evidence) = self.scene.as_ref().unwrap();
            effects.focus_reveals.push(UiFocusReveal {
                scene: scene.id,
                evidence: *evidence,
                target,
                route: scene.graph.path(target).into(),
            });
        }
    }
    fn dispatch(
        &self,
        target: UiNodeId,
        kind: UiInputEventKind,
        position: Option<[f32; 2]>,
        effects: &mut UiInputEffects,
    ) {
        let (scene, evidence) = self.scene.as_ref().unwrap();
        scene.dispatch(*evidence, target, kind, position, effects);
    }
    fn within(&self, owner: UiNodeId) -> bool {
        self.hover
            .is_some_and(|id| self.graph().path(id).contains(&owner))
    }
    fn move_pointer(&mut self, position: [f32; 2], effects: &mut UiInputEffects) {
        self.pointer = Some(position);
        let next = self
            .scene
            .as_ref()
            .unwrap()
            .0
            .hit_test(position)
            .filter(|id| self.graph().eligible(*id));
        self.set_hover(next, effects);
    }
    fn set_hover(&mut self, next: Option<UiNodeId>, effects: &mut UiInputEffects) {
        if self.hover == next {
            return;
        }
        let before = self
            .hover
            .map(|id| self.graph().path(id))
            .unwrap_or_default();
        let after = next.map(|id| self.graph().path(id)).unwrap_or_default();
        for id in before.iter().filter(|id| !after.contains(id)) {
            self.dispatch(*id, UiInputEventKind::PointerLeave, self.pointer, effects);
        }
        for id in after.iter().rev().filter(|id| !before.contains(id)) {
            self.dispatch(*id, UiInputEventKind::PointerEnter, self.pointer, effects);
        }
        self.hover = next;
    }
    fn set_focus(&mut self, next: Option<UiNodeId>, visible: bool, effects: &mut UiInputEffects) {
        if self.focus != next {
            self.key = None;
            if let Some(id) = self.focus {
                self.dispatch(id, UiInputEventKind::Blur, None, effects);
            }
            self.focus = next;
            if let Some(id) = next {
                self.dispatch(id, UiInputEventKind::Focus, None, effects);
            }
        }
        self.focus_visible = next.is_some() && visible;
    }
    fn cancel(&mut self, effects: &mut UiInputEffects) {
        if let Some(press) = self.press.take() {
            self.dispatch(
                press.target,
                UiInputEventKind::PointerCancel,
                self.pointer,
                effects,
            );
        }
        self.key = None;
    }
    fn feedback(&self) -> BTreeMap<UiNodeId, (bool, bool, bool)> {
        if self.scene.is_none() {
            return BTreeMap::new();
        }
        let mut result = BTreeMap::new();
        if let Some(id) = self.hover {
            for id in self.graph().path(id) {
                result.entry(id).or_insert((false, false, false)).0 = true;
            }
        }
        if let Some(press) = self.press
            && self.within(press.owner)
            && self
                .hover
                .is_some_and(|id| self.graph().owner(id) == press.owner)
        {
            result.entry(press.owner).or_insert((false, false, false)).1 = true;
        }
        if let Some((UiActivationKey::Space, id)) = self.key {
            result.entry(id).or_insert((false, false, false)).1 = true;
        }
        if self.focus_visible
            && let Some(id) = self.focus
        {
            result.entry(id).or_insert((false, false, false)).2 = true;
        }
        result
    }
    fn finish_feedback(
        &self,
        before: BTreeMap<UiNodeId, (bool, bool, bool)>,
        effects: &mut UiInputEffects,
    ) {
        let after = self.feedback();
        let ids: BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
        let scene = self.scene.as_ref().map(|(scene, _)| scene.id);
        for id in ids {
            let value = after.get(&id).copied().unwrap_or_default();
            if before.get(&id).copied().unwrap_or_default() != value
                && let Some(scene) = scene
            {
                effects.feedback.push(UiStyleFeedback {
                    scene,
                    node: id,
                    hovered: value.0,
                    pressed: value.1,
                    focus_visible: value.2,
                });
            }
        }
    }
}
