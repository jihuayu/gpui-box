//! Normal GPUI event → retained transaction → persistent layout → paint adapter.
use gpui::*;
use gpui_box::retained::{
    BackendCapabilities, DisplayCommand, Rect, Style, Transaction, UiTree, length,
    logical::LogicalUi,
    reactive::{Property, Signal},
    taffy,
};
use std::{cell::RefCell, rc::Rc};

pub struct RetainedDemo {
    tree: Rc<RefCell<LogicalUi>>,
    color: Signal<u32>,
    scroll: Signal<[f32; 2]>,
    last_commit: String,
    reversed: bool,
    #[cfg(test)]
    render_count: usize,
    #[cfg(feature = "retained-wgpu")]
    gpu: Option<Rc<RefCell<gpui_box::retained::wgpu::WgpuFrameRenderer>>>,
    #[cfg(feature = "retained-wgpu")]
    opacity: Signal<f32>,
    #[cfg(feature = "retained-wgpu")]
    presented_image: Rc<RefCell<Option<std::sync::Arc<RenderImage>>>>,
}

impl RetainedDemo {
    pub fn new(_: &mut Context<Self>) -> Self {
        let mut tree = UiTree::new(Style::default(), 0x202b3aff).expect("create retained root");
        let tile = tree
            .append(tree.root(), Style::default(), 0x245fc7ff)
            .expect("create tile");
        // Taffy layout contract is explicit. Geometry is filled by Flex children.
        let mut root = tree.style(tree.root()).unwrap().clone();
        root.size.width = gpui_box::retained::percent(1.0);
        root.size.height = gpui_box::retained::percent(1.0);
        let mut style = tree.style(tile).unwrap().clone();
        style.flex_grow = 1.0;
        style.size.height = gpui_box::retained::percent(1.0);
        let mut transaction = Transaction::default();
        transaction.style(tree.root(), root);
        transaction.style(tile, style);
        let viewport = tree
            .append(
                tree.root(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(16.0),
                        top: length(4.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: taffy::Size {
                        width: length(96.0),
                        height: length(24.0),
                    },
                    ..Default::default()
                },
                0x334455ff,
            )
            .expect("create scroll viewport");
        for color in [0xeeaa4480, 0x4488ff80] {
            tree.append(
                viewport,
                Style {
                    size: taffy::Size {
                        width: length(64.0),
                        height: length(24.0),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                color,
            )
            .expect("create scroll content");
        }
        transaction.clip(
            viewport,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 24.0,
            }),
        );
        tree.commit(transaction, BackendCapabilities::default())
            .unwrap();
        let mut tree =
            LogicalUi::from_tree(tree, BackendCapabilities::default()).expect("create logical UI");
        let root_scope = tree.root_scope();
        let color = tree.state(root_scope, 0, || 0x245fc7ff).unwrap();
        let scroll = tree.state(root_scope, 1, || [0.0; 2]).unwrap();
        tree.bind(color, Property::color(tile)).expect("bind color");
        tree.bind(scroll, Property::scroll(viewport))
            .expect("bind scroll");
        #[cfg(feature = "retained-wgpu")]
        let opacity = {
            let opacity = tree.state(root_scope, 2, || 1.0).unwrap();
            tree.bind(opacity, Property::opacity(viewport))
                .expect("bind opacity");
            opacity
        };
        let keyed = tree
            .reconcile_keys(root_scope, &[1, 2, 3])
            .expect("create keyed scopes");
        for scope in keyed.order {
            let key = tree.key(scope).unwrap();
            let color = tree
                .state(scope, 0, || match key {
                    1 => 0xeeaa44ff,
                    2 => 0x44cc88ff,
                    _ => 0x4488ffff,
                })
                .unwrap();
            let initial = *tree.read(color).unwrap();
            let node = tree
                .append_render(
                    scope,
                    tree.root_render(),
                    Style {
                        size: taffy::Size {
                            width: length(24.0),
                            height: length(24.0),
                        },
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                    initial,
                )
                .expect("create keyed render output");
            tree.bind(color, Property::color(node)).unwrap();
        }
        Self {
            tree: Rc::new(RefCell::new(tree)),
            color,
            scroll,
            last_commit: "Ready".into(),
            reversed: false,
            #[cfg(test)]
            render_count: 0,
            #[cfg(feature = "retained-wgpu")]
            gpu: None,
            #[cfg(feature = "retained-wgpu")]
            opacity,
            #[cfg(feature = "retained-wgpu")]
            presented_image: Rc::new(RefCell::new(None)),
        }
    }

    #[cfg(feature = "retained-wgpu")]
    pub fn with_gpu(cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(cx);
        view.tree
            .borrow_mut()
            .set_capabilities(BackendCapabilities {
                solid_color_patch: true,
            });
        view.gpu = Some(Rc::new(RefCell::new(
            gpui_box::retained::wgpu::WgpuFrameRenderer::new(3).expect("create GPU frame consumer"),
        )));
        view
    }
}

impl Render for RetainedDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        let tree = self.tree.clone();
        #[cfg(feature = "retained-wgpu")]
        let gpu = self.gpu.clone();
        #[cfg(feature = "retained-wgpu")]
        let presented_image = self.presented_image.clone();
        #[cfg(feature = "retained-wgpu")]
        let effect_button = self.gpu.as_ref().map(|_| {
            div()
                .id("retained-opacity")
                .debug_selector(|| "retained-opacity".into())
                .cursor_pointer()
                .child("Toggle isolated group opacity")
                .on_click(cx.listener(|this, _, _, cx| {
                    let opacity = this.opacity;
                    let commit = this
                        .tree
                        .borrow_mut()
                        .transaction(|tx| {
                            let next = if *tx.read(opacity)? == 1.0 { 0.5 } else { 1.0 };
                            tx.set(opacity, next)?;
                            Ok(())
                        })
                        .expect("commit group opacity");
                    if commit.commit.request_frame {
                        cx.notify();
                    }
                }))
                .into_any_element()
        });
        #[cfg(not(feature = "retained-wgpu"))]
        let effect_button: Option<AnyElement> = None;
        div()
            .size_full()
            .p_6()
            .bg(rgb(0x171d28))
            .text_color(rgb(0xe8edf5))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("retained-change")
                    .debug_selector(|| "retained-change".into())
                    .cursor_pointer()
                    .child("Retained UI · Change color")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let color = this.color;
                        let commit = this
                            .tree
                            .borrow_mut()
                            .transaction(|tx| {
                                let next = if *tx.read(color)? == 0x245fc7ff {
                                    0xc75824ff
                                } else {
                                    0x245fc7ff
                                };
                                tx.set(color, next)?;
                                Ok(())
                            })
                            .expect("commit color");
                        this.last_commit = format!(
                            "Revision {} · {} changed · {:?}",
                            commit.commit.revision,
                            commit.commit.changed_nodes,
                            commit.commit.routes
                        );
                        if commit.commit.request_frame {
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .id("retained-equal")
                    .debug_selector(|| "retained-equal".into())
                    .cursor_pointer()
                    .child("Write temporary color then restore in one transaction")
                    .on_click(cx.listener(|this, _, _, _| {
                        let color = this.color;
                        let commit = this
                            .tree
                            .borrow_mut()
                            .transaction(|tx| {
                                let current = *tx.read(color)?;
                                tx.set(color, 0xffffffff)?;
                                tx.transaction(|tx| {
                                    tx.set(color, current)?;
                                    Ok(())
                                })
                            })
                            .expect("commit equal transaction");
                        debug_assert!(!commit.commit.request_frame);
                    })),
            )
            .child(
                div()
                    .id("retained-scroll")
                    .debug_selector(|| "retained-scroll".into())
                    .cursor_pointer()
                    .child("Scroll clipped content")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let scroll = this.scroll;
                        let commit = this
                            .tree
                            .borrow_mut()
                            .transaction(|tx| {
                                let next = if tx.read(scroll)?[0] == 0.0 {
                                    24.0
                                } else {
                                    0.0
                                };
                                tx.set(scroll, [next, 0.0])?;
                                Ok(())
                            })
                            .expect("commit scroll");
                        if commit.commit.request_frame {
                            cx.notify();
                        }
                    })),
            )
            .children(effect_button)
            .child(
                div()
                    .id("retained-keyed")
                    .debug_selector(|| "retained-keyed".into())
                    .cursor_pointer()
                    .child("Reverse keyed components")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reversed = !this.reversed;
                        let mut tree = this.tree.borrow_mut();
                        let root = tree.root_scope();
                        let keys = if this.reversed { [3, 2, 1] } else { [1, 2, 3] };
                        let change = tree
                            .reconcile_keys(root, &keys)
                            .expect("reorder keyed components");
                        if change.request_frame {
                            cx.notify();
                        }
                    })),
            )
            .child(self.last_commit.clone())
            .child(
                canvas(
                    move |bounds, _window, _| {
                        let frame = tree
                            .borrow_mut()
                            .frame([f32::from(bounds.size.width), f32::from(bounds.size.height)])
                            .expect("prepare retained frame");
                        #[cfg(feature = "retained-wgpu")]
                        if let Some(gpu) = &gpu {
                            let scale = _window.scale_factor();
                            let surface = [
                                (f32::from(bounds.size.width) * scale).ceil().max(1.0) as u32,
                                (f32::from(bounds.size.height) * scale).ceil().max(1.0) as u32,
                            ];
                            gpu.borrow_mut()
                                .submit(&frame, surface, scale)
                                .expect("submit UI frame to GPU");
                        }
                        #[cfg(feature = "retained-wgpu")]
                        let raster = gpu
                            .as_ref()
                            .filter(|_| !frame.properties.effects.is_empty())
                            .map(|gpu| {
                                gpu.borrow_mut()
                                    .window_image()
                                    .expect("read isolated groups for window paint")
                            });
                        #[cfg(feature = "retained-wgpu")]
                        if let Some(previous) = presented_image.replace(raster.clone())
                            && raster.as_ref().is_none_or(|image| image.id != previous.id)
                        {
                            _window
                                .drop_image(previous)
                                .expect("retire previous readback image");
                        }
                        #[cfg(not(feature = "retained-wgpu"))]
                        let raster: Option<std::sync::Arc<RenderImage>> = None;
                        (frame, raster)
                    },
                    |bounds, (frame, raster), window, _| {
                        if let Some(raster) = raster {
                            window
                                .paint_image(bounds, bounds, Corners::default(), raster, 0, false)
                                .expect("paint isolated groups");
                            return;
                        }
                        for item in frame.items.iter() {
                            let mask = item.clip.map(|clip| ContentMask {
                                bounds: Bounds {
                                    origin: bounds.origin + point(px(clip.x), px(clip.y)),
                                    size: size(px(clip.width), px(clip.height)),
                                },
                            });
                            window.with_content_mask(mask, |window| {
                                for command in item.commands.iter() {
                                    match command {
                                        DisplayCommand::Fill(color) => window.paint_quad(fill(
                                            Bounds {
                                                origin: bounds.origin
                                                    + point(px(item.bounds.x), px(item.bounds.y)),
                                                size: size(
                                                    px(item.bounds.width),
                                                    px(item.bounds.height),
                                                ),
                                            },
                                            rgba(*color),
                                        )),
                                    }
                                }
                            });
                        }
                    },
                )
                .w_full()
                .h_8(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn normal_click_updates_retained_frame_and_equal_transaction_keeps_revision(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, cx| RetainedDemo::new(cx));
        let before = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        let button = cx
            .debug_bounds("retained-change")
            .expect("color button rendered");
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let after = view.read_with(cx, |view, _| {
            assert_eq!(*view.tree.borrow().read(view.color).unwrap(), 0xc75824ff);
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_ne!(before.items[1].commands[0], after.items[1].commands[0]);
        let equal = cx
            .debug_bounds("retained-equal")
            .expect("equality button rendered");
        cx.simulate_click(equal.center(), Modifiers::default());
        cx.run_until_parked();
        let repeated = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!(after.revision, repeated.revision);
        assert!(std::sync::Arc::ptr_eq(&after.items, &repeated.items));
        let stats = view.read_with(cx, |view, _| view.tree.borrow().stats());
        assert_eq!(
            (
                stats.transactions,
                stats.changed_sources,
                stats.sink_writes,
                stats.frame_requests
            ),
            (2, 1, 1, 1)
        );
        let scroll = cx.debug_bounds("retained-scroll").unwrap();
        cx.simulate_click(scroll.center(), Modifiers::default());
        cx.run_until_parked();
        let scrolled = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((scrolled.layout_runs, scrolled.recorded_chunks), (0, 0));
        assert_ne!(after.items[3].bounds, scrolled.items[3].bounds);
        assert!(std::sync::Arc::ptr_eq(
            &after.items[3].commands,
            &scrolled.items[3].commands
        ));
        let renders = view.read_with(cx, |view, _| view.render_count);
        for _ in 0..16 {
            cx.run_until_parked();
        }
        assert_eq!(
            view.read_with(cx, |view, _| view.render_count),
            renders,
            "idle event loop must not schedule reactive rendering"
        );
        let before_batch = scrolled.revision;
        view.update(cx, |view, cx| {
            let result = view
                .tree
                .borrow_mut()
                .transaction(|tx| {
                    tx.set(view.color, 0x66aa88ff)?;
                    tx.transaction(|tx| {
                        tx.set(view.scroll, [0.0, 0.0])?;
                        Ok(())
                    })
                })
                .unwrap();
            assert_eq!(result.commit.changed_nodes, 2);
            if result.commit.request_frame {
                cx.notify();
            }
        });
        cx.run_until_parked();
        let (revision, count, stats) = view.read_with(cx, |view, _| {
            (
                view.tree.borrow().published_frame().unwrap().revision,
                view.render_count,
                view.tree.borrow().stats(),
            )
        });
        assert_eq!(revision, before_batch + 1);
        assert_eq!(
            count,
            renders + 1,
            "outer transaction requests one view render"
        );
        assert_eq!((stats.transactions, stats.frame_requests), (4, 3));
        let (scope_ids, node_ids) = view.read_with(cx, |view, _| {
            let tree = view.tree.borrow();
            let scopes = tree.children(tree.root_scope()).unwrap().to_vec();
            let nodes = scopes
                .iter()
                .map(|scope| tree.renders(*scope).unwrap()[0])
                .collect::<Vec<_>>();
            (scopes, nodes)
        });
        let keyed = cx.debug_bounds("retained-keyed").unwrap();
        cx.simulate_click(keyed.center(), Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let tree = view.tree.borrow();
            assert_eq!(
                tree.children(tree.root_scope()).unwrap(),
                scope_ids.iter().rev().copied().collect::<Vec<_>>()
            );
            let output = tree.published_frame().unwrap();
            assert_eq!(
                output
                    .items
                    .iter()
                    .filter(|item| node_ids.contains(&item.node))
                    .map(|item| item.node)
                    .collect::<Vec<_>>(),
                node_ids.iter().rev().copied().collect::<Vec<_>>()
            );
            assert_eq!(output.recorded_chunks, 0);
        });
    }

    #[cfg(feature = "retained-wgpu")]
    #[gpui::test]
    #[ignore = "requires a hardware WGPU adapter"]
    fn normal_click_reaches_gpu_and_matches_reference_pixels(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| RetainedDemo::with_gpu(cx));
        let gpu = view.read_with(cx, |view, _| view.gpu.clone().unwrap());
        let before = gpu.borrow_mut().image().unwrap();
        let button = cx.debug_bounds("retained-change").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        let after = gpu.borrow_mut().image().unwrap();
        assert!(
            before.as_raw() != after.as_raw(),
            "click must change GPU pixels"
        );
        let scale = cx.update(|window, _| window.scale_factor());
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [after.width(), after.height()], scale)
            .unwrap();
        assert!(
            after.as_raw() == reference.as_raw(),
            "UI frame and full renderer pixels differ"
        );
        gpu.borrow_mut()
            .set_policy(gpui_box::retained::wgpu::CompositorPolicy::Damage)
            .unwrap();
        gpu.borrow_mut()
            .submit(&frame, [after.width(), after.height()], scale)
            .unwrap();
        let scroll = cx.debug_bounds("retained-scroll").unwrap();
        cx.simulate_click(scroll.center(), Modifiers::default());
        cx.run_until_parked();
        let scrolled = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        let image = gpu.borrow_mut().image().unwrap();
        assert!(
            image.as_raw() != after.as_raw(),
            "scroll changes GPU pixels"
        );
        let reference = gpu
            .borrow_mut()
            .reference_image(&scrolled, [image.width(), image.height()], scale)
            .unwrap();
        assert!(
            image.as_raw() == reference.as_raw(),
            "scroll damage matches full renderer"
        );
        let opacity = cx.debug_bounds("retained-opacity").unwrap();
        cx.simulate_click(opacity.center(), Modifiers::default());
        cx.run_until_parked();
        let isolated = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((isolated.layout_runs, isolated.recorded_chunks), (0, 0));
        let image = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&isolated, [image.width(), image.height()], scale)
            .unwrap();
        assert!(
            image.as_raw() == reference.as_raw(),
            "normal UI isolated group pixels"
        );
        let presented = gpu.borrow_mut().window_image().unwrap();
        let bgra = presented.as_bytes(0).unwrap();
        for (actual, expected) in bgra
            .as_chunks::<4>()
            .0
            .iter()
            .zip(image.as_raw().as_chunks::<4>().0)
        {
            assert_eq!(
                *actual,
                [expected[2], expected[1], expected[0], expected[3]]
            );
        }
        assert!(std::sync::Arc::ptr_eq(
            &presented,
            &gpu.borrow_mut().window_image().unwrap()
        ));
        assert!(cx.update(|window, _| window.has_image_atlas_entry(&presented)));
        cx.simulate_click(opacity.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(
            !cx.update(|window, _| window.has_image_atlas_entry(&presented)),
            "retire the readback sprite when isolation is removed"
        );
        let keyed = cx.debug_bounds("retained-keyed").unwrap();
        cx.simulate_click(keyed.center(), Modifiers::default());
        cx.run_until_parked();
        let reordered = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        let actual = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&reordered, [actual.width(), actual.height()], scale)
            .unwrap();
        assert!(
            actual.as_raw() == reference.as_raw(),
            "normal keyed click reaches GPU"
        );
    }
}
