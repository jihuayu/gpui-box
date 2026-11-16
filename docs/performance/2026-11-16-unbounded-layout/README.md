# Normal unbounded layout: validation evidence

Date: 2026-11-16. Branch: `codex/rendering-performance-research`.
Baseline before this stage: `e57e989762d687e4eb135c08ca4aa7d47a8eaf90`.
Windows / Rust 1.98.1. See the
[contract and design decisions](../../RETAINED_UNBOUNDED_LAYOUT.md).

## Native checks

Commands use the installed offline dependency cache:

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --all-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --examples --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
```

Final results: framework retained **210 passed**; text/WGPU **243 passed**;
Taffy default **186 passed** / project features **180 passed**; hello **13
passed, 1 hardware test ignored**. Clippy `-D warnings`, examples, Flex-only
and all-feature library checks pass. Hello ran against the current worktree;
the pre-existing retained_demo WIP is excluded from this commit. Retained-only
reports the existing unused `SpatialUpdate` fields; native dependencies print
Windows linker informational warnings.

The framework adds 12 Unbounded tests, the hello suite adds one real paragraph
test, and the fork adds five tests (normal Flex/cache transitions, arithmetic,
actual Block/Grid hosts, fractional Grid tracks and mixed-cache-key separation).
The CPU test checks actual red/transparent pixels inside/outside the finite
clip and matching hit testing. Native full/selective comparisons assert
geometry/clip equality, not a newly captured hardware GPU image. Real text
uses the bundled Noto font, finite50 wrapping and restored unbounded metrics.

## Independent browser block-axis reference

[browser-reference.html](browser-reference.html) creates normal auto-height
Column Flex layouts with finite inline width. Playwright CLI runs a separate
Chromium and reads DOM bounding boxes; it does not call the framework or reuse
Taffy. [browser-reference.json](browser-reference.json) preserves all actual,
expected and per-case results with the browser user agent:
**HeadlessChrome 156.0.0.0, 14 passed / 0 failed**.

Cases cover normal basis versus authored preferred height, min-driven finite
growth, max-bounded wrap, indefinite percent flex-basis with real content,
padding/gap/margins, min-over-max and nested child growth. The auto-height
browser block axis corresponds to the framework's normal Unbounded block axis.
The page does not claim browser equivalence for independently unbounded inline
input, fixed attachment or the complete intrinsic/percentage dependency graph.
Earlier intrinsic-limit percentage-height differences remain open.

Reproduce from the repository root, with a free localhost port:

```powershell
python -m http.server 43871 --bind 127.0.0.1 --directory docs/performance/2026-11-16-unbounded-layout
# In a second terminal:
npx --yes --package @playwright/cli playwright-cli -s=unbounded-layout open http://127.0.0.1:43871/browser-reference.html
npx --yes --package @playwright/cli playwright-cli -s=unbounded-layout --raw eval 'window.referenceResult'
npx --yes --package @playwright/cli playwright-cli -s=unbounded-layout close
```

CLI file navigation was blocked, so capture used this local HTTP server. The
first fixture load exposed a top-level function name collision with
`window.location`; renaming the helper to `localPosition` fixed the fixture
before the final 14/14 result. A missing favicon had no layout effect.
Local CLI session snapshots/logs are disposable capture artifacts, excluded
from the commit. The exact engine version may change with CLI installation;
the saved user agent describes this capture only.

## What this establishes

Normal unbounded layout is no longer rejected or mapped onto an intrinsic
query. A finite paint viewport does not become a percent/OOF layout base.
Viewport-only edits update clipping without a Taffy run; changed available
space triggers ordinary layout and preserves old frame snapshots. These are
capability and correctness results. No benchmark timing, measured memory gain,
sustained 60/120Hz, visible native presentation, GPU effects tail latency or CPU
smoothness qualification was performed. The full RFC remains active.
