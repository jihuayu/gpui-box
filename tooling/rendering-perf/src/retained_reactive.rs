//! Signal → typed sink → owned packet → persistent GPU upload validation.
use super::*;
use gpui_box::retained::reactive::{Property, ReactiveUi};

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let style = Style {
        size: taffy::Size {
            width: length(4.0),
            height: length(4.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0x171d28ff,
    )?;
    let nodes = (0..10_000)
        .map(|_| tree.append(tree.root(), style.clone(), 0x245fc7ff))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ui = ReactiveUi::new(
        tree,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let mut signals = vec![];
    for node in &nodes {
        let signal = ui.signal(0x245fc7ff);
        ui.bind(signal, Property::color(*node))?;
        signals.push(signal);
    }
    let before = ui.frame([800.0, 400.0])?;
    renderer.set_policy(CompositorPolicy::Full)?;
    renderer.submit(&before, [800, 400], 1.0)?;
    let commit = ui.transaction(|tx| {
        for signal in &signals {
            tx.set(*signal, 0xaa55ccff)?;
        }
        tx.transaction(|tx| {
            tx.set(signals[0], 0x33cc88ff)?;
            Ok(())
        })
    })?;
    assert_eq!(
        (
            commit.changed_sources,
            commit.sink_writes,
            commit.commit.changed_nodes
        ),
        (10_000, 10_000, 10_000)
    );
    assert_eq!(ui.stats().frame_requests, 1);
    let frame = ui.frame([800.0, 400.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 10_000));
    let submitted = renderer.submit(&frame, [800, 400], 1.0)?.unwrap();
    assert_eq!(submitted.gpu.uploaded_bytes, 1_600_000);
    assert!((1..=2).contains(&submitted.gpu.upload_ranges));
    compare(
        renderer,
        &frame,
        [800, 400],
        1.0,
        "reactive-batch-10000",
        output,
        results,
    )?;
    results.last_mut().unwrap()["sink_writes"] = commit.sink_writes.into();
    results.last_mut().unwrap()["frame_requests"] = ui.stats().frame_requests.into();
    let stats = ui.stats();
    ui.transaction(|tx| {
        tx.set(signals[0], 0xffffffff)?;
        tx.transaction(|tx| {
            tx.set(signals[0], 0x33cc88ff)?;
            Ok(())
        })
    })?;
    assert_eq!(
        (ui.stats().sink_writes, ui.stats().frame_requests),
        (stats.sink_writes, stats.frame_requests)
    );
    let equal = ui.frame([800.0, 400.0])?;
    assert!(std::sync::Arc::ptr_eq(&frame.items, &equal.items));
    assert!(renderer.submit(&equal, [800, 400], 1.0)?.is_none());
    results.last_mut().unwrap()["restore_schedules_no_gpu_work"] = true.into();
    Ok(())
}
