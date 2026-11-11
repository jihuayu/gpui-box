//! Normal GPUI event → retained transaction → persistent layout → paint adapter.
use gpui::*;
use gpui_box::retained::{BackendCapabilities, DisplayCommand, NodeId, Style, Transaction, UiTree};
use std::{cell::RefCell, rc::Rc};

pub struct RetainedDemo {
    tree: Rc<RefCell<UiTree>>,
    tile: NodeId,
    alternate: bool,
    last_commit: String,
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
        tree.commit(transaction, BackendCapabilities::default())
            .unwrap();
        Self {
            tree: Rc::new(RefCell::new(tree)),
            tile,
            alternate: false,
            last_commit: "Ready".into(),
        }
    }
}

impl Render for RetainedDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tree = self.tree.clone();
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
            .child(self.last_commit.clone())
            .child(
                canvas(
                    move |bounds, _, _| {
                        tree.borrow_mut()
                            .frame([f32::from(bounds.size.width), f32::from(bounds.size.height)])
                            .expect("prepare retained frame")
                    },
                    |bounds, frame, window, _| {
                        for item in frame.items.iter() {
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
    }
}
