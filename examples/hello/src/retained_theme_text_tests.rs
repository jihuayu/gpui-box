//! Logical theme sources drive ordinary style/text/Flex and immutable frames.
use super::*;
use gpui_box::retained::{logical::*, style::*, theme::*, ui_ir::UiProperties};
use std::{cell::Cell, rc::Rc};
#[derive(Clone, PartialEq)]
struct Values {
    width: f32,
    padding: f32,
    color: u32,
    foreground: u32,
}
impl Default for Values {
    fn default() -> Self {
        Self {
            width: 90.0,
            padding: 4.0,
            color: 0x153050ff,
            foreground: 0xffffffff,
        }
    }
}
struct AppTheme;
impl ThemeKey for AppTheme {
    type Values = Values;
    fn validate(v: &Values) -> Result<(), Error> {
        if [v.width, v.padding]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidTheme);
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Props {
    paint: TextPaint,
    mounts: Rc<Cell<usize>>,
}
impl PartialEq for Props {
    fn eq(&self, o: &Self) -> bool {
        self.paint == o.paint && Rc::ptr_eq(&self.mounts, &o.mounts)
    }
}
struct Label;
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
impl Component for Label {
    type Props = Props;
    fn mount(p: ReadProp<Props>, cx: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        let seen = cx.read_prop(p)?.mounts.clone();
        seen.set(seen.get() + 1);
        let theme = cx.use_theme::<AppTheme>()?;
        let node = cx.append_layout(LayoutStyle::column(), 0)?;
        let selected = cx.select_theme(0, theme, |v| {
            StylePlan::new(&[StyleRef::new(
                "theme-label",
                vec![
                    StyleDeclaration::Width(StyleRule::value(LayoutDimension::Px(v.width))),
                    StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(
                        LayoutLength::Px(v.padding),
                    ))),
                    StyleDeclaration::FlexShrink(StyleRule::value(0.0)),
                    StyleDeclaration::Color(StyleRule::value(v.color)),
                ],
            )
            .map_err(|_| Error::InvalidStyle)?])
            .resolve_values(
                &LayoutStyle::column(),
                &UiProperties::default(),
                &responsive::LayoutEnvironment::new([240.0, 240.0]),
                StyleState::default(),
            )
            .map_err(|_| Error::InvalidStyle)
        })?;
        cx.bind_prop(selected, Property::style_resolution(node))?;
        let text = cx.computed(1, &[theme.dependency(), p.dependency()], move |i| {
            let mut text = i.read_prop(p)?.paint.clone();
            text.color = i.read_prop(theme)?.values().foreground;
            Ok(Some(text))
        })?;
        cx.bind(text, Property::text(node))?;
        Ok(())
    }
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 300],
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
fn theme_scope_real_noto_rewrap_and_color_match_full_flex_without_remount() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_theme_scope_real_noto_matches_full_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let paint = TextPaint {
        resource: raster
            .prepare(
                shape(
                    &system,
                    "one two three four five 中文主题段落 six seven eight",
                ),
                1.25,
            )
            .unwrap(),
        color: 0xffffffff,
    };
    let mounts = Rc::new(Cell::new(0));
    let mut props = ThemeScopeProps::<AppTheme, Label> {
        theme: ThemeGroup::defaults().unwrap(),
        child: Props {
            paint: paint.clone(),
            mounts: mounts.clone(),
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
    let scope = ui
        .edit_local_frame(
            constraints,
            [240.0, 240.0],
            |_| Ok(()),
            |(), e| {
                let sites = e.component_sites(root, &[0, 1])?;
                let scope = e.components_at::<ThemeScope<AppTheme, Label>>(
                    sites[0],
                    host,
                    [(0, props.clone())],
                )?[0];
                let sibling = e.components_at::<Footer>(sites[1], host, [(0, ())])?[0];
                Ok((scope, sibling, sites[0]))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let component = ui.children(scope.0).unwrap()[0];
    let label = ui.renders(component).unwrap()[0];
    let footer = ui.renders(scope.1).unwrap()[0];
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (width, padding, color, foreground) in [
        (90.0, 4.0, 0x153050ff, 0xffffffff),
        (145.0, 8.0, 0x775533ff, 0xffcc88ff),
        (65.0, 2.0, 0x225544ff, 0xccffffff),
    ] {
        props.theme = ThemeGroup::<AppTheme>::create(|v| {
            v.width = width;
            v.padding = padding;
            v.color = color;
            v.foreground = foreground;
        })
        .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), e| {
                e.components_at::<ThemeScope<AppTheme, Label>>(scope.2, host, [(0, props.clone())])
            },
        )
        .unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let title = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(width), LayoutDimension::Auto)
                    .padding(LayoutEdges::all(LayoutLength::Px(padding)))
                    .flex_shrink(0.0),
                color,
            )
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
        let mut text = paint.clone();
        text.color = foreground;
        let mut tx = Transaction::default();
        tx.text(title, Some(text));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [240.0, 240.0]).unwrap();
        assert_eq!(
            ui.layout_result(label).unwrap(),
            full.layout_result(title).unwrap()
        );
        assert_eq!(
            ui.layout_result(footer).unwrap(),
            full.layout_result(sibling).unwrap()
        );
        assert!(
            pixels(frame.resolve()) == pixels(reference.clone()),
            "complete CPU theme image"
        );
        assert_eq!(mounts.get(), 1);
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("theme Noto adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [300, 300], 1.25).unwrap();
            assert!(
                gpu.image().unwrap().as_raw()
                    == gpu
                        .reference_image(&reference, [300, 300], 1.25)
                        .unwrap()
                        .as_raw(),
                "complete GPU theme image"
            );
        }
        old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
    }
    drop((ui, props, paint, raster, system));
    let (frame, bytes) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), bytes);
}
