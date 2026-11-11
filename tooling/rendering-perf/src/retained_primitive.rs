use super::*;

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0x171d28ff,
    )?;
    let node = tree.append(
        tree.root(),
        Style {
            size: taffy::Size {
                width: length(80.0),
                height: length(48.0),
            },
            flex_shrink: 0.0,
            ..Default::default()
        },
        0x4488ffaa,
    )?;
    let mut tx = Transaction::default();
    tx.translation(node, [8.25, 8.25]);
    tx.decoration(
        node,
        QuadDecoration {
            radius: 16.0,
            border_width: 3.0,
            border_color: 0xff8844ff,
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let before = tree.frame([160.0, 96.0])?;
    renderer.submit(&before, [200, 120], 1.25)?;
    let image = renderer.image()?;
    assert_eq!(
        image.get_pixel(11, 11),
        image.get_pixel(1, 1),
        "rounded corner must preserve background"
    );
    assert_ne!(
        image.get_pixel(50, 11),
        image.get_pixel(50, 35),
        "inside border must differ from fill"
    );
    compare(
        renderer,
        &before,
        [200, 120],
        1.25,
        "primitive-rounded-border",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.color(node, 0x44cc88aa);
    let result = tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    assert_eq!(
        result.decisions[0].reason,
        RouteReason::StableSolidQuadColor
    );
    let frame = tree.frame([160.0, 96.0])?;
    assert_eq!(frame.recorded_chunks, 0);
    assert!(std::sync::Arc::ptr_eq(
        &before.items[1].commands,
        &frame.items[1].commands
    ));
    assert_eq!(
        renderer
            .submit(&frame, [200, 120], 1.25)?
            .unwrap()
            .gpu
            .uploaded_bytes,
        160
    );
    compare(
        renderer,
        &frame,
        [200, 120],
        1.25,
        "primitive-color-patch",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.color(node, 0);
    tx.decoration(
        node,
        QuadDecoration {
            radius: 8.0,
            border_width: 2.0,
            border_color: 0xff8844ff,
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([160.0, 96.0])?;
    assert_eq!(frame.layout_runs, 0);
    assert!(frame.damage.bounds.is_some());
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&frame, [200, 120], 1.25)?;
    compare(
        renderer,
        &frame,
        [200, 120],
        1.25,
        "primitive-transparent-border",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.decoration(
        node,
        QuadDecoration {
            radius: 1000.0,
            border_width: 1000.0,
            border_color: 0xaacc44ff,
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([160.0, 96.0])?;
    renderer.submit(&frame, [200, 120], 1.25)?;
    compare(
        renderer,
        &frame,
        [200, 120],
        1.25,
        "primitive-clamped-decoration",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.decoration(node, QuadDecoration::default());
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([160.0, 96.0])?;
    renderer.submit(&frame, [200, 120], 1.25)?;
    compare(
        renderer,
        &frame,
        [200, 120],
        1.25,
        "primitive-remove-decoration",
        output,
        results,
    )?;
    renderer.set_policy(CompositorPolicy::Full)?;
    Ok(())
}
