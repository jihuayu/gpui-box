use super::*;
use gpui_box::retained::{logical::LogicalUi, reactive::Property};

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let absolute = |x, y, width, height| Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(x),
            top: length(y),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(width),
            height: length(height),
        },
        ..Default::default()
    };
    let mut ui = LogicalUi::new(
        absolute(0.0, 0.0, 160.0, 96.0),
        0x171d28ff,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let root = ui.root_scope();
    let scopes = ui.reconcile_keys(root, &[10, 20, 30])?.order;
    let host = ui.append_render(root, ui.root_render(), absolute(0.0, 0.0, 160.0, 96.0), 0)?;
    let red = ui.state(scopes[0], 0, || 0xff000080)?;
    let a = ui.append_render(scopes[0], host, absolute(8.0, 8.0, 48.0, 48.0), 0xff000080)?;
    ui.bind(red, Property::color(a))?;
    let b = ui.append_render(scopes[1], host, absolute(8.0, 8.0, 48.0, 48.0), 0x0000ff80)?;
    let extra = ui.append_render(scopes[0], host, absolute(72.0, 8.0, 24.0, 24.0), 0x33cc88ff)?;
    ui.sync_render_order()?;
    let before = ui.frame([160.0, 96.0])?;
    renderer.submit(&before, [160, 96], 1.0)?;
    let initial_image = renderer.image()?;
    assert_eq!(before.hit_test(16.0, 16.0), Some(b));
    compare(
        renderer,
        &before,
        [160, 96],
        1.0,
        "logical-portal-fragment",
        output,
        results,
    )?;
    ui.reconcile_keys(root, &[30, 20, 10])?;
    let reordered = ui.frame([160.0, 96.0])?;
    assert_eq!(reordered.recorded_chunks, 0);
    assert_eq!(reordered.hit_test(16.0, 16.0), Some(a));
    for item in reordered.items.iter() {
        let old = before
            .items
            .iter()
            .find(|old| old.node == item.node)
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&old.commands, &item.commands));
    }
    renderer.submit(&reordered, [160, 96], 1.0)?;
    assert!(renderer.image()?.as_raw() != initial_image.as_raw());
    compare(
        renderer,
        &reordered,
        [160, 96],
        1.0,
        "logical-keyed-order",
        output,
        results,
    )?;
    ui.transaction(|tx| {
        tx.set(red, 0xffaa4480)?;
        Ok(())
    })?;
    let changed = ui.frame([160.0, 96.0])?;
    assert_eq!(
        renderer
            .submit(&changed, [160, 96], 1.0)?
            .unwrap()
            .gpu
            .uploaded_bytes,
        160
    );
    compare(
        renderer,
        &changed,
        [160, 96],
        1.0,
        "logical-retained-state",
        output,
        results,
    )?;
    let removed = ui.reconcile_keys(root, &[20, 30, 40])?;
    assert!(removed.removed.contains(&scopes[0]));
    assert!(ui.read(red).is_err());
    let frame = ui.frame([160.0, 96.0])?;
    assert!(
        !frame
            .items
            .iter()
            .any(|item| item.node == a || item.node == extra)
    );
    renderer.submit(&frame, [160, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [160, 96],
        1.0,
        "logical-retire-portal",
        output,
        results,
    )?;
    let new = removed.inserted[0];
    let fresh = ui.state(new, 0, || 0x8899aaff)?;
    assert_ne!(fresh, red);
    let node = ui.append_render(new, host, absolute(8.0, 8.0, 48.0, 48.0), *ui.read(fresh)?)?;
    ui.bind(fresh, Property::color(node))?;
    ui.sync_render_order()?;
    let frame = ui.frame([160.0, 96.0])?;
    renderer.submit(&frame, [160, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [160, 96],
        1.0,
        "logical-reuse-generation",
        output,
        results,
    )?;
    for i in 0..32 {
        ui.reconcile_keys(
            root,
            if i % 2 == 0 {
                &[40, 30, 20]
            } else {
                &[20, 30, 40]
            },
        )?;
        let frame = ui.frame([160.0, 96.0])?;
        if let Some(submit) = renderer.submit(&frame, [160, 96], 1.0)? {
            assert!(submit.gpu.in_flight <= 3);
        }
    }
    compare(
        renderer,
        &ui.frame([160.0, 96.0])?,
        [160, 96],
        1.0,
        "logical-queued-order",
        output,
        results,
    )?;
    Ok(())
}
