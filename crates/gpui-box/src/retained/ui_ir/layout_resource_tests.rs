use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicUsize, Ordering},
};

const ROOT: UiNodeId = UiNodeId::new(0, 1);
const HOST: UiNodeId = UiNodeId::new(1, 1);
const FLEX: UiNodeId = UiNodeId::new(2, 1);
const CONTENT: UiNodeId = UiNodeId::new(3, 1);
const FOOTER: UiNodeId = UiNodeId::new(4, 1);
const ALGORITHM: UiResourceId = UiResourceId(1);
const MEASURE: UiResourceId = UiResourceId(2);
const DATA: UiResourceId = UiResourceId(3);

#[derive(Debug)]
struct Stack {
    gap: f32,
    lifetime: Arc<()>,
}
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let _keep = &self.lifetime;
        let width = input.known_size.width.or(match input.available.width {
            LayoutAvailableSpace::Definite(v) => Some(v),
            _ => None,
        });
        let mut placements = vec![];
        let mut y = 0.0;
        let mut used_width: f32 = 0.0;
        for child in 0..children.len() {
            let slot = ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    input.available.width,
                    LayoutAvailableSpace::Unbounded,
                )
                .with_bounds(width.map_or(BoxConstraints::UNBOUNDED, |w| {
                    BoxConstraints::new([w, 0.0], [w, f32::INFINITY])
                })),
                percentage_basis: input.percentage_basis,
                ..Default::default()
            };
            let result = children.measure(child, slot)?;
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, y],
            });
            used_width = used_width.max(result.size[0]);
            let extra = children
                .parent_data(child)?
                .as_ref()
                .and_then(|d| d.get::<f32>())
                .copied()
                .unwrap_or(0.0);
            y += result.size[1]
                + if child + 1 < children.len() {
                    self.gap + extra
                } else {
                    0.0
                };
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(used_width),
                input.known_size.height.unwrap_or(y),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}
#[derive(Debug)]
struct Cells {
    count: usize,
    calls: Arc<AtomicUsize>,
    lifetime: Arc<()>,
    fail: bool,
}
impl ContentMeasure for Cells {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let _keep = &self.lifetime;
        if self.fail {
            return Err(Error::InvalidMeasure);
        }
        let columns = match input.available.width {
            LayoutAvailableSpace::MinContent => 1,
            LayoutAvailableSpace::Definite(w) => (w / 10.0).floor() as usize,
            _ => self.count,
        }
        .max(1)
        .min(self.count);
        let height = self.count.div_ceil(columns) as f32 * 8.0;
        Ok(ContentMeasureOutput {
            size: [columns as f32 * 10.0, height],
            baseline: Some(6.0),
            last_baseline: Some(height - 2.0),
        })
    }
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn column() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn snapshot() -> UiSnapshot {
    let mut root = UiNode::new(ROOT, column());
    root.children = vec![HOST, FOOTER];
    let mut host = UiNode::new(HOST, column());
    host.parent_layout = Some(ALGORITHM);
    host.children = vec![FLEX, CONTENT];
    let mut flex = UiNode::new(
        FLEX,
        LayoutStyle::row()
            .flex_wrap(LayoutFlexWrap::Wrap)
            .align_items(LayoutAlignItems::Start)
            .align_content(LayoutAlignContent::Start)
            .flex_shrink(0.0),
    );
    flex.parent_data = Some(DATA);
    let mut nodes = vec![];
    for i in 0..3 {
        let id = UiNodeId::new(5 + i, 1);
        flex.children.push(id);
        let mut node = UiNode::new(id, fixed(40.0, 10.0));
        node.properties.color = 0xff0000ff;
        nodes.push(node);
    }
    let mut content = UiNode::new(CONTENT, LayoutStyle::column().flex_shrink(0.0));
    content.content = UiContent::Measure(MEASURE);
    content.properties.color = 0x00ff00ff;
    let mut footer = UiNode::new(FOOTER, fixed(20.0, 7.0));
    footer.properties.color = 0x0000ffff;
    nodes.extend([root, host, flex, content, footer]);
    UiSnapshot::new(980, 1, ROOT, nodes)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn from_snapshot(
    snapshot: UiSnapshot,
    resources: &UiResources,
    w: f32,
) -> (UiDocument, UiPublication) {
    UiDocument::from_snapshot(
        snapshot,
        resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints(w),
        [180.0, 180.0],
    )
    .unwrap()
}
fn batch(doc: &UiDocument, operations: Vec<UiOperation>) -> UiBatch {
    UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 980,
        epoch: 1,
        base_revision: doc.revision(),
        revision: doc.revision() + 1,
        sequence: doc.next_sequence(),
        operations,
    }
}
fn apply(
    doc: &mut UiDocument,
    resources: &UiResources,
    w: f32,
    operations: Vec<UiOperation>,
) -> Result<UiPublication, UiError> {
    let batch = batch(doc, operations);
    doc.apply(
        batch,
        resources,
        BackendCapabilities::default(),
        constraints(w),
        [180.0, 180.0],
    )
}
fn resources() -> (UiResources, Arc<AtomicUsize>, Weak<()>, Weak<()>) {
    let algorithm = Arc::new(());
    let measure = Arc::new(());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut resources = UiResources::new();
    resources
        .parent_layout(
            ALGORITHM,
            ParentLayoutSnapshot::new(Stack {
                gap: 2.0,
                lifetime: algorithm.clone(),
            }),
        )
        .unwrap();
    resources
        .measure(
            MEASURE,
            ContentMeasureSnapshot::new(Cells {
                count: 30,
                calls: calls.clone(),
                lifetime: measure.clone(),
                fail: false,
            }),
        )
        .unwrap();
    resources
        .parent_data(DATA, ParentData::new(3.0f32))
        .unwrap();
    (
        resources,
        calls,
        Arc::downgrade(&algorithm),
        Arc::downgrade(&measure),
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn reference(width: f32, count: usize, extra: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree.append_layout(tree.root(), column(), 0).unwrap();
    let mut flex_style = LayoutStyle::row()
        .flex_wrap(LayoutFlexWrap::Wrap)
        .align_items(LayoutAlignItems::Start)
        .align_content(LayoutAlignContent::Start)
        .flex_shrink(0.0);
    flex_style.margin.bottom = LayoutLength::Px(2.0 + extra);
    let flex = tree.append_layout(host, flex_style, 0).unwrap();
    for _ in 0..3 {
        tree.append_layout(flex, fixed(40.0, 10.0), 0xff0000ff)
            .unwrap();
    }
    let columns = ((width / 10.0).floor() as usize).max(1).min(count);
    let height = count.div_ceil(columns) as f32 * 8.0;
    tree.append_layout(host, fixed(width, height), 0x00ff00ff)
        .unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 7.0), 0x0000ffff)
        .unwrap();
    tree.frame_in_viewport(constraints(width), [180.0, 180.0])
        .unwrap()
}
#[test]
fn owned_layout_resources_follow_flex_wrap_metadata_measurements_and_immutable_frames() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_owned_layout_resources_match_numeric_complete_flex() {
    run(true);
}
fn run(hardware: bool) {
    let (mut resources, calls, algorithm_life, measure_life) = resources();
    let (mut doc, initial) = from_snapshot(snapshot(), &resources, 100.0);
    assert!(calls.load(Ordering::Relaxed) > 0);
    let old_pixels = pixels(initial.frame.resolve());
    assert_eq!(doc.layout_result(CONTENT).unwrap().size, [100.0, 24.0]);
    assert_eq!(
        pixels(initial.frame.resolve()),
        pixels(reference(100.0, 30, 3.0))
    );
    resources
        .parent_data(UiResourceId(4), ParentData::new(7.0f32))
        .unwrap();
    let measure = ContentMeasureSnapshot::new(Cells {
        count: 12,
        calls: calls.clone(),
        lifetime: Arc::new(()),
        fail: false,
    });
    resources.measure(UiResourceId(5), measure).unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (width, count, extra, ops) in [
        (60.0, 30, 3.0, vec![]),
        (
            60.0,
            30,
            7.0,
            vec![UiOperation::ParentData {
                node: FLEX,
                resource: Some(UiResourceId(4)),
            }],
        ),
        (
            120.0,
            12,
            7.0,
            vec![UiOperation::Content {
                node: CONTENT,
                value: UiContent::Measure(UiResourceId(5)),
            }],
        ),
    ] {
        let output = apply(&mut doc, &resources, width, ops).unwrap();
        let full = reference(width, count, extra);
        assert_eq!(pixels(output.frame.resolve()), pixels(full.clone()));
        assert_eq!(
            doc.layout_result(HOST).unwrap().size,
            [full.items[1].bounds.width, full.items[1].bounds.height]
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(
                gpu.submit_local(&output.frame, [180, 180], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    let calls_before = calls.load(Ordering::Relaxed);
    let frame = doc.published_frame();
    let equal = apply(
        &mut doc,
        &resources,
        120.0,
        vec![
            UiOperation::ParentLayout {
                node: HOST,
                resource: Some(ALGORITHM),
            },
            UiOperation::ParentData {
                node: FLEX,
                resource: Some(UiResourceId(4)),
            },
        ],
    )
    .unwrap();
    assert!(!equal.commit.request_frame);
    assert_eq!(frame.revision(), equal.frame.revision());
    assert_eq!(calls_before, calls.load(Ordering::Relaxed));
    let restored = doc.snapshot();
    let (_, fresh) = from_snapshot(restored, &resources, 120.0);
    assert_eq!(pixels(equal.frame.resolve()), pixels(fresh.frame.resolve()));
    assert_eq!(old_pixels, pixels(initial.frame.resolve()));
    drop((doc, resources));
    assert!(algorithm_life.upgrade().is_none());
    assert!(measure_life.upgrade().is_none());
    assert_eq!(old_pixels, pixels(initial.frame.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn wrong_missing_and_rebound_layout_resources_reject_without_partial_publication() {
    let (mut resources, _, _, _) = resources();
    let (mut doc, _) = from_snapshot(snapshot(), &resources, 100.0);
    let old = doc.snapshot();
    let frame = doc.published_frame();
    assert!(matches!(
        resources.parent_data(ALGORITHM, ParentData::new(5u32)),
        Err(UiError::ResourceConflict(ALGORITHM))
    ));
    for (operation, error) in [
        (
            UiOperation::ParentLayout {
                node: HOST,
                resource: Some(UiResourceId(99)),
            },
            UiError::MissingResource(UiResourceId(99)),
        ),
        (
            UiOperation::ParentLayout {
                node: HOST,
                resource: Some(MEASURE),
            },
            UiError::ResourceKindMismatch(MEASURE),
        ),
        (
            UiOperation::ParentData {
                node: FLEX,
                resource: Some(ALGORITHM),
            },
            UiError::ResourceKindMismatch(ALGORITHM),
        ),
        (
            UiOperation::Content {
                node: CONTENT,
                value: UiContent::Measure(DATA),
            },
            UiError::ResourceKindMismatch(DATA),
        ),
    ] {
        assert_eq!(
            apply(&mut doc, &resources, 100.0, vec![operation]).err(),
            Some(error)
        );
        assert_eq!(doc.snapshot(), old);
        assert_eq!(frame.revision(), doc.published_frame().revision());
    }
    let failure = ContentMeasureSnapshot::new(Cells {
        count: 12,
        calls: Arc::new(AtomicUsize::new(0)),
        lifetime: Arc::new(()),
        fail: true,
    });
    resources.measure(UiResourceId(9), failure).unwrap();
    assert!(
        apply(
            &mut doc,
            &resources,
            100.0,
            vec![
                UiOperation::Content {
                    node: CONTENT,
                    value: UiContent::Measure(UiResourceId(9))
                },
                UiOperation::ParentData {
                    node: FLEX,
                    resource: None
                }
            ]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), old);
    assert!(
        apply(
            &mut doc,
            &resources,
            100.0,
            vec![UiOperation::Content {
                node: HOST,
                value: UiContent::Measure(MEASURE)
            }]
        )
        .is_err()
    );
    let mut grid = column();
    grid.display = LayoutDisplay::Grid;
    assert!(
        apply(
            &mut doc,
            &resources,
            100.0,
            vec![UiOperation::Layout {
                node: HOST,
                value: Box::new(grid)
            }]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), old);
}

#[test]
fn parent_and_measure_transitions_share_final_state_replay_and_thread_safe_messages() {
    fn transferable<T: Send + Sync>() {}
    transferable::<UiResources>();
    transferable::<UiBatch>();
    transferable::<UiSnapshot>();
    let (resources, _, _, _) = resources();
    let (mut doc, _) = from_snapshot(snapshot(), &resources, 100.0);
    let request = batch(
        &doc,
        vec![
            UiOperation::ParentLayout {
                node: HOST,
                resource: None,
            },
            UiOperation::ParentData {
                node: FLEX,
                resource: None,
            },
        ],
    );
    let replay = request.clone();
    doc.apply(
        request,
        &resources,
        BackendCapabilities::default(),
        constraints(100.0),
        [180.0; 2],
    )
    .unwrap();
    let again = doc
        .apply(
            replay,
            &resources,
            BackendCapabilities::default(),
            constraints(100.0),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(again.status, UiApplyStatus::Replayed);
    let output = apply(
        &mut doc,
        &resources,
        100.0,
        vec![
            UiOperation::ParentLayout {
                node: HOST,
                resource: Some(ALGORITHM),
            },
            UiOperation::ParentData {
                node: FLEX,
                resource: Some(DATA),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        pixels(output.frame.resolve()),
        pixels(reference(100.0, 30, 3.0))
    );
    let next = UiNodeId::new(20, 1);
    let mut leaf = UiNode::new(next, fixed(10.0, 5.0));
    leaf.parent_data = Some(DATA);
    apply(
        &mut doc,
        &resources,
        100.0,
        vec![
            UiOperation::Create(Box::new(leaf)),
            UiOperation::Children {
                node: HOST,
                value: vec![FLEX, CONTENT, next],
            },
        ],
    )
    .unwrap();
    assert_eq!(doc.node(next).unwrap().parent_data, Some(DATA));
}

#[test]
fn document_dry_queries_preserve_source_frame_and_resource_contracts() {
    let (resources, _, _, _) = resources();
    let (doc, _) = from_snapshot(snapshot(), &resources, 100.0);
    let before = doc.snapshot();
    let frame = doc.published_frame();
    let geometry = doc.layout_result(CONTENT).unwrap();
    let input = ChildLayoutInput::new(LayoutConstraints::new(
        LayoutAvailableSpace::Definite(60.0),
        LayoutAvailableSpace::Unbounded,
    ));
    let result = doc.dry_measure_node(CONTENT, input).unwrap();
    assert_eq!(result.size, [60.0, 40.0]);
    assert_eq!(
        doc.dry_measure_node_axis(CONTENT, input, LayoutAxis::Vertical)
            .unwrap(),
        40.0
    );
    let baseline = doc.dry_measure_node_baselines(CONTENT, input).unwrap();
    assert_eq!(baseline.size, [60.0, 40.0]);
    assert_eq!(baseline.baseline, Some(6.0));
    assert_eq!(baseline.last_baseline, Some(38.0));
    let parent_input = input.with_known_size(
        LayoutSize::new(Some(60.0), None),
        LayoutSize::new(true, true),
    );
    assert_eq!(
        doc.dry_measure_node(HOST, parent_input).unwrap().size,
        [60.0, 75.0]
    );
    assert!(matches!(
        doc.dry_measure_node(UiNodeId::new(99, 1), input),
        Err(UiError::InvalidIdentity(_))
    ));
    let mut invalid = input;
    invalid.known_size.width = Some(-1.0);
    assert!(doc.dry_measure_node(CONTENT, invalid).is_err());
    assert_eq!(doc.snapshot(), before);
    assert_eq!(doc.layout_result(CONTENT).unwrap(), geometry);
    assert_eq!(doc.published_frame().revision(), frame.revision());
}

#[derive(Debug)]
struct InvalidParent;
impl ParentLayout for InvalidParent {
    fn layout(
        &self,
        _: ParentLayoutInput,
        _: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        Ok(ParentLayoutOutput {
            size: [f32::NAN, 0.0],
            baseline: None,
            last_baseline: None,
            placements: vec![],
        })
    }
}
#[test]
fn failed_custom_parent_output_and_effective_style_reject_atomically() {
    use crate::retained::style::*;
    let (mut resources, _, _, _) = resources();
    resources
        .parent_layout(UiResourceId(30), ParentLayoutSnapshot::new(InvalidParent))
        .unwrap();
    let (mut doc, initial) = from_snapshot(snapshot(), &resources, 100.0);
    let before = doc.snapshot();
    let mut properties = doc.node(FOOTER).unwrap().properties.clone();
    properties.color = 0xffffffff;
    assert!(
        apply(
            &mut doc,
            &resources,
            100.0,
            vec![
                UiOperation::Properties {
                    node: FOOTER,
                    value: properties
                },
                UiOperation::ParentLayout {
                    node: HOST,
                    resource: Some(UiResourceId(30))
                }
            ]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), before);
    assert_eq!(
        pixels(initial.frame.resolve()),
        pixels(doc.published_frame().resolve())
    );
    let style = StyleRef::new(
        "invalid-custom-display",
        vec![StyleDeclaration::Display(StyleRule::value(
            LayoutDisplay::Grid,
        ))],
    )
    .unwrap();
    assert!(
        apply(
            &mut doc,
            &resources,
            100.0,
            vec![UiOperation::Styles {
                node: HOST,
                value: vec![style]
            }]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), before);
}

#[derive(Debug)]
struct Weighted;
impl ParentAllocationLayout for Weighted {
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = input
            .known_size
            .width
            .or(match input.constraints.width {
                LayoutAvailableSpace::Definite(v) => Some(v),
                _ => None,
            })
            .ok_or(Error::InvalidParentLayout)?;
        let weights = (0..children.len())
            .map(|i| {
                children
                    .parent_data(i)?
                    .as_ref()
                    .and_then(|d| d.get::<f32>())
                    .copied()
                    .ok_or(Error::InvalidParentLayout)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sum: f32 = weights.iter().sum();
        if sum <= 0.0 || weights.iter().any(|w| !w.is_finite() || *w <= 0.0) {
            return Err(Error::InvalidParentLayout);
        }
        Ok(weights
            .into_iter()
            .map(|weight| {
                let width = width * weight / sum;
                ChildLayoutInput::new(
                    LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(width),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                )
            })
            .collect())
    }
}
impl ParentLayout for Weighted {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            let slot = children
                .allocation(child)?
                .ok_or(Error::InvalidParentLayout)?;
            let result = children.measure(child, slot)?;
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [x, 0.0],
            });
            x += result.size[0];
            height = height.max(result.size[1]);
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(x),
                input.known_size.height.unwrap_or(height),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}
#[test]
fn owned_parent_resource_retains_independent_allocation_and_metadata_validation() {
    let mut resources = UiResources::new();
    resources
        .parent_layout(
            ALGORITHM,
            ParentLayoutSnapshot::new(Weighted).with_allocation_layout(Weighted),
        )
        .unwrap();
    resources
        .parent_data(DATA, ParentData::new(1.0f32))
        .unwrap();
    resources
        .parent_data(UiResourceId(4), ParentData::new(2.0f32))
        .unwrap();
    let mut root = UiNode::new(ROOT, column());
    root.parent_layout = Some(ALGORITHM);
    root.children = vec![FLEX, CONTENT];
    let mut left = UiNode::new(
        FLEX,
        LayoutStyle::column().size(LayoutDimension::Px(999.0), LayoutDimension::Px(10.0)),
    );
    left.parent_data = Some(DATA);
    left.properties.color = 0xff0000ff;
    let mut right = UiNode::new(CONTENT, fixed(1.0, 20.0));
    right.parent_data = Some(UiResourceId(4));
    right.properties.color = 0x00ff00ff;
    let (mut doc, _) = from_snapshot(
        UiSnapshot::new(980, 1, ROOT, vec![root, left, right]),
        &resources,
        120.0,
    );
    assert_eq!(doc.layout_result(FLEX).unwrap().size, [40.0, 10.0]);
    assert_eq!(doc.layout_result(CONTENT).unwrap().size, [80.0, 20.0]);
    let old = doc.snapshot();
    assert!(
        apply(
            &mut doc,
            &resources,
            120.0,
            vec![UiOperation::ParentData {
                node: CONTENT,
                resource: None
            }]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), old);
    let output = apply(
        &mut doc,
        &resources,
        90.0,
        vec![UiOperation::ParentData {
            node: CONTENT,
            resource: Some(DATA),
        }],
    )
    .unwrap();
    let mut baseline = UiTree::new_layout(
        LayoutStyle::row()
            .size(LayoutDimension::Px(90.0), LayoutDimension::Auto)
            .align_items(LayoutAlignItems::Start),
        0,
    )
    .unwrap();
    baseline
        .append_layout(baseline.root(), fixed(45.0, 10.0), 0xff0000ff)
        .unwrap();
    baseline
        .append_layout(baseline.root(), fixed(45.0, 20.0), 0x00ff00ff)
        .unwrap();
    assert_eq!(
        pixels(output.frame.resolve()),
        pixels(
            baseline
                .frame_in_viewport(constraints(90.0), [180.0; 2])
                .unwrap()
        )
    );
}

#[test]
fn replacing_parent_algorithm_and_leaf_resource_kinds_uses_normal_final_state() {
    let (mut resources, calls, _, _) = resources();
    let replacement = ParentLayoutSnapshot::new(Stack {
        gap: 8.0,
        lifetime: Arc::new(()),
    });
    resources
        .parent_layout(UiResourceId(10), replacement.clone())
        .unwrap();
    resources
        .parent_layout(UiResourceId(10), replacement.clone())
        .unwrap();
    resources
        .parent_layout(UiResourceId(11), replacement)
        .unwrap();
    resources
        .image(
            UiResourceId(12),
            ImageResource::rgba8([2, 2], vec![0, 0, 255, 255].repeat(4)).unwrap(),
        )
        .unwrap();
    let (mut doc, _) = from_snapshot(snapshot(), &resources, 100.0);
    let changed = apply(
        &mut doc,
        &resources,
        100.0,
        vec![UiOperation::ParentLayout {
            node: HOST,
            resource: Some(UiResourceId(10)),
        }],
    )
    .unwrap();
    assert_eq!(
        pixels(changed.frame.resolve()),
        pixels(reference(100.0, 30, 9.0))
    );
    let count = calls.load(Ordering::Relaxed);
    let before = doc.published_frame();
    let equal = apply(
        &mut doc,
        &resources,
        100.0,
        vec![UiOperation::ParentLayout {
            node: HOST,
            resource: Some(UiResourceId(11)),
        }],
    )
    .unwrap();
    assert!(!equal.commit.request_frame);
    assert_eq!(before.revision(), equal.frame.revision());
    assert_eq!(calls.load(Ordering::Relaxed), count);
    for content in [
        UiContent::Image(UiResourceId(12)),
        UiContent::Measure(MEASURE),
        UiContent::Fixed {
            size: [10.0, 6.0],
            first_baseline: Some(4.0),
            last_baseline: Some(4.0),
        },
        UiContent::Box,
        UiContent::Measure(MEASURE),
    ] {
        let output = apply(
            &mut doc,
            &resources,
            100.0,
            vec![UiOperation::Content {
                node: CONTENT,
                value: content,
            }],
        )
        .unwrap();
        let (_, fresh) = from_snapshot(doc.snapshot(), &resources, 100.0);
        assert_eq!(
            pixels(output.frame.resolve()),
            pixels(fresh.frame.resolve())
        );
    }
    // Resource handles carry pure immutable protocols across threads; no App,
    // Window, mutable renderer or engine ID is needed by the receiving owner.
    let source = doc.snapshot();
    let returned = std::thread::spawn(move || {
        let (owner, output) = from_snapshot(source, &resources, 100.0);
        drop((owner, resources));
        pixels(output.frame.resolve())
    })
    .join()
    .unwrap();
    assert_eq!(returned, pixels(doc.published_frame().resolve()));
}
