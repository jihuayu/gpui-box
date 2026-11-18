use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn owner_host_virtual_list_native_wheel_installs_complete_cpu_picture(cx: &mut TestAppContext) {
    use gpui_box::retained::virtual_list::*;
    #[derive(Clone, PartialEq)]
    struct RowProps {
        color: u32,
    }
    struct Row;
    impl Component for Row {
        type Props = RowProps;
        fn mount(
            props: ReadProp<RowProps>,
            cx: &mut ComponentContext<'_, '_, '_, '_>,
        ) -> Result<(), Error> {
            let node = cx.append_layout(
                LayoutStyle::row()
                    .size(LayoutDimension::Percent(1.0), LayoutDimension::Px(20.0))
                    .flex_shrink(0.0),
                0,
            )?;
            let color = cx.computed(0, &[props.dependency()], move |i| {
                Ok(i.read_prop(props)?.color)
            })?;
            cx.bind(color, Property::color(node))?;
            Ok(())
        }
    }
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (scroll, host, initial) = view.update(cx, |this, cx| {
        let root = this.ui.root_scope();
        let render = this.ui.root_render();
        let viewport = this.shown.as_ref().unwrap().viewport;
        let scroll = this.ui.state(root, 100, || [0.0; 2]).unwrap();
        let rows = VirtualListItems::<Row>::new(
            VirtualListMetrics::fixed(100_000, 20.0).unwrap(),
            |index| {
                Ok((
                    index as u64,
                    RowProps {
                        color: if index % 2 == 0 {
                            0x117755ff
                        } else {
                            0x775511ff
                        },
                    },
                ))
            },
        );
        let scope = this
            .ui
            .edit_local_frame(
                LayoutConstraints::viewport(viewport[0], viewport[1]),
                viewport,
                |_| Ok(()),
                |(), edit| {
                    edit.components::<VirtualList<Row>>(
                        root,
                        render,
                        [(1, VirtualListProps::new(rows, scroll, [240.0, 80.0]))],
                    )
                },
            )
            .unwrap()
            .0
            .result
            .value[0];
        let host = this.ui.renders(scope).unwrap()[0];
        // Clip the original demo content while testing the new independent
        // list site in normal flow. Do not change virtual-list internals.
        let old = this.ui.renders(root).unwrap().to_vec();
        this.ui
            .edit(
                |_| Ok(()),
                |(), edit| {
                    for node in old {
                        edit.properties()
                            .layout_style(node, LayoutStyle::column().display(LayoutDisplay::None));
                    }
                    Ok(())
                },
            )
            .unwrap();
        let initial = this.installs;
        cx.notify();
        (scroll, host, initial)
    });
    cx.run_until_parked();
    let point = view.read_with(cx, |this, _| {
        let rect = this
            .shown
            .as_ref()
            .unwrap()
            .frame
            .resolve_geometry(host)
            .unwrap()
            .bounds;
        this.bounds.unwrap().origin + point(px(rect.x + 5.0), px(rect.y + 5.0))
    });
    cx.simulate_event(ScrollEvent {
        position: point,
        delta: ScrollDelta::Pixels(gpui::point(px(0.0), px(-45.0))),
        ..Default::default()
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(*this.ui.read(scroll).unwrap(), [0.0, 45.0]);
        assert!(this.installs > initial);
        assert!(this.error.is_none(), "{:?}", this.error);
        let shown = this.shown.as_ref().unwrap();
        assert_eq!(this.router.scene_id(), Some(shown.scene.id()));
        assert!(shown.frame.stats().retained_items < 40);
        assert!(this.cpu_task.is_none() && this.pending.is_none() && this.ready.is_none());
    });
}

#[gpui::test]
fn owner_host_source_task_effect_replaces_work_and_installs_complete_cpu_result(
    cx: &mut TestAppContext,
) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (old, waiter, launched, previous) = view.update(cx, |this, cx| {
        let root = this.ui.root_scope();
        let input = this.ui.state(root, 100, || 1u32).unwrap();
        let color = this.ui.state(root, 101, || 0x223149ffu32).unwrap();
        this.ui
            .bind(color, Property::color(this.scroll_host))
            .unwrap();
        let launched = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let seen = launched.clone();
        let executor = cx.background_executor().clone();
        this.ui
            .task_effect(
                root,
                30,
                input,
                UiTaskEffect::new(move |input, context: UiTaskContext<u32>| {
                    seen.borrow_mut().push(input);
                    executor
                        .spawn(async move {
                            let _ = context.complete(input * 10);
                        })
                        .detach();
                }),
            )
            .unwrap();
        let old = this.ui.task::<u32>(root, 30).unwrap().unwrap();
        this.ui
            .transaction(|tx| {
                tx.set(input, 2)?;
                tx.set(input, 9)
            })
            .unwrap();
        let mut task = this.ui.task::<u32>(root, 30).unwrap().unwrap();
        let count = this.count;
        let previous = this.shown.as_ref().unwrap().frame.revision();
        let waiter = cx.spawn(async move |view, cx| {
            assert_eq!(task.ready().await, UiTaskStatus::Ready);
            view.update(cx, |this, cx| {
                assert!(matches!(
                    this.ui
                        .apply_task(&task, |value, tx| {
                            tx.set(count, *value)?;
                            tx.set(color, 0x117755ff)
                        })
                        .unwrap(),
                    TaskApply::Applied(_)
                ));
                cx.notify();
            })
            .unwrap();
        });
        (old, waiter, launched, previous)
    });
    cx.run_until_parked();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(*launched.borrow(), [1, 9]);
    view.read_with(cx, |this, _| {
        assert_eq!(*this.ui.read(this.count).unwrap(), 90);
        let shown = this.shown.as_ref().unwrap();
        assert!(shown.frame.revision() > previous);
        assert_eq!(
            shown
                .frame
                .resolve_geometry(this.scroll_host)
                .unwrap()
                .fill_color,
            0x117755ff
        );
        assert_eq!(this.router.scene_id(), Some(shown.scene.id()));
        assert!(this.cpu_task.is_none() && this.pending.is_none() && this.ready.is_none());
        assert!(this.error.is_none(), "{:?}", this.error);
        assert_eq!(this.ui.task_pool().stats().in_use, 0);
    });
    drop(waiter);
}

#[gpui::test]
fn native_owner_host_real_gpui_mouse_keyboard_scroll_and_outside_release(cx: &mut TestAppContext) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let bounds = cx.debug_bounds("owner-runtime-host").unwrap();
    let click = bounds.origin + point(px(35.0), px(80.0));
    cx.simulate_click(click, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(*view.ui.read(view.count).unwrap(), 1);
        assert!(view.error.is_none(), "{:?}", view.error);
        assert!(view.installs > 0);
        assert!(view.router.captured().is_none());
    });
    cx.simulate_keystrokes("enter");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(*view.ui.read(view.count).unwrap(), 2)
    });
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.simulate_keystrokes("space");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(*view.ui.read(view.count).unwrap(), 12);
        let scene = view.ui.input_scene().unwrap();
        let id = scene.node_id(view.scroll_host).unwrap();
        assert!(scene.scroll_state(id).unwrap().offset[1] > 0.0);
    });
    cx.simulate_event(ScrollEvent {
        position: bounds.origin + point(px(60.0), px(160.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(20.0))),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.simulate_mouse_down(click, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let outside = point(bounds.origin.x + px(500.0), bounds.origin.y - px(10.0));
    cx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(*view.ui.read(view.count).unwrap(), 12);
        assert!(view.router.captured().is_none());
        assert!(view.error.is_none(), "{:?}", view.error);
    });
    // A burst can replace pending pictures, never append an unbounded backlog.
    // During resize, installed pixels keep their old logical geometry.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let old = view.shown.as_ref().unwrap().clone();
            let mut bounds = view.bounds.unwrap();
            bounds.size = size(px(420.0), px(250.0));
            let pending = view.prepare(bounds, window, cx);
            assert!(pending.is_some());
            for width in [440.0, 460.0] {
                bounds.size.width = px(width);
                view.prepare(bounds, window, cx);
            }
            assert_eq!(view.pending.as_ref().unwrap().viewport[0], 460.0);
            bounds.size.width = px(420.0);
            view.prepare(bounds, window, cx);
            assert_eq!(view.shown.as_ref().unwrap().scene.id(), old.scene.id());
            assert_eq!(
                view.adapter().unwrap().bounds.width,
                420.0f32.min(old.viewport[0])
            );
        })
    });
    cx.run_until_parked();
    cx.simulate_resize(size(px(420.0), px(350.0)));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.cpu_task.is_none() && view.pending.is_none() && view.ready.is_none());
        assert_eq!(
            view.router.scene_id(),
            Some(view.shown.as_ref().unwrap().scene.id())
        );
        assert_eq!(view.shown.as_ref().unwrap().viewport[0], 420.0);
    });
    // Losing native focus cancels an activation hold.
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-tab space");
    cx.deactivate_window();
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.router.focused().is_none());
        assert!(view.router.captured().is_none());
        assert_eq!(*view.ui.read(view.count).unwrap(), 12);
    });
}

#[gpui::test]
fn owner_host_executor_task_output_reaches_complete_cpu_picture_and_installed_scene(
    cx: &mut TestAppContext,
) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (task, waiter) = view.update(cx, |this, cx| {
        let executor = cx.background_executor().clone();
        let root = this.ui.root_scope();
        let count = this.count;
        let mut task = this
            .ui
            .start_task(root, 10, move |context: UiTaskContext<u32>| {
                executor
                    .spawn(async move {
                        context.complete(42).unwrap();
                    })
                    .detach();
            })
            .unwrap();
        let handle = task.clone();
        let waiter = cx.spawn(async move |view, cx| {
            assert_eq!(task.ready().await, UiTaskStatus::Ready);
            view.update(cx, |this, cx| {
                assert!(matches!(
                    this.ui
                        .apply_task(&task, |value, tx| {
                            tx.set(count, *value)?;
                            Ok(())
                        })
                        .unwrap(),
                    TaskApply::Applied(_)
                ));
                cx.notify();
            })
            .unwrap();
        });
        (handle, waiter)
    });
    cx.run_until_parked();
    assert_eq!(task.status(), UiTaskStatus::Applied);
    view.read_with(cx, |this, _| {
        assert_eq!(*this.ui.read(this.count).unwrap(), 42);
        assert!(this.cpu_task.is_none() && this.pending.is_none() && this.ready.is_none());
        assert_eq!(
            this.router.scene_id(),
            Some(this.shown.as_ref().unwrap().scene.id())
        );
        assert!(this.error.is_none(), "{:?}", this.error);
    });
    drop(waiter);
}

#[gpui::test]
fn owner_host_capacity_wake_retries_after_cancelled_worker_exits(cx: &mut TestAppContext) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (old, waiter, pool, previous_revision) = view.update(cx, |this, cx| {
        let pool = UiTaskPool::new(1).unwrap();
        this.ui.set_task_pool(pool.clone()).unwrap();
        let mut capacity = pool.open_capacity().unwrap();
        let root = this.ui.root_scope();
        let count = this.count;
        let color = this.ui.state(root, 100, || 0x223149ffu32).unwrap();
        this.ui
            .bind(color, Property::color(this.scroll_host))
            .unwrap();
        let previous_revision = this.shown.as_ref().unwrap().frame.revision();
        let executor = cx.background_executor().clone();
        let worker = std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot = worker.clone();
        let old = this
            .ui
            .start_task(root, 1, move |context: UiTaskContext<u32>| {
                *slot.borrow_mut() = Some(context)
            })
            .unwrap();
        assert!(matches!(
            this.ui
                .start_task(root, 2, |_: UiTaskContext<u32>| panic!("full")),
            Err(Error::TaskCapacity)
        ));
        this.ui.cancel_task(root, 1).unwrap();
        assert_eq!(pool.stats().in_use, 1);
        let mut worker = worker.borrow_mut().take();
        let release = cx.background_executor().clone();
        release
            .spawn(async move {
                drop(worker.take());
            })
            .detach();
        let waiter = cx.spawn(async move |view, cx| {
            assert_eq!(capacity.ready().await.unwrap().in_use, 0);
            let mut task = view
                .update(cx, |this, _| {
                    this.ui
                        .start_task(root, 2, move |context: UiTaskContext<u32>| {
                            executor
                                .spawn(async move {
                                    context.complete(73).unwrap();
                                })
                                .detach();
                        })
                        .unwrap()
                })
                .unwrap();
            assert_eq!(task.ready().await, UiTaskStatus::Ready);
            view.update(cx, |this, cx| {
                this.ui
                    .apply_task(&task, |value, tx| {
                        tx.set(count, *value)?;
                        tx.set(color, 0x375533ff)?;
                        Ok(())
                    })
                    .unwrap();
                cx.notify();
            })
            .unwrap();
        });
        (old, waiter, pool, previous_revision)
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(*this.ui.read(this.count).unwrap(), 73);
        assert_eq!(
            this.router.scene_id(),
            Some(this.shown.as_ref().unwrap().scene.id())
        );
        assert!(this.error.is_none());
        assert!(this.shown.as_ref().unwrap().frame.revision() > previous_revision);
        assert_eq!(
            this.shown
                .as_ref()
                .unwrap()
                .frame
                .resolve_geometry(this.scroll_host)
                .unwrap()
                .fill_color,
            0x375533ff
        );
    });
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(pool.stats().peak, 1);
    assert_eq!(pool.stats().in_use, 0);
    drop(waiter);
}

#[gpui::test]
fn owner_host_click_starts_scoped_work_and_installs_its_complete_cpu_result(
    cx: &mut TestAppContext,
) {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let mut real_cx = TestAppContext::build_with_text_system(cx.dispatcher.clone(), None, platform);
    let cx = &mut real_cx;
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx.open_window(size(px(600.0), px(460.0)), |window, cx| {
        OwnerDemo::new(window, cx, true)
    });
    let view = window.root(cx).unwrap();
    let cx = VisualTestContext::from_window(*window, cx).into_mut();
    cx.run_until_parked();
    let (button, color, initial) = view.update(cx, |this, cx| {
        let scope = this.ui.root_scope();
        let button = this.ui.renders(scope).unwrap()[1];
        let color = this.ui.state(scope, 100, || 0x223149ffu32).unwrap();
        this.ui
            .bind(color, Property::color(this.scroll_host))
            .unwrap();
        let executor = cx.background_executor().clone();
        let count = this.count;
        this.ui
            .edit(
                |_| Ok(()),
                |(), edit| {
                    let handler = edit.event_handler(
                        scope,
                        1,
                        EventHandler::new(move |event, event_cx| {
                            if event.kind == UiInputEventKind::Activate {
                                let executor = executor.clone();
                                event_cx.start_task(20, move |context: UiTaskContext<u32>| {
                                    executor
                                        .spawn(async move {
                                            context.complete(83).unwrap();
                                        })
                                        .detach();
                                })?;
                                event_cx.set(count, 1)?;
                            }
                            Ok(())
                        }),
                    )?;
                    edit.interaction(
                        scope,
                        button,
                        Some(UiInteraction {
                            focus: UiFocusPolicy::Tab,
                            activate_handler: Some(handler),
                            ..Default::default()
                        }),
                    )?;
                    Ok(())
                },
            )
            .unwrap();
        cx.notify();
        (button, color, this.shown.as_ref().unwrap().frame.revision())
    });
    cx.run_until_parked();
    let point = view.read_with(cx, |this, _| {
        let rect = this
            .shown
            .as_ref()
            .unwrap()
            .frame
            .resolve_geometry(button)
            .unwrap()
            .bounds;
        this.bounds.unwrap().origin + point(px(rect.x + 20.0), px(rect.y + 20.0))
    });
    cx.simulate_click(point, Modifiers::default());
    cx.run_until_parked();
    let waiter = view.update(cx, |this, cx| {
        assert_eq!(*this.ui.read(this.count).unwrap(), 1);
        let count = this.count;
        let mut task = this
            .ui
            .task::<u32>(this.ui.root_scope(), 20)
            .unwrap()
            .unwrap();
        cx.spawn(async move |view, cx| {
            assert_eq!(task.ready().await, UiTaskStatus::Ready);
            view.update(cx, |this, cx| {
                this.ui
                    .apply_task(&task, |value, tx| {
                        tx.set(count, *value)?;
                        tx.set(color, 0x775533ff)?;
                        Ok(())
                    })
                    .unwrap();
                cx.notify();
            })
            .unwrap();
        })
    });
    cx.run_until_parked();
    view.read_with(cx, |this, _| {
        assert_eq!(*this.ui.read(this.count).unwrap(), 83);
        let shown = this.shown.as_ref().unwrap();
        assert!(shown.frame.revision() > initial);
        assert_eq!(
            shown
                .frame
                .resolve_geometry(this.scroll_host)
                .unwrap()
                .fill_color,
            0x775533ff
        );
        assert_eq!(this.router.scene_id(), Some(shown.scene.id()));
        assert!(this.error.is_none(), "{:?}", this.error);
        assert_eq!(this.ui.task_pool().stats().in_use, 0);
    });
    drop(waiter);
}
