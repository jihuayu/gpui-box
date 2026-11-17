use super::*;

#[derive(Debug)]
struct OneChild;
impl ParentLayout for OneChild {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::new(
                LayoutAvailableSpace::MaxContent,
                LayoutAvailableSpace::MaxContent,
            ),
            percentage_basis: input.percentage_basis,
        };
        let child = if children.is_empty() {
            None
        } else {
            Some(children.measure_baselines(0, slot)?)
        };
        let size = child.map_or([0.0; 2], |child| child.size);
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(size[0]),
                input.known_size.height.unwrap_or(size[1]),
            ],
            baseline: child.and_then(|child| child.baseline),
            last_baseline: child.and_then(|child| child.last_baseline),
            placements: if child.is_some() {
                vec![ParentPlacement {
                    child: 0,
                    input: slot,
                    origin: [0.0; 2],
                }]
            } else {
                vec![]
            },
        })
    }
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
        .align_items(LayoutAlignItems::Start)
}
fn fixture(custom: bool) -> (UiTree, NodeId, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(fixed(180.0, 160.0), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            fixed(60.0, 40.0)
                .padding(LayoutEdges::all(LayoutLength::Px(4.0)))
                .overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    let child = tree
        .append_layout(host, fixed(100.0, 80.0), 0xff0000ff)
        .unwrap();
    let nested = tree
        .append_layout(child, fixed(10.0, 10.0), 0x00ff00ff)
        .unwrap();
    let mut tx = Transaction::default();
    if custom {
        tx.parent_layout(host, Some(ParentLayoutSnapshot::new(OneChild)));
    }
    tx.clip(
        host,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 40.0,
        }),
    );
    tx.scroll(host, [1000.0; 2]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, host, child, nested)
}

#[test]
fn actual_overflow_shrink_reclamps_scroll_and_moves_cached_descendants_like_full_flex() {
    for local in [false, true] {
        let (mut tree, host, child, nested) = fixture(true);
        let (mut reference, ref_host, ref_child, ref_nested) = fixture(false);
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let mut old = None;
        for (step, (width, height, hidden)) in [
            (100.0, 80.0, false),
            (20.0, 15.0, false),
            (100.0, 80.0, false),
            (100.0, 80.0, true),
            (100.0, 80.0, false),
        ]
        .into_iter()
        .enumerate()
        {
            for (owner, node) in [(&mut tree, child), (&mut reference, ref_child)] {
                let mut tx = Transaction::default();
                tx.layout_style(
                    node,
                    fixed(width, height).display(if hidden {
                        LayoutDisplay::None
                    } else {
                        LayoutDisplay::Flex
                    }),
                );
                owner.commit(tx, BackendCapabilities::default()).unwrap();
            }
            let frame = if local {
                tree.local_frame([180.0, 160.0]).unwrap().resolve()
            } else {
                tree.frame([180.0, 160.0]).unwrap()
            };
            let full = reference.frame([180.0, 160.0]).unwrap();
            for (a, b) in [(host, ref_host), (child, ref_child), (nested, ref_nested)] {
                assert_eq!(
                    tree.layout_result(a).unwrap(),
                    reference.layout_result(b).unwrap(),
                    "step={step}"
                );
            }
            let overflow = tree.layout_result(host).unwrap().scrollable_overflow;
            assert_eq!(
                overflow,
                if hidden {
                    [0.0, 4.0, 0.0, 4.0]
                } else {
                    [0.0, width + 8.0, 0.0, height + 8.0]
                }
            );
            let item = frame.items.iter().find(|item| item.node == child);
            if !hidden {
                let x = 4.0 - (width + 8.0 - 60.0).max(0.0);
                let y = 4.0 - (height + 8.0 - 40.0).max(0.0);
                assert_eq!((item.unwrap().bounds.x, item.unwrap().bounds.y), (x, y));
            }
            for (x, y) in [
                (1.0, 1.0),
                (5.0, 5.0),
                (25.0, 20.0),
                (59.0, 39.0),
                (65.0, 45.0),
            ] {
                let actual = frame.hit_test(x, y);
                let expected = full.hit_test(x, y).map(|id| {
                    if id == ref_nested {
                        nested
                    } else if id == ref_child {
                        child
                    } else if id == ref_host {
                        host
                    } else {
                        tree.root()
                    }
                });
                assert_eq!(actual, expected, "step={step}/point={x},{y}");
            }
            let pixels = super::tests::pixels(frame.clone());
            assert_eq!(pixels, super::tests::pixels(full));
            old.get_or_insert((frame, pixels));
        }
        drop((tree, reference));
        let (frame, pixels) = old.unwrap();
        assert_eq!(super::tests::pixels(frame), pixels);
    }
}

#[derive(Debug, Clone)]
struct OffsetBaseline {
    offset: f32,
}
impl ParentMeasure for OffsetBaseline {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let child = children.measure_baselines(
            0,
            ChildLayoutInput::new(LayoutConstraints::new(
                LayoutAvailableSpace::MaxContent,
                LayoutAvailableSpace::MaxContent,
            )),
        )?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: child.baseline.map(|v| v + self.offset),
            last_baseline: child.last_baseline.map(|v| v + self.offset),
        })
    }
}
impl ParentLayout for OffsetBaseline {
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
                origin: [0.0, self.offset],
                input: ChildLayoutInput::new(LayoutConstraints::new(
                    LayoutAvailableSpace::MaxContent,
                    LayoutAvailableSpace::MaxContent,
                )),
            }],
        })
    }
}
#[test]
fn finite_signed_parent_baselines_outside_fixed_host_height_match_ordinary_flex() {
    for offset in [-30.0, 0.0] {
        for kind in [LayoutContainerType::Normal, LayoutContainerType::Size] {
            let style = fixed(60.0, 20.0)
                .container_type(kind)
                .padding(LayoutEdges::all(LayoutLength::Px(2.0)));
            let mut outputs = vec![];
            for custom in [false, true] {
                let mut tree = UiTree::new_layout(style.clone(), 0).unwrap();
                let mut child_style = fixed(20.0, 60.0);
                child_style.margin.top = LayoutLength::Px(offset);
                let child = tree
                    .append_layout(tree.root(), child_style, 0xff0000ff)
                    .unwrap();
                let mut tx = Transaction::default();
                tx.measure(
                    child,
                    Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                        width: 20.0,
                        height: 60.0,
                        first_baseline: Some(3.0),
                        last_baseline: Some(55.0),
                    })),
                );
                if custom {
                    tx.parent_layout(
                        tree.root(),
                        Some(
                            ParentLayoutSnapshot::new(OffsetBaseline { offset })
                                .with_measure(OffsetBaseline { offset }),
                        ),
                    );
                }
                tree.commit(tx, BackendCapabilities::default()).unwrap();
                let query = tree
                    .dry_measure_node_baselines(
                        tree.root(),
                        ChildLayoutInput::new(LayoutConstraints::viewport(80.0, 80.0)),
                    )
                    .unwrap();
                let frame = tree.frame([80.0; 2]).unwrap();
                let result = tree.layout_result(tree.root()).unwrap();
                assert_eq!(
                    (query.baseline, query.last_baseline),
                    (result.baseline, result.last_baseline)
                );
                if kind == LayoutContainerType::Normal {
                    assert_eq!(
                        (result.baseline, result.last_baseline),
                        (Some(offset + 5.0), Some(offset + 57.0))
                    );
                } else {
                    assert_eq!((result.baseline, result.last_baseline), (None, None));
                }
                outputs.push((result, super::tests::pixels(frame)));
            }
            assert_eq!(outputs[0], outputs[1]);
        }
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_actual_overflow_scroll_reclamp_matches_full_flex() {
    let (mut tree, _, child, _) = fixture(true);
    let (mut full, _, ref_child, _) = fixture(false);
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("owned overflow adapter: {:?}", gpu.adapter_info());
    for size in [[100.0, 80.0], [20.0, 15.0], [100.0, 80.0]] {
        for (owner, node) in [(&mut tree, child), (&mut full, ref_child)] {
            let mut tx = Transaction::default();
            tx.layout_style(node, fixed(size[0], size[1]));
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let frame = tree.local_frame([180.0, 160.0]).unwrap();
        gpu.submit_local(&frame, [180, 160], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&full.frame([180.0, 160.0]).unwrap(), [180, 160], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
