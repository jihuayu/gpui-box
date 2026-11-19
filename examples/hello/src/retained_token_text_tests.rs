//! Owned token programs drive existing Style IR, real text and full Flex output.
use super::*;
use gpui_box::retained::{style::*, tokens::*, ui_ir::*};
fn key<T: TokenType>(name: &str) -> TokenKey<T> {
    TokenKey::new("app", name).unwrap()
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 400],
            scale: 1.25,
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
fn token_style_real_noto_flex_matches_literal_reference_and_unread_token_skips_style() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_token_style_real_noto_matches_literal_full_flex() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let resource = raster
        .prepare(
            shape(&system, "one two three four five 中文 token flex"),
            1.25,
        )
        .unwrap();
    let width = key::<LayoutLength>("width");
    let pad = key::<LayoutEdges>("padding");
    let color = key::<u32>("surface");
    let opacity = key::<f32>("opacity");
    let unused = key::<f32>("unused");
    let schema = TokenSchema::new(vec![
        TokenDefinition::value(width.clone(), LayoutLength::Px(90.0)),
        TokenDefinition::value(pad.clone(), LayoutEdges::all(LayoutLength::Px(4.0))),
        TokenDefinition::value(color.clone(), 0x225588ff),
        TokenDefinition::value(opacity.clone(), 1.0),
        TokenDefinition::value(unused.clone(), 0.0),
    ])
    .unwrap();
    let sheet = StyleRef::new(
        "token-label",
        vec![
            StyleDeclaration::Width(StyleRule::expression(
                width.expression().dimension().unwrap(),
            )),
            StyleDeclaration::Padding(StyleRule::expression(pad.expression())),
            StyleDeclaration::Color(StyleRule::expression(color.expression())),
            StyleDeclaration::Opacity(StyleRule::expression(opacity.expression())),
        ],
    )
    .unwrap();
    let root = UiNodeId::new(0, 1);
    let title = UiNodeId::new(1, 1);
    let footer = UiNodeId::new(2, 1);
    let mut r = UiNode::new(root, LayoutStyle::column());
    r.children = vec![title, footer];
    let mut t = UiNode::new(title, LayoutStyle::column().flex_shrink(0.0));
    t.styles = vec![sheet];
    t.tokens = schema.resolve(vec![]).unwrap();
    t.content = UiContent::Text {
        resource: UiResourceId(1),
        color: 0xffffffff,
    };
    let mut f = UiNode::new(
        footer,
        LayoutStyle::column()
            .size(LayoutDimension::Px(70.0), LayoutDimension::Px(12.0))
            .flex_shrink(0.0),
    );
    f.properties.color = 0x55aa33ff;
    let mut resources = UiResources::new();
    resources.text(UiResourceId(1), resource.clone()).unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let (mut doc, initial) = UiDocument::from_snapshot(
        UiSnapshot::new(77, 1, root, vec![r, t, f]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints,
        [240.0, 320.0],
    )
    .unwrap();
    let old_pixels = pixels(initial.frame.resolve());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (phase, (w, p, c)) in [
        (90.0, 4.0, 0x225588ff),
        (145.0, 8.0, 0x775533ff),
        (65.0, 2.0, 0x225544ff),
        (65.0, 2.0, 0x225544ff),
    ]
    .into_iter()
    .enumerate()
    {
        let values = schema
            .resolve(vec![
                TokenDefinition::value(width.clone(), LayoutLength::Px(w)),
                TokenDefinition::value(pad.clone(), LayoutEdges::all(LayoutLength::Px(p))),
                TokenDefinition::value(color.clone(), c),
                TokenDefinition::value(unused.clone(), phase as f32),
            ])
            .unwrap();
        let batch = UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: 77,
            epoch: 1,
            base_revision: doc.revision(),
            revision: doc.revision() + 1,
            sequence: doc.next_sequence(),
            operations: vec![UiOperation::Tokens {
                node: title,
                value: values,
            }],
        };
        let output = doc
            .apply(
                batch,
                &resources,
                BackendCapabilities::default(),
                constraints,
                [240.0, 320.0],
            )
            .unwrap();
        if phase == 3 {
            assert_eq!(output.planning.styles_evaluated, 0);
        }
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let node = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(w), LayoutDimension::Auto)
                    .padding(LayoutEdges::all(LayoutLength::Px(p)))
                    .flex_shrink(0.0),
                c,
            )
            .unwrap();
        full.append_layout(
            full.root(),
            LayoutStyle::column()
                .size(LayoutDimension::Px(70.0), LayoutDimension::Px(12.0))
                .flex_shrink(0.0),
            0x55aa33ff,
        )
        .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            node,
            Some(TextPaint {
                resource: resource.clone(),
                color: 0xffffffff,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [240.0, 320.0]).unwrap();
        let actual = output.frame.resolve();
        for (a, b) in actual.items.iter().zip(reference.items.iter()) {
            assert_eq!((a.bounds, a.content_bounds), (b.bounds, b.content_bounds));
        }
        assert!(
            pixels(actual) == pixels(reference.clone()),
            "token CPU complete image phase {phase}"
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            if phase == 0 {
                eprintln!("token style adapter: {:?}", gpu.adapter_info());
            }
            gpu.submit_local(&output.frame, [300, 400], 1.25).unwrap();
            assert!(
                gpu.image().unwrap().as_raw()
                    == gpu
                        .reference_image(&reference, [300, 400], 1.25)
                        .unwrap()
                        .as_raw(),
                "token GPU complete image phase {phase}"
            );
        }
    }
    let snapshot = doc.snapshot();
    let old = doc.published_frame();
    let bad = schema
        .resolve(vec![TokenDefinition::value(opacity, 2.0)])
        .unwrap();
    let batch = UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 77,
        epoch: 1,
        base_revision: doc.revision(),
        revision: doc.revision() + 1,
        sequence: doc.next_sequence(),
        operations: vec![UiOperation::Tokens {
            node: title,
            value: bad,
        }],
    };
    assert!(
        doc.apply(
            batch,
            &resources,
            BackendCapabilities::default(),
            constraints,
            [240.0, 320.0]
        )
        .is_err()
    );
    assert_eq!(snapshot, doc.snapshot());
    assert_eq!(old.revision(), doc.published_frame().revision());
    drop((doc, resources, raster, system));
    assert!(old_pixels == pixels(initial.frame.resolve()));
}
