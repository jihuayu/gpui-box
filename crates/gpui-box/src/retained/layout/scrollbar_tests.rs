use super::*;
use super::{
    AlignContent, AlignItems, AvailableSpace, BoxSizing, Dimension, FlexDirection, FlexWrap,
    JustifyContent,
};
use crate::retained::{logical::LogicalUi, reactive::Property, *};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}
fn host(width: f32, rtl: bool) -> LayoutStyle {
    fixed(60.0, 40.0)
        .padding(Edges::all(Length::Px(3.0)))
        .overflow(Overflow::Scroll, Overflow::Scroll)
        .scrollbar_width(width)
        .direction(if rtl {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        })
}
fn image() -> ImageResource {
    ImageResource::rgba8([2, 1], vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap()
}

#[test]
fn scrollbar_flex_geometry_matches_independent_browser_matrix() {
    let mut count = 0;
    let mut failures = vec![];
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-scrollbar-gutters/browser-reference.tsv"
    )
    .lines()
    {
        let fields: Vec<_> = line.split('\t').collect();
        let direction = match fields[0] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => unreachable!(),
        };
        let wrap = match fields[1] {
            "nowrap" => FlexWrap::NoWrap,
            "wrap" => FlexWrap::Wrap,
            "wrap-reverse" => FlexWrap::WrapReverse,
            _ => unreachable!(),
        };
        let gutter: f32 = fields[3].parse().unwrap();
        let mut style = LayoutStyle::row()
            .size(Dimension::Px(80.0), Dimension::Px(60.0))
            .flex_direction(direction)
            .flex_wrap(wrap)
            .direction(if fields[2] == "true" {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            })
            .padding(Edges::all(Length::Px(3.0)))
            .gap(Length::Px(2.0), Length::Px(2.0))
            .align_items(AlignItems::FlexStart)
            .align_content(AlignContent::FlexStart)
            .justify_content(JustifyContent::FlexStart)
            .scrollbar_width(gutter)
            .overflow(
                if fields[4] == "both" {
                    Overflow::Scroll
                } else {
                    Overflow::Hidden
                },
                Overflow::Scroll,
            );
        style.border = Edges::all(Length::Px(2.0));
        style.box_sizing = if fields[5] == "content-box" {
            BoxSizing::ContentBox
        } else {
            BoxSizing::BorderBox
        };
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let mut children = vec![];
        for (w, h) in [(40.0, 20.0), (30.0, 16.0), (20.0, 22.0)] {
            children.push(tree.append_layout(tree.root(), fixed(w, h), 0).unwrap());
        }
        tree.frame([200.0, 160.0]).unwrap();
        let root = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![
            root.size[0],
            root.size[1],
            root.content_size[0],
            root.content_size[1],
        ];
        for node in children {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = fields[6..].iter().map(|v| v.parse().unwrap()).collect();
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            failures.push(format!("{:?}: {actual:?} vs {expected:?}", &fields[..6]));
        }
        count += 1;
    }
    assert_eq!(count, 288);
    assert!(
        failures.is_empty(),
        "{} mismatches: {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [100, 90],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn reference(width: f32, rtl: bool) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(100.0, 90.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let inset = 3.0 + if rtl { width } else { 0.0 };
    let mut style = fixed(60.0, 40.0);
    style.padding = Edges {
        left: Length::Px(inset),
        right: Length::Px(3.0 + if rtl { 0.0 } else { width }),
        top: Length::Px(3.0),
        bottom: Length::Px(3.0 + width),
    };
    let node = tree.append_layout(tree.root(), style, 0x0000ffff).unwrap();
    let mut tx = Transaction::default();
    tx.image(node, Some(image()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 90.0]).unwrap()
}

#[test]
fn scrollbar_style_validation_and_axes_reach_public_layout_results() {
    for overflow in [
        Overflow::Visible,
        Overflow::Clip,
        Overflow::Hidden,
        Overflow::Scroll,
    ] {
        for horizontal in [false, true] {
            let (x, y) = if horizontal {
                (overflow, Overflow::Visible)
            } else {
                (Overflow::Visible, overflow)
            };
            let style = host(6.0, false).overflow(x, y);
            let mut tree = UiTree::new_layout(style, 0).unwrap();
            tree.frame([100.0, 90.0]).unwrap();
            let result = tree.layout_result(tree.root()).unwrap();
            let active = if overflow == Overflow::Scroll {
                6.0
            } else {
                0.0
            };
            assert_eq!(
                result.scrollbar_size,
                if horizontal {
                    [0.0, active]
                } else {
                    [active, 0.0]
                }
            );
            assert_eq!(
                result.content_size,
                if horizontal {
                    [54.0, 34.0 - active]
                } else {
                    [54.0 - active, 34.0]
                }
            );
        }
    }
    for bad in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            UiTree::new_layout(host(bad, false), 0),
            Err(Error::InvalidStyle)
        ));
    }
}

#[test]
fn scrollbar_typed_updates_refresh_content_geometry_and_rollback_invalid_width() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(fixed(100.0, 90.0), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let node = ui
            .append_render_layout(scope, ui.root_render(), host(0.0, false), 0x0000ffff)
            .unwrap();
        let source = ui.state(scope, 992, || host(0.0, false)).unwrap();
        ui.bind(source, Property::layout_style(node)).unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().image(node, Some(image()));
                Ok(())
            },
        )
        .unwrap();
        let mut old = None;
        for (width, rtl) in [
            (0.0, false),
            (6.0, false),
            (6.0, true),
            (6.0, true),
            (12.0, true),
            (0.0, false),
        ] {
            let next = host(width, rtl);
            let build = |tx: &mut reactive::ReactiveTransaction<'_>| {
                tx.set(source, next)?;
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(100.0, 90.0),
                    [100.0, 90.0],
                    build,
                    |(), _| Ok(()),
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(100.0, 90.0),
                    [100.0, 90.0],
                    build,
                    |(), _| Ok(()),
                )
                .unwrap()
                .1
            };
            assert_eq!(
                ui.layout_result(node).unwrap().content_size,
                [54.0 - width, 34.0 - width]
            );
            assert_eq!(ui.layout_result(node).unwrap().scrollbar_size, [width; 2]);
            let item = frame.items.iter().find(|item| item.node == node).unwrap();
            assert_eq!(
                item.content_bounds,
                Rect {
                    x: 3.0 + if rtl { width } else { 0.0 },
                    y: 3.0,
                    width: 54.0 - width,
                    height: 34.0 - width
                }
            );
            assert_eq!(pixels(frame.clone()), pixels(reference(width, rtl)));
            old.get_or_insert(frame);
        }
        let before = ui.read(source).unwrap().clone();
        let stats = ui.stats();
        let revision = ui.published_frame().unwrap().revision;
        assert!(matches!(
            ui.edit_frame(
                LayoutConstraints::viewport(100.0, 90.0),
                [100.0, 90.0],
                |tx| {
                    tx.set(source, host(-1.0, false))?;
                    Ok(())
                },
                |(), _| Ok(())
            ),
            Err(Error::InvalidStyle)
        ));
        assert_eq!(ui.read(source).unwrap(), &before);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, revision);
        let frame = ui.frame([100.0, 90.0]).unwrap();
        ui.transaction(|tx| {
            tx.set(source, host(6.0, false))?;
            tx.set(source, before.clone())?;
            Ok(())
        })
        .unwrap();
        let same = ui.frame([100.0, 90.0]).unwrap();
        assert_eq!(same.revision, frame.revision);
        assert_eq!(same.layout_runs, 0);
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[derive(Debug)]
struct Fill;
impl ParentLayout for Fill {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let size = [
            input.known_size.width.unwrap_or(0.0),
            input.known_size.height.unwrap_or(0.0),
        ];
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::viewport(size[0], size[1]),
            known_size: LayoutSize::new(Some(size[0]), Some(size[1])),
            ..Default::default()
        };
        let child = children.measure_baselines(0, slot)?;
        Ok(ParentLayoutOutput {
            size,
            baseline: child.baseline,
            last_baseline: child.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [0.0; 2],
            }],
        })
    }
}

#[test]
fn scrollbar_gutters_deflate_flex_constraint_box_and_owned_parent_slots() {
    for mode in 0..3 {
        for rtl in [false, true] {
            let mut style = host(6.0, rtl);
            if mode == 1 {
                style.display = LayoutDisplay::ConstraintBox {
                    width: ConstraintFit::Tight,
                    height: ConstraintFit::Tight,
                };
            }
            let mut tree = UiTree::new_layout(style, 0).unwrap();
            let root = tree.root();
            let child = tree
                .append_layout(
                    root,
                    LayoutStyle::row()
                        .size(Dimension::Percent(1.0), Dimension::Percent(1.0))
                        .flex_shrink(0.0),
                    1,
                )
                .unwrap();
            if mode == 2 {
                let mut tx = Transaction::default();
                tx.parent_layout(root, Some(ParentLayoutSnapshot::new(Fill)));
                tree.commit(tx, BackendCapabilities::default()).unwrap();
            }
            tree.frame([60.0, 40.0]).unwrap();
            let result = tree.layout_result(child).unwrap();
            assert_eq!(result.size, [48.0, 28.0], "mode {mode} rtl {rtl}");
            assert_eq!(result.location, [if rtl { 9.0 } else { 3.0 }, 3.0]);
            assert_eq!(tree.layout_result(root).unwrap().content_size, [48.0, 28.0]);
        }
    }
}

#[test]
fn scrollbar_width_updates_reclamp_scroll_and_retain_cached_descendant_positions() {
    let mut tree = UiTree::new_layout(host(6.0, false), 0).unwrap();
    let root = tree.root();
    let child = tree.append_layout(root, fixed(100.0, 80.0), 1).unwrap();
    let nested = tree.append_layout(child, fixed(4.0, 4.0), 2).unwrap();
    let mut tx = Transaction::default();
    tx.scroll(root, [1000.0; 2]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for gutter in [6.0, 12.0, 0.0, 6.0] {
        let mut tx = Transaction::default();
        tx.layout_style(root, host(gutter, false));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([100.0, 90.0]).unwrap().resolve();
        let layout = tree.layout.layout(tree.node(root).unwrap().layout).unwrap();
        assert_eq!(
            [layout.scroll_width(), layout.scroll_height()],
            [46.0 + gutter, 46.0 + gutter]
        );
        let item = frame.items.iter().find(|item| item.node == nested).unwrap();
        assert_eq!(
            [item.bounds.x, item.bounds.y],
            [-43.0 - gutter, -43.0 - gutter]
        );
    }
}

#[test]
fn scrollbar_builder_inputs_keep_incoming_border_box_distinct_from_deflated_child_slot() {
    for outgoing in [false, true] {
        let root_style = LayoutStyle::column().size(Dimension::Percent(1.0), Dimension::Auto);
        let mut ui = LogicalUi::new_layout(root_style, 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let mut host_style = LayoutStyle::column()
            .padding(Edges::all(Length::Px(3.0)))
            .overflow(Overflow::Visible, Overflow::Scroll)
            .scrollbar_width(6.0);
        if outgoing {
            host_style.display = LayoutDisplay::ConstraintBox {
                width: ConstraintFit::Tight,
                height: ConstraintFit::Loose,
            };
            host_style.container_type = ContainerType::InlineSize;
        }
        let node = ui.append_render_layout(scope, root, host_style, 0).unwrap();
        let input =
            LayoutConstraints::new(AvailableSpace::Definite(100.0), AvailableSpace::Unbounded);
        let (commit, _) = ui
            .edit_local_frame(
                input,
                [100.0, 90.0],
                |_| Ok(()),
                |(), edit| {
                    let create =
                        |input: logical::InlineConstraintBuilderInput,
                         edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                            assert_eq!(
                                input.known_width,
                                Some(if outgoing { 88.0 } else { 100.0 })
                            );
                            let child =
                                edit.append_layout(scope, node, LayoutStyle::column(), 0)?;
                            edit.inline_allocation_builder(child, |input, edit| {
                                assert_eq!(input.known_width, Some(88.0));
                                edit.append_layout(scope, child, fixed(40.0, 20.0), 1)
                            })
                        };
                    if outgoing {
                        edit.inline_constraint_builder(node, create)
                    } else {
                        edit.inline_allocation_builder(node, create)
                    }
                },
            )
            .unwrap();
        assert_eq!(
            ui.layout_result(commit.result.value).unwrap().size,
            [40.0, 20.0]
        );
        assert_eq!(ui.layout_result(node).unwrap().content_size, [88.0, 20.0]);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_scrollbar_gutter_image_updates_match_numeric_content_geometry() {
    let mut tree = UiTree::new_layout(fixed(100.0, 90.0), 0).unwrap();
    let node = tree
        .append_layout(tree.root(), host(0.0, false), 0x0000ffff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.image(node, Some(image()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("scrollbar adapter: {:?}", gpu.adapter_info());
    for (width, rtl) in [
        (0.0, false),
        (6.0, false),
        (6.0, true),
        (12.0, true),
        (0.0, false),
    ] {
        let mut tx = Transaction::default();
        tx.layout_style(node, host(width, rtl));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([100.0, 90.0]).unwrap();
        gpu.submit_local(&frame, [100, 90], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(width, rtl), [100, 90], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
