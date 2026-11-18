use super::*;
use core::prelude::v1::test;
use gpui_box::retained::{
    text::{GpuiTextPreparer, Text, TextPreparation, TextPreparer, TextProps},
    text_style::*,
    theme::*,
};
use std::rc::Rc;

#[gpui::test]
fn owner_host_inherited_text_click_installs_rewrapped_complete_cpu_picture(
    cx: &mut TestAppContext,
) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let prep = Rc::new(GpuiTextPreparer::new(Rc::new(WindowTextSystem::new(
        cx.text_system().clone(),
    ))));
    let window = cx.open_window(size(px(600.0), px(460.0)), |w, cx| {
        OwnerDemo::new(w, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (source, scope, node, initial, content) = view.update(cx, |this, cx| {
        let root = this.ui.root_scope();
        let host = this.ui.root_render();
        let shown = this.shown.as_ref().unwrap();
        let viewport = shown.viewport;
        let scale = shown.scale;
        let source = this
            .ui
            .state(root, 100, || {
                ThemeGroup::<TextStyleKey>::create(|v| v.font.family = "Noto Sans CJK SC".into())
                    .unwrap()
            })
            .unwrap();
        let old = this.ui.renders(root).unwrap().to_vec();
        let content = "one two three 中文字号继承 four five";
        let mut p = TextProps::new(content, TextPreparation::new(prep.clone()));
        p.scale = scale;
        p.layout = LayoutStyle::column()
            .size(LayoutDimension::Px(140.0), LayoutDimension::Auto)
            .flex_shrink(0.0);
        let scope = this
            .ui
            .edit_local_frame(
                LayoutConstraints::viewport(viewport[0], viewport[1]),
                viewport,
                |_| Ok(()),
                |(), e| {
                    for old in old {
                        e.properties()
                            .layout_style(old, LayoutStyle::column().display(LayoutDisplay::None));
                    }
                    e.provide_theme(root, source)?;
                    let site = e.component_sites(root, &[1])?[0];
                    let scope = e.components_at::<Text>(site, host, [(0, p)])?[0];
                    let node = e.renders(scope)?[0];
                    let handler = e.event_handler(
                        scope,
                        100,
                        EventHandler::new(move |event, cx| {
                            if event.kind == UiInputEventKind::Activate {
                                cx.set(
                                    source,
                                    ThemeGroup::<TextStyleKey>::create(|v| {
                                        v.font.family = "Noto Sans CJK SC".into();
                                        v.font_size = 22.0;
                                        v.color = 0xffcc88ff;
                                    })?,
                                )?;
                            }
                            Ok(())
                        }),
                    )?;
                    e.interaction(
                        scope,
                        node,
                        Some(UiInteraction {
                            focus: UiFocusPolicy::Tab,
                            activate_handler: Some(handler),
                            ..Default::default()
                        }),
                    )?;
                    Ok(scope)
                },
            )
            .unwrap()
            .0
            .result
            .value;
        let node = this.ui.renders(scope).unwrap()[0];
        let initial = this.installs;
        cx.notify();
        (source, scope, node, initial, content)
    });
    cx.run_until_parked();
    let click = view.read_with(cx, |this, _| {
        let b = this
            .shown
            .as_ref()
            .unwrap()
            .frame
            .resolve_geometry(node)
            .unwrap()
            .bounds;
        this.bounds.unwrap().origin + point(px(b.x + 8.0), px(b.y + 8.0))
    });
    cx.simulate_click(click, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert!(this.installs > initial);
        assert_eq!(this.ui.read(source).unwrap().values().font_size, 22.0);
        assert_eq!(this.ui.renders(scope).unwrap(), [node]);
        assert!(this.error.is_none(), "{:?}", this.error);
        let shown = this.shown.as_ref().unwrap();
        assert_eq!(this.router.scene_id(), Some(shown.scene.id()));
        assert!(this.cpu_task.is_none() && this.pending.is_none() && this.ready.is_none());
        let values = this.ui.read(source).unwrap().values();
        let paint = TextPaint {
            resource: prep
                .prepare(content, &values.typography().unwrap(), shown.scale, 0)
                .unwrap(),
            color: values.color,
        };
        let mut full = UiTree::new_layout(
            LayoutStyle::column()
                .size(
                    LayoutDimension::Px(shown.viewport[0]),
                    LayoutDimension::Px(shown.viewport[1]),
                )
                .padding(LayoutEdges::all(LayoutLength::Px(20.0)))
                .gap(LayoutLength::Px(16.0), LayoutLength::Px(0.0)),
            0x17202fff,
        )
        .unwrap();
        let text = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(140.0), LayoutDimension::Auto)
                    .flex_shrink(0.0),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(text, Some(paint));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = full.frame(shown.viewport).unwrap();
        assert_eq!(
            this.ui.layout_result(node).unwrap(),
            full.layout_result(text).unwrap()
        );
        let mut job = SoftwareJob::new(
            frame,
            SoftwareOptions {
                size: shown.size,
                scale: shown.scale,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 128 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        let mut bytes = job.into_frame().unwrap().pixels().to_vec();
        for p in bytes.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        let Raster::Image(image) = &shown.raster else {
            panic!("CPU picture expected")
        };
        assert!(
            image.as_bytes(0).unwrap() == bytes.as_slice(),
            "installed inherited-text picture differs from ordinary full Flex"
        );
    });
}
