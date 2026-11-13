use super::*;

fn style(w: f32, h: f32) -> Style {
    Style {
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}
fn options(size: [u32; 2]) -> SoftwareOptions {
    SoftwareOptions {
        size,
        scale: 1.0,
        background: 0,
        effects: SoftwareEffects::Exact,
        max_bytes: 16 * 1024 * 1024,
    }
}
fn finish(packet: FramePacket, options: SoftwareOptions, quota: usize) -> SoftwareFrame {
    let mut job = SoftwareJob::new(packet, options).unwrap();
    assert!(!job.step(0).unwrap());
    while !job.step(quota).unwrap() {}
    job.into_frame().unwrap()
}
fn pixel(frame: &SoftwareFrame, x: usize, y: usize) -> [u8; 4] {
    frame.pixels()[(y * frame.size[0] as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn partial_work_never_publishes_and_shown_geometry_keeps_old_revision() {
    let mut tree = UiTree::new(style(50.0, 36.0), 0xff0000ff).unwrap();
    let before = tree.frame([64.0; 2]).unwrap();
    let shown = finish(before.clone(), options([64; 2]), 17);
    let mut tx = Transaction::default();
    tx.translation(tree.root(), [8.0, 0.0]);
    tx.color(tree.root(), 0x0000ffff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let after = tree.frame([64.0; 2]).unwrap();
    let mut job = SoftwareJob::new(after.clone(), options([64; 2])).unwrap();
    assert!(!job.step(100).unwrap());
    assert_eq!(shown.hit_test([1.0, 1.0]), Some(tree.root()));
    assert_eq!(pixel(&shown, 1, 1), [255, 0, 0, 255]);
    assert_eq!(job.into_frame().err(), Some(SoftwareError::InvalidFrame));
    let after = finish(after, options([64; 2]), 1);
    assert_eq!(after.hit_test([1.0, 1.0]), None);
    assert_eq!(pixel(&after, 1, 1), [0; 4]);
    assert_eq!(pixel(&after, 9, 1), [0, 0, 255, 255]);
    assert!(after.packet().revision > shown.packet().revision);
    assert_eq!(
        shown.pixels(),
        finish(before, options([64; 2]), usize::MAX).pixels()
    );
}

#[test]
fn quotas_preserve_isolated_nested_opacity_and_overlap_across_tiles() {
    let mut tree = UiTree::new(style(70.0, 45.0), 0xff0000ff).unwrap();
    let child = tree
        .append(tree.root(), style(50.0, 40.0), 0x0000ffff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.opacity(tree.root(), 0.5);
    tx.opacity(child, 0.5);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let packet = tree.frame([80.0, 50.0]).unwrap();
    let small = finish(packet.clone(), options([80, 50]), 3);
    let large = finish(packet, options([80, 50]), usize::MAX);
    assert_eq!(small.pixels(), large.pixels());
    assert_eq!(pixel(&small, 35, 20), [64, 0, 64, 128]);
    assert_eq!(pixel(&small, 60, 20), [128, 0, 0, 128]);
    assert_eq!(pixel(&small, 79, 49), [0; 4]);
    assert_eq!(small.stats.tiles, 6);
}

#[test]
fn rounded_border_clip_image_and_background_use_owned_resources() {
    let mut tree = UiTree::new(style(40.0, 40.0), 0xff0000ff).unwrap();
    let mut tx = Transaction::default();
    tx.decoration(
        tree.root(),
        QuadDecoration {
            radius: 8.0,
            border_width: 3.0,
            border_color: 0x00ff00ff,
        },
    );
    tx.clip(
        tree.root(),
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 40.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = finish(tree.frame([40.0; 2]).unwrap(), options([40; 2]), 8192);
    assert_eq!(pixel(&frame, 0, 0), [0; 4]);
    assert_eq!(pixel(&frame, 20, 1), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 20, 20), [255, 0, 0, 255]);
    assert_eq!(pixel(&frame, 35, 20), [0; 4]);
    let image = ImageResource::rgba8([1, 1], vec![0, 0, 255, 128]).unwrap();
    let mut tx = Transaction::default();
    tx.image(tree.root(), Some(image));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let packet = tree.frame([40.0; 2]).unwrap();
    drop(tree);
    let frame = finish(packet, options([40; 2]), 11);
    assert_eq!(pixel(&frame, 20, 20), [127, 0, 128, 255]);
}

#[test]
fn errors_cancel_and_resources_are_admitted_before_publication() {
    let mut tree = UiTree::new(style(8.0, 8.0), 0xff0000ff).unwrap();
    let mut tx = Transaction::default();
    tx.shadows(
        tree.root(),
        vec![BoxShadow {
            blur_radius: 2.0,
            ..Default::default()
        }],
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let packet = tree.frame([8.0; 2]).unwrap();
    let mut exact = SoftwareJob::new(packet.clone(), options([8; 2])).unwrap();
    assert_eq!(exact.step(1), Err(SoftwareError::UnsupportedShadows));
    assert_eq!(exact.step(1), Err(SoftwareError::Cancelled));
    let mut permitted = options([8; 2]);
    permitted.effects = SoftwareEffects::OmitDecorativeShadows;
    let result = finish(packet.clone(), permitted, 17);
    assert_eq!(result.stats.omitted_shadows, 1);
    let mut cancelled = SoftwareJob::new(packet.clone(), permitted).unwrap();
    cancelled.step(10).unwrap();
    cancelled.cancel();
    assert_eq!(cancelled.into_frame().err(), Some(SoftwareError::Cancelled));
    let mut bad = permitted;
    bad.max_bytes = 10;
    assert_eq!(
        SoftwareJob::new(packet.clone(), bad).err(),
        Some(SoftwareError::ResourceLimit)
    );
    bad = permitted;
    bad.scale = f32::NAN;
    assert_eq!(
        SoftwareJob::new(packet.clone(), bad).err(),
        Some(SoftwareError::InvalidSurface)
    );
    bad = permitted;
    bad.size = [u32::MAX; 2];
    bad.max_bytes = usize::MAX;
    assert_eq!(
        SoftwareJob::new(packet, bad).err(),
        Some(SoftwareError::ResourceLimit)
    );
}

#[test]
fn malformed_effect_cycle_and_noncontiguous_isolation_never_publish() {
    let mut tree = UiTree::new(style(8.0, 8.0), 0xff0000ff).unwrap();
    let child = tree
        .append(tree.root(), style(2.0, 2.0), 0x0000ffff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.opacity(tree.root(), 0.5);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let packet = tree.frame([8.0; 2]).unwrap();
    let mut cycle = packet.clone();
    Arc::make_mut(&mut cycle.properties).effects.insert(
        tree.root(),
        PropertyNode {
            parent: Some(tree.root()),
            value: 0.5,
        },
    );
    let mut job = SoftwareJob::new(cycle, options([8; 2])).unwrap();
    assert_eq!(job.step(10), Err(SoftwareError::ResourceLimit));
    let mut split = packet.clone();
    let mut items = packet.items.iter().cloned().collect::<Vec<_>>();
    let mut middle = items[1].clone();
    middle.node = child;
    middle.properties.effect = None;
    items.insert(1, middle);
    split.items = Arc::new(items.into());
    let mut job = SoftwareJob::new(split, options([8; 2])).unwrap();
    assert_eq!(job.step(10), Err(SoftwareError::InvalidFrame));
}
