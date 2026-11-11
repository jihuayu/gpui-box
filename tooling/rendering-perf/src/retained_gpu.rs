//! Integrated persistent UI → FramePacket → GPU arena lifecycle validation.
use gpui_box::retained::{wgpu::WgpuFrameRenderer, *};
use std::{error::Error, fs, path::PathBuf};

fn compare(
    renderer: &mut WgpuFrameRenderer,
    frame: &FramePacket,
    size: [u32; 2],
    scale: f32,
    name: &str,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let actual = renderer.image()?;
    let reference = renderer.reference_image(frame, size, scale)?;
    assert!(
        actual.as_raw() == reference.as_raw(),
        "pixel mismatch in {name}"
    );
    actual.save(output.join(format!("{name}.png")))?;
    results.push(serde_json::json!({"case":name,"pixel_equal":true,"revision":frame.revision,"layout_runs":frame.layout_runs,"recorded_chunks":frame.recorded_chunks}));
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/retained-runtime-gpu".into()),
    );
    fs::create_dir_all(&output)?;
    let mut renderer = WgpuFrameRenderer::new(3)?;
    let info = renderer.adapter_info();
    println!("Integrated retained GPU {:?} {}", info.backend, info.name);
    fs::write(
        output.join("adapter.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"name":info.name,"backend":format!("{:?}",info.backend),"driver":info.driver_info}),
        )?,
    )?;
    let mut results = vec![];
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
    let style = Style {
        size: taffy::Size {
            width: length(24.0),
            height: length(24.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut children = vec![];
    for _ in 0..8 {
        children.push(tree.append(tree.root(), style.clone(), 0x245fc7ff)?);
    }
    let size = [320, 96];
    let mut frame = tree.frame([320.0, 96.0])?;
    assert!(renderer.submit(&frame, size, 1.0)?.unwrap().full_resync);
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "initial",
        &output,
        &mut results,
    )?;
    let mut tx = Transaction::default();
    tx.color(children[3], 0xd75522ff);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    frame = tree.frame([320.0, 96.0])?;
    assert_eq!(frame.layout_runs, 0);
    let color = renderer.submit(&frame, size, 1.0)?.unwrap();
    assert_eq!(color.gpu.uploaded_bytes, 160);
    assert_eq!(color.gpu.upload_ranges, 1);
    assert!(!color.full_resync);
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "color",
        &output,
        &mut results,
    )?;
    let mut tx = Transaction::default();
    for child in &children[1..4] {
        tx.color(*child, 0x6ad755ff);
    }
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([320.0, 96.0])?;
    let batch = renderer.submit(&frame, size, 1.0)?.unwrap();
    assert_eq!(
        (batch.gpu.uploaded_bytes, batch.gpu.upload_ranges),
        (480, 1)
    );
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "batch",
        &output,
        &mut results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(tree.root(), [6.0, 8.0]);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([320.0, 96.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    renderer.submit(&frame, size, 1.0)?;
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "translation",
        &output,
        &mut results,
    )?;
    children.reverse();
    let mut tx = Transaction::default();
    tx.reorder(tree.root(), children.clone());
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([320.0, 96.0])?;
    renderer.submit(&frame, size, 1.0)?;
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "reorder",
        &output,
        &mut results,
    )?;
    let removed = children.pop().unwrap();
    tree.remove(removed)?;
    children.push(tree.append(tree.root(), style.clone(), 0xab55c7ff)?);
    frame = tree.frame([320.0, 96.0])?;
    renderer.submit(&frame, size, 1.0)?;
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "replace",
        &output,
        &mut results,
    )?;
    for i in 0..32 {
        if i % 4 == 0 {
            let removed = children.pop().unwrap();
            tree.remove(removed)?;
            children.push(tree.append(tree.root(), style.clone(), 0x22aa88ff)?);
        }
        let mut tx = Transaction::default();
        tx.translation(children[2], [i as f32, 0.0]);
        tree.commit(tx, BackendCapabilities::default())?;
        frame = tree.frame([320.0, 96.0])?;
        if let Some(submit) = renderer.submit(&frame, size, 1.0)? {
            assert!(submit.gpu.in_flight <= 3);
        }
    }
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "in-flight-reuse",
        &output,
        &mut results,
    )?;
    let old = frame.clone();
    let mut tx = Transaction::default();
    tx.color(children[1], 0x00ffffff);
    tree.commit(tx, BackendCapabilities::default())?;
    let dropped = tree.frame([320.0, 96.0])?;
    let mut tx = Transaction::default();
    tx.color(children[2], 0xff00ffff);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([320.0, 96.0])?;
    assert_eq!(frame.base_revision, Some(dropped.revision));
    assert!(renderer.submit(&frame, size, 1.0)?.unwrap().full_resync);
    assert!(matches!(
        renderer.submit(&old, size, 1.0),
        Err(gpui_box::retained::Error::StaleFrame)
    ));
    assert!(renderer.submit(&frame, size, 1.0)?.is_none());
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "dropped-base",
        &output,
        &mut results,
    )?;
    renderer.reset()?;
    assert!(matches!(
        renderer.submit(&old, size, 1.0),
        Err(gpui_box::retained::Error::StaleFrame)
    ));
    assert!(renderer.submit(&frame, size, 1.0)?.unwrap().full_resync);
    compare(
        &mut renderer,
        &frame,
        size,
        1.0,
        "reset",
        &output,
        &mut results,
    )?;
    frame = tree.frame([240.0, 80.0])?;
    renderer.submit(&frame, [360, 120], 1.5)?;
    compare(
        &mut renderer,
        &frame,
        [360, 120],
        1.5,
        "resize-scale",
        &output,
        &mut results,
    )?;
    for _ in 0..40 {
        tree.append(tree.root(), style.clone(), 0x8899aaff)?;
    }
    frame = tree.frame([240.0, 80.0])?;
    let grow = renderer.submit(&frame, [360, 120], 1.5)?.unwrap();
    assert!(grow.gpu.buffer_generation > 1);
    compare(
        &mut renderer,
        &frame,
        [360, 120],
        1.5,
        "growth",
        &output,
        &mut results,
    )?;
    assert!(renderer.submit(&frame, [0, 0], 1.0).is_err());
    let mut other = UiTree::new(
        Style {
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0x556677ff,
    )?;
    let other_frame = other.frame([320.0, 96.0])?;
    assert!(
        renderer
            .submit(&other_frame, size, 1.0)?
            .unwrap()
            .full_resync
    );
    assert!(matches!(
        renderer.submit(&frame, [360, 120], 1.5),
        Err(gpui_box::retained::Error::StaleFrame)
    ));
    compare(
        &mut renderer,
        &other_frame,
        size,
        1.0,
        "new-tree",
        &output,
        &mut results,
    )?;
    let overlap_style = Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(8.0),
            top: length(8.0),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(40.0),
            height: length(40.0),
        },
        ..Default::default()
    };
    let red = other.append(other.root(), overlap_style.clone(), 0xff000080)?;
    let blue = other.append(other.root(), overlap_style, 0x0000ff80)?;
    let overlap = other.frame([320.0, 96.0])?;
    renderer.submit(&overlap, size, 1.0)?;
    let overlap_before = renderer.image()?;
    compare(
        &mut renderer,
        &overlap,
        size,
        1.0,
        "transparent-overlap",
        &output,
        &mut results,
    )?;
    let mut tx = Transaction::default();
    tx.reorder(other.root(), vec![blue, red]);
    other.commit(tx, BackendCapabilities::default())?;
    let reordered = other.frame([320.0, 96.0])?;
    renderer.submit(&reordered, size, 1.0)?;
    assert!(
        renderer.image()?.as_raw() != overlap_before.as_raw(),
        "transparent stacking order must affect pixels"
    );
    compare(
        &mut renderer,
        &reordered,
        size,
        1.0,
        "transparent-reorder",
        &output,
        &mut results,
    )?;
    let mut tx = Transaction::default();
    tx.color(red, 0xff0000ff);
    other.commit(tx, BackendCapabilities::default())?;
    let opaque = other.frame([320.0, 96.0])?;
    let alpha = renderer.submit(&opaque, size, 1.0)?.unwrap();
    assert_eq!(alpha.gpu.uploaded_bytes, 160);
    compare(
        &mut renderer,
        &opaque,
        size,
        1.0,
        "alpha-update",
        &output,
        &mut results,
    )?;

    let mut many = UiTree::new(
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
    let mut nodes = Vec::new();
    for _ in 0..10_000 {
        nodes.push(many.append(
            many.root(),
            Style {
                size: taffy::Size {
                    width: length(4.0),
                    height: length(4.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            0x2466aaff,
        )?);
    }
    let many_frame = many.frame([1200.0, 400.0])?;
    renderer.submit(&many_frame, [1200, 400], 1.0)?;
    let mut tx = Transaction::default();
    for node in nodes {
        tx.translation(node, [1.0, 1.0]);
    }
    let commit = many.commit(tx, BackendCapabilities::default())?;
    assert_eq!(commit.changed_nodes, 10_000);
    let moved = many.frame([1200.0, 400.0])?;
    assert_eq!((moved.layout_runs, moved.recorded_chunks), (0, 0));
    let submitted = renderer.submit(&moved, [1200, 400], 1.0)?.unwrap();
    assert_eq!(submitted.gpu.uploaded_bytes, 1_600_000);
    // Reused slots may put the unchanged root between two dirty ranges.
    assert!((1..=2).contains(&submitted.gpu.upload_ranges));
    compare(
        &mut renderer,
        &moved,
        [1200, 400],
        1.0,
        "batch-10000",
        &output,
        &mut results,
    )?;
    fs::write(
        output.join("validation.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!(
        "{} hardware pixel scenarios passed; one-color bytes=160, batch bytes=480 in one range; bounded frames=3",
        results.len()
    );
    Ok(())
}
