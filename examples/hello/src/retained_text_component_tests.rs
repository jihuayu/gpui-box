//! Automatic logical text inheritance uses ordinary owned paragraph/Flex output.
use super::*;
use gpui_box::retained::{logical::*, text_style::*};
use std::rc::Rc;

struct Footer;
impl Component for Footer {
    type Props = ();
    fn mount(_: ReadProp<()>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        cx.append_layout(
            LayoutStyle::column()
                .size(LayoutDimension::Px(70.0), LayoutDimension::Px(12.0))
                .flex_shrink(0.0),
            0x55aa33ff,
        )?;
        Ok(())
    }
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
fn leaf(ui: &LogicalUi, mut scope: ScopeId) -> (ScopeId, NodeId) {
    while let Some(child) = ui.children(scope).unwrap().first() {
        scope = *child;
    }
    (scope, ui.renders(scope).unwrap()[0])
}
#[test]
fn inherited_text_real_noto_matches_full_flex_and_color_does_not_prepare_or_layout() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_inherited_text_real_noto_matches_full_flex() {
    run(true);
}
fn run(hardware: bool) {
    let system = Rc::new(text_system());
    let preparer = Rc::new(GpuiTextPreparer::new(system.clone()));
    let preparation = TextPreparation::new(preparer.clone());
    let mut text = TextProps::new("one two three 中文自动继承 four five", preparation);
    text.layout = LayoutStyle::column()
        .size(LayoutDimension::Px(90.0), LayoutDimension::Auto)
        .flex_shrink(0.0);
    text.scale = 1.25;
    type Inner = TextStyleScope<Text>;
    type Outer = TextStyleScope<Inner>;
    let mut props = TextStyleScopeProps::<Inner> {
        style: TextStyle {
            font_family: TextValue::Value("Noto Sans CJK SC".into()),
            font_size: TextValue::Value(16.0),
            color: TextValue::Value(0xffffffff),
            ..Default::default()
        },
        child: TextStyleScopeProps {
            style: TextStyle {
                color: TextValue::Value(0x55ccffff),
                ..Default::default()
            },
            child: text,
        },
    };
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let (outer, footer, site) = ui
        .edit_local_frame(
            constraints,
            [240.0, 320.0],
            |_| Ok(()),
            |(), e| {
                let sites = e.component_sites(root, &[0, 1])?;
                let outer = e.components_at::<Outer>(sites[0], host, [(0, props.clone())])?[0];
                let footer = e.components_at::<Footer>(sites[1], host, [(0, ())])?[0];
                Ok((outer, footer, sites[0]))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let (text_scope, node) = leaf(&ui, outer);
    let footer = ui.renders(footer).unwrap()[0];
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let initial = ui.published_local_frame().unwrap().clone();
    let initial_bytes = pixels(initial.resolve());
    let mut previous_resource = None;
    let mut previous_runs = 0;
    for phase in 0..9 {
        match phase {
            1 => props.style.color = TextValue::Value(0xffcc88ff), // masked by inner
            2 => props.child.style.color = TextValue::Inherit,
            3 => {
                props.style.font_size = TextValue::Value(20.0);
                props.child.child.layout = LayoutStyle::column()
                    .size(LayoutDimension::Px(120.0), LayoutDimension::Auto)
                    .flex_shrink(0.0);
            }
            4 => {
                props.child.child.style.font_size = TextValue::Value(14.0);
                props.child.child.style.line_height = TextValue::Value(TextLineHeight::Px(32.0));
            }
            5 => props.style.font_size = TextValue::Value(28.0), // local metrics override
            6 => props.child.child.content = "short 中文".into(),
            7 => props.child.child.font_revision += 1,
            8 => {
                props.child.child.layout = LayoutStyle::column()
                    .size(LayoutDimension::Px(70.0), LayoutDimension::Auto)
                    .flex_shrink(0.0)
            }
            _ => {}
        }
        ui.edit(
            |_| Ok(()),
            |(), e| e.components_at::<Outer>(site, host, [(0, props.clone())]),
        )
        .unwrap();
        assert_eq!(leaf(&ui, outer), (text_scope, node));
        let frame = ui.published_local_frame().unwrap().clone();
        let actual = frame.resolve();
        let resource = actual
            .items
            .iter()
            .find(|p| p.node == node)
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .resource
            .clone();
        let expected_prepared = [1, 1, 1, 2, 3, 3, 4, 5, 5][phase];
        assert_eq!(
            preparer.prepared_resources(),
            expected_prepared,
            "phase {phase}"
        );
        if matches!(phase, 1 | 2 | 5) {
            assert_eq!(Some(&resource), previous_resource.as_ref());
            assert_eq!(actual.layout_runs, previous_runs);
        }
        if phase == 8 {
            assert_eq!(Some(&resource), previous_resource.as_ref());
        }
        let font_size = [16.0, 16.0, 16.0, 20.0, 14.0, 14.0, 14.0, 14.0, 14.0][phase];
        let height = if phase >= 4 { 32.0 } else { font_size * 2.0 };
        let color = if phase <= 1 { 0x55ccffff } else { 0xffcc88ff };
        let paragraph = ParagraphSnapshot::shape(
            &system,
            &props.child.child.content,
            font("Noto Sans CJK SC"),
            font_size,
            height,
        )
        .unwrap();
        let mut raster = GlyphRasterizer::new(Arc::clone(&system));
        let reference_paint = TextPaint {
            resource: raster.prepare(paragraph, 1.25).unwrap(),
            color,
        };
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let title = full
            .append_layout(full.root(), props.child.child.layout.clone(), 0)
            .unwrap();
        let sibling = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(70.0), LayoutDimension::Px(12.0))
                    .flex_shrink(0.0),
                0x55aa33ff,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(title, Some(reference_paint));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [240.0, 320.0]).unwrap();
        assert_eq!(
            ui.layout_result(node).unwrap(),
            full.layout_result(title).unwrap(),
            "phase {phase}"
        );
        assert_eq!(
            ui.layout_result(footer).unwrap(),
            full.layout_result(sibling).unwrap(),
            "phase {phase}"
        );
        assert!(
            pixels(actual.clone()) == pixels(reference.clone()),
            "complete CPU text image phase {phase}"
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            if phase == 0 {
                eprintln!("inherited text adapter: {:?}", gpu.adapter_info());
            }
            gpu.submit_local(&frame, [300, 400], 1.25).unwrap();
            assert!(
                gpu.image().unwrap().as_raw()
                    == gpu
                        .reference_image(&reference, [300, 400], 1.25)
                        .unwrap()
                        .as_raw(),
                "complete GPU text image phase {phase}"
            );
        }
        previous_resource = Some(resource);
        previous_runs = actual.layout_runs;
    }
    let old = ui.published_local_frame().unwrap().clone();
    let valid = props.clone();
    for bad in 0..4 {
        props = valid.clone();
        match bad {
            0 => props.child.child.scale = 0.0,
            1 => props.child.child.content = "tab\tstop".into(),
            2 => props.child.child.style.line_height = TextValue::Value(TextLineHeight::Px(1.0)),
            _ => props.child.style.font_size = TextValue::Value(-1.0),
        }
        assert!(
            ui.edit(
                |_| Ok(()),
                |(), e| e.components_at::<Outer>(site, host, [(0, props.clone())])
            )
            .is_err()
        );
        assert_eq!(
            old.revision(),
            ui.published_local_frame().unwrap().revision()
        );
        assert!(
            pixels(old.resolve()) == pixels(ui.published_local_frame().unwrap().resolve()),
            "failed edit retained full pixels"
        );
    }
    ui.edit(
        |_| Ok(()),
        |(), e| e.components_at::<Outer>(site, host, [(0, valid.clone())]),
    )
    .unwrap();
    assert_eq!(leaf(&ui, outer), (text_scope, node));
    drop((ui, props, valid, preparer, system));
    assert_eq!(pixels(initial.resolve()), initial_bytes);
}

#[test]
fn standalone_text_with_no_provider_updates_complete_frame_and_rejects_invalid_retry() {
    let system = Rc::new(text_system());
    let prep = Rc::new(GpuiTextPreparer::new(system.clone()));
    let mut p = TextProps::new("hello 中文", TextPreparation::new(prep));
    p.scale = 1.25;
    p.style.font_family = TextValue::Value("Noto Sans CJK SC".into());
    p.layout = LayoutStyle::column()
        .size(LayoutDimension::Px(90.0), LayoutDimension::Auto)
        .flex_shrink(0.0);
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let scope = ui
        .edit_local_frame(
            LayoutConstraints::viewport(240.0, 320.0),
            [240.0, 320.0],
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, [(0, p.clone())]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let node = ui.renders(scope).unwrap()[0];
    let before = ui.layout_result(node).unwrap().size[1];
    p.content = "one two three four five six 中文 very long content".into();
    ui.edit(
        |_| Ok(()),
        |(), e| e.components::<Text>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    assert!(ui.layout_result(node).unwrap().size[1] > before);
    let old = ui.published_local_frame().unwrap().clone();
    p.style.font_size = TextValue::Value(f32::NAN);
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, [(0, p.clone())])
        )
        .is_err()
    );
    assert_eq!(
        old.revision(),
        ui.published_local_frame().unwrap().revision()
    );
    assert!(
        pixels(old.resolve()) == pixels(ui.published_local_frame().unwrap().resolve()),
        "failed edit retained full pixels"
    );
}

#[test]
fn text_adapter_receives_font_revision_and_wrong_scale_rejects_without_losing_old_frame() {
    struct Adapter {
        inner: Rc<GpuiTextPreparer>,
        calls: Rc<std::cell::RefCell<Vec<u64>>>,
    }
    impl TextPreparer for Adapter {
        fn prepare(
            &self,
            text: &str,
            typography: &TextTypography,
            scale: f32,
            revision: u64,
        ) -> Result<TextResource, Error> {
            self.calls.borrow_mut().push(revision);
            self.inner.prepare(
                text,
                typography,
                if revision == 99 { 1.0 } else { scale },
                revision,
            )
        }
    }
    let system = Rc::new(text_system());
    let calls = Rc::new(std::cell::RefCell::new(vec![]));
    let mut p = TextProps::new(
        "adapter 中文",
        TextPreparation::new(Adapter {
            inner: Rc::new(GpuiTextPreparer::new(system)),
            calls: calls.clone(),
        }),
    );
    p.scale = 1.25;
    p.style.font_family = TextValue::Value("Noto Sans CJK SC".into());
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    ui.edit_local_frame(
        LayoutConstraints::viewport(240.0, 320.0),
        [240.0, 320.0],
        |_| Ok(()),
        |(), e| e.components::<Text>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    p.font_revision = 1;
    ui.edit(
        |_| Ok(()),
        |(), e| e.components::<Text>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    let old = ui.published_local_frame().unwrap().clone();
    let bytes = pixels(old.resolve());
    p.font_revision = 99;
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, [(0, p.clone())])
        ),
        Err(Error::InvalidTextResource)
    ));
    assert_eq!(
        old.revision(),
        ui.published_local_frame().unwrap().revision()
    );
    assert!(bytes == pixels(ui.published_local_frame().unwrap().resolve()));
    p.font_revision = 2;
    ui.edit(
        |_| Ok(()),
        |(), e| e.components::<Text>(root, host, [(0, p.clone())]),
    )
    .unwrap();
    assert_eq!(*calls.borrow(), [0, 1, 99, 2]);
    assert!(bytes == pixels(old.resolve()));
}
