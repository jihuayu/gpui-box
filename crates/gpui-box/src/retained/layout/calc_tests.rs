use super::*;
use crate::retained::{
    BackendCapabilities, FrameBuildPolicy, FramePacket, NodeId, UiTree, logical::LogicalUi, pacing,
    reactive::Property, responsive::*, software,
};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}
fn calc_tree(f: &[&str]) -> (UiTree, Vec<NodeId>) {
    let kind = f[0];
    let w: f32 = f[2].parse().unwrap();
    let mut style = if kind.starts_with("grid") {
        LayoutStyle::grid(GridStyle::default().columns(vec![
                GridTrack::minmax(
                    GridMinTrack::Calc {
                        px: 2.0,
                        percent: 0.05,
                    },
                    GridMaxTrack::Fr(1.0),
                )
                .into(),
                GridTrack::calc(20.0, 0.2).into(),
            ]))
    } else {
        LayoutStyle::row()
    };
    if kind == "grid-fit" {
        style.grid = Some(std::sync::Arc::new(GridStyle::default().columns(vec![
            GridTrack::fit_content_calc(20.0, 0.2).into(),
            GridTrack::fr(1.0).into(),
        ])));
    }
    if kind == "grid-repeat" {
        style.grid = Some(std::sync::Arc::new(GridStyle::default().columns(vec![
            GridTemplate::repeat(
                GridRepetition::AutoFit,
                vec![GridTrack::minmax(
                    GridMinTrack::Calc {
                        px: 20.0,
                        percent: 0.1,
                    },
                    GridMaxTrack::Fr(1.0),
                )],
            ),
        ])));
    }
    style = style
        .size(Dimension::Px(w), Dimension::Px(160.0))
        .flex_direction(match kind {
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            _ => FlexDirection::Row,
        })
        .padding(Edges::all(Length::calc(2.0, 0.01)))
        .gap(Length::calc(3.0, 0.01), Length::calc(3.0, 0.01))
        .align_items(AlignItems::Start)
        .align_content(AlignContent::Start)
        .justify_content(JustifyContent::Start)
        .direction(if f[1] == "true" {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        });
    // Browser fixture's positioned host establishes the absolute containing block.
    style.position = LayoutPosition::Relative;
    let mut tree = UiTree::new_layout(style, 0).unwrap();
    let mut nodes = vec![];
    for i in 0..3 {
        let mut style = LayoutStyle::column()
            .min_size(Dimension::Px(0.0), Dimension::Auto)
            .padding(Edges::all(Length::calc(1.0, 0.01)))
            .margin(Edges {
                left: Length::calc(-2.0, 0.01),
                right: Length::calc(-2.0, 0.01),
                top: Length::Px(0.0),
                bottom: Length::Px(0.0),
            });
        style.border = Edges::all(Length::Px(1.0));
        if f[4] == "content-box" {
            style.box_sizing = BoxSizing::ContentBox;
        }
        let value = if f[3] == "true" {
            Dimension::calc(-100.0, 0.1)
        } else {
            Dimension::calc(-8.0, 0.3)
        };
        match kind {
            "row" | "row-reverse" => {
                style = style
                    .flex_basis(value)
                    .flex_grow((i + 1) as f32)
                    .min_size(Dimension::calc(-5.0, 0.1), Dimension::Auto)
                    .max_size(Dimension::calc(50.0, 0.3), Dimension::Auto);
            }
            "column" => {
                style = style
                    .size(value, Dimension::calc(5.0, 0.2))
                    .min_size(Dimension::Px(0.0), Dimension::calc(-5.0, 0.05))
                    .flex_shrink(0.0);
            }
            "absolute" => {
                style = style.size(value, Dimension::calc(3.0, 0.2));
                style.position = LayoutPosition::Absolute;
                style.inset.left = Length::calc(i as f32 * 20.0, 0.05);
                style.inset.top = Length::calc(i as f32 * 25.0, 0.05);
            }
            _ => {}
        }
        let node = tree.append_layout(tree.root(), style, 0).unwrap();
        nodes.push(node);
        tree.append_layout(
            node,
            fixed(
                300.0,
                [20.0, 30.0, 40.0][i] + if f[5] == "true" { 30.0 } else { 0.0 },
            ),
            0,
        )
        .unwrap();
    }
    (tree, nodes)
}
#[test]
fn owned_calc_geometry_matches_independent_browser() {
    let mut failures = vec![];
    let mut count = 0;
    let mut groups = std::collections::BTreeMap::<String, f32>::new();
    for line in
        include_str!("../../../../../docs/performance/2026-11-17-owned-calc/browser-reference.tsv")
            .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let (mut tree, nodes) = calc_tree(&f);
        tree.frame([400.0; 2]).unwrap();
        let r = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![r.size[0], r.size[1]];
        for node in nodes {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = f[6..].iter().map(|v| v.parse().unwrap()).collect();
        let delta = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        groups
            .entry(f[0].to_owned())
            .and_modify(|v| *v = v.max(delta))
            .or_insert(delta);
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.1)
        {
            failures.push(format!("{:?}: {actual:?} != {expected:?}", &f[..6]));
        }
        count += 1;
    }
    assert_eq!(count, 336);
    eprintln!("calc reference maximum deltas by mode: {groups:?}");
    assert!(
        failures.is_empty(),
        "{} failures: {:?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}

fn pixels(frame: FramePacket) -> Vec<u8> {
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
fn mixed_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(Dimension::calc(-12.0, 0.5), Dimension::Auto)
        .padding(Edges::all(Length::calc(2.0, 0.01)))
        .flex_shrink(0.0)
}
fn root() -> LayoutStyle {
    LayoutStyle::column().size(Dimension::Percent(1.0), Dimension::Auto)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(AvailableSpace::Definite(w), AvailableSpace::Unbounded)
}
fn control(w: f32, host_width: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(root(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column()
                .size(Dimension::Px(host_width), Dimension::Auto)
                .padding(Edges::all(Length::Px(2.0 + w * 0.01)))
                .flex_shrink(0.0),
            0x0000ffff,
        )
        .unwrap();
    tree.append_layout(host, fixed(100.0, 30.0), 0xff0000ff)
        .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}
#[test]
fn owned_calc_typed_updates_p0_and_invalid_rollback_match_numeric_reference() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, ui.root_render(), mixed_style(), 0x0000ffff)
            .unwrap();
        ui.append_render_layout(scope, host, fixed(100.0, 30.0), 0xff0000ff)
            .unwrap();
        let source = ui.state(scope, 0, mixed_style).unwrap();
        ui.bind(source, Property::layout_style(host)).unwrap();
        let mut old = None;
        for w in [120.0, 240.0, 160.0, 120.0] {
            let frame = if local {
                ui.local_frame_in_viewport(constraints(w), [180.0; 2])
                    .unwrap()
                    .resolve()
            } else {
                ui.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
            };
            assert!(pixels(frame.clone()) == pixels(control(w, w * 0.5 - 12.0)));
            old.get_or_insert(frame);
        }
        for px in [-4.0, -20.0, -12.0] {
            let mut style = mixed_style();
            style.size.width = Dimension::calc(px, 0.5);
            ui.transaction(|tx| tx.set(source, style)).unwrap();
            let frame = if local {
                ui.local_frame_in_viewport(constraints(120.0), [180.0; 2])
                    .unwrap()
                    .resolve()
            } else {
                ui.frame_in_viewport(constraints(120.0), [180.0; 2])
                    .unwrap()
            };
            assert!(pixels(frame) == pixels(control(120.0, 60.0 + px)));
        }
        let before = ui.published_frame().unwrap().revision;
        ui.transaction(|tx| {
            tx.set(source, fixed(20.0, 20.0))?;
            tx.set(source, mixed_style())
        })
        .unwrap();
        assert_eq!(ui.published_frame().unwrap().revision, before);
        let stats = ui.stats();
        for bad in [f32::NAN, f32::INFINITY] {
            let mut style = mixed_style();
            style.size.width = Dimension::calc(bad, 0.5);
            assert!(matches!(
                ui.transaction(|tx| tx.set(source, style)),
                Err(Error::InvalidStyle)
            ));
        }
        assert_eq!(ui.read(source).unwrap(), &mixed_style());
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, before);
        assert_eq!(
            ui.frame_in_viewport(constraints(120.0), [180.0; 2])
                .unwrap()
                .layout_runs,
            0
        );
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn owned_calc_basis_builders_and_queries_share_definite_chain_proof() {
    for mode in [0, 1] {
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::row()
                .size(Dimension::Percent(1.0), Dimension::Auto)
                .align_items(AlignItems::Start),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let item = || {
            LayoutStyle::column()
                .flex_basis(Dimension::calc(-10.0, 0.5))
                .min_size(Dimension::calc(-5.0, 0.1), Dimension::Auto)
                .flex_grow(1.0)
        };
        let host = ui.append_render_layout(scope, root, item(), 0).unwrap();
        ui.append_render_layout(
            scope,
            root,
            item().size(Dimension::Auto, Dimension::Px(10.0)),
            0,
        )
        .unwrap();
        for w in [120.0, 240.0, 160.0] {
            ui.edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        assert_eq!(input.known_width, Some(w * 0.5));
                        assert!(input.known_width_is_definite);
                        let branch = edit.reconcile_keys(scope, &[mode])?[0];
                        let old = edit.renders(branch)?.to_vec();
                        let target = if let Some(&n) = old.first() {
                            n
                        } else {
                            edit.append_layout(branch, host, fixed(20.0, 20.0), 1)?
                        };
                        let source = edit.state(branch, 0, || fixed(20.0, 20.0))?;
                        edit.responsive_rule(
                            branch,
                            0,
                            source,
                            target,
                            LayoutQuery::ContainerInline(host),
                            LayoutBreakpointStyles::new(
                                fixed(20.0, 20.0),
                                vec![(90.0, fixed(20.0, 60.0))],
                            )?,
                        )?;
                        edit.responsive_registered(&LayoutEnvironment::new([500.0; 2]))?;
                        Ok(())
                    })
                },
            )
            .unwrap();
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [w * 0.5, if w >= 180.0 { 60.0 } else { 20.0 }]
            );
        }
    }
}

#[test]
fn owned_calc_indefinite_basis_does_not_become_numeric_and_hard_bounds_still_win() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(Dimension::calc(50.0, 0.0), Dimension::Auto),
        0,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 30.0), 1)
        .unwrap();
    let unbounded = LayoutConstraints::new(AvailableSpace::Unbounded, AvailableSpace::Unbounded);
    tree.frame_in_viewport(unbounded, [180.0; 2]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [20.0, 30.0]);
    tree.frame_in_viewport(
        unbounded.with_bounds(BoxConstraints::tight([40.0, 10.0])),
        [180.0; 2],
    )
    .unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [40.0, 10.0]);
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(Dimension::calc(50.0, 0.0), Dimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let host = ui
        .append_render_layout(ui.root_scope(), ui.root_render(), LayoutStyle::column(), 0)
        .unwrap();
    let mut called = false;
    assert!(matches!(
        ui.edit_frame(
            unbounded,
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

#[test]
fn owned_calc_fit_content_limits_and_constraint_boxes_match_numeric_inputs() {
    for kind in 0..3 {
        for w in [120.0, 240.0, 160.0] {
            let host = |calc: bool| {
                let mut s = if kind == 2 {
                    LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
                } else {
                    LayoutStyle::column()
                };
                s.size.width = if kind == 0 {
                    if calc {
                        Dimension::fit_content_calc(-12.0, 0.5)
                    } else {
                        Dimension::FitContentPx(w * 0.5 - 12.0)
                    }
                } else {
                    Dimension::Auto
                };
                if kind == 1 {
                    s.max_size.width = if calc {
                        Dimension::fit_content_calc(-12.0, 0.5)
                    } else {
                        Dimension::FitContentPx(w * 0.5 - 12.0)
                    };
                }
                if kind == 2 {
                    s.size.width = if calc {
                        Dimension::calc(-12.0, 0.5)
                    } else {
                        Dimension::Px(w * 0.5 - 12.0)
                    };
                }
                s.padding = Edges::all(if calc {
                    Length::calc(2.0, 0.01)
                } else {
                    Length::Px(2.0 + w * 0.01)
                });
                s
            };
            let build = |calc| {
                let mut tree = UiTree::new_layout(root(), 0).unwrap();
                let n = tree
                    .append_layout(tree.root(), host(calc), 0x0000ffff)
                    .unwrap();
                tree.append_layout(n, fixed(100.0, 30.0), 0xff0000ff)
                    .unwrap();
                let f = tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap();
                (tree.layout_result(n).unwrap(), f)
            };
            let (a, fa) = build(true);
            let (b, fb) = build(false);
            assert_eq!(a, b, "kind={kind} width={w}");
            assert_eq!(pixels(fa), pixels(fb));
        }
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_owned_calc_updates_match_numeric_reference() {
    let mut tree = UiTree::new_layout(root(), 0).unwrap();
    let host = tree
        .append_layout(tree.root(), mixed_style(), 0x0000ffff)
        .unwrap();
    tree.append_layout(host, fixed(100.0, 30.0), 0xff0000ff)
        .unwrap();
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("owned calc adapter: {:?}", gpu.adapter_info());
    for w in [120.0, 240.0, 160.0, 120.0] {
        let mut tx = crate::retained::Transaction::default();
        tx.layout_style(host, mixed_style());
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .local_frame_in_viewport(constraints(w), [180.0; 2])
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&control(w, w * 0.5 - 12.0), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    gpu.finish().unwrap();
}
