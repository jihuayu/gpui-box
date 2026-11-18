use super::*;
use gpui_box::retained::{input::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const HOST: UiNodeId = UiNodeId::new(1, 1);
const PREFIX: UiNodeId = UiNodeId::new(2, 1);
const TEXT: UiNodeId = UiNodeId::new(3, 1);
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn host(w: f32) -> LayoutStyle {
    fixed(w, 40.0)
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll)
}
fn leaf() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [140, 100],
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
fn focus_reveal_real_noto_uses_final_wrap_and_matches_numeric_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_focus_reveal_real_noto_matches_numeric_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long = raster
        .prepare(
            shape(
                &system,
                "one two three four five six seven 中文段落 repeat one two three four five",
            ),
            1.0,
        )
        .unwrap();
    let short = raster.prepare(shape(&system, "one"), 1.0).unwrap();
    let mut resources = UiResources::new();
    resources.text(UiResourceId(1), long.clone()).unwrap();
    resources.text(UiResourceId(2), short.clone()).unwrap();
    let mut root = UiNode::new(ROOT, fixed(140.0, 100.0));
    root.children = vec![HOST];
    let mut host_node = UiNode::new(HOST, host(80.0));
    host_node.children = vec![PREFIX, TEXT];
    let prefix = UiNode::new(PREFIX, fixed(70.0, 70.0));
    let mut text = UiNode::new(TEXT, leaf());
    text.content = UiContent::Text {
        resource: UiResourceId(1),
        color: 0x00ffffff,
    };
    text.interaction.focus = UiFocusPolicy::Tab;
    let (mut doc, initial) = UiDocument::from_snapshot(
        UiSnapshot::new(904, 1, ROOT, vec![root, host_node, prefix, text]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(140.0, 100.0),
        [140.0, 100.0],
    )
    .unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(initial.input.clone(), UiInputEvidence::Published)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (width, resource, offset) in [
        (80.0, UiResourceId(1), 73.0),
        (80.0, UiResourceId(2), 65.0),
        (120.0, UiResourceId(1), 73.0),
    ] {
        let effects = router.route(UiInput::Focus(Some(TEXT))).unwrap();
        let mut plan = doc.plan_input_scroll(&effects).unwrap();
        plan.batch.operations.extend([
            UiOperation::Scroll {
                node: HOST,
                value: [0.0; 2],
            },
            UiOperation::Layout {
                node: HOST,
                value: Box::new(host(width)),
            },
            UiOperation::Content {
                node: TEXT,
                value: UiContent::Text {
                    resource,
                    color: 0x00ffffff,
                },
            },
        ]);
        let output = doc
            .apply(
                plan.batch,
                &resources,
                BackendCapabilities::default(),
                LayoutConstraints::viewport(140.0, 100.0),
                [140.0, 100.0],
            )
            .unwrap();
        assert_eq!(doc.node(HOST).unwrap().properties.scroll, [0.0, offset]);
        let prepared = if resource == UiResourceId(1) {
            long.clone()
        } else {
            short.clone()
        };
        let mut baseline = UiTree::new_layout(fixed(140.0, 100.0), 0).unwrap();
        baseline.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = baseline
            .append_layout(baseline.root(), host(width), 0)
            .unwrap();
        baseline.append_layout(host, fixed(70.0, 70.0), 0).unwrap();
        let text = baseline.append_layout(host, leaf(), 0).unwrap();
        let mut tx = Transaction::default();
        tx.scroll(host, [0.0, offset]);
        tx.text(
            text,
            Some(TextPaint {
                resource: prepared,
                color: 0x00ffffff,
            }),
        );
        baseline.commit(tx, BackendCapabilities::default()).unwrap();
        let full = baseline.frame([140.0, 100.0]).unwrap();
        assert_eq!(
            doc.layout_result(TEXT).unwrap(),
            baseline.layout_result(text).unwrap()
        );
        assert_eq!(pixels(output.frame.resolve()), pixels(full.clone()));
        assert_eq!(output.input.hit_test([10.0, 10.0]), Some(TEXT));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(
                gpu.submit_local(&output.frame, [140, 100], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [140, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        router
            .adopt(output.input, UiInputEvidence::ImageInstalled)
            .unwrap();
    }
    let expected = pixels(initial.frame.resolve());
    drop((doc, resources, long, short, raster, system));
    assert_eq!(expected, pixels(initial.frame.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
