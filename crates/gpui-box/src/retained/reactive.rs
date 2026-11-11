//! UI-owner signals and typed property sinks. Only the outer transaction publishes.
use super::*;
use std::{any::Any, cell::Cell, collections::BTreeSet, marker::PhantomData, rc::Rc};
mod computed;
pub use computed::{ComputedInputs, Dependency};
mod effects;
pub use effects::{EffectCleanup, EffectId, EffectInputs};
mod delivery;
pub use delivery::{UpdateBatch, UpdateQueueStats, UpdateReceiver, UpdateSender};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// A signal belongs to one runtime and one lifetime of a source slot. Computed
/// signals share typed read/bind APIs, but `set` returns `ReadonlySignal`.
#[derive(Debug, PartialEq, Eq)]
pub struct Signal<T> {
    id: SourceId,
    marker: PhantomData<fn() -> T>,
}
impl<T> Copy for Signal<T> {}
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BindingId {
    owner: u64,
    serial: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Color,
    Style,
    Translation,
    Clip,
    Scroll,
    Opacity,
    Measure,
    Decoration,
    Image,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    Text,
}

/// Typed property endpoint. A render property has at most one signal writer.
pub struct Property<T> {
    node: NodeId,
    field: Field,
    write: fn(NodeId, &T, &mut Transaction),
}
impl Property<u32> {
    pub fn color(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Color,
            write: |node, value, tx| tx.color(node, *value),
        }
    }
}
impl Property<Style> {
    pub fn style(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Style,
            write: |node, value, tx| tx.style(node, value.clone()),
        }
    }
}
impl Property<[f32; 2]> {
    pub fn translation(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Translation,
            write: |node, value, tx| tx.translation(node, *value),
        }
    }
    pub fn scroll(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Scroll,
            write: |node, value, tx| tx.scroll(node, *value),
        }
    }
}
impl Property<Option<Rect>> {
    pub fn clip(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Clip,
            write: |node, value, tx| tx.clip(node, *value),
        }
    }
}
impl Property<f32> {
    pub fn opacity(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Opacity,
            write: |node, value, tx| tx.opacity(node, *value),
        }
    }
}
impl Property<Option<MeasureSnapshot>> {
    pub fn measure(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Measure,
            write: |node, value, tx| tx.measure(node, value.clone()),
        }
    }
}
impl Property<QuadDecoration> {
    pub fn decoration(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Decoration,
            write: |node, value, tx| tx.decoration(node, *value),
        }
    }
}
impl Property<Option<ImageResource>> {
    pub fn image(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Image,
            write: |node, value, tx| tx.image(node, value.clone()),
        }
    }
}
#[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
impl Property<Option<text::TextPaint>> {
    pub fn text(node: NodeId) -> Self {
        Self {
            node,
            field: Field::Text,
            write: |node, value, tx| tx.text(node, value.clone()),
        }
    }
}

trait Value {
    fn as_any(&self) -> &dyn Any;
    fn equal(&self, other: &dyn Value) -> bool;
}
impl<T: PartialEq + 'static> Value for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn equal(&self, other: &dyn Value) -> bool {
        other
            .as_any()
            .downcast_ref::<T>()
            .is_some_and(|other| self == other)
    }
}

struct Source {
    value: Rc<dyn Value>,
    bindings: BTreeSet<BindingId>,
    dependents: BTreeSet<SourceId>,
    effects: BTreeSet<EffectId>,
    computation: Option<computed::Computation>,
    rank: u64,
    scheduled_epoch: Cell<u64>,
    delivery_lifetime: Option<Arc<()>>,
}
struct SourceSlot {
    generation: u64,
    source: Option<Source>,
}
type SinkWrite = dyn Fn(&dyn Value, &mut Transaction);
struct Binding {
    source: SourceId,
    node: NodeId,
    field: Field,
    write: Box<SinkWrite>,
}

/// Counters describe successfully published outer transactions, excluding setup.
/// Derived evaluations include successful read-triggered settlements, excluding
/// failed propagation attempts and rolled-back nested scopes. Changed sources
/// include both writable and derived final values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReactiveStats {
    pub transactions: u64,
    pub source_writes: u64,
    pub equal_writes: u64,
    pub changed_sources: u64,
    pub sink_writes: u64,
    pub frame_requests: u64,
    pub computed_evaluations: u64,
    pub computed_equal: u64,
    pub effect_runs: u64,
    pub effect_cleanups: u64,
}

pub struct ReactiveCommit<R> {
    pub value: R,
    pub commit: Commit,
    pub changed_sources: usize,
    pub sink_writes: usize,
    pub computed_evaluations: u64,
    pub computed_equal: u64,
    pub effect_runs: usize,
    pub effect_cleanups: usize,
}

/// Signals, sinks and render nodes have one UI-thread owner. A transaction is a
/// synchronous event boundary; callers notify the host once when request_frame
/// is true. There is no timer or per-write frame callback.
pub struct ReactiveUi {
    tree: UiTree,
    identity: u64,
    sources: Vec<SourceSlot>,
    free: Vec<usize>,
    bindings: BTreeMap<BindingId, Binding>,
    endpoints: BTreeMap<(NodeId, Field), BindingId>,
    next_binding: u64,
    effects: BTreeMap<EffectId, effects::Effect>,
    next_effect: u64,
    schedule_epoch: Cell<u64>,
    capabilities: BackendCapabilities,
    stats: ReactiveStats,
    delivery: Option<std::sync::Weak<delivery::Shared>>,
}

impl ReactiveUi {
    pub fn new(tree: UiTree, capabilities: BackendCapabilities) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        Ok(Self {
            tree,
            identity,
            sources: vec![],
            free: vec![],
            bindings: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            next_binding: 0,
            effects: BTreeMap::new(),
            next_effect: 0,
            schedule_epoch: Cell::new(0),
            capabilities,
            stats: ReactiveStats::default(),
            delivery: None,
        })
    }

    pub fn tree(&self) -> &UiTree {
        &self.tree
    }
    pub fn root(&self) -> NodeId {
        self.tree.root()
    }
    pub fn published_frame(&self) -> Option<&FramePacket> {
        self.tree.published_frame()
    }
    pub fn frame(&mut self, viewport: [f32; 2]) -> Result<FramePacket, Error> {
        self.tree.frame(viewport)
    }
    pub fn stats(&self) -> ReactiveStats {
        self.stats
    }
    /// Set the capabilities of the actual packet consumer for subsequent commits.
    pub fn set_capabilities(&mut self, capabilities: BackendCapabilities) {
        self.capabilities = capabilities;
    }
    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }
    pub fn source_count(&self) -> usize {
        self.sources
            .iter()
            .filter(|slot| slot.source.is_some())
            .count()
    }

    fn source(&self, id: SourceId) -> Result<&Source, Error> {
        if id.owner != self.identity {
            return Err(Error::InvalidSignal);
        }
        self.sources
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.source.as_ref())
            .ok_or(Error::InvalidSignal)
    }

    pub fn signal<T: PartialEq + 'static>(&mut self, value: T) -> Signal<T> {
        self.insert_source(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
            dependents: BTreeSet::new(),
            effects: BTreeSet::new(),
            computation: None,
            rank: 0,
            scheduled_epoch: Cell::new(0),
            delivery_lifetime: None,
        })
    }

    fn insert_source<T>(&mut self, source: Source) -> Signal<T> {
        let slot = self.free.pop().unwrap_or(self.sources.len());
        if slot == self.sources.len() {
            self.sources.push(SourceSlot {
                generation: 0,
                source: None,
            });
        }
        let entry = &mut self.sources[slot];
        entry.source = Some(source);
        Signal {
            id: SourceId {
                owner: self.identity,
                slot,
                generation: entry.generation,
            },
            marker: PhantomData,
        }
    }

    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        self.source(signal.id)?
            .value
            .as_any()
            .downcast_ref::<T>()
            .ok_or(Error::InvalidSignal)
    }

    /// Validate and synchronize the endpoint before publishing the binding.
    pub fn bind<T: 'static>(
        &mut self,
        signal: Signal<T>,
        property: Property<T>,
    ) -> Result<(BindingId, Commit), Error> {
        let value = self.read(signal)?;
        self.tree.node(property.node)?;
        if self
            .endpoints
            .contains_key(&(property.node, property.field))
        {
            return Err(Error::BindingConflict);
        }
        let serial = self
            .next_binding
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let mut tx = Transaction::default();
        (property.write)(property.node, value, &mut tx);
        let commit = self.tree.commit(tx, self.capabilities)?;
        let id = BindingId {
            owner: self.identity,
            serial,
        };
        let binding = Binding {
            source: signal.id,
            node: property.node,
            field: property.field,
            write: Box::new(move |value, tx| {
                (property.write)(
                    property.node,
                    value
                        .as_any()
                        .downcast_ref::<T>()
                        .expect("typed signal sink"),
                    tx,
                );
            }),
        };
        self.sources[signal.id.slot]
            .source
            .as_mut()
            .unwrap()
            .bindings
            .insert(id);
        self.endpoints.insert((binding.node, binding.field), id);
        self.bindings.insert(id, binding);
        self.next_binding = serial;
        Ok((id, commit))
    }

    pub fn unbind(&mut self, id: BindingId) -> Result<(), Error> {
        let binding = self.bindings.remove(&id).ok_or(Error::InvalidBinding)?;
        self.endpoints.remove(&(binding.node, binding.field));
        self.sources[binding.source.slot]
            .source
            .as_mut()
            .unwrap()
            .bindings
            .remove(&id);
        Ok(())
    }

    pub fn dispose_signal<T>(&mut self, signal: Signal<T>) -> Result<(), Error> {
        self.dispose_source(signal.id)
    }

    fn dispose_source(&mut self, source: SourceId) -> Result<(), Error> {
        if !self.source(source)?.dependents.is_empty() || !self.source(source)?.effects.is_empty() {
            return Err(Error::SignalInUse);
        }
        let dependencies = self
            .source(source)?
            .computation
            .as_ref()
            .map(|c| c.dependencies.clone());
        let bindings: Vec<_> = self.source(source)?.bindings.iter().copied().collect();
        for binding in bindings {
            self.unbind(binding)?;
        }
        if let Some(dependencies) = dependencies {
            for id in dependencies {
                self.sources[id.slot]
                    .source
                    .as_mut()
                    .unwrap()
                    .dependents
                    .remove(&source);
            }
        }
        let slot = &mut self.sources[source.slot];
        slot.source = None;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(source.slot);
        }
        Ok(())
    }

    pub fn append(&mut self, parent: NodeId, style: Style, color: u32) -> Result<NodeId, Error> {
        self.tree.append(parent, style, color)
    }

    pub(super) fn dispose_scope_sources(&mut self, sources: Vec<Box<dyn ScopedSource>>) {
        let mut ids: Vec<_> = sources.iter().map(|source| source.dependency().0).collect();
        ids.sort_by_key(|id| std::cmp::Reverse(self.source(*id).unwrap().rank));
        for id in ids {
            self.dispose_source(id)
                .expect("prevalidated scope source disposal");
        }
    }

    pub fn commit_structure(&mut self, tx: StructureTransaction) -> Result<Commit, Error> {
        self.tree.commit_structure(tx)
    }

    /// Retire every endpoint in a removed subtree, including invisible descendants.
    pub fn remove(&mut self, node: NodeId) -> Result<(), Error> {
        self.tree.remove(node)?;
        let dead: Vec<_> = self
            .bindings
            .iter()
            .filter(|(_, binding)| self.tree.node(binding.node).is_err())
            .map(|(id, _)| *id)
            .collect();
        for id in dead {
            self.unbind(id)?;
        }
        Ok(())
    }

    /// Collect final source values, validate the complete sink batch and publish
    /// both together. Errors (including an invalid property) publish neither.
    pub fn transaction<R>(
        &mut self,
        update: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<R, Error>,
    ) -> Result<ReactiveCommit<R>, Error> {
        let mut tx = ReactiveTransaction {
            runtime: self,
            values: BTreeMap::new(),
            writes: 0,
            equal: 0,
            dirty_roots: BTreeMap::new(),
            computed_evaluations: 0,
            computed_equal: 0,
        };
        let value = update(&mut tx)?;
        tx.settle()?;
        let ReactiveTransaction {
            values,
            writes,
            equal,
            computed_evaluations,
            computed_equal,
            ..
        } = tx;
        let mut properties = Transaction::default();
        let mut sink_writes = 0;
        let mut effects = BTreeSet::new();
        for (id, value) in &values {
            effects.extend(&self.source(*id)?.effects);
            for binding in &self.source(*id)?.bindings {
                (self.bindings[binding].write)(value.as_ref(), &mut properties);
                sink_writes += 1;
            }
        }
        let commit = self.tree.commit(properties, self.capabilities)?;
        let changed_sources = values.len();
        for (id, value) in values {
            self.sources[id.slot].source.as_mut().unwrap().value = value;
        }
        self.stats.transactions += 1;
        self.stats.source_writes += writes;
        self.stats.equal_writes += equal;
        self.stats.changed_sources += changed_sources as u64;
        self.stats.sink_writes += sink_writes as u64;
        self.stats.frame_requests += u64::from(commit.request_frame);
        self.stats.computed_evaluations += computed_evaluations;
        self.stats.computed_equal += computed_equal;
        let mut effect_cleanups = 0;
        for id in &effects {
            effect_cleanups += usize::from(self.run_effect(*id));
        }
        self.stats.effect_runs += effects.len() as u64;
        self.stats.effect_cleanups += effect_cleanups as u64;
        Ok(ReactiveCommit {
            value,
            commit,
            changed_sources,
            sink_writes,
            computed_evaluations,
            computed_equal,
            effect_runs: effects.len(),
            effect_cleanups,
        })
    }
}

/// Nested transactions see parent writes. A failed inner scope rolls back only
/// its writes, so the outer scope may handle the error and continue.
pub struct ReactiveTransaction<'a> {
    runtime: &'a ReactiveUi,
    values: BTreeMap<SourceId, Rc<dyn Value>>,
    writes: u64,
    equal: u64,
    dirty_roots: BTreeMap<SourceId, Rc<dyn Value>>,
    computed_evaluations: u64,
    computed_equal: u64,
}

pub(super) trait ScopedSource {
    fn as_any(&self) -> &dyn Any;
    fn dependency(&self) -> Dependency;
}
impl<T: 'static> ScopedSource for Signal<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn dependency(&self) -> Dependency {
        (*self).dependency()
    }
}
impl ReactiveTransaction<'_> {
    /// Reads of a derived source settle all pending graph work at this batch
    /// boundary. Further writes may trigger a new settlement; sinks publish once.
    pub fn read<T: 'static>(&mut self, signal: Signal<T>) -> Result<&T, Error> {
        let source = self.runtime.source(signal.id)?;
        if source.computation.is_some() {
            self.settle()?;
        }
        let source = self.runtime.source(signal.id)?;
        self.values
            .get(&signal.id)
            .unwrap_or(&source.value)
            .as_any()
            .downcast_ref::<T>()
            .ok_or(Error::InvalidSignal)
    }

    /// Returns false on source equality, before looking at any subscriber.
    pub fn set<T: PartialEq + 'static>(
        &mut self,
        signal: Signal<T>,
        value: T,
    ) -> Result<bool, Error> {
        let source = self.runtime.source(signal.id)?;
        if source.computation.is_some() {
            return Err(Error::ReadonlySignal);
        }
        self.writes += 1;
        let current = self.values.get(&signal.id).unwrap_or(&source.value);
        if current.equal(&value) {
            self.equal += 1;
            return Ok(false);
        }
        let value: Rc<dyn Value> = Rc::new(value);
        if !source.dependents.is_empty() {
            let baseline = self
                .dirty_roots
                .entry(signal.id)
                .or_insert_with(|| current.clone());
            if baseline.equal(value.as_ref()) {
                self.dirty_roots.remove(&signal.id);
            }
        }
        if source.value.equal(value.as_ref()) {
            self.values.remove(&signal.id);
        } else {
            self.values.insert(signal.id, value);
        }
        Ok(true)
    }

    pub fn transaction<R>(
        &mut self,
        update: impl FnOnce(&mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let checkpoint = (
            self.values.clone(),
            self.dirty_roots.clone(),
            self.writes,
            self.equal,
            self.computed_evaluations,
            self.computed_equal,
        );
        match update(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                (
                    self.values,
                    self.dirty_roots,
                    self.writes,
                    self.equal,
                    self.computed_evaluations,
                    self.computed_equal,
                ) = checkpoint;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> (ReactiveUi, NodeId) {
        let mut tree = UiTree::new(Style::default(), 0xff0000ff).unwrap();
        let child = tree
            .append(tree.root(), Style::default(), 0x0000ffff)
            .unwrap();
        (
            ReactiveUi::new(
                tree,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )
            .unwrap(),
            child,
        )
    }

    #[test]
    fn source_equality_and_final_restore_do_not_dispatch_sinks_or_request_frames() {
        let (mut ui, child) = fixture();
        let color = ui.signal(0x0000ffff);
        assert!(
            !ui.bind(color, Property::color(child))
                .unwrap()
                .1
                .request_frame
        );
        let before = ui.frame([100.0, 100.0]).unwrap();
        let result = ui
            .transaction(|tx| {
                assert!(!tx.set(color, 0x0000ffff)?);
                tx.set(color, 0x00ff00ff)?;
                tx.transaction(|tx| {
                    tx.set(color, 0x0000ffff)?;
                    Ok(())
                })?;
                Ok(())
            })
            .unwrap();
        assert_eq!((result.changed_sources, result.sink_writes), (0, 0));
        assert!(!result.commit.request_frame);
        let after = ui.frame([100.0, 100.0]).unwrap();
        assert_eq!(before.revision, after.revision);
        assert!(Arc::ptr_eq(&before.items, &after.items));
        assert_eq!(
            ui.stats(),
            ReactiveStats {
                transactions: 1,
                source_writes: 3,
                equal_writes: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn nested_savepoint_rollback_and_outer_failure_are_atomic() {
        let (mut ui, child) = fixture();
        let color = ui.signal(0x0000ffff);
        let opacity = ui.signal(1.0);
        ui.bind(color, Property::color(child)).unwrap();
        ui.bind(opacity, Property::opacity(child)).unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        let result = ui
            .transaction(|tx| {
                tx.set(color, 0x00ff00ff)?;
                let failed: Result<(), Error> = tx.transaction(|tx| {
                    tx.set(color, 0)?;
                    tx.set(opacity, 0.5)?;
                    Err(Error::InvalidChildren)
                });
                assert_eq!(failed, Err(Error::InvalidChildren));
                assert_eq!(*tx.read(color)?, 0x00ff00ff);
                assert_eq!(*tx.read(opacity)?, 1.0);
                tx.transaction(|tx| {
                    tx.set(opacity, 0.75)?;
                    Ok(())
                })
            })
            .unwrap();
        assert_eq!((result.changed_sources, result.sink_writes), (2, 2));
        assert_eq!(result.commit.revision, before.revision + 1);
        assert_eq!(ui.stats().frame_requests, 1);
        let committed = ui.frame([100.0, 100.0]).unwrap();
        let failed = ui.transaction(|tx| {
            tx.set(color, 0)?;
            tx.set(opacity, f32::NAN)?;
            Ok(())
        });
        assert!(matches!(failed, Err(Error::InvalidOpacity)));
        assert_eq!(*ui.read(color).unwrap(), 0x00ff00ff);
        assert_eq!(*ui.read(opacity).unwrap(), 0.75);
        assert_eq!(
            ui.frame([100.0, 100.0]).unwrap().revision,
            committed.revision
        );
        assert_eq!(ui.stats().transactions, 1);
    }

    #[test]
    fn binding_conflict_disposal_and_subtree_removal_do_not_revive_sinks() {
        let (mut ui, child) = fixture();
        let nested = ui.append(child, Style::default(), 0).unwrap();
        let color = ui.signal(0x0000ffff);
        let other = ui.signal(0u32);
        let (binding, _) = ui.bind(color, Property::color(child)).unwrap();
        ui.bind(color, Property::color(nested)).unwrap();
        assert!(matches!(
            ui.bind(other, Property::color(child)),
            Err(Error::BindingConflict)
        ));
        ui.remove(child).unwrap();
        assert_eq!(ui.binding_count(), 0);
        assert_eq!(ui.unbind(binding), Err(Error::InvalidBinding));
        let replacement = ui.append(ui.root(), Style::default(), 0x112233ff).unwrap();
        let result = ui
            .transaction(|tx| {
                tx.set(color, 0xffffffff)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(result.sink_writes, 0);
        assert!(!result.commit.request_frame);
        let frame = ui.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            frame
                .items
                .iter()
                .find(|item| item.node == replacement)
                .unwrap()
                .commands[0],
            DisplayCommand::Fill(0x112233ff)
        );
        ui.dispose_signal(color).unwrap();
        let fresh = ui.signal(42u32);
        assert_eq!(ui.read(color), Err(Error::InvalidSignal));
        assert_eq!(*ui.read(fresh).unwrap(), 42);
        let (foreign, _) = fixture();
        assert_eq!(foreign.read(fresh), Err(Error::InvalidSignal));
        assert_eq!(ui.source_count(), 2);
    }

    #[test]
    fn one_source_fans_out_once_per_final_value_and_uses_one_render_commit() {
        let (mut ui, child) = fixture();
        let other = ui.append(ui.root(), Style::default(), 0x0000ffff).unwrap();
        let color = ui.signal(0x0000ffff);
        ui.bind(color, Property::color(child)).unwrap();
        ui.bind(color, Property::color(other)).unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        let result = ui
            .transaction(|tx| {
                tx.set(color, 0x00ff00ff)?;
                tx.transaction(|tx| {
                    tx.set(color, 0xaabbccff)?;
                    Ok(())
                })?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            (
                result.changed_sources,
                result.sink_writes,
                result.commit.changed_nodes
            ),
            (1, 2, 2)
        );
        assert_eq!(result.commit.revision, before.revision + 1);
        assert_eq!(
            result.commit.routes,
            vec![
                (child, UpdatePath::InstancePatch),
                (other, UpdatePath::InstancePatch)
            ]
        );
        let after = ui.frame([100.0, 100.0]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
        assert_eq!(
            before.items[1].commands[0],
            DisplayCommand::Fill(0x0000ffff)
        );
        assert_eq!(after.items[1].fill_color, 0xaabbccff);
        assert!(Arc::ptr_eq(
            &before.items[1].commands,
            &after.items[1].commands
        ));
    }

    #[test]
    fn typed_layout_clip_translation_sinks_and_unbinding_use_the_same_tree() {
        let (mut ui, child) = fixture();
        let style = ui.signal(ui.tree().style(child).unwrap().clone());
        let clip = ui.signal(None);
        let translation = ui.signal([0.0; 2]);
        ui.bind(style, Property::style(child)).unwrap();
        ui.bind(clip, Property::clip(child)).unwrap();
        let (binding, _) = ui.bind(translation, Property::translation(child)).unwrap();
        ui.frame([100.0, 100.0]).unwrap();
        ui.transaction(|tx| {
            let mut next = tx.read(style)?.clone();
            next.size = Size {
                width: length(40.0),
                height: length(32.0),
            };
            tx.set(style, next)?;
            tx.set(translation, [5.0, 6.0])?;
            tx.set(
                clip,
                Some(Rect {
                    x: 4.0,
                    y: 3.0,
                    width: 16.0,
                    height: 10.0,
                }),
            )?;
            Ok(())
        })
        .unwrap();
        let frame = ui.frame([100.0, 100.0]).unwrap();
        assert_eq!(frame.layout_runs, 1);
        assert_eq!(
            frame.items[1].bounds,
            Rect {
                x: 5.0,
                y: 6.0,
                width: 40.0,
                height: 32.0
            }
        );
        assert_eq!(
            frame.items[1].visible_bounds(),
            Rect {
                x: 9.0,
                y: 9.0,
                width: 16.0,
                height: 10.0
            }
        );
        ui.unbind(binding).unwrap();
        assert!(
            !ui.transaction(|tx| {
                tx.set(translation, [10.0, 10.0])?;
                Ok(())
            })
            .unwrap()
            .commit
            .request_frame
        );
        assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, frame.revision);
        ui.bind(translation, Property::translation(child)).unwrap();
        assert_ne!(ui.frame([100.0, 100.0]).unwrap().revision, frame.revision);
    }
}
