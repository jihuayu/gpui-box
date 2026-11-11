use super::*;
use gpui_box::retained::frame_queue::FrameQueue;

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let queue = FrameQueue::new();
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
    let before = tree.frame([160.0, 96.0])?;
    queue.publish(before.clone())?;
    renderer.submit(&queue.take_latest()?.unwrap(), [160, 96], 1.0)?;
    let weak = std::sync::Arc::downgrade(&before.items);
    let mut newest = None;
    let mut wakes = 0;
    for i in 0..64 {
        let mut tx = Transaction::default();
        tx.color(
            tree.root(),
            if i % 2 == 0 { 0x4488ffff } else { 0x44bb88ff },
        );
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let frame = tree.frame([160.0, 96.0])?;
        wakes += usize::from(queue.publish(frame.clone())?);
        newest = Some(frame);
    }
    assert_eq!(wakes, 1);
    assert_eq!((queue.stats()?.coalesced, queue.stats()?.pending), (63, 1));
    let frame = queue.take_latest()?.unwrap();
    let submit = renderer.submit(&frame, [160, 96], 1.0)?.unwrap();
    assert!(submit.full_resync, "skipped base must resynchronize");
    assert!(submit.gpu.in_flight <= 3);
    compare(
        renderer,
        &frame,
        [160, 96],
        1.0,
        "queue-latest-resync",
        output,
        results,
    )?;
    results.last_mut().unwrap()["producer_coalesced"] = 63.into();
    results.last_mut().unwrap()["producer_wakes"] = wakes.into();
    assert!(!queue.publish(newest.unwrap())?);
    drop(before);
    renderer.finish()?;
    assert!(
        weak.upgrade().is_none(),
        "finished GPU frames must release old snapshots"
    );
    assert!(queue.take_latest()?.is_none());
    assert!(renderer.submit(&frame, [160, 96], 1.0)?.is_none());
    Ok(())
}
