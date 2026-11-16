use super::*;

fn setup() -> LogicalUi {
    LogicalUi::new_layout(
        LayoutStyle::row().size(LayoutDimension::Px(300.0), LayoutDimension::Px(120.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap()
}
fn host(width: f32) -> LayoutStyle {
    LayoutStyle::column()
        .fixed_inline_size(width)
        .align_self(LayoutAlignSelf::Start)
}
fn rules(base: LayoutStyle, width: f32, wider: LayoutStyle) -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(base, vec![(width, wider)]).unwrap()
}

#[test]
fn nested_queries_run_in_dependency_order_with_auto_height_and_no_idle_work() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let outer = ui
        .append_render_layout(scope, ui.root_render(), host(120.0), 0)
        .unwrap();
    let inner_style = host(50.0);
    let inner = ui
        .append_render_layout(scope, outer, inner_style.clone(), 0)
        .unwrap();
    let leaf_style = LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0));
    let leaf = ui
        .append_render_layout(scope, inner, leaf_style.clone(), 1)
        .unwrap();
    let inner_source = ui.state(scope, 0, || inner_style.clone()).unwrap();
    let leaf_source = ui.state(scope, 1, || leaf_style.clone()).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    // Register dependent first to prove execution does not follow insertion order.
    responsive
        .bind_container(
            &mut ui,
            leaf_source,
            leaf,
            inner,
            rules(
                leaf_style.clone(),
                70.0,
                leaf_style
                    .clone()
                    .size(LayoutDimension::Px(20.0), LayoutDimension::Px(30.0)),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            inner_source,
            inner,
            outer,
            rules(inner_style, 100.0, host(80.0)),
        )
        .unwrap();
    let environment = LayoutEnvironment::new([300.0, 120.0]);
    let frame = responsive.frame(&mut ui, &environment).unwrap();
    assert_eq!(frame.changed_styles, 2);
    assert_eq!(frame.layout_runs, 3);
    assert_eq!(ui.layout_result(inner).unwrap().size, [80.0, 30.0]);
    assert_eq!(ui.layout_result(outer).unwrap().size, [120.0, 30.0]);
    assert!(frame.layout_stats.layout_writes > frame.frame.layout_stats.layout_writes);
    let idle = responsive.frame(&mut ui, &environment).unwrap();
    assert_eq!((idle.changed_styles, idle.layout_runs), (0, 0));
    assert_eq!(idle.layout_stats, LayoutStats::default());
    assert!(Arc::ptr_eq(&frame.frame.items, &idle.frame.items));
}

#[test]
fn unsafe_hosts_and_foreign_targets_reject_before_sink_registration() {
    for style in [
        LayoutStyle::column(),
        LayoutStyle::column().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
    ] {
        let mut ui = setup();
        let scope = ui.root_scope();
        let container = ui
            .append_render_layout(scope, ui.root_render(), style, 0)
            .unwrap();
        let target = ui
            .append_render_layout(scope, container, LayoutStyle::row(), 0)
            .unwrap();
        let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        assert_eq!(
            responsive.bind_container(
                &mut ui,
                source,
                target,
                container,
                rules(LayoutStyle::row(), 50.0, LayoutStyle::column())
            ),
            Err(Error::UnsafeContainerQuery)
        );
        assert_eq!(ui.reactive().binding_count(), 0);
    }
    let mut ui = setup();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, ui.root_render(), host(80.0), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, ui.root_render(), LayoutStyle::row(), 0)
        .unwrap();
    let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    assert_eq!(
        responsive.bind_container(
            &mut ui,
            source,
            sibling,
            container,
            rules(LayoutStyle::row(), 50.0, LayoutStyle::column())
        ),
        Err(Error::UnsafeContainerQuery)
    );
    assert_eq!(ui.reactive().binding_count(), 0);
}

#[test]
fn host_producer_branches_and_runtime_topology_are_revalidated_before_changes() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, ui.root_render(), host(80.0), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let target_source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let host_source = ui.state(scope, 1, || host(80.0)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            target_source,
            target,
            container,
            rules(LayoutStyle::row(), 50.0, LayoutStyle::column()),
        )
        .unwrap();
    assert_eq!(
        responsive.bind(
            &mut ui,
            host_source,
            container,
            rules(host(80.0), 50.0, LayoutStyle::column())
        ),
        Err(Error::UnsafeContainerQuery)
    );
    assert_eq!(ui.reactive().binding_count(), 1);
    let environment = LayoutEnvironment::new([300.0, 120.0]);
    let before = responsive.frame(&mut ui, &environment).unwrap();
    let root = ui.root_render();
    ui.edit(|_| Ok(()), |_, draft| draft.reparent(target, root))
        .unwrap();
    assert!(matches!(
        responsive.frame(&mut ui, &environment),
        Err(Error::UnsafeContainerQuery)
    ));
    assert_eq!(
        ui.published_frame().unwrap().revision,
        before.frame.revision
    );
}

#[test]
fn stale_query_host_errors_instead_of_silently_dropping_the_rule() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, ui.root_render(), host(80.0), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            target,
            container,
            rules(LayoutStyle::row(), 50.0, LayoutStyle::column()),
        )
        .unwrap();
    let root = ui.root_render();
    ui.edit(
        |_| Ok(()),
        |_, draft| {
            draft.reparent(target, root)?;
            draft.remove(container)
        },
    )
    .unwrap();
    assert!(matches!(
        responsive.frame(&mut ui, &LayoutEnvironment::new([300.0, 120.0])),
        Err(Error::InvalidNode)
    ));
    assert_eq!(
        ui.read(source).unwrap().flex_direction,
        LayoutFlexDirection::Row
    );
}

#[test]
fn viewport_host_updates_precede_exact_content_threshold_and_hidden_restore() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let narrow = host(70.0).padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let wide = host(90.0).padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let hidden = wide.clone().display(LayoutDisplay::None);
    let container = ui
        .append_render_layout(scope, ui.root_render(), narrow.clone(), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let host_source = ui.state(scope, 0, || narrow.clone()).unwrap();
    let source = ui.state(scope, 1, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    // A host producer may be registered before its consumer.
    responsive
        .bind(
            &mut ui,
            host_source,
            container,
            LayoutBreakpointStyles::new(narrow, vec![(300.0, wide), (400.0, hidden)]).unwrap(),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            source,
            target,
            container,
            rules(LayoutStyle::row(), 80.0, LayoutStyle::column()),
        )
        .unwrap();
    for (viewport, direction) in [
        (299.0, LayoutFlexDirection::Row),
        (300.0, LayoutFlexDirection::Column),
        (400.0, LayoutFlexDirection::Row),
        (300.0, LayoutFlexDirection::Column),
    ] {
        responsive
            .frame(&mut ui, &LayoutEnvironment::new([viewport, 120.0]))
            .unwrap();
        assert_eq!(ui.read(source).unwrap().flex_direction, direction);
    }
}

#[test]
fn invalid_host_producer_registered_first_cannot_admit_a_consumer() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, ui.root_render(), host(80.0), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let host_source = ui.state(scope, 0, || host(80.0)).unwrap();
    let source = ui.state(scope, 1, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            host_source,
            container,
            rules(host(80.0), 400.0, LayoutStyle::column()),
        )
        .unwrap();
    assert_eq!(
        responsive.bind_container(
            &mut ui,
            source,
            target,
            container,
            rules(LayoutStyle::row(), 50.0, LayoutStyle::column())
        ),
        Err(Error::UnsafeContainerQuery)
    );
    assert_eq!(ui.reactive().binding_count(), 1);
    assert_eq!(responsive.len(), 1);
}

#[test]
fn runtime_inline_feedback_fails_before_viewport_sources_or_frames_change() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let container = ui
        .append_render_layout(scope, ui.root_render(), host(80.0), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, ui.root_render(), LayoutStyle::row(), 0)
        .unwrap();
    let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let viewport_source = ui.state(scope, 1, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            target,
            container,
            rules(LayoutStyle::row(), 50.0, LayoutStyle::column()),
        )
        .unwrap();
    responsive
        .bind(
            &mut ui,
            viewport_source,
            sibling,
            rules(LayoutStyle::row(), 400.0, LayoutStyle::column()),
        )
        .unwrap();
    let before = responsive
        .frame(&mut ui, &LayoutEnvironment::new([300.0, 120.0]))
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |_, draft| {
            draft
                .properties()
                .layout_style(container, LayoutStyle::column());
            Ok(())
        },
    )
    .unwrap();
    assert!(matches!(
        responsive.frame(&mut ui, &LayoutEnvironment::new([500.0, 120.0])),
        Err(Error::UnsafeContainerQuery)
    ));
    assert_eq!(
        ui.read(viewport_source).unwrap().flex_direction,
        LayoutFlexDirection::Row
    );
    assert_eq!(
        ui.published_frame().unwrap().revision,
        before.frame.revision
    );
}

#[test]
fn query_content_width_subtracts_allocated_scrollbar_gutter() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let mut style = host(100.0).to_taffy().unwrap();
    style.overflow.y = taffy::Overflow::Scroll;
    style.scrollbar_width = 12.0;
    let container = ui.append_render(scope, ui.root_render(), style, 0).unwrap();
    let target = ui
        .append_render_layout(scope, container, LayoutStyle::row(), 0)
        .unwrap();
    let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            target,
            container,
            rules(LayoutStyle::row(), 89.0, LayoutStyle::column()),
        )
        .unwrap();
    responsive
        .frame(&mut ui, &LayoutEnvironment::new([300.0, 120.0]))
        .unwrap();
    assert_eq!(
        ui.read(source).unwrap().flex_direction,
        LayoutFlexDirection::Row
    );
}
