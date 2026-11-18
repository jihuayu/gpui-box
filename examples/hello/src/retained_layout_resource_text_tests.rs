use super::*;
use gpui_box::retained::{input::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const TEXT: UiNodeId = UiNodeId::new(1, 1);
const FOOTER: UiNodeId = UiNodeId::new(2, 1);
#[derive(Debug)]
struct Stack;
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let width = input
            .known_size
            .width
            .or(match input.available.width {
                LayoutAvailableSpace::Definite(v) => Some(v),
                _ => None,
            })
            .unwrap_or(100.0);
        let mut y = 0.0;
        let mut placements = vec![];
        let mut baseline = None;
        let mut last = None;
        for child in 0..children.len() {
            let slot = ChildLayoutInput::new(
                LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(width),
                    LayoutAvailableSpace::Unbounded,
                )
                .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
            );
            let result = children.measure_baselines(child, slot)?;
            if child == 0 {
                baseline = result.baseline;
                last = result.last_baseline;
            }
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, y],
            });
            let gap = children
                .parent_data(child)?
                .as_ref()
                .and_then(|v| v.get::<f32>())
                .copied()
                .unwrap_or(0.0);
            y += result.size[1] + if child + 1 < children.len() { gap } else { 0.0 };
        }
        Ok(ParentLayoutOutput {
            size: [width, input.known_size.height.unwrap_or(y)],
            baseline,
            last_baseline: last,
            placements,
        })
    }
}
fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn leaf() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
fn owned_parent_resources_measure_real_noto_and_publish_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_owned_parent_resources_real_noto_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let text = raster
        .prepare(
            shape(
                &system,
                "one two three four five 中文段落 repeat one two three four",
            ),
            1.0,
        )
        .unwrap();
    let mut resources = UiResources::new();
    resources
        .parent_layout(UiResourceId(1), ParentLayoutSnapshot::new(Stack))
        .unwrap();
    resources
        .parent_data(UiResourceId(2), ParentData::new(4.0f32))
        .unwrap();
    resources
        .parent_data(UiResourceId(4), ParentData::new(9.0f32))
        .unwrap();
    resources.text(UiResourceId(3), text.clone()).unwrap();
    let mut host = UiNode::new(ROOT, root());
    host.children = vec![TEXT, FOOTER];
    host.parent_layout = Some(UiResourceId(1));
    let mut label = UiNode::new(TEXT, leaf());
    label.content = UiContent::Text {
        resource: UiResourceId(3),
        color: 0x00ffffff,
    };
    label.parent_data = Some(UiResourceId(2));
    label.interaction.focus = UiFocusPolicy::Tab;
    let mut footer = UiNode::new(
        FOOTER,
        leaf().size(LayoutDimension::Auto, LayoutDimension::Px(7.0)),
    );
    footer.properties.color = 0xff0000ff;
    let (mut doc, initial) = UiDocument::from_snapshot(
        UiSnapshot::new(981, 1, ROOT, vec![host, label, footer]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints(80.0),
        [180.0; 2],
    )
    .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (width, gap, data) in [
        (80.0, 4.0, UiResourceId(2)),
        (140.0, 4.0, UiResourceId(2)),
        (100.0, 9.0, UiResourceId(4)),
    ] {
        let batch = UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: 981,
            epoch: 1,
            base_revision: doc.revision(),
            revision: doc.revision() + 1,
            sequence: doc.next_sequence(),
            operations: vec![UiOperation::ParentData {
                node: TEXT,
                resource: Some(data),
            }],
        };
        let output = doc
            .apply(
                batch,
                &resources,
                BackendCapabilities::default(),
                constraints(width),
                [180.0; 2],
            )
            .unwrap();
        let mut reference = UiTree::new_layout(root().row_gap(LayoutLength::Px(gap)), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let label = reference
            .append_layout(reference.root(), leaf(), 0)
            .unwrap();
        reference
            .append_layout(
                reference.root(),
                leaf().size(LayoutDimension::Auto, LayoutDimension::Px(7.0)),
                0xff0000ff,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            label,
            Some(TextPaint {
                resource: text.clone(),
                color: 0x00ffffff,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        assert_eq!(
            doc.layout_result(TEXT).unwrap(),
            reference.layout_result(label).unwrap()
        );
        assert_eq!(pixels(output.frame.resolve()), pixels(full.clone()));
        let query = doc
            .dry_measure_node_baselines(TEXT, ChildLayoutInput::new(constraints(width)))
            .unwrap();
        assert_eq!(query.size, doc.layout_result(TEXT).unwrap().size);
        assert_eq!(query.baseline, doc.layout_result(TEXT).unwrap().baseline);
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(
                gpu.submit_local(&output.frame, [180, 180], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    let old = pixels(initial.frame.resolve());
    drop((doc, resources, text, raster, system));
    assert_eq!(old, pixels(initial.frame.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
