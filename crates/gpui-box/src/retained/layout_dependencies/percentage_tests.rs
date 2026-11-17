use super::{row_tests::*, *};
use crate::retained::{logical::LogicalUi, reactive::Property, responsive::*};

fn percent_item(grow: f32, basis: f32) -> LayoutStyle {
    item(grow)
        .flex_basis(LayoutDimension::Percent(basis))
        .min_size(LayoutDimension::Percent(0.1), LayoutDimension::Auto)
        .max_size(LayoutDimension::Percent(0.9), LayoutDimension::Auto)
}

#[test]
fn percentage_row_allocations_match_browser_with_percent_insets_and_limits() {
    let mut count = 0;
    let mut failures = vec![];
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-percentage-inline-allocation/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let w: f32 = f[3].parse().unwrap();
        let style = root(f[0] == "row-reverse")
            .flex_wrap(match f[1] {
                "nowrap" => LayoutFlexWrap::NoWrap,
                "wrap" => LayoutFlexWrap::Wrap,
                "wrap-reverse" => LayoutFlexWrap::WrapReverse,
                _ => unreachable!(),
            })
            .direction(if f[2] == "true" { LayoutDirection::Rtl } else { LayoutDirection::Ltr })
            .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
            .gap(LayoutLength::Px(4.0), LayoutLength::Px(4.0))
            .align_items(LayoutAlignItems::FlexStart)
            .align_content(LayoutAlignContent::FlexStart)
            .justify_content(LayoutJustifyContent::FlexStart);
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let mut nodes = vec![];
        for i in 0..3 {
            let preferred = LayoutDimension::Percent([0.55, 0.3, 0.25][i]);
            let mut style = item([1.0, 2.0, 1.0][i])
                .flex_basis(match f[4] {
                    "auto-percent" => LayoutDimension::Auto,
                    "mixed" if i == 1 => LayoutDimension::Px(60.0),
                    _ => preferred,
                })
                .size(if f[4] == "auto-percent" { preferred } else { LayoutDimension::Auto }, LayoutDimension::Auto)
                .min_size(LayoutDimension::Percent([0.1, 0.08, 0.06][i]), LayoutDimension::Auto)
                .max_size(if i == 2 { LayoutDimension::Auto } else { LayoutDimension::Percent([0.7, 0.9][i]) }, LayoutDimension::Auto)
                .padding(LayoutEdges::all(LayoutLength::Percent(0.01)))
                .margin(LayoutEdges {
                    left: LayoutLength::Percent(0.015), right: LayoutLength::Percent(0.015),
                    top: LayoutLength::Px(0.0), bottom: LayoutLength::Px(0.0),
                });
            style.border = LayoutEdges::all(LayoutLength::Px(1.0));
            if f[5] == "content-box" { style.box_sizing = LayoutBoxSizing::ContentBox; }
            let node = tree.append_layout(tree.root(), style, 0).unwrap();
            nodes.push(node);
            tree.append_layout(node, fixed(400.0, if f[6] == "true" { [80.0, 10.0, 100.0][i] } else { [20.0, 60.0, 40.0][i] }), 0).unwrap();
        }
        for &node in &nodes {
            let style = &tree.node(node).unwrap().style;
            assert!(independent_inline_allocation_chain(&tree, node, style).unwrap());
            assert!(!independent_inline_chain(&tree, node, style).unwrap(), "unconditional proof must not assume an external basis");
        }
        tree.frame_in_viewport(constraints(w), [300.0; 2]).unwrap();
        let root = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![root.size[0], root.size[1]];
        for node in nodes {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = f[7..].iter().map(|v| v.parse().unwrap()).collect();
        if actual.iter().zip(expected).any(|(a, b)| (a - b).abs() > 0.06) {
            failures.push(format!("{:?}: {actual:?}", &f[..7]));
        }
        count += 1;
    }
    assert_eq!(count, 432);
    assert!(
        failures.is_empty(),
        "{} failures: {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

fn reference(w: f32, nested: bool) -> FramePacket {
    let mut tree = UiTree::new_layout(root(false), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let mut host = tree
        .append_layout(tree.root(), percent_item(0.0, 0.25), 0x0000ffff)
        .unwrap();
    tree.append_layout(
        tree.root(),
        percent_item(1.0, 0.25).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
        0x00ff00ff,
    )
    .unwrap();
    if nested {
        let row = tree.append_layout(host, root(false), 0).unwrap();
        host = tree
            .append_layout(row, percent_item(0.0, 0.5), 0x0000ffff)
            .unwrap();
    }
    tree.append_layout(
        host,
        fixed(400.0, if w < 200.0 { 20.0 } else { 60.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}

#[test]
fn percentage_row_builder_composes_nested_flex_and_atomic_local_or_resolved_output() {
    for nested in [false, true] {
        for local in [false, true] {
            let mut ui =
                LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
            let scope = ui.root_scope();
            let root = ui.root_render();
            let mut host = ui
                .append_render_layout(scope, root, percent_item(0.0, 0.25), 0x0000ffff)
                .unwrap();
            ui.append_render_layout(
                scope,
                root,
                percent_item(1.0, 0.25).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
                0x00ff00ff,
            )
            .unwrap();
            if nested {
                let row = ui
                    .append_render_layout(scope, host, root_style(), 0)
                    .unwrap();
                host = ui
                    .append_render_layout(scope, row, percent_item(0.0, 0.5), 0x0000ffff)
                    .unwrap();
            }
            let mut old = None;
            for w in [120.0, 240.0, 160.0] {
                let build = |(), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        let expected = w * if nested { 0.125 } else { 0.25 };
                        assert_eq!(input.known_width, Some(expected));
                        assert!(input.known_width_is_definite);
                        assert_eq!(
                            input.percentage_basis,
                            Some(w * if nested { 0.25 } else { 1.0 })
                        );
                        let branch = edit.reconcile_keys(scope, &[u64::from(w >= 200.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 200.0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?;
                        }
                        Ok(())
                    })
                };
                let frame = if local {
                    ui.edit_local_frame(constraints(w), [180.0; 2], |_| Ok(()), build)
                        .unwrap()
                        .1
                        .resolve()
                } else {
                    ui.edit_frame(constraints(w), [180.0; 2], |_| Ok(()), build)
                        .unwrap()
                        .1
                };
                assert!(pixels(frame.clone()) == pixels(reference(w, nested)));
                old.get_or_insert(frame);
            }
            let old = old.unwrap();
            let expected = pixels(old.clone());
            drop(ui);
            assert_eq!(pixels(old), expected);
        }
    }
}
fn root_style() -> LayoutStyle {
    root(false)
}

#[test]
fn percentage_query_stages_use_final_sibling_allocation() {
    let outer = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let mut ui = LogicalUi::new_layout(outer, 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let row = ui
        .append_render_layout(scope, ui.root_render(), root(false), 0)
        .unwrap();
    let host = ui
        .append_render_layout(scope, row, percent_item(1.0, 0.25), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, row, percent_item(1.0, 0.25), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let side = ui.state(scope, 1, || percent_item(1.0, 0.25)).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            source,
            target,
            host,
            LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(70.0, fixed(400.0, 60.0))])
                .unwrap(),
        )
        .unwrap();
    rules
        .bind_container(
            &mut ui,
            side,
            sibling,
            row,
            LayoutBreakpointStyles::new(
                percent_item(1.0, 0.25),
                vec![(180.0, percent_item(3.0, 0.5))],
            )
            .unwrap(),
        )
        .unwrap();
    for (w, expected) in [(160.0, 80.0), (240.0, 75.0), (180.0, 56.25), (160.0, 80.0)] {
        rules
            .local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([500.0; 2]),
                constraints(w),
                [180.0; 2],
            )
            .unwrap();
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            [expected, if expected < 70.0 { 20.0 } else { 60.0 }]
        );
    }
}

#[test]
fn percentage_registered_query_automatically_resettles_after_sibling_source_update() {
    let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let side = ui.state(scope, 1, || percent_item(1.0, 0.25)).unwrap();
    ui.bind(side, Property::layout_style(sibling)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                0,
                source,
                target,
                LayoutQuery::ContainerInline(host),
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(80.0, fixed(400.0, 60.0))])?,
            )
        },
    )
    .unwrap();
    ui.frame_with_environment(
        &LayoutEnvironment::new([500.0; 2]),
        constraints(180.0),
        [180.0; 2],
    )
    .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [90.0, 60.0]);
    ui.transaction(|tx| tx.set(side, percent_item(3.0, 0.5)))
        .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [56.25, 20.0]);
    assert_eq!(ui.read(source).unwrap(), &fixed(400.0, 20.0));
    let before = ui.published_frame().unwrap().revision;
    ui.transaction(|tx| tx.set(side, percent_item(3.0, 0.5)))
        .unwrap();
    assert_eq!(ui.published_frame().unwrap().revision, before);
}

#[test]
fn percentage_query_rejects_unanchored_evaluation_and_content_dependent_sibling_alternatives() {
    for alternatives in [false, true] {
        let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui
            .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
            .unwrap();
        let sibling = ui
            .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
            .unwrap();
        let target = ui
            .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
            .unwrap();
        let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
        let side = ui.state(scope, 1, || percent_item(1.0, 0.25)).unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        rules
            .bind_container(
                &mut ui,
                source,
                target,
                host,
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(80.0, fixed(400.0, 60.0))])
                    .unwrap(),
            )
            .unwrap();
        if alternatives {
            assert!(matches!(
                rules.bind(
                    &mut ui,
                    side,
                    sibling,
                    LayoutBreakpointStyles::new(
                        percent_item(1.0, 0.25),
                        vec![(900.0, LayoutStyle::column())]
                    )
                    .unwrap()
                ),
                Err(Error::UnsafeContainerQuery)
            ));
        } else {
            let old = ui
                .frame_in_viewport(constraints(180.0), [180.0; 2])
                .unwrap();
            assert!(matches!(
                rules.frame_in_viewport(
                    &mut ui,
                    &LayoutEnvironment::new([500.0; 2]),
                    LayoutConstraints::new(
                        LayoutAvailableSpace::Unbounded,
                        LayoutAvailableSpace::Unbounded
                    ),
                    [180.0; 2]
                ),
                Err(Error::UnsafeContainerQuery)
            ));
            assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        }
    }
}

#[derive(Debug)]
struct NoLayout;
impl ParentLayout for NoLayout {
    fn layout(
        &self,
        _: ParentLayoutInput,
        _: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        Err(Error::InvalidParentLayout)
    }
}
impl ParentInlineLayout for NoLayout {
    fn allocate(
        &self,
        _: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        Ok(vec![
            ChildInlineAllocation {
                width: 80.0,
                percentage_basis: None
            };
            count
        ])
    }
}

#[test]
fn percentage_proof_does_not_infer_definiteness_across_custom_protocols() {
    let mut tree = UiTree::new_layout(root(false), 0).unwrap();
    let row = tree.append_layout(tree.root(), root(false), 0).unwrap();
    let host = tree.append_layout(row, percent_item(1.0, 0.5), 0).unwrap();
    let mut tx = Transaction::default();
    tx.parent_layout(
        tree.root(),
        Some(ParentLayoutSnapshot::new(NoLayout).with_inline_layout(NoLayout)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(
        !independent_inline_allocation_chain(&tree, host, &tree.node(host).unwrap().style).unwrap()
    );
}

#[test]
fn percentage_allocations_accept_numeric_or_tight_external_anchors_in_unbounded_space() {
    for tight in [false, true] {
        let style = if tight {
            root(false)
        } else {
            root(false).size(LayoutDimension::Px(180.0), LayoutDimension::Auto)
        };
        let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, ui.root_render(), percent_item(0.0, 0.5), 0)
            .unwrap();
        let mut input = LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        );
        if tight {
            input.bounds = BoxConstraints::new([180.0, 0.0], [180.0, f32::INFINITY]);
        }
        ui.edit_frame(
            input,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_width, Some(90.0));
                    assert!(input.known_width_is_definite);
                    edit.append_layout(scope, host, fixed(400.0, 20.0), 1)
                })
            },
        )
        .unwrap();
        assert_eq!(ui.layout_result(host).unwrap().size, [90.0, 20.0]);
    }
}

#[test]
fn percentage_query_computed_sibling_feedback_rejects_before_publication() {
    let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            source,
            target,
            host,
            LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(80.0, fixed(400.0, 60.0))])
                .unwrap(),
        )
        .unwrap();
    let feedback = ui
        .computed(scope, 1, &[source.dependency()], move |input| {
            Ok(percent_item(
                1.0,
                if input.read(source)?.size.height == LayoutDimension::Px(60.0) {
                    0.5
                } else {
                    0.25
                },
            ))
        })
        .unwrap();
    ui.bind(feedback, Property::layout_style(sibling)).unwrap();
    assert!(matches!(
        rules.frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([500.0; 2]),
            constraints(180.0),
            [180.0; 2]
        ),
        Err(Error::UnsafeContainerQuery)
    ));
}

#[test]
fn percentage_builders_reject_unanchored_roots_and_non_flex_definiteness_paths() {
    for mode in 0..5 {
        let initial_style = match mode {
            3 => LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
            4 => root(false).size(LayoutDimension::MaxContent, LayoutDimension::Auto),
            _ => root(false),
        };
        let mut ui =
            LogicalUi::new_layout(initial_style, 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let row = ui
            .append_render_layout(scope, root, root_style(), 0)
            .unwrap();
        let host = ui
            .append_render_layout(scope, row, percent_item(1.0, 0.5), 0)
            .unwrap();
        let constraints = if mode < 3 {
            LayoutConstraints::new(
                [
                    LayoutAvailableSpace::Unbounded,
                    LayoutAvailableSpace::MinContent,
                    LayoutAvailableSpace::MaxContent,
                ][mode],
                LayoutAvailableSpace::Unbounded,
            )
        } else {
            constraints(180.0)
        };
        let mut called = false;
        let result = ui.edit_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |_, _| {
                    called = true;
                    Ok(())
                })
            },
        );
        assert!(
            matches!(result, Err(Error::UnsafeConstraintBuilder)),
            "mode {mode}"
        );
        assert!(!called);
    }
}

#[test]
fn percentage_sibling_late_changes_reject_and_rollback_even_if_used_width_is_equal() {
    let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, root, percent_item(1.0, 0.25), 0)
        .unwrap();
    let source = ui.state(scope, 0, || percent_item(1.0, 0.25)).unwrap();
    ui.bind(source, Property::layout_style(sibling)).unwrap();
    let old = ui
        .frame_in_viewport(constraints(180.0), [180.0; 2])
        .unwrap();
    let stats = ui.stats();
    let result = ui.edit_frame(
        constraints(180.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.inline_allocation_builder(host, |_, edit| {
                // Changing an inactive preferred width leaves the numeric basis and
                // final width equal, but changes the observed allocation dependency.
                edit.properties().layout_style(
                    sibling,
                    percent_item(1.0, 0.25)
                        .size(LayoutDimension::Percent(0.7), LayoutDimension::Auto),
                );
                Ok(())
            })
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.read(source).unwrap(), &percent_item(1.0, 0.25));
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.published_frame().unwrap().revision, old.revision);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_percentage_inline_allocations_match_complete_reference() {
    let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, percent_item(0.0, 0.25), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(
        scope,
        root,
        percent_item(1.0, 0.25).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
        0x00ff00ff,
    )
    .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("percentage allocation adapter: {:?}", gpu.adapter_info());
    for w in [120.0, 240.0, 160.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |_, edit| {
                        let branch = edit.reconcile_keys(scope, &[u64::from(w >= 200.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 200.0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?;
                        }
                        Ok(())
                    })
                },
            )
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&reference(w, false), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    gpu.finish().unwrap();
}
