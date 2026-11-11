//! Normal GPUI event → retained transaction → persistent layout → paint adapter.
use gpui::*;
use gpui_box::retained::{
    BackendCapabilities, DisplayCommand, NodeId, Rect, Style, Transaction, UiTree, length, taffy,
};
use std::{cell::RefCell, rc::Rc};

pub struct RetainedDemo {
    tree: Rc<RefCell<UiTree>>,
    tile: NodeId,
    viewport: NodeId,
    scrolled: bool,
    alternate: bool,
    last_commit: String,
    #[cfg(feature = "retained-wgpu")]
    gpu: Option<Rc<RefCell<gpui_box::retained::wgpu::WgpuFrameRenderer>>>,
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
        Self {
            tree: Rc::new(RefCell::new(tree)),
            tile,
            viewport,
            scrolled: false,
            alternate: false,
            last_commit: "Ready".into(),
            #[cfg(feature = "retained-wgpu")]
            gpu: None,
        }
    }

    #[cfg(feature = "retained-wgpu")]
    pub fn with_gpu(cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(cx);
        view.gpu = Some(Rc::new(RefCell::new(
            gpui_box::retained::wgpu::WgpuFrameRenderer::new(3).expect("create GPU frame consumer"),
        )));
        view
    }
}

impl Render for RetainedDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tree = self.tree.clone();
        #[cfg(feature = "retained-wgpu")]
        let gpu = self.gpu.clone();
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
                        let next = !this.alternate;
                        let mut tx = Transaction::default();
                        tx.color(this.tile, if next { 0xc75824ff } else { 0x245fc7ff });
                        let commit = this
                            .tree
                            .borrow_mut()
                            .commit(tx, BackendCapabilities::default())
                            .expect("commit color");
                        this.alternate = next;
                        this.last_commit = format!(
                            "Revision {} · {} changed · {:?}",
                            commit.revision, commit.changed_nodes, commit.routes
                        );
                        if commit.request_frame {
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
                        let current = if this.alternate {
                            0xc75824ff
                        } else {
                            0x245fc7ff
                        };
                        let mut tx = Transaction::default();
                        tx.color(this.tile, 0xffffffff);
                        tx.color(this.tile, current);
                        let commit = this
                            .tree
                            .borrow_mut()
                            .commit(tx, BackendCapabilities::default())
                            .expect("commit equal transaction");
                        debug_assert!(!commit.request_frame);
                    })),
            )
            .child(
                div()
                    .id("retained-scroll")
                    .debug_selector(|| "retained-scroll".into())
                    .cursor_pointer()
                    .child("Scroll clipped content")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scrolled = !this.scrolled;
                        let mut tx = Transaction::default();
                        tx.scroll(this.viewport, [if this.scrolled { 24.0 } else { 0.0 }, 0.0]);
                        let commit = this
                            .tree
                            .borrow_mut()
                            .commit(tx, BackendCapabilities::default())
                            .expect("commit scroll");
                        if commit.request_frame {
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
                        frame
                    },
                    |bounds, frame, window, _| {
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
            assert!(view.alternate);
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
    }
}
