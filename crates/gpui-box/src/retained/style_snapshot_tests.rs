use super::*;
use std::{hint::black_box, time::Instant};

fn scene(grid: bool) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
    )
    .unwrap();
    let (group, leaf) = tree
        .edit(BackendCapabilities::default(), |edit| {
            let mut first = None;
            for _ in 0..128 {
                let style = if grid {
                    LayoutStyle::grid(
                        GridStyle::default().columns(
                            (0..16)
                                .map(|_| GridTemplate::Track(GridTrack::px(12.0)))
                                .collect(),
                        ),
                    )
                } else {
                    LayoutStyle::row()
                };
                let group = edit.append_layout(edit.root(), style, 0)?;
                for _ in 0..16 {
                    let leaf = edit.append_layout(
                        group,
                        LayoutStyle::row().size(LayoutDimension::Px(8.0), LayoutDimension::Px(8.0)),
                        0xff0000ff,
                    )?;
                    first.get_or_insert((group, leaf));
                }
            }
            Ok(first.unwrap())
        })
        .unwrap()
        .value;
    tree.measure_with_constraints(bounds(512.0)).unwrap();
    (tree, group, leaf)
}

fn bounds(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}

fn sample(name: &str, samples: usize, mut run: impl FnMut(usize)) {
    for i in 0..12 {
        run(i);
    }
    let mut times = Vec::with_capacity(samples);
    for i in 0..samples {
        let start = Instant::now();
        run(i);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "STYLE_BENCH,{name},{samples},{:.3},{:.3},{:.3}",
        times[samples / 2],
        times[(samples * 95 / 100).min(samples - 1)],
        times[samples - 1],
    );
}

/// Run the exact same executable before/after changing snapshot storage. Each
/// timed operation includes destruction of its temporary draft/result. This is
/// owner work, without GPU, presentation or an application timing claim.
#[test]
#[ignore = "release-mode comparative timing; run explicitly with one test thread"]
fn style_snapshot_benchmark() {
    let samples = std::env::var("GPUI_STYLE_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    assert!(samples >= 20);
    println!(
        "STYLE_SIZE,Style,{}\nSTYLE_SIZE,Node,{}\nSTYLE_SIZE,Slot,{}",
        std::mem::size_of::<Style>(),
        std::mem::size_of::<Node>(),
        std::mem::size_of::<Slot>(),
    );
    for grid in [false, true] {
        let name = if grid { "grid" } else { "flex" };
        let (mut tree, group, leaf) = scene(grid);
        sample(&format!("{name}_clone"), samples, |_| {
            black_box(tree.clone_for_layout_plan());
        });
        sample(&format!("{name}_dry_subtree"), samples, |i| {
            black_box(
                tree.measure_node_with_constraints(group, bounds(180.0 + (i % 2) as f32))
                    .unwrap(),
            );
        });
        sample(&format!("{name}_atomic_layout"), samples, |i| {
            black_box(
                tree.edit_validated(
                    BackendCapabilities::default(),
                    |edit| {
                        edit.properties().layout_style(
                            leaf,
                            LayoutStyle::row().size(
                                LayoutDimension::Px(8.0 + (i % 2) as f32),
                                LayoutDimension::Px(8.0),
                            ),
                        );
                        Ok(())
                    },
                    |draft| {
                        black_box(draft.measure_with_constraints(bounds(512.0))?);
                        Ok(())
                    },
                )
                .unwrap(),
            );
        });
        sample(&format!("{name}_dry_root"), samples, |i| {
            black_box(
                tree.measure_with_constraints(bounds(500.0 + (i % 2) as f32))
                    .unwrap(),
            );
        });
        sample(&format!("{name}_create_layout"), samples, |_| {
            black_box(scene(grid));
        });
    }
}

#[test]
fn candidates_share_styles_but_replace_only_their_own_layout_inputs() {
    let (mut owner, group, leaf) = scene(true);
    owner.frame([512.0, 1400.0]).unwrap();
    let original = owner.node(group).unwrap().style.clone();
    assert!(std::ptr::eq(
        original.as_ref(),
        owner
            .layout
            .style(owner.node(group).unwrap().layout)
            .unwrap(),
    ));
    let mut candidate = owner.clone_for_layout_plan();
    assert!(Arc::ptr_eq(
        &original,
        &candidate.node(group).unwrap().style
    ));
    assert!(std::ptr::eq(
        original.as_ref(),
        candidate
            .layout
            .style(candidate.node(group).unwrap().layout)
            .unwrap(),
    ));
    let mut replacement = owner.style(group).unwrap().clone();
    replacement.grid_template_columns.clear();
    replacement.display = taffy::Display::Flex;
    let mut tx = Transaction::default();
    tx.style(group, replacement);
    candidate
        .commit(tx, BackendCapabilities::default())
        .unwrap();
    candidate.frame([512.0, 1400.0]).unwrap();
    assert_eq!(owner.style(group).unwrap().display, taffy::Display::Grid);
    assert_eq!(original.grid_template_columns.len(), 16);
    assert_eq!(
        candidate.style(group).unwrap().display,
        taffy::Display::Flex
    );
    assert!(Arc::ptr_eq(
        &owner.node(leaf).unwrap().style,
        &candidate.node(leaf).unwrap().style,
    ));
    let mut tx = Transaction::default();
    tx.style(group, owner.style(group).unwrap().clone());
    let revision = owner.revision;
    let result = owner.commit(tx, BackendCapabilities::default()).unwrap();
    assert_eq!(result.revision, revision);
    assert!(!result.request_frame);
    assert!(Arc::ptr_eq(&original, &owner.node(group).unwrap().style));
    drop(owner);
    assert_eq!(original.grid_template_columns.len(), 16);
    assert_eq!(candidate.style(leaf).unwrap().size.width, length(8.0));
}

#[test]
fn image_effective_snapshots_do_not_change_authored_style_or_old_candidates() {
    let mut owner = UiTree::new(Style::default(), 0).unwrap();
    let node = owner
        .append(owner.root(), Style::default(), 0xff0000ff)
        .unwrap();
    let authored = owner.node(node).unwrap().style.clone();
    let mut tx = Transaction::default();
    tx.measure(
        node,
        Some(MeasureSnapshot::Image(ImageIntrinsicSize {
            width: 40.0,
            height: 20.0,
        })),
    );
    owner.commit(tx, BackendCapabilities::default()).unwrap();
    let layout_id = owner.node(node).unwrap().layout;
    assert!(!authored.item_is_replaced);
    assert_eq!(authored.aspect_ratio, None);
    assert!(Arc::ptr_eq(&authored, &owner.node(node).unwrap().style));
    assert_eq!(
        owner.layout.style(layout_id).unwrap().aspect_ratio,
        Some(2.0)
    );
    let old = owner.clone_for_layout_plan();
    let mut tx = Transaction::default();
    tx.measure(
        node,
        Some(MeasureSnapshot::Image(ImageIntrinsicSize {
            width: 20.0,
            height: 40.0,
        })),
    );
    owner.commit(tx, BackendCapabilities::default()).unwrap();
    assert_eq!(old.layout.style(layout_id).unwrap().aspect_ratio, Some(2.0));
    assert_eq!(
        owner.layout.style(layout_id).unwrap().aspect_ratio,
        Some(0.5)
    );
    let mut tx = Transaction::default();
    tx.measure(node, None);
    owner.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(std::ptr::eq(
        authored.as_ref(),
        owner.layout.style(layout_id).unwrap()
    ));
    assert!(!owner.layout.style(layout_id).unwrap().item_is_replaced);
    for (contain, ratio, replaced) in [
        (taffy::Contain::INLINE_SIZE, None, true),
        (taffy::Contain::NONE, Some(3.0), true),
    ] {
        let style = Arc::new(Style {
            contain,
            aspect_ratio: ratio,
            item_is_replaced: replaced,
            ..Style::default()
        });
        let image = MeasureSnapshot::Image(ImageIntrinsicSize {
            width: 40.0,
            height: 20.0,
        });
        assert!(Arc::ptr_eq(
            &style,
            &measure::effective_style(&style, Some(&image))
        ));
    }
}

#[test]
fn rejected_layout_candidate_preserves_shared_style_and_published_frame() {
    let mut tree = UiTree::new(
        Style {
            size: Size {
                width: length(80.0),
                height: length(40.0),
            },
            ..Style::default()
        },
        0xff0000ff,
    )
    .unwrap();
    let root = tree.root();
    let before = tree.frame([80.0, 40.0]).unwrap();
    let style = tree.node(root).unwrap().style.clone();
    let revision = tree.revision;
    let result = tree.edit_validated(
        BackendCapabilities::default(),
        |edit| {
            edit.properties().layout_style(
                root,
                LayoutStyle::column().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0)),
            );
            Ok(())
        },
        |candidate| {
            candidate.frame([80.0, 40.0])?;
            Err(Error::InvalidMeasure)
        },
    );
    assert!(matches!(result, Err(Error::InvalidMeasure)));
    assert_eq!(tree.revision, revision);
    assert!(Arc::ptr_eq(&style, &tree.node(root).unwrap().style));
    assert!(std::ptr::eq(
        style.as_ref(),
        tree.layout.style(tree.node(root).unwrap().layout).unwrap()
    ));
    assert_eq!(
        tree.frame([80.0, 40.0]).unwrap().hit_test(60.0, 20.0),
        before.hit_test(60.0, 20.0)
    );
}
