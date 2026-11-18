use super::*;
use gpui_box::retained::{input::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const HOST: UiNodeId = UiNodeId::new(1, 1);
const TEXT: UiNodeId = UiNodeId::new(2, 1);
fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Px(120.0), LayoutDimension::Px(100.0))
}
fn host() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(80.0), LayoutDimension::Px(40.0))
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
            size: [120, 100],
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
fn wheel_scroll_real_noto_matches_direct_layout_baseline_and_complete_pixels() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_wheel_scroll_real_noto_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(
        &system,
        "one two three four five six seven eight nine 中文段落 repeat one two three four five six",
    );
    let mut raster = GlyphRasterizer::new(system.clone());
    let prepared = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let mut resources = UiResources::new();
    resources.text(UiResourceId(1), prepared.clone()).unwrap();
    let mut root_node = UiNode::new(ROOT, root());
    root_node.children = vec![HOST];
    let mut host_node = UiNode::new(HOST, host());
    host_node.children = vec![TEXT];
    host_node.properties.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 40.0,
    });
    let mut text_node = UiNode::new(TEXT, leaf());
    text_node.content = UiContent::Text {
        resource: UiResourceId(1),
        color: 0x00ffffff,
    };
    let (mut document, initial) = UiDocument::from_snapshot(
        UiSnapshot::new(802, 1, ROOT, vec![root_node, host_node, text_node]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(120.0, 100.0),
        [120.0, 100.0],
    )
    .unwrap();
    assert!(initial.input.scroll_state(HOST).unwrap().maximum[1] > 20.0);
    let layout = document.layout_result(TEXT).unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(initial.input.clone(), UiInputEvidence::Published)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("wheel real Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for delta in [10.0, 10.0, 1000.0, -15.0] {
        let effects = router
            .route(UiInput::Wheel {
                position: [10.0, 10.0],
                delta: [0.0, delta],
            })
            .unwrap();
        let plan = document.plan_input_scroll(&effects).unwrap();
        let publication = document
            .apply(
                plan.batch,
                &resources,
                BackendCapabilities::default(),
                LayoutConstraints::viewport(120.0, 100.0),
                [120.0, 100.0],
            )
            .unwrap();
        let offset = document.node(HOST).unwrap().properties.scroll;
        let mut reference = UiTree::new_layout(root(), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = reference
            .append_layout(reference.root(), host(), 0)
            .unwrap();
        let leaf = reference.append_layout(host, leaf(), 0).unwrap();
        let mut tx = Transaction::default();
        tx.scroll(host, offset);
        tx.clip(
            host,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            }),
        );
        tx.text(
            leaf,
            Some(TextPaint {
                resource: prepared.clone(),
                color: 0x00ffffff,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference.frame([120.0, 100.0]).unwrap();
        assert_eq!(
            document.layout_result(TEXT).unwrap(),
            layout,
            "scroll does not change paragraph layout/baseline"
        );
        assert_eq!(
            document.layout_result(TEXT).unwrap(),
            reference.layout_result(leaf).unwrap()
        );
        assert_eq!(pixels(publication.frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(
                gpu.submit_local(&publication.frame, [120, 100], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [120, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        router
            .adopt(publication.input, UiInputEvidence::ImageInstalled)
            .unwrap();
    }
    let expected = pixels(initial.frame.resolve());
    drop((document, resources, prepared, paragraph, raster, system));
    assert_eq!(expected, pixels(initial.frame.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
