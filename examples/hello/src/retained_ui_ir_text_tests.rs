use super::*;
use gpui_box::retained::ui_ir::*;

const ROOT: UiNodeId = UiNodeId::new(0, 1);
const TEXT: UiNodeId = UiNodeId::new(1, 1);
fn grid(width: f32) -> LayoutStyle {
    LayoutStyle::grid(GridStyle::default().columns(vec![
        GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(1.0)).into(),
        GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(2.0)).into(),
    ]))
    .size(LayoutDimension::Px(width), LayoutDimension::Auto)
}
fn leaf() -> LayoutStyle {
    LayoutStyle::row()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .grid_column(GridPlacement::Line(2), GridPlacement::Auto)
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
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
fn owned_ui_batches_rewrap_real_text_and_switch_intrinsic_resource_owners() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_owned_ui_ir_real_text_matches_direct_tree() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let image = ImageResource::rgba8([10, 5], vec![0, 255, 0, 255].repeat(50)).unwrap();
    let mut resources = UiResources::new();
    resources.text(UiResourceId(10), text.clone()).unwrap();
    resources.image(UiResourceId(11), image.clone()).unwrap();
    let root = || {
        let mut node = UiNode::new(ROOT, grid(180.0));
        node.children = vec![TEXT];
        node
    };
    let mut node = UiNode::new(TEXT, leaf());
    node.content = UiContent::Text {
        resource: UiResourceId(10),
        color: 0x00ffffff,
    };
    let (mut document, first) = UiDocument::from_snapshot(
        UiSnapshot::new(100, 1, ROOT, vec![root(), node]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(180.0, 400.0),
        [300.0, 400.0],
    )
    .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("UI IR real glyph adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old_pixels = pixels(first.frame.resolve());
    for (step, width) in [180.0, 240.0, 120.0, 180.0, 180.0, 180.0, 180.0, 180.0]
        .into_iter()
        .enumerate()
    {
        let content = match step {
            4 => UiContent::Image(UiResourceId(11)),
            5 => UiContent::Fixed {
                size: [10.0, 20.0],
                first_baseline: Some(5.0),
                last_baseline: Some(15.0),
            },
            6 => UiContent::Box,
            _ => UiContent::Text {
                resource: UiResourceId(10),
                color: if step == 1 { 0xff0000ff } else { 0x00ffffff },
            },
        };
        let batch = UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: 100,
            epoch: 1,
            base_revision: document.revision(),
            revision: document.revision() + 1,
            sequence: document.next_sequence(),
            operations: vec![
                UiOperation::Layout {
                    node: ROOT,
                    value: Box::new(grid(width)),
                },
                UiOperation::Content {
                    node: TEXT,
                    value: content.clone(),
                },
            ],
        };
        let bounds = LayoutConstraints::viewport(width, 400.0);
        let output = document
            .apply(
                batch,
                &resources,
                BackendCapabilities::default(),
                bounds,
                [300.0, 400.0],
            )
            .unwrap();
        let mut full = UiTree::new_layout(grid(width), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let reference = full.append_layout(full.root(), leaf(), 0).unwrap();
        let mut tx = Transaction::default();
        match content {
            UiContent::Text { color, .. } => tx.text(
                reference,
                Some(TextPaint {
                    resource: text.clone(),
                    color,
                }),
            ),
            UiContent::Image(_) => tx.image(reference, Some(image.clone())),
            UiContent::Fixed {
                size,
                first_baseline,
                last_baseline,
            } => tx.measure(
                reference,
                Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                    width: size[0],
                    height: size[1],
                    first_baseline,
                    last_baseline,
                })),
            ),
            UiContent::Box => {}
            UiContent::Measure(_) => {
                unreachable!("this resource-transition fixture has no custom measure")
            }
        }
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference_frame = full.frame_in_viewport(bounds, [300.0, 400.0]).unwrap();
        assert_eq!(
            document.layout_result(TEXT).unwrap(),
            full.layout_result(reference).unwrap()
        );
        assert_eq!(
            pixels(output.frame.resolve()),
            pixels(reference_frame.clone())
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&output.frame, [300, 400], 1.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference_frame, [300, 400], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    drop((document, resources, text, image, system, raster, paragraph));
    assert_eq!(pixels(first.frame.resolve()), old_pixels);
    assert_eq!(first.hit_test(65.0, 5.0), Some(TEXT));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
