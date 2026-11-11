//! Background value inbox → one owner transaction → persistent GPU packet.
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
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 0u32)?;
    let color = ui.computed(root, 1, &[input.dependency()], move |i| {
        Ok(if *i.read(input)? % 2 == 0 {
            0x245fc7ff
        } else {
            0xcc7733ff
        })
    })?;
    let observed = std::rc::Rc::new(std::cell::Cell::new(0));
    let observer = observed.clone();
    ui.effect(root, 0, &[color.dependency()], move |_| {
        observer.set(observer.get() + 1);
        None
    })?;
    let node = ui.append_render(
        root,
        ui.root_render(),
        Style {
            size: taffy::Size {
                width: length(64.0),
                height: length(48.0),
            },
            ..Default::default()
        },
        0,
    )?;
    ui.bind(color, Property::color(node))?;
    let mut receiver = ui.open_updates(8)?;
    let sender = ui.update_sender(&receiver, input)?;
    let before = ui.frame([160.0, 80.0])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&before, [160, 80], 1.0)?;
    let worker = sender.clone();
    std::thread::spawn(move || {
        for value in 1..=999 {
            worker.send(value).unwrap();
        }
    })
    .join()
    .unwrap();
    assert_eq!(ui.stats().transactions, 0);
    assert_eq!(receiver.stats()?.coalesced, 998);
    let batch = ui.apply_updates(&mut receiver)?;
    let commit = batch.commit.unwrap();
    assert_eq!(
        (
            commit.computed_evaluations,
            commit.sink_writes,
            commit.effect_runs
        ),
        (1, 1, 1)
    );
    assert_eq!(commit.commit.revision, before.revision + 1);
    assert_eq!(observed.get(), 2);
    let frame = ui.frame([160.0, 80.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    let submission = renderer.submit(&frame, [160, 80], 1.0)?.unwrap();
    assert_eq!(submission.gpu.uploaded_bytes, 160);
    compare(
        renderer,
        &frame,
        [160, 80],
        1.0,
        "delivery-worker-burst",
        output,
        results,
    )?;
    results.last_mut().unwrap()["source_messages"] = 999.into();
    results.last_mut().unwrap()["owner_transactions"] = 1.into();
    results.last_mut().unwrap()["instance_bytes"] = 160.into();
    sender.send(1001)?;
    assert!(
        !ui.apply_updates(&mut receiver)?
            .commit
            .unwrap()
            .commit
            .request_frame
    );
    let equal = ui.frame([160.0, 80.0])?;
    assert!(renderer.submit(&equal, [160, 80], 1.0)?.is_none());
    compare(
        renderer,
        &equal,
        [160, 80],
        1.0,
        "delivery-derived-equal",
        output,
        results,
    )?;
    let scopes = ui.reconcile_keys(root, &[1])?.order;
    let retired = ui.state(scopes[0], 0, || 0u32)?;
    let stale = ui.update_sender(&receiver, retired)?;
    stale.send(42)?;
    ui.reconcile_keys(root, &[])?;
    let batch = ui.apply_updates(&mut receiver)?;
    assert_eq!((batch.applied, batch.retired), (0, 1));
    assert!(batch.commit.is_none());
    assert!(stale.send(43).is_err());
    let after = ui.frame([160.0, 80.0])?;
    assert!(renderer.submit(&after, [160, 80], 1.0)?.is_none());
    compare(
        renderer,
        &after,
        [160, 80],
        1.0,
        "delivery-retired-source",
        output,
        results,
    )?;
    drop(ui);
    assert!(sender.stats()?.closed);
    Ok(())
}
