use super::*;
use crate::retained::{
    BackendCapabilities, ImageIntrinsicSize, MeasureSnapshot, Transaction, UiTree,
};

#[test]
fn automatic_flex_minimum_matches_independent_browser_image_and_content_cases() {
    let mut mismatches = vec![];
    let mut count = 0;
    let mut deviations = 0;
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-flex-auto-min/browser-reference.tsv"
    )
    .lines()
    {
        let fields: Vec<_> = line.split('\t').collect();
        let replaced = fields[0] == "true";
        let dir = match fields[1] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => unreachable!(),
        };
        let row = matches!(dir, FlexDirection::Row | FlexDirection::RowReverse);
        let main = match fields[5] {
            "definite" => Dimension::Px(30.0),
            "min-content" => Dimension::MinContent,
            "max-content" => Dimension::MaxContent,
            _ => unreachable!(),
        };
        let mut tree = UiTree::new_layout(
            LayoutStyle::row()
                .flex_direction(dir)
                .size(
                    if row { main } else { Dimension::Px(240.0) },
                    if row { Dimension::Px(240.0) } else { main },
                )
                .align_items(AlignItems::Start),
            0,
        )
        .unwrap();
        let mut style = LayoutStyle::column();
        style.aspect_ratio = Some(2.0);
        if fields[4] != "plain" {
            style.padding = Edges::all(Length::Px(5.0));
            style.border = Edges::all(Length::Px(2.0));
        }
        if fields[4] == "content" {
            style.box_sizing = BoxSizing::ContentBox;
        }
        let main = if row {
            &mut style.size.height
        } else {
            &mut style.size.width
        };
        if fields[2] != "auto" {
            *main = Dimension::Px(fields[2].parse().unwrap());
        }
        let axis = |size: &mut LayoutSize<Dimension>, value| {
            if row {
                size.width = Dimension::Px(value)
            } else {
                size.height = Dimension::Px(value)
            }
        };
        let cross = |size: &mut LayoutSize<Dimension>, value| {
            if row {
                size.height = Dimension::Px(value)
            } else {
                size.width = Dimension::Px(value)
            }
        };
        match fields[3] {
            "specified" => axis(&mut style.size, 60.0),
            "maximum" => axis(&mut style.max_size, 70.0),
            "minimum" => axis(&mut style.min_size, 10.0),
            "cross-min" => cross(&mut style.min_size, 60.0),
            "cross-min-zero" => cross(&mut style.min_size, 0.0),
            "cross-max" => cross(&mut style.max_size, 30.0),
            "scroll" => style = style.overflow(Overflow::Scroll, Overflow::Scroll),
            "hidden" => style = style.overflow(Overflow::Hidden, Overflow::Hidden),
            "clip" => style = style.overflow(Overflow::Clip, Overflow::Clip),
            "none" => {}
            _ => unreachable!(),
        }
        let node = tree.append_layout(tree.root(), style, 0xff0000ff).unwrap();
        if replaced {
            let mut tx = Transaction::default();
            tx.measure(
                node,
                Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                    width: 100.0,
                    height: 50.0,
                })),
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
        } else {
            tree.append_layout(
                node,
                LayoutStyle::column()
                    .size(Dimension::Px(100.0), Dimension::Px(50.0))
                    .flex_shrink(0.0),
                0,
            )
            .unwrap();
        }
        tree.frame([300.0, 300.0]).unwrap();
        let result = tree.layout_result(node).unwrap();
        let actual = [
            result.location[0],
            result.location[1],
            result.size[0],
            result.size[1],
        ];
        let mut expected: Vec<f32> = fields[6..].iter().map(|v| v.parse().unwrap()).collect();
        // CSS Flexbox 4.5 caps the content size suggestion by a definite
        // cross maximum transferred through the ratio. Chromium145 retains
        // the unbounded content width in these twelve non-replaced cases.
        if !replaced && row && fields[2] != "auto" && fields[3] == "cross-max" {
            assert_eq!(
                expected[2],
                if fields[4] == "plain" { 100.0 } else { 114.0 }
            );
            expected[2] = if fields[4] == "content" { 74.0 } else { 60.0 };
            deviations += 1;
        }
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            mismatches.push(format!(
                "{} actual={actual:?} expected={expected:?}",
                fields[..6].join("/")
            ));
        }
        count += 1;
    }
    assert_eq!(count, 816);
    assert_eq!(deviations, 12);
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

fn root_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(30.0), Dimension::Px(160.0))
        .align_items(AlignItems::Start)
}
fn ratio_style(height: f32, explicit: bool) -> LayoutStyle {
    let mut style = LayoutStyle::column().size(
        if explicit {
            Dimension::Px((height * 2.0).max(100.0))
        } else {
            Dimension::Auto
        },
        Dimension::Px(height),
    );
    if !explicit {
        style.aspect_ratio = Some(2.0);
    } else {
        // This independent control fixes the expected used geometry. An
        // ordinary preferred width alone would still shrink in the 30px row.
        style.flex_shrink = 0.0;
    }
    style
}
fn reference(height: f32) -> crate::retained::FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(crate::retained::FrameBuildPolicy::Full);
    let host = tree
        .append_layout(tree.root(), ratio_style(height, true), 0x0000ffff)
        .unwrap();
    tree.append_layout(
        host,
        LayoutStyle::column()
            .size(Dimension::Px(100.0), Dimension::Px(50.0))
            .flex_shrink(0.0),
        0xff0000ff,
    )
    .unwrap();
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
fn automatic_minimum_uses_typed_updates_complete_cpu_controls_and_old_owner_lifetimes() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui
            .append_render_layout(scope, root, ratio_style(20.0, false), 0x0000ffff)
            .unwrap();
        ui.append_render_layout(
            scope,
            host,
            LayoutStyle::column()
                .size(Dimension::Px(100.0), Dimension::Px(50.0))
                .flex_shrink(0.0),
            0xff0000ff,
        )
        .unwrap();
        let source = ui.state(scope, 771, || ratio_style(20.0, false)).unwrap();
        ui.bind(source, Property::layout_style(host)).unwrap();
        let mut old = None;
        for height in [20.0, 80.0, 20.0] {
            ui.transaction(|tx| {
                tx.set(source, ratio_style(height, false))?;
                Ok(())
            })
            .unwrap();
            let frame = if local {
                ui.local_frame([240.0, 160.0]).unwrap().resolve()
            } else {
                ui.frame([240.0, 160.0]).unwrap()
            };
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [(height * 2.0).max(100.0), height]
            );
            assert!(
                pixels(frame.clone()) == pixels(reference(height)),
                "height={height} local={local}"
            );
            let rev = frame.revision;
            ui.transaction(|tx| {
                tx.set(source, ratio_style(height + 1.0, false))?;
                tx.set(source, ratio_style(height, false))?;
                Ok(())
            })
            .unwrap();
            let idle = ui.frame([240.0, 160.0]).unwrap();
            assert_eq!(idle.revision, rev);
            assert_eq!(idle.layout_stats, crate::retained::LayoutStats::default());
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn decoded_image_ratio_box_updates_keep_real_resources_and_cpu_pixels() {
    use crate::retained::{FrameBuildPolicy, ImageResource};
    let resource = ImageResource::rgba8([100, 50], [255, 0, 255, 255].repeat(5000)).unwrap();
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let image = tree
        .append_layout(tree.root(), LayoutStyle::column(), 0)
        .unwrap();
    let mut full = UiTree::new_layout(root_style(), 0).unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let control = full
        .append_layout(full.root(), LayoutStyle::column(), 0)
        .unwrap();
    for (owner, node) in [(&mut tree, image), (&mut full, control)] {
        let mut tx = Transaction::default();
        tx.image(node, Some(resource.clone()));
        owner.commit(tx, BackendCapabilities::default()).unwrap();
    }
    let mut old = None;
    for height in [20.0, 80.0, 20.0] {
        for (owner, node, ratio) in [(&mut tree, image, true), (&mut full, control, false)] {
            let mut style = LayoutStyle::column()
                .size(
                    if ratio {
                        Dimension::Auto
                    } else {
                        Dimension::Px(height * 2.0)
                    },
                    Dimension::Px(height),
                )
                .padding(Edges::all(Length::Px(5.0)));
            style.box_sizing = BoxSizing::ContentBox;
            style.border = Edges::all(Length::Px(2.0));
            if ratio {
                style.aspect_ratio = Some(2.0)
            } else {
                style.flex_shrink = 0.0;
            }
            let mut tx = Transaction::default();
            tx.layout_style(node, style);
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let frame = tree.local_frame([240.0, 160.0]).unwrap();
        let reference = full.frame([240.0, 160.0]).unwrap();
        assert_eq!(
            tree.layout_result(image).unwrap().size,
            [height * 2.0 + 14.0, height + 14.0]
        );
        assert_eq!(
            tree.layout_result(image).unwrap(),
            full.layout_result(control).unwrap()
        );
        assert!(
            pixels(frame.resolve()) == pixels(reference),
            "height={height}"
        );
        old.get_or_insert(frame);
    }
    let old = old.unwrap();
    let expected = pixels(old.resolve());
    drop((tree, full, resource));
    assert!(pixels(old.resolve()) == expected);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_automatic_minimum_matches_independent_fixed_geometry() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let host = tree
        .append_layout(tree.root(), ratio_style(20.0, false), 0x0000ffff)
        .unwrap();
    tree.append_layout(
        host,
        LayoutStyle::column()
            .size(Dimension::Px(100.0), Dimension::Px(50.0))
            .flex_shrink(0.0),
        0xff0000ff,
    )
    .unwrap();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("auto-min adapter: {:?}", renderer.adapter_info());
    for height in [20.0, 80.0, 20.0] {
        let mut tx = Transaction::default();
        tx.layout_style(host, ratio_style(height, false));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([240.0, 160.0]).unwrap();
        renderer
            .submit_local(&frame, [240, 160], 1.0)
            .unwrap()
            .unwrap();
        assert!(
            renderer.image().unwrap().as_raw()
                == renderer
                    .reference_image(&reference(height), [240, 160], 1.0)
                    .unwrap()
                    .as_raw(),
            "height={height}"
        );
    }
    renderer.finish().unwrap();
}
