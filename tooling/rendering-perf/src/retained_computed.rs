//! Real dependency propagation through one retained packet and GPU submit.
use super::*;
use gpui_box::retained::{logical::LogicalUi, reactive::Property};

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
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let scope = ui.root_scope();
    let input = ui.state(scope, 0, || 0u32)?;
    let bucket = ui.computed(scope, 1, &[input.dependency()], move |i| {
        Ok(*i.read(input)? / 4)
    })?;
    let color = ui.computed(scope, 2, &[bucket.dependency()], move |i| {
        Ok(if *i.read(bucket)? % 2 == 0 {
            0x245fc7ff
        } else {
            0xcc7733ff
        })
    })?;
    let translation = ui.computed(scope, 3, &[bucket.dependency()], move |i| {
        Ok([*i.read(bucket)? as f32 * 4.0, 0.0])
    })?;
    let observed = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    let events = observed.clone();
    ui.effect(
        scope,
        0,
        &[
            bucket.dependency(),
            color.dependency(),
            translation.dependency(),
        ],
        move |i| {
            let bucket = *i.read(bucket).unwrap();
            assert_eq!(i.read(translation).unwrap()[0], bucket as f32 * 4.0);
            assert_eq!(
                *i.read(color).unwrap(),
                if bucket % 2 == 0 {
                    0x245fc7ff
                } else {
                    0xcc7733ff
                }
            );
            events.borrow_mut().push(("run", bucket));
            let events = events.clone();
            Some(Box::new(move || {
                events.borrow_mut().push(("cleanup", bucket))
            }))
        },
    )?;
    let style = Style {
        size: taffy::Size {
            width: length(20.0),
            height: length(20.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    for _ in 0..10 {
        let node = ui.append_render(scope, ui.root_render(), style.clone(), 0x245fc7ff)?;
        ui.bind(color, Property::color(node))?;
        ui.bind(translation, Property::translation(node))?;
    }
    let size = [320, 80];
    let initial = ui.frame([320.0, 80.0])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&initial, size, 1.0)?;
    let commit = ui.transaction(|tx| {
        tx.set(input, 3)?;
        Ok(())
    })?;
    assert_eq!(
        (
            commit.computed_evaluations,
            commit.computed_equal,
            commit.sink_writes
        ),
        (1, 1, 0)
    );
    assert!(!commit.commit.request_frame);
    assert_eq!((commit.effect_runs, commit.effect_cleanups), (0, 0));
    let equal = ui.frame([320.0, 80.0])?;
    assert!(std::sync::Arc::ptr_eq(&initial.items, &equal.items));
    assert!(renderer.submit(&equal, size, 1.0)?.is_none());
    compare(
        renderer,
        &equal,
        size,
        1.0,
        "computed-equality-cutoff",
        output,
        results,
    )?;
    let commit = ui.transaction(|tx| {
        tx.set(input, 8)?;
        tx.transaction(|tx| {
            tx.set(input, 4)?;
            Ok(())
        })?;
        Ok(())
    })?;
    assert_eq!(
        (
            commit.computed_evaluations,
            commit.sink_writes,
            commit.commit.changed_nodes
        ),
        (3, 20, 10)
    );
    assert_eq!(commit.commit.revision, initial.revision + 1);
    assert_eq!((commit.effect_runs, commit.effect_cleanups), (1, 1));
    let frame = ui.frame([320.0, 80.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    let submitted = renderer.submit(&frame, size, 1.0)?.unwrap();
    assert_eq!(submitted.gpu.uploaded_bytes, 1600);
    compare(
        renderer,
        &frame,
        size,
        1.0,
        "computed-material-property-batch",
        output,
        results,
    )?;
    results.last_mut().unwrap()["derived_evaluations"] = commit.computed_evaluations.into();
    results.last_mut().unwrap()["sink_writes"] = commit.sink_writes.into();
    results.last_mut().unwrap()["uploaded_bytes"] = submitted.gpu.uploaded_bytes.into();
    results.last_mut().unwrap()["post_commit_effect_runs"] = commit.effect_runs.into();
    let commit = ui.transaction(|tx| {
        tx.set(input, 12)?;
        assert_eq!(*tx.read(bucket)?, 3);
        tx.set(input, 4)?;
        Ok(())
    })?;
    assert_eq!((commit.computed_evaluations, commit.sink_writes), (6, 0));
    assert!(!commit.commit.request_frame);
    let restored = ui.frame([320.0, 80.0])?;
    assert!(std::sync::Arc::ptr_eq(&frame.items, &restored.items));
    assert!(renderer.submit(&restored, size, 1.0)?.is_none());
    compare(
        renderer,
        &restored,
        size,
        1.0,
        "computed-read-restore",
        output,
        results,
    )?;
    let opacity = ui.computed(scope, 4, &[input.dependency()], move |i| {
        Ok(*i.read(input)? as f32 / 4.0)
    })?;
    ui.bind(opacity, Property::opacity(ui.root_render()))?;
    let before = ui.frame([320.0, 80.0])?;
    renderer.submit(&before, size, 1.0)?;
    assert!(matches!(
        ui.transaction(|tx| {
            tx.set(input, 20)?;
            Ok(())
        }),
        Err(gpui_box::retained::Error::InvalidOpacity)
    ));
    assert_eq!(*ui.read(input)?, 4);
    assert_eq!(*ui.read(bucket)?, 1);
    let after = ui.frame([320.0, 80.0])?;
    assert_eq!(before.revision, after.revision);
    assert_eq!(*observed.borrow(), [("run", 0), ("cleanup", 0), ("run", 1)]);
    assert!(renderer.submit(&after, size, 1.0)?.is_none());
    compare(
        renderer,
        &after,
        size,
        1.0,
        "computed-invalid-sink-rollback",
        output,
        results,
    )?;
    drop(ui);
    assert_eq!(observed.borrow().last(), Some(&("cleanup", 1)));
    Ok(())
}
