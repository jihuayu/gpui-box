//! A deliberately limited client policy: two explicit numeric columns and two
//! fixed rows. This consumes shared item metadata, not a framework Grid solver.
use super::*;
use crate::retained::{logical::LogicalUi, reactive::Property};
use std::sync::Mutex;

#[derive(Debug, Clone)]
struct ExplicitCells(Arc<Mutex<Vec<ChildLayoutStyle>>>);
fn cell(style: ChildLayoutStyle, width: f32) -> Result<([f32; 2], [f32; 2]), Error> {
    let column = match style.grid_column.start {
        GridPlacement::Line(1 | -3) => 0.0,
        GridPlacement::Line(2 | -2) => 1.0,
        _ => return Err(Error::InvalidParentLayout),
    };
    let span = match style.grid_column.end {
        GridPlacement::Auto | GridPlacement::Span(1) => 1.0,
        GridPlacement::Span(2) if column == 0.0 => 2.0,
        _ => return Err(Error::InvalidParentLayout),
    };
    let row = match style.grid_row.start {
        GridPlacement::Line(1 | -3) => 0.0,
        GridPlacement::Line(2 | -2) => 1.0,
        _ => return Err(Error::InvalidParentLayout),
    };
    if style.grid_row.end != GridPlacement::Auto {
        return Err(Error::InvalidParentLayout);
    }
    Ok((
        [column * width / 2.0, row * 30.0],
        [span * width / 2.0, 30.0],
    ))
}
fn slot(style: ChildLayoutStyle, width: f32) -> Result<ChildLayoutInput, Error> {
    let (_, area) = cell(style, width)?;
    let w = match style.size.width {
        LayoutDimension::Auto => area[0],
        LayoutDimension::Px(w) => w,
        _ => return Err(Error::InvalidParentLayout),
    };
    Ok(ChildLayoutInput {
        constraints: LayoutConstraints::new(
            LayoutAvailableSpace::Definite(area[0]),
            LayoutAvailableSpace::Definite(area[1]),
        )
        .with_bounds(BoxConstraints::tight([w, 10.0])),
        percentage_basis: LayoutSize::new(Some(area[0]), Some(area[1])),
        ..ChildLayoutInput::default()
    })
}
impl ParentAllocationLayout for ExplicitCells {
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = input
            .known_size
            .width
            .or(match input.constraints.width {
                LayoutAvailableSpace::Definite(width) => Some(width),
                _ => None,
            })
            .ok_or(Error::InvalidParentLayout)?;
        (0..children.len())
            .map(|i| {
                let style = children.style(i)?;
                self.0.lock().unwrap().push(style);
                slot(style, width)
            })
            .collect()
    }
}
impl ParentLayout for ExplicitCells {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let width = input.known_size.width.ok_or(Error::InvalidParentLayout)?;
        let mut placements = vec![];
        for child in 0..children.len() {
            let style = children.style(child)?;
            self.0.lock().unwrap().push(style);
            let (mut origin, area) = cell(style, width)?;
            let input = children
                .allocation(child)?
                .ok_or(Error::InvalidParentLayout)?;
            let measured = children.measure(child, input)?;
            origin[0] += match style.justify_self.position {
                LayoutAlignSelf::Auto | LayoutAlignSelf::Stretch | LayoutAlignSelf::Start => 0.0,
                LayoutAlignSelf::Center => (area[0] - measured.size[0]) / 2.0,
                LayoutAlignSelf::End => area[0] - measured.size[0],
                _ => return Err(Error::InvalidParentLayout),
            };
            placements.push(ParentPlacement {
                child,
                input,
                origin,
            });
        }
        Ok(ParentLayoutOutput {
            size: [width, input.known_size.height.unwrap_or(60.0)],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}
fn host(width: f32, grid: bool) -> LayoutStyle {
    let style = if grid {
        LayoutStyle::grid(
            GridStyle::default()
                .columns(vec![GridTrack::fr(1.0).into(); 2])
                .rows(vec![GridTrack::px(30.0).into(); 2]),
        )
    } else {
        LayoutStyle::row()
    };
    style.size(LayoutDimension::Px(width), LayoutDimension::Px(60.0))
}
fn item(index: usize, mode: usize) -> LayoutStyle {
    let span = if index == 0 && mode == 1 { 2 } else { 1 };
    let mut style = LayoutStyle::column()
        .size(
            if index == 1 {
                LayoutDimension::Px(12.0)
            } else {
                LayoutDimension::Auto
            },
            LayoutDimension::Px(10.0),
        )
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
        .grid_column(
            GridPlacement::Line(if index == 0 { 1 } else { -2 }),
            GridPlacement::Span(span),
        )
        .grid_row(
            GridPlacement::Line(if index == 0 || mode == 2 { 2 } else { -3 }),
            GridPlacement::Auto,
        )
        .align_self(LayoutAlignSelf::Start)
        .justify_self(if index == 1 {
            if mode == 2 {
                LayoutAlignSelf::Center.safe()
            } else {
                LayoutAlignSelf::End.unsafe_overflow()
            }
        } else {
            LayoutAlignSelf::Stretch.into()
        });
    if mode == 3 && index == 0 {
        style.display = LayoutDisplay::None;
    }
    style
}
fn reference(width: f32, mode: usize) -> (UiTree, [NodeId; 2], FramePacket) {
    let mut tree = UiTree::new_layout(host(width, true), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let nodes = [0, 1].map(|i| {
        tree.append_layout(tree.root(), item(i, mode), [0xff0000ff, 0x00ff00ff][i])
            .unwrap()
    });
    let frame = tree.frame([180.0; 2]).unwrap();
    (tree, nodes, frame)
}

#[test]
fn custom_grid_item_styles_drive_typed_slots_and_match_full_grid_cpu_frames() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(host(120.0, false), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let nodes = [0, 1].map(|i| {
            ui.append_render_layout(scope, root, item(i, 0), [0xff0000ff, 0x00ff00ff][i])
                .unwrap()
        });
        let source = ui.state(scope, 0, || item(1, 0)).unwrap();
        ui.bind(source, Property::layout_style(nodes[1])).unwrap();
        let observed = Arc::new(Mutex::new(vec![]));
        let policy = ExplicitCells(observed.clone());
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(
                    root,
                    Some(ParentLayoutSnapshot::new(policy.clone()).with_allocation_layout(policy)),
                );
                Ok(())
            },
        )
        .unwrap();
        let mut old = None;
        for (mode, width) in [(0, 120.0), (1, 160.0), (2, 100.0), (3, 120.0), (0, 120.0)] {
            let render = |(), edit: &mut crate::retained::logical::LogicalEdit<'_, '_, '_>| {
                edit.properties().layout_style(root, host(width, false));
                edit.properties().layout_style(nodes[0], item(0, mode));
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(width, 180.0),
                    [180.0; 2],
                    |tx| {
                        tx.set(source, item(1, mode))?;
                        Ok(())
                    },
                    render,
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(width, 180.0),
                    [180.0; 2],
                    |tx| {
                        tx.set(source, item(1, mode))?;
                        Ok(())
                    },
                    render,
                )
                .unwrap()
                .1
            };
            let (full, reference_nodes, reference) = reference(width, mode);
            for (actual, expected) in nodes.into_iter().zip(reference_nodes) {
                let a = ui.layout_result(actual).unwrap();
                let b = full.layout_result(expected).unwrap();
                assert_eq!((a.size, a.location), (b.size, b.location), "mode={mode}");
            }
            assert_eq!(tests::pixels(frame.clone()), tests::pixels(reference));
            old.get_or_insert(frame);
            let styles = observed.lock().unwrap();
            assert!(
                styles
                    .iter()
                    .any(|s| s.grid_column.start == GridPlacement::Line(-2))
            );
            assert_eq!(
                styles.last().unwrap().justify_self,
                item(1, mode).justify_self
            );
        }
        let before = ui.published_frame().unwrap().revision;
        let calls = observed.lock().unwrap().len();
        ui.transaction(|tx| tx.set(source, item(1, 0))).unwrap();
        assert_eq!(
            ui.frame_in_viewport(LayoutConstraints::viewport(120.0, 180.0), [180.0; 2])
                .unwrap()
                .layout_runs,
            0
        );
        assert_eq!(ui.published_frame().unwrap().revision, before);
        assert_eq!(observed.lock().unwrap().len(), calls);
        let old = old.unwrap();
        let bytes = tests::pixels(old.clone());
        drop(ui);
        assert_eq!(tests::pixels(old), bytes);
    }
}

#[test]
fn grid_style_observation_rejects_equal_size_post_builder_placement_changes() {
    let mut tree = UiTree::new_layout(host(120.0, false), 0).unwrap();
    let root = tree.root();
    let nodes = [0, 1].map(|i| tree.append_layout(root, item(i, 0), 0).unwrap());
    let policy = ExplicitCells(Arc::new(Mutex::new(vec![])));
    tests::install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(policy.clone()).with_allocation_layout(policy)),
    );
    let mut ui = LogicalUi::from_tree(tree, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let source = ui.state(scope, 0, || 0u32).unwrap();
    let old = ui
        .frame_in_viewport(LayoutConstraints::viewport(120.0, 180.0), [180.0; 2])
        .unwrap();
    let before = ui.stats();
    let result = ui.edit_frame(
        LayoutConstraints::viewport(120.0, 180.0),
        [180.0; 2],
        |tx| {
            tx.set(source, 1)?;
            Ok(())
        },
        |(), edit| {
            edit.allocation_builder(nodes[0], |_, edit| {
                // Same12px width, different row and justify-self. Every observed
                // item input remains a dependency, even if the slot is identical.
                edit.properties().layout_style(nodes[1], item(1, 2));
                Ok(())
            })
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.read(source).unwrap(), &0);
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.published_frame().unwrap().revision, old.revision);
}

#[test]
fn legacy_named_or_invalid_grid_item_values_reject_without_lossy_conversion() {
    use taffy::style_helpers::TaffyGridLine;
    let policy = ExplicitCells(Arc::new(Mutex::new(vec![])));
    for placement in [
        taffy::GridPlacement::NamedLine("named".into(), 1),
        taffy::GridPlacement::NamedSpan("named".into(), 2),
        taffy::GridPlacement::from_line_index(0),
        taffy::GridPlacement::Span(0),
    ] {
        let mut tree = UiTree::new_layout(host(120.0, false), 0).unwrap();
        let mut style = item(0, 0).to_taffy().unwrap();
        style.grid_column.start = placement;
        tree.append(tree.root(), style, 0).unwrap();
        let root = tree.root();
        tests::install(
            &mut tree,
            root,
            Some(ParentLayoutSnapshot::new(policy.clone()).with_allocation_layout(policy.clone())),
        );
        assert!(matches!(
            tree.frame([180.0; 2]),
            Err(Error::InvalidParentLayout)
        ));
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_custom_grid_item_metadata_matches_full_grid() {
    let mut tree = UiTree::new_layout(host(120.0, false), 0).unwrap();
    let root = tree.root();
    let nodes = [0, 1].map(|i| {
        tree.append_layout(root, item(i, 0), [0xff0000ff, 0x00ff00ff][i])
            .unwrap()
    });
    let policy = ExplicitCells(Arc::new(Mutex::new(vec![])));
    tests::install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(policy.clone()).with_allocation_layout(policy)),
    );
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("custom Grid item adapter: {:?}", gpu.adapter_info());
    for (mode, width) in [(0, 120.0), (1, 160.0), (2, 100.0), (3, 120.0), (0, 120.0)] {
        let mut tx = Transaction::default();
        tx.layout_style(root, host(width, false));
        for (i, node) in nodes.into_iter().enumerate() {
            tx.layout_style(node, item(i, mode));
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .local_frame_in_viewport(LayoutConstraints::viewport(width, 180.0), [180.0; 2])
            .unwrap();
        let (_, _, reference) = reference(width, mode);
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
