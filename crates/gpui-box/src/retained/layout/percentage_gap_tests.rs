use super::*;
use crate::retained::UiTree;

#[test]
fn percentage_gaps_match_independent_browser_main_and_cross_axis_matrix() {
    let mut failures = vec![];
    let mut count = 0;
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-flex-percentage-gaps/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let dir = match f[0] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => unreachable!(),
        };
        let row = matches!(dir, FlexDirection::Row | FlexDirection::RowReverse);
        let main = match f[2] {
            "definite" => Dimension::Px(if row { 85.0 } else { 75.0 }),
            "min-content" => Dimension::MinContent,
            "max-content" => Dimension::MaxContent,
            "auto" => Dimension::Auto,
            "percent" => Dimension::Percent(0.5),
            _ => unreachable!(),
        };
        let cross = if f[3] == "definite" {
            Dimension::Px(if row { 75.0 } else { 85.0 })
        } else if row {
            Dimension::Auto
        } else {
            Dimension::MaxContent
        };
        let mut style = LayoutStyle::row()
            .flex_direction(dir)
            .flex_wrap(match f[1] {
                "nowrap" => FlexWrap::NoWrap,
                "wrap" => FlexWrap::Wrap,
                "wrap-reverse" => FlexWrap::WrapReverse,
                _ => unreachable!(),
            })
            .size(
                if row { main } else { cross },
                if row { cross } else { main },
            )
            .align_items(AlignItems::Start)
            .align_content(AlignContent::Start)
            .gap(
                if f[5] == "mixed-column" {
                    Length::Px(7.0)
                } else {
                    Length::Percent(0.1)
                },
                if f[5] == "mixed-row" {
                    Length::Px(7.0)
                } else {
                    Length::Percent(0.1)
                },
            );
        if f[4] != "plain" {
            style.padding = Edges::all(Length::Px(3.0));
            style.border = Edges::all(Length::Px(2.0));
        }
        if f[4] == "content" {
            style.box_sizing = BoxSizing::ContentBox;
        }
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let mut children = vec![];
        for _ in 0..3 {
            children.push(
                tree.append_layout(
                    tree.root(),
                    LayoutStyle::row()
                        .size(Dimension::Px(30.0), Dimension::Px(20.0))
                        .flex_shrink(0.0),
                    1,
                )
                .unwrap(),
            );
        }
        tree.frame([300.0, 200.0]).unwrap();
        let mut actual = tree.layout_result(tree.root()).unwrap().size.to_vec();
        for child in children {
            let c = tree.layout_result(child).unwrap();
            actual.extend([c.location[0], c.location[1], c.size[0], c.size[1]]);
        }
        let mut expected: Vec<f32> = f[6..].iter().map(|s| s.parse().unwrap()).collect();
        if !row && f[1] != "nowrap" && f[2] == "min-content" {
            // Flexbox9.9.1.3: multi-line min-content main size is the
            // largest item contribution. Chromium145 instead keeps one
            // unwrapped column. Retain and assert that raw discrepancy first.
            let inset = if f[4] == "plain" { 0.0 } else { 5.0 };
            let browser_height =
                60.0 + inset * 2.0 + if f[5] == "mixed-column" { 14.0 } else { 0.0 };
            assert_eq!(expected[1], browser_height);
            assert_eq!(expected[2], inset);
            assert_eq!(expected[6], inset);
            assert_eq!(expected[10], inset);
            let inner_width = if f[3] == "definite" {
                85.0 - if f[4] == "border" { 10.0 } else { 0.0 }
            } else {
                90.0 + if f[5] == "mixed-row" { 14.0 } else { 0.0 }
            };
            let gap = if f[5] == "mixed-row" {
                7.0
            } else {
                inner_width * 0.1
            };
            expected = vec![inner_width + inset * 2.0, 20.0 + inset * 2.0];
            for i in 0..3 {
                let index = if f[1] == "wrap-reverse" { 2 - i } else { i };
                expected.extend([inset + index as f32 * (30.0 + gap), inset, 30.0, 20.0]);
            }
        }
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            failures.push(format!("{:?}: {actual:?} != {expected:?}", &f[..6]));
        }
        count += 1;
    }
    assert_eq!(count, 1080);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

use crate::retained::{
    BackendCapabilities, ChildLayoutInput, FramePacket, ParentAllocationInput,
    ParentAllocationLayout, ParentLayout, ParentLayoutChildren, ParentLayoutInput,
    ParentLayoutOutput, ParentLayoutSnapshot, ParentMeasure, ParentMeasureOutput, ParentPlacement,
};

#[derive(Debug, Clone, Copy)]
struct Assigned {
    definite: LayoutSize<bool>,
    independent: bool,
}
impl Assigned {
    fn slot(self) -> ChildLayoutInput {
        ChildLayoutInput::new(LayoutConstraints::new(
            AvailableSpace::Definite(180.0),
            AvailableSpace::Unbounded,
        ))
        .with_known_size(LayoutSize::new(Some(84.0), Some(64.0)), self.definite)
    }
    fn request(self) -> ChildLayoutInput {
        if self.independent {
            ChildLayoutInput::new(LayoutConstraints::viewport(999.0, 999.0))
        } else {
            self.slot()
        }
    }
}
impl ParentAllocationLayout for Assigned {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![self.slot(); count])
    }
}
impl ParentMeasure for Assigned {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        assert_eq!(
            children.allocation(0)?,
            self.independent.then_some(self.slot())
        );
        let child = children.measure_baselines(0, self.request())?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: child.baseline,
            last_baseline: child.last_baseline,
        })
    }
}
impl ParentLayout for Assigned {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let metrics = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: metrics.size,
            baseline: metrics.baseline,
            last_baseline: metrics.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: self.request(),
                origin: [0.0; 2],
            }],
        })
    }
}
fn snapshot(definite: LayoutSize<bool>, independent: bool) -> ParentLayoutSnapshot {
    let client = Assigned {
        definite,
        independent,
    };
    let snapshot = ParentLayoutSnapshot::new(client).with_measure(client);
    if independent {
        snapshot.with_allocation_layout(client)
    } else {
        snapshot
    }
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(Dimension::Px(100.0), Dimension::Px(100.0))
        .align_items(AlignItems::Start)
}
fn host_style(direction: FlexDirection) -> LayoutStyle {
    LayoutStyle::row()
        .flex_direction(direction)
        .flex_wrap(FlexWrap::Wrap)
        .align_items(AlignItems::Start)
        .align_content(AlignContent::Start)
        .padding(Edges::all(Length::Px(2.0)))
        .gap(Length::Percent(0.1), Length::Percent(0.1))
}
fn leaf_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(30.0), Dimension::Px(20.0))
        .flex_shrink(0.0)
}
fn reference(
    direction: FlexDirection,
    definite: LayoutSize<bool>,
) -> (UiTree, Vec<crate::retained::NodeId>, FramePacket) {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(crate::retained::FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            host_style(direction)
                .size(Dimension::Px(84.0), Dimension::Px(64.0))
                .flex_shrink(0.0)
                .gap(
                    Length::Px(if definite.height { 6.0 } else { 0.0 }),
                    Length::Px(if definite.width { 8.0 } else { 0.0 }),
                ),
            0x0000ffff,
        )
        .unwrap();
    let mut nodes = vec![host];
    for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
        nodes.push(tree.append_layout(host, leaf_style(), color).unwrap());
    }
    let frame = tree.frame([180.0, 180.0]).unwrap();
    (tree, nodes, frame)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = crate::retained::software::SoftwareJob::new(
        frame,
        crate::retained::software::SoftwareOptions {
            size: [180, 180],
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

fn auto_row_style(direction: FlexDirection, wrap: FlexWrap, gap: f32) -> LayoutStyle {
    LayoutStyle::row()
        .flex_direction(direction)
        .flex_wrap(wrap)
        .align_items(AlignItems::Start)
        .align_content(AlignContent::Start)
        .gap(Length::Px(0.0), Length::Percent(gap))
}
fn auto_row_reference(direction: FlexDirection, wrap: FlexWrap, gap: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(crate::retained::FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            auto_row_style(direction, wrap, gap)
                .size(Dimension::Px(90.0), Dimension::Auto)
                .gap(Length::Px(0.0), Length::Px(90.0 * gap)),
            0x0000ffff,
        )
        .unwrap();
    for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
        tree.append_layout(host, leaf_style(), color).unwrap();
    }
    tree.frame([180.0, 180.0]).unwrap()
}

#[test]
fn auto_inline_percentage_gap_source_rewraps_and_matches_fixed_full_cpu_control() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        for direction in [FlexDirection::Row, FlexDirection::RowReverse] {
            for wrap in [FlexWrap::Wrap, FlexWrap::WrapReverse] {
                let mut ui =
                    LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
                let scope = ui.root_scope();
                let root = ui.root_render();
                let host = ui
                    .append_render_layout(
                        scope,
                        root,
                        auto_row_style(direction, wrap, 0.0),
                        0x0000ffff,
                    )
                    .unwrap();
                for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
                    ui.append_render_layout(scope, host, leaf_style(), color)
                        .unwrap();
                }
                let source = ui
                    .state(scope, 992, || auto_row_style(direction, wrap, 0.0))
                    .unwrap();
                ui.bind(source, Property::layout_style(host)).unwrap();
                let mut old = None;
                for gap in [0.0, 0.1, 0.0] {
                    let style = auto_row_style(direction, wrap, gap);
                    ui.transaction(|tx| {
                        tx.set(source, style.clone())?;
                        Ok(())
                    })
                    .unwrap();
                    let frame = if local {
                        ui.local_frame([180.0, 180.0]).unwrap().resolve()
                    } else {
                        ui.frame([180.0, 180.0]).unwrap()
                    };
                    assert_eq!(
                        ui.layout_result(host).unwrap().size,
                        [90.0, if gap > 0.0 { 40.0 } else { 20.0 }]
                    );
                    assert_eq!(
                        pixels(frame.clone()),
                        pixels(auto_row_reference(direction, wrap, gap))
                    );
                    ui.transaction(|tx| {
                        tx.set(source, auto_row_style(direction, wrap, 0.2))?;
                        tx.set(source, style)?;
                        Ok(())
                    })
                    .unwrap();
                    let idle = ui.frame([180.0, 180.0]).unwrap();
                    assert_eq!(idle.revision, frame.revision);
                    assert_eq!(idle.layout_runs, 0);
                    old.get_or_insert(frame);
                }
                let old = old.unwrap();
                let expected = pixels(old.clone());
                drop(ui);
                assert_eq!(pixels(old), expected);
            }
        }
    }
}

#[test]
fn percentage_gap_definiteness_survives_measured_and_independent_parent_source_updates() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for independent in [false, true] {
        for local in [false, true] {
            for direction in [FlexDirection::Row, FlexDirection::Column] {
                let mut ui =
                    LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
                let scope = ui.root_scope();
                let root = ui.root_render();
                let host = ui
                    .append_render_layout(scope, root, host_style(direction), 0x0000ffff)
                    .unwrap();
                let mut nodes = vec![host];
                for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
                    nodes.push(
                        ui.append_render_layout(scope, host, leaf_style(), color)
                            .unwrap(),
                    );
                }
                let source = ui
                    .state(scope, 991, || {
                        Some(snapshot(LayoutSize::new(true, false), independent))
                    })
                    .unwrap();
                ui.bind(source, Property::parent_layout(root)).unwrap();
                let mut old = None;
                for definite in [
                    LayoutSize::new(true, false),
                    LayoutSize::new(true, true),
                    LayoutSize::new(false, true),
                    LayoutSize::new(false, false),
                    LayoutSize::new(true, false),
                ] {
                    let current = Some(snapshot(definite, independent));
                    ui.transaction(|tx| {
                        tx.set(source, current.clone())?;
                        Ok(())
                    })
                    .unwrap();
                    let frame = if local {
                        ui.local_frame([180.0, 180.0]).unwrap().resolve()
                    } else {
                        ui.frame([180.0, 180.0]).unwrap()
                    };
                    let (full, controls, control) = reference(direction, definite);
                    for (&node, control) in nodes.iter().zip(controls) {
                        assert_eq!(
                            ui.layout_result(node).unwrap(),
                            full.layout_result(control).unwrap(),
                            "{independent}/{local}/{direction:?}/{definite:?}"
                        );
                    }
                    assert_eq!(pixels(frame.clone()), pixels(control));
                    ui.transaction(|tx| {
                        tx.set(
                            source,
                            Some(snapshot(
                                LayoutSize::new(!definite.width, !definite.height),
                                independent,
                            )),
                        )?;
                        tx.set(source, current)?;
                        Ok(())
                    })
                    .unwrap();
                    let idle = ui.frame([180.0, 180.0]).unwrap();
                    assert_eq!(idle.revision, frame.revision);
                    assert_eq!(idle.layout_runs, 0);
                    old.get_or_insert(frame);
                }
                let old = old.unwrap();
                let expected = pixels(old.clone());
                drop(ui);
                assert_eq!(pixels(old), expected);
            }
        }
    }
}

#[test]
fn percentage_gap_dry_baselines_hard_bounds_and_mixed_lengths_keep_independent_inputs() {
    use crate::retained::{BaselineMeasureSnapshot, MeasureSnapshot, Transaction};
    let mut tree = UiTree::new_layout(
        host_style(FlexDirection::Column)
            .flex_wrap(FlexWrap::NoWrap)
            .gap(Length::Percent(0.1), Length::Px(7.0))
            .justify_content(JustifyContent::End),
        0,
    )
    .unwrap();
    let root = tree.root();
    for _ in 0..3 {
        let child = tree.append_layout(root, leaf_style(), 1).unwrap();
        let mut tx = Transaction::default();
        tx.measure(
            child,
            Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                width: 30.0,
                height: 20.0,
                first_baseline: Some(3.0),
                last_baseline: Some(17.0),
            })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
    }
    for definite in [false, true, false] {
        let input = Assigned {
            definite: LayoutSize::new(true, definite),
            independent: false,
        }
        .slot();
        let result = tree.dry_measure_node_baselines(root, input).unwrap();
        assert_eq!(result.size, [84.0, 64.0]);
        assert_eq!(result.baseline, Some(if definite { -7.0 } else { 5.0 }));
        assert_eq!(result.last_baseline, Some(59.0));
        let mut tight = input;
        tight.constraints.bounds = BoxConstraints::tight([84.0, 64.0]);
        assert_eq!(
            tree.dry_measure_node_baselines(root, tight)
                .unwrap()
                .baseline,
            Some(-7.0)
        );
    }
    assert!(tree.published_frame().is_none());
    let mut tx = Transaction::default();
    tx.layout_style(
        root,
        LayoutStyle::column()
            .padding(Edges::all(Length::Px(2.0)))
            .gap(Length::Px(7.0), Length::Percent(0.5)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([300.0, 200.0]).unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, [34.0, 78.0]);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_percentage_gaps_follow_assigned_definiteness_and_fixed_gap_controls() {
    use crate::retained::Transaction;
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(root, host_style(FlexDirection::Column), 0x0000ffff)
        .unwrap();
    for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
        tree.append_layout(host, leaf_style(), color).unwrap();
    }
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("percentage-gap adapter: {:?}", renderer.adapter_info());
    for definite in [
        LayoutSize::new(true, false),
        LayoutSize::new(true, true),
        LayoutSize::new(true, false),
    ] {
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(snapshot(definite, true)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([180.0, 180.0]).unwrap();
        renderer
            .submit_local(&frame, [180, 180], 1.0)
            .unwrap()
            .unwrap();
        let (_, _, reference) = reference(FlexDirection::Column, definite);
        assert_eq!(
            renderer.image().unwrap().as_raw(),
            renderer
                .reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();

    for direction in [FlexDirection::Row, FlexDirection::RowReverse] {
        for wrap in [FlexWrap::Wrap, FlexWrap::WrapReverse] {
            let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
            let host = tree
                .append_layout(
                    tree.root(),
                    auto_row_style(direction, wrap, 0.0),
                    0x0000ffff,
                )
                .unwrap();
            for color in [0xff0000ff, 0x00ff00ff, 0xffff00ff] {
                tree.append_layout(host, leaf_style(), color).unwrap();
            }
            let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
            for gap in [0.0, 0.1, 0.0] {
                let mut tx = Transaction::default();
                tx.layout_style(host, auto_row_style(direction, wrap, gap));
                tree.commit(tx, BackendCapabilities::default()).unwrap();
                let frame = tree.local_frame([180.0, 180.0]).unwrap();
                renderer
                    .submit_local(&frame, [180, 180], 1.0)
                    .unwrap()
                    .unwrap();
                let reference = auto_row_reference(direction, wrap, gap);
                assert_eq!(
                    renderer.image().unwrap().as_raw(),
                    renderer
                        .reference_image(&reference, [180, 180], 1.0)
                        .unwrap()
                        .as_raw(),
                    "{direction:?}/{wrap:?}/{gap}"
                );
            }
            renderer.finish().unwrap();
        }
    }
}
