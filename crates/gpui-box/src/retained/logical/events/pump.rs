//! One routed owner transaction; image installation remains host evidence.
use super::*;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventControl {
    pub propagation_stopped: bool,
    pub default_prevented: bool,
}
pub struct LogicalInputCommit {
    pub commit: ReactiveCommit<()>,
    pub delivered: usize,
    pub ignored: usize,
    pub stopped: usize,
    pub prevented: usize,
    /// One entry per wheel, including source-controlled consumption. Prevented
    /// or retired defaults consume no additional distance.
    pub outcomes: Vec<UiScrollOutcome>,
    /// Complete immutable output to install before adopting its input scene.
    pub frame: local::LocalFrame,
}

fn same_event(a: &UiInputDispatch, b: &UiInputDispatch) -> bool {
    a.scene == b.scene
        && a.evidence == b.evidence
        && a.target == b.target
        && a.kind == b.kind
        && a.position == b.position
        && a.wheel_delta == b.wheel_delta
        && a.route == b.route
}
fn wheel_matches(event: &UiInputDispatch, wheel: &UiWheelScroll) -> bool {
    event.kind == UiInputEventKind::Wheel
        && event.scene == wheel.scene
        && Some(event.target) == wheel.target
        && event.route == wheel.route
        && event.position == Some(wheel.position)
        && event.wheel_delta == Some(wheel.delta)
}
impl LogicalUi {
    /// Route one input against the host's installed scene. Router state adopts
    /// only after the complete owner transaction succeeds; it never adopts a
    /// newer image automatically. Host still installs frame then adopts scene.
    pub fn handle_input(
        &mut self,
        router: &mut UiInputRouter,
        input: UiInput,
    ) -> Result<LogicalInputCommit, Error> {
        let installed = router
            .scene_id()
            .ok_or(Error::Input(UiInputError::NoScene))?;
        let current = self.input_scene()?.id();
        if installed.runtime != current.runtime {
            return Err(Error::Input(UiInputError::WrongRuntime));
        }
        if installed.epoch != current.epoch {
            return Err(Error::Input(UiInputError::WrongEpoch));
        }
        if installed.tree != current.tree || installed.source_revision > current.source_revision {
            return Err(Error::Input(UiInputError::StaleScene));
        }
        let mut candidate = router.clone();
        let effects = candidate.route(input).map_err(Error::Input)?;
        self.process_input_effects_adopting(&effects, || *router = candidate)
    }
    /// Host calls this only after installing the supplied scene's matching image.
    /// Enter/leave/cancel feedback and callbacks use the same atomic processing.
    /// If that processing changes appearance, the returned newer frame needs a
    /// subsequent host install; there is no automatic adopt-until-stable loop.
    pub fn adopt_input_scene(
        &mut self,
        router: &mut UiInputRouter,
        scene: UiInputScene,
        evidence: UiInputEvidence,
    ) -> Result<LogicalInputCommit, Error> {
        let current = self.input_scene()?.id();
        if scene.id.runtime != current.runtime {
            return Err(Error::Input(UiInputError::WrongRuntime));
        }
        if scene.id.epoch != current.epoch {
            return Err(Error::Input(UiInputError::WrongEpoch));
        }
        if scene.id.tree != current.tree || scene.id.source_revision > current.source_revision {
            return Err(Error::Input(UiInputError::StaleScene));
        }
        let mut candidate = router.clone();
        let effects = candidate.adopt(scene, evidence).map_err(Error::Input)?;
        self.process_input_effects_adopting(&effects, || *router = candidate)
    }
    /// Process one router route/adopt/reset result once, in router order.
    /// Dispatch callbacks share source settlement; final feedback and defaults
    /// adopt together. This does not mutate the caller's router or install pixels.
    pub fn process_input_effects(
        &mut self,
        effects: &UiInputEffects,
    ) -> Result<LogicalInputCommit, Error> {
        self.process_input_effects_adopting(effects, || {})
    }
    fn process_input_effects_adopting(
        &mut self,
        effects: &UiInputEffects,
        adopted: impl FnOnce(),
    ) -> Result<LogicalInputCommit, Error> {
        let scene = self.input_scene()?;
        input::scroll::plan_wheels(&scene, &effects.wheel_scrolls).map_err(Error::Input)?;
        input::scroll::validate_reveals(&scene, &effects.focus_reveals).map_err(Error::Input)?;
        let feedback = self.style_feedback(effects)?;
        let constraints = self
            .ui
            .tree()
            .layout_constraints
            .ok_or(Error::InvalidFrame)?;
        let viewport = self.ui.tree().viewport.ok_or(Error::InvalidFrame)?;
        let handlers: Vec<_> = effects
            .dispatches
            .iter()
            .map(|event| {
                if event.scene.runtime != scene.id.runtime
                    || event.scene.epoch != scene.id.epoch
                    || event.scene.tree != scene.id.tree
                    || event.scene.source_revision > scene.id.source_revision
                    || !scene.graph.accepts(event)
                {
                    return None;
                }
                self.events
                    .handlers
                    .iter()
                    .find(|(_, h)| h.id == event.handler)
                    .map(|((scope, _), h)| (*scope, h.callback.clone()))
            })
            .collect();
        let defaults = RefCell::new(effects.clone());
        defaults.borrow_mut().dispatches.clear();
        let outcomes = RefCell::new(vec![]);
        let mut after = vec![];
        let tasks = RefCell::new(Some(super::super::tasks::EventTasks::new(&self.tasks)));
        let mut delivered = 0;
        let mut ignored = 0;
        let mut stopped = 0;
        let mut prevented = 0;
        let mut prevented_wheels = BTreeSet::new();
        let mut residual_wheels = BTreeMap::new();
        let adopted = RefCell::new(Some(adopted));
        let on_adopt = || {
            if let Some(adopted) = adopted.borrow_mut().take() {
                adopted();
            }
        };
        let commit = self.edit_validated(
            |tx| {
                let mut tasks = tasks.borrow_mut();
                let mut index = 0;
                let mut suppress_activation = false;
                while index < effects.dispatches.len() {
                    let first = index;
                    index += 1;
                    while index < effects.dispatches.len()
                        && same_event(&effects.dispatches[first], &effects.dispatches[index])
                    {
                        index += 1;
                    }
                    let event = &effects.dispatches[first];
                    if event.kind == UiInputEventKind::Activate && suppress_activation {
                        ignored += index - first;
                        continue;
                    }
                    let mut group = EventControl::default();
                    let mut residual = (event.kind == UiInputEventKind::Wheel)
                        .then_some(event.wheel_delta)
                        .flatten();
                    let mut wheel_anchor = None;
                    for (event, handler) in effects.dispatches[first..index]
                        .iter()
                        .zip(&handlers[first..index])
                    {
                        if group.propagation_stopped {
                            stopped += 1;
                            continue;
                        }
                        let Some((scope, handler)) = handler else {
                            ignored += 1;
                            continue;
                        };
                        (handler.0)(
                            event,
                            &mut EventContext {
                                tx,
                                effects: &mut after,
                                control: &mut group,
                                cancelable: event.cancelable(),
                                scope: *scope,
                                tasks: tasks.as_mut().expect("event task draft"),
                                wheel_remaining: residual.as_mut(),
                                wheel_anchor: Some(&mut wheel_anchor),
                                current_target: event.current_target,
                            },
                        )?;
                        delivered += 1;
                    }
                    if let Some(delta) = residual {
                        for (i, wheel) in effects.wheel_scrolls.iter().enumerate() {
                            if wheel_matches(event, wheel) {
                                residual_wheels.insert(i, (delta, wheel_anchor));
                            }
                        }
                    }
                    if group.default_prevented {
                        prevented += 1;
                        match event.kind {
                            UiInputEventKind::Wheel => {
                                for (i, wheel) in effects.wheel_scrolls.iter().enumerate() {
                                    if wheel_matches(event, wheel) {
                                        prevented_wheels.insert(i);
                                    }
                                }
                            }
                            UiInputEventKind::Focus => {
                                defaults.borrow_mut().focus_reveals.retain(|request| {
                                    !(request.scene == event.scene
                                        && request.target == event.target
                                        && request.route == event.route)
                                });
                            }
                            UiInputEventKind::PointerUp => suppress_activation = true,
                            _ => {}
                        }
                    }
                }
                defaults.borrow_mut().wheel_scrolls = effects
                    .wheel_scrolls
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !prevented_wheels.contains(i))
                    .map(|(i, event)| {
                        let mut event = event.clone();
                        if let Some((delta, anchor)) = residual_wheels.get(&i) {
                            event.delta = *delta;
                            if let Some(anchor) = anchor {
                                let index = event
                                    .route
                                    .iter()
                                    .position(|id| id == anchor)
                                    .expect("handler on routed ancestry");
                                event.target = Some(*anchor);
                                event.route = event.route[index..].into();
                            }
                        }
                        event
                    })
                    .collect();
                Ok(())
            },
            |(), draft| {
                tasks
                    .borrow_mut()
                    .take()
                    .expect("event task draft")
                    .stage(draft);
                draft.stage_style_feedback(&feedback)
            },
            Some((constraints, viewport)),
            None::<fn(&mut UiTree) -> Result<(), Error>>,
            Some(ScrollDefaults {
                effects: &defaults,
                outcomes: &outcomes,
                skip_stale: true,
                adopted: Some(&on_adopt),
            }),
        )?;
        let mut consumed = outcomes.into_inner().into_iter();
        let outcomes = effects
            .wheel_scrolls
            .iter()
            .enumerate()
            .map(|(i, wheel)| {
                if prevented_wheels.contains(&i) {
                    UiScrollOutcome {
                        scene: wheel.scene,
                        target: wheel.target,
                        consumed: [0, 1].map(|axis| {
                            wheel.delta[axis]
                                - residual_wheels
                                    .get(&i)
                                    .map(|(delta, _)| delta)
                                    .unwrap_or(&wheel.delta)[axis]
                        }),
                        remaining: residual_wheels
                            .get(&i)
                            .map(|(delta, _)| *delta)
                            .unwrap_or(wheel.delta),
                    }
                } else {
                    let mut outcome = consumed.next().expect("one validated wheel outcome");
                    outcome.scene = wheel.scene;
                    outcome.target = wheel.target;
                    outcome.consumed =
                        [0, 1].map(|axis| wheel.delta[axis] - outcome.remaining[axis]);
                    outcome
                }
            })
            .collect();
        let frame = self
            .published_local_frame()
            .expect("complete input candidate")
            .clone();
        // Callback business actions run after full candidate adoption, never on
        // failed source/producer/default/frame validation.
        for effect in after {
            effect();
        }
        Ok(LogicalInputCommit {
            commit: commit.result,
            delivered,
            ignored,
            stopped,
            prevented,
            outcomes,
            frame,
        })
    }
}

#[cfg(test)]
mod tests;
