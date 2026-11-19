use super::*;
use crate::retained::{logical::*, reactive::Property, style::*, ui_ir::*};
fn key<T: TokenType>(name: &str) -> TokenKey<T> {
    TokenKey::new("app", name).unwrap()
}
#[test]
fn complete_token_defaults_derived_values_and_overrides_are_pure_and_reset() {
    let space = key::<LayoutLength>("space");
    let factor = key::<f32>("factor");
    let derived = key::<LayoutDimension>("width");
    let color = key::<u32>("color");
    let tint = key::<u32>("tint");
    let schema = TokenSchema::new(vec![
        TokenDefinition::value(space.clone(), LayoutLength::Px(10.0)),
        TokenDefinition::value(factor.clone(), 2.0),
        TokenDefinition::new(
            derived.clone(),
            space
                .expression()
                .scale(&factor.expression())
                .unwrap()
                .plus(&TokenExpr::value(LayoutLength::Percent(0.5)))
                .unwrap()
                .dimension()
                .unwrap(),
        ),
        TokenDefinition::value(color.clone(), 0x000000ff),
        TokenDefinition::new(
            tint.clone(),
            color
                .expression()
                .mix(&TokenExpr::value(0xffffffff), &TokenExpr::value(0.5))
                .unwrap(),
        ),
    ])
    .unwrap();
    let defaults = schema.resolve(vec![]).unwrap();
    assert_eq!(
        defaults.read(&derived).unwrap(),
        LayoutDimension::Calc {
            px: 20.0,
            percent: 0.5
        }
    );
    assert_eq!(defaults.read(&tint).unwrap(), 0x808080ff);
    let changed = schema
        .resolve(vec![TokenDefinition::value(
            space.clone(),
            LayoutLength::Px(20.0),
        )])
        .unwrap();
    assert_eq!(
        changed.read(&derived).unwrap(),
        LayoutDimension::Calc {
            px: 40.0,
            percent: 0.5
        }
    );
    let reset = schema
        .resolve(vec![TokenDefinition::value(color, 0xff0000ff)])
        .unwrap();
    assert_eq!(reset.read(&space).unwrap(), LayoutLength::Px(10.0));
    assert_eq!(defaults, schema.resolve(vec![]).unwrap());
    fn send<T: Send + Sync>() {}
    send::<TokenSchema>();
    send::<TokenSnapshot>();
    send::<TokenProgram>();
}
#[test]
fn missing_cycles_duplicate_type_limits_and_invalid_arithmetic_have_diagnostics() {
    let a = key::<f32>("a");
    let b = key::<f32>("b");
    let c = key::<f32>("c");
    assert!(
        matches!(TokenSchema::new(vec![TokenDefinition::new(a.clone(),b.expression())]),Err(TokenError::Missing(id)) if id==*b.id())
    );
    assert!(
        matches!(TokenSchema::new(vec![TokenDefinition::new(c.clone(),a.expression()),TokenDefinition::new(a.clone(),b.expression()),TokenDefinition::new(b.clone(),a.expression())]),Err(TokenError::Cycle(path)) if path.len()==3 && path.first()==path.last() && !path.contains(c.id()))
    );
    assert!(matches!(
        TokenSchema::new(vec![
            TokenDefinition::value(a.clone(), 1.0),
            TokenDefinition::value(a.clone(), 2.0)
        ]),
        Err(TokenError::Duplicate(_))
    ));
    assert!(TokenProgram::new(vec![TokenOp::Add { left: 0, right: 0 }]).is_err());
    assert!(TokenExpr::<u32>::from_program(TokenExpr::value(1.0f32).program().clone()).is_err());
    assert!(
        TokenSchema::new(vec![TokenDefinition {
            id: a.id().clone(),
            expression: TokenExpr::value(1u32).program().clone()
        }])
        .is_err()
    );
    assert!(TokenSchema::new(vec![TokenDefinition::value(a.clone(), f32::NAN)]).is_err());
    assert!(
        TokenSchema::new(vec![TokenDefinition::new(
            a.clone(),
            TokenExpr::value(f32::MAX)
                .times(&TokenExpr::value(2.0))
                .unwrap()
        )])
        .is_err()
    );
    assert!(
        TokenSchema::with_limits(
            vec![TokenDefinition::value(a.clone(), 1.0)],
            TokenLimits {
                max_tokens: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
    let schema = TokenSchema::new(vec![TokenDefinition::value(a.clone(), 1.0)]).unwrap();
    assert!(matches!(
        schema.resolve(vec![TokenDefinition::value(b, 2.0)]),
        Err(TokenError::UnknownOverride(_))
    ));
    assert!(TokenKey::<f32>::new("", "name").is_err());
    let mut operations = vec![TokenOp::Value(TokenValue::Number(1.0))];
    for index in 0..64 {
        operations.push(TokenOp::Add {
            left: index,
            right: 0,
        });
    }
    assert!(matches!(
        TokenProgram::new(operations),
        Err(TokenError::LimitExceeded)
    ));
    assert!(
        TokenExpr::<f32>::value(f32::NAN)
            .evaluate(&TokenSnapshot::default())
            .is_err()
    );
    assert!(
        TokenExpr::value(LayoutLength::Auto)
            .scale(&TokenExpr::value(2.0))
            .unwrap()
            .evaluate(&TokenSnapshot::default())
            .is_err()
    );
    let basis = key::<LayoutDimension>("basis");
    let values = TokenSchema::new(vec![TokenDefinition::value(
        basis.clone(),
        LayoutDimension::Content,
    )])
    .unwrap()
    .resolve(vec![])
    .unwrap();
    let env = responsive::LayoutEnvironment::new([200.0, 100.0]);
    let rule = StyleRef::new(
        "basis",
        vec![StyleDeclaration::FlexBasis(StyleRule::expression(
            basis.expression(),
        ))],
    )
    .unwrap();
    assert_eq!(
        StylePlan::new(&[rule])
            .resolve_token_values(
                &LayoutStyle::column(),
                &UiProperties::default(),
                &env,
                StyleState::default(),
                &values
            )
            .unwrap()
            .layout
            .flex_basis,
        LayoutDimension::Content
    );
    let rule = StyleRef::new(
        "width",
        vec![StyleDeclaration::Width(StyleRule::expression(
            basis.expression(),
        ))],
    )
    .unwrap();
    assert!(
        StylePlan::new(&[rule])
            .resolve_token_values(
                &LayoutStyle::column(),
                &UiProperties::default(),
                &env,
                StyleState::default(),
                &values
            )
            .is_err()
    );
}
#[test]
fn style_expression_shorthands_winners_conditions_and_property_validation_are_ordinary() {
    let pad = key::<LayoutEdges>("pad");
    let color = key::<u32>("color");
    let width = key::<LayoutDimension>("width");
    let opacity = key::<f32>("opacity");
    let schema = TokenSchema::new(vec![
        TokenDefinition::value(pad.clone(), LayoutEdges::all(LayoutLength::Px(4.0))),
        TokenDefinition::value(color.clone(), 0x112233ff),
        TokenDefinition::value(width.clone(), LayoutDimension::Px(80.0)),
        TokenDefinition::value(opacity.clone(), 1.0),
    ])
    .unwrap();
    let values = schema.resolve(vec![]).unwrap();
    let base = StyleRef::new(
        "base",
        vec![
            StyleDeclaration::Padding(StyleRule::expression(pad.expression())),
            StyleDeclaration::Width(StyleRule::expression(width.expression())),
            StyleDeclaration::Color(StyleRule::expression(color.expression())),
            StyleDeclaration::Opacity(StyleRule::expression(opacity.expression())),
        ],
    )
    .unwrap();
    let plan = StylePlan::new(&[base]);
    let env = responsive::LayoutEnvironment::new([200.0, 100.0]);
    let output = plan
        .resolve_tokens(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default(),
            &values,
        )
        .unwrap();
    assert_eq!(
        output.layout.padding,
        LayoutEdges::all(LayoutLength::Px(4.0))
    );
    assert_eq!(output.properties.color, 0x112233ff);
    assert_eq!(output.winners.len(), 7);
    assert!(matches!(
        plan.resolve_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default()
        ),
        Err(StyleError::Token(TokenError::Missing(_)))
    ));
    let invalid = schema
        .resolve(vec![TokenDefinition::value(opacity, 2.0)])
        .unwrap();
    assert!(matches!(
        plan.resolve_token_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default(),
            &invalid
        ),
        Err(StyleError::InvalidValue(Error::InvalidOpacity))
    ));
    let missing = key::<u32>("missing");
    let first = StyleRef::new(
        "first",
        vec![StyleDeclaration::Color(StyleRule::expression(
            missing.expression(),
        ))],
    )
    .unwrap();
    let last = StyleRef::new(
        "last",
        vec![StyleDeclaration::Color(
            StyleRule::value(0x556677ff).when_expression(
                StyleCondition {
                    hovered: Some(true),
                    ..Default::default()
                },
                color.expression(),
            ),
        )],
    )
    .unwrap();
    let plan = StylePlan::new(&[first, last]);
    assert_eq!(
        plan.resolve_token_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default(),
            &TokenSnapshot::default()
        )
        .unwrap()
        .properties
        .color,
        0x556677ff
    );
    assert_eq!(
        plan.resolve_token_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState {
                hovered: true,
                ..Default::default()
            },
            &values
        )
        .unwrap()
        .properties
        .color,
        0x112233ff
    );
    let reset =
        StyleRef::new("reset", vec![StyleDeclaration::Color(StyleRule::initial())]).unwrap();
    let plan = StylePlan::new(&[reset]);
    assert_eq!(
        plan.resolve_token_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default(),
            &values
        )
        .unwrap()
        .properties
        .color,
        UiProperties::default().color
    );
    let shadows = key::<Arc<[BoxShadow]>>("shadows");
    let table = TokenSchema::new(vec![TokenDefinition::value(
        shadows.clone(),
        Arc::from([BoxShadow::default()]),
    )])
    .unwrap()
    .resolve(vec![])
    .unwrap();
    let limited = StyleRef::with_limits(
        "limited",
        vec![StyleDeclaration::Shadows(StyleRule::expression(
            shadows.expression(),
        ))],
        StyleLimits {
            max_shadows_per_value: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        StylePlan::new(&[limited]).resolve_token_values(
            &LayoutStyle::column(),
            &UiProperties::default(),
            &env,
            StyleState::default(),
            &table
        ),
        Err(StyleError::LimitExceeded)
    ));
}
#[test]
fn token_ui_source_batches_replay_recover_and_failed_adoption_preserve_complete_output() {
    let color = key::<u32>("color");
    let width = key::<LayoutDimension>("width");
    let opacity = key::<f32>("opacity");
    let schema = TokenSchema::new(vec![
        TokenDefinition::value(color.clone(), 0x112233ff),
        TokenDefinition::value(width.clone(), LayoutDimension::Px(60.0)),
        TokenDefinition::value(opacity.clone(), 1.0),
    ])
    .unwrap();
    let root = UiNodeId::new(0, 1);
    let child = UiNodeId::new(1, 1);
    let mut r = UiNode::new(root, LayoutStyle::column());
    r.children = vec![child];
    let mut n = UiNode::new(
        child,
        LayoutStyle::column().size(LayoutDimension::Auto, LayoutDimension::Px(20.0)),
    );
    n.tokens = schema.resolve(vec![]).unwrap();
    n.styles = vec![
        StyleRef::new(
            "tokens",
            vec![
                StyleDeclaration::Color(StyleRule::expression(color.expression())),
                StyleDeclaration::Width(StyleRule::expression(width.expression())),
                StyleDeclaration::Opacity(StyleRule::expression(opacity.expression())),
            ],
        )
        .unwrap(),
    ];
    let resources = UiResources::new();
    let constraints = LayoutConstraints::viewport(200.0, 100.0);
    let (mut doc, _) = UiDocument::from_snapshot(
        UiSnapshot::new(1, 1, root, vec![r, n]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints,
        [200.0, 100.0],
    )
    .unwrap();
    let old = doc.published_frame();
    let old_revision = old.revision();
    let changed = schema
        .resolve(vec![
            TokenDefinition::value(color, 0x556677ff),
            TokenDefinition::value(width, LayoutDimension::Px(90.0)),
        ])
        .unwrap();
    let batch = UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 1,
        epoch: 1,
        base_revision: 0,
        revision: 1,
        sequence: 1,
        operations: vec![UiOperation::Tokens {
            node: child,
            value: changed,
        }],
    };
    let result = doc
        .apply(
            batch.clone(),
            &resources,
            BackendCapabilities::default(),
            constraints,
            [200.0, 100.0],
        )
        .unwrap();
    assert_eq!(result.planning.styles_evaluated, 1);
    assert!(result.frame.revision() > old_revision);
    assert_eq!(
        doc.apply(
            batch,
            &resources,
            BackendCapabilities::default(),
            constraints,
            [200.0, 100.0]
        )
        .unwrap()
        .status,
        UiApplyStatus::Replayed
    );
    let accepted = doc.snapshot();
    let mut obsolete = accepted.clone();
    obsolete.schema = 6;
    assert!(matches!(
        UiDocument::from_snapshot(
            obsolete,
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            constraints,
            [200.0, 100.0]
        ),
        Err(UiError::UnsupportedSchema(6))
    ));
    let bad = schema
        .resolve(vec![TokenDefinition::value(opacity, 2.0)])
        .unwrap();
    let failed = UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 1,
        epoch: 1,
        base_revision: 1,
        revision: 2,
        sequence: 2,
        operations: vec![UiOperation::Tokens {
            node: child,
            value: bad,
        }],
    };
    assert!(
        doc.apply(
            failed,
            &resources,
            BackendCapabilities::default(),
            constraints,
            [200.0, 100.0]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), accepted);
    let mut recovery = accepted;
    recovery.revision = 2;
    recovery.next_sequence = 3;
    doc.recover(
        recovery,
        &resources,
        BackendCapabilities::default(),
        constraints,
        [200.0, 100.0],
    )
    .unwrap();
    assert!(
        old.resolve()
            .items
            .iter()
            .any(|p| p.fill_color == 0x112233ff)
    );
}
#[test]
fn logical_token_theme_updates_existing_style_sinks_without_remount_and_rolls_back() {
    struct Card;
    impl Component for Card {
        type Props = ();
        fn mount(_: ReadProp<()>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
            let tokens = cx.use_tokens()?;
            let width = key::<LayoutDimension>("width");
            let opacity = key::<f32>("opacity");
            let plan = StylePlan::new(&[StyleRef::new(
                "tokens",
                vec![
                    StyleDeclaration::Width(StyleRule::expression(width.expression())),
                    StyleDeclaration::Opacity(StyleRule::expression(opacity.expression())),
                ],
            )
            .unwrap()]);
            let value = cx.computed(0, &[tokens.dependency()], move |i| {
                plan.resolve_token_values(
                    &LayoutStyle::column().size(LayoutDimension::Auto, LayoutDimension::Px(20.0)),
                    &UiProperties::default(),
                    &responsive::LayoutEnvironment::new([200.0, 100.0]),
                    StyleState::default(),
                    i.read_prop(tokens)?.values(),
                )
                .map_err(|_| Error::InvalidStyle)
            })?;
            let node = cx.append_layout(LayoutStyle::column(), 0)?;
            cx.bind(value, Property::style_resolution(node))?;
            Ok(())
        }
    }
    let width = key::<LayoutDimension>("width");
    let opacity = key::<f32>("opacity");
    let schema = TokenSchema::new(vec![
        TokenDefinition::value(width.clone(), LayoutDimension::Px(40.0)),
        TokenDefinition::value(opacity.clone(), 1.0),
    ])
    .unwrap();
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let source = ui
        .state(root, 0, || {
            theme::ThemeGroup::<TokenThemeKey>::from_values(schema.resolve(vec![]).unwrap())
                .unwrap()
        })
        .unwrap();
    let scope = ui
        .edit_local_frame(
            LayoutConstraints::viewport(200.0, 100.0),
            [200.0, 100.0],
            |_| Ok(()),
            |(), e| {
                e.provide_theme(root, source)?;
                e.components::<Card>(root, host, [(0, ())])
            },
        )
        .unwrap()
        .0
        .result
        .value[0];
    let node = ui.renders(scope).unwrap()[0];
    ui.transaction(|tx| {
        tx.set(
            source,
            theme::ThemeGroup::from_values(schema.resolve(vec![TokenDefinition::value(
                width,
                LayoutDimension::Px(80.0),
            )])?)?,
        )
    })
    .unwrap();
    assert_eq!(ui.layout_result(node).unwrap().size, [80.0, 20.0]);
    let old = ui.published_local_frame().unwrap().clone();
    assert!(
        ui.transaction(|tx| tx.set(
            source,
            theme::ThemeGroup::from_values(
                schema.resolve(vec![TokenDefinition::value(opacity, 2.0)])?
            )?
        ))
        .is_err()
    );
    assert!(old.same(ui.published_local_frame().unwrap()));
    assert_eq!(ui.renders(scope).unwrap(), [node]);
}
