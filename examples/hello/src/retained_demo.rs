//! Normal GPUI event → retained transaction → persistent layout → paint adapter.
use gpui::*;
use gpui_box::retained::{
    BackendCapabilities, BoxShadow as RetainedShadow, CustomMeasureSnapshot, ImageResource, MeasureSnapshot, QuadDecoration,
    Rect, Style, Transaction, UiTree, length,
    logical::LogicalUi,
    reactive::{Property, Signal, UpdateSender},
    responsive::{BreakpointStyles, FlexStyle, LayoutEnvironment, ResponsiveBindings, StyleQuery},
    taffy,
};
use std::{cell::RefCell, rc::Rc};
#[cfg(feature = "retained-wgpu")]
pub(super) mod delivery_check;

enum WindowRaster {
    Vector,
    #[cfg(feature = "retained-wgpu")]
    Image(std::sync::Arc<RenderImage>),
    #[cfg(all(feature = "retained-wgpu", target_os = "windows"))]
    Gpu(WindowsGpuSurface),
}

// The vector bridge keeps its existing stack-owned packet without another heap
// allocation; this enum has only one live instance per canvas prepaint.
#[allow(clippy::large_enum_variant)]
enum SceneFrame {
    Resolved(gpui_box::retained::FramePacket),
    #[cfg(feature = "retained-wgpu")]
    Local(gpui_box::retained::local::LocalFrame),
}
impl SceneFrame {
    fn resolve(&self)->gpui_box::retained::FramePacket {
        match self {
            Self::Resolved(frame) => frame.clone(),
            #[cfg(feature = "retained-wgpu")]
            Self::Local(frame) => frame.resolve(),
        }
    }
}

#[cfg(feature = "retained-wgpu")]
impl WindowRaster {
    fn image(&self) -> Option<std::sync::Arc<RenderImage>> {
        match self { Self::Image(image) => Some(image.clone()), _ => None }
    }
}

pub struct RetainedDemo {
    tree: Rc<RefCell<LogicalUi>>,
    responsive: Rc<RefCell<ResponsiveBindings>>,
    color: Signal<u32>,
    observed_color: Rc<std::cell::Cell<u32>>,
    color_sender: UpdateSender<u32>,
    _updates_task: Task<()>,
    scroll: Signal<[f32; 2]>,
    measure: Signal<Option<MeasureSnapshot>>,
    decoration: Signal<QuadDecoration>,
    shadows: Signal<std::sync::Arc<[RetainedShadow]>>,
    image: Signal<Option<ImageResource>>,
    paragraph_measure: Signal<Option<MeasureSnapshot>>,
    text: Signal<Option<gpui_box::retained::text::TextPaint>>,
    rasterizer: Rc<RefCell<gpui_box::retained::text::GlyphRasterizer>>,
    paragraph_long: bool,
    image_options: [ImageResource; 2],
    image_cache: Rc<RefCell<std::collections::BTreeMap<u64, std::sync::Arc<RenderImage>>>>,
    last_commit: String,
    reversed: bool,
    portal: (gpui_box::retained::NodeId, [gpui_box::retained::NodeId; 2]),
    portal_moved: bool,
    draft_output: Option<gpui_box::retained::NodeId>,
    draft_parent: gpui_box::retained::logical::ScopeId,
    draft_effects: Rc<std::cell::Cell<usize>>,
    draft_cleanups: Rc<std::cell::Cell<usize>>,
    render_count: usize,
    #[cfg(all(feature = "native-check", target_os = "windows"))]
    canvas_bounds: Rc<std::cell::Cell<Option<Bounds<DevicePixels>>>>,
    #[cfg(feature = "retained-wgpu")]
    gpu: Option<Rc<RefCell<gpui_box::retained::wgpu::WgpuFrameRenderer>>>,
    #[cfg(feature = "retained-wgpu")]
    _gpu_completion_task: Option<Task<()>>,
    #[cfg(feature = "retained-wgpu")]
    opacity: Signal<f32>,
    #[cfg(feature = "retained-wgpu")]
    presented_image: Rc<RefCell<Option<std::sync::Arc<RenderImage>>>>,
    #[cfg(feature = "retained-wgpu")]
    frame_queue: std::sync::Arc<gpui_box::retained::frame_queue::FrameQueue>,
}

impl RetainedDemo {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
        // Normal mouse events now travel through a read-only derived source
        // before the typed sink, while preserving the gallery's color contract.
        let material = tree
            .computed(root_scope, 9, &[color.dependency()], move |inputs| {
                Ok(*inputs.read(color)?)
            })
            .expect("create computed material");
        tree.bind(material, Property::color(tile))
            .expect("bind computed color");
        let observed_color = Rc::new(std::cell::Cell::new(0));
        let observed = observed_color.clone();
        tree.effect(root_scope, 0, &[material.dependency()], move |inputs| {
            observed.set(*inputs.read(material).expect("declared material"));
            None
        })
        .expect("observe committed material");
        let decoration = tree.state(root_scope, 5, QuadDecoration::default).unwrap();
        tree.bind(decoration, Property::decoration(tile))
            .expect("bind decoration");
        let shadows = tree.state(root_scope, 15, || std::sync::Arc::<[RetainedShadow]>::from([])).unwrap();
        tree.bind(shadows, Property::shadows(tile)).expect("bind retained shadows");
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
        let portal_scope = keyed.order[0];
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
        let mut hosts = vec![];
        for x in [112.0, 152.0] {
            hosts.push(
                tree.append_render(
                    root_scope,
                    tree.root_render(),
                    Style {
                        position: taffy::Position::Absolute,
                        inset: taffy::Rect {
                            left: length(x),
                            top: length(4.0),
                            right: taffy::LengthPercentageAuto::auto(),
                            bottom: taffy::LengthPercentageAuto::auto(),
                        },
                        size: taffy::Size {
                            width: length(32.0),
                            height: length(24.0),
                        },
                        ..Default::default()
                    },
                    0x223344ff,
                )
                .unwrap(),
            );
        }
        let portal = tree
            .append_render(
                portal_scope,
                hosts[0],
                Style {
                    size: taffy::Size {
                        width: length(16.0),
                        height: length(16.0),
                    },
                    ..Default::default()
                },
                0x245fc7ff,
            )
            .unwrap();
        let portal_color = tree.state(portal_scope, 0, || 0xeeaa44ff).unwrap();
        tree.bind(portal_color, Property::color(portal)).unwrap();
        let portal = (portal, [hosts[0], hosts[1]]);
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
        let paragraph_measure = tree
            .state(root_scope, 7, || None::<MeasureSnapshot>)
            .unwrap();
        let paragraph_node = tree
            .append_render(
                root_scope,
                tree.root_render(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(260.0),
                        top: length(4.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: taffy::Size {
                        width: length(120.0),
                        height: taffy::Dimension::auto(),
                    },
                    ..Default::default()
                },
                0x8877bbff,
            )
            .unwrap();
        tree.bind(paragraph_measure, Property::measure(paragraph_node))
            .unwrap();
        let text = tree
            .state(
                root_scope,
                8,
                || None::<gpui_box::retained::text::TextPaint>,
            )
            .unwrap();
        let text_node = tree
            .append_render(
                root_scope,
                tree.root_render(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(384.0),
                        top: length(0.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: taffy::Size {
                        width: length(220.0),
                        height: taffy::Dimension::auto(),
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        tree.bind(text, Property::text(text_node)).unwrap();
        let mut receiver = tree.open_updates(128).expect("open source delivery");
        let color_sender = tree
            .update_sender(&receiver, color)
            .expect("create color producer");
        let updates_task = cx.spawn(async move |view, cx| {
            while let Ok(true) = receiver.ready().await {
                // Establish a GPUI effect cycle before notifying the window;
                // bare AsyncApp::update_entity does not flush pending effects.
                if cx
                    .update(|cx| {
                        view.update(cx, |view, cx| {
                            match view.tree.borrow_mut().apply_updates(&mut receiver) {
                                Ok(batch) => {
                                    if batch
                                        .commit
                                        .is_some_and(|commit| commit.commit.request_frame)
                                    {
                                        cx.notify();
                                    }
                                }
                                Err(error) => {
                                    view.last_commit =
                                        format!("Background update rejected: {error}")
                                }
                            }
                        })
                    })
                    .is_err()
                {
                    break;
                }
                // A continuously producing worker must not keep this future
                // running without returning control to the host executor.
                let mut yielded = false;
                std::future::poll_fn(|cx| {
                    if std::mem::replace(&mut yielded, true) {
                        std::task::Poll::Ready(())
                    } else {
                        cx.waker().wake_by_ref();
                        std::task::Poll::Pending
                    }
                })
                .await;
            }
        });
        Self {
            tree: Rc::new(RefCell::new(tree)),
            responsive: Rc::new(RefCell::new(responsive)),
            color,
            observed_color,
            color_sender,
            _updates_task: updates_task,
            scroll,
            measure,
            decoration,
            shadows,
            image,
            paragraph_measure,
            text,
            rasterizer: Rc::new(RefCell::new(
                gpui_box::retained::text::GlyphRasterizer::new(cx.text_system().clone()),
            )),
            paragraph_long: false,
            image_options,
            image_cache: Rc::new(RefCell::new(std::collections::BTreeMap::new())),
            last_commit: "Ready".into(),
            reversed: false,
            portal,
            portal_moved: false,
            draft_output: None,
            draft_parent: portal_scope,
            draft_effects: Rc::new(std::cell::Cell::new(0)),
            draft_cleanups: Rc::new(std::cell::Cell::new(0)),
            render_count: 0,
            #[cfg(all(feature = "native-check", target_os = "windows"))]
            canvas_bounds: Rc::new(std::cell::Cell::new(None)),
            #[cfg(feature = "retained-wgpu")]
            gpu: None,
            #[cfg(feature = "retained-wgpu")]
            _gpu_completion_task: None,
            #[cfg(feature = "retained-wgpu")]
            opacity,
            #[cfg(feature = "retained-wgpu")]
            presented_image: Rc::new(RefCell::new(None)),
            #[cfg(feature = "retained-wgpu")]
            frame_queue: std::sync::Arc::new(gpui_box::retained::frame_queue::FrameQueue::new()),
        }
    }

    #[cfg(all(feature = "retained-wgpu", test))]
    pub fn with_gpu(cx: &mut Context<Self>) -> Self {
        Self::with_renderer(cx, gpui_box::retained::wgpu::WgpuFrameRenderer::new(3).expect("create GPU frame consumer"))
    }

    #[cfg(feature = "retained-wgpu")]
    pub fn with_gpu_for_window(window: &Window, cx: &mut Context<Self>) -> Self {
        Self::with_renderer(cx, gpui_box::retained::wgpu::WgpuFrameRenderer::new_for_window(window, 3).expect("create native GPU frame consumer"))
    }

    #[cfg(feature = "retained-wgpu")]
    fn with_renderer(cx: &mut Context<Self>, renderer: gpui_box::retained::wgpu::WgpuFrameRenderer) -> Self {
        let mut view = Self::new(cx);
        view.tree
            .borrow_mut()
            .set_capabilities(BackendCapabilities {
                solid_color_patch: true,
            });
        view.gpu = Some(Rc::new(RefCell::new(
            renderer,
        )));
        let gpu = view.gpu.as_ref().unwrap();
        let mut completions = gpu
            .borrow_mut()
            .open_completions()
            .expect("open GPU completions");
        let gpu = Rc::downgrade(gpu);
        view._gpu_completion_task = Some(cx.spawn(async move |view, cx| {
            loop {
                let error = match completions.ready().await {
                    Ok(Some(_)) => {
                        let Some(gpu) = gpu.upgrade() else { break };
                        // Owner-only retirement. This does not notify or redraw.
                        gpu.borrow_mut().poll().err().map(|error| error.to_string())
                    }
                    Ok(None) => break,
                    Err(error) => Some(error),
                };
                if let Some(error) = error {
                    let _ = view.update(cx, |view, _| {
                        view.last_commit = format!("GPU completion failed: {error}");
                    });
                    break;
                }
            }
        }));
        view
    }
}

impl Render for RetainedDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_count += 1;
        let tree = self.tree.clone();
        let image_cache = self.image_cache.clone();
        let rasterizer = self.rasterizer.clone();
        let text_signal = self.text;
        let responsive = self.responsive.clone();
        #[cfg(all(feature = "native-check", target_os = "windows"))]
        let canvas_bounds = self.canvas_bounds.clone();
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
            .child(div().id("retained-text").debug_selector(||"retained-text".into()).cursor_pointer().child("Draw retained shaped text")
                .on_click(cx.listener(|this,_,window,cx| {
                    let snapshot=gpui_box::retained::paragraph::ParagraphSnapshot::shape(window.text_system(),"Retained glyphs · 中文",font("Noto Sans CJK SC"),16.0,32.0).expect("shape retained text");
                    let resource=this.rasterizer.borrow_mut().prepare(snapshot,window.scale_factor()).expect("rasterize retained glyphs");
                    let signal=this.text;
                    let commit=this.tree.borrow_mut().transaction(|tx| {tx.set(signal,Some(gpui_box::retained::text::TextPaint {resource,color:0xffdd88ff}))?;Ok(())}).expect("attach retained text");
                    if commit.commit.request_frame {cx.notify();}
                })))
            .child(div().id("retained-text-color").debug_selector(||"retained-text-color".into()).cursor_pointer().child("Change retained text color")
                .on_click(cx.listener(|this,_,_,cx| {
                    let signal=this.text;
                    let commit=this.tree.borrow_mut().transaction(|tx| {
                        if let Some(mut text)=tx.read(signal)?.clone() {text.color=if text.color==0xffdd88ff {0x88ddffff} else {0xffdd88ff};tx.set(signal,Some(text))?;}
                        Ok(())
                    }).expect("patch text color");
                    if commit.commit.request_frame {cx.notify();}
                })))
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
                .id("retained-paragraph-measure")
                .debug_selector(|| "retained-paragraph-measure".into())
                .cursor_pointer()
                .child("Measure owned shaped paragraph (colored geometry)")
                .on_click(cx.listener(|this,_,window,cx| {
                    let next = !this.paragraph_long;
                    let text = if next {"owned paragraph wraps when its available width becomes smaller"} else {"short"};
                    let snapshot = gpui_box::retained::paragraph::ParagraphSnapshot::shape(window.text_system(),text,window.text_style().font(),16.0,32.0).expect("shape owned paragraph");
                    let signal = this.paragraph_measure;
                    let result = this.tree.borrow_mut().transaction(|tx| {tx.set(signal,Some(MeasureSnapshot::Paragraph(snapshot)))?;Ok(())}).expect("commit paragraph measurement");
                    this.paragraph_long = next;
                    if result.commit.request_frame {cx.notify();}
                })))
            .child(div()
                .id("retained-decoration")
                .debug_selector(|| "retained-decoration".into())
                .cursor_pointer()
                .child("Toggle rounded border and shadow")
                .on_click(cx.listener(|this,_,_,cx| {
                    let decoration = this.decoration;
                    let shadows = this.shadows;
                    let result = this.tree.borrow_mut().transaction(|tx| {
                        let next = if tx.read(decoration)?.radius == 0.0 {
                            QuadDecoration {radius:12.0,border_width:3.0,border_color:0xeeaa44ff}
                        } else {QuadDecoration::default()};
                        tx.set(shadows, if next.radius > 0.0 { std::sync::Arc::from([
                            RetainedShadow {color:0x00000088,offset:[3.0,3.0],blur_radius:2.0,spread_radius:1.0,inset:false},
                            RetainedShadow {color:0x00000066,offset:[1.0,1.0],blur_radius:0.0,spread_radius:2.0,inset:true},
                        ]) } else {std::sync::Arc::from([])})?;
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
            .child(
                div().id("retained-portal").debug_selector(|| "retained-portal".into()).cursor_pointer()
                    .child("Move retained portal")
                    .on_click(cx.listener(|this,_,_,cx| {
                        let moved=!this.portal_moved;
                        let commit=this.tree.borrow_mut().reparent_render(this.portal.0,this.portal.1[usize::from(moved)]).expect("move portal atomically");
                        this.portal_moved=moved;
                        if commit.request_frame {cx.notify();}
                    })),
            )
            .child(
                div().id("retained-edit").debug_selector(|| "retained-edit".into()).cursor_pointer()
                    .child("Toggle output in one transaction")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let mut tree = this.tree.borrow_mut();
                        let parent = this.draft_parent;
                        let root = tree.root_render();
                        let previous = this.draft_output;
                        let runs = this.draft_effects.clone();
                        let cleanups = this.draft_cleanups.clone();
                        let source = this.color;
                        let next_color = if *tree.read(source).unwrap() == 0x66aa88ff { 0x245fc7ff } else { 0x66aa88ff };
                        let result = tree.edit(|tx| { tx.set(source, next_color)?; Ok(()) }, |(), edit| {
                            if previous.is_some() {
                                edit.reconcile_keys(parent, &[])?;
                                Ok(None)
                            } else {
                                let owner = edit.reconcile_keys(parent, &[100])?[0];
                                let state = edit.state(owner, 0, || 0u32)?;
                                let material = edit.computed(owner, 1, &[source.dependency(), state.dependency()], move |i| {
                                    Ok(*i.read(source)? + *i.read(state)?)
                                })?;
                                edit.effect(owner, 0, &[material.dependency()], move |_| {
                                    runs.set(runs.get() + 1);
                                    let cleanups = cleanups.clone();
                                    Some(Box::new(move || cleanups.set(cleanups.get() + 1)))
                                })?;
                                let node = edit.append(owner, root, Style {
                                    position: taffy::Position::Absolute,
                                    inset: taffy::Rect { left: length(192.0), top: length(4.0),
                                        right: taffy::LengthPercentageAuto::auto(), bottom: taffy::LengthPercentageAuto::auto() },
                                    size: taffy::Size { width: length(24.0), height: length(24.0) },
                                    ..Default::default()
                                }, 0)?;
                                edit.bind(material, Property::color(node))?;
                                edit.properties().decoration(node, QuadDecoration::default());
                                Ok(Some(node))
                            }
                        }).expect("publish source and output draft");
                        this.draft_output = result.result.value;
                        this.last_commit = format!("Atomic output: {} inserted, {} removed", result.inserted.len(), result.removed.len());
                        if result.result.commit.request_frame { cx.notify(); }
                    })),
            )
            .child(
                div().id("retained-background").debug_selector(|| "retained-background".into()).cursor_pointer()
                    .child("Queue background color burst")
                    .on_click(cx.listener(|this, _, _, _| {
                        let sender = this.color_sender.clone();
                        let final_color = if this.observed_color.get() == 0x66aa88ff { 0x245fc7ff } else { 0x66aa88ff };
                        std::thread::spawn(move || {
                            for color in [0xff8844ff, 0x4488ffff, final_color] {
                                if sender.send(color).is_err() { break; }
                            }
                        });
                    })),
            )
            .child(div().h_6().flex_shrink_0().truncate().child(format!("{} · material #{:08x}", self.last_commit, self.observed_color.get())))
            .child(
                canvas(
                    move |bounds, _window, _| {
                        let current=tree.borrow().read(text_signal).expect("text signal").clone();
                        if let Some(mut text)=current && text.resource.scale()!=_window.scale_factor() {
                            text.resource=rasterizer.borrow_mut().prepare(text.resource.paragraph().clone(),_window.scale_factor()).expect("refresh text scale");
                            tree.borrow_mut().transaction(|tx| {tx.set(text_signal,Some(text))?;Ok(())}).expect("publish scaled glyph resources");
                        }
                        let mut environment = LayoutEnvironment::new([f32::from(bounds.size.width), f32::from(bounds.size.height)]);
                        environment.device_scale = _window.scale_factor();
                        #[cfg(feature = "retained-wgpu")]
                        let frame=if gpu.is_some() {
                            SceneFrame::Local(responsive.borrow_mut().local_frame(&mut tree.borrow_mut(),&environment).expect("publish local retained scene"))
                        } else {SceneFrame::Resolved(responsive.borrow_mut().frame(&mut tree.borrow_mut(),&environment).expect("resolve responsive retained frame").frame)};
                        #[cfg(not(feature = "retained-wgpu"))]
                        let frame=SceneFrame::Resolved(responsive.borrow_mut().frame(&mut tree.borrow_mut(),&environment).expect("resolve responsive retained frame").frame);
                        #[cfg(feature = "retained-wgpu")]
                        if let Some(gpu) = &gpu {
                            let scale = _window.scale_factor();
                            #[cfg(target_os = "windows")]
                            let physical = _window.gpu_surface_bounds(bounds);
                            #[cfg(all(feature = "native-check", target_os = "windows"))]
                            canvas_bounds.set(Some(physical));
                            #[cfg(target_os = "windows")]
                            let surface = [physical.size.width.0.max(1) as u32, physical.size.height.0.max(1) as u32];
                            #[cfg(not(target_os = "windows"))]
                            let surface = [
                                (f32::from(bounds.size.width) * scale).ceil().max(1.0) as u32,
                                (f32::from(bounds.size.height) * scale).ceil().max(1.0) as u32,
                            ];
                            if let SceneFrame::Local(packet)=&frame {
                                gpu.borrow_mut().submit_local_for_window(_window,packet,surface,scale).expect("submit local UI frame to GPU");
                            } else {
                            let resolved=frame.resolve();
                            frame_queue.publish(resolved.clone()).expect("publish visual packet");
                            if let Some(packet) = frame_queue.take_latest().expect("consume visual packet") {
                                gpu.borrow_mut()
                                    .submit_for_window(_window, &packet, surface, scale)
                                    .expect("submit UI frame to GPU");
                            } else {
                                // Scale/surface demand can change without a new
                                // logical revision; same-packet submit handles it.
                                gpu.borrow_mut()
                                    .submit_for_window(_window, &resolved, surface, scale)
                                    .expect("refresh surface geometry");
                            }
                            }
                        }
                        #[cfg(feature = "retained-wgpu")]
                        let raster = if let Some(gpu) = &gpu {
                            let mut gpu = gpu.borrow_mut();
                            #[cfg(target_os = "windows")]
                            let native = if std::env::var_os("GPUI_RETAINED_READBACK").is_none() {
                                gpu.window_surface().expect("export native GPU snapshot")
                            } else { None };
                            #[cfg(target_os = "windows")]
                            if let Some(surface) = native { WindowRaster::Gpu(surface) }
                            else { WindowRaster::Image(gpu.window_image().expect("read GPU window image")) }
                            #[cfg(not(target_os = "windows"))]
                            { WindowRaster::Image(gpu.window_image().expect("read GPU window image")) }
                        } else { WindowRaster::Vector };
                        #[cfg(feature = "retained-wgpu")]
                        if let Some(previous) = presented_image.replace(raster.image())
                            && raster.image().as_ref().is_none_or(|image| image.id != previous.id)
                        {
                            _window
                                .drop_image(previous)
                                .expect("retire previous readback image");
                        }
                        #[cfg(not(feature = "retained-wgpu"))]
                        let raster = WindowRaster::Vector;
                        // Resolve world geometry only for the vector bridge.
                        // Native GPU paint retains the immutable local snapshot.
                        let resolved=matches!(raster,WindowRaster::Vector).then(||frame.resolve());
                        let mut cache = image_cache.borrow_mut();
                        let live:std::collections::BTreeSet<_> = resolved.iter().flat_map(|frame|frame.items.iter()).filter_map(|item| item.image.as_ref().map(ImageResource::id)).collect();
                        let retired:Vec<_> = cache.keys().filter(|id| !live.contains(id)).copied().collect();
                        for id in retired {
                            if let Some(image) = cache.remove(&id) {
                                _window.drop_image(image).expect("retire decoded image sprite");
                            }
                        }
                        for resource in resolved.iter().flat_map(|frame|frame.items.iter()).filter_map(|item| item.image.as_ref()) {
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
                    |bounds, (frame, _raster, images), window, _| {
                        #[cfg(all(feature = "retained-wgpu", target_os = "windows"))]
                        if let WindowRaster::Gpu(surface) = &_raster {
                            window.paint_gpu_surface(bounds, surface.clone());
                            return;
                        }
                        #[cfg(feature = "retained-wgpu")]
                        if let WindowRaster::Image(raster) = _raster {
                            window
                                .paint_image(bounds, bounds, Corners::default(), raster, 0, false)
                                .expect("paint isolated groups");
                            return;
                        }
                        let frame=frame.resolve();
                        for item in frame.items.iter() {
                            let viewport = Rect {x:0.0,y:0.0,width:f32::from(bounds.size.width),height:f32::from(bounds.size.height)};
                            let mask = Some(item.clip.map_or(viewport, |clip| clip.intersect(viewport))).map(|clip| ContentMask {
                                bounds: Bounds {
                                    origin: bounds.origin + point(px(clip.x), px(clip.y)),
                                    size: size(px(clip.width), px(clip.height)),
                                },
                            });
                            window.with_content_mask(mask, |window| {
                                let element_bounds = Bounds {origin:bounds.origin+point(px(item.bounds.x),px(item.bounds.y)),size:size(px(item.bounds.width),px(item.bounds.height))};
                                let radius = item.commands.first().map_or(0.0, |command| command.quad().1.resolved(item.bounds).radius);
                                let shadows = item.shadows.iter().map(|shadow| gpui::BoxShadow {
                                    color:rgba(shadow.color).into(),offset:point(px(shadow.offset[0]),px(shadow.offset[1])),
                                    blur_radius:px(shadow.blur_radius),spread_radius:px(shadow.spread_radius),inset:shadow.inset,
                                }).collect::<Vec<_>>();
                                window.paint_drop_shadows(element_bounds,Corners::all(px(radius)),&shadows);
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
                                window.paint_inset_shadows(element_bounds,Corners::all(px(radius)),&shadows);
                                if let Some(resource) = &item.image {
                                    let r = item.content_bounds;
                                    let image_bounds = Bounds {origin:bounds.origin+point(px(r.x),px(r.y)),size:size(px(r.width),px(r.height))};
                                    window.paint_image(image_bounds,image_bounds,Corners::default(),images[&resource.id()].clone(),0,false).expect("paint retained decoded image");
                                }
                                if let (Some(text),Some(chunk))=(&item.text,&item.glyphs) {
                                    for glyph in chunk.glyphs.iter() {
                                        let origin=point(px(item.content_bounds.x+glyph.origin[0]),px(item.content_bounds.y+glyph.origin[1]));
                                        window.paint_glyph(bounds.origin+origin,glyph.font,glyph.glyph,glyph.font_size,rgba(text.color).into()).expect("paint owned retained glyph");
                                    }
                                }
                            });
                        }
                    },
                )
                .w_full()
                .flex_shrink_0()
                .h_8(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn background_source_delivery_wakes_owner_once_for_burst_and_idles(cx: &mut TestAppContext) {
        // This test intentionally wakes the foreground future from an actual
        // OS worker rather than GPUI's deterministic simulated worker queue.
        cx.executor().allow_parking();
        let (view, cx) = cx.add_window_view(|_, cx| RetainedDemo::new(cx));
        cx.run_until_parked();
        let (sender, before, renders, stats) = view.read_with(cx, |view, _| {
            (
                view.color_sender.clone(),
                view.tree.borrow().published_frame().unwrap().clone(),
                view.render_count,
                view.tree.borrow().stats(),
            )
        });
        std::thread::spawn(move || {
            for _ in 0..100 {
                sender.send(0x4488ffff).unwrap();
                sender.send(0x66aa88ff).unwrap();
            }
        })
        .join()
        .unwrap();
        view.read_with(cx, |view, _| {
            assert_eq!(*view.tree.borrow().read(view.color).unwrap(), 0x245fc7ff);
            assert_eq!(view.tree.borrow().stats(), stats);
            assert_eq!(view.color_sender.stats().unwrap().pending, 1);
        });
        cx.run_until_parked();
        let after = view.read_with(cx, |view, _| {
            let current = view.tree.borrow().stats();
            assert_eq!(current.transactions, stats.transactions + 1);
            assert_eq!(current.computed_evaluations, stats.computed_evaluations + 1);
            assert_eq!(current.effect_runs, stats.effect_runs + 1);
            assert_eq!(view.observed_color.get(), 0x66aa88ff);
            assert_eq!(view.render_count, renders + 1);
            let queue = view.color_sender.stats().unwrap();
            assert_eq!(
                (
                    queue.enqueued,
                    queue.coalesced,
                    queue.applied,
                    queue.waker_notifications
                ),
                (200, 199, 1, 1)
            );
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!(after.revision, before.revision + 1);
        // The ordinary canvas consumer declares no instance-patch capability,
        // so its one changed material legitimately records one local chunk.
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 1));
        for _ in 0..16 {
            cx.run_until_parked();
        }
        let (sender, renders) = view.read_with(cx, |view, _| {
            assert_eq!(view.render_count, renders + 1);
            (view.color_sender.clone(), view.render_count)
        });
        // Equal committed values still wake the owner once to inspect the
        // source, then cause no computed work, frame intent or view render.
        std::thread::spawn(move || {
            sender.send(0x66aa88ff).unwrap();
        })
        .join()
        .unwrap();
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.render_count, renders);
            assert_eq!(
                view.tree.borrow().published_frame().unwrap().revision,
                after.revision
            );
            assert_eq!(view.color_sender.stats().unwrap().waker_notifications, 2);
        });
    }

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
            assert_eq!(view.observed_color.get(), 0xc75824ff);
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
            (3, 3, 2, 2)
        );
        assert_eq!(stats.computed_evaluations, 1);
        assert_eq!(stats.effect_runs, 1);
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
        let (portal, old_portal, owner) = view.read_with(cx, |view, _| {
            let tree = view.tree.borrow();
            let frame = tree.published_frame().unwrap();
            let item = frame
                .items
                .iter()
                .find(|i| i.node == view.portal.0)
                .unwrap()
                .clone();
            let owner = tree
                .children(tree.root_scope())
                .unwrap()
                .iter()
                .copied()
                .find(|scope| tree.renders(*scope).unwrap().contains(&view.portal.0))
                .unwrap();
            (view.portal.0, (frame.revision, item), owner)
        });
        let button = cx.debug_bounds("retained-portal").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let tree = view.tree.borrow();
            let frame = tree.published_frame().unwrap();
            let item = frame.items.iter().find(|i| i.node == portal).unwrap();
            assert_eq!(frame.revision, old_portal.0 + 1);
            assert_eq!(frame.recorded_chunks, 0);
            assert!(std::sync::Arc::ptr_eq(
                &item.commands,
                &old_portal.1.commands
            ));
            assert_ne!(item.bounds.x, old_portal.1.bounds.x);
            assert!(tree.renders(owner).unwrap().contains(&portal));
        });
        for inserted in [true, false] {
            let (revision, bindings) = view.read_with(cx, |view, _| {
                let tree = view.tree.borrow();
                (
                    tree.published_frame().unwrap().revision,
                    tree.reactive().binding_count(),
                )
            });
            let button = cx.debug_bounds("retained-edit").unwrap();
            cx.simulate_click(button.center(), Modifiers::default());
            cx.run_until_parked();
            view.read_with(cx, |view, _| {
                let tree = view.tree.borrow();
                assert_eq!(tree.published_frame().unwrap().revision, revision + 1);
                assert_eq!(view.draft_output.is_some(), inserted);
                assert_eq!(
                    tree.reactive().binding_count(),
                    if inserted { bindings + 1 } else { bindings - 1 }
                );
                if let Some(node) = view.draft_output {
                    let frame = tree.published_frame().unwrap();
                    let item = frame.items.iter().find(|i| i.node == node).unwrap();
                    assert_eq!(item.fill_color, *tree.read(view.color).unwrap());
                    let owner = tree.children(view.draft_parent).unwrap()[0];
                    assert!(tree.renders(owner).unwrap().contains(&node));
                    assert_eq!(view.draft_effects.get(), 1);
                    assert_eq!(view.draft_cleanups.get(), 0);
                } else {
                    assert!(tree.children(view.draft_parent).unwrap().is_empty());
                    assert_eq!(view.draft_effects.get(), 1);
                    assert_eq!(view.draft_cleanups.get(), 1);
                }
            });
        }
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
        cx.executor().allow_parking();
        let platform = std::sync::Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
            "Noto Sans CJK SC",
        ));
        let mut real_cx =
            TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
        real_cx.update(|cx| {
            cx.text_system()
                .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                    "../assets/NotoSansSC-Regular.otf"
                ))])
                .unwrap()
        });
        let cx = &mut real_cx;
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
        assert!(!measured.build_stats.full_rebuild);
        assert!(measured.layout_stats.layout_writes > 0);
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
        assert_eq!(frame.items.iter().map(|item| item.shadows.len()).sum::<usize>(),2);
        let actual = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [actual.width(), actual.height()], scale)
            .unwrap();
        assert!(
            actual.as_raw() == reference.as_raw(),
            "rounded border and typed shadow click reach GPU"
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
        let button = cx.debug_bounds("retained-paragraph-measure").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            assert!(matches!(
                view.tree.borrow().read(view.paragraph_measure).unwrap(),
                Some(MeasureSnapshot::Paragraph(_))
            ));
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (1, 0));
        assert!(!frame.build_stats.full_rebuild);
        assert!(frame.layout_stats.layout_writes > 0);
        let measured = gpu.borrow_mut().image().unwrap();
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [measured.width(), measured.height()], scale)
            .unwrap();
        assert_eq!(
            measured.as_raw(),
            reference.as_raw(),
            "normal paragraph measurement updates GPU geometry"
        );
        let button = cx.debug_bounds("retained-text").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert!(frame.items.iter().any(|item| {
            item.glyphs
                .as_ref()
                .is_some_and(|chunk| !chunk.glyphs.is_empty())
        }));
        let text_pixels = gpu.borrow_mut().image().unwrap();
        assert_ne!(
            text_pixels.as_raw(),
            measured.as_raw(),
            "real glyph drawing changes pixels"
        );
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [text_pixels.width(), text_pixels.height()], scale)
            .unwrap();
        assert_eq!(text_pixels.as_raw(), reference.as_raw());
        let rasterized = view.read_with(cx, |view, _| view.rasterizer.borrow().rasterized_glyphs());
        let button = cx.debug_bounds("retained-text-color").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            assert_eq!(view.rasterizer.borrow().rasterized_glyphs(), rasterized);
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
        let changed = gpu.borrow_mut().image().unwrap();
        assert_ne!(changed.as_raw(), text_pixels.as_raw());
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [changed.width(), changed.height()], scale)
            .unwrap();
        assert_eq!(changed.as_raw(), reference.as_raw());
        let revision = frame.revision;
        let button = cx.debug_bounds("retained-portal").unwrap();
        cx.simulate_click(button.center(), Modifiers::default());
        cx.run_until_parked();
        let frame = view.read_with(cx, |view, _| {
            view.tree.borrow().published_frame().unwrap().clone()
        });
        assert_eq!(frame.revision, revision + 1);
        assert_eq!(frame.recorded_chunks, 0);
        let moved = gpu.borrow_mut().image().unwrap();
        assert!(
            moved.as_raw() != changed.as_raw(),
            "visible portal move changes GPU pixels"
        );
        let reference = gpu
            .borrow_mut()
            .reference_image(&frame, [moved.width(), moved.height()], scale)
            .unwrap();
        assert!(
            moved.as_raw() == reference.as_raw(),
            "portal move matches full reference pixels"
        );
        let mut pixels = moved;
        for _ in 0..2 {
            let revision = view.read_with(cx, |view, _| {
                view.tree.borrow().published_frame().unwrap().revision
            });
            let button = cx.debug_bounds("retained-edit").unwrap();
            cx.simulate_click(button.center(), Modifiers::default());
            cx.run_until_parked();
            let frame = view.read_with(cx, |view, _| {
                view.tree.borrow().published_frame().unwrap().clone()
            });
            assert_eq!(frame.revision, revision + 1);
            let next = gpu.borrow_mut().image().unwrap();
            assert!(
                next.as_raw() != pixels.as_raw(),
                "atomic output toggle changes pixels"
            );
            let reference = gpu
                .borrow_mut()
                .reference_image(&frame, [next.width(), next.height()], scale)
                .unwrap();
            assert!(
                next.as_raw() == reference.as_raw(),
                "atomic output toggle matches full reference"
            );
            pixels = next;
        }
    }
}
