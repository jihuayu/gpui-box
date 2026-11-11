//! UI-owner signals and typed property sinks. Only the outer transaction publishes.
use super::*;
use std::{any::Any, collections::BTreeSet, marker::PhantomData, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SourceId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// A signal belongs to one runtime and one lifetime of a source slot.
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReactiveStats {
    pub transactions: u64,
    pub source_writes: u64,
    pub equal_writes: u64,
    pub changed_sources: u64,
    pub sink_writes: u64,
    pub frame_requests: u64,
}

pub struct ReactiveCommit<R> {
    pub value: R,
    pub commit: Commit,
    pub changed_sources: usize,
    pub sink_writes: usize,
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
    capabilities: BackendCapabilities,
    stats: ReactiveStats,
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
            capabilities,
            stats: ReactiveStats::default(),
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
        let slot = self.free.pop().unwrap_or(self.sources.len());
        if slot == self.sources.len() {
            self.sources.push(SourceSlot {
                generation: 0,
                source: None,
            });
        }
        let entry = &mut self.sources[slot];
        entry.source = Some(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
        });
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
        let bindings: Vec<_> = self.source(source)?.bindings.iter().copied().collect();
        for binding in bindings {
            self.unbind(binding)?;
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
        for source in sources {
            source.dispose(self);
        }
    }

    pub(super) fn reorder(&mut self, node: NodeId, children: Vec<NodeId>) -> Result<Commit, Error> {
        let mut tx = Transaction::default();
        tx.reorder(node, children);
        self.tree.commit(tx, self.capabilities)
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
        };
        let value = update(&mut tx)?;
        let ReactiveTransaction {
            values,
            writes,
            equal,
            ..
        } = tx;
        let mut properties = Transaction::default();
        let mut sink_writes = 0;
        for (id, value) in &values {
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
        Ok(ReactiveCommit {
            value,
            commit,
            changed_sources,
            sink_writes,
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
}

pub(super) trait ScopedSource {
    fn as_any(&self) -> &dyn Any;
    fn dispose(&self, ui: &mut ReactiveUi);
}
impl<T: 'static> ScopedSource for Signal<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn dispose(&self, ui: &mut ReactiveUi) {
        let _ = ui.dispose_signal(*self);
    }
}
impl ReactiveTransaction<'_> {
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
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
        self.writes += 1;
        if *self.read(signal)? == value {
            self.equal += 1;
            return Ok(false);
        }
        let value: Rc<dyn Value> = Rc::new(value);
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
        let checkpoint = (self.values.clone(), self.writes, self.equal);
        match update(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                (self.values, self.writes, self.equal) = checkpoint;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (ReactiveUi, NodeId) {
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
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 2));
        assert_eq!(
            before.items[1].commands[0],
            DisplayCommand::Fill(0x0000ffff)
        );
        assert_eq!(after.items[1].commands[0], DisplayCommand::Fill(0xaabbccff));
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
