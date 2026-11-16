//! Real font shaping → owned wrapping → persistent Taffy correctness.
use gpui::{TextRun, WindowTextSystem, font, px};
use gpui_box::retained::{paragraph::ParagraphSnapshot, *};
use gpui_box::retained::{reactive::*, text::*};
use std::{borrow::Cow, sync::Arc};

fn text_system() -> WindowTextSystem {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let system = Arc::new(gpui::TextSystem::new(platform));
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    WindowTextSystem::new(system)
}
fn shape(system: &WindowTextSystem, text: &str) -> ParagraphSnapshot {
    ParagraphSnapshot::shape(system, text, font("Noto Sans CJK SC"), 16.0, 32.0).unwrap()
}

#[test]
fn automatic_environment_updates_rewrap_real_text_and_preserve_full_cpu_pixels() {
    use gpui_box::retained::{logical::LogicalUi, responsive::*};
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let root_style = LayoutStyle::column().size(LayoutDimension::Px(200.0), LayoutDimension::Auto);
    let mut ui =
        LogicalUi::new_layout(root_style.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let base = LayoutStyle::column().fixed_inline_size(60.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), base.clone(), 0)
        .unwrap();
    let leaf_style = LayoutStyle::row();
    let leaf = ui
        .append_render_layout(scope, host, leaf_style.clone(), 0)
        .unwrap();
    let host_source = ui.state(scope, 1, || base.clone()).unwrap();
    let text_style = ui.state(scope, 2, || leaf_style.clone()).unwrap();
    let padding = LayoutEdges::all(LayoutLength::Px(5.0));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().text(leaf, Some(paint.clone()));
            edit.responsive_rule(
                scope,
                1,
                host_source,
                host,
                LayoutQuery::ViewportInline,
                LayoutBreakpointStyles::new(
                    base.clone(),
                    vec![(150.0, base.fixed_inline_size(140.0))],
                )?,
            )?;
            edit.responsive_rule(
                scope,
                2,
                text_style,
                leaf,
                LayoutQuery::ContainerInline(host),
                LayoutBreakpointStyles::new(
                    leaf_style.clone(),
                    vec![(100.0, leaf_style.clone().padding(padding))],
                )?,
            )
        },
    )
    .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [200, 400],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Definite(200.0),
        LayoutAvailableSpace::Unbounded,
    );
    let mut old = None;
    let mut heights = vec![];
    for (i, width) in [100.0, 200.0, 100.0, 200.0].into_iter().enumerate() {
        if i == 0 {
            ui.local_frame_with_environment(
                &LayoutEnvironment::new([width, 400.0]),
                constraints,
                [200.0, 400.0],
            )
            .unwrap();
        } else {
            ui.edit(
                |_| Ok(()),
                |(), edit| edit.layout_environment(&LayoutEnvironment::new([width, 400.0])),
            )
            .unwrap();
        }
        let wide = width >= 150.0;
        let frame = ui.published_local_frame().unwrap().clone();
        let actual = pixels(frame.resolve());
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0));
        let content_width = if wide { 130.0 } else { 60.0 };
        let layout = paragraph.layout(Some(content_width)).unwrap();
        heights.push(layout.size[1]);
        assert_eq!(
            ui.layout_result(leaf).unwrap().size[1],
            layout.size[1] + if wide { 10.0 } else { 0.0 }
        );
        let mut full = UiTree::new_layout(root_style.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(
                full.root(),
                LayoutStyle::column().fixed_inline_size(if wide { 140.0 } else { 60.0 }),
                0,
            )
            .unwrap();
        let style = if wide {
            leaf_style.clone().padding(padding)
        } else {
            leaf_style.clone()
        };
        let t = full.append_layout(h, style, 0).unwrap();
        let mut tx = Transaction::default();
        tx.text(t, Some(paint.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [200.0, 400.0]).unwrap())
        );
        old.get_or_insert((frame, actual));
    }
    assert!(heights.windows(2).any(|h| h[0] != h[1]));
    drop((ui, system, raster, paragraph, paint));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
}

#[test]
fn last_baseline_real_paragraphs_rewrap_align_and_keep_complete_frame_lifetimes() {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let peer_paragraph = shape(&system, "peer");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let peer = TextPaint {
        resource: raster.prepare(peer_paragraph.clone(), 1.0).unwrap(),
        color: 0xff0000ff,
    };
    let root = LayoutStyle::row()
        .size(LayoutDimension::Px(300.0), LayoutDimension::Px(400.0))
        .align_items(LayoutAlignItems::LastBaseline);
    let mut tree = UiTree::new_layout(root.clone(), 0).unwrap();
    let a = tree
        .append_layout(tree.root(), LayoutStyle::row(), 0)
        .unwrap();
    let b = tree
        .append_layout(tree.root(), LayoutStyle::row(), 0)
        .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 400],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    let mut heights = vec![];
    for width in [60.0, 140.0, 90.0, 60.0] {
        let style = LayoutStyle::row()
            .size(LayoutDimension::Px(width), LayoutDimension::Auto)
            .flex_shrink(0.0);
        let peer_style = LayoutStyle::row()
            .size(LayoutDimension::Px(80.0), LayoutDimension::Auto)
            .flex_shrink(0.0);
        let mut tx = Transaction::default();
        tx.layout_style(a, style.clone());
        tx.layout_style(b, peer_style.clone());
        tx.text(a, Some(paint.clone()));
        tx.text(b, Some(peer.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([300.0, 400.0]).unwrap();
        let layout = paragraph.layout(Some(width)).unwrap();
        let other = peer_paragraph.layout(Some(80.0)).unwrap();
        heights.push(layout.size[1]);
        assert_eq!(
            layout.last_baseline,
            layout.first_baseline + layout.size[1] - paragraph.line_height()
        );
        let baseline = 400.0
            - (layout.size[1] - layout.last_baseline).max(other.size[1] - other.last_baseline);
        for (id, text) in [(a, &layout), (b, &other)] {
            let r = tree.layout_result(id).unwrap();
            assert_eq!(r.size[1], text.size[1]);
            // Authored inline extent is preserved even when the shaped text is shorter.
            assert_eq!(r.last_baseline, Some(text.last_baseline));
            assert_eq!(r.location[1] + r.last_baseline.unwrap(), baseline);
        }
        let mut reference =
            UiTree::new_layout(root.clone().align_items(LayoutAlignItems::Start), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let ra = reference.append_layout(reference.root(), style, 0).unwrap();
        let rb = reference
            .append_layout(reference.root(), peer_style, 0)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(ra, Some(paint.clone()));
        tx.text(rb, Some(peer.clone()));
        tx.translation(ra, [0.0, baseline - layout.last_baseline]);
        tx.translation(rb, [0.0, baseline - other.last_baseline]);
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let actual = pixels(frame.resolve());
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0));
        assert_eq!(actual, pixels(reference.frame([300.0, 400.0]).unwrap()));
        old.get_or_insert((frame, actual));
    }
    assert!(heights.windows(2).any(|h| h[0] != h[1]));
    drop((tree, system, raster, paint, peer, paragraph, peer_paragraph));
    let (old, expected) = old.unwrap();
    assert_eq!(pixels(old.resolve()), expected);
}

#[test]
fn safe_alignment_preserves_real_glyph_wrapping_and_complete_cpu_pixels() {
    use gpui_box::retained::logical::LogicalUi;
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let height = paragraph.layout(Some(60.0)).unwrap().size[1];
    assert!(height > 30.0);
    let root = LayoutStyle::row().size(LayoutDimension::Px(300.0), LayoutDimension::Px(700.0));
    let host_style = |safe| {
        LayoutStyle::row()
            .size(LayoutDimension::Px(60.0), LayoutDimension::Px(30.0))
            .align_items(if safe {
                LayoutAlignItems::Center.safe()
            } else {
                LayoutAlignItems::Center.unsafe_overflow()
            })
    };
    let leaf_style = LayoutStyle::row()
        .size(LayoutDimension::Px(60.0), LayoutDimension::Auto)
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(root.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), host_style(true), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, leaf_style.clone(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().text(leaf, Some(text.clone()));
            Ok(())
        },
    )
    .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    let mut geometry = None;
    for safe in [true, false, true, false] {
        let (_, frame) = ui
            .edit_local_frame(
                LayoutConstraints::viewport(300.0, 700.0),
                [300.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().layout_style(host, host_style(safe));
                    Ok(())
                },
            )
            .unwrap();
        let y = if safe { 0.0 } else { (30.0 - height) * 0.5 };
        assert_eq!(ui.layout_result(leaf).unwrap().size, [60.0, height]);
        assert_eq!(ui.layout_result(leaf).unwrap().location, [0.0, y]);
        let resolved = frame.resolve();
        let glyphs = resolved
            .items
            .iter()
            .find(|p| p.node == leaf)
            .unwrap()
            .glyphs
            .as_ref()
            .unwrap();
        if let Some(previous) = &geometry {
            assert!(Arc::ptr_eq(previous, glyphs));
        } else {
            geometry = Some(glyphs.clone());
        }
        let mut full = UiTree::new_layout(root.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(
                full.root(),
                host_style(true).align_items(LayoutAlignItems::Start),
                0,
            )
            .unwrap();
        let node = full.append_layout(h, leaf_style.clone(), 0).unwrap();
        let mut tx = Transaction::default();
        tx.text(node, Some(text.clone()));
        tx.translation(node, [0.0, y]);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let actual = pixels(resolved);
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0 && p[2] > 0));
        assert_eq!(actual, pixels(full.frame([300.0, 700.0]).unwrap()));
        old.get_or_insert((frame, actual));
    }
    drop((ui, system, raster, text, paragraph, geometry));
    let (old, expected) = old.unwrap();
    assert_eq!(pixels(old.resolve()), expected);
}

#[test]
fn pixel_snap_moves_real_glyph_origins_without_rewrapping_at_rounded_box_width() {
    pixel_snap_real_paragraph(false);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_pixel_snap_real_paragraph_matches_independent_origin_projection() {
    pixel_snap_real_paragraph(true);
}

fn pixel_snap_real_paragraph(gpu: bool) {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let scale = 2.0;
    // Find a real font wrap boundary crossed by device-edge rounding. This
    // avoids assuming a synthetic per-character advance or a platform metric.
    let (width, rounded) = (200..2500)
        .map(|i| i as f32 / 10.0)
        .find_map(|width| {
            let rounded = (width * scale).round() / scale;
            (paragraph.layout(Some(width)).unwrap().size[1]
                != paragraph.layout(Some(rounded)).unwrap().size[1])
                .then_some((width, rounded))
        })
        .expect("real fractional wrap threshold");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let resource = raster.prepare(paragraph.clone(), scale).unwrap();
    let text = TextPaint {
        resource: resource.clone(),
        color: 0x00ffffff,
    };
    let root_style = LayoutStyle::column()
        .size(LayoutDimension::Px(300.0), LayoutDimension::Px(700.0))
        .padding(LayoutEdges::all(LayoutLength::Px(0.3)));
    let leaf_style = LayoutStyle::row()
        .size(LayoutDimension::Px(width), LayoutDimension::Auto)
        .flex_shrink(0.0);
    let mut tree = UiTree::new_layout(root_style.clone(), 0).unwrap();
    let leaf = tree
        .append_layout(tree.root(), leaf_style.clone(), 0)
        .unwrap();
    let mut tx = Transaction::default();
    tx.text(leaf, Some(text.clone()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let original = tree.frame([300.0, 700.0]).unwrap();
    let old = original
        .items
        .iter()
        .find(|item| item.node == leaf)
        .unwrap();
    let chunk = old.glyphs.as_ref().unwrap().clone();
    let logical = tree.layout_result(leaf).unwrap();
    assert_eq!(logical.content_size[0], width);
    assert_ne!(
        paragraph.layout(Some(width)).unwrap().size[1],
        paragraph.layout(Some(rounded)).unwrap().size[1]
    );
    let rasterized = raster.rasterized_glyphs();
    tree.set_pixel_snap(PixelSnap::DevicePixels { scale })
        .unwrap();
    let snapped = tree.local_frame([300.0, 700.0]).unwrap();
    let frame = snapped.resolve();
    let item = frame.items.iter().find(|item| item.node == leaf).unwrap();
    assert!(Arc::ptr_eq(&chunk, item.glyphs.as_ref().unwrap()));
    assert!(Arc::ptr_eq(&chunk, &resource.chunk(width).unwrap()));
    assert_eq!(tree.layout_result(leaf).unwrap(), logical);
    assert_eq!(raster.rasterized_glyphs(), rasterized);
    // The independent reference retains the original logical wrap width and
    // moves only the transparent text box to the expected device-pixel origin.
    let snap = |value: f32| (value * scale).round() / scale;
    let mut full = UiTree::new_layout(root_style, 0).unwrap();
    let expected_leaf = full.append_layout(full.root(), leaf_style, 0).unwrap();
    let mut tx = Transaction::default();
    tx.text(expected_leaf, Some(text));
    tx.translation(
        expected_leaf,
        [
            snap(old.content_bounds.x) - old.content_bounds.x,
            snap(old.content_bounds.y) - old.content_bounds.y,
        ],
    );
    full.commit(tx, BackendCapabilities::default()).unwrap();
    let reference = full.frame([300.0, 700.0]).unwrap();
    #[cfg(feature = "retained-wgpu")]
    if gpu {
        let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
        eprintln!(
            "pixel snap paragraph adapter: {:?}",
            renderer.adapter_info()
        );
        renderer
            .submit_local(&snapped, [600, 1400], scale)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        let expected = renderer
            .reference_image(&reference, [600, 1400], scale)
            .unwrap();
        assert!(actual.as_raw() == expected.as_raw());
        renderer.finish().unwrap();
    }
    #[cfg(not(feature = "retained-wgpu"))]
    let _ = gpu;
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [600, 1400],
                scale: 2.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 16 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let actual = pixels(frame);
    assert!(actual.chunks_exact(4).any(|p| p[1] > 0 && p[2] > 0));
    assert_eq!(actual, pixels(reference));
    drop((tree, full, raster, system));
    assert_eq!(pixels(snapped.resolve()), actual);
}

#[test]
fn responsive_rules_and_keyed_paragraph_builder_share_atomic_local_output() {
    responsive_paragraph_builder_frames(false);
}

#[test]
fn scoped_rules_and_real_paragraph_share_atomic_local_output() {
    responsive_paragraph_builder_frames(true);
}

fn responsive_paragraph_builder_frames(scoped: bool) {
    use gpui_box::retained::{logical::LogicalUi, responsive::*};
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let container_style = LayoutStyle::column().fixed_inline_size(60.0);
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(column.clone(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, root, container_style.clone(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(scope, container, boundary.clone(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, footer_style.clone(), 0xff0000ff)
        .unwrap();
    let size_source = ui.state(scope, 20, || container_style.clone()).unwrap();
    let box_source = ui.state(scope, 21, || boundary.clone()).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    if !scoped {
        responsive
            .bind(
                &mut ui,
                size_source,
                container,
                LayoutBreakpointStyles::new(
                    container_style.clone(),
                    vec![(100.0, LayoutStyle::column().fixed_inline_size(150.0))],
                )
                .unwrap(),
            )
            .unwrap();
        responsive
            .bind_container(
                &mut ui,
                box_source,
                host,
                container,
                LayoutBreakpointStyles::new(
                    boundary.clone(),
                    vec![(
                        100.0,
                        boundary
                            .clone()
                            .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                    )],
                )
                .unwrap(),
            )
            .unwrap();
    }
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [200, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    for env_width in [80.0, 180.0, 80.0, 180.0] {
        let wide = env_width >= 100.0;
        let width = if wide { 140.0 } else { 60.0 };
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(180.0),
            LayoutAvailableSpace::Unbounded,
        );
        let (commit, frame) = ui
            .edit_local_frame(
                constraints,
                [200.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    if scoped {
                        edit.responsive_rule(
                            scope,
                            20,
                            size_source,
                            container,
                            LayoutQuery::ViewportInline,
                            LayoutBreakpointStyles::new(
                                container_style.clone(),
                                vec![(100.0, LayoutStyle::column().fixed_inline_size(150.0))],
                            )?,
                        )?;
                        edit.responsive_rule(
                            scope,
                            21,
                            box_source,
                            host,
                            LayoutQuery::ContainerInline(container),
                            LayoutBreakpointStyles::new(
                                boundary.clone(),
                                vec![(
                                    100.0,
                                    boundary
                                        .clone()
                                        .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                                )],
                            )?,
                        )?;
                        edit.responsive_registered(&LayoutEnvironment::new([env_width, 700.0]))?;
                    } else {
                        edit.responsive(&responsive, &LayoutEnvironment::new([env_width, 700.0]))?;
                    }
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, width);
                        let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                        let renders = edit.renders(branch)?;
                        let leaf = match renders.first() {
                            Some(node) => *node,
                            None => edit.append_layout(branch, host, LayoutStyle::row(), 0)?,
                        };
                        edit.properties().text(leaf, Some(text.clone()));
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            ui.layout_result(commit.result.value).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, expected.size[1] + if wide { 10.0 } else { 0.0 }]
        );
        let actual = pixels(frame.resolve());
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let c = full
            .append_layout(
                full.root(),
                LayoutStyle::column().fixed_inline_size(if wide { 150.0 } else { 60.0 }),
                0,
            )
            .unwrap();
        let h = full
            .append_layout(
                c,
                if wide {
                    boundary
                        .clone()
                        .padding(LayoutEdges::all(LayoutLength::Px(5.0)))
                } else {
                    boundary.clone()
                },
                0,
            )
            .unwrap();
        let leaf = full.append_layout(h, LayoutStyle::row(), 0).unwrap();
        full.append_layout(full.root(), footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(leaf, Some(text.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [200.0, 700.0]).unwrap())
        );
        assert!(
            actual
                .chunks_exact(4)
                .any(|p| p[0] == 0 && p[1] > 0 && p[2] > 0 && p[3] > 0)
        );
        if old.is_none() {
            old = Some((frame, actual));
        }
    }
    drop(ui);
    drop(raster);
    drop(system);
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
}

#[test]
fn atomic_local_builder_and_sparse_text_material_match_full_glyph_pixels() {
    use gpui_box::retained::logical::LogicalUi;
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let resource = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize)
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(column.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, boundary.clone(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, footer_style.clone(), 0xff0000ff)
        .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [200, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    for width in [50.0, 90.0, 140.0, 50.0] {
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(width + 10.0),
            LayoutAvailableSpace::Unbounded,
        );
        let (commit, first) = ui
            .edit_local_frame(
                constraints,
                [200.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().translation(host, [0.0; 2]);
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, width);
                        let branch = edit.reconcile_keys(scope, &[u64::from(width >= 100.0)])?[0];
                        let renders = edit.renders(branch)?;
                        let leaf = match renders.first() {
                            Some(leaf) => *leaf,
                            None => edit.append_layout(branch, host, LayoutStyle::row(), 0)?,
                        };
                        edit.properties().text(
                            leaf,
                            Some(TextPaint {
                                resource: resource.clone(),
                                color: 0xffffffff,
                            }),
                        );
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        let leaf = commit.result.value;
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            ui.layout_result(leaf).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, expected.size[1] + 10.0]
        );
        let (_, next) = ui
            .edit_local_frame(
                constraints,
                [200.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().translation(host, [2.0, 0.0]);
                    edit.properties().text(
                        leaf,
                        Some(TextPaint {
                            resource: resource.clone(),
                            color: 0x00ffffff,
                        }),
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert!(!next.stats().full_publication);
        assert_eq!(next.stats().changed_nodes, 1);
        assert_eq!(next.stats().material_nodes, 1);
        let actual = pixels(next.resolve());
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let full_host = full
            .append_layout(full.root(), boundary.clone(), 0)
            .unwrap();
        let full_leaf = full
            .append_layout(full_host, LayoutStyle::row(), 0)
            .unwrap();
        full.append_layout(full.root(), footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.translation(full_host, [2.0, 0.0]);
        tx.text(
            full_leaf,
            Some(TextPaint {
                resource: resource.clone(),
                color: 0x00ffffff,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [200.0, 700.0]).unwrap())
        );
        assert!(
            actual
                .chunks_exact(4)
                .any(|p| p[0] == 0 && p[1] > 0 && p[2] > 0 && p[3] > 0)
        );
        assert_eq!(next.resolve_geometry(leaf).unwrap().bounds.x, 7.0);
        if old.is_none() {
            old = Some((first.clone(), pixels(first.resolve())));
        }
    }
    let (old, original) = old.unwrap();
    drop(ui);
    drop(system);
    drop(raster);
    assert_eq!(pixels(old.resolve()), original);
}

#[derive(Debug)]
struct ParagraphParent;
impl ParentLayout for ParagraphParent {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let width = input.known_size.width.or(match input.available.width {
            LayoutAvailableSpace::Definite(v) => Some(v),
            _ => None,
        });
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::new(
                input.available.width,
                LayoutAvailableSpace::Unbounded,
            )
            .with_bounds(
                width
                    .map(|w| BoxConstraints::new([w, 0.0], [w, f32::INFINITY]))
                    .unwrap_or(BoxConstraints::UNBOUNDED),
            ),
            percentage_basis: input.percentage_basis,
        };
        let measured = children.measure(0, slot)?;
        // Repeated identical actual paragraph queries must retain the baseline
        // and wrapped size when the owned kernel uses its exact measure cache.
        for _ in 0..8 {
            assert_eq!(children.measure(0, slot)?, measured);
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(measured.size[0]),
                input.known_size.height.unwrap_or(measured.size[1]),
            ],
            baseline: measured.baseline,
            last_baseline: None,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [0.0; 2],
            }],
        })
    }
}

#[derive(Debug)]
struct HalfInline;
impl ParentInlineLayout for HalfInline {
    fn allocate(
        &self,
        input: ParentInlineInput,
        children: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        let width = input.known_width.unwrap_or(match input.available {
            LayoutAvailableSpace::Definite(w) => w,
            _ => 40.0,
        }) * 0.5;
        Ok(vec![
            ChildInlineAllocation {
                width,
                percentage_basis: Some(width)
            };
            children
        ])
    }
}

#[derive(Debug)]
struct LooseParagraphInput;
impl ParentAllocationLayout for LooseParagraphInput {
    fn allocate(
        &self,
        input: ParentAllocationInput,
        children: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = match input.constraints.width {
            LayoutAvailableSpace::Definite(w) => w,
            _ => 80.0,
        };
        Ok(vec![
            ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(width),
                    LayoutAvailableSpace::Unbounded
                )
                .with_bounds(BoxConstraints::new([0.0; 2], [width, f32::INFINITY])),
                percentage_basis: LayoutSize::new(Some(width), None),
            };
            children
        ])
    }
}

#[test]
fn full_allocation_builder_uses_real_paragraph_height_without_size_containment() {
    use gpui_box::retained::logical::LogicalUi;
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(column.clone(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, column.clone(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                root,
                Some(
                    ParentLayoutSnapshot::new(ParagraphParent)
                        .with_allocation_layout(LooseParagraphInput),
                ),
            );
            Ok(())
        },
    )
    .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut first = None;
    for width in [60.0, 140.0, 90.0, 60.0] {
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(width),
            LayoutAvailableSpace::Unbounded,
        );
        let (commit, frame) = ui
            .edit_local_frame(
                constraints,
                [300.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::new([0.0; 2], [width, f32::INFINITY])
                        );
                        assert_eq!(input.percentage_basis.height, None);
                        let branch = edit.reconcile_keys(scope, &[u64::from(width >= 100.0)])?[0];
                        let renders = edit.renders(branch)?;
                        let (leaf, footer) = if renders.is_empty() {
                            let leaf = edit.append_layout(branch, host, LayoutStyle::row(), 0)?;
                            let footer =
                                edit.append_layout(branch, host, footer_style.clone(), 0xff0000ff)?;
                            (leaf, footer)
                        } else {
                            (renders[0], renders[1])
                        };
                        edit.properties().text(leaf, Some(text.clone()));
                        Ok((leaf, footer))
                    })
                },
            )
            .unwrap();
        let height = paragraph.layout(Some(width)).unwrap().size[1];
        assert_eq!(ui.layout_result(root).unwrap().size, [width, height + 10.0]);
        assert_eq!(ui.layout_result(host).unwrap().size, [width, height + 10.0]);
        let (leaf, footer) = commit.result.value;
        assert_eq!(ui.layout_result(footer).unwrap().location[1], height);
        assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full.append_layout(full.root(), column.clone(), 0).unwrap();
        let leaf = full.append_layout(host, LayoutStyle::row(), 0).unwrap();
        full.append_layout(host, footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(leaf, Some(text.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let actual = pixels(frame.resolve());
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0 && p[2] > 0));
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [300.0, 700.0]).unwrap())
        );
        first.get_or_insert((frame, actual));
    }
    drop((ui, system, raster, text, paragraph));
    let (old, expected) = first.unwrap();
    assert_eq!(pixels(old.resolve()), expected);
}

#[derive(Debug)]
struct KnownWidthParagraphInput;
impl ParentAllocationLayout for KnownWidthParagraphInput {
    fn allocate(
        &self,
        input: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = input.known_size.width.unwrap_or(0.0);
        Ok(vec![
            ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(width),
                    LayoutAvailableSpace::Unbounded
                )
                .with_bounds(BoxConstraints::loose([width, f32::INFINITY])),
                percentage_basis: LayoutSize::new(Some(width), None),
            };
            count
        ])
    }
}

#[test]
fn ordinary_flex_boundary_and_mixed_allocation_builder_rewrap_real_paragraphs() {
    use gpui_box::retained::logical::LogicalUi;
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let outer = LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Px(400.0));
    let rail_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(400.0))
        .flex_shrink(0.0);
    let cut_style = LayoutStyle::column()
        .container_type(LayoutContainerType::Size)
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0));
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(outer.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    ui.append_render_layout(scope, ui.root_render(), rail_style.clone(), 0x0000ffff)
        .unwrap();
    let cut = ui
        .append_render_layout(scope, ui.root_render(), cut_style.clone(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(scope, cut, column.clone(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                cut,
                Some(
                    ParentLayoutSnapshot::new(ParagraphParent)
                        .with_allocation_layout(KnownWidthParagraphInput),
                ),
            );
            Ok(())
        },
    )
    .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    for width in [60.0, 140.0, 90.0, 60.0] {
        let constraints = LayoutConstraints::viewport(width + 20.0, 700.0);
        let (commit, frame) = ui
            .edit_local_frame(
                constraints,
                [300.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::loose([width, f32::INFINITY])
                        );
                        assert_eq!(input.percentage_basis.height, None);
                        let branch = edit.reconcile_keys(scope, &[u64::from(width >= 100.0)])?[0];
                        let outputs = edit.renders(branch)?;
                        let (leaf, footer) = if outputs.is_empty() {
                            (
                                edit.append_layout(branch, host, LayoutStyle::row(), 0)?,
                                edit.append_layout(branch, host, footer_style.clone(), 0xff0000ff)?,
                            )
                        } else {
                            (outputs[0], outputs[1])
                        };
                        edit.properties().text(leaf, Some(paint.clone()));
                        Ok((leaf, footer))
                    })
                },
            )
            .unwrap();
        let height = paragraph.layout(Some(width)).unwrap().size[1];
        assert_eq!(ui.layout_result(cut).unwrap().size, [width, 400.0]);
        assert_eq!(ui.layout_result(host).unwrap().size, [width, height + 10.0]);
        assert_eq!(
            ui.layout_result(commit.result.value.1).unwrap().location[1],
            height
        );
        assert_eq!(frame.hit_test(21.0, 1.0), Some(commit.result.value.0));
        let mut full = UiTree::new_layout(outer.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        full.append_layout(full.root(), rail_style.clone(), 0x0000ffff)
            .unwrap();
        let cut = full
            .append_layout(full.root(), cut_style.clone(), 0)
            .unwrap();
        let host = full.append_layout(cut, column.clone(), 0).unwrap();
        let leaf = full.append_layout(host, LayoutStyle::row(), 0).unwrap();
        full.append_layout(host, footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(leaf, Some(paint.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let actual = pixels(frame.resolve());
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0 && p[2] > 0));
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [300.0, 700.0]).unwrap())
        );
        old.get_or_insert((frame, actual));
    }
    drop((ui, system, raster, paragraph, paint));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
}

#[test]
fn independent_parent_width_queries_and_keyed_builder_reflow_real_paragraphs() {
    use gpui_box::retained::{logical::LogicalUi, responsive::*};
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0x00ffffff,
    };
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let padded = boundary
        .clone()
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(column.clone(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let parent = ui
        .append_render_layout(scope, root, column.clone(), 0)
        .unwrap();
    // Query the custom parent's assigned child width, then build inside its
    // content slot after the style stage has selected padding.
    let query = ui
        .append_render_layout(
            scope,
            parent,
            LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
            0,
        )
        .unwrap();
    let slot = ui
        .append_render_layout(scope, query, boundary.clone(), 0)
        .unwrap();
    let source = ui.state(scope, 1, || boundary.clone()).unwrap();
    let footer = ui
        .append_render_layout(scope, root, footer_style.clone(), 0xff0000ff)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                parent,
                Some(ParentLayoutSnapshot::new(ParagraphParent).with_inline_layout(HalfInline)),
            );
            Ok(())
        },
    )
    .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(8192).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut first = None;
    for width in [100.0, 180.0, 280.0, 100.0] {
        let wide = width >= 140.0;
        let content_width = width * 0.5 - if wide { 10.0 } else { 0.0 };
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(width),
            LayoutAvailableSpace::Unbounded,
        );
        let (_, frame) = ui
            .edit_local_frame(
                constraints,
                [300.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.responsive_rule(
                        scope,
                        1,
                        source,
                        slot,
                        LayoutQuery::ContainerInline(query),
                        LayoutBreakpointStyles::new(
                            boundary.clone(),
                            vec![(70.0, padded.clone())],
                        )?,
                    )?;
                    edit.responsive_registered(&LayoutEnvironment::new([width, 700.0]))?;
                    edit.inline_constraint_builder(slot, |input, edit| {
                        assert_eq!(input.max, content_width);
                        let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                        let leaf = edit
                            .renders(branch)?
                            .first()
                            .copied()
                            .map(Ok)
                            .unwrap_or_else(|| {
                                edit.append_layout(branch, slot, LayoutStyle::row(), 0)
                            })?;
                        edit.properties().text(leaf, Some(text.clone()));
                        Ok(())
                    })
                },
            )
            .unwrap();
        let expected_height =
            paragraph.layout(Some(content_width)).unwrap().size[1] + if wide { 10.0 } else { 0.0 };
        assert_eq!(
            ui.layout_result(parent).unwrap().size,
            [width, expected_height]
        );
        assert_eq!(
            ui.layout_result(query).unwrap().size,
            [width * 0.5, expected_height]
        );
        assert_eq!(
            ui.layout_result(footer).unwrap().location[1],
            expected_height
        );
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let p = full.append_layout(full.root(), column.clone(), 0).unwrap();
        let h = full
            .append_layout(p, LayoutStyle::column().fixed_inline_size(width * 0.5), 0)
            .unwrap();
        let b = full
            .append_layout(
                h,
                if wide {
                    padded.clone()
                } else {
                    boundary.clone()
                },
                0,
            )
            .unwrap();
        let leaf = full.append_layout(b, LayoutStyle::row(), 0).unwrap();
        full.append_layout(full.root(), footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(leaf, Some(text.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let actual = pixels(frame.resolve());
        assert!(actual.chunks_exact(4).any(|p| p[1] > 0 && p[2] > 0));
        assert!(actual == pixels(full.frame_in_viewport(constraints, [300.0, 700.0]).unwrap()));
        if first.is_none() {
            first = Some((frame, actual));
        }
    }
    drop((ui, system, raster));
    let (old, expected) = first.unwrap();
    assert!(pixels(old.resolve()) == expected);
}

#[test]
fn custom_parent_allocates_real_paragraph_and_matches_independent_flex_glyph_pixels() {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0xffffffff,
    };
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let boundary = column
        .clone()
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut tree = UiTree::new_layout(column.clone(), 0).unwrap();
    let host = tree
        .append_layout(tree.root(), boundary.clone(), 0)
        .unwrap();
    let leaf = tree.append_layout(host, LayoutStyle::row(), 0).unwrap();
    let footer = tree
        .append_layout(tree.root(), footer_style.clone(), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.text(leaf, Some(text.clone()));
    tx.parent_layout(host, Some(ParentLayoutSnapshot::new(ParagraphParent)));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [200, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    for width in [50.0, 90.0, 140.0, 50.0] {
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(width + 10.0),
            LayoutAvailableSpace::Unbounded,
        );
        let frame = tree.frame_in_viewport(constraints, [200.0, 700.0]).unwrap();
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            tree.layout_result(leaf).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            tree.layout_result(host).unwrap().size,
            [width + 10.0, expected.size[1] + 10.0]
        );
        assert_eq!(
            tree.layout_result(host).unwrap().baseline,
            Some(paragraph.first_baseline() + 5.0)
        );
        assert_eq!(
            tree.layout_result(footer).unwrap().location,
            [0.0, expected.size[1] + 10.0]
        );
        let actual = pixels(frame.clone());
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full
            .append_layout(full.root(), boundary.clone(), 0)
            .unwrap();
        let leaf = full.append_layout(host, LayoutStyle::row(), 0).unwrap();
        full.append_layout(full.root(), footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(leaf, Some(text.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [200.0, 700.0]).unwrap())
        );
        assert!(
            actual
                .chunks_exact(4)
                .any(|p| p[0] > 0 && p[1] > 0 && p[2] > 0 && p[3] > 0)
        );
        if old.is_none() {
            old = Some((frame, actual));
        }
    }
    let (old, original) = old.unwrap();
    drop(tree);
    drop(system);
    drop(raster);
    assert_eq!(pixels(old), original);
}

#[test]
fn inline_constraint_builder_publishes_real_text_auto_height_and_following_sibling_pixels() {
    use gpui_box::retained::logical::LogicalUi;
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let text = TextPaint {
        resource: raster.prepare(paragraph.clone(), 1.0).unwrap(),
        color: 0xffffffff,
    };
    let column = LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize)
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let footer_style = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let mut ui = LogicalUi::new_layout(column.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), boundary.clone(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, ui.root_render(), footer_style.clone(), 0xff0000ff)
        .unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [200, 700],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let mut old = None;
    for width in [50.0, 90.0, 140.0, 50.0] {
        let constraints = LayoutConstraints::new(
            LayoutAvailableSpace::Definite(width + 10.0),
            LayoutAvailableSpace::Unbounded,
        );
        let (commit, frame) = ui
            .edit_frame(
                constraints,
                [200.0, 700.0],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, width);
                        let branch =
                            edit.reconcile_keys(scope, &[u64::from(input.max >= 100.0)])?[0];
                        let renders = edit.renders(branch)?;
                        if let Some(leaf) = renders.first() {
                            return Ok(*leaf);
                        }
                        let leaf = edit.append_layout(branch, host, LayoutStyle::row(), 0)?;
                        edit.properties().text(leaf, Some(text.clone()));
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        let leaf = commit.result.value;
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            ui.layout_result(leaf).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            [width + 10.0, expected.size[1] + 10.0]
        );
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, expected.size[1] + 10.0]
        );
        assert_eq!(
            ui.layout_result(leaf).unwrap().baseline,
            Some(paragraph.first_baseline())
        );
        let actual = pixels(frame.clone());
        let mut full = UiTree::new_layout(column.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let full_host = full
            .append_layout(full.root(), boundary.clone(), 0)
            .unwrap();
        let full_leaf = full
            .append_layout(full_host, LayoutStyle::row(), 0)
            .unwrap();
        full.append_layout(full.root(), footer_style.clone(), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(full_leaf, Some(text.clone()));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            actual,
            pixels(full.frame_in_viewport(constraints, [200.0, 700.0]).unwrap())
        );
        assert!(
            actual
                .chunks_exact(4)
                .any(|p| p[0] > 0 && p[1] > 0 && p[2] > 0 && p[3] > 0),
            "real glyphs rendered"
        );
        if old.is_none() {
            old = Some((frame, actual));
        }
    }
    let (old, original) = old.unwrap();
    drop(ui);
    drop(system);
    drop(raster);
    assert_eq!(pixels(old), original);
}

#[test]
fn constraint_builder_branches_measure_real_paragraphs_at_final_flex_width() {
    use gpui_box::retained::logical::LogicalUi;
    let paragraph = shape(&text_system(), "one two three four five six 中文段落");
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_scope();
    let render_root = ui.root_render();
    let host = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.append_layout(
                    root,
                    render_root,
                    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                        .container_type(LayoutContainerType::Size)
                        .flex_grow(1.0)
                        .flex_basis(LayoutDimension::Px(0.0))
                        .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                    0,
                )
            },
        )
        .unwrap()
        .result
        .value;
    let mut old = None;
    for width in [50.0, 90.0, 140.0, 50.0] {
        let (commit, frame) = ui
            .edit_frame(
                LayoutConstraints::viewport(width + 10.0, 300.0),
                [200.0, 300.0],
                |_| Ok(()),
                |(), edit| {
                    edit.constraint_builder(host, |input, edit| {
                        assert_eq!(input.constraints.bounds.max.width, width);
                        let branch = edit.reconcile_keys(root, &[u64::from(width >= 100.0)])?[0];
                        let renders = edit.renders(branch)?;
                        if let Some(leaf) = renders.first() {
                            return Ok(*leaf);
                        }
                        let leaf =
                            edit.append_layout(branch, host, LayoutStyle::row(), 0xffffffff)?;
                        edit.properties()
                            .measure(leaf, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        let leaf = commit.result.value;
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            ui.layout_result(leaf).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            ui.layout_result(leaf).unwrap().baseline,
            Some(paragraph.first_baseline())
        );
        if old.is_none() {
            old = Some((frame, leaf));
        }
    }
    let (frame, leaf) = old.unwrap();
    assert_eq!(
        frame
            .items
            .iter()
            .find(|item| item.node == leaf)
            .unwrap()
            .bounds
            .width,
        50.0
    );
}

#[test]
fn nested_parent_constraint_boxes_reflow_real_paragraphs_at_allocated_width() {
    let paragraph = shape(&text_system(), "one two three four five six 中文段落");
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().align_items(LayoutAlignItems::Start),
        0,
    )
    .unwrap();
    let boundary = tree
        .append_layout(
            tree.root(),
            LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let leaf = tree
        .append_layout(boundary, LayoutStyle::row(), 0xffffffff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(leaf, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for width in [50.0, 90.0, 140.0, 50.0] {
        let mut tx = Transaction::default();
        tx.layout_style(
            boundary,
            LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .size(LayoutDimension::Px(width + 10.0), LayoutDimension::Auto)
                .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame_in_viewport(
            LayoutConstraints::new(
                LayoutAvailableSpace::Definite(300.0),
                LayoutAvailableSpace::Unbounded,
            ),
            [300.0, 500.0],
        )
        .unwrap();
        let expected = paragraph.layout(Some(width)).unwrap();
        assert_eq!(
            tree.layout_result(leaf).unwrap().size,
            [width, expected.size[1]]
        );
        assert_eq!(
            tree.layout_result(boundary).unwrap().size,
            [width + 10.0, expected.size[1] + 10.0]
        );
        assert_eq!(
            tree.layout_result(boundary).unwrap().baseline,
            Some(paragraph.first_baseline() + 5.0)
        );
    }
}

#[test]
fn parent_box_width_reflows_real_paragraph_and_intrinsic_height_without_rewriting_style() {
    let paragraph = shape(&text_system(), "one two three four five six seven 中文段落");
    for intrinsic_height in 0..3 {
        let mut style = LayoutStyle::row().size(
            LayoutDimension::Px(500.0),
            if intrinsic_height == 1 {
                LayoutDimension::MinContent
            } else {
                LayoutDimension::Auto
            },
        );
        if intrinsic_height == 2 {
            style = style.min_size(LayoutDimension::Px(200.0), LayoutDimension::MinContent);
        }
        let mut tree = UiTree::new_layout(style.clone(), 0xffffffff).unwrap();
        let mut tx = Transaction::default();
        tx.measure(
            tree.root(),
            Some(MeasureSnapshot::Paragraph(paragraph.clone())),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let available = LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        );
        let mut old = None;
        for width in [50.0, 90.0, 140.0, 50.0] {
            let input =
                available.with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY]));
            let frame = tree
                .local_frame_in_viewport(input, [500.0, 1000.0])
                .unwrap();
            let result = tree.layout_result(tree.root()).unwrap();
            assert_eq!(
                result.size,
                paragraph.layout(Some(width)).unwrap().size,
                "{intrinsic_height}/{width}"
            );
            assert_eq!(result.baseline, Some(paragraph.first_baseline()));
            assert_eq!(
                tree.local_frame_in_viewport(input, [500.0, 1000.0])
                    .unwrap()
                    .revision(),
                frame.revision()
            );
            if old.is_none() {
                old = Some(frame);
            }
        }
        assert_eq!(
            old.unwrap()
                .resolve_geometry(tree.root())
                .unwrap()
                .bounds
                .width,
            50.0
        );
    }
}

#[test]
fn unbounded_paragraph_measurement_and_finite_viewport_remain_independent() {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six seven 中文段落");
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().align_items(LayoutAlignItems::Start),
        0,
    )
    .unwrap();
    let child = tree
        .append_layout(tree.root(), LayoutStyle::row(), 0xffffffff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(child, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    tree.frame_in_viewport(constraints, [50.0, 500.0]).unwrap();
    assert_eq!(
        tree.layout_result(child).unwrap().size,
        paragraph.layout(None).unwrap().size
    );
    assert_eq!(
        tree.layout_result(child).unwrap().baseline,
        Some(paragraph.first_baseline())
    );
    let first = tree.layout_result(child).unwrap();
    assert_eq!(
        tree.frame_in_viewport(constraints, [90.0, 500.0])
            .unwrap()
            .layout_runs,
        0
    );
    assert_eq!(tree.layout_result(child).unwrap(), first);
    tree.frame_in_viewport(
        LayoutConstraints::new(
            LayoutAvailableSpace::Definite(50.0),
            LayoutAvailableSpace::Unbounded,
        ),
        [50.0, 500.0],
    )
    .unwrap();
    assert_eq!(
        tree.layout_result(child).unwrap().size,
        paragraph.layout(Some(50.0)).unwrap().size
    );
    tree.frame_in_viewport(constraints, [50.0, 500.0]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap(), first);
}

#[test]
fn intrinsic_height_limit_uses_final_flex_width_for_shaped_paragraph() {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six seven 中文段落");
    for width in [50.0, 90.0, 140.0] {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row()
                .size(LayoutDimension::Px(width), LayoutDimension::Px(500.0))
                .align_items(LayoutAlignItems::Start),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::row()
                    .size(LayoutDimension::Px(400.0), LayoutDimension::Px(1.0))
                    .min_size(LayoutDimension::Px(0.0), LayoutDimension::MaxContent),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.measure(child, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([width, 500.0]).unwrap();
        let result = tree.layout_result(child).unwrap();
        assert_eq!(
            result.size,
            [width, paragraph.layout(Some(width)).unwrap().size[1]]
        );
        assert_eq!(result.baseline, Some(paragraph.first_baseline()));
    }
}

#[test]
fn draft_constructed_text_measures_like_fresh_and_can_become_a_container() {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(shape(&system, "draft 中文"), 1.0).unwrap(),
        color: 0xffffffff,
    };
    let mut tree = UiTree::new(Style::default(), 0).unwrap();
    let root = tree.root();
    let node = tree
        .edit(BackendCapabilities::default(), |edit| {
            let node = edit.append(root, Style::default(), 0)?;
            edit.properties().text(node, Some(paint.clone()));
            Ok(node)
        })
        .unwrap()
        .value;
    let frame = tree.frame([200.0, 100.0]).unwrap();
    let mut fresh = UiTree::new(Style::default(), 0).unwrap();
    let leaf = fresh.append(fresh.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.text(leaf, Some(paint));
    fresh.commit(tx, BackendCapabilities::default()).unwrap();
    let expected = fresh.frame([200.0, 100.0]).unwrap();
    let item = frame.items.iter().find(|i| i.node == node).unwrap();
    let other = expected.items.iter().find(|i| i.node == leaf).unwrap();
    assert_eq!(item.bounds, other.bounds);
    assert!(item.text.is_some());
    let child = tree
        .edit(BackendCapabilities::default(), |edit| {
            edit.properties().text(node, None);
            edit.append(node, Style::default(), 0x4488ffff)
        })
        .unwrap()
        .value;
    let next = tree.frame([200.0, 100.0]).unwrap();
    assert_eq!(next.revision, frame.revision + 1);
    assert!(next.items.iter().any(|i| i.node == child));
    assert!(
        next.items
            .iter()
            .find(|i| i.node == node)
            .unwrap()
            .text
            .is_none()
    );
}

#[test]
fn glyph_resources_share_masks_bound_width_cache_and_outlive_the_font_owner() {
    let system = text_system();
    let paragraph = shape(&system, "office a\u{301} 中文\nsecond line");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let resource = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let count = raster.rasterized_glyphs();
    let shared = raster.prepare(paragraph, 1.0).unwrap();
    assert_eq!(raster.rasterized_glyphs(), count);
    let chunk = resource.chunk(120.0).unwrap();
    let other = shared.chunk(120.0).unwrap();
    assert!(chunk.glyphs.len() > 10);
    for (a, b) in chunk.glyphs.iter().zip(other.glyphs.iter()) {
        assert!(Arc::ptr_eq(&a.bitmap, &b.bitmap));
        assert_eq!(a.origin, b.origin);
    }
    assert!(Arc::ptr_eq(&chunk, &resource.chunk(120.0).unwrap()));
    let weak_chunk = Arc::downgrade(&chunk);
    let weak_bitmap = Arc::downgrade(&chunk.glyphs[0].bitmap);
    drop(chunk);
    for width in [121.0, 122.0, 123.0, 124.0] {
        resource.chunk(width).unwrap();
    }
    assert!(weak_chunk.upgrade().is_none());
    drop(other);
    drop(shared);
    drop(system);
    drop(raster);
    let chunk = std::thread::spawn(move || resource.chunk(120.0).unwrap())
        .join()
        .unwrap();
    assert!(weak_bitmap.upgrade().is_some());
    drop(chunk);
    assert!(weak_bitmap.upgrade().is_none());
}

#[test]
fn glyph_preparation_rejects_foreign_fonts_stale_epochs_and_invalid_scale() {
    let system = text_system();
    let paragraph = shape(&system, "font epoch 中文");
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    for scale in [0.0, -1.0, f32::INFINITY, f32::NAN, f32::from_bits(1)] {
        assert_eq!(
            raster.prepare(paragraph.clone(), scale).unwrap_err(),
            Error::InvalidTextResource
        );
    }
    assert_eq!(
        raster.prepare(paragraph.clone(), f32::MAX).unwrap_err(),
        Error::UnsupportedText
    );
    let mut foreign = GlyphRasterizer::new(Arc::clone(&text_system()));
    assert_eq!(
        foreign.prepare(paragraph.clone(), 1.0).unwrap_err(),
        Error::InvalidTextResource
    );
    let old = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let first = old.chunk(200.0).unwrap();
    let scaled = raster.prepare(paragraph.clone(), 1.5).unwrap();
    assert_ne!(
        first.glyphs[0].bitmap.id(),
        scaled.chunk(200.0).unwrap().glyphs[0].bitmap.id()
    );
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    assert_eq!(
        raster.prepare(paragraph, 1.0).unwrap_err(),
        Error::InvalidTextResource
    );
    let current = raster
        .prepare(shape(&system, "font epoch 中文"), 1.0)
        .unwrap();
    assert_ne!(
        first.glyphs[0].bitmap.id(),
        current.chunk(200.0).unwrap().glyphs[0].bitmap.id()
    );
    assert!(Arc::ptr_eq(&first, &old.chunk(200.0).unwrap()));
}

#[test]
fn typed_text_equality_color_and_overflow_damage_preserve_owned_glyph_chunks() {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let resource = raster.prepare(shape(&system, "Glyphs 中文"), 1.0).unwrap();
    let mut tree = UiTree::new(Style::default(), 0).unwrap();
    let node = tree
        .append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(120.0),
                    height: length(1.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let mut ui = ReactiveUi::new(tree, BackendCapabilities::default()).unwrap();
    let initial = Some(TextPaint {
        resource: resource.clone(),
        color: 0xffdd88ff,
    });
    let signal = ui.signal(initial.clone());
    ui.bind(signal, Property::text(node)).unwrap();
    let before = ui.frame([300.0, 120.0]).unwrap();
    let equal = ui
        .transaction(|tx| {
            tx.set(signal, initial.clone())?;
            Ok(())
        })
        .unwrap();
    assert_eq!((equal.changed_sources, equal.sink_writes), (0, 0));
    assert!(!equal.commit.request_frame);
    let paint = TextPaint {
        resource,
        color: 0x66ccff80,
    };
    let changed = ui
        .transaction(|tx| {
            tx.set(signal, Some(paint))?;
            Ok(())
        })
        .unwrap();
    assert!(
        changed
            .commit
            .routes
            .contains(&(node, UpdatePath::ResourcePatch))
    );
    let after = ui.frame([300.0, 120.0]).unwrap();
    assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
    assert!(Arc::ptr_eq(
        before.items[1].glyphs.as_ref().unwrap(),
        after.items[1].glyphs.as_ref().unwrap()
    ));
    let item = &after.items[1];
    let ink = item.ink_bounds().unwrap();
    assert!(ink.y + ink.height > item.bounds.y + item.bounds.height);
    let damage = after.damage.bounds.unwrap();
    assert_eq!(
        damage.intersect(ink),
        ink,
        "damage includes ink outside the layout box"
    );
}

#[test]
fn text_image_swaps_adopt_final_measurement_and_conflicts_roll_back_atomically() {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(Arc::clone(&system));
    let paint = TextPaint {
        resource: raster.prepare(shape(&system, "text"), 1.0).unwrap(),
        color: 0xffffffff,
    };
    let image = ImageResource::rgba8([13, 17], vec![255; 13 * 17 * 4]).unwrap();
    let mut tree = UiTree::new(
        Style {
            align_items: taffy::AlignItems::START,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let node = tree.append(tree.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.text(node, Some(paint.clone()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([300.0, 120.0]).unwrap();
    for with_image in [false, true] {
        let mut invalid = Transaction::default();
        invalid.color(tree.root(), 0xff0000ff);
        if with_image {
            invalid.image(node, Some(image.clone()));
        } else {
            invalid.measure(node, None);
        }
        assert_eq!(
            tree.commit(invalid, BackendCapabilities::default())
                .unwrap_err(),
            Error::InvalidMeasure
        );
        let frame = tree.frame([300.0, 120.0]).unwrap();
        assert_eq!(frame.revision, before.revision);
        assert_eq!(frame.items[0].fill_color, 0);
    }
    for _ in 0..2 {
        let mut tx = Transaction::default();
        tx.text(node, None);
        tx.image(node, Some(image.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([300.0, 120.0]).unwrap();
        assert_eq!(
            [frame.items[1].bounds.width, frame.items[1].bounds.height],
            [13.0, 17.0]
        );
        assert!(frame.items[1].glyphs.is_none());
        let mut tx = Transaction::default();
        tx.image(node, None);
        tx.text(node, Some(paint.clone()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([300.0, 120.0]).unwrap();
        assert_eq!(frame.items[1].bounds, before.items[1].bounds);
        assert!(frame.items[1].image.is_none());
    }
    let mut tx = Transaction::default();
    tx.text(node, None);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([300.0, 120.0]).unwrap();
    assert!(frame.items[1].glyphs.is_none());
    assert_eq!(frame.items[1].bounds.height, 0.0);
}

#[test]
fn owned_paragraph_wrap_matches_real_gpui_layout_for_unicode_and_cluster_boundaries() {
    let system = text_system();
    let text = "office affine a\u{301} 中文混排 one two three\n\nlast line\n";
    let snapshot = shape(&system, text);
    assert_eq!(snapshot.hard_line_count(), 4);
    assert!(snapshot.min_content_width() > 0.0);
    assert!(snapshot.max_content_width() > snapshot.min_content_width());
    let cached = snapshot.layout(Some(80.0)).unwrap();
    assert!(Arc::ptr_eq(&cached, &snapshot.layout(Some(80.0)).unwrap()));
    let weak = Arc::downgrade(&cached);
    drop(cached);
    for width in [81.0, 82.0, 83.0, 84.0] {
        snapshot.layout(Some(width)).unwrap();
    }
    assert!(
        weak.upgrade().is_none(),
        "four-width memoization must evict old results"
    );
    let mut narrowest_height = 0.0;
    for width in [0.0, 1.0, 31.0, 80.0, 200.0, 1000.0] {
        let owned = snapshot.layout(Some(width)).unwrap();
        let mut expected_height = 0.0;
        for (original, actual) in text.split('\n').zip(&owned.lines) {
            let expected = system
                .shape_text(
                    original.to_owned().into(),
                    px(16.0),
                    &[TextRun {
                        len: original.len(),
                        font: font("Noto Sans CJK SC"),
                        ..Default::default()
                    }],
                    Some(px(width)),
                    None,
                )
                .unwrap();
            assert_eq!(actual.wrap_boundaries(), expected[0].wrap_boundaries());
            assert_eq!(actual.runs().len(), expected[0].runs().len());
            expected_height += f32::from(expected[0].size(px(32.0)).height);
            for (a, b) in actual.runs().iter().zip(expected[0].runs()) {
                assert_eq!(a.font_id, b.font_id);
                assert_eq!(a.glyphs.len(), b.glyphs.len());
                for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
                    assert_eq!((a.id, a.position, a.index), (b.id, b.position, b.index));
                }
            }
        }
        assert_eq!(owned.size[1], expected_height);
        if width == 0.0 {
            narrowest_height = owned.size[1];
        }
        assert!(owned.size[1] <= narrowest_height);
    }
    let owned = snapshot.clone();
    let before = owned.layout(Some(80.0)).unwrap().size;
    drop(system);
    let after = std::thread::spawn(move || owned.layout(Some(80.0)).unwrap().size)
        .join()
        .unwrap();
    assert_eq!(
        before, after,
        "pure wrapping survives context disposal and thread transfer"
    );
}

#[test]
fn paragraph_constraints_reflow_siblings_preserve_chunks_and_equal_input_is_idle() {
    let system = text_system();
    let paragraph = shape(
        &system,
        "one two three four five six seven eight nine ten 中文测试",
    );
    let make = |width: f32| Style {
        size: taffy::Size {
            width: length(width),
            height: taffy::Dimension::auto(),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            align_items: taffy::AlignItems::START,
            size: taffy::Size {
                width: length(400.0),
                height: length(400.0),
            },
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let text = tree.append(tree.root(), make(200.0), 0).unwrap();
    let sibling = tree
        .append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(10.0),
                    height: length(10.0),
                },
                ..Default::default()
            },
            0xff0000ff,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([400.0; 2]).unwrap();
    let bounds = |frame: &FramePacket, node| {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    };
    assert_eq!(
        bounds(&before, text).height,
        paragraph.layout(Some(200.0)).unwrap().size[1]
    );
    assert_eq!(bounds(&before, sibling).y, bounds(&before, text).height);
    let mut tx = Transaction::default();
    tx.style(text, make(80.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let narrow = tree.frame([400.0; 2]).unwrap();
    assert_eq!(
        bounds(&narrow, text).height,
        paragraph.layout(Some(80.0)).unwrap().size[1]
    );
    assert!(bounds(&narrow, sibling).y > bounds(&before, sibling).y);
    assert_eq!(narrow.recorded_chunks, 0);
    assert!(Arc::ptr_eq(
        &before.items[1].commands,
        &narrow.items[1].commands
    ));
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    assert!(
        !tree
            .commit(tx, BackendCapabilities::default())
            .unwrap()
            .request_frame
    );
    assert_eq!(
        tree.frame([400.0; 2]).unwrap().layout_stats,
        LayoutStats::default()
    );
    let mut style = make(80.0);
    style.padding = taffy::Rect {
        left: length(4.0),
        right: length(4.0),
        top: length(3.0),
        bottom: length(3.0),
    };
    let mut tx = Transaction::default();
    tx.style(text, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let padded = tree.frame([400.0; 2]).unwrap();
    assert_eq!(
        bounds(&padded, text).height,
        paragraph.layout(Some(72.0)).unwrap().size[1] + 6.0
    );
}

#[test]
fn font_epoch_replacement_and_invalid_paragraph_inputs_are_explicit() {
    let system = text_system();
    let old = shape(&system, "retained font epoch");
    let old_size = old.layout(Some(80.0)).unwrap().size;
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let next = shape(&system, "retained font epoch");
    assert!(next.font_epoch() > old.font_epoch());
    assert_ne!(old, next);
    assert_eq!(old.layout(Some(80.0)).unwrap().size, old_size);
    assert!(old.layout(Some(f32::NAN)).is_err());
    assert!(
        ParagraphSnapshot::shape(&system, "x", font("Noto Sans CJK SC"), f32::NAN, 32.0).is_err()
    );
    assert!(ParagraphSnapshot::shape(&system, "x", font("Noto Sans CJK SC"), 16.0, 1.0).is_err());
    assert_eq!(
        ParagraphSnapshot::shape(&system, "tab\tstop", font("Noto Sans CJK SC"), 16.0, 32.0)
            .unwrap_err(),
        gpui_box::retained::Error::UnsupportedText
    );
    assert_eq!(shape(&system, "").layout(None).unwrap().size, [0.0, 32.0]);
}

#[test]
fn paragraph_min_max_content_baseline_and_new_text_have_independent_dependencies() {
    let system = text_system();
    let paragraph = shape(&system, "one two 中文 paragraph");
    let mut tree = UiTree::new(
        Style {
            align_items: taffy::AlignItems::BASELINE,
            size: taffy::Size {
                width: length(800.0),
                height: length(300.0),
            },
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let text = tree
        .append(
            tree.root(),
            Style {
                flex_shrink: 0.0,
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let marker = tree.append(tree.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tx.measure(
        marker,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 4.0,
            height: 40.0,
            baseline: Some(35.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let max = tree.frame([800.0, 300.0]).unwrap();
    let bounds = |frame: &FramePacket, node| {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    };
    assert!((bounds(&max, text).width - paragraph.max_content_width()).abs() < 0.01);
    assert!(
        (bounds(&max, text).y + paragraph.first_baseline() - bounds(&max, marker).y - 35.0).abs()
            < 0.01
    );
    let longer = shape(&system, "one two 中文 paragraph\nsecond hard line");
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(longer.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.frame([800.0, 300.0]).unwrap();
    assert_eq!(next.layout_runs, 1);
    assert_eq!(next.recorded_chunks, 0);
    assert_eq!(bounds(&next, text).height, 64.0);
    assert_eq!(
        bounds(&max, text).height,
        32.0,
        "old frame retains old text metrics"
    );
    let mut small = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(1.0),
                height: length(300.0),
            },
            align_items: taffy::AlignItems::START,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let child = small.append(small.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(child, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    small.commit(tx, BackendCapabilities::default()).unwrap();
    let min = small.frame([1.0, 300.0]).unwrap();
    assert!(
        (bounds(&min, child).width - paragraph.min_content_width()).abs() < 0.01,
        "automatic minimum: {:?}, min {}",
        bounds(&min, child),
        paragraph.min_content_width()
    );
}
