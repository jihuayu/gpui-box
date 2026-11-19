# Typed token tables and owned Style IR expressions

Status: experimental Rust API/data, 2026-11-19. Complete RFC remains active.
See [decision and costs](ADR_RETAINED_TOKENS.md).

`retained::tokens` provides `TokenKey<T>`, complete `TokenSchema` defaults,
`TokenDefinition`, immutable resolved `TokenSnapshot` and owned `TokenProgram`
expressions. IDs use group/name/kind data, not Rust TypeId, pointers or callback
registries. Types include color, number, integer, layout lengths/dimensions,
edge/size values, the current style enums/records, font descriptors and durations.
TokenType is sealed to the supported owned vocabulary. Rust type checking rejects
a color expression for a width property; raw programs also check operand types.

```rust,ignore
let spacing = TokenKey::<LayoutLength>::new("app", "spacing")?;
let factor = TokenKey::<f32>::new("app", "density")?;
let width = TokenKey::<LayoutDimension>::new("app", "card-width")?;
let surface = TokenKey::<u32>::new("app", "surface")?;
let schema = TokenSchema::new(vec![
    TokenDefinition::value(spacing.clone(), LayoutLength::Px(12.0)),
    TokenDefinition::value(factor.clone(), 2.0),
    TokenDefinition::new(width.clone(), spacing.expression()
        .scale(&factor.expression())?.dimension()?),
    TokenDefinition::value(surface.clone(), 0x225588ff),
])?;
let values = schema.resolve(vec![
    TokenDefinition::value(factor, 3.0),
])?;
let sheet = StyleRef::new("card", vec![
    StyleDeclaration::Width(StyleRule::expression(width.expression())),
    StyleDeclaration::Color(StyleRule::expression(surface.expression())),
])?;
let plan = StylePlan::new(&[sheet]);
let effective = plan.resolve_token_values(&base_layout, &base_properties,
    &environment, state, &values)?;
```

Schema construction verifies all defaults, including unused definitions. A resolve
starts from those complete defaults and replaces explicitly listed definitions;
omitted overrides reset to defaults. Derived definitions then see the final table.
Duplicates, unknown overrides, missing references, type mismatch and actual cycle
paths are errors. Each reference includes its kind; identical textual group/name
with a different kind is a distinct typed ID. Keep names/kinds consistent across
frontends. Snapshots share Arc storage, compare by value across equal builds,
expose read-only values and can be sent across threads.

Programs are flat immutable operations with earlier operand indices and the last
operation as result. Every operation is validated/evaluated, including unused
operations. No arbitrary Rust/JS/Go code runs. Supported derived operations are
number add/multiply, linear length add/scale, length-to-dimension, encoded-byte
RGBA mix and edge/size component projections. Auto cannot participate in length
arithmetic. Mixed px/percent becomes the existing owned Calc form, preserving
percentage dependence; layout resolves its basis normally. RGBA mix interpolates
straight encoded channels and rounds nearest; it is not linear-light or
premultiplied interpolation. Font/duration values can be referenced, with no font
preparation or animation scheduler implied.

Raw `TokenProgram::new(Vec<TokenOp>)` and `TokenExpr::from_program` validate the
flat form. Default limits: 4,096 token definitions, 65,536 total definition ops,
4,096 ops/expression and depth 64 inside an expression. Cross-token graphs use
iterative dependency ordering and iterative cycle reporting. `TokenSchema::with_limits`
can tighten limits. Expression builders remain constrained by default per-program
limits. Number/length arithmetic rejects nonfinite results; retained property
validation decides whether a value is legal for its actual destination (for
example negative margin versus nonnegative padding, or opacity 0..1).

StyleValue now supports Expression, and StyleRule supplies expression and
when_expression. Shorthands lower to typed projection operations. Existing
composition still selects the final complete rule per atomic property, then
its last matching condition, then evaluates the selected value. Overridden
declarations/inactive branches do not require their token references at runtime;
their programs remain structurally validated. StylePlan::token_dependencies
returns a conservative set from all branches of winning rules. Existing resolve/
resolve_values use an empty token table and report Missing if a selected token
is needed. New resolve_tokens/resolve_token_values take an explicit snapshot.
Final values undergo ordinary layout/visual and selected payload-bound checks.

Logical components can provide `ThemeGroup<TokenThemeKey>` with the existing
ThemeScope or provide_theme. ComponentContext::use_tokens/select_token exposes
read-only consumer values; callers can evaluate a plan in an ordinary computed
binding. Portal ownership, stable provider identity, equality, source retirement,
failure rollback and complete-frame adoption are the existing rules. Combining
several token groups into one resolved table is explicit; there is no automatic
dynamic group merge. Font token consumers can project into TextStyleScope using
the existing typed text contract.

Owned UI source schema **7** adds UiNode.tokens and UiOperation::Tokens. The
sender/front-end resolves logical providers and supplies the complete table for
that node; the primitive UI graph does not invent logical ancestry. Constructors
use a shared empty snapshot by default. Replay/recovery include token values.
UiDocument's ordinary planner reevaluates when referenced token values differ;
an unrelated token update still adopts the authored record but skips style
evaluation. All final output follows the same candidate/Flex/input/frame path.
Style expression work is bounded per definition/node, and resolved Grid/shadow
payloads retain normal limits. Schema 6 is rejected, not silently reinterpreted.

These are owned symbolic Rust records, not a stable serialized/wire format or
cross-language ABI. No token macros/compiler, component parts, ThemePatch, curve
schema, arbitrary expression functions, cross-frame incremental token solver,
render-domain StyleEpoch, budgeted PendingScene or independent presentation is
included. Schema resolution still rebuilds/validates the table; expressions use
temporary value vectors and ordinary source/candidate/style/layout costs remain.
See [correctness/reproduction](performance/2026-11-19-tokens/README.md). This
stage makes no wall-time/FPS or numerical success-rate claim.
