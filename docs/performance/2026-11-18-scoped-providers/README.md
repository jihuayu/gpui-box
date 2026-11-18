# Scoped provider verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `c1ed478f910ad469dd03fbda8a80d8d42f8f7c51`.
See [contract/costs](../../RETAINED_SCOPED_PROVIDERS.md).
Complete RFC remains active; no timing/FPS/memory/native fluency benefit claim.

## Controls

Nine registry/component controls:

- Root Document and OtherDocument keys share State=u32 but name independent
  captured inputs. Two child subtrees resolve root/local overrides for both
  Store and Save command. Nested Consumers bind provider-driven width20/35 and
  send through separate receivers with payload0/1. No lookup by State type.
- Same input/channel redeclaration is idempotent; changing either identity rejects
  without replacing the existing declaration/resolution.
- Missing required Store reports logical key path[0,7] and Document type; caught
  missing Command error poisons outer edit, registry stays unmodified.
- Optional absence stays None after ancestor declaration. New consumer resolves
  current ancestor; declaring locally after consuming that key rejects.
- Tentative Command provider plus invalid final opacity rolls back registry and
  releases the last sender; receiver ready ends. No leaked provider reference.
- Removing provider/consumer scope clears sparse registry references; previously
  accepted Save42 still dispatches/completes after last sender retires.
- A surviving alias to child-owned Store source blocks child retirement, in both
  manual and transactional reconciliation, preserving live input/scopes.
- Foreign Store source and retired scope reject before adoption. A resolved
  sender to a closed channel returns ordinary Closed, never switches channels.
- ComponentContext declares Store overrides and OtherDocument key before nested
  Consumer mount. Context optional/required Command resolution works, nested
  component inherits the same captured input, no extra business subscription.

One new compile-fail control rejects wrong StoreKey State. Existing source/
context/selector/read-store/typed-command capability controls remain covered.

Core integration extends Command→Store→UI reference: a mount-once ProviderRow
looks up Store/Command through Context, builds ordinary selectors/bindings and
retains scope/node across3model states. Command sender is also resolved from that
consumer's cached environment. Worker commits business revision; UI adoption
remains explicit. Complete CPU/GPU pixels match independent numeric full Flex,
3hardware frames. Unrelated projection-equal Store commit skips component callback
and preserves LocalFrame. Earlier direct Store/Command paths remain covered by
the same extended harness.

Real Noto ProviderLabel resolves captured input/channel; provider selector drives
width100→140→80 while live Props supplies long→short→long text/colors. Same
scope/node and3component callbacks; complete LayoutResult/baseline and every
CPU/GPU pixel match real paragraph-metric numeric full Flex,3hardware frames.
Old glyph packet survives business/UI/font/raster owner retirement. Existing
non-provider text cases keep their complete references.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.

| Check | Observed result |
| --- | --- |
| No-default-feature Store/Command | 16passed,2compile-fail docs |
| Complete retained | 683passed,1ignored |
| Complete text/WGPU | 717passed,62ignored |
| Complete hello | 77passed,28ignored |
| Compile-fail docs | 9passed in both core configurations |
| Nine provider / core UI / real-Noto CPU | Included in complete suites |
| Explicit provider→Store→UI GPU | 1passed,3complete images |
| Explicit real-Noto provider GPU | 1passed,3complete images |
| Library Clippy -D warnings | Passed |
| Complete retained-gpu harness check | Passed |
| Scoped format/diff /314local links across6docs | Passed |
| Five unrelated WIP hashes | Unchanged |

Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96. Complete suites /
hardware cover final production/test code. Initial targeted8provider cases
preceded the nested Context case and full reference extensions. Counts overlap;
only explicitly executed ignored wrappers count as hardware evidence. Existing
unused SpatialUpdate/dependency linker warnings remain.

Production changes extend logical edit
adoption/manual retirement; complete lifecycle/producer/responsive suites cover
those existing contracts. Sparse Registry adds one directory on LogicalUi and
draft reference/optional candidate, not per-Scope/RenderNode fields. Public Error
adds InvalidProvider and MissingProvider diagnostics; source schema remains6.

Registry copy-on-write, first-use ancestry traversal, source retirement/live
provider scans and existing source/producer/candidate/layout/geometry work remain.
No provider permission revocation, generalized task/handler scope or native
presentation acceptance is inferred from dependency lookup/offscreen images.
No lint or validation requirement is suppressed.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --no-default-features --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scoped_providers -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_scoped_providers_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
