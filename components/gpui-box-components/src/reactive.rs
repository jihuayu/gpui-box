//! Explicit Entity-backed state. Reads are tracked by GPUI's existing view machinery.
use gpui::{App, AppContext, Context, Entity, Subscription};
use std::{cell::Cell, rc::Rc};

pub struct Signal<T: 'static> {
    entity: Entity<State<T>>,
}
struct State<T> {
    value: T,
    revision: u64,
}
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            entity: self.entity.clone(),
        }
    }
}
impl<T: Clone + PartialEq + 'static> Signal<T> {
    pub fn new(value: T, cx: &mut App) -> Self {
        Self {
            entity: cx.new(|_| State { value, revision: 0 }),
        }
    }
    pub fn get(&self, cx: &App) -> T {
        self.entity.read(cx).value.clone()
    }
    pub fn revision(&self, cx: &App) -> u64 {
        self.entity.read(cx).revision
    }
    /// Equal values do not invalidate observers. GPUI batches notifications per update.
    pub fn set(&self, value: T, cx: &mut App) -> bool {
        self.entity.update(cx, |state, cx| {
            if state.value == value {
                return false;
            }
            state.value = value;
            state.revision += 1;
            cx.notify();
            true
        })
    }
    pub fn update(&self, update: impl FnOnce(&mut T), cx: &mut App) -> bool {
        let mut value = self.get(cx);
        update(&mut value);
        self.set(value, cx)
    }
}

/// A view/model owns this scope. Dropping it removes every retained subscription.
#[derive(Default)]
pub struct ReactiveScope {
    subscriptions: Vec<Subscription>,
    state: Rc<ScopeState>,
}
#[derive(Default)]
struct ScopeState {
    pending: Cell<bool>,
    epoch: Cell<u64>,
    notifications: Cell<u64>,
}
impl ReactiveScope {
    pub fn observe<T: Clone + PartialEq + 'static, V: 'static>(
        &mut self,
        signal: &Signal<T>,
        cx: &mut Context<V>,
    ) {
        let owner = cx.weak_entity();
        let state = Rc::downgrade(&self.state);
        self.subscriptions
            .push(cx.observe(&signal.entity, move |_, _, cx| {
                let Some(current) = state.upgrade() else {
                    return;
                };
                if current.pending.replace(true) {
                    return;
                }
                let epoch = current.epoch.get();
                let state = state.clone();
                let owner = owner.clone();
                cx.defer(move |cx| {
                    let Some(state) = state.upgrade() else {
                        return;
                    };
                    if state.epoch.get() != epoch {
                        return;
                    }
                    state.pending.set(false);
                    let _ = owner.update(cx, |_, cx| {
                        state.notifications.set(state.notifications.get() + 1);
                        cx.notify();
                    });
                });
            }));
    }
    pub fn notification_count(&self) -> u64 {
        self.state.notifications.get()
    }
    pub fn clear(&mut self) {
        self.subscriptions.clear();
        self.state.epoch.set(self.state.epoch.get() + 1);
        self.state.pending.set(false);
    }
}

/// An eager, equality-filtered projection. The source remains authoritative.
pub struct Memo<T: 'static> {
    signal: Signal<T>,
    _subscription: Subscription,
}
impl<T: Clone + PartialEq + 'static> Memo<T> {
    pub fn new<S: Clone + PartialEq + 'static>(
        source: &Signal<S>,
        project: impl Fn(&S) -> T + 'static,
        cx: &mut App,
    ) -> Self {
        let signal = Signal::new(project(&source.get(cx)), cx);
        let output = signal.clone();
        let subscription = cx.observe(&source.entity, move |entity, cx| {
            let value = project(&entity.read(cx).value);
            output.set(value, cx);
        });
        Self {
            signal,
            _subscription: subscription,
        }
    }
    pub fn signal(&self) -> Signal<T> {
        self.signal.clone()
    }
    pub fn get(&self, cx: &App) -> T {
        self.signal.get(cx)
    }
}

/// Commands mutate one aggregate. Selectors notify only when their projection changes.
pub struct Store<T: 'static>(Signal<T>);
impl<T: Clone + PartialEq + 'static> Store<T> {
    pub fn new(value: T, cx: &mut App) -> Self {
        Self(Signal::new(value, cx))
    }
    pub fn snapshot(&self, cx: &App) -> T {
        self.0.get(cx)
    }
    pub fn command(&self, command: impl FnOnce(&mut T), cx: &mut App) -> bool {
        self.0.update(command, cx)
    }
    pub fn select<U: Clone + PartialEq + 'static>(
        &self,
        project: impl Fn(&T) -> U + 'static,
        cx: &mut App,
    ) -> Memo<U> {
        Memo::new(&self.0, project, cx)
    }
    pub fn signal(&self) -> Signal<T> {
        self.0.clone()
    }
}

type Read<T> = Rc<dyn Fn(&App) -> T>;
type Write<T> = Rc<dyn Fn(T, &mut App)>;
/// Explicit committed-value binding. TextInput retains its own IME draft and selection.
#[derive(Clone)]
pub struct Binding<T> {
    read: Read<T>,
    write: Write<T>,
}
impl<T: Clone + PartialEq + 'static> Binding<T> {
    pub fn signal(signal: Signal<T>) -> Self {
        let source = signal.clone();
        Self {
            read: Rc::new(move |cx| source.get(cx)),
            write: Rc::new(move |value, cx| {
                signal.set(value, cx);
            }),
        }
    }
    pub fn get(&self, cx: &App) -> T {
        (self.read)(cx)
    }
    pub fn set(&self, value: T, cx: &mut App) {
        (self.write)(value, cx);
    }
}
