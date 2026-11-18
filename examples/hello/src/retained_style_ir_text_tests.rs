use super::*;
use gpui_box::retained::{style::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const TEXT: UiNodeId = UiNodeId::new(1, 1);
fn leaf() -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(100.0), LayoutDimension::Auto)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 400],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
#[test]
fn style_rules_rewrap_real_noto_and_publish_complete_direct_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_style_ir_real_noto_matches_direct_reference() {
    run(true);
}
#[test]
fn logical_feedback_real_noto_component_rewrap_matches_complete_reference() {
    run_logical(false, false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_logical_feedback_real_noto_matches_complete_reference() {
    run_logical(true, false);
}
#[test]
fn input_pump_real_noto_component_matches_complete_reference() {
    run_logical(false, true);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_input_pump_real_noto_matches_complete_reference() {
    run_logical(true, true);
}
fn run_logical(hardware: bool, pump: bool) {
    use gpui_box::retained::{input::*, logical::*};
    struct Label;
    impl Component for Label {
        type Props = TextPaint;
        fn mount(
            props: ReadProp<TextPaint>,
            cx: &mut ComponentContext<'_, '_, '_, '_>,
        ) -> Result<(), Error> {
            let node = cx.append_layout(leaf(), 0)?;
            let state = cx.interaction_state(0, node, false, true)?;
            let plan = StylePlan::new(&[StyleRef::new(
                "label",
                vec![
                    StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(60.0)).when(
                        StyleCondition {
                            hovered: Some(true),
                            ..Default::default()
                        },
                        LayoutDimension::Px(125.0),
                    )),
                    StyleDeclaration::Opacity(StyleRule::value(1.0).when(
                        StyleCondition {
                            pressed: Some(true),
                            ..Default::default()
                        },
                        0.6,
                    )),
                    StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(
                        LayoutLength::Px(3.0),
                    ))),
                ],
            )
            .unwrap()]);
            let resolution = cx.computed(1, &[state.dependency()], move |i| {
                plan.resolve_values(
                    &leaf(),
                    &UiProperties::default(),
                    &responsive::LayoutEnvironment::new([300.0, 400.0]),
                    *i.read_prop(state)?,
                )
                .map_err(|_| Error::InvalidStyle)
            })?;
            cx.bind(resolution, Property::style_resolution(node))?;
            let text = cx.computed(2, &[props.dependency()], move |i| {
                Ok(Some(i.read_prop(props)?.clone()))
            })?;
            cx.bind(text, Property::text(node))?;
            let handler = cx.event_handler(
                0,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::PointerDown {
                        let owned = cx.read_prop(props)?.clone();
                        cx.after_commit(move || assert_eq!(owned.color, 0x00ffffff));
                    }
                    Ok(())
                }),
            )?;
            cx.interaction(
                node,
                Some(UiInteraction {
                    focus: UiFocusPolicy::Tab,
                    pointer_handler: Some(handler),
                    ..Default::default()
                }),
            )?;
            Ok(())
        }
    }
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let prepared = raster
        .prepare(shape(&system, "one two three four five six 中文段落"), 1.0)
        .unwrap();
    let root_style = LayoutStyle::column().size(LayoutDimension::Px(180.0), LayoutDimension::Auto);
    let mut ui =
        LogicalUi::new_layout(root_style.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let child = ui
        .edit_local_frame(
            LayoutConstraints::viewport(180.0, 400.0),
            [300.0, 400.0],
            |_| Ok(()),
            |(), edit| {
                Ok(edit.components::<Label>(
                    scope,
                    root,
                    [(
                        0,
                        TextPaint {
                            resource: prepared.clone(),
                            color: 0x00ffffff,
                        },
                    )],
                )?[0])
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let node = ui.renders(child).unwrap()[0];
    let initial = ui.published_local_frame().unwrap().clone();
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("logical feedback Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (event, width, opacity) in [
        (UiInput::PointerMove([5.0; 2]), 125.0, 1.0),
        (UiInput::PrimaryDown([5.0; 2]), 125.0, 0.6),
        (UiInput::PrimaryUp([5.0; 2]), 125.0, 1.0),
        (UiInput::PointerLeave, 60.0, 1.0),
    ] {
        if pump {
            let commit = ui.handle_input(&mut router, event).unwrap();
            assert!(commit.delivered > 0);
        } else {
            let effects = router.route(event).unwrap();
            ui.apply_input_effects(&effects).unwrap();
        }
        let mut full = UiTree::new_layout(root_style.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let text = full
            .append_layout(
                full.root(),
                leaf()
                    .size(LayoutDimension::Px(width), LayoutDimension::Auto)
                    .padding(LayoutEdges::all(LayoutLength::Px(3.0))),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            text,
            Some(TextPaint {
                resource: prepared.clone(),
                color: 0x00ffffff,
            }),
        );
        tx.opacity(text, opacity);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full
            .frame_in_viewport(LayoutConstraints::viewport(180.0, 400.0), [300.0, 400.0])
            .unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(node).unwrap(),
            full.layout_result(text).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [300, 400], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [300, 400], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        if pump {
            let scene = ui.input_scene().unwrap();
            ui.adopt_input_scene(&mut router, scene, UiInputEvidence::ImageInstalled)
                .unwrap();
        } else {
            router
                .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
                .unwrap();
        }
    }
    let expected = pixels(initial.resolve());
    drop((ui, prepared, raster, system));
    assert_eq!(pixels(initial.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let mut resources = UiResources::new();
    resources.text(UiResourceId(1), text.clone()).unwrap();
    let wrap = StyleRef::new(
        "paragraph",
        vec![
            StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(60.0)).when(
                StyleCondition {
                    min_width: Some(200.0),
                    ..Default::default()
                },
                LayoutDimension::calc(-10.0, 0.75),
            )),
            StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(LayoutLength::Px(3.0)))),
            StyleDeclaration::PaddingLeft(StyleRule::value(LayoutLength::Px(4.0))),
        ],
    );
    // Within-definition duplicate shorthand/longhand is rejected. Across
    // definitions it is an intentional caller override.
    assert!(matches!(
        wrap,
        Err(StyleError::DuplicateProperty(StyleProperty::PaddingLeft))
    ));
    let wrap = StyleRef::new(
        "paragraph",
        vec![
            StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(60.0)).when(
                StyleCondition {
                    min_width: Some(200.0),
                    ..Default::default()
                },
                LayoutDimension::calc(-10.0, 0.75),
            )),
            StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(LayoutLength::Px(3.0)))),
        ],
    )
    .unwrap();
    let override_style = StyleRef::new(
        "external",
        vec![
            StyleDeclaration::PaddingLeft(StyleRule::value(LayoutLength::Px(4.0))),
            StyleDeclaration::Opacity(StyleRule::value(1.0).when(
                StyleCondition {
                    hovered: Some(true),
                    ..Default::default()
                },
                0.6,
            )),
        ],
    )
    .unwrap();
    let root_style = LayoutStyle::column().size(LayoutDimension::Px(180.0), LayoutDimension::Auto);
    let mut root = UiNode::new(ROOT, root_style.clone());
    root.children = vec![TEXT];
    let mut child = UiNode::new(TEXT, leaf());
    child.styles = vec![wrap.clone(), override_style.clone()];
    child.content = UiContent::Text {
        resource: UiResourceId(1),
        color: 0x00ffffff,
    };
    let mut snapshot = UiSnapshot::new(110, 1, ROOT, vec![root, child]);
    snapshot.style_environment = responsive::LayoutEnvironment::new([120.0, 400.0]);
    let constraints = LayoutConstraints::viewport(180.0, 400.0);
    let (mut document, first) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints,
        [300.0, 400.0],
    )
    .unwrap();
    let initial_pixels = pixels(first.frame.resolve());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("style IR glyph adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (width, hovered, external) in [
        (120.0, false, true),
        (240.0, false, true),
        (240.0, true, true),
        (120.0, true, false),
        (120.0, false, true),
    ] {
        let batch = UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: 110,
            epoch: 1,
            base_revision: document.revision(),
            revision: document.revision() + 1,
            sequence: document.next_sequence(),
            operations: vec![
                UiOperation::StyleEnvironment(responsive::LayoutEnvironment::new([width, 400.0])),
                UiOperation::StyleState {
                    node: TEXT,
                    value: StyleState {
                        hovered,
                        ..Default::default()
                    },
                },
                UiOperation::Styles {
                    node: TEXT,
                    value: if external {
                        vec![wrap.clone(), override_style.clone()]
                    } else {
                        vec![wrap.clone()]
                    },
                },
            ],
        };
        let output = document
            .apply(
                batch,
                &resources,
                BackendCapabilities::default(),
                constraints,
                [300.0, 400.0],
            )
            .unwrap();
        let used_width = if width >= 200.0 { 125.0 } else { 60.0 };
        let left = if external { 4.0 } else { 3.0 };
        let style = leaf()
            .size(LayoutDimension::Px(used_width), LayoutDimension::Auto)
            .padding(LayoutEdges {
                left: LayoutLength::Px(left),
                ..LayoutEdges::all(LayoutLength::Px(3.0))
            });
        let mut full = UiTree::new_layout(root_style.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let reference = full.append_layout(full.root(), style, 0).unwrap();
        let mut tx = Transaction::default();
        tx.text(
            reference,
            Some(TextPaint {
                resource: text.clone(),
                color: 0x00ffffff,
            }),
        );
        tx.opacity(reference, if external && hovered { 0.6 } else { 1.0 });
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let expected = full.frame_in_viewport(constraints, [300.0, 400.0]).unwrap();
        assert_eq!(
            document.layout_result(TEXT).unwrap(),
            full.layout_result(reference).unwrap()
        );
        assert_eq!(
            document.layout_result(TEXT).unwrap().size[1],
            paragraph
                .layout(Some(used_width - left - 3.0))
                .unwrap()
                .size[1]
                + 6.0
        );
        assert_eq!(pixels(output.frame.resolve()), pixels(expected.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&output.frame, [300, 400], 1.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&expected, [300, 400], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    drop((document, resources, text, paragraph, raster, system));
    assert_eq!(pixels(first.frame.resolve()), initial_pixels);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
