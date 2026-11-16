use super::*;
use crate::retained::{
    BackendCapabilities, FrameBuildPolicy, FramePacket, Transaction, UiTree, logical::LogicalUi,
    pacing, responsive::*, software,
};

fn overflow(value: &str) -> AlignmentOverflow {
    match value {
        "default" => AlignmentOverflow::Default,
        "safe" => AlignmentOverflow::Safe,
        "unsafe" => AlignmentOverflow::Unsafe,
        _ => panic!("invalid reference"),
    }
}
fn item(value: &str) -> AlignItems {
    match value {
        "normal" => AlignItems::Normal,
        "start" => AlignItems::Start,
        "end" => AlignItems::End,
        "flex-start" => AlignItems::FlexStart,
        "flex-end" => AlignItems::FlexEnd,
        "self-start" => AlignItems::SelfStart,
        "self-end" => AlignItems::SelfEnd,
        "center" => AlignItems::Center,
        "stretch" => AlignItems::Stretch,
        _ => panic!("invalid reference"),
    }
}
fn own(value: &str) -> AlignSelf {
    match value {
        "normal" => AlignSelf::Normal,
        "start" => AlignSelf::Start,
        "end" => AlignSelf::End,
        "flex-start" => AlignSelf::FlexStart,
        "flex-end" => AlignSelf::FlexEnd,
        "self-start" => AlignSelf::SelfStart,
        "self-end" => AlignSelf::SelfEnd,
        "center" => AlignSelf::Center,
        "stretch" => AlignSelf::Stretch,
        _ => panic!("invalid reference"),
    }
}
fn content(value: &str) -> AlignContent {
    match value {
        "normal" => AlignContent::Normal,
        "start" => AlignContent::Start,
        "end" => AlignContent::End,
        "flex-start" => AlignContent::FlexStart,
        "flex-end" => AlignContent::FlexEnd,
        "center" => AlignContent::Center,
        "stretch" => AlignContent::Stretch,
        "space-between" => AlignContent::SpaceBetween,
        "space-around" => AlignContent::SpaceAround,
        "space-evenly" => AlignContent::SpaceEvenly,
        _ => panic!("invalid reference"),
    }
}
fn justify(value: &str) -> JustifyContent {
    match value {
        "normal" => JustifyContent::Normal,
        "start" => JustifyContent::Start,
        "end" => JustifyContent::End,
        "flex-start" => JustifyContent::FlexStart,
        "flex-end" => JustifyContent::FlexEnd,
        "center" => JustifyContent::Center,
        "stretch" => JustifyContent::Stretch,
        "space-between" => JustifyContent::SpaceBetween,
        "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly,
        _ => panic!("invalid reference"),
    }
}

#[test]
fn public_alignment_geometry_matches_independent_chromium_reference() {
    let mut mismatches = vec![];
    let mut groups = std::collections::BTreeMap::<String, (usize, String)>::new();
    let mut cases = 0;
    let mut deviations = 0;
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-flex-alignment/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let direction = match f[1] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => panic!("invalid reference"),
        };
        let wrap = match f[3] {
            "nowrap" => FlexWrap::NoWrap,
            "wrap" => FlexWrap::Wrap,
            "wrap-reverse" => FlexWrap::WrapReverse,
            _ => panic!("invalid reference"),
        };
        let mut root = LayoutStyle::row()
            .size(Dimension::Px(100.0), Dimension::Px(60.0))
            .flex_direction(direction)
            .flex_wrap(wrap)
            .direction(if f[2] == "true" {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            })
            .align_items(AlignItems::Start);
        let w: f32 = f[7].parse().unwrap();
        let h: f32 = f[8].parse().unwrap();
        let mut child = LayoutStyle::row()
            .size(Dimension::Px(w), Dimension::Px(h))
            .flex_shrink(0.0)
            .direction(if f[4] == "true" {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            });
        match f[0] {
            "main" => {
                root = root.justify_content(Alignment {
                    position: justify(f[5]),
                    overflow: overflow(f[6]),
                })
            }
            "items" => {
                root = root.align_items(Alignment {
                    position: item(f[5]),
                    overflow: overflow(f[6]),
                })
            }
            "self" => {
                child = child.align_self(Alignment {
                    position: own(f[5]),
                    overflow: overflow(f[6]),
                })
            }
            "content" => {
                root = root.align_content(Alignment {
                    position: content(f[5]),
                    overflow: overflow(f[6]),
                })
            }
            _ => panic!("invalid reference"),
        }
        let mut tree = UiTree::new_layout(root, 0).unwrap();
        let count: usize = f[9].parse().unwrap();
        let nodes: Vec<_> = (0..count)
            .map(|_| tree.append_layout(tree.root(), child.clone(), 1).unwrap())
            .collect();
        tree.frame([100.0, 60.0]).unwrap();
        let actual: Vec<f32> = nodes
            .into_iter()
            .flat_map(|id| {
                let r = tree.layout_result(id).unwrap();
                [r.location[0], r.location[1], r.size[0], r.size[1]]
            })
            .collect();
        let expected: Vec<f32> = f[10..].iter().map(|v| v.parse().unwrap()).collect();
        assert_eq!(actual.len(), expected.len());
        // Chromium 145 keeps flex-start for overflowing space-between, while
        // current CSS Flexbox requires safe flex-start -> logical start. Keep
        // this exact, enumerated difference separate from compatibility passes.
        let axis = usize::from(
            f[0] == "main" && !f[1].starts_with("row")
                || f[0] == "content" && f[1].starts_with("row"),
        );
        let reversed_fallback = f[5] == "space-between"
            && f[6] == "default"
            && ((f[0] == "main" && f[1].ends_with("reverse"))
                || (f[0] == "content" && f[3] == "wrap-reverse"));
        let extent = if axis == 0 { w } else { h };
        let free = (if axis == 0 { 100.0 } else { 60.0 }) - extent * count as f32;
        if reversed_fallback && free < 0.0 {
            let physical_sign = if axis == 0 && f[2] == "true" {
                -1.0
            } else {
                1.0
            };
            for index in 0..actual.len() {
                let target = expected[index]
                    + if index % 4 == axis {
                        -free * physical_sign
                    } else {
                        0.0
                    };
                assert!(
                    (actual[index] - target).abs() <= 0.02,
                    "explicit space-between deviation: {line}"
                );
            }
            deviations += 1;
            continue;
        }
        // Browser CSS layout quantizes fractional distribution to 1/64px.
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            let group = groups.entry(f[..5].join("/")).or_default();
            group.0 += 1;
            if group.1.is_empty() {
                group.1 = format!(
                    "{} actual={actual:?} browser={expected:?}",
                    f[5..10].join("/")
                );
            }
            mismatches.push(format!(
                "{}: actual={actual:?} browser={expected:?}",
                f[..10].join("/")
            ));
        }
        cases += 1;
    }
    assert_eq!(cases, 5364);
    assert_eq!(deviations, 12);
    if !groups.is_empty() {
        eprintln!("mismatch groups: {groups:#?}");
    }
    assert!(
        mismatches.is_empty(),
        "{} / {cases} failed:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn invalid_alignment_modifiers_reject_before_tree_or_transaction_mutation() {
    for style in [
        LayoutStyle::row().align_items(AlignItems::Stretch.safe()),
        LayoutStyle::row().align_items(AlignItems::Baseline.unsafe_overflow()),
        LayoutStyle::row().align_self(AlignSelf::Auto.safe()),
        LayoutStyle::row().align_self(AlignSelf::Normal.unsafe_overflow()),
        LayoutStyle::row().align_content(AlignContent::SpaceBetween.safe()),
        LayoutStyle::row().justify_content(JustifyContent::Stretch.safe()),
    ] {
        assert_eq!(style.validate(), Err(Error::InvalidStyle));
        let mut tree = UiTree::new_layout(LayoutStyle::row(), 0).unwrap();
        let before = tree.frame([100.0, 60.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0xff0000ff);
        tx.layout_style(tree.root(), style);
        assert!(matches!(
            tree.commit(tx, BackendCapabilities::default()),
            Err(Error::InvalidStyle)
        ));
        let after = tree.frame([100.0, 60.0]).unwrap();
        assert_eq!(before.revision, after.revision);
        assert!(std::sync::Arc::ptr_eq(&before.items, &after.items));
    }
}

fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [200, 160],
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
fn host_style(safe: bool) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(100.0), Dimension::Px(60.0))
        .align_items(if safe {
            AlignItems::Center.safe()
        } else {
            AlignItems::Center.unsafe_overflow()
        })
        .justify_content(JustifyContent::Center.safe())
}
fn overlarge_child() -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(140.0), Dimension::Px(100.0))
        .flex_shrink(0.0)
}
fn ordinary_reference(safe: bool) -> FramePacket {
    // Independent geometry, with no alignment keywords to repeat the algorithm.
    let mut full = UiTree::new_layout(
        LayoutStyle::row().size(Dimension::Px(200.0), Dimension::Px(160.0)),
        0,
    )
    .unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = full
        .append_layout(
            full.root(),
            LayoutStyle::row()
                .size(Dimension::Px(100.0), Dimension::Px(60.0))
                .align_items(AlignItems::Start),
            0,
        )
        .unwrap();
    let leaf = full
        .append_layout(host, overlarge_child(), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.translation(leaf, [0.0, if safe { 0.0 } else { -20.0 }]);
    full.commit(tx, BackendCapabilities::default()).unwrap();
    full.frame([200.0, 160.0]).unwrap()
}
fn fixture() -> (LogicalUi, crate::retained::NodeId, crate::retained::NodeId) {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row().size(Dimension::Px(200.0), Dimension::Px(160.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), host_style(true), 0)
        .unwrap();
    let child = ui
        .append_render_layout(scope, host, overlarge_child(), 0xff0000ff)
        .unwrap();
    (ui, host, child)
}

#[test]
fn responsive_alignment_sources_publish_local_and_resolved_pixels_and_noops() {
    for local in [false, true] {
        let (mut ui, host, leaf) = fixture();
        let scope = ui.root_scope();
        let source = ui.state(scope, 10, || host_style(true)).unwrap();
        let mut old = None;
        let mut previous = None;
        for width in [60.0, 60.0, 140.0, 140.0, 60.0] {
            let safe = width < 100.0;
            let render = |(): (), edit: &mut crate::retained::logical::LogicalEdit<'_, '_, '_>| {
                edit.responsive_rule(
                    scope,
                    1,
                    source,
                    host,
                    LayoutQuery::ViewportInline,
                    LayoutBreakpointStyles::new(
                        host_style(true),
                        vec![(100.0, host_style(false))],
                    )?,
                )?;
                edit.responsive_registered(&LayoutEnvironment::new([width, 160.0]))?;
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(200.0, 160.0),
                    [200.0, 160.0],
                    |_| Ok(()),
                    render,
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(200.0, 160.0),
                    [200.0, 160.0],
                    |_| Ok(()),
                    render,
                )
                .unwrap()
                .1
            };
            assert_eq!(ui.read(source).unwrap(), &host_style(safe));
            assert_eq!(
                ui.layout_result(leaf).unwrap().location,
                [0.0, if safe { 0.0 } else { -20.0 }]
            );
            assert_eq!(pixels(frame.clone()), pixels(ordinary_reference(safe)));
            assert_eq!(
                frame.hit_test(1.0, 90.0),
                if safe {
                    Some(leaf)
                } else {
                    Some(ui.root_render())
                }
            );
            if let Some((previous_safe, revision)) = previous {
                if previous_safe == safe {
                    assert_eq!(frame.revision, revision);
                }
            }
            previous = Some((safe, frame.revision));
            old.get_or_insert_with(|| (frame.clone(), pixels(frame)));
        }
        let before = ui.published_frame().unwrap().revision;
        let stats = ui.stats();
        let failed = ui.edit_frame(
            LayoutConstraints::viewport(200.0, 160.0),
            [200.0, 160.0],
            |tx| {
                tx.set(source, host_style(false))?;
                Ok(())
            },
            |(), edit| {
                edit.properties().layout_style(
                    host,
                    host_style(true).align_items(AlignItems::Stretch.safe()),
                );
                Ok(())
            },
        );
        assert!(matches!(failed, Err(Error::InvalidStyle)));
        assert_eq!(ui.read(source).unwrap(), &host_style(true));
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, before);
        drop(ui);
        let (old, expected) = old.unwrap();
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn auto_margins_precede_safe_alignment_and_normal_cross_alignment_stretches() {
    for safe in [false, true] {
        let mut tree = UiTree::new_layout(host_style(safe), 0).unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::row()
                    .size(Dimension::Px(20.0), Dimension::Px(20.0))
                    .margin(Edges {
                        left: Length::Auto,
                        top: Length::Auto,
                        ..Edges::zero()
                    }),
                1,
            )
            .unwrap();
        tree.frame([100.0, 60.0]).unwrap();
        assert_eq!(tree.layout_result(child).unwrap().location, [80.0, 40.0]);
    }
    let mut tree = UiTree::new_layout(host_style(true).align_items(AlignItems::Normal), 0).unwrap();
    let child = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row().size(Dimension::Px(20.0), Dimension::Auto),
            1,
        )
        .unwrap();
    tree.frame([100.0, 60.0]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [20.0, 60.0]);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_safe_alignment_updates_match_independent_full_positions() {
    let (mut ui, host, _) = fixture();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("alignment adapter: {:?}", renderer.adapter_info());
    for safe in [true, false, true, false] {
        let (_, frame) = ui
            .edit_local_frame(
                LayoutConstraints::viewport(200.0, 160.0),
                [200.0, 160.0],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().layout_style(host, host_style(safe));
                    Ok(())
                },
            )
            .unwrap();
        renderer
            .submit_local(&frame, [200, 160], 1.0)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&ordinary_reference(safe), [200, 160], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();
}
