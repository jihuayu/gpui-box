use super::*;
use crate::retained::style::*;

const ROOT: UiNodeId = UiNodeId::new(0, 1);
fn bounds() -> LayoutConstraints {
    LayoutConstraints::viewport(240.0, 180.0)
}
fn sheet(name: &str, declaration: StyleDeclaration) -> StyleRef {
    StyleRef::new(name, vec![declaration]).unwrap()
}
fn color(condition: StyleCondition) -> StyleRef {
    sheet(
        "conditional",
        StyleDeclaration::Color(StyleRule::value(0xff0000ff).when(condition, 0x0000ffff)),
    )
}
fn fixture(styles: Vec<Vec<StyleRef>>) -> (UiDocument, UiResources) {
    let mut root = UiNode::new(
        ROOT,
        LayoutStyle::row()
            .size(LayoutDimension::Px(240.0), LayoutDimension::Auto)
            .flex_wrap(LayoutFlexWrap::Wrap),
    );
    let mut nodes = vec![];
    for (index, styles) in styles.into_iter().enumerate() {
        let id = UiNodeId::new(index as u64 + 1, 1);
        root.children.push(id);
        let mut node = UiNode::new(
            id,
            LayoutStyle::column().size(LayoutDimension::Px(30.0), LayoutDimension::Px(20.0)),
        );
        node.styles = styles;
        nodes.push(node);
    }
    nodes.push(root);
    let mut snapshot = UiSnapshot::new(200, 1, ROOT, nodes);
    snapshot.style_environment = LayoutEnvironment::new([120.0, 180.0]);
    let resources = UiResources::new();
    let (document, _) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [240.0, 180.0],
    )
    .unwrap();
    (document, resources)
}
fn input(document: &UiDocument, operations: Vec<UiOperation>) -> UiBatch {
    UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 200,
        epoch: 1,
        base_revision: document.revision(),
        revision: document.revision() + 1,
        sequence: document.next_sequence(),
        operations,
    }
}
fn apply(
    document: &mut UiDocument,
    resources: &UiResources,
    operations: Vec<UiOperation>,
) -> Result<UiPublication, UiError> {
    document.apply(
        input(document, operations),
        resources,
        BackendCapabilities::default(),
        bounds(),
        [240.0, 180.0],
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [240, 180],
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
fn complete_control(document: &UiDocument, resources: &UiResources, output: &UiPublication) {
    let (full, publication) = UiDocument::from_snapshot(
        document.snapshot(),
        resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [240.0, 180.0],
    )
    .unwrap();
    for id in document.nodes.keys() {
        assert_eq!(document.resolved_node(*id), full.resolved_node(*id));
        assert_eq!(
            document.layout_result(*id).unwrap(),
            full.layout_result(*id).unwrap()
        );
    }
    assert_eq!(
        pixels(output.frame.resolve()),
        pixels(publication.frame.resolve())
    );
}

#[test]
fn compiled_winners_keep_only_live_dependencies_and_owned_diagnostics() {
    let first = color(StyleCondition {
        hovered: Some(true),
        min_width: Some(100.0),
        pointer: Some(true),
        ..Default::default()
    });
    let last = sheet(
        "caller",
        StyleDeclaration::Color(StyleRule::value(0x00ff00ff).when(
            StyleCondition {
                pressed: Some(true),
                reduced_motion: Some(false),
                ..Default::default()
            },
            0xffffffff,
        )),
    );
    let plan = StylePlan::new(&[first, last]);
    assert_eq!(
        plan.dependencies(),
        StyleDependencies {
            pressed: true,
            reduced_motion: true,
            ..Default::default()
        }
    );
    let mut environment = LayoutEnvironment::new([120.0, 180.0]);
    environment.reduced_motion = Some(false);
    let output = plan
        .resolve(
            &LayoutStyle::default(),
            &UiProperties::default(),
            &environment,
            StyleState {
                pressed: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(output.properties.color, 0xffffffff);
    assert_eq!(
        output.winners[&StyleProperty::Color],
        StyleWinner {
            style_index: 1,
            declaration_index: 0,
            case_index: Some(0),
            initial: false
        }
    );
}

#[test]
fn isolated_state_changes_share_untouched_records_and_skip_unread_inputs() {
    let hover = color(StyleCondition {
        hovered: Some(true),
        ..Default::default()
    });
    let (mut document, resources) = fixture(vec![vec![hover]; 32]);
    let id = UiNodeId::new(1, 1);
    let authored = document.nodes.clone();
    let effective = document.resolved_nodes.clone();
    let old = document.published_frame();
    let old_pixels = pixels(old.resolve());
    let publication = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleState {
            node: id,
            value: StyleState {
                hovered: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert_eq!(publication.planning.nodes_considered, 1);
    assert_eq!(publication.planning.styles_evaluated, 1);
    assert_eq!(publication.planning.style_plans_built, 0);
    assert_eq!(publication.planning.topology_validations, 0);
    for (&other, value) in &authored {
        if other != id {
            assert!(Arc::ptr_eq(value, &document.nodes[&other]));
            assert!(Arc::ptr_eq(
                &effective[&other],
                &document.resolved_nodes[&other]
            ));
        }
    }
    complete_control(&document, &resources, &publication);
    let revision = publication.frame.revision();
    let publication = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleState {
            node: id,
            value: StyleState {
                hovered: true,
                selected: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert_eq!(publication.planning.styles_evaluated, 0);
    assert_eq!(publication.frame.revision(), revision);
    assert!(!publication.commit.request_frame);
    complete_control(&document, &resources, &publication);
    let state = document.node(id).unwrap().style_state;
    let publication = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleState {
            node: id,
            value: state,
        }],
    )
    .unwrap();
    assert_eq!(publication.planning.effective_records_reused, 1);
    assert_eq!(publication.planning.styles_evaluated, 0);
    assert_eq!(pixels(old.resolve()), old_pixels);
}

#[test]
fn environment_index_uses_each_field_and_final_batch_values() {
    let cases = [
        StyleCondition {
            min_width: Some(200.0),
            ..Default::default()
        },
        StyleCondition {
            direction: Some(LayoutDirection::Rtl),
            ..Default::default()
        },
        StyleCondition {
            reduced_motion: Some(true),
            ..Default::default()
        },
        StyleCondition {
            touch: Some(false),
            ..Default::default()
        },
        StyleCondition {
            pointer: Some(true),
            ..Default::default()
        },
    ];
    let (mut document, resources) = fixture(
        cases
            .into_iter()
            .map(|case| vec![color(case)])
            .chain(std::iter::once(vec![sheet(
                "constant",
                StyleDeclaration::Color(StyleRule::value(0x00ff00ff)),
            )]))
            .collect(),
    );
    let mut environment = document.style_environment().clone();
    // Scale and height are accepted metadata, but no current style rule reads them.
    environment.layout_viewport[1] = 300.0;
    environment.device_scale = 2.0;
    environment.application_zoom = 1.2;
    environment.text_scale = 1.1;
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleEnvironment(environment.clone())],
    )
    .unwrap();
    assert_eq!(output.planning.nodes_considered, 0);
    assert_eq!(output.planning.styles_evaluated, 0);
    for index in 0..5 {
        match index {
            0 => environment.layout_viewport[0] = 240.0,
            1 => environment.direction = LayoutDirection::Rtl,
            2 => environment.reduced_motion = Some(true),
            3 => environment.touch = Some(false),
            _ => environment.pointer = Some(true),
        }
        let output = apply(
            &mut document,
            &resources,
            vec![UiOperation::StyleEnvironment(environment.clone())],
        )
        .unwrap();
        assert_eq!(output.planning.nodes_considered, 1);
        assert_eq!(output.planning.styles_evaluated, 1);
        assert_eq!(output.planning.style_plans_built, 0);
        complete_control(&document, &resources, &output);
    }
    let mut transient = environment.clone();
    transient.layout_viewport[0] = 80.0;
    let output = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::StyleEnvironment(transient),
            UiOperation::StyleEnvironment(environment.clone()),
        ],
    )
    .unwrap();
    assert_eq!(output.planning.nodes_considered, 0);
    assert_eq!(output.planning.styles_evaluated, 0);
    // Overwriting the only width-dependent rule removes its subscription.
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::Styles {
            node: UiNodeId::new(1, 1),
            value: vec![
                color(StyleCondition {
                    min_width: Some(200.0),
                    ..Default::default()
                }),
                sheet(
                    "external",
                    StyleDeclaration::Color(StyleRule::value(0xffffffff)),
                ),
            ],
        }],
    )
    .unwrap();
    assert_eq!(output.planning.style_plans_built, 1);
    environment.layout_viewport[0] = 20.0;
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleEnvironment(environment)],
    )
    .unwrap();
    assert_eq!(output.planning.nodes_considered, 0);
    complete_control(&document, &resources, &output);
}

#[test]
fn definition_changes_and_failed_candidates_preserve_index_authority() {
    let (mut document, resources) = fixture(vec![vec![color(StyleCondition {
        min_width: Some(200.0),
        ..Default::default()
    })]]);
    let original = document.snapshot();
    let first = UiNodeId::new(1, 1);
    let replacement = color(StyleCondition {
        pointer: Some(true),
        ..Default::default()
    });
    // Fail after preparing the new plan/index and staging normal layout.
    let batch = input(
        &document,
        vec![UiOperation::Styles {
            node: first,
            value: vec![replacement.clone()],
        }],
    );
    assert!(
        document
            .apply(
                batch,
                &resources,
                BackendCapabilities::default(),
                bounds(),
                [f32::NAN, 180.0]
            )
            .is_err()
    );
    assert_eq!(document.snapshot(), original);
    let mut environment = document.style_environment().clone();
    environment.layout_viewport[0] = 240.0;
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleEnvironment(environment.clone())],
    )
    .unwrap();
    assert_eq!(output.planning.styles_evaluated, 1);
    assert_eq!(
        document.resolved_node(first).unwrap().properties.color,
        0x0000ffff
    );
    // A new dependency and new environment in one batch use the final values.
    environment.pointer = Some(true);
    let output = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::StyleEnvironment(environment),
            UiOperation::Styles {
                node: first,
                value: vec![replacement],
            },
        ],
    )
    .unwrap();
    assert_eq!(output.planning.styles_evaluated, 1);
    assert_eq!(
        document.resolved_node(first).unwrap().properties.color,
        0x0000ffff
    );
    complete_control(&document, &resources, &output);
    let batch = input(&document, vec![]);
    document
        .apply(
            batch.clone(),
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [240.0, 180.0],
        )
        .unwrap();
    let replay = document
        .apply(
            batch,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [10.0, 10.0],
        )
        .unwrap();
    assert_eq!(replay.planning, UiPlanningStats::default());
}

#[test]
fn removal_recovery_and_content_changes_keep_complete_resolution_equivalent() {
    let hover = color(StyleCondition {
        hovered: Some(true),
        min_width: Some(100.0),
        ..Default::default()
    });
    let (mut document, resources) = fixture(vec![vec![hover.clone()]; 4]);
    let removed = UiNodeId::new(2, 1);
    let old = document.published_frame();
    let old_pixels = pixels(old.resolve());
    let output = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::RemoveSubtree(removed),
            UiOperation::Content {
                node: UiNodeId::new(1, 1),
                value: UiContent::Fixed {
                    size: [12.0, 8.0],
                    first_baseline: Some(6.0),
                    last_baseline: Some(6.0),
                },
            },
        ],
    )
    .unwrap();
    assert_eq!(output.planning.styles_evaluated, 0);
    assert_eq!(output.planning.topology_validations, 1);
    complete_control(&document, &resources, &output);
    let mut environment = document.style_environment().clone();
    environment.layout_viewport[0] = 80.0;
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleEnvironment(environment.clone())],
    )
    .unwrap();
    assert_eq!(output.planning.nodes_considered, 3);
    assert_eq!(output.planning.styles_evaluated, 3);
    let new = UiNodeId::new(2, 2);
    let mut node = UiNode::new(
        new,
        LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0)),
    );
    node.styles = vec![hover];
    let mut children = document.node(ROOT).unwrap().children.clone();
    children.push(new);
    let output = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::Create(node),
            UiOperation::Children {
                node: ROOT,
                value: children,
            },
        ],
    )
    .unwrap();
    assert_eq!(output.planning.style_plans_built, 1);
    assert_eq!(output.planning.topology_validations, 1);
    complete_control(&document, &resources, &output);
    let mut snapshot = document.snapshot();
    snapshot.revision += 2;
    snapshot.next_sequence += 2;
    let output = document
        .recover(
            snapshot,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [240.0, 180.0],
        )
        .unwrap();
    assert_eq!(output.planning.style_plans_built, 4);
    assert_eq!(output.planning.topology_validations, 1);
    environment.layout_viewport[0] = 140.0;
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::StyleEnvironment(environment)],
    )
    .unwrap();
    assert_eq!(output.planning.styles_evaluated, 4);
    complete_control(&document, &resources, &output);
    assert_eq!(pixels(old.resolve()), old_pixels);
}

#[test]
fn every_state_input_and_hidden_authored_base_reenter_the_same_layout_contract() {
    let conditions = [
        StyleCondition {
            hovered: Some(true),
            ..Default::default()
        },
        StyleCondition {
            pressed: Some(true),
            ..Default::default()
        },
        StyleCondition {
            focus_visible: Some(true),
            ..Default::default()
        },
        StyleCondition {
            disabled: Some(true),
            ..Default::default()
        },
        StyleCondition {
            selected: Some(true),
            ..Default::default()
        },
    ];
    let (mut document, resources) = fixture(
        conditions
            .into_iter()
            .map(|condition| vec![color(condition)])
            .collect(),
    );
    for index in 0..5 {
        let mut state = StyleState::default();
        match index {
            0 => state.hovered = true,
            1 => state.pressed = true,
            2 => state.focus_visible = true,
            3 => state.disabled = true,
            _ => state.selected = true,
        }
        let id = UiNodeId::new(index + 1, 1);
        let output = apply(
            &mut document,
            &resources,
            vec![UiOperation::StyleState {
                node: id,
                value: state,
            }],
        )
        .unwrap();
        assert_eq!(output.planning.styles_evaluated, 1);
        assert_eq!(
            document.resolved_node(id).unwrap().properties.color,
            0x0000ffff
        );
        complete_control(&document, &resources, &output);
    }
    let id = UiNodeId::new(1, 1);
    let properties = UiProperties {
        color: 0x00ff00ff,
        opacity: 0.5,
        ..Default::default()
    };
    let style = LayoutStyle::row().size(LayoutDimension::Px(45.0), LayoutDimension::Px(25.0));
    let output = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::Properties {
                node: id,
                value: properties.clone(),
            },
            UiOperation::Layout {
                node: id,
                value: style,
            },
        ],
    )
    .unwrap();
    assert_eq!(output.planning.styles_evaluated, 1);
    assert_eq!(document.node(id).unwrap().properties.color, 0x00ff00ff);
    assert_eq!(
        document.resolved_node(id).unwrap().properties.color,
        0x0000ffff
    );
    assert_eq!(document.layout_result(id).unwrap().size, [45.0, 25.0]);
    complete_control(&document, &resources, &output);
    let output = apply(
        &mut document,
        &resources,
        vec![UiOperation::Styles {
            node: id,
            value: vec![],
        }],
    )
    .unwrap();
    assert_eq!(output.planning.style_plans_built, 0);
    assert_eq!(output.planning.styles_evaluated, 0);
    assert_eq!(document.resolved_node(id).unwrap().properties, properties);
    complete_control(&document, &resources, &output);
    // No-op source batch must still adopt changed host constraints/viewport.
    let output = document
        .apply(
            input(&document, vec![]),
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 180.0),
            [140.0, 180.0],
        )
        .unwrap();
    assert_eq!(output.planning.nodes_considered, 0);
    assert!(output.frame.hit_test(200.0, 10.0).is_none());
    assert_eq!(document.layout_result(ROOT).unwrap().size[0], 240.0);
}

#[test]
fn dense_and_sparse_record_merges_match_ordered_map_reference() {
    let previous: NodeMap = (0..128)
        .map(|slot| {
            let id = UiNodeId::new(slot * 2, 1);
            (id, Arc::new(UiNode::new(id, LayoutStyle::default())))
        })
        .collect();
    for stride in [1, 2, 4, 16, 128] {
        let mut edits = BTreeMap::new();
        let mut expected = previous.clone();
        for slot in (0..260).step_by(stride) {
            let id = UiNodeId::new(slot, 1);
            let value = if slot % 3 == 0 {
                None
            } else {
                Some(Arc::new(UiNode::new(
                    id,
                    LayoutStyle::row().size(LayoutDimension::Px(5.0), LayoutDimension::Auto),
                )))
            };
            if let Some(value) = &value {
                expected.insert(id, value.clone());
            } else {
                expected.remove(&id);
            }
            edits.insert(id, value);
        }
        assert_eq!(planning::apply_effective_edits(&previous, edits), expected);
    }
    assert_eq!(
        planning::apply_effective_edits(&previous, BTreeMap::new()),
        previous
    );
}
