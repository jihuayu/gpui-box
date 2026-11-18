use super::*;
use crate::retained::*;
use std::{cell::RefCell, rc::Rc};

#[test]
fn text_inherit_initial_and_explicit_values_resolve_only_enumerated_properties() {
    let inherited = ResolvedTextStyle {
        font_size: 24.0,
        line_height: TextLineHeight::Multiple(1.5),
        color: 0x123456ff,
        ..Default::default()
    };
    let style = TextStyle {
        font_size: TextValue::Value(20.0),
        color: TextValue::Initial,
        ..Default::default()
    }
    .resolve(&inherited)
    .unwrap();
    assert_eq!(style.typography().unwrap().line_height, 30.0);
    assert_eq!(style.font_size, 20.0);
    assert_eq!(style.color, ResolvedTextStyle::default().color);
    let style = TextStyle {
        line_height: TextValue::Value(TextLineHeight::Px(42.0)),
        ..Default::default()
    }
    .resolve(&inherited)
    .unwrap();
    assert_eq!(style.typography().unwrap().line_height, 42.0);
    assert_eq!(style.color, inherited.color);
    let inherited = ResolvedTextStyle {
        font: FontDescriptor {
            family: "demo family".into(),
            weight: 500,
            slant: FontSlant::Italic,
        },
        ..inherited
    };
    let style = TextStyle {
        font_weight: TextValue::Value(700),
        font_slant: TextValue::Initial,
        ..Default::default()
    }
    .resolve(&inherited)
    .unwrap();
    assert_eq!(style.font.family.as_ref(), "demo family");
    assert_eq!(style.font.weight, 700);
    assert_eq!(style.font.slant, FontSlant::Normal);
    fn send<T: Send + Sync>() {}
    send::<TextStyle>();
    send::<ResolvedTextStyle>();
}

#[test]
fn text_inheritance_uses_logical_ancestry_despite_shared_portal_render_host() {
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let a = ui
        .state(root, 0, || {
            ThemeGroup::<TextStyleKey>::create(|v| v.font_size = 20.0).unwrap()
        })
        .unwrap();
    let b = ui
        .state(root, 1, || {
            ThemeGroup::<TextStyleKey>::create(|v| v.font_size = 30.0).unwrap()
        })
        .unwrap();
    let seen = Rc::new(Observed::default());
    ui.edit_local_frame(
        LayoutConstraints::viewport(200.0, 100.0),
        [200.0, 100.0],
        |_| Ok(()),
        |(), e| {
            e.provide_theme(root, a)?;
            let branches = e.reconcile_keys(root, &[1, 2])?;
            e.provide_theme(branches[1], b)?;
            let portal = e.append_layout(branches[1], host, LayoutStyle::column(), 0)?;
            e.components::<Consumer>(branches[0], portal, [(0, seen.clone())])?;
            e.components::<Consumer>(branches[1], portal, [(0, seen.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    let rows = seen.0.borrow().clone();
    assert_eq!(ui.read_prop(rows[0].1).unwrap().font_size, 20.0);
    assert_eq!(ui.read_prop(rows[1].1).unwrap().font_size, 30.0);
    ui.transaction(|tx| {
        tx.set(
            a,
            ThemeGroup::<TextStyleKey>::create(|v| v.font_size = 24.0)?,
        )
    })
    .unwrap();
    assert_eq!(ui.read_prop(rows[0].1).unwrap().font_size, 24.0);
    assert_eq!(ui.read_prop(rows[1].1).unwrap().font_size, 30.0);
}
#[test]
fn invalid_text_style_and_theme_defaults_reject_before_adoption() {
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            TextStyle {
                font_size: TextValue::Value(size),
                ..Default::default()
            }
            .resolve(&ResolvedTextStyle::default())
            .is_err()
        );
    }
    for line in [
        TextLineHeight::Px(0.0),
        TextLineHeight::Multiple(-1.0),
        TextLineHeight::Multiple(f32::MAX),
    ] {
        assert!(
            ThemeGroup::<TextStyleKey>::from_values(ResolvedTextStyle {
                line_height: line,
                ..Default::default()
            })
            .is_err()
        );
    }
    assert!(
        TextStyle {
            font_family: TextValue::Value("".into()),
            ..Default::default()
        }
        .resolve(&ResolvedTextStyle::default())
        .is_err()
    );
}
#[derive(Default)]
struct Observed(RefCell<Vec<(ScopeId, ReadProp<ResolvedTextStyle>, Signal<u32>)>>);
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
        let style = cx.computed(0, &[], |_| Ok(TextStyle::default()))?;
        let resolved = cx.resolve_text_style(1, prop(style))?;
        let local = cx.state(2, || 42u32)?;
        seen.0.borrow_mut().push((cx.scope(), resolved, local));
        Ok(())
    }
}
#[test]
fn nested_partial_text_scopes_follow_live_outer_values_without_render_boxes_or_remount() {
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(Observed::default());
    type Inner = TextStyleScope<Consumer>;
    type Outer = TextStyleScope<Inner>;
    let mut p = TextStyleScopeProps::<Inner> {
        style: TextStyle {
            font_size: TextValue::Value(20.0),
            color: TextValue::Value(0x225588ff),
            ..Default::default()
        },
        child: TextStyleScopeProps {
            style: TextStyle {
                color: TextValue::Value(0xaabbccff),
                ..Default::default()
            },
            child: seen.clone(),
        },
    };
    ui.edit_local_frame(
        LayoutConstraints::viewport(200.0, 100.0),
        [200.0, 100.0],
        |_| Ok(()),
        |(), e| e.components::<Outer>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    let row = seen.0.borrow()[0];
    assert_eq!(ui.read_prop(row.1).unwrap().font_size, 20.0);
    assert_eq!(ui.read_prop(row.1).unwrap().color, 0xaabbccff);
    p.style.font_size = TextValue::Value(24.0);
    p.child.style.color = TextValue::Inherit;
    ui.edit(
        |_| Ok(()),
        |(), e| e.components::<Outer>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    assert_eq!(seen.0.borrow().len(), 1);
    assert_eq!(ui.read_prop(row.1).unwrap().font_size, 24.0);
    assert_eq!(ui.read_prop(row.1).unwrap().color, 0x225588ff);
    assert_eq!(*ui.read(row.2).unwrap(), 42);
    assert!(ui.renders(row.0).unwrap().is_empty());
    let old = ui.published_local_frame().unwrap().clone();
    p.child.style.font_size = TextValue::Value(-1.0);
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), e| e.components::<Outer>(root, host, [(0, p.clone())])
        )
        .is_err()
    );
    assert!(old.same(ui.published_local_frame().unwrap()));
    assert_eq!(ui.read_prop(row.1).unwrap().font_size, 24.0);
    ui.reconcile_keys(root, &[]).unwrap();
    assert!(ui.read_prop(row.1).is_err());
}

#[test]
fn absent_text_provider_uses_initial_defaults_and_keeps_complete_frame_updates_active() {
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(Observed::default());
    ui.edit_local_frame(
        LayoutConstraints::viewport(200.0, 100.0),
        [200.0, 100.0],
        |_| Ok(()),
        |(), e| e.components::<Consumer>(root, host, [(0, seen.clone())]),
    )
    .unwrap();
    let row = seen.0.borrow()[0];
    assert_eq!(*ui.read_prop(row.1).unwrap(), ResolvedTextStyle::default());
    let width = ui
        .state(root, 50, || {
            LayoutStyle::column().size(LayoutDimension::Px(40.0), LayoutDimension::Px(20.0))
        })
        .unwrap();
    let node = ui
        .append_render_layout(root, host, LayoutStyle::column(), 0)
        .unwrap();
    ui.bind(width, reactive::Property::layout_style(node))
        .unwrap();
    ui.transaction(|tx| {
        tx.set(
            width,
            LayoutStyle::column().size(LayoutDimension::Px(60.0), LayoutDimension::Px(20.0)),
        )
    })
    .unwrap();
    assert_eq!(ui.layout_result(node).unwrap().size, [60.0, 20.0]);
    ui.reconcile_keys(root, &[]).unwrap();
    assert!(ui.read_prop(row.1).is_err());
}
