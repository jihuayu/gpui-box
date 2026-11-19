//! Render-owned animation uses the same real text/Flex/resources on both consumers.
use super::*;
use gpui_box::retained::animation::*;
use std::time::{Duration, Instant};
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 250],
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
fn render_animation_real_noto_cpu_matches_fresh_full_flex_and_old_output_survives() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires explicit WGPU adapter"]
fn hardware_render_animation_real_noto_matches_fresh_full_flex() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let resource = raster
        .prepare(
            shape(&system, "动画由 render owner 采样 one two three four five"),
            1.25,
        )
        .unwrap();
    let (mut tree, parent, sibling) = fixture(resource.clone(), 90.0);
    if hardware {
        let mut tx = Transaction::default();
        tx.shadows(
            parent,
            vec![BoxShadow {
                offset: [2.0, 2.0],
                blur_radius: 2.0,
                color: 0x00000060,
                ..Default::default()
            }],
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
    }
    let source = tree.local_frame([240.0, 200.0]).unwrap();
    let source_packet = source.resolve();
    let saved = (!hardware).then(|| pixels(source_packet.clone()));
    let now = Instant::now();
    let mut runtime = AnimationRuntime::new(source, AnimationLimits::default()).unwrap();
    runtime
        .apply(
            AnimationBatch {
                scene: runtime.scene(),
                sequence: 1,
                controls: vec![
                    AnimationControl::Animate(AnimationSpec {
                        id: 1,
                        node: parent,
                        destination: AnimationValue::Translation([30.0, 8.0]),
                        curve: AnimationCurve::Linear,
                        duration: Duration::from_secs(1),
                        start: AnimationStart::Now,
                        velocity: RetargetVelocity::Reset,
                    }),
                    AnimationControl::Animate(AnimationSpec {
                        id: 2,
                        node: parent,
                        destination: AnimationValue::Opacity(0.5),
                        curve: AnimationCurve::Linear,
                        duration: Duration::from_secs(1),
                        start: AnimationStart::Now,
                        velocity: RetargetVelocity::Reset,
                    }),
                ],
            },
            now,
        )
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = runtime.sample(now).unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut final_output = None;
    for (phase, millis) in [0, 250, 500, 750, 1000].into_iter().enumerate() {
        let time = now + Duration::from_millis(millis);
        let width = if phase >= 2 { 130.0 } else { 90.0 };
        if phase == 2 {
            let style = label_style(width);
            let mut tx = Transaction::default();
            tx.layout_style(parent, style);
            tx.color(sibling, 0x993344ff);
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            runtime
                .set_scene(tree.local_frame([240.0, 200.0]).unwrap(), time)
                .unwrap();
        }
        let output = runtime.sample(time).unwrap();
        let progress = millis as f32 / 1000.0;
        let (mut full, node, footer) = fixture(resource.clone(), width);
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let mut tx = Transaction::default();
        tx.translation(node, [30.0 * progress, 8.0 * progress]);
        tx.opacity(node, 1.0 - 0.5 * progress);
        if hardware {
            tx.shadows(
                node,
                vec![BoxShadow {
                    offset: [2.0, 2.0],
                    blur_radius: 2.0,
                    color: 0x00000060,
                    ..Default::default()
                }],
            );
        }
        if phase >= 2 {
            tx.color(footer, 0x993344ff);
        }
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame([240.0, 200.0]).unwrap();
        let actual = output.frame().resolve();
        for (a, b) in actual.items.iter().zip(reference.items.iter()) {
            assert_eq!(
                (a.bounds, a.content_bounds, a.clip, a.content_clip),
                (b.bounds, b.content_bounds, b.clip, b.content_clip)
            );
        }
        if !hardware {
            assert!(
                pixels(actual) == pixels(reference.clone()),
                "complete animated CPU image phase {phase}"
            );
        }
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            if phase == 0 {
                eprintln!("render animation adapter: {:?}", gpu.adapter_info());
            }
            let submission = gpu
                .submit_local(output.frame(), [300, 250], 1.25)
                .unwrap()
                .unwrap();
            let image = gpu.image().unwrap();
            assert!(
                image.as_raw()
                    == gpu
                        .reference_image(&reference, [300, 250], 1.25)
                        .unwrap()
                        .as_raw(),
                "complete animated GPU image phase {phase}"
            );
            if phase == 4 {
                final_output = Some((output.clone(), image));
            }
            // Changed opacity takes the ordinary isolation/composition route.
            if phase > 0 {
                assert!(submission.full_resync);
            }
        }
    }
    assert_eq!(runtime.drain_notices().count(), 2);
    drop((runtime, tree, raster, system, resource));
    if let Some(saved) = saved {
        assert!(saved == pixels(old.source().resolve()));
    }
    assert_eq!(
        format!("{:?}", source_packet.items),
        format!("{:?}", old.source().resolve().items)
    );
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        // Reset consumes a complete sampled output with its own ordered stream.
        let (last, image) = final_output.unwrap();
        gpu.reset().unwrap();
        gpu.submit_local(last.frame(), [300, 250], 1.25).unwrap();
        assert!(image.as_raw() == gpu.image().unwrap().as_raw());
    }
}
fn fixture(resource: TextResource, width: f32) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(240.0), LayoutDimension::Px(200.0)),
        0x17202fff,
    )
    .unwrap();
    let node = tree
        .append_layout(tree.root(), label_style(width), 0x225588ff)
        .unwrap();
    let sibling = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column()
                .size(LayoutDimension::Px(60.0), LayoutDimension::Px(12.0))
                .flex_shrink(0.0),
            0x55aa33ff,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.text(
        node,
        Some(TextPaint {
            resource,
            color: 0xffffffff,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, node, sibling)
}
fn label_style(width: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Auto)
        .padding(LayoutEdges::all(LayoutLength::Px(4.0)))
        .flex_shrink(0.0)
}
