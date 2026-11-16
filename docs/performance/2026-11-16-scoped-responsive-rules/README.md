# Scope-owned responsive lifecycle validation — 2026-11-16

Windows x64 / Rust 1.98.1 / locked offline. No Taffy source or GPU formats changed
in this stage; no new frame-time benchmark or sustained-rate acceptance was run.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib responsive_rules -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
```

Results: text/WGPU **331 passed, 6 ignored**; retained **298 passed**;
hello **21 passed, 1 ignored**; Clippy `-D warnings` passed. Explicit scoped-rule
run **9 passed**, including eight regular cases and one hardware case. The
ordinary library run does not execute default ignored hardware tests.

Eight new normal cases cover:

1. A keyed branch creates provisional source/node/rule identities and registers
   a dependent ContainerInline rule before its viewport producer. Repeated
   environment widths 100/100/200/200/100 retain/recreate branches in both
   resolved/local APIs. Actual layout width is 180; viewport selects 70/130,
   padding 0/5, child slot 70/120 and footer y=20/50. All CPU pixels match
   independently authored ordinary forced-full trees; hit testing and old frames
   remain correct. Retirement in an edit without selection removes both rules.
2. Equal declarations and changed breakpoint styles retain exact writer
   identity. Removal allows ordinary endpoint binding in the same edit; a later
   source transaction still updates the correct layout.
3. Removing an already observed writer rejects publication. A separate final
   geometry failure after successful selection also preserves registry, source,
   statistics and old frame. A subsequent valid selection still uses the old
   40-wide declaration rather than the rejected 80-wide update.
4. A caught invalid self-query registration leaves the endpoint available;
   duplicate source/target declarations conflict without changing the registry.
5. Legacy `LogicalUi::reconcile_keys` retires scoped rules that use a surviving
   source, detaches the writer and leaves that source usable for another binding.
6. Foreign-owned targets are rejected. A surviving rule prevents its source's
   scope from retiring in both draft and legacy reconciliation; removing the
   rule allows retirement.
7. An aborted new declaration publishes neither handles nor metadata; creating
   and cancelling a branch in one edit leaves no rule or writer.
8. A surviving portal rule cannot keep a retired query host, even when its
   render target is moved outside that host. Failure preserves the old registry,
   frame and geometry; removing the rule permits the move and retirement.

The first new-branch regression initially found that dead rules were still
included when validating a replacement branch's provisional target. Filtering
rules of the pending retired scope set resolved that failure; final writer
retirement still occurs inside the publication boundary.

The new hello variant uses real bundled Noto Sans CJK SC shaping and glyph
rasterization with scope-owned viewport/container declarations. Widths 60/150
and padding 0/5 yield 60/140 paragraph slots. Repeated narrow/wide keyed content
has actual paragraph-driven height/footer geometry; every CPU pixel equals an
independent ordinary full tree, with explicit cyan glyph coverage. Old output
reproduces its pixels after UI/font/raster owners are dropped.

Explicit hardware: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**.
Each environment 100/200/100/200 creates a replacement keyed scope, new sources,
new viewport/container rules and inline content. Every actual local submission
matches every pixel of an independent fresh forced-full tree. The test also
checks red content coverage and hit testing, then finishes the renderer.
This is output correctness on that adapter, not modern desktop heavy-effects,
visible presentation, sustained 60/120Hz or input-to-visible tail qualification.

The combined feature run rebuilt dependencies after Cargo build-cache GC; no
restart was needed for observation timeouts. Existing retained-only SpatialUpdate
unused fields and Windows macro linker informational warnings remain.

Costs: sparse owned registry storage, whole-registry clone on first mutation,
whole preview/proof scans for non-equal declaration and final validation,
rule-list cloning for selection, and rule/scope retirement scans. Earlier full
atomic candidate/per-stage preview/source-savepoint costs remain. Passing these
tests does not demonstrate a speedup, memory reduction or CPU smooth-delivery
acceptance. See [contract/ADR](../../RETAINED_SCOPED_RESPONSIVE_RULES.md) and
[full progress](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
