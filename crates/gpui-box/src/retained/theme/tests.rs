use super::*;
use crate::retained::{
    reactive::{Property, Signal},
    *,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq)]
struct Values {
    color: u32,
    width: f32,
    gap: f32,
}
impl Default for Values {
    fn default() -> Self {
        Self {
            color: 0x225588ff,
            width: 30.0,
            gap: 4.0,
        }
    }
}
struct AppTheme;
impl ThemeKey for AppTheme {
    type Values = Values;
    fn validate(v: &Values) -> Result<(), Error> {
        if !v.width.is_finite() || v.width < 0.0 || !v.gap.is_finite() || v.gap < 0.0 {
            return Err(Error::InvalidTheme);
        }
        Ok(())
    }
}
struct OtherTheme;
impl ThemeKey for OtherTheme {
    type Values = Values;
}
fn group(width: f32, color: u32) -> ThemeGroup<AppTheme> {
    ThemeGroup::<AppTheme>::create(|v| {
        v.width = width;
        v.color = color;
    })
    .unwrap()
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
#[derive(Default)]
struct Observed {
    mounts: Cell<usize>,
    rows: RefCell<Vec<(ScopeId, NodeId, Signal<u32>, ReadProp<ThemeGroup<AppTheme>>)>>,
}
impl PartialEq for Observed {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
struct Consumer;
impl Component for Consumer {
    type Props = Rc<Observed>;
    fn mount(
        p: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let seen = cx.read_prop(p)?.clone();
        seen.mounts.set(seen.mounts.get() + 1);
        let theme = cx.use_theme::<AppTheme>()?;
        let local = cx.state(0, || 42u32)?;
        let width = cx.select_theme(1, theme, |v| Ok(fixed(v.width, 20.0)))?;
        let color = cx.select_theme(2, theme, |v| Ok(v.color))?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        cx.bind_prop(width, Property::layout_style(node))?;
        cx.bind_prop(color, Property::color(node))?;
        seen.rows
            .borrow_mut()
            .push((cx.scope(), node, local, theme));
        Ok(())
    }
}
fn ui() -> LogicalUi {
    LogicalUi::new_layout(
        fixed(200.0, 160.0),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap()
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [200, 160],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
#[test]
fn complete_groups_reset_omitted_values_validate_and_compare_by_value() {
    let outer = ThemeGroup::<AppTheme>::create(|v| {
        v.width = 80.0;
        v.gap = 12.0;
    })
    .unwrap();
    let inner = ThemeGroup::<AppTheme>::create(|v| v.color = 0xff0000ff).unwrap();
    assert_eq!(inner.values().width, 30.0);
    assert_eq!(inner.values().gap, 4.0);
    assert_ne!(inner, outer);
    assert_eq!(
        inner,
        ThemeGroup::create(|v: &mut Values| v.color = 0xff0000ff).unwrap()
    );
    assert!(ThemeGroup::<AppTheme>::create(|v| v.width = -1.0).is_err());
    assert!(ThemeGroup::<OtherTheme>::create(|v| v.width = f32::NAN).is_err());
    fn send<T: Send + Sync>() {}
    send::<ThemeGroup<AppTheme>>();
}
#[test]
fn nearest_logical_theme_overrides_portal_render_host_and_switch_retains_local_state() {
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let a = ui.state(root, 0, || group(30.0, 0x225588ff)).unwrap();
    let b = ui.state(root, 1, || group(50.0, 0x885522ff)).unwrap();
    let seen = Rc::new(Observed::default());
    let (children, portal) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(200.0, 160.0),
            [200.0, 160.0],
            |_| Ok(()),
            |(), e| {
                e.provide_theme(root, a)?;
                let children = e.reconcile_keys(root, &[1, 2])?;
                e.provide_theme(children[1], b)?;
                let portal = e.append_layout(children[1], host, fixed(100.0, 80.0), 0)?;
                // Both render into the inner provider's host. First still inherits root.
                e.components::<Consumer>(children[0], portal, [(0, seen.clone())])?;
                e.components::<Consumer>(children[1], portal, [(0, seen.clone())])?;
                Ok((children, portal))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let rows = seen.rows.borrow().clone();
    assert_eq!(ui.layout_result(rows[0].1).unwrap().size, [30.0, 20.0]);
    assert_eq!(ui.layout_result(rows[1].1).unwrap().size, [50.0, 20.0]);
    let old = ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    ui.transaction(|tx| {
        tx.set(a, group(65.0, 0x117733ff))?;
        tx.set(b, group(45.0, 0x773311ff))?;
        tx.set(rows[0].2, 91)
    })
    .unwrap();
    assert_eq!(ui.layout_result(rows[0].1).unwrap().size, [65.0, 20.0]);
    assert_eq!(ui.layout_result(rows[1].1).unwrap().size, [45.0, 20.0]);
    assert_eq!(*ui.read(rows[0].2).unwrap(), 91);
    assert_eq!(seen.mounts.get(), 2);
    // Changing only render parentage does not replace provider identity.
    let mut structure = LogicalStructureTransaction::default();
    structure.reparent(rows[0].1, host);
    ui.commit_structure(structure).unwrap();
    assert_eq!(ui.read_prop(rows[0].3).unwrap(), ui.read(a).unwrap());
    assert!(ui.layout_result(portal).is_ok());
    assert_eq!(ui.children(children[0]).unwrap()[0], rows[0].0);
    drop(ui);
    assert_eq!(pixels(old.resolve()), old_pixels);
}
#[test]
fn typed_group_keys_do_not_alias_and_equal_projection_skips_layout_and_sink_writes() {
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let a = ui.state(root, 0, || group(30.0, 0x225588ff)).unwrap();
    let b = ui
        .state(root, 1, || ThemeGroup::<OtherTheme>::defaults().unwrap())
        .unwrap();
    ui.edit_local_frame(
        LayoutConstraints::viewport(200.0, 160.0),
        [200.0, 160.0],
        |_| Ok(()),
        |(), e| {
            e.provide_theme(root, a)?;
            e.provide_theme(root, b)?;
            assert_ne!(
                e.use_theme::<AppTheme>(root)?.dependency(),
                e.use_theme::<OtherTheme>(root)?.dependency()
            );
            e.components::<Consumer>(root, host, [(0, Rc::new(Observed::default()))])?;
            Ok(())
        },
    )
    .unwrap();
    let before = ui.reactive().stats();
    let old = ui.published_local_frame().unwrap().clone();
    ui.transaction(|tx| tx.set(a, ThemeGroup::create(|v: &mut Values| v.gap = 8.0).unwrap()))
        .unwrap();
    assert_eq!(ui.reactive().stats().sink_writes, before.sink_writes);
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        old.revision()
    );
    let commit = ui
        .transaction(|tx| tx.set(a, group(30.0, 0x119955ff)))
        .unwrap();
    assert_eq!(commit.sink_writes, 1);
    assert_eq!(
        ui.published_local_frame().unwrap().resolve().layout_runs,
        old.resolve().layout_runs
    );
    let before = ui.published_local_frame().unwrap().revision();
    let same = ui
        .transaction(|tx| tx.set(a, group(30.0, 0x119955ff)))
        .unwrap();
    assert_eq!(same.changed_sources, 0);
    assert_eq!(ui.published_local_frame().unwrap().revision(), before);
}
#[test]
fn theme_source_identity_missing_foreign_and_caught_provider_errors_reject_atomically() {
    let mut ui = ui();
    let root = ui.root_scope();
    let a = ui.state(root, 0, || group(30.0, 1)).unwrap();
    let b = ui.state(root, 1, || group(40.0, 2)).unwrap();
    ui.provide_theme(root, a).unwrap();
    ui.provide_theme(root, a).unwrap();
    assert!(matches!(
        ui.provide_theme(root, b),
        Err(Error::InvalidProvider)
    ));
    let mut other = super::tests::ui();
    let foreign = other
        .state(other.root_scope(), 0, || group(1.0, 3))
        .unwrap();
    assert!(ui.provide_theme(root, foreign).is_err());
    let mut missing = super::tests::ui();
    let owner = missing.root_scope();
    let result = missing.edit(
        |_| Ok(()),
        |(), e| {
            assert!(e.try_use_theme::<AppTheme>(owner)?.is_none());
            Ok(())
        },
    );
    assert!(result.is_ok());
    let result = missing.edit(
        |_| Ok(()),
        |(), e| {
            assert!(matches!(
                e.use_theme::<AppTheme>(owner),
                Err(Error::MissingProvider { .. })
            ));
            Ok(())
        },
    );
    assert!(result.is_err());
}
#[test]
fn failed_theme_layout_keeps_source_local_state_and_complete_frame_then_valid_retry_succeeds() {
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let source = ui.state(root, 0, || group(30.0, 0x225588ff)).unwrap();
    let seen = Rc::new(Observed::default());
    ui.edit_local_frame(
        LayoutConstraints::viewport(200.0, 160.0),
        [200.0, 160.0],
        |_| Ok(()),
        |(), e| {
            e.provide_theme(root, source)?;
            e.components::<Consumer>(root, host, [(0, seen.clone())])?;
            let p = e.use_theme::<AppTheme>(root)?;
            let opacity =
                e.select_theme(root, 20, p, |v| Ok(if v.width > 100.0 { 2.0 } else { 1.0 }))?;
            e.bind_prop(opacity, Property::opacity(host))?;
            Ok(())
        },
    )
    .unwrap();
    let old = ui.published_local_frame().unwrap().clone();
    let bytes = pixels(old.resolve());
    assert!(
        ui.transaction(|tx| tx.set(source, group(150.0, 0x998811ff)))
            .is_err()
    );
    assert_eq!(ui.read(source).unwrap().values().width, 30.0);
    assert!(old.same(ui.published_local_frame().unwrap()));
    assert_eq!(*ui.read(seen.rows.borrow()[0].2).unwrap(), 42);
    ui.transaction(|tx| tx.set(source, group(70.0, 0x229944ff)))
        .unwrap();
    assert_eq!(
        ui.layout_result(seen.rows.borrow()[0].1).unwrap().size,
        [70.0, 20.0]
    );
    assert_eq!(seen.mounts.get(), 1);
    assert_eq!(pixels(old.resolve()), bytes);
}

#[test]
fn theme_scope_component_replaces_complete_props_without_render_box_or_child_remount() {
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(Observed::default());
    let mut props = ThemeScopeProps::<AppTheme, Consumer> {
        theme: group(30.0, 0x225588ff),
        child: seen.clone(),
    };
    let scopes = ui
        .edit_local_frame(
            LayoutConstraints::viewport(200.0, 160.0),
            [200.0, 160.0],
            |_| Ok(()),
            |(), e| {
                e.components::<ThemeScope<AppTheme, Consumer>>(root, host, [(1, props.clone())])
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let row = seen.rows.borrow()[0];
    assert!(ui.renders(scopes[0]).unwrap().is_empty());
    props.theme = group(75.0, 0x119955ff);
    ui.edit(
        |_| Ok(()),
        |(), e| e.components::<ThemeScope<AppTheme, Consumer>>(root, host, [(1, props.clone())]),
    )
    .unwrap();
    assert_eq!(seen.mounts.get(), 1);
    assert_eq!(ui.layout_result(row.1).unwrap().size, [75.0, 20.0]);
    assert_eq!(*ui.read(row.2).unwrap(), 42);
    ui.reconcile_keys(root, &[]).unwrap();
    assert!(ui.read_prop(row.3).is_err());
}

#[test]
fn retiring_theme_source_rejects_live_provider_alias_and_failed_provider_mount_rolls_back() {
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let owner = ui.reconcile_keys(root, &[9]).unwrap().order[0];
    let source = ui.state(owner, 0, || group(30.0, 1)).unwrap();
    ui.provide_theme(root, source).unwrap();
    assert!(matches!(
        ui.reconcile_keys(root, &[]),
        Err(Error::InvalidProvider)
    ));
    assert!(ui.read(source).is_ok());
    let mut fresh = super::tests::ui();
    let root = fresh.root_scope();
    let host2 = fresh.root_render();
    let source = fresh.state(root, 0, || group(30.0, 1)).unwrap();
    assert!(
        fresh
            .edit_local_frame(
                LayoutConstraints::viewport(200.0, 160.0),
                [200.0, 160.0],
                |_| Ok(()),
                |(), e| {
                    e.provide_theme(root, source)?;
                    e.properties().opacity(host2, 2.0);
                    Ok(())
                }
            )
            .is_err()
    );
    assert!(
        fresh
            .edit(|_| Ok(()), |(), e| e.try_use_theme::<AppTheme>(root))
            .unwrap()
            .result
            .value
            .is_none()
    );
    assert!(ui.reactive().tree().style(host).is_ok());
}

#[test]
fn transparent_theme_scope_forwards_live_child_payload_and_final_tuple_without_remount() {
    struct Pair;
    impl Component for Pair {
        type Props = f32;
        fn mount(p: ReadProp<f32>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
            let theme = cx.use_theme::<AppTheme>()?;
            let value = cx.computed(0, &[theme.dependency(), p.dependency()], move |i| {
                let width = i.read_prop(theme)?.values().width;
                let expected = *i.read_prop(p)?;
                if width != expected {
                    return Err(Error::InvalidTheme);
                }
                Ok(fixed(width, 20.0))
            })?;
            let node = cx.append_layout(LayoutStyle::column(), 0x225588ff)?;
            cx.bind(value, Property::layout_style(node))?;
            Ok(())
        }
    }
    let mut ui = ui();
    let root = ui.root_scope();
    let host = ui.root_render();
    let scope = ui
        .edit_local_frame(
            LayoutConstraints::viewport(200.0, 160.0),
            [200.0, 160.0],
            |_| Ok(()),
            |(), e| {
                e.components::<ThemeScope<AppTheme, Pair>>(
                    root,
                    host,
                    [(
                        0,
                        ThemeScopeProps {
                            theme: group(30.0, 1),
                            child: 30.0,
                        },
                    )],
                )
            },
        )
        .unwrap()
        .0
        .result
        .value[0];
    let child = ui.children(scope).unwrap()[0];
    let node = ui.renders(child).unwrap()[0];
    ui.edit(
        |_| Ok(()),
        |(), e| {
            e.components::<ThemeScope<AppTheme, Pair>>(
                root,
                host,
                [(
                    0,
                    ThemeScopeProps {
                        theme: group(70.0, 2),
                        child: 70.0,
                    },
                )],
            )
        },
    )
    .unwrap();
    assert_eq!(ui.children(scope).unwrap(), [child]);
    assert_eq!(ui.renders(child).unwrap(), [node]);
    assert_eq!(ui.layout_result(node).unwrap().size, [70.0, 20.0]);
    let old = ui.published_local_frame().unwrap().clone();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), e| e.components::<ThemeScope<AppTheme, Pair>>(
                root,
                host,
                [(
                    0,
                    ThemeScopeProps {
                        theme: group(80.0, 2),
                        child: 70.0
                    }
                )]
            )
        )
        .is_err()
    );
    assert!(old.same(ui.published_local_frame().unwrap()));
}
