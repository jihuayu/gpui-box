use super::*;
use std::sync::Mutex;

#[derive(Debug, Clone)]
struct Cells {
    count: usize,
    partial: bool,
    log: Arc<Mutex<Vec<ContentMeasureInput>>>,
}
impl ContentMeasure for Cells {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        self.log.lock().unwrap().push(input);
        let width = match input.available.width {
            LayoutAvailableSpace::MinContent => 10.0,
            LayoutAvailableSpace::MaxContent | LayoutAvailableSpace::Unbounded => {
                self.count as f32 * 10.0
            }
            LayoutAvailableSpace::Definite(w) => w,
        };
        let columns = ((width / 10.0).floor() as usize).max(1).min(self.count);
        let width = columns as f32 * 10.0;
        let height = self.count.div_ceil(columns) as f32 * 8.0;
        let size = if self.partial {
            match input.requested_axis {
                Some(LayoutAxis::Horizontal) => [width, 0.0],
                Some(LayoutAxis::Vertical) => [0.0, height],
                None => [width, height],
            }
        } else {
            [width, height]
        };
        Ok(ContentMeasureOutput {
            size,
            baseline: input.requested_axis.is_none().then_some(6.0),
            last_baseline: input.requested_axis.is_none().then_some(height - 2.0),
        })
    }
}
fn content(count: usize, partial: bool) -> (MeasureSnapshot, Arc<Mutex<Vec<ContentMeasureInput>>>) {
    let log = Arc::new(Mutex::new(vec![]));
    (
        MeasureSnapshot::Content(ContentMeasureSnapshot::new(Cells {
            count,
            partial,
            log: log.clone(),
        })),
        log,
    )
}
fn install(tree: &mut UiTree, node: NodeId, value: MeasureSnapshot) {
    let mut tx = Transaction::default();
    tx.measure(node, Some(value));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
}
fn query(width: LayoutAvailableSpace) -> ChildLayoutInput {
    ChildLayoutInput::new(LayoutConstraints::new(
        width,
        LayoutAvailableSpace::Unbounded,
    ))
}

#[test]
fn content_axis_probes_skip_unrelated_height_and_keep_complete_baselines_and_overflow() {
    let style = LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(2.0)));
    let mut tree = UiTree::new_layout(style, 0).unwrap();
    let root = tree.root();
    let (value, log) = content(12, true);
    install(&mut tree, root, value);
    for (space, width) in [
        (LayoutAvailableSpace::MinContent, 14.0),
        (LayoutAvailableSpace::MaxContent, 124.0),
        (LayoutAvailableSpace::Definite(44.0), 44.0),
    ] {
        assert_eq!(
            tree.dry_measure_node_axis(root, query(space), LayoutAxis::Horizontal)
                .unwrap(),
            width
        );
    }
    let requests = log.lock().unwrap().clone();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|q| q.requested_axis == Some(LayoutAxis::Horizontal))
    );
    log.lock().unwrap().clear();
    assert_eq!(
        tree.dry_measure_node_axis(
            root,
            query(LayoutAvailableSpace::Definite(44.0)),
            LayoutAxis::Vertical
        )
        .unwrap(),
        28.0
    );
    let requests = log.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].requested_axis, None);
    assert_eq!(requests[1].requested_axis, Some(LayoutAxis::Vertical));
    let complete = tree
        .dry_measure_node_baselines(root, query(LayoutAvailableSpace::Definite(44.0)))
        .unwrap();
    assert_eq!(complete.size, [44.0, 28.0]);
    assert_eq!(complete.baseline, Some(8.0));
    assert_eq!(complete.last_baseline, Some(24.0));
    assert!(tree.published_frame().is_none());
    let frame = tree.frame([44.0, 180.0]).unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, [44.0, 28.0]);
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|q| q.requested_axis.is_none())
    );
    assert_eq!(tree.published_frame().unwrap().revision, frame.revision);
}

#[test]
fn content_internal_intrinsic_limit_probes_use_their_own_axis_and_match_complete_clients() {
    for case in 0..8 {
        let mut style = LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(2.0)));
        match case {
            0 => {}
            1 => style.min_size.width = LayoutDimension::MinContent,
            2 => style.min_size.height = LayoutDimension::MaxContent,
            3 => {
                style.min_size.height = LayoutDimension::MinContent;
                style.size.width = LayoutDimension::Px(44.0);
            }
            4 => {
                style.max_size.width = LayoutDimension::MaxContent;
                style.min_size.height = LayoutDimension::MinContent;
            }
            5 => {
                style.aspect_ratio = Some(2.0);
                style.min_size.height = LayoutDimension::MinContent;
            }
            6 => {
                style.container_type = LayoutContainerType::InlineSize;
                style.contain_intrinsic_size = LayoutSize::new(Some(40.0), None);
            }
            7 => {
                style.overflow = LayoutSize::new(LayoutOverflow::Scroll, LayoutOverflow::Scroll);
                style.scrollbar_width = 6.0;
            }
            _ => unreachable!(),
        }
        let lowered = style.to_taffy().unwrap();
        let mut tree = UiTree::new(lowered.clone(), 0).unwrap();
        let root = tree.root();
        let (value, log) = content(12, true);
        install(&mut tree, root, value);
        let mut full = UiTree::new(lowered, 0).unwrap();
        let control = full.root();
        install(&mut full, control, content(12, false).0);
        for input in [
            query(LayoutAvailableSpace::MinContent),
            query(LayoutAvailableSpace::MaxContent),
            query(LayoutAvailableSpace::Definite(44.0)),
            query(LayoutAvailableSpace::Unbounded),
        ] {
            for axis in [LayoutAxis::Horizontal, LayoutAxis::Vertical] {
                let actual = tree.dry_measure_node_axis(root, input, axis).unwrap();
                let expected = full.dry_measure_node(control, input).unwrap();
                assert_eq!(
                    actual,
                    axis.select(expected.size),
                    "case{case}/{input:?}/{axis:?}"
                );
            }
        }
        if case == 2 {
            assert!(
                log.lock()
                    .unwrap()
                    .iter()
                    .any(|q| q.requested_axis == Some(LayoutAxis::Vertical))
            );
        }
        tree.frame([80.0, 180.0]).unwrap();
        full.frame([80.0, 180.0]).unwrap();
        assert_eq!(
            tree.layout_result(root).unwrap(),
            full.layout_result(control).unwrap(),
            "case{case}"
        );
    }
}

fn root_style() -> LayoutStyle {
    LayoutStyle::column().align_items(LayoutAlignItems::Start)
}
fn host_style(width: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(width), LayoutDimension::Auto)
        .flex_wrap(LayoutFlexWrap::Wrap)
        .align_items(LayoutAlignItems::Start)
        .align_content(LayoutAlignContent::Start)
        .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
        .flex_shrink(0.0)
}
fn cell_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(10.0), LayoutDimension::Px(8.0))
        .flex_shrink(0.0)
}
fn reference(count: usize, width: f32) -> (UiTree, NodeId, NodeId, FramePacket) {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(tree.root(), host_style(width), 0xff0000ff)
        .unwrap();
    for _ in 0..count {
        let leaf = tree.append_layout(host, cell_style(), 0).unwrap();
        install(
            &mut tree,
            leaf,
            MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 10.0,
                height: 8.0,
                baseline: Some(6.0),
            }),
        );
    }
    let footer = tree
        .append_layout(tree.root(), cell_style(), 0x00ff00ff)
        .unwrap();
    let frame = tree.frame([180.0; 2]).unwrap();
    (tree, host, footer, frame)
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

#[test]
fn content_axis_sources_keep_normal_flex_full_cpu_reference_p0_and_retired_frames() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, host_style(44.0), 0xff0000ff)
            .unwrap();
        let footer = ui
            .append_render_layout(scope, root, cell_style(), 0x00ff00ff)
            .unwrap();
        let source = ui.state(scope, 555, || Some(content(12, true).0)).unwrap();
        ui.bind(source, Property::measure(host)).unwrap();
        let style_source = ui.state(scope, 556, || host_style(44.0)).unwrap();
        ui.bind(style_source, Property::layout_style(host)).unwrap();
        let mut old = None;
        for (count, width) in [(12, 44.0), (5, 44.0), (12, 84.0), (12, 44.0)] {
            let value = Some(content(count, true).0);
            ui.transaction(|tx| {
                tx.set(source, value.clone())?;
                tx.set(style_source, host_style(width))?;
                Ok(())
            })
            .unwrap();
            let frame = if local {
                ui.local_frame([180.0; 2]).unwrap().resolve()
            } else {
                ui.frame([180.0; 2]).unwrap()
            };
            let (full, a, b, control) = reference(count, width);
            for (node, reference) in [(host, a), (footer, b)] {
                assert_eq!(
                    ui.layout_result(node).unwrap(),
                    full.layout_result(reference).unwrap()
                );
            }
            assert_eq!(pixels(frame.clone()), pixels(control));
            ui.transaction(|tx| {
                tx.set(source, Some(content(count + 1, true).0))?;
                tx.set(source, value)?;
                Ok(())
            })
            .unwrap();
            let idle = ui.frame([180.0; 2]).unwrap();
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

#[derive(Debug)]
struct FailComplete;
impl ContentMeasure for FailComplete {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        if input.requested_axis.is_none() {
            return Err(Error::InvalidMeasure);
        }
        Ok(ContentMeasureOutput {
            size: [20.0, 0.0],
            baseline: None,
            last_baseline: None,
        })
    }
}
#[test]
fn valid_scalar_content_does_not_mask_failed_atomic_complete_layout() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    let mut probe = UiTree::new_layout(LayoutStyle::row(), 0).unwrap();
    let probe_root = probe.root();
    install(
        &mut probe,
        probe_root,
        MeasureSnapshot::Content(ContentMeasureSnapshot::new(FailComplete)),
    );
    assert_eq!(
        probe
            .dry_measure_node_axis(
                probe_root,
                query(LayoutAvailableSpace::MaxContent),
                LayoutAxis::Horizontal
            )
            .unwrap(),
        20.0
    );
    assert!(matches!(
        probe.dry_measure_node_baselines(probe_root, query(LayoutAvailableSpace::MaxContent)),
        Err(Error::InvalidMeasure)
    ));
    assert!(probe.published_frame().is_none());
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let node = ui
            .append_render_layout(scope, root, host_style(44.0), 0xff0000ff)
            .unwrap();
        let value = Some(content(12, true).0);
        let source = ui.state(scope, 557, || value.clone()).unwrap();
        ui.bind(source, Property::measure(node)).unwrap();
        let old = ui.frame([180.0; 2]).unwrap();
        let geometry = ui.layout_result(node).unwrap();
        let stats = ui.stats();
        let invalid = Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            FailComplete,
        )));
        let sources = |tx: &mut reactive::ReactiveTransaction<'_>| {
            tx.set(source, invalid.clone())?;
            Ok(())
        };
        let result = if local {
            ui.edit_local_frame(
                LayoutConstraints::viewport(180.0, 180.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        } else {
            ui.edit_frame(
                LayoutConstraints::viewport(180.0, 180.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        };
        assert!(matches!(result, Err(Error::InvalidMeasure)));
        assert_eq!(ui.read(source).unwrap(), &value);
        assert_eq!(ui.layout_result(node).unwrap(), geometry);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_content_axis_updates_keep_complete_numeric_cpu_geometry_and_gpu_pixels() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(root, host_style(44.0), 0xff0000ff)
        .unwrap();
    tree.append_layout(root, cell_style(), 0x00ff00ff).unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("content-axis adapter: {:?}", gpu.adapter_info());
    for (count, width) in [(12, 44.0), (5, 44.0), (12, 84.0), (12, 44.0)] {
        let mut tx = Transaction::default();
        tx.measure(host, Some(content(count, true).0));
        tx.layout_style(host, host_style(width));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([180.0; 2]).unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(count, width).3, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
