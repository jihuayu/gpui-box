//! Explicit typed input drives one keyed UI task. Pure parameter capture happens
//! after producer settlement; only an adopted candidate may schedule its worker.
use super::*;

type Schedule<I, T> = dyn Fn(I, UiTaskContext<T>);
/// Stable factory identity. Clone for equal redeclaration. The callback runs
/// only after adoption and ordinary effects; it must schedule owned work on the
/// host executor. I is cloned on owner when the final source value changes.
///
/// ```
/// use gpui_box::retained::{BackendCapabilities, LayoutStyle};
/// use gpui_box::retained::logical::{LogicalUi, UiTaskContext, UiTaskEffect};
/// let mut ui = LogicalUi::new_layout(LayoutStyle::column(), 0,
///     BackendCapabilities::default()).unwrap();
/// let scope = ui.root_scope();
/// let query = ui.state(scope, 0, || 1u32).unwrap();
/// let effect = UiTaskEffect::new(|input, context: UiTaskContext<u32>| {
///     std::thread::spawn(move || { let _ = context.complete(input * 2); });
/// });
/// ui.task_effect(scope, 0, query, effect.clone()).unwrap();
/// ui.transaction(|tx| { tx.set(query, 2)?; tx.set(query, 3) }).unwrap();
/// // Only final input 3 replaces 1; repeated equal declaration keeps its task.
/// ui.task_effect(scope, 0, query, effect).unwrap();
/// // The host awaits ui.task::<u32>(scope, 0), then uses apply_task normally.
/// ```
pub struct UiTaskEffect<I, T>(Rc<Schedule<I, T>>);
impl<I, T> Clone for UiTaskEffect<I, T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<I, T> UiTaskEffect<I, T> {
    pub fn new(schedule: impl Fn(I, UiTaskContext<T>) + 'static) -> Self {
        Self(Rc::new(schedule))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTaskEffectStatus {
    /// Input is current, but no admitted worker exists. A host capacity wake or
    /// later owner edit may retry. Old input's task has been cancelled.
    WaitingForCapacity,
    Task(UiTaskStatus),
}
trait Watch {
    fn dependency(&self) -> Dependency;
    fn launch(
        &self,
        scope: ScopeId,
        key: u64,
        edit: &mut LogicalEdit<'_, '_, '_>,
    ) -> Result<Launch, Error>;
}
struct Typed<I, T> {
    input: Signal<I>,
    effect: UiTaskEffect<I, T>,
}
impl<I: Clone + PartialEq + 'static, T: Send + Sync + 'static> Watch for Typed<I, T> {
    fn dependency(&self) -> Dependency {
        self.input.dependency()
    }
    fn launch(
        &self,
        scope: ScopeId,
        key: u64,
        edit: &mut LogicalEdit<'_, '_, '_>,
    ) -> Result<Launch, Error> {
        let input = edit.render.read(self.input)?.clone();
        let effect = self.effect.clone();
        let (_, launch) = edit
            .tasks
            .get_or_insert_with(|| edit.live_tasks.clone())
            .start(scope, key, move |context| (effect.0)(input, context))?;
        Ok(launch)
    }
}
#[derive(Clone)]
pub(super) struct Registration {
    watch: Rc<dyn Watch>,
    identity: Rc<dyn Any>,
    observed: Option<reactive::SourceObservation>,
    waiting: bool,
}
impl Registry {
    pub(in crate::retained::logical) fn has_effects(&self) -> bool {
        !self.watches.is_empty()
    }
    fn effect_status(&self, scope: ScopeId, key: u64) -> Result<UiTaskEffectStatus, Error> {
        let entry = self.watches.get(&(scope, key)).ok_or(Error::InvalidTask)?;
        if entry.waiting {
            return Ok(UiTaskEffectStatus::WaitingForCapacity);
        }
        self.entries
            .get(&(scope, key))
            .map(|task| UiTaskEffectStatus::Task(task.status()))
            .ok_or(Error::InvalidTask)
    }
}
impl LogicalEdit<'_, '_, '_> {
    /// Declare one reactive UI task from an explicit ordinary Signal input.
    /// The effect exclusively owns its task key; manual start/cancel rejects.
    pub fn task_effect<I: Clone + PartialEq + 'static, T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: Signal<I>,
        effect: UiTaskEffect<I, T>,
    ) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        self.render.validate_live_dependency(input.dependency())?;
        let registry = self.tasks.as_ref().unwrap_or(self.live_tasks);
        if let Some(old) = registry.watches.get(&(scope, key)) {
            if old.watch.dependency() == input.dependency()
                && old
                    .identity
                    .downcast_ref::<UiTaskEffect<I, T>>()
                    .is_some_and(|old| Rc::ptr_eq(&old.0, &effect.0))
            {
                attempt.success = true;
                return Ok(());
            }
        } else if registry.entries.contains_key(&(scope, key)) {
            return Err(Error::InvalidTask);
        }
        let identity = Rc::new(effect.clone());
        Rc::make_mut(
            &mut self
                .tasks
                .get_or_insert_with(|| self.live_tasks.clone())
                .watches,
        )
        .insert(
            (scope, key),
            Registration {
                watch: Rc::new(Typed { input, effect }),
                identity,
                observed: None,
                waiting: false,
            },
        );
        attempt.success = true;
        Ok(())
    }
    pub fn task_effect_prop<I: Clone + PartialEq + 'static, T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: ReadProp<I>,
        effect: UiTaskEffect<I, T>,
    ) -> Result<(), Error> {
        self.task_effect(scope, key, input.signal, effect)
    }
    /// Remove the reactive declaration and cancel its current task on adoption.
    pub fn remove_task_effect(&mut self, scope: ScopeId, key: u64) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(scope)?;
        let registry = self.tasks.get_or_insert_with(|| self.live_tasks.clone());
        if Rc::make_mut(&mut registry.watches)
            .remove(&(scope, key))
            .is_none()
        {
            return Err(Error::InvalidTask);
        }
        registry.cancel(scope, key);
        attempt.success = true;
        Ok(())
    }
    pub(in crate::retained::logical) fn settle_task_effects(&mut self) -> Result<(), Error> {
        let registry = self.tasks.as_ref().unwrap_or(self.live_tasks);
        if registry.watches.is_empty() {
            return Ok(());
        }
        let dead = self.retired_scopes()?;
        let keys: Vec<_> = registry
            .watches
            .keys()
            .filter(|(scope, _)| !dead.contains(scope))
            .copied()
            .collect();
        for (scope, key) in keys {
            let registration =
                self.tasks.as_ref().unwrap_or(self.live_tasks).watches[&(scope, key)].clone();
            let unchanged = registration
                .observed
                .as_ref()
                .map(|sources| self.render.matches_sources(sources))
                .transpose()?
                .unwrap_or(false);
            if unchanged && !registration.waiting {
                continue;
            }
            let mut sources = reactive::SourceObservation::default();
            // Record the typed source through the same reactive engine used by
            // layout producers, without a second equality definition.
            self.render
                .record_dependency(&mut sources, registration.watch.dependency())?;
            self.tasks
                .get_or_insert_with(|| self.live_tasks.clone())
                .cancel(scope, key);
            let waiting = match registration.watch.launch(scope, key, self) {
                Ok(launch) => {
                    self.task_launches.push(launch);
                    false
                }
                Err(Error::TaskCapacity) => true,
                Err(error) => return Err(error),
            };
            let registry = self.tasks.as_mut().expect("effect task draft");
            let entry = Rc::make_mut(&mut registry.watches)
                .get_mut(&(scope, key))
                .unwrap();
            entry.observed = Some(sources);
            entry.waiting = waiting;
        }
        Ok(())
    }
    pub(in crate::retained::logical) fn validate_task_effects(&self) -> Result<(), Error> {
        for registration in self
            .tasks
            .as_ref()
            .unwrap_or(self.live_tasks)
            .watches
            .values()
        {
            self.render
                .validate_live_dependency(registration.watch.dependency())?;
            if let Some(sources) = &registration.observed
                && !self.render.matches_sources(sources)?
            {
                return Err(Error::InvalidTask);
            }
        }
        Ok(())
    }
}
impl LogicalUi {
    pub fn task_effect<I: Clone + PartialEq + 'static, T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: Signal<I>,
        effect: UiTaskEffect<I, T>,
    ) -> Result<(), Error> {
        self.edit(
            |_| Ok(()),
            |(), edit| edit.task_effect(scope, key, input, effect),
        )
        .map(|_| ())
    }
    pub fn task_effect_prop<I: Clone + PartialEq + 'static, T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: ReadProp<I>,
        effect: UiTaskEffect<I, T>,
    ) -> Result<(), Error> {
        self.task_effect(scope, key, input.signal, effect)
    }
    pub fn remove_task_effect(&mut self, scope: ScopeId, key: u64) -> Result<(), Error> {
        self.edit(|_| Ok(()), |(), edit| edit.remove_task_effect(scope, key))
            .map(|_| ())
    }
    pub fn task_effect_status(
        &self,
        scope: ScopeId,
        key: u64,
    ) -> Result<UiTaskEffectStatus, Error> {
        self.scope(scope)?;
        self.tasks.effect_status(scope, key)
    }
    /// Retry waiting current inputs after an explicit host capacity notification.
    /// Ordinary logical edits and frame production also give waiting inputs an
    /// opportunity. Applied/Abandoned/Cancelled terminal tasks do not restart;
    /// use a new factory (or an explicit input revision) for deliberate retries.
    pub fn retry_task_effects(&mut self) -> Result<ReactiveCommit<()>, Error> {
        self.edit(|_| Ok(()), |(), _| Ok(()))
            .map(|edit| edit.result)
    }
}
#[cfg(test)]
mod tests;
