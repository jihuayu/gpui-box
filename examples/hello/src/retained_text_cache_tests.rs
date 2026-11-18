//! Owner preparation cache obeys input identity, bounds and immutable frames.
use super::*;
use gpui_box::retained::{logical::*, text_style::*};
use std::rc::Rc;

fn typography(size: f32) -> TextTypography {
    TextTypography {
        font: FontDescriptor {
            family: "Noto Sans CJK SC".into(),
            ..Default::default()
        },
        font_size: size,
        line_height: size * 2.0,
    }
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [240, 280],
            scale: 1.25,
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
fn owner_text_cache_keys_complete_inputs_and_lru_retains_recent_resources() {
    let system = Rc::new(text_system());
    let prep = GpuiTextPreparer::new(system.clone());
    prep.set_cache_limits(TextCacheLimits {
        max_entries: 2,
        max_payload_bytes: usize::MAX,
    });
    let style = typography(16.0);
    let a = prep.prepare("A 中文", &style, 1.25, 0).unwrap();
    let b = prep.prepare("B 中文", &style, 1.25, 0).unwrap();
    assert_eq!(
        a,
        prep.prepare(&String::from("A 中文"), &style, 1.25, 0)
            .unwrap()
    );
    let c = prep.prepare("C 中文", &style, 1.25, 0).unwrap(); // B is LRU
    assert_eq!(a, prep.prepare("A 中文", &style, 1.25, 0).unwrap());
    assert_ne!(b, prep.prepare("B 中文", &style, 1.25, 0).unwrap());
    let s = prep.cache_stats();
    assert_eq!(s.entries, 2);
    assert_eq!(s.hits, 2);
    assert_eq!(s.evictions, 2);
    // Old immutable resources are unaffected by eviction or budget changes.
    assert!(c.chunk(80.0).is_ok());
    prep.set_cache_limits(TextCacheLimits {
        max_entries: 16,
        max_payload_bytes: usize::MAX,
    });
    let base = prep.prepare("key 中文", &style, 1.25, 0).unwrap();
    let mut variants = vec![
        (style.clone(), 1.0, 0),
        (typography(18.0), 1.25, 0),
        (style.clone(), 1.25, 1),
    ];
    let mut line = style.clone();
    line.line_height += 2.0;
    variants.push((line, 1.25, 0));
    let mut family = style.clone();
    family.font.family = ".SystemUIFont".into();
    variants.push((family, 1.25, 0));
    let mut weight = style.clone();
    weight.font.weight = 700;
    variants.push((weight, 1.25, 0));
    let mut slant = style.clone();
    slant.font.slant = FontSlant::Italic;
    variants.push((slant, 1.25, 0));
    for (t, scale, revision) in variants {
        let other = prep.prepare("key 中文", &t, scale, revision).unwrap();
        assert_ne!(base, other);
        assert_eq!(
            other,
            prep.prepare("key 中文", &t, scale, revision).unwrap()
        );
    }
    assert_ne!(base, prep.prepare("key 不同", &style, 1.25, 0).unwrap());
    let before = prep.cache_stats();
    assert!(prep.prepare("tab\t", &style, 1.25, 0).is_err());
    assert_eq!(prep.cache_stats().entries, before.entries);
    assert!(prep.prepare("key 中文", &style, f32::NAN, 0).is_err());
    let epoch = system.font_epoch();
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    assert_ne!(epoch, system.font_epoch());
    let fresh = prep.prepare("key 中文", &style, 1.25, 0).unwrap();
    assert_ne!(base, fresh);
    assert_eq!(prep.cache_stats().font_invalidations, 1);
    assert_eq!(prep.cache_stats().entries, 1);
    assert!(base.chunk(80.0).is_ok());
}
#[test]
fn owner_text_cache_payload_limit_disable_and_clear_preserve_external_resources() {
    let prep = GpuiTextPreparer::new(Rc::new(text_system()));
    let style = typography(16.0);
    let a = prep.prepare("payload 中文", &style, 1.25, 0).unwrap();
    let bytes = prep.cache_stats().payload_bytes;
    assert!(bytes > 0);
    prep.set_cache_limits(TextCacheLimits {
        max_entries: 10,
        max_payload_bytes: bytes - 1,
    });
    assert_eq!(prep.cache_stats().entries, 0);
    let b = prep.prepare("payload 中文", &style, 1.25, 0).unwrap();
    assert_ne!(a, b);
    assert_eq!(prep.cache_stats().entries, 0);
    assert_eq!(prep.cache_stats().bypassed, 1);
    prep.set_cache_limits(TextCacheLimits {
        max_entries: 10,
        max_payload_bytes: bytes,
    });
    let c = prep.prepare("payload 中文", &style, 1.25, 0).unwrap();
    assert_eq!(prep.cache_stats().payload_bytes, bytes);
    assert_eq!(c, prep.prepare("payload 中文", &style, 1.25, 0).unwrap());
    prep.clear_cache();
    assert_eq!(prep.cache_stats().payload_bytes, 0);
    assert_ne!(c, prep.prepare("payload 中文", &style, 1.25, 0).unwrap());
    prep.set_cache_limits(TextCacheLimits {
        max_entries: 0,
        max_payload_bytes: usize::MAX,
    });
    let d = prep.prepare("payload 中文", &style, 1.25, 0).unwrap();
    assert_eq!(prep.cache_stats().entries, 0);
    assert!(d.chunk(80.0).is_ok());
    assert!(a.chunk(80.0).is_ok());
}
#[test]
fn repeated_text_components_share_cached_resources_and_match_uncached_full_flex() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_repeated_text_cache_matches_uncached_full_flex() {
    run(true);
}
fn run(hardware: bool) {
    let system = Rc::new(text_system());
    let prep = Rc::new(GpuiTextPreparer::new(system.clone()));
    let uncached = GpuiTextPreparer::new(system);
    uncached.set_cache_limits(TextCacheLimits {
        max_entries: 0,
        max_payload_bytes: 0,
    });
    let mut p = TextProps::new("same 中文 label", TextPreparation::new(prep.clone()));
    p.scale = 1.25;
    p.style.font_family = TextValue::Value("Noto Sans CJK SC".into());
    p.layout = LayoutStyle::column()
        .size(LayoutDimension::Px(100.0), LayoutDimension::Auto)
        .flex_shrink(0.0);
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let scopes = ui
        .edit_local_frame(
            constraints,
            [192.0, 224.0],
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, (0..4).map(|key| (key, p.clone()))),
        )
        .unwrap()
        .0
        .result
        .value;
    let nodes: Vec<_> = scopes.iter().map(|s| ui.renders(*s).unwrap()[0]).collect();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    let mut first = None;
    for (phase, size) in [16.0, 20.0, 16.0, 16.0, 16.0].into_iter().enumerate() {
        if phase == 3 {
            prep.clear_cache();
            p.font_revision = 1;
        }
        if phase == 4 {
            p.layout = p
                .layout
                .clone()
                .size(LayoutDimension::Px(70.0), LayoutDimension::Auto);
        }
        p.style.font_size = TextValue::Value(size);
        ui.edit(
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, (0..4).map(|key| (key, p.clone()))),
        )
        .unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        let actual = frame.resolve();
        let resources: Vec<_> = actual
            .items
            .iter()
            .filter_map(|item| item.text.as_ref().map(|p| p.resource.clone()))
            .collect();
        assert_eq!(resources.len(), 4);
        assert!(resources.iter().all(|r| r == &resources[0]));
        if phase == 0 {
            first = Some(resources[0].clone());
        }
        if phase == 2 {
            assert_eq!(first.as_ref(), Some(&resources[0]));
        }
        assert_eq!(prep.prepared_resources(), [1, 2, 2, 3, 3][phase]);
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let mut reference_nodes = vec![];
        for _ in 0..4 {
            let node = full
                .append_layout(full.root(), p.layout.clone(), 0)
                .unwrap();
            let resource = uncached
                .prepare(&p.content, &typography(size), 1.25, p.font_revision)
                .unwrap();
            let mut tx = Transaction::default();
            tx.text(
                node,
                Some(TextPaint {
                    resource,
                    color: 0xffffffff,
                }),
            );
            full.commit(tx, BackendCapabilities::default()).unwrap();
            reference_nodes.push(node);
        }
        let reference = full.frame_in_viewport(constraints, [192.0, 224.0]).unwrap();
        for (node, reference_node) in nodes.iter().zip(reference_nodes) {
            assert_eq!(
                ui.layout_result(*node).unwrap(),
                full.layout_result(reference_node).unwrap()
            );
        }
        assert!(
            pixels(actual) == pixels(reference.clone()),
            "cached CPU full picture phase {phase}"
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            if phase == 0 {
                eprintln!("text cache adapter: {:?}", gpu.adapter_info());
            }
            gpu.submit_local(&frame, [240, 280], 1.25).unwrap();
            assert!(
                gpu.image().unwrap().as_raw()
                    == gpu
                        .reference_image(&reference, [240, 280], 1.25)
                        .unwrap()
                        .as_raw(),
                "cached GPU full picture phase {phase}"
            );
        }
        assert_eq!(
            scopes
                .iter()
                .map(|s| ui.renders(*s).unwrap()[0])
                .collect::<Vec<_>>(),
            nodes
        );
    }
    drop((ui, p, prep));
    assert!(old_pixels == pixels(old.resolve()));
}
