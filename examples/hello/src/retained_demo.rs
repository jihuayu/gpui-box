//! Normal GPUI event → retained transaction → persistent layout → paint adapter.
use gpui::*;
use gpui_box::retained::{
    BackendCapabilities, CustomMeasureSnapshot, ImageResource, MeasureSnapshot, QuadDecoration,
    Rect, Style, Transaction, UiTree, length,
    logical::LogicalUi,
    reactive::{Property, Signal},
    responsive::{BreakpointStyles, FlexStyle, LayoutEnvironment, ResponsiveBindings, StyleQuery},
    taffy,
};
use std::{cell::RefCell, rc::Rc};

pub struct RetainedDemo {
    tree: Rc<RefCell<LogicalUi>>,
    responsive: Rc<RefCell<ResponsiveBindings>>,
    color: Signal<u32>,
    scroll: Signal<[f32; 2]>,
    measure: Signal<Option<MeasureSnapshot>>,
    decoration: Signal<QuadDecoration>,
    image: Signal<Option<ImageResource>>,
    image_options: [ImageResource; 2],
    image_cache: Rc<RefCell<std::collections::BTreeMap<u64, std::sync::Arc<RenderImage>>>>,
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
    #[cfg(feature = "retained-wgpu")]
    frame_queue: std::sync::Arc<gpui_box::retained::frame_queue::FrameQueue>,
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
        let decoration = tree.state(root_scope, 5, QuadDecoration::default).unwrap();
        tree.bind(decoration, Property::decoration(tile))
            .expect("bind decoration");
        tree.bind(scroll, Property::scroll(viewport))
            .expect("bind scroll");
        let measure = tree
            .state(root_scope, 3, || {
                Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                    width: 24.0,
                    height: 12.0,
                    baseline: None,
                }))
            })
            .unwrap();
        let measured = tree
            .append_render(
                root_scope,
                tree.root_render(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(128.0),
                        top: length(4.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    ..Default::default()
                },
                0xdd66aaff,
            )
            .expect("create measured leaf");
        tree.bind(measure, Property::measure(measured))
            .expect("bind owned measurement");
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
        let responsive_node = tree
            .append_render(root_scope, tree.root_render(), Style::default(), 0)
            .unwrap();
        let responsive_style = tree.state(root_scope, 4, Style::default).unwrap();
        let mut compact = FlexStyle::column().into_style();
        compact.position = taffy::Position::Absolute;
        compact.inset.left = length(184.0);
        compact.inset.top = length(4.0);
        let mut wide = compact.clone();
        wide.flex_direction = taffy::FlexDirection::Row;
        let rules = BreakpointStyles::new(
            FlexStyle::try_from_style(compact).unwrap(),
            vec![(600.0, FlexStyle::try_from_style(wide).unwrap())],
        )
        .unwrap();
        let mut responsive = ResponsiveBindings::new();
        responsive
            .bind(
                &mut tree,
                responsive_style,
                responsive_node,
                StyleQuery::ViewportInline,
                rules,
            )
            .unwrap();
        for color in [0xddaa44ff, 0x44aaddff] {
            tree.append_render(
                root_scope,
                responsive_node,
                Style {
                    size: taffy::Size {
                        width: length(12.0),
                        height: length(12.0),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                color,
            )
            .unwrap();
        }
        let image_options = [
            ImageResource::rgba8(
                [2, 2],
                vec![
                    255, 64, 0, 255, 0, 180, 255, 255, 0, 220, 90, 128, 255, 255, 255, 0,
                ],
            )
            .unwrap(),
            ImageResource::rgba8(
                [2, 2],
                vec![
                    0, 180, 255, 255, 255, 64, 0, 255, 255, 255, 255, 0, 0, 220, 90, 128,
                ],
            )
            .unwrap(),
        ];
        let image = tree
            .state(root_scope, 6, || Some(image_options[0].clone()))
            .unwrap();
        let image_node = tree
            .append_render(
                root_scope,
                tree.root_render(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(224.0),
                        top: length(4.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: taffy::Size {
                        width: length(24.0),
                        height: length(24.0),
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        tree.bind(image, Property::image(image_node)).unwrap();
        Self {
            tree: Rc::new(RefCell::new(tree)),
            responsive: Rc::new(RefCell::new(responsive)),
            color,
            scroll,
            measure,
            decoration,
            image,
            image_options,
            image_cache: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
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
            #[cfg(feature = "retained-wgpu")]
            frame_queue: std::sync::Arc::new(gpui_box::retained::frame_queue::FrameQueue::new()),
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
        let image_cache = self.image_cache.clone();
        let responsive = self.responsive.clone();
        #[cfg(feature = "retained-wgpu")]
        let gpu = self.gpu.clone();
        #[cfg(feature = "retained-wgpu")]
        let presented_image = self.presented_image.clone();
        #[cfg(feature = "retained-wgpu")]
        let frame_queue = self.frame_queue.clone();
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
            .child(div()
                .id("retained-image")
                .debug_selector(|| "retained-image".into())
                .cursor_pointer()
                .child("Replace retained image pixels")
                .on_click(cx.listener(|this,_,_,cx| {
                    let signal = this.image;
                    let options = this.image_options.clone();
                    let result = this.tree.borrow_mut().transaction(|tx| {
                        let next = if tx.read(signal)?.as_ref() == Some(&options[0]) { &options[1] } else { &options[0] };
                        tx.set(signal,Some(next.clone()))?;
                        Ok(())
                    }).expect("replace image resource");
                    if result.commit.request_frame {cx.notify();}
                })))
            .child(div()
                .id("retained-decoration")
                .debug_selector(|| "retained-decoration".into())
                .cursor_pointer()
                .child("Toggle rounded border")
                .on_click(cx.listener(|this,_,_,cx| {
                    let decoration = this.decoration;
                    let result = this.tree.borrow_mut().transaction(|tx| {
                        let next = if tx.read(decoration)?.radius == 0.0 {
                            QuadDecoration {radius:12.0,border_width:3.0,border_color:0xeeaa44ff}
                        } else {QuadDecoration::default()};
                        tx.set(decoration,next)?; Ok(())
                    }).expect("commit rounded border");
                    if result.commit.request_frame {cx.notify();}
                })))
            .child(
                div()
                    .id("retained-measure")
                    .debug_selector(|| "retained-measure".into())
                    .cursor_pointer()
                    .child("Update owned intrinsic measurement")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let measure = this.measure;
                        let result = this.tree.borrow_mut().transaction(|tx| {
                            let large = matches!(tx.read(measure)?, Some(MeasureSnapshot::Fixed(value)) if value.width == 24.0);
                            tx.set(measure, Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                                width: if large { 48.0 } else { 24.0 },
                                height: if large { 20.0 } else { 12.0 },
                                baseline: None,
                            })))?;
                            Ok(())
                        }).expect("commit intrinsic input");
                        if result.commit.request_frame { cx.notify(); }
                    })),
            )
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
                        let mut environment = LayoutEnvironment::new([f32::from(bounds.size.width), f32::from(bounds.size.height)]);
                        environment.device_scale = _window.scale_factor();
                        let frame = responsive.borrow_mut().frame(&mut tree.borrow_mut(), &environment)
                            .expect("resolve responsive retained frame").frame;
                        #[cfg(feature = "retained-wgpu")]
                        if let Some(gpu) = &gpu {
                            let scale = _window.scale_factor();
                            let surface = [
                                (f32::from(bounds.size.width) * scale).ceil().max(1.0) as u32,
                                (f32::from(bounds.size.height) * scale).ceil().max(1.0) as u32,
                            ];
                            frame_queue.publish(frame.clone()).expect("publish visual packet");
                            if let Some(packet) = frame_queue.take_latest().expect("consume visual packet") {
                                gpu.borrow_mut().submit(&packet, surface, scale).expect("submit UI frame to GPU");
                            } else {
                                // Scale/surface demand can change without a new
                                // logical revision; same-packet submit handles it.
                                gpu.borrow_mut().submit(&frame, surface, scale).expect("refresh surface geometry");
                            }
                        }
                        #[cfg(feature = "retained-wgpu")]
                        let raster = gpu
                            .as_ref()
                            .filter(|_| !frame.properties.effects.is_empty() || frame.items.iter().any(|item| item.image.is_some()))
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
                        let mut cache = image_cache.borrow_mut();
                        let live:std::collections::BTreeSet<_> = frame.items.iter().filter_map(|item| item.image.as_ref().map(ImageResource::id)).collect();
                        let retired:Vec<_> = cache.keys().filter(|id| !live.contains(id)).copied().collect();
                        for id in retired {
                            if let Some(image) = cache.remove(&id) {
                                _window.drop_image(image).expect("retire decoded image sprite");
                            }
                        }
                        for resource in frame.items.iter().filter_map(|item| item.image.as_ref()) {
                            cache.entry(resource.id()).or_insert_with(|| {
                                let mut bgra = resource.pixels().to_vec();
                                for pixel in bgra.as_chunks_mut::<4>().0 {pixel.swap(0,2);}
                                let [width,height] = resource.size();
                                let image = image::RgbaImage::from_raw(width,height,bgra).expect("validated pixels");
                                std::sync::Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
                            });
                        }
                        (frame, raster, cache.clone())
                    },
                    |bounds, (frame, raster, images), window, _| {
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
                                    let (color,decoration) = item.resolved_quad(command);
                                    let decoration = decoration.resolved(item.bounds);
                                    window.paint_quad(quad(
                                            Bounds {
                                                origin: bounds.origin
                                                    + point(px(item.bounds.x), px(item.bounds.y)),
                                                size: size(
                                                    px(item.bounds.width),
                                                    px(item.bounds.height),
                                                ),
                                            },
                                            px(decoration.radius),
                                            rgba(color),
                                            px(decoration.border_width),
                                            rgba(decoration.border_color),
                                            BorderStyle::default(),
                                        ));
                                }
                                if let Some(resource) = &item.image {
                                    let r = item.content_bounds;
                                    let image_bounds = Bounds {origin:bounds.origin+point(px(r.x),px(r.y)),size:size(px(r.width),px(r.height))};
                                    window.paint_image(image_bounds,image_bounds,Corners::default(),images[&resource.id()].clone(),0,false).expect("paint retained decoded image");
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
        assert_ne!(before.items[1].fill_color, after.items[1].fill_color);
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
            (3, 2, 2, 2)
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
        assert_eq!((stats.transactions, stats.frame_requests), (5, 4));
        let old_image = view.read_with(cx, |view, _| {
            let resource = view
                .tree
                .borrow()
                .read(view.image)
                .unwrap()
                .as_ref()
                .unwrap()
                .id();
            view.image_cache.borrow()[&resource].clone()
        });
        assert!(cx.update(|window, _| window.has_image_atlas_entry(&old_image)));
        let button = cx.debug_bounds("retained-image").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let image_frame = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!(
            (image_frame.layout_runs, image_frame.recorded_chunks),
            (0, 0)
        );
        assert!(
            !cx.update(|window, _| window.has_image_atlas_entry(&old_image)),
            "replacement retires legacy atlas entry"
        );
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
        let measured = cx.debug_bounds("retained-measure").unwrap();
        cx.simulate_click(measured.center(), Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let tree = view.tree.borrow();
            let frame = tree.published_frame().unwrap();
            assert_eq!((frame.layout_runs, frame.recorded_chunks), (1, 0));
            assert!(frame.layout_stats.measure_calls > 0);
            assert!(
                frame
                    .items
                    .iter()
                    .any(|item| item.bounds.width == 48.0 && item.bounds.height == 20.0)
            );
        });
        for (width, direction) in [
            (500.0, taffy::FlexDirection::Column),
            (800.0, taffy::FlexDirection::Row),
        ] {
            cx.simulate_resize(size(px(width), px(600.0)));
            cx.run_until_parked();
            view.read_with(cx, |view, _| {
                let tree = view.tree.borrow();
                let source = tree.reactive().tree();
                assert!(tree.published_frame().unwrap().items.iter().any(|item| {
                    source.style(item.node).unwrap().position == taffy::Position::Absolute
                        && source.style(item.node).unwrap().inset.left == length(184.0)
                        && source.style(item.node).unwrap().flex_direction == direction
                }));
            });
        }
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
        let measure = cx.debug_bounds("retained-measure").unwrap();
        cx.simulate_click(measure.center(), Modifiers::default());
        cx.run_until_parked();
        let measured = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        let actual = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&measured, [actual.width(), actual.height()], scale)
            .unwrap();
        assert_eq!((measured.layout_runs, measured.recorded_chunks), (1, 0));
        assert!(
            actual.as_raw() == reference.as_raw(),
            "normal measure click reaches GPU"
        );
        for width in [500.0, 800.0] {
            cx.simulate_resize(size(px(width), px(600.0)));
            cx.run_until_parked();
            let frame = view.read_with(cx, |view, _| {
                view.tree.borrow().published_frame().unwrap().clone()
            });
            let actual = gpu.borrow_mut().image().unwrap();
            let reference = gpu
                .borrow_mut()
                .reference_image(&frame, [actual.width(), actual.height()], scale)
                .unwrap();
            assert!(
                actual.as_raw() == reference.as_raw(),
                "responsive window resize reaches GPU"
            );
        }
        let decoration = cx.debug_bounds("retained-decoration").unwrap();
        cx.simulate_click(decoration.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 1));
        let actual = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [actual.width(), actual.height()], scale)
            .unwrap();
        assert!(
            actual.as_raw() == reference.as_raw(),
            "rounded border click reaches GPU"
        );
        let old_resource = view.read_with(cx, |view, _| {
            view.tree
                .borrow()
                .read(view.image)
                .unwrap()
                .as_ref()
                .unwrap()
                .id()
        });
        let button = cx.debug_bounds("retained-image").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            assert_ne!(
                view.tree
                    .borrow()
                    .read(view.image)
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .id(),
                old_resource
            );
            assert!(!view.image_cache.borrow().contains_key(&old_resource));
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
        let replaced = gpu.borrow_mut().image().unwrap();
        assert_ne!(
            actual.as_raw(),
            replaced.as_raw(),
            "image click changes real GPU pixels"
        );
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [replaced.width(), replaced.height()], scale)
            .unwrap();
        assert_eq!(replaced.as_raw(), reference.as_raw());
    }
}
