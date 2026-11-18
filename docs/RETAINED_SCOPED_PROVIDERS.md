# Scoped Store and Command providers

Status: experimental explicit Rust component/runtime API,2026-11-18. Complete
RFC remains active. Extends [components](RETAINED_TYPED_COMPONENTS.md),
[business Store inputs](RETAINED_BUSINESS_STORE.md) and
[typed Command transport](RETAINED_TYPED_COMMANDS.md). No global singleton or
second source graph, layout box, renderer algorithm or source schema change.

2026-11-19 follow-up: [typed complete theme groups](RETAINED_THEMES.md) reuse
the same registry with a separate ThemeKey namespace, logical nearest-provider
inheritance and ordinary read-only source projections. Transparent ThemeScope
does not add a layout box; portable token expressions/text inheritance remain open.

## Keys, declarations and resolution

`gpui_box::store::StoreKey` names one logical Store capability and associates its
owned Send+Sync State type. Different keys may use the same State type, without
confusing two documents/accounts. Command capabilities use the concrete Command
type; multiple independent channels for one business concept require distinct
command wrapper types/provider subtrees rather than an implicit channel choice.

```rust,ignore
struct DocumentStore;
impl StoreKey for DocumentStore { type State = Document; }

let mut input = ui.attach_store(app_scope, 10, &document.read_only())?;
ui.provide_store::<DocumentStore>(app_scope, input.prop())?;
ui.provide_command(app_scope, save_sender.clone())?;

impl Component for Editor {
    type Props = ();
    fn mount(_: ReadProp<()>, cx: &mut ComponentContext<'_, '_, '_, '_>)
        -> Result<(), Error>
    {
        let document = cx.use_store::<DocumentStore>()?;
        let save = cx.use_command::<SaveDocument>()?;
        let title = cx.select_store(0, document, |s| Ok(s.title.clone()))?;
        // Bind title, retain save for an owner-local event handler/effect.
        // Sending business work during speculative mount is not transactional.
        Ok(())
    }
}
```

LogicalUi provides declaration wrappers; LogicalEdit and ComponentContext provide
declarations plus `use_store/use_command` and optional `try_use_*`. Context always
uses its own component scope. Lookup starts at that logical scope and walks its
logical parents to the nearest declaration. Physical render parent/portal host
does not change provider ancestry. A local declaration shadows an ancestor for
new consumers, without creating render output or another Store subscription.

Store providers carry the **already captured UI ReadProp snapshot**, not a
ReadStore that samples independently during each component mount. All consumers
of that provider therefore share the same explicit StoreInput adoption boundary
and ordinary selectors. The host retains StoreInput and calls apply_store at an
admitted update opportunity. Provider lookup itself does not record the current
Store value as a producer dependency; actual selector/read consumption does.
No StoreWriter, implicit business mutation or renderer pointer is exposed.

Command providers carry only the typed sender capability. Declaration/lookup
does not send work or execute business callbacks. Normal Busy/Closed and ticket
lifecycle still apply. A closed underlying channel is not silently replaced.

## Stable lifetime and missing dependencies

First lookup caches the selected entry for the consumer's scope lifetime.
Subsequent lookups return that same Store input/channel even if another ancestor
later declares a closer provider. Optional absence is also cached: a component
does not suddenly gain a dependency that was absent during its initialization.
New/remounted consumers use the current ancestry. There is no live retargeting.

Providers must be declared before consuming that key in the same scope. Equal
Store input handle or equal Command channel redeclaration is idempotent. Replacing
an existing declaration, or providing after that scope resolved the same key,
rejects InvalidProvider. To replace identity, remount the provider/component
scope explicitly; changing State inside the same Store uses ordinary revisions
and does not remount consumers. Declaration order in one edit matters: providers
must precede the child mount that uses them, as in explicit component contexts.

Required absence returns `MissingProvider { path, dependency }` containing the
root-to-consumer logical keys and Rust type name. Optional absence returns None;
stale/foreign scopes or malformed inputs are errors. Errors use the component
failure guard: catching a required-dependency/declaration error cannot publish a
half-initialized component. Rust associated State types reject wrong Store-key
input types at compile time. These TypeId/type-name keys are local Rust metadata,
not serialized cross-language protocol IDs.

## Transaction and retirement

The registry is a sparse LogicalUi side table keyed by generation-checked ScopeId,
not a pointer/map on every Scope/RenderNode. Provider entries and per-scope
environments use Rc; draft mutations copy the registry directory lazily, then
copy only the changed environment. Declarations/resolutions use the existing
LogicalEdit candidate and adopt with ordinary scopes/sources/layout/output.
Final frame failure drops tentative registry entries and Command references.
Mount side effects outside the effect API retain their ordinary rollback limits.

Successful scope retirement removes its declarations and cached resolutions.
Store subscription lifetime remains owned by its original StoreInput/source,
not by the provider alias. Both transaction-based and legacy manual reconciliation
reject surviving provider/cached aliases to a retiring source. Ordinary selector
dependencies keep their existing SignalInUse validation too. A failed removal
does not clear provider state. Re-entering a retired key has a new generation and
resolves fresh dependencies.

Command references are released as their provider/consumer environments retire;
external event handles/clones can intentionally keep a sender alive. Accepted
business requests are independent of those references: dropping a provider's last
sender stops future admission but preserves accepted FIFO work. No implicit save
cancellation, task abort, permission revocation or handler-generation policy is
added by dependency lookup. Those event/task lifecycle contracts remain separate.

## Costs, validation and remaining scope

See [controls and reproduction](performance/2026-11-18-scoped-providers/README.md).
First lookup walks logical ancestors and stores one cached Rc/absence. Repeated
lookup avoids the ancestry walk but still uses scope/key lookup and the ordinary
generation checks; returning a CommandSender clones its channel reference. Sparse
registry mutations clone its directory; final validation scans live provider and
resolved entries for source lifetime validity. Manual retirement checks likewise
scan surviving entries. Input validation can compute retired scopes. Ordinary
source/producer/candidate/logical ordering/layout/geometry costs remain. No
wall-time/FPS/memory improvement is claimed; no source schema6 or GPU format change.

Implemented: explicit StoreKey, nearest logical provider lookup, local overrides,
fixed consumer identities/optional absence, required dependency diagnostics,
transactional registry and retirement. Remaining: use_* TLS/hook macros, automatic
event-handler/task integration and stale callback rejection, KeepAlive activation
policies, named runtime instances beyond keys/subtrees, cross-language key/schema,
resource budgets and independent native presentation. Full RFC stays active.
"99%" remains figurative common fluid/intuitive behavior, not a numeric gate.
