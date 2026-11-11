use super::*;
use gpui_box::retained::{logical::LogicalUi, responsive::*};

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut ui = LogicalUi::new(
        Style {
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0x171d28ff,
        BackendCapabilities::default(),
    )?;
    let root = ui.root_scope();
    let scope = ui.reconcile_keys(root, &[1])?.order[0];
    let target = ui.append_render(scope, ui.root_render(), Style::default(), 0x334455ff)?;
    let source = ui.state(scope, 0, Style::default)?;
    let mut responsive = ResponsiveBindings::new();
    responsive.bind(
        &mut ui,
        source,
        target,
        StyleQuery::ViewportInline,
        BreakpointStyles::new(FlexStyle::column(), vec![(200.0, FlexStyle::row())])?,
    )?;
    let mut nodes = vec![];
    for color in [0xff8844ff, 0x44bb88ff, 0x4488ffff] {
        nodes.push(ui.append_render(
            scope,
            target,
            Style {
                size: taffy::Size {
                    width: length(32.0),
                    height: length(24.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            color,
        )?);
    }
    for (width, direction, name) in [
        (199, taffy::Direction::Ltr, "responsive-compact"),
        (200, taffy::Direction::Ltr, "responsive-threshold"),
        (240, taffy::Direction::Ltr, "responsive-fluid"),
        (240, taffy::Direction::Rtl, "responsive-rtl"),
    ] {
        let mut environment = LayoutEnvironment::new([width as f32, 96.0]);
        environment.direction = direction;
        let result = responsive.frame(&mut ui, &environment)?;
        renderer.submit(&result.frame, [width, 96], 1.0)?;
        compare(
            renderer,
            &result.frame,
            [width, 96],
            1.0,
            name,
            output,
            results,
        )?;
        for node in &nodes {
            assert!(result.frame.items.iter().any(|item| item.node == *node));
        }
        if name == "responsive-fluid" {
            assert_eq!(result.changed_styles, 0);
        }
    }
    let mut environment = LayoutEnvironment::new([240.0, 96.0]);
    environment.direction = taffy::Direction::Rtl;
    let idle = responsive.frame(&mut ui, &environment)?;
    assert_eq!((idle.layout_runs, idle.changed_styles), (0, 0));
    assert!(renderer.submit(&idle.frame, [240, 96], 1.0)?.is_none());
    ui.reconcile_keys(root, &[])?;
    let retired = responsive.frame(&mut ui, &environment)?;
    assert!(responsive.is_empty());
    renderer.submit(&retired.frame, [240, 96], 1.0)?;
    compare(
        renderer,
        &retired.frame,
        [240, 96],
        1.0,
        "responsive-retire",
        output,
        results,
    )?;

    let host = ui.append_render(
        root,
        ui.root_render(),
        Style {
            size: taffy::Size {
                width: length(140.0),
                height: length(80.0),
            },
            min_size: taffy::Size {
                width: length(140.0),
                height: length(80.0),
            },
            max_size: taffy::Size {
                width: length(140.0),
                height: length(80.0),
            },
            padding: taffy::Rect {
                left: length(10.0),
                right: length(10.0),
                ..taffy::Rect::zero()
            },
            flex_shrink: 0.0,
            ..Default::default()
        },
        0x334455ff,
    )?;
    let target = ui.append_render(root, host, Style::default(), 0)?;
    let source = ui.state(root, 0, Style::default)?;
    responsive.bind(
        &mut ui,
        source,
        target,
        StyleQuery::ContainerInline(host),
        BreakpointStyles::new(FlexStyle::column(), vec![(130.0, FlexStyle::row())])?,
    )?;
    for color in [0xff8844ff, 0x44bb88ff] {
        ui.append_render(
            root,
            target,
            Style {
                size: taffy::Size {
                    width: length(24.0),
                    height: length(24.0),
                },
                ..Default::default()
            },
            color,
        )?;
    }
    let result = responsive.frame(&mut ui, &LayoutEnvironment::new([240.0, 96.0]))?;
    assert_eq!(
        ui.read(source)?.flex_direction,
        taffy::FlexDirection::Column
    );
    assert_eq!(result.layout_runs, 2);
    renderer.submit(&result.frame, [240, 96], 1.0)?;
    compare(
        renderer,
        &result.frame,
        [240, 96],
        1.0,
        "responsive-content-box",
        output,
        results,
    )?;
    ui.transaction(|tx| {
        tx.set(
            source,
            Style {
                display: taffy::Display::None,
                ..Default::default()
            },
        )?;
        Ok(())
    })?;
    let hidden = ui.frame([240.0, 96.0])?;
    assert_eq!(hidden.items.len(), 2);
    renderer.submit(&hidden, [240, 96], 1.0)?;
    compare(
        renderer,
        &hidden,
        [240, 96],
        1.0,
        "responsive-hidden-subtree",
        output,
        results,
    )?;
    Ok(())
}
