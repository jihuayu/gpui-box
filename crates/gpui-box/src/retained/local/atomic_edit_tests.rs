use super::*;
use crate::retained::{logical::LogicalUi, reactive::Property};
use std::{cell::RefCell, rc::Rc};

fn column() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
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
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}

#[test]
fn repeated_inline_input_validation_retains_snapshot_after_checked_layout() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui.append_render_layout(scope, root, column(), 0).unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let calls = std::cell::Cell::new(0);
    let mut first: Option<LocalFrame> = None;
    for _ in 0..3 {
        let (commit, frame) = ui
            .edit_local_frame(
                constraints(100.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, _| {
                        assert_eq!(input.known_width, Some(100.0));
                        calls.set(calls.get() + 1);
                        Ok(())
                    })
                },
            )
            .unwrap();
        assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
        assert_eq!(ui.reactive().tree().frame.as_ref().unwrap().layout_runs, 1);
        assert!(
            ui.reactive()
                .tree()
                .layout
                .tracked_layout_input(ui.reactive().tree().node(host).unwrap().layout)
                .unwrap()
                .is_none(),
            "observation is still retired normally"
        );
        if let Some(first) = &first {
            assert!(Arc::ptr_eq(&frame.0, &first.0));
            assert!(!commit.result.commit.request_frame);
        } else {
            first = Some(frame);
        }
    }
    assert_eq!(calls.get(), 3, "reuse must not skip requested callbacks");

    let first = first.unwrap();
    let source = ui.state(scope, 99, || 1u32).unwrap();
    let failed = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |tx| tx.set(source, 2),
        |_, edit| {
            edit.inline_allocation_builder(host, |_, _| Ok(()))?;
            edit.properties().layout_style(
                root,
                column().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
            );
            Ok(())
        },
    );
    assert!(matches!(failed, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(*ui.read(source).unwrap(), 1);
    assert!(Arc::ptr_eq(
        &first.0,
        &ui.published_local_frame().unwrap().0
    ));
}

#[test]
fn atomic_local_property_material_edits_keep_sparse_predecessor_without_resolving() {
    let mut ui = LogicalUi::new_layout(
        column(),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, fixed(80.0, 80.0), 0)
        .unwrap();
    let red = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let blue = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0x0000ffff)
        .unwrap();
    let offset = ui.state(scope, 1, || [0.0; 2]).unwrap();
    ui.bind(offset, Property::translation(host)).unwrap();
    let (_, first) = ui
        .edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let template = first.0.template.items.clone();
    let topology = first.0.topology.clone();
    let baseline_revision = ui.reactive().tree().frame.as_ref().unwrap().revision;
    let mut previous = first.clone();
    let queue = frame_queue::LocalFrameQueue::new();
    for step in 1..5 {
        let (commit, next) = ui
            .edit_local_frame(
                constraints(100.0),
                [180.0; 2],
                |event| {
                    event.set(offset, [step as f32 * 2.0, 0.0])?;
                    Ok(())
                },
                |(), edit| {
                    edit.properties()
                        .color(blue, 0x000000ff + (step as u32) * 0x00002000);
                    Ok(blue)
                },
            )
            .unwrap();
        assert_eq!(commit.result.value, blue);
        assert!(next.follows(&previous));
        assert!(Arc::ptr_eq(&topology, &next.0.topology));
        assert!(!next.stats().full_publication);
        assert_eq!(next.stats().changed_nodes, 1);
        assert_eq!(next.stats().material_nodes, 1);
        assert!(
            next.0.resolved.get().is_none(),
            "atomic validation must not materialize a resolved frame"
        );
        assert_eq!(
            ui.reactive().tree().frame.as_ref().unwrap().revision,
            baseline_revision
        );
        let published = ui.published_local_frame().unwrap();
        assert!(Arc::ptr_eq(&published.0, &next.0));
        assert!(published.0.resolved.get().is_none());
        queue.publish(next.clone()).unwrap();
        assert_eq!(next.hit_test(2.0 * step as f32 + 1.0, 1.0), Some(red));
        previous = next;
    }
    assert_eq!(queue.stats().unwrap().coalesced, 3);
    let latest = queue.take_latest().unwrap().unwrap();
    assert!(Arc::ptr_eq(&latest.0, &previous.0));
    assert!(latest.0.resolved.get().is_none());
    assert_eq!(template[1].spatial.translation, [0.0; 2]);
    let resolved = latest.resolve();
    assert_eq!(
        pixels(resolved),
        pixels(
            ui.frame_in_viewport(constraints(100.0), [180.0; 2])
                .unwrap()
        )
    );
    let (commit, same) = ui
        .edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    // Resolved subscriber advances the baseline, requiring one complete local rebuild.
    assert!(same.stats().full_publication);
    assert!(!commit.result.commit.request_frame);
    let (_, idle) = ui
        .edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    assert!(Arc::ptr_eq(&idle.0, &same.0));
    drop(ui);
    assert_eq!(first.hit_test(1.0, 1.0), Some(red));
}

#[test]
fn inline_keyed_local_frames_publish_content_height_lifetimes_and_old_pixels() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize)
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let host = ui
        .append_render_layout(scope, root, boundary.clone(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let mut old = None;
    for width in [80.0, 80.0, 140.0, 140.0, 80.0] {
        let events = events.clone();
        let (commit, frame) = ui
            .edit_local_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, width - 10.0);
                        let key = u64::from(input.max >= 100.0);
                        let branch = edit.reconcile_keys(scope, &[key])?[0];
                        let value = edit.state(branch, 1, || key)?;
                        edit.effect(branch, 2, &[], move |_| {
                            events.borrow_mut().push(("run", key));
                            let events = events.clone();
                            Some(Box::new(move || events.borrow_mut().push(("cleanup", key))))
                        })?;
                        let renders = edit.renders(branch)?;
                        let leaf = match renders.first() {
                            Some(node) => *node,
                            None => edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if key == 0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?,
                        };
                        Ok((value, leaf))
                    })
                },
            )
            .unwrap();
        let (state, leaf) = commit.result.value;
        let height = if width < 100.0 { 20.0 } else { 60.0 };
        assert_eq!(*ui.read(state).unwrap(), u64::from(width >= 100.0));
        assert_eq!(ui.layout_result(host).unwrap().size, [width, height + 10.0]);
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, height + 10.0]
        );
        assert_eq!(frame.hit_test(6.0, 6.0), Some(leaf));
        assert_eq!(frame.hit_test(1.0, height + 11.0), Some(footer));
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full
            .append_layout(full.root(), boundary.clone(), 0)
            .unwrap();
        full.append_layout(host, fixed(400.0, height), 0xff0000ff)
            .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        assert_eq!(
            pixels(frame.resolve()),
            pixels(
                full.frame_in_viewport(constraints(width), [180.0; 2])
                    .unwrap()
            )
        );
        if old.is_none() {
            old = Some((frame.clone(), pixels(frame.resolve())));
        }
    }
    assert_eq!(
        *events.borrow(),
        [
            ("run", 0),
            ("cleanup", 0),
            ("run", 1),
            ("cleanup", 1),
            ("run", 0)
        ]
    );
    drop(ui);
    let (old, expected) = old.unwrap();
    assert_eq!(pixels(old.resolve()), expected);
}

#[test]
fn failed_sparse_local_frame_preserves_sources_effects_owner_snapshot_and_provisional_ids() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, fixed(40.0, 40.0), 0)
        .unwrap();
    let child = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let offset = ui.state(scope, 1, || [0.0; 2]).unwrap();
    ui.bind(offset, Property::translation(host)).unwrap();
    let effects = Rc::new(RefCell::new(vec![]));
    let log = effects.clone();
    ui.effect(scope, 4, &[offset.dependency()], move |input| {
        log.borrow_mut().push(*input.read(offset).unwrap());
        None
    })
    .unwrap();
    ui.edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let (_, old) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |event| {
                event.set(offset, [2.0, 0.0])?;
                Ok(())
            },
            |(), _| Ok(()),
        )
        .unwrap();
    assert!(old.0.resolved.get().is_none());
    let stats = ui.stats();
    let effect_log = effects.borrow().clone();
    let result = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |event| {
            event.set(offset, [f32::MAX, 0.0])?;
            Ok(())
        },
        |(), edit| {
            edit.properties().translation(child, [f32::MAX, 0.0]);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidFrame)));
    assert_eq!(*ui.read(offset).unwrap(), [2.0, 0.0]);
    assert_eq!(effects.borrow().as_slice(), effect_log);
    assert_eq!(ui.stats().transactions, stats.transactions);
    assert!(Arc::ptr_eq(&old.0, &ui.published_local_frame().unwrap().0));
    assert!(old.0.resolved.get().is_none());
    let mut escaped = None;
    let result = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let branch = edit.reconcile_keys(scope, &[1])?[0];
            let source = edit.state(branch, 2, || 5u32)?;
            let parent = edit.append_layout(branch, host, fixed(10.0, 10.0), 0)?;
            let leaf = edit.append_layout(branch, parent, fixed(5.0, 5.0), 0)?;
            escaped = Some((branch, source, leaf));
            edit.properties().translation(parent, [f32::MAX, 0.0]);
            edit.properties().translation(leaf, [f32::MAX, 0.0]);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidFrame)));
    assert!(ui.children(scope).unwrap().is_empty());
    let (old_scope, old_source, old_node) = escaped.unwrap();
    assert!(ui.read(old_source).is_err());
    assert!(ui.layout_result(old_node).is_err());
    let (commit, next) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let branch = edit.reconcile_keys(scope, &[1])?[0];
                let leaf = edit.append_layout(branch, host, fixed(5.0, 5.0), 0x0000ffff)?;
                Ok((branch, leaf))
            },
        )
        .unwrap();
    assert_ne!(commit.result.value, (old_scope, old_node));
    assert_ne!(commit.result.value.0, old_scope);
    assert_ne!(commit.result.value.1, old_node);
    assert_eq!(next.hit_test(3.0, 1.0), Some(child));
    assert_eq!(old.hit_test(3.0, 1.0), Some(child));
}

#[test]
fn final_constraint_observation_failure_keeps_old_local_snapshot() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let host = ui
        .append_render_layout(scope, ui.root_render(), boundary.clone(), 0)
        .unwrap();
    let (_, first) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_constraint_builder(host, |_, edit| {
                    edit.append_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    let result = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.inline_constraint_builder(host, |input, _| {
                assert_eq!(input.max, 100.0);
                Ok(())
            })?;
            edit.properties().layout_style(
                host,
                boundary.size(LayoutDimension::Px(60.0), LayoutDimension::Auto),
            );
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert!(Arc::ptr_eq(
        &first.0,
        &ui.published_local_frame().unwrap().0
    ));
    assert_eq!(ui.layout_result(host).unwrap().size, [100.0, 20.0]);
}

#[test]
fn resolved_and_local_atomic_frames_reject_clipped_overflow_consistently() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let node = ui
            .append_render_layout(scope, root, fixed(10.0, 10.0), 0xff0000ff)
            .unwrap();
        let old = ui
            .local_frame_in_viewport(constraints(100.0), [180.0; 2])
            .unwrap();
        let render = |(): (), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
            edit.properties().translation(node, [f32::MAX, 0.0]);
            edit.properties().clip(
                node,
                Some(Rect {
                    x: f32::MAX,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }),
            );
            Ok(())
        };
        let error = if local {
            ui.edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), render)
                .err()
        } else {
            ui.edit_frame(constraints(100.0), [180.0; 2], |_| Ok(()), render)
                .err()
        };
        assert_eq!(error, Some(Error::InvalidFrame));
        assert!(Arc::ptr_eq(&old.0, &ui.published_local_frame().unwrap().0));
        assert_eq!(old.hit_test(1.0, 1.0), Some(node));
    }
}

#[derive(Debug)]
struct Parent {
    reject: bool,
}
impl ParentLayout for Parent {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        if self.reject {
            return Err(Error::InvalidParentLayout);
        }
        let width = input.known_size.width.unwrap_or(80.0);
        let slot = ChildLayoutInput {
            constraints: constraints(width)
                .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
            percentage_basis: input.percentage_basis,
            ..ChildLayoutInput::default()
        };
        let measured = children.measure(0, slot)?;
        Ok(ParentLayoutOutput {
            size: [width, input.known_size.height.unwrap_or(measured.size[1])],
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

#[test]
fn full_constraint_builder_and_custom_parent_use_atomic_local_layout_and_error_boundary() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Tight)
                .container_type(LayoutContainerType::Size)
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(60.0)),
            0,
        )
        .unwrap();
    let (commit, first) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.constraint_builder(host, |input, edit| {
                    assert_eq!(
                        input.constraints.bounds,
                        BoxConstraints::tight([100.0, 60.0])
                    );
                    let parent = edit.append_layout(scope, host, column(), 0)?;
                    let leaf = edit.append_layout(scope, parent, fixed(20.0, 20.0), 0xff0000ff)?;
                    let value = edit.state(scope, 5, || {
                        Some(ParentLayoutSnapshot::new(Parent { reject: false }))
                    })?;
                    edit.bind(value, Property::parent_layout(parent))?;
                    Ok((value, parent, leaf))
                })
            },
        )
        .unwrap();
    let (source, parent, leaf) = commit.result.value;
    assert_eq!(ui.layout_result(parent).unwrap().size, [100.0, 60.0]);
    assert_eq!(ui.layout_result(leaf).unwrap().size, [100.0, 20.0]);
    assert_eq!(first.hit_test(1.0, 1.0), Some(leaf));
    let saved = ui.read(source).unwrap().clone();
    let rejected = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |event| {
            event.set(
                source,
                Some(ParentLayoutSnapshot::new(Parent { reject: true })),
            )?;
            Ok(())
        },
        |(), _| Ok(()),
    );
    assert!(matches!(rejected, Err(Error::InvalidParentLayout)));
    assert_eq!(*ui.read(source).unwrap(), saved);
    assert!(Arc::ptr_eq(
        &first.0,
        &ui.published_local_frame().unwrap().0
    ));
    assert_eq!(ui.layout_result(leaf).unwrap().size, [100.0, 20.0]);
}

#[test]
fn sparse_local_image_opacity_clip_and_scroll_match_fresh_full_output() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host_style = fixed(80.0, 35.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll);
    let host = ui
        .append_render_layout(scope, root, host_style.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(40.0, 80.0), 0)
        .unwrap();
    ui.edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let image = ImageResource::rgba8([1, 1], vec![255, 0, 0, 255]).unwrap();
    // Adding an image installs its intrinsic input, so first publication lays out.
    let (_, initial) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.properties().image(leaf, Some(image));
                Ok(())
            },
        )
        .unwrap();
    let mut before = initial;
    for step in 1..4 {
        let image = ImageResource::rgba8([1, 1], vec![0, step * 50, 255, 255]).unwrap();
        let clip = Rect {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 35.0,
        };
        let (_, next) = ui
            .edit_local_frame(
                constraints(100.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().image(leaf, Some(image.clone()));
                    edit.properties().opacity(host, 0.5);
                    edit.properties().clip(host, Some(clip));
                    edit.properties().scroll(host, [0.0, 10.0 * step as f32]);
                    Ok(())
                },
            )
            .unwrap();
        assert!(next.follows(&before));
        assert!(!next.stats().full_publication);
        assert!(next.0.resolved.get().is_none());
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(full.root(), host_style.clone(), 0)
            .unwrap();
        let l = full.append_layout(h, fixed(40.0, 80.0), 0).unwrap();
        let mut tx = Transaction::default();
        tx.image(l, Some(image));
        tx.opacity(h, 0.5);
        tx.clip(h, Some(clip));
        tx.scroll(h, [0.0, 10.0 * step as f32]);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            pixels(next.resolve()),
            pixels(
                full.frame_in_viewport(constraints(100.0), [180.0; 2])
                    .unwrap()
            )
        );
        before = next;
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_atomic_local_frames_match_full_pixels_after_sparse_updates_and_skipped_frame() {
    use crate::retained::wgpu::WgpuFrameRenderer;
    let mut ui = LogicalUi::new_layout(
        column(),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, fixed(80.0, 40.0), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let mut renderer = WgpuFrameRenderer::new(2).unwrap();
    eprintln!("atomic local adapter: {:?}", renderer.adapter_info());
    let (_, first) = ui
        .edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    renderer
        .submit_local(&first, [180, 180], 1.0)
        .unwrap()
        .unwrap();
    for step in 1..6 {
        let color = 0x0000ffff + (step as u32) * 0x20000000;
        let (_, frame) = ui
            .edit_local_frame(
                constraints(100.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.properties()
                        .translation(host, [step as f32 * 3.0, 0.0]);
                    edit.properties().color(leaf, color);
                    Ok(())
                },
            )
            .unwrap();
        assert!(frame.0.resolved.get().is_none());
        if step == 2 {
            continue;
        }
        let result = renderer
            .submit_local(&frame, [180, 180], 1.0)
            .unwrap()
            .unwrap();
        if step == 3 {
            assert!(
                frame.0.resolved.get().is_some(),
                "skipped predecessor requires complete resync"
            );
        } else {
            assert!(
                frame.0.resolved.get().is_none(),
                "consecutive supported patches preserve local output"
            );
        }
        assert_eq!(result.full_resync, step == 3);
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(full.root(), fixed(80.0, 40.0), 0)
            .unwrap();
        full.append_layout(h, fixed(20.0, 20.0), color).unwrap();
        let mut tx = Transaction::default();
        tx.translation(h, [step as f32 * 3.0, 0.0]);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full
            .frame_in_viewport(constraints(100.0), [180.0; 2])
            .unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
        assert_eq!(frame.hit_test(step as f32 * 3.0 + 1.0, 1.0), Some(leaf));
    }
    renderer.finish().unwrap();
}

#[test]
fn atomic_local_resize_hidden_and_invalid_environment_preserve_consumer_revisions() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui.append_render_layout(scope, root, column(), 0).unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let (_, first) = ui
        .edit_local_frame(constraints(100.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let (_, clipped) = ui
        .edit_local_frame(
            constraints(100.0),
            [10.0, 180.0],
            |_| Ok(()),
            |(), _| Ok(()),
        )
        .unwrap();
    assert!(clipped.stats().full_publication);
    assert_eq!(clipped.hit_test(15.0, 1.0), None);
    assert_eq!(first.hit_test(15.0, 1.0), Some(leaf));
    assert_eq!(
        ui.reactive().tree().frame.as_ref().unwrap().layout_runs,
        0,
        "paint viewport change does not alter layout constraints"
    );
    let invalid = ui.edit_local_frame(
        constraints(100.0),
        [f32::NAN, 180.0],
        |_| Ok(()),
        |(), _| Ok(()),
    );
    assert!(matches!(invalid, Err(Error::InvalidViewport)));
    assert!(Arc::ptr_eq(
        &clipped.0,
        &ui.published_local_frame().unwrap().0
    ));
    let (_, hidden) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.properties().layout_style(
                    host,
                    LayoutStyle {
                        display: LayoutDisplay::None,
                        ..column()
                    },
                );
                Ok(())
            },
        )
        .unwrap();
    assert!(hidden.resolve_geometry(leaf).is_none());
    let (_, shown) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.properties().layout_style(host, column());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(pixels(shown.resolve()), pixels(first.resolve()));
    assert_eq!(ui.published_frame().unwrap().revision, shown.revision());
}
