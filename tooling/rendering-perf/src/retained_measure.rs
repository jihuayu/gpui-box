use super::*;
use gpui_box::retained::{logical::LogicalUi, reactive::Property};

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut ui = LogicalUi::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            align_items: taffy::AlignItems::START,
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0x171d28ff,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let root = ui.root_scope();
    let nodes = (0..3)
        .map(|i| {
            ui.append_render(
                root,
                ui.root_render(),
                Style::default(),
                [0xff8844ff, 0x44bb88ff, 0x4488ffff][i],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let initial = Some(MeasureSnapshot::Image(ImageIntrinsicSize {
        width: 40.0,
        height: 20.0,
    }));
    let measure = ui.state(root, 0, || initial.clone())?;
    ui.bind(measure, Property::measure(nodes[0]))?;
    for (key, node) in nodes[1..].iter().enumerate() {
        let value = ui.state(root, key as u64 + 1, || {
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 32.0,
                height: 16.0,
                baseline: None,
            }))
        })?;
        ui.bind(value, Property::measure(*node))?;
    }
    let before = ui.frame([160.0, 96.0])?;
    renderer.submit(&before, [160, 96], 1.0)?;
    compare(
        renderer,
        &before,
        [160, 96],
        1.0,
        "measure-initial",
        output,
        results,
    )?;
    ui.transaction(|tx| {
        tx.set(
            measure,
            Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                width: 64.0,
                height: 32.0,
            })),
        )?;
        Ok(())
    })?;
    let changed = ui.frame([160.0, 96.0])?;
    assert_eq!((changed.layout_runs, changed.recorded_chunks), (1, 0));
    assert!(changed.layout_stats.measure_calls < before.layout_stats.measure_calls);
    assert_eq!(changed.hit_test(8.0, 35.0), Some(nodes[1]));
    renderer.submit(&changed, [160, 96], 1.0)?;
    compare(
        renderer,
        &changed,
        [160, 96],
        1.0,
        "measure-resize-dependents",
        output,
        results,
    )?;
    let result = ui.transaction(|tx| {
        tx.set(measure, initial.clone())?;
        tx.set(
            measure,
            Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                width: 64.0,
                height: 32.0,
            })),
        )?;
        Ok(())
    })?;
    assert!(!result.commit.request_frame);
    assert!(
        renderer
            .submit(&ui.frame([160.0, 96.0])?, [160, 96], 1.0)?
            .is_none()
    );
    let resized = ui.frame([200.0, 100.0])?;
    renderer.submit(&resized, [250, 125], 1.25)?;
    compare(
        renderer,
        &resized,
        [250, 125],
        1.25,
        "measure-fractional-scale",
        output,
        results,
    )?;
    for i in 0..32 {
        ui.transaction(|tx| {
            tx.set(
                measure,
                Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                    width: 30.0 + i as f32,
                    height: 10.0 + i as f32,
                })),
            )?;
            Ok(())
        })?;
        let frame = ui.frame([200.0, 100.0])?;
        assert!(
            renderer
                .submit(&frame, [250, 125], 1.25)?
                .unwrap()
                .gpu
                .in_flight
                <= 3
        );
    }
    compare(
        renderer,
        &ui.frame([200.0, 100.0])?,
        [250, 125],
        1.25,
        "measure-queued-inputs",
        output,
        results,
    )?;
    Ok(())
}
