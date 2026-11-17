use super::*;
use crate::retained::{logical::LogicalUi, reactive::Property, responsive::*};

pub(super) fn root(reverse: bool) -> LayoutStyle {
    LayoutStyle::row()
        .flex_direction(if reverse {
            LayoutFlexDirection::RowReverse
        } else {
            LayoutFlexDirection::Row
        })
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .align_items(LayoutAlignItems::Start)
}
pub(super) fn item(grow: f32) -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(grow)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
}
pub(super) fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
pub(super) fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
pub(super) fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
fn reference(w: f32, reverse: bool) -> FramePacket {
    let mut tree = UiTree::new_layout(root(reverse), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(tree.root(), item(1.0), 0x0000ffff)
        .unwrap();
    tree.append_layout(
        host,
        fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.append_layout(
        tree.root(),
        item(1.0).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
        0x00ff00ff,
    )
    .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}

#[test]
fn row_proof_and_final_geometry_match_independent_browser_across_content_height_changes() {
    let mut count = 0;
    let mut failures = vec![];
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-row-inline-allocation/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let w: f32 = f[3].parse().unwrap();
        let direction = if f[0] == "row" {
            LayoutFlexDirection::Row
        } else {
            LayoutFlexDirection::RowReverse
        };
        let wrap = match f[1] {
            "nowrap" => LayoutFlexWrap::NoWrap,
            "wrap" => LayoutFlexWrap::Wrap,
            "wrap-reverse" => LayoutFlexWrap::WrapReverse,
            _ => unreachable!(),
        };
        let rtl = if f[2] == "true" {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        };
        let style = root(false)
            .flex_direction(direction)
            .flex_wrap(wrap)
            .direction(rtl)
            .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
            .gap(LayoutLength::Px(4.0), LayoutLength::Px(4.0))
            .align_items(LayoutAlignItems::FlexStart)
            .align_content(LayoutAlignContent::FlexStart)
            .justify_content(LayoutJustifyContent::FlexStart);
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let root = tree.root();
        let mut nodes = vec![];
        for i in 0..3 {
            let mut style = item([1.0, 2.0, 1.0][i])
                .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
                .margin(LayoutEdges {
                    left: LayoutLength::Px(2.0),
                    right: LayoutLength::Px(2.0),
                    top: LayoutLength::Px(0.0),
                    bottom: LayoutLength::Px(0.0),
                })
                .max_size(
                    if i == 2 {
                        LayoutDimension::Auto
                    } else {
                        LayoutDimension::Px([100.0, 200.0][i])
                    },
                    LayoutDimension::Auto,
                );
            if f[4] == "numeric" {
                style.flex_basis = LayoutDimension::Px([90.0, 60.0, 70.0][i]);
                style.min_size.width = LayoutDimension::Px([30.0, 20.0, 10.0][i]);
            }
            if f[5] == "content-box" {
                style.box_sizing = LayoutBoxSizing::ContentBox;
            }
            let node = tree.append_layout(root, style, 0).unwrap();
            nodes.push(node);
            let h = if f[6] == "true" {
                [80.0, 10.0, 100.0][i]
            } else {
                [20.0, 60.0, 40.0][i]
            };
            tree.append_layout(node, fixed(400.0, h), 0).unwrap();
        }
        for &node in &nodes {
            assert!(
                independent_inline_allocation_chain(&tree, node, &tree.node(node).unwrap().style)
                    .unwrap()
            );
        }
        tree.frame_in_viewport(constraints(w), [300.0; 2]).unwrap();
        let root = tree.layout_result(root).unwrap();
        let mut actual = vec![root.size[0], root.size[1]];
        for node in nodes {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = f[7..].iter().map(|v| v.parse().unwrap()).collect();
        if actual
            .iter()
            .zip(expected)
            .any(|(a, b)| (a - b).abs() > 0.03)
        {
            failures.push(format!("{:?}: {actual:?}", &f[..7]));
        }
        count += 1;
    }
    assert_eq!(count, 288);
    assert!(
        failures.is_empty(),
        "{} failures: {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

#[test]
fn row_inline_builder_allocates_columns_without_content_width_or_height_feedback() {
    for reverse in [false, true] {
        for local in [false, true] {
            let mut ui =
                LogicalUi::new_layout(root(reverse), 0, BackendCapabilities::default()).unwrap();
            let scope = ui.root_scope();
            let root = ui.root_render();
            let host = ui
                .append_render_layout(scope, root, item(1.0), 0x0000ffff)
                .unwrap();
            ui.append_render_layout(
                scope,
                root,
                item(1.0).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
                0x00ff00ff,
            )
            .unwrap();
            let mut old = None;
            for w in [120.0, 180.0, 240.0, 120.0] {
                let build = |(), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        assert_eq!(input.known_width, Some(w * 0.5));
                        let branch = edit.reconcile_keys(scope, &[u64::from(w >= 180.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
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
                assert_eq!(
                    ui.layout_result(host).unwrap().size,
                    [w * 0.5, if w < 180.0 { 20.0 } else { 60.0 }]
                );
                assert!(pixels(frame.clone()) == pixels(reference(w, reverse)));
                old.get_or_insert(frame);
            }
            let old = old.unwrap();
            let expected = pixels(old.clone());
            drop(ui);
            assert_eq!(pixels(old), expected);
        }
    }
}

#[test]
fn row_query_stages_include_sibling_allocation_producers_registered_after_consumers() {
    let outer_style =
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let mut ui = LogicalUi::new_layout(outer_style, 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let outer = ui.root_render();
    let row = ui
        .append_render_layout(scope, outer, root(false), 0)
        .unwrap();
    let host = ui.append_render_layout(scope, row, item(1.0), 0).unwrap();
    let sibling = ui.append_render_layout(scope, row, item(1.0), 0).unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let sibling_source = ui.state(scope, 1, || item(1.0)).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            source,
            target,
            host,
            LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                .unwrap(),
        )
        .unwrap();
    rules
        .bind_container(
            &mut ui,
            sibling_source,
            sibling,
            outer,
            LayoutBreakpointStyles::new(item(1.0), vec![(180.0, item(3.0))]).unwrap(),
        )
        .unwrap();
    for (w, expected) in [(160.0, 80.0), (180.0, 45.0), (400.0, 100.0), (160.0, 80.0)] {
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
            [expected, if expected < 90.0 { 20.0 } else { 60.0 }]
        );
    }
}

#[test]
fn wrapped_row_builder_changes_row_height_without_changing_content_independent_line_membership() {
    for wrap in [LayoutFlexWrap::Wrap, LayoutFlexWrap::WrapReverse] {
        let root_style = root(false)
            .flex_wrap(wrap)
            .gap(LayoutLength::Px(2.0), LayoutLength::Px(0.0));
        let host_style = item(1.0).flex_basis(LayoutDimension::Px(80.0));
        let side_style = host_style
            .clone()
            .size(LayoutDimension::Auto, LayoutDimension::Px(10.0));
        let mut ui =
            LogicalUi::new_layout(root_style.clone(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui
            .append_render_layout(scope, root, host_style.clone(), 0x0000ffff)
            .unwrap();
        ui.append_render_layout(scope, root, side_style.clone(), 0x00ff00ff)
            .unwrap();
        for (w, height) in [(120.0, 20.0), (180.0, 60.0), (120.0, 20.0)] {
            let (_, frame) = ui
                .edit_local_frame(
                    constraints(w),
                    [180.0; 2],
                    |_| Ok(()),
                    |(), edit| {
                        edit.inline_allocation_builder(host, |input, edit| {
                            assert_eq!(
                                input.known_width,
                                Some(if w < 160.0 { w } else { w * 0.5 })
                            );
                            let branch =
                                edit.reconcile_keys(scope, &[u64::from(height > 20.0)])?[0];
                            if edit.renders(branch)?.is_empty() {
                                edit.append_layout(branch, host, fixed(400.0, height), 0xff0000ff)?;
                            }
                            Ok(())
                        })
                    },
                )
                .unwrap();
            let mut full = UiTree::new_layout(root_style.clone(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let a = full
                .append_layout(full.root(), host_style.clone(), 0x0000ffff)
                .unwrap();
            full.append_layout(a, fixed(400.0, height), 0xff0000ff)
                .unwrap();
            full.append_layout(full.root(), side_style.clone(), 0x00ff00ff)
                .unwrap();
            let control = full.frame_in_viewport(constraints(w), [180.0; 2]).unwrap();
            assert_eq!(
                ui.layout_result(host).unwrap(),
                full.layout_result(a).unwrap()
            );
            assert!(pixels(frame.resolve()) == pixels(control));
        }
    }
}

#[test]
fn row_content_bases_auto_minima_intrinsic_limits_and_effective_image_ratio_reject() {
    for bad in 0..5 {
        let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui.append_render_layout(scope, root, item(1.0), 0).unwrap();
        let mut style = item(1.0);
        match bad {
            0 => style.flex_basis = LayoutDimension::Content,
            1 => style.min_size.width = LayoutDimension::Auto,
            2 => style.max_size.width = LayoutDimension::MaxContent,
            3 => style.aspect_ratio = Some(2.0),
            _ => {}
        }
        let sibling = ui.append_render_layout(scope, root, style, 0).unwrap();
        if bad == 4 {
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties()
                        .image(sibling, Some(ImageResource::rgba8([2, 1], vec![255; 8])?));
                    Ok(())
                },
            )
            .unwrap();
        }
        let mut called = false;
        assert!(matches!(
            ui.edit_frame(
                constraints(180.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| edit.inline_allocation_builder(host, |_, _| {
                    called = true;
                    Ok(())
                })
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
        assert!(!called);
    }
}

#[test]
fn row_query_validates_sibling_alternatives_and_detects_allocation_cycles() {
    for cycle in [false, true] {
        let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui.append_render_layout(scope, root, item(1.0), 0).unwrap();
        let sibling = ui.append_render_layout(scope, root, item(1.0), 0).unwrap();
        let target = ui
            .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
            .unwrap();
        let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
        let side_source = ui.state(scope, 1, || item(1.0)).unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        rules
            .bind_container(
                &mut ui,
                source,
                target,
                host,
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                    .unwrap(),
            )
            .unwrap();
        if cycle {
            let feedback = ui
                .computed(scope, 2, &[source.dependency()], move |input| {
                    Ok(item(
                        if input.read(source)?.size.height == LayoutDimension::Px(60.0) {
                            3.0
                        } else {
                            1.0
                        },
                    ))
                })
                .unwrap();
            ui.bind(feedback, Property::layout_style(sibling)).unwrap();
            assert!(matches!(
                rules.frame_in_viewport(
                    &mut ui,
                    &LayoutEnvironment::new([180.0; 2]),
                    constraints(180.0),
                    [180.0; 2]
                ),
                Err(Error::UnsafeContainerQuery)
            ));
        } else {
            let error = rules.bind(
                &mut ui,
                side_source,
                sibling,
                LayoutBreakpointStyles::new(item(1.0), vec![(100.0, LayoutStyle::column())])
                    .unwrap(),
            );
            assert!(matches!(error, Err(Error::UnsafeContainerQuery)));
            assert_eq!(ui.reactive().binding_count(), 1);
        }
    }
}

#[test]
fn row_observations_reject_equal_width_item_or_order_dependency_changes_and_preserve_sources() {
    for mode in 0..3 {
        let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui.append_render_layout(scope, root, item(1.0), 0).unwrap();
        let sibling = ui.append_render_layout(scope, root, item(1.0), 0).unwrap();
        let target = ui
            .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
            .unwrap();
        let source = ui.state(scope, 0, || item(1.0)).unwrap();
        ui.bind(source, Property::layout_style(sibling)).unwrap();
        let before = ui
            .frame_in_viewport(constraints(180.0), [180.0; 2])
            .unwrap();
        let stats = ui.stats();
        let result = ui.edit_local_frame(
            constraints(180.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |_, edit| {
                    edit.properties().layout_style(target, fixed(400.0, 60.0));
                    match mode {
                        0 => edit.properties().layout_style(
                            sibling,
                            item(1.0).size(LayoutDimension::Px(70.0), LayoutDimension::Auto),
                        ),
                        1 => edit.set_children(root, vec![sibling, host])?,
                        _ => {
                            let branch = edit.reconcile_keys(scope, &[1])?[0];
                            edit.append_layout(branch, root, item(0.0), 0)?;
                        }
                    }
                    Ok(())
                })
            },
        );
        assert!(
            matches!(result, Err(Error::UnsafeConstraintBuilder)),
            "mode {mode}"
        );
        assert_eq!(ui.read(source).unwrap(), &item(1.0));
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_row_inline_allocation_matches_full_reference() {
    let mut ui = LogicalUi::new_layout(root(false), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, item(1.0), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(
        scope,
        root,
        item(1.0).size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
        0x00ff00ff,
    )
    .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("Row allocation adapter: {:?}", gpu.adapter_info());
    for w in [120.0, 180.0, 240.0, 120.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        let branch = edit.reconcile_keys(
                            scope,
                            &[u64::from(input.known_width.unwrap() >= 90.0)],
                        )?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
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
