# Typed logical theme groups

Status: experimental Rust API, 2026-11-19. Full RFC remains active.
See [decision, ownership and limits](ADR_RETAINED_THEMES.md).

`retained::theme::{ThemeKey,ThemeGroup,ThemeScope,ThemeScopeProps}` supplies
complete immutable theme values through the existing logical provider/source
engine. It adds no render backend dependency or second reactive graph.

```rust,ignore
#[derive(Clone, PartialEq)]
struct Colors { surface: u32, accent: u32 }
impl Default for Colors {
    fn default() -> Self {
        Self { surface: 0xf8f9fbff, accent: 0x4464f0ff }
    }
}
struct AppColors;
impl ThemeKey for AppColors { type Values = Colors; }

let theme = ui.state(owner, 20, || ThemeGroup::<AppColors>::defaults().unwrap())?;
ui.provide_theme(owner, theme)?;

// In an ordinary component's mount:
let colors = cx.use_theme::<AppColors>()?;
let fill = cx.select_theme(0, colors, |v| Ok(v.surface))?;
cx.bind_prop(fill, Property::color(node))?;

// A complete source transaction can switch several groups/business inputs.
ui.transaction(|tx| tx.set(theme, ThemeGroup::<AppColors>::create(|v| {
    v.surface = 0x17202fff;
})?))?;
```

ThemeKey is an explicit group identity; two keys can use the same Values type.
Values require Default/Clone/PartialEq/Send/Sync. ThemeGroup::create starts from
fresh defaults and applies a construction-time override callback; omitted
fields reset to group defaults, never merge an outer scope's overrides.
from_values constructs an explicit complete group; defaults is validated too.
ThemeKey::validate can reject sizes/ranges; nonreflexive values reject first.
Values must be immutable and validate pure, as for other owned UI snapshots.
No retained callback is executed by a renderer. Equal independently built values
stop ordinary source propagation; clones share their values Arc.

LogicalEdit/ComponentContext expose provide_theme(Signal), provide_theme_prop
(ReadProp), use_theme, try_use_theme and select_theme. LogicalUi exposes provider
declaration; ordinary edit provides explicit lookup/selection. A consumer receives
only ReadProp<ThemeGroup<K>> and selection returns ReadProp<T>, not a writable
Signal. Projections are ordinary owner-local pure computed functions. They may
produce literal StylePlan/StyleResolution or font/text resources using existing
APIs; only actual unequal projection outputs reach normal sinks/layout.

Nearest logical provider wins, including through portals. Changing a render
host does not change theme ownership. Provider identity is captured once for the
consumer lifetime. Declare it before mounting consumers; changing its source
identity or declaring after that same scope resolved the key rejects. A later
closer ancestor provider affects new consumers; existing resolutions stay fixed.
Update the existing group's source value, or explicitly remount to choose another
provider. Required absence names the logical key path/type; optional absence
returns None. A surviving provider/consumer cannot refer to a retired source.

For a declarative override without an extra Flex box:

```rust,ignore
let props = ThemeScopeProps::<AppColors, Card> {
    theme: ThemeGroup::create(|v: &mut Colors| v.accent = 0xff7744ff)?,
    child: card_props,
};
edit.components::<ThemeScope<AppColors, Card>>(parent, host, [(key, props)])?;
```

The wrapper projects theme and child Props from the same live input, provides
the theme, and forwards a read-only derived child input to one ordinary keyed
child scope. ThemeScope owns no render node or layout producer. Same key/type/
host retains the child, local state and bindings when theme/child Props change;
the final tuple settles together. Child Props require Clone; immutable handles
are appropriate for large payloads. Use ordinary independent component sites
when mixing this subtree with siblings. Scope source keys are local as usual.

Once a theme provider has been adopted and an explicit frame has established
constraints/viewport, subsequent source/edit updates participate in logical
complete-frame validation even without layout producers. Failure preserves theme values, scopes/local state and the old
complete frame. Initial construction without a frame still needs an explicit
frame call, as other retained construction does. Hosts build/install the complete
CPU/GPU image and adopt the matching input scene; theme scope does not present.

This stage is typed whole groups, not the entire token-language RFC. Named typed
token IDs/references, derived-token expression graphs, missing/cycle diagnostics,
portable Style IR token evaluation, automatic text inheritance, ThemePatch,
parts restrictions, system theme/font subscriptions and independent renderer
StyleEpoch/PendingScene preparation remain open. Layout/paint values do not
implicitly inherit. Large theme fanout still runs normal owner work; selector
equality avoids unchanged sinks but does not promise O(1) switches or 120 Hz.
