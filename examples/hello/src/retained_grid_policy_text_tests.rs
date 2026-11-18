use super::*;

/// Client policy for one explicitly placed paragraph, not a second Grid engine.
#[derive(Debug)]
struct GridParagraph;
impl ParentAllocationLayout for GridParagraph {
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = input
            .known_size
            .width
            .or(match input.constraints.width {
                LayoutAvailableSpace::Definite(w) => Some(w),
                _ => None,
            })
            .ok_or(Error::InvalidParentLayout)?;
        let item = children.style(0)?;
        let span = match item.grid_column.end {
            GridPlacement::Span(n @ 1..=2) => n,
            _ => return Err(Error::InvalidParentLayout),
        };
        if item.size.width != LayoutDimension::Percent(0.5) {
            return Err(Error::InvalidParentLayout);
        }
        let area = width * f32::from(span) / 2.0;
        let w = area * 0.5;
        Ok(vec![ChildLayoutInput {
            constraints: LayoutConstraints::new(
                LayoutAvailableSpace::Definite(area),
                LayoutAvailableSpace::Definite(80.0),
            )
            .with_bounds(BoxConstraints::new([w, 0.0], [w, f32::INFINITY])),
            percentage_basis: LayoutSize::new(Some(area), Some(80.0)),
            ..ChildLayoutInput::default()
        }])
    }
}
impl ParentLayout for GridParagraph {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let style = children.style(0)?;
        let slot = children.allocation(0)?.ok_or(Error::InvalidParentLayout)?;
        let child = children.measure_baselines(0, slot)?;
        let area = slot
            .percentage_basis
            .width
            .ok_or(Error::InvalidParentLayout)?;
        let width = input.known_size.width.ok_or(Error::InvalidParentLayout)?;
        let x = match style.grid_column.start {
            GridPlacement::Line(1) => 0.0,
            GridPlacement::Line(-2) => width / 2.0,
            _ => return Err(Error::InvalidParentLayout),
        };
        let y = match style.grid_row.start {
            GridPlacement::Line(1) => 0.0,
            GridPlacement::Line(2) => 80.0,
            _ => return Err(Error::InvalidParentLayout),
        };
        let offset = match style.justify_self.position {
            LayoutAlignSelf::End => area - child.size[0],
            LayoutAlignSelf::Center => (area - child.size[0]) / 2.0,
            _ => return Err(Error::InvalidParentLayout),
        };
        Ok(ParentLayoutOutput {
            size: [width, input.known_size.height.unwrap_or(160.0)],
            baseline: None,
            last_baseline: None,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [x + offset, y],
            }],
        })
    }
}
fn root(width: f32, grid: bool) -> LayoutStyle {
    let style = if grid {
        LayoutStyle::grid(
            GridStyle::default()
                .columns(vec![GridTrack::fr(1.0).into(); 2])
                .rows(vec![GridTrack::px(80.0).into(); 2]),
        )
    } else {
        LayoutStyle::row()
    };
    style.size(LayoutDimension::Px(width), LayoutDimension::Px(160.0))
}
fn item(wide: bool) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Percent(0.5), LayoutDimension::Auto)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .grid_column(
            GridPlacement::Line(if wide { 1 } else { -2 }),
            GridPlacement::Span(if wide { 2 } else { 1 }),
        )
        .grid_row(
            GridPlacement::Line(if wide { 2 } else { 1 }),
            GridPlacement::Auto,
        )
        .justify_self(if wide {
            LayoutAlignSelf::Center.safe()
        } else {
            LayoutAlignSelf::End.unsafe_overflow()
        })
        .align_self(LayoutAlignSelf::Start)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 400],
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
#[test]
fn grid_metadata_parent_rewraps_real_noto_against_full_grid() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_grid_metadata_parent_real_noto_matches_full_grid() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(&system, "one two three four 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("Grid policy glyph adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let mut tree = UiTree::new_layout(root(180.0, false), 0).unwrap();
        let host = tree.root();
        let leaf = tree.append_layout(host, item(false), 0).unwrap();
        let mut tx = Transaction::default();
        tx.parent_layout(
            host,
            Some(ParentLayoutSnapshot::new(GridParagraph).with_allocation_layout(GridParagraph)),
        );
        tx.text(leaf, Some(paint.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let mut old = None;
        for (width, wide) in [(180.0, false), (240.0, true), (180.0, true), (180.0, false)] {
            let mut tx = Transaction::default();
            tx.layout_style(host, root(width, false));
            tx.layout_style(leaf, item(wide));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let bounds = LayoutConstraints::viewport(width, 400.0);
            let frame = if local {
                tree.local_frame_in_viewport(bounds, [300.0, 400.0])
                    .unwrap()
                    .resolve()
            } else {
                tree.frame_in_viewport(bounds, [300.0, 400.0]).unwrap()
            };
            let mut full = UiTree::new_layout(root(width, true), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let reference = full.append_layout(full.root(), item(wide), 0).unwrap();
            let mut tx = Transaction::default();
            tx.text(reference, Some(paint.clone()));
            full.commit(tx, BackendCapabilities::default()).unwrap();
            let reference_frame = full.frame_in_viewport(bounds, [300.0, 400.0]).unwrap();
            assert_eq!(
                tree.layout_result(leaf).unwrap(),
                full.layout_result(reference).unwrap()
            );
            let wrap_width = width * if wide { 0.5 } else { 0.25 };
            assert_eq!(
                tree.layout_result(leaf).unwrap().size[1],
                paragraph.layout(Some(wrap_width)).unwrap().size[1]
            );
            assert_eq!(pixels(frame.clone()), pixels(reference_frame.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit(&frame, [300, 400], 1.0).unwrap().unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&reference_frame, [300, 400], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let before = pixels(old.clone());
        drop(tree);
        assert_eq!(pixels(old), before);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
