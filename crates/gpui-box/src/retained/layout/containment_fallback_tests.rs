use super::*;
use crate::retained::{
    BackendCapabilities, ChildLayoutInput, LayoutSizingMode, Transaction, UiTree,
};

#[test]
fn intrinsic_containment_fallback_matches_independent_browser_flex_matrix() {
    let mut failures = vec![];
    let mut count = 0;
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-containment-fallback/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let direction = match f[0] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => unreachable!(),
        };
        let mut tree = UiTree::new_layout(
            LayoutStyle::row()
                .flex_direction(direction)
                .size(
                    Dimension::Px(
                        if f[4] == "shrink"
                            && matches!(direction, FlexDirection::Row | FlexDirection::RowReverse)
                        {
                            30.0
                        } else {
                            300.0
                        },
                    ),
                    Dimension::Px(
                        if f[4] == "shrink"
                            && matches!(
                                direction,
                                FlexDirection::Column | FlexDirection::ColumnReverse
                            )
                        {
                            30.0
                        } else {
                            200.0
                        },
                    ),
                )
                .align_items(AlignItems::Start),
            0,
        )
        .unwrap();
        let mut style = LayoutStyle::column()
            .container_type(match f[1] {
                "none" => ContainerType::Normal,
                "inline-size" => ContainerType::InlineSize,
                "size" => ContainerType::Size,
                _ => unreachable!(),
            })
            .padding(Edges::all(Length::Px(3.0)));
        style.border = Edges::all(Length::Px(2.0));
        style.contain_intrinsic_size = match f[2] {
            "none" => LayoutSize::new(None, None),
            "0px 0px" => LayoutSize::new(Some(0.0), Some(0.0)),
            "60px 25px" => LayoutSize::new(Some(60.0), Some(25.0)),
            "none 25px" => LayoutSize::new(None, Some(25.0)),
            "60px none" => LayoutSize::new(Some(60.0), None),
            _ => unreachable!(),
        };
        if f[3] == "content" {
            style.box_sizing = BoxSizing::ContentBox;
        }
        style = match f[4] {
            "none" => style,
            "preferred" => style.size(Dimension::Px(80.0), Dimension::Px(40.0)),
            "min" => style.min_size(Length::Px(100.0), Length::Px(50.0)),
            "max" => style.max_size(Length::Px(40.0), Length::Px(20.0)),
            "intrinsic" => style
                .min_size(Dimension::MinContent, Dimension::Auto)
                .max_size(Dimension::MaxContent, Dimension::Auto),
            "shrink" => style.min_size(Dimension::Px(0.0), Dimension::Px(0.0)),
            "grow" => style.flex_grow(1.0),
            _ => unreachable!(),
        };
        let host = tree.append_layout(tree.root(), style, 0).unwrap();
        let child = tree
            .append_layout(
                host,
                LayoutStyle::row()
                    .size(Dimension::Px(180.0), Dimension::Px(90.0))
                    .flex_shrink(0.0),
                1,
            )
            .unwrap();
        tree.frame([300.0, 200.0]).unwrap();
        let h = tree.layout_result(host).unwrap();
        let c = tree.layout_result(child).unwrap();
        let actual = [
            h.location[0],
            h.location[1],
            h.size[0],
            h.size[1],
            c.size[0],
            c.size[1],
        ];
        let expected: Vec<f32> = f[5..].iter().map(|v| v.parse().unwrap()).collect();
        if actual
            .iter()
            .zip(expected.iter())
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            failures.push(format!("{:?}: {actual:?} != {expected:?}", &f[..5]));
        }
        count += 1;
    }
    assert_eq!(count, 840);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn intrinsic_fallback_is_content_in_dry_queries_and_external_allocation_wins() {
    let tree = UiTree::new_layout(
        LayoutStyle::column()
            .container_type(ContainerType::Size)
            .contain_intrinsic_size(Some(60.0), Some(25.0))
            .size(Dimension::Px(100.0), Dimension::Px(70.0))
            .padding(Edges::all(Length::Px(2.0))),
        0,
    )
    .unwrap();
    let root = tree.root();
    for (mode, expected) in [
        (LayoutSizingMode::Content, [64.0, 29.0]),
        (LayoutSizingMode::Inherent, [100.0, 70.0]),
    ] {
        let input = ChildLayoutInput::new(LayoutConstraints::new(
            AvailableSpace::MaxContent,
            AvailableSpace::MaxContent,
        ))
        .with_sizing_mode(mode);
        assert_eq!(tree.dry_measure_node(root, input).unwrap().size, expected);
        let mut tight = input;
        tight.constraints.bounds = BoxConstraints::tight([35.0, 20.0]);
        assert_eq!(
            tree.dry_measure_node(root, tight).unwrap().size,
            [35.0, 20.0]
        );
    }
    assert!(tree.published_frame().is_none());
}

#[test]
fn intrinsic_fallback_style_validation_and_transaction_failure_are_atomic() {
    let original = LayoutStyle::column()
        .container_type(ContainerType::Size)
        .contain_intrinsic_size(Some(40.0), Some(20.0));
    let mut tree = UiTree::new_layout(original.clone(), 0).unwrap();
    tree.frame([200.0, 100.0]).unwrap();
    for invalid in [-1.0, f32::NAN, f32::INFINITY] {
        let bad = original.clone().contain_intrinsic_size(Some(invalid), None);
        assert_eq!(bad.validate(), Err(Error::InvalidStyle));
        let mut tx = Transaction::default();
        tx.layout_style(
            tree.root(),
            original
                .clone()
                .contain_intrinsic_size(Some(70.0), Some(30.0)),
        );
        tx.layout_style(tree.root(), bad);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidStyle
        );
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, [40.0, 20.0]);
        assert_eq!(
            tree.frame([200.0, 100.0]).unwrap().layout_stats,
            crate::retained::LayoutStats::default()
        );
    }
}

#[test]
fn intrinsic_fallback_replaced_image_suppresses_only_natural_ratio() {
    use crate::retained::{ImageIntrinsicSize, ImageResource, MeasureSnapshot};
    for decoded in [false, true] {
        for ratio in [None, Some(2.0)] {
            let mut style = LayoutStyle::column()
                .container_type(ContainerType::Size)
                .contain_intrinsic_size(Some(60.0), Some(25.0));
            style.aspect_ratio = ratio;
            let mut tree = UiTree::new_layout(style, 0).unwrap();
            let mut tx = Transaction::default();
            if decoded {
                tx.image(
                    tree.root(),
                    Some(ImageResource::rgba8([80, 40], [255, 0, 255, 255].repeat(3200)).unwrap()),
                );
            } else {
                tx.measure(
                    tree.root(),
                    Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                        width: 80.0,
                        height: 40.0,
                    })),
                );
            }
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.frame([240.0, 160.0]).unwrap();
            assert_eq!(
                tree.layout_result(tree.root()).unwrap().size,
                [60.0, if ratio.is_some() { 30.0 } else { 25.0 }],
                "{decoded}/{ratio:?}"
            );
            assert!(!frame.items.is_empty());
        }
    }
}

#[test]
fn intrinsic_fallback_reserves_gutters_but_never_invents_paintable_overflow() {
    use crate::retained::{CustomMeasureSnapshot, MeasureSnapshot};
    let mut style = LayoutStyle::column()
        .container_type(ContainerType::Size)
        .contain_intrinsic_size(Some(60.0), Some(25.0))
        .padding(Edges::all(Length::Px(2.0)))
        .to_taffy()
        .unwrap();
    style.overflow = taffy::Point {
        x: taffy::Overflow::Scroll,
        y: taffy::Overflow::Scroll,
    };
    style.scrollbar_width = 6.0;
    let mut tree = UiTree::new(style, 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        tree.root(),
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 10.0,
            height: 5.0,
            baseline: None,
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([240.0, 160.0]).unwrap();
    let result = tree.layout_result(tree.root()).unwrap();
    assert_eq!(result.size, [70.0, 35.0]);
    assert_eq!(result.scrollable_overflow, [0.0, 14.0, 0.0, 9.0]);
}

fn root_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(240.0), Dimension::Px(160.0))
        .align_items(AlignItems::Start)
}
fn placeholder(width: f32, explicit: bool) -> LayoutStyle {
    let mut style = LayoutStyle::column().padding(Edges::all(Length::Px(2.0)));
    if explicit {
        style = style
            .size(Dimension::Px(width + 4.0), Dimension::Px(29.0))
            .flex_shrink(0.0);
    } else {
        style = style
            .container_type(ContainerType::Size)
            .contain_intrinsic_size(Some(width), Some(25.0));
    }
    style
}
fn child_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Percent(0.5), Dimension::Px(12.0))
        .flex_shrink(0.0)
}
fn reference(width: f32) -> crate::retained::FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(crate::retained::FrameBuildPolicy::Full);
    let host = tree
        .append_layout(tree.root(), placeholder(width, true), 0x0000ffff)
        .unwrap();
    tree.append_layout(host, child_style(), 0xff0000ff).unwrap();
    tree.frame([240.0, 160.0]).unwrap()
}
fn pixels(frame: crate::retained::FramePacket) -> Vec<u8> {
    let mut job = crate::retained::software::SoftwareJob::new(
        frame,
        crate::retained::software::SoftwareOptions {
            size: [240, 160],
            scale: 1.0,
            background: 0,
            effects: crate::retained::pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}

#[test]
fn intrinsic_fallback_typed_updates_keep_cpu_pixels_p0_visibility_and_snapshots() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui
            .append_render_layout(scope, root, placeholder(60.0, false), 0x0000ffff)
            .unwrap();
        let child = ui
            .append_render_layout(scope, host, child_style(), 0xff0000ff)
            .unwrap();
        let source = ui.state(scope, 990, || placeholder(60.0, false)).unwrap();
        ui.bind(source, Property::layout_style(host)).unwrap();
        let mut old = None;
        for width in [60.0, 100.0, 0.0, 60.0] {
            ui.transaction(|tx| {
                tx.set(source, placeholder(width, false))?;
                Ok(())
            })
            .unwrap();
            let frame = if local {
                ui.local_frame([240.0, 160.0]).unwrap().resolve()
            } else {
                ui.frame([240.0, 160.0]).unwrap()
            };
            assert_eq!(ui.layout_result(host).unwrap().size, [width + 4.0, 29.0]);
            assert_eq!(ui.layout_result(child).unwrap().size, [width / 2.0, 12.0]);
            assert_eq!(pixels(frame.clone()), pixels(reference(width)));
            ui.transaction(|tx| {
                tx.set(source, placeholder(width + 10.0, false))?;
                tx.set(source, placeholder(width, false))?;
                Ok(())
            })
            .unwrap();
            let idle = ui.frame([240.0, 160.0]).unwrap();
            assert_eq!(idle.revision, frame.revision);
            assert_eq!(idle.layout_runs, 0);
            old.get_or_insert(frame);
        }
        ui.transaction(|tx| {
            tx.set(
                source,
                placeholder(60.0, false).display(LayoutDisplay::None),
            )?;
            Ok(())
        })
        .unwrap();
        assert!(
            !ui.frame([240.0, 160.0])
                .unwrap()
                .items
                .iter()
                .any(|item| item.node == child)
        );
        ui.transaction(|tx| {
            tx.set(source, placeholder(100.0, false))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            pixels(ui.frame([240.0, 160.0]).unwrap()),
            pixels(reference(100.0))
        );
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_intrinsic_fallback_updates_match_independent_fixed_geometry() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let host = tree
        .append_layout(tree.root(), placeholder(60.0, false), 0x0000ffff)
        .unwrap();
    tree.append_layout(host, child_style(), 0xff0000ff).unwrap();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("intrinsic-fallback adapter: {:?}", renderer.adapter_info());
    for width in [60.0, 100.0, 0.0, 60.0] {
        let mut tx = Transaction::default();
        tx.layout_style(host, placeholder(width, false));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([240.0, 160.0]).unwrap();
        renderer
            .submit_local(&frame, [240, 160], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            renderer.image().unwrap().as_raw(),
            renderer
                .reference_image(&reference(width), [240, 160], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();
}
