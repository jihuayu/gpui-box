use super::*;
use crate::retained::ui_ir::*;

fn sheet(name: &str, declarations: Vec<StyleDeclaration>) -> StyleRef {
    StyleRef::new(name, declarations).unwrap()
}
fn environment(width: f32) -> LayoutEnvironment {
    LayoutEnvironment::new([width, 180.0])
}
fn output(styles: &[StyleRef], hovered: bool, pressed: bool) -> StyleResolution {
    resolve(
        &LayoutStyle::default(),
        &UiProperties::default(),
        styles,
        &environment(120.0),
        StyleState {
            hovered,
            pressed,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn atomic_composition_replaces_complete_rules_and_explains_each_winner() {
    let base = sheet(
        "base",
        vec![StyleDeclaration::Color(
            StyleRule::value(1)
                .when(
                    StyleCondition {
                        hovered: Some(true),
                        ..Default::default()
                    },
                    2,
                )
                .when(
                    StyleCondition {
                        pressed: Some(true),
                        ..Default::default()
                    },
                    3,
                ),
        )],
    );
    let external = sheet(
        "external",
        vec![StyleDeclaration::Color(StyleRule::value(4).when(
            StyleCondition {
                selected: Some(true),
                ..Default::default()
            },
            5,
        ))],
    );
    assert_eq!(
        output(std::slice::from_ref(&base), true, false)
            .properties
            .color,
        2
    );
    let both = output(std::slice::from_ref(&base), true, true);
    assert_eq!(both.properties.color, 3);
    assert_eq!(both.winners[&StyleProperty::Color].case_index, Some(1));
    let replaced = output(&[base.clone(), external.clone()], true, true);
    assert_eq!(replaced.properties.color, 4);
    assert_eq!(replaced.winners[&StyleProperty::Color].style_index, 1);
    assert_eq!(replaced.winners[&StyleProperty::Color].case_index, None);
    let initial = sheet("reset", vec![StyleDeclaration::Color(StyleRule::initial())]);
    let reset = output(&[base, external, initial], true, true);
    assert_eq!(reset.properties.color, UiProperties::default().color);
    assert!(reset.winners[&StyleProperty::Color].initial);
}

#[test]
fn shorthands_expand_before_duplicates_and_composition_uses_order() {
    let padding = sheet(
        "padding",
        vec![StyleDeclaration::Padding(StyleRule::value(
            LayoutEdges::all(LayoutLength::Px(12.0)),
        ))],
    );
    let left = sheet(
        "left",
        vec![StyleDeclaration::PaddingLeft(StyleRule::value(
            LayoutLength::Px(20.0),
        ))],
    );
    assert_eq!(
        output(&[padding.clone(), left.clone()], false, false)
            .layout
            .padding
            .left,
        LayoutLength::Px(20.0)
    );
    assert_eq!(
        output(&[left, padding.clone()], false, false)
            .layout
            .padding
            .left,
        LayoutLength::Px(12.0)
    );
    assert!(matches!(
        StyleRef::new(
            "duplicate",
            vec![
                StyleDeclaration::Padding(StyleRule::value(LayoutEdges::zero())),
                StyleDeclaration::PaddingLeft(StyleRule::value(LayoutLength::Px(1.0)))
            ]
        ),
        Err(StyleError::DuplicateProperty(StyleProperty::PaddingLeft))
    ));
    let initial = sheet(
        "reset",
        vec![StyleDeclaration::PaddingLeft(StyleRule::initial())],
    );
    let reset = output(&[padding, initial], false, false);
    assert_eq!(reset.layout.padding.left, LayoutLength::Px(0.0));
    assert_eq!(reset.layout.padding.right, LayoutLength::Px(12.0));
    assert_eq!(
        reset.winners[&StyleProperty::PaddingRight].declaration_index,
        1
    );
}

#[test]
fn conditions_use_conjunction_explicit_width_interval_and_unknown_capabilities() {
    let conditional = sheet(
        "conditions",
        vec![StyleDeclaration::Color(
            StyleRule::value(1)
                .when(
                    StyleCondition {
                        min_width: Some(100.0),
                        max_width: Some(200.0),
                        hovered: Some(true),
                        pressed: Some(false),
                        ..Default::default()
                    },
                    2,
                )
                .when(
                    StyleCondition {
                        reduced_motion: Some(false),
                        ..Default::default()
                    },
                    3,
                ),
        )],
    );
    let resolve_at = |width, reduced_motion, hovered| {
        let mut environment = environment(width);
        environment.reduced_motion = reduced_motion;
        resolve(
            &LayoutStyle::default(),
            &UiProperties::default(),
            std::slice::from_ref(&conditional),
            &environment,
            StyleState {
                hovered,
                ..Default::default()
            },
        )
        .unwrap()
        .properties
        .color
    };
    for (width, expected) in [(99.0, 1), (100.0, 2), (199.0, 2), (200.0, 1)] {
        assert_eq!(resolve_at(width, None, true), expected);
    }
    assert_eq!(resolve_at(120.0, None, false), 1);
    assert_eq!(resolve_at(120.0, Some(false), true), 3);
    assert_eq!(resolve_at(120.0, Some(true), true), 2);
}

#[test]
fn every_branch_is_validated_and_invalid_combinations_fail_after_selection() {
    assert!(matches!(
        StyleRef::new(
            "invalid unused",
            vec![StyleDeclaration::Opacity(StyleRule::value(1.0).when(
                StyleCondition {
                    hovered: Some(true),
                    ..Default::default()
                },
                f32::NAN
            ))]
        ),
        Err(StyleError::NonReflexiveValue)
    ));
    assert!(
        StyleRef::new(
            "negative padding",
            vec![StyleDeclaration::PaddingLeft(StyleRule::value(
                LayoutLength::Px(-1.0)
            ))]
        )
        .is_err()
    );
    assert!(
        StyleRef::new(
            "bad interval",
            vec![StyleDeclaration::Color(StyleRule::value(1).when(
                StyleCondition {
                    min_width: Some(10.0),
                    max_width: Some(10.0),
                    ..Default::default()
                },
                2
            ))]
        )
        .is_err()
    );
    let grid = sheet(
        "grid",
        vec![StyleDeclaration::Display(StyleRule::value(
            LayoutDisplay::Grid,
        ))],
    );
    let gap = sheet(
        "axis gap",
        vec![StyleDeclaration::MainCrossGap(StyleRule::value(Some(
            LayoutSize::new(LayoutLength::Px(2.0), LayoutLength::Px(2.0)),
        )))],
    );
    assert!(
        resolve(
            &LayoutStyle::default(),
            &UiProperties::default(),
            &[grid, gap],
            &environment(120.0),
            StyleState::default()
        )
        .is_err()
    );
    assert!(matches!(
        StyleRef::with_limits(
            "bounded",
            vec![StyleDeclaration::Padding(StyleRule::value(
                LayoutEdges::zero()
            ))],
            StyleLimits {
                max_declarations: 3,
                ..Default::default()
            }
        ),
        Err(StyleError::LimitExceeded)
    ));
    let mut declarations = vec![StyleDeclaration::Color(StyleRule::value(1))];
    if let StyleDeclaration::Color(rule) = &mut declarations[0] {
        rule.cases.push(StyleCase {
            condition: StyleCondition::default(),
            value: StyleValue::Initial,
        });
    }
    assert!(matches!(
        StyleRef::with_limits(
            "cases",
            declarations,
            StyleLimits {
                max_cases_per_declaration: 0,
                ..Default::default()
            }
        ),
        Err(StyleError::LimitExceeded)
    ));
    assert!(matches!(
        StyleRef::with_limits(
            "long name",
            vec![],
            StyleLimits {
                max_name_bytes: 1,
                ..Default::default()
            }
        ),
        Err(StyleError::LimitExceeded)
    ));
    assert!(matches!(
        StyleRef::with_limits(
            "grid",
            vec![StyleDeclaration::Grid(StyleRule::value(Some(Arc::new(
                GridStyle::default().columns(vec![GridTrack::px(10.0).into(); 2])
            ))))],
            StyleLimits {
                max_grid_track_entries_per_value: 1,
                ..Default::default()
            }
        ),
        Err(StyleError::LimitExceeded)
    ));
    assert!(matches!(
        StyleRef::with_limits(
            "shadows",
            vec![StyleDeclaration::Shadows(StyleRule::value(
                vec![BoxShadow::default()].into()
            ))],
            StyleLimits {
                max_shadows_per_value: 0,
                ..Default::default()
            }
        ),
        Err(StyleError::LimitExceeded)
    ));
}

const ROOT: UiNodeId = UiNodeId::new(0, 1);
const CHILD: UiNodeId = UiNodeId::new(1, 1);
fn bounds() -> LayoutConstraints {
    LayoutConstraints::viewport(180.0, 180.0)
}
fn batch(doc: &UiDocument, operations: Vec<UiOperation>) -> UiBatch {
    UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 8,
        epoch: 1,
        base_revision: doc.revision(),
        revision: doc.revision() + 1,
        sequence: doc.next_sequence(),
        operations,
    }
}
fn apply(
    doc: &mut UiDocument,
    operations: Vec<UiOperation>,
    resources: &UiResources,
) -> Result<UiPublication, UiError> {
    doc.apply(
        batch(doc, operations),
        resources,
        BackendCapabilities {
            solid_color_patch: true,
        },
        bounds(),
        [180.0; 2],
    )
}
fn setup() -> (UiDocument, UiResources) {
    let resources = UiResources::new();
    let mut root = UiNode::new(
        ROOT,
        LayoutStyle::column().size(LayoutDimension::Px(120.0), LayoutDimension::Auto),
    );
    root.children = vec![CHILD];
    let mut child = UiNode::new(
        CHILD,
        LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0)),
    );
    child.styles = vec![sheet(
        "button",
        vec![
            StyleDeclaration::Color(StyleRule::value(0xff0000ff).when(
                StyleCondition {
                    hovered: Some(true),
                    ..Default::default()
                },
                0x0000ffff,
            )),
            StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(20.0)).when(
                StyleCondition {
                    min_width: Some(200.0),
                    ..Default::default()
                },
                LayoutDimension::Px(50.0),
            )),
        ],
    )];
    let mut snapshot = UiSnapshot::new(8, 1, ROOT, vec![child, root]);
    snapshot.style_environment = environment(120.0);
    let (doc, _) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities {
            solid_color_patch: true,
        },
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    (doc, resources)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
fn reference(width: f32, color: u32) -> FramePacket {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(120.0), LayoutDimension::Auto),
        0,
    )
    .unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    tree.append_layout(
        tree.root(),
        LayoutStyle::row().size(LayoutDimension::Px(width), LayoutDimension::Px(10.0)),
        color,
    )
    .unwrap();
    tree.frame([180.0; 2]).unwrap()
}

#[test]
fn owned_style_state_environment_and_external_override_use_normal_atomic_routes() {
    fn transferable<T: Send + Sync>() {}
    transferable::<StyleRef>();
    transferable::<StyleResolution>();
    let (mut doc, resources) = setup();
    let old = doc.published_frame();
    let old_pixels = pixels(old.resolve());
    let hover = apply(
        &mut doc,
        vec![UiOperation::StyleState {
            node: CHILD,
            value: StyleState {
                hovered: true,
                ..Default::default()
            },
        }],
        &resources,
    )
    .unwrap();
    assert_eq!(hover.frame.resolve().layout_runs, 0);
    assert!(
        hover
            .commit
            .routes
            .iter()
            .any(|(_, path)| *path == UpdatePath::InstancePatch)
    );
    assert_eq!(
        pixels(hover.frame.resolve()),
        pixels(reference(20.0, 0x0000ffff))
    );
    let env = apply(
        &mut doc,
        vec![UiOperation::StyleEnvironment(environment(240.0))],
        &resources,
    )
    .unwrap();
    assert_eq!(doc.layout_result(CHILD).unwrap().size[0], 50.0);
    assert_eq!(
        pixels(env.frame.resolve()),
        pixels(reference(50.0, 0x0000ffff))
    );
    let mut styles = doc.node(CHILD).unwrap().styles.clone();
    styles.push(sheet(
        "external",
        vec![StyleDeclaration::Color(StyleRule::value(0x00ff00ff))],
    ));
    let replaced = apply(
        &mut doc,
        vec![UiOperation::Styles {
            node: CHILD,
            value: styles,
        }],
        &resources,
    )
    .unwrap();
    assert_eq!(
        pixels(replaced.frame.resolve()),
        pixels(reference(50.0, 0x00ff00ff))
    );
    let revision = replaced.frame.revision();
    let no_op = apply(
        &mut doc,
        vec![UiOperation::StyleState {
            node: CHILD,
            value: StyleState {
                hovered: false,
                ..Default::default()
            },
        }],
        &resources,
    )
    .unwrap();
    assert_eq!(no_op.frame.revision(), revision);
    assert!(!no_op.commit.request_frame);
    let restored = apply(
        &mut doc,
        vec![UiOperation::Styles {
            node: CHILD,
            value: vec![],
        }],
        &resources,
    )
    .unwrap();
    assert_eq!(pixels(restored.frame.resolve()), pixels(reference(20.0, 0)));
    let (recovered, frame) = UiDocument::from_snapshot(
        doc.snapshot(),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    assert_eq!(
        pixels(frame.frame.resolve()),
        pixels(restored.frame.resolve())
    );
    drop((doc, recovered, resources));
    assert_eq!(pixels(old.resolve()), old_pixels);
}

#[test]
fn style_failures_preserve_authored_state_environment_frame_and_limits() {
    let (mut doc, resources) = setup();
    let original = doc.snapshot();
    let revision = doc.published_frame().revision();
    let conflict = vec![
        sheet(
            "grid",
            vec![StyleDeclaration::Display(StyleRule::value(
                LayoutDisplay::Grid,
            ))],
        ),
        sheet(
            "gap",
            vec![StyleDeclaration::MainCrossGap(StyleRule::value(Some(
                LayoutSize::new(LayoutLength::Px(1.0), LayoutLength::Px(1.0)),
            )))],
        ),
    ];
    let invalid = apply(
        &mut doc,
        vec![
            UiOperation::StyleEnvironment(environment(300.0)),
            UiOperation::Styles {
                node: CHILD,
                value: conflict,
            },
        ],
        &resources,
    );
    assert!(matches!(invalid, Err(UiError::Style(_))));
    assert_eq!(doc.snapshot(), original);
    assert_eq!(doc.published_frame().revision(), revision);
    let mut invalid = environment(120.0);
    invalid.device_scale = f32::NAN;
    assert!(
        apply(
            &mut doc,
            vec![UiOperation::StyleEnvironment(invalid)],
            &resources
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), original);
    let limits = UiLimits {
        max_styles_per_node: 0,
        ..Default::default()
    };
    assert!(matches!(
        UiDocument::from_snapshot(
            doc.snapshot(),
            &resources,
            limits,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2]
        ),
        Err(UiError::LimitExceeded)
    ));
    for limits in [
        UiLimits {
            max_style_declarations_per_node: 1,
            ..Default::default()
        },
        UiLimits {
            max_style_cases_per_node: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            UiDocument::from_snapshot(
                doc.snapshot(),
                &resources,
                limits,
                BackendCapabilities::default(),
                bounds(),
                [180.0; 2]
            ),
            Err(UiError::LimitExceeded)
        ));
    }
    // Document limits also cover possible values in an unselected case, even
    // when the immutable definition was accepted with larger producer limits.
    for (declaration, limits) in [
        (
            StyleDeclaration::Grid(StyleRule::value(None).when(
                StyleCondition {
                    selected: Some(true),
                    ..Default::default()
                },
                Some(Arc::new(
                    GridStyle::default().columns(vec![GridTrack::px(10.0).into(); 2]),
                )),
            )),
            UiLimits {
                max_grid_track_entries: 1,
                ..Default::default()
            },
        ),
        (
            StyleDeclaration::Shadows(StyleRule::value(Arc::from([])).when(
                StyleCondition {
                    selected: Some(true),
                    ..Default::default()
                },
                vec![BoxShadow::default()].into(),
            )),
            UiLimits {
                max_shadows_per_node: 0,
                ..Default::default()
            },
        ),
    ] {
        let mut snapshot = doc.snapshot();
        snapshot
            .nodes
            .iter_mut()
            .find(|node| node.id == CHILD)
            .unwrap()
            .styles = vec![sheet("larger producer budget", vec![declaration])];
        assert!(matches!(
            UiDocument::from_snapshot(
                snapshot,
                &resources,
                limits,
                BackendCapabilities::default(),
                bounds(),
                [180.0; 2]
            ),
            Err(UiError::LimitExceeded)
        ));
    }
    let mut legacy = doc.snapshot();
    legacy.schema = 1;
    assert!(matches!(
        UiDocument::from_snapshot(
            legacy,
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2]
        ),
        Err(UiError::UnsupportedSchema(1))
    ));
}

#[test]
fn equal_style_definitions_replay_and_snapshot_recovery_keep_authored_rules() {
    let (mut doc, resources) = setup();
    let old = doc.published_frame();
    let original = doc.node(CHILD).unwrap().styles.clone();
    // Reconstructing equal immutable definitions is equality by value.
    let equivalent = StyleRef::new("button", original[0].declarations().to_vec()).unwrap();
    assert_eq!(equivalent, original[0]);
    let input = batch(
        &doc,
        vec![UiOperation::Styles {
            node: CHILD,
            value: vec![equivalent],
        }],
    );
    let result = doc
        .apply(
            input.clone(),
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(result.frame.revision(), old.revision());
    let replay = doc
        .apply(
            input,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(replay.status, UiApplyStatus::Replayed);
    let mut recover = doc.snapshot();
    recover.revision += 3;
    recover.next_sequence += 3;
    recover.style_environment = environment(250.0);
    recover
        .nodes
        .iter_mut()
        .find(|n| n.id == CHILD)
        .unwrap()
        .style_state
        .hovered = true;
    let restored = doc
        .recover(
            recover,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(
        pixels(restored.frame.resolve()),
        pixels(reference(50.0, 0x0000ffff))
    );
    assert_eq!(
        doc.node(CHILD).unwrap().layout.size.width,
        LayoutDimension::Px(20.0)
    );
    assert_eq!(
        doc.resolved_node(CHILD).unwrap().layout.size.width,
        LayoutDimension::Px(50.0)
    );
    assert_eq!(doc.node(CHILD).unwrap().styles, original);
    assert!(old.hit_test(5.0, 5.0).is_some());
}

#[test]
fn style_composition_drives_flex_grid_calc_and_property_updates_against_direct_tree() {
    let resources = UiResources::new();
    let condition = StyleCondition {
        selected: Some(true),
        ..Default::default()
    };
    let layout_style = sheet(
        "responsive-layout",
        vec![
            StyleDeclaration::Display(
                StyleRule::value(LayoutDisplay::Flex).when(condition.clone(), LayoutDisplay::Grid),
            ),
            StyleDeclaration::FlexDirection(StyleRule::value(LayoutFlexDirection::Row)),
            StyleDeclaration::FlexWrap(StyleRule::value(LayoutFlexWrap::Wrap)),
            StyleDeclaration::Grid(StyleRule::value(Some(Arc::new(
                GridStyle::default().columns(vec![
                    GridTrack::minmax(
                        GridMinTrack::Px(0.0),
                        GridMaxTrack::Fr(1.0)
                    )
                    .into();
                    2
                ]),
            )))),
            StyleDeclaration::ColumnGap(StyleRule::value(LayoutLength::calc(1.0, 0.05))),
            StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(100.0))),
        ],
    );
    let property_style = sheet(
        "effects",
        vec![StyleDeclaration::Opacity(StyleRule::value(1.0).when(
            StyleCondition {
                hovered: Some(true),
                ..Default::default()
            },
            0.5,
        ))],
    );
    let mut root = UiNode::new(ROOT, LayoutStyle::row());
    root.styles = vec![layout_style, property_style];
    root.children = (1..=3).map(|slot| UiNodeId::new(slot, 1)).collect();
    let mut nodes = vec![root];
    for slot in 1..=3 {
        let mut node = UiNode::new(
            UiNodeId::new(slot, 1),
            LayoutStyle::row()
                .size(LayoutDimension::Px(40.0), LayoutDimension::Px(10.0))
                .flex_shrink(0.0),
        );
        node.properties.color = [0xff0000ff, 0x00ff00ff, 0x0000ffff][slot as usize - 1];
        nodes.push(node);
    }
    let (mut doc, _) = UiDocument::from_snapshot(
        UiSnapshot::new(8, 1, ROOT, nodes),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    let mut previous_size = None;
    for (grid, hovered) in [
        (false, false),
        (true, false),
        (true, true),
        (false, true),
        (false, false),
    ] {
        let output = apply(
            &mut doc,
            vec![UiOperation::StyleState {
                node: ROOT,
                value: StyleState {
                    selected: grid,
                    hovered,
                    ..Default::default()
                },
            }],
            &resources,
        )
        .unwrap();
        let layout = if grid {
            LayoutStyle::grid(GridStyle::default().columns(
                vec![GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(1.0)).into(); 2],
            ))
        } else {
            LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap)
        }
        .size(LayoutDimension::Px(100.0), LayoutDimension::Auto)
        .gap(LayoutLength::Px(0.0), LayoutLength::calc(1.0, 0.05));
        let mut reference = UiTree::new_layout(layout, 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        for i in 0..3 {
            reference
                .append_layout(
                    reference.root(),
                    LayoutStyle::row()
                        .size(LayoutDimension::Px(40.0), LayoutDimension::Px(10.0))
                        .flex_shrink(0.0),
                    [0xff0000ff, 0x00ff00ff, 0x0000ffff][i],
                )
                .unwrap();
        }
        let mut tx = Transaction::default();
        tx.opacity(reference.root(), if hovered { 0.5 } else { 1.0 });
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let frame = reference.frame([180.0; 2]).unwrap();
        assert_eq!(pixels(output.frame.resolve()), pixels(frame));
        if grid && hovered {
            assert_eq!(output.frame.resolve().layout_runs, 0);
            assert!(
                output
                    .commit
                    .routes
                    .iter()
                    .any(|(_, route)| *route == UpdatePath::PropertyPatch)
            );
        }
        let size = doc.layout_result(ROOT).unwrap().size;
        if let Some(previous) = previous_size {
            assert_eq!(size, previous);
        }
        previous_size = Some(size);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_style_conditions_match_complete_cpu_independent_reference() {
    let (mut doc, resources) = setup();
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("style IR adapter: {:?}", gpu.adapter_info());
    for (width, hovered) in [(120.0, false), (240.0, true), (120.0, true), (120.0, false)] {
        let output = apply(
            &mut doc,
            vec![
                UiOperation::StyleEnvironment(environment(width)),
                UiOperation::StyleState {
                    node: CHILD,
                    value: StyleState {
                        hovered,
                        ..Default::default()
                    },
                },
            ],
            &resources,
        )
        .unwrap();
        let reference = reference(
            if width < 200.0 { 20.0 } else { 50.0 },
            if hovered { 0x0000ffff } else { 0xff0000ff },
        );
        gpu.submit_local(&output.frame, [180, 180], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
