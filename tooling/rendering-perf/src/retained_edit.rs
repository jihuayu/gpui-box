use super::*;
use gpui_box::retained::{logical::LogicalUi, reactive::Property};
use std::{sync::Arc, time::Instant};
fn absolute(x: f32, y: f32, w: f32, h: f32) -> Style {
    Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(x),
            top: length(y),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let caps = BackendCapabilities {
        solid_color_patch: true,
    };
    let mut ui = LogicalUi::new(absolute(0.0, 0.0, 128.0, 80.0), 0x112233ff, caps)?;
    let owner = ui.root_scope();
    let root = ui.root_render();
    let scopes = ui.reconcile_keys(owner, &[1])?.order;
    let old_host = ui.append_render(owner, root, absolute(4.0, 4.0, 48.0, 48.0), 0x223344ff)?;
    let portal = ui.append_render(
        scopes[0],
        old_host,
        absolute(4.0, 4.0, 24.0, 24.0),
        0xff4400ff,
    )?;
    let color = ui.state(scopes[0], 0, || 0xff4400ffu32)?;
    let derived = ui.computed(scopes[0], 1, &[color.dependency()], move |i| {
        Ok(*i.read(color)?)
    })?;
    let portal_binding = ui.bind(derived, Property::color(portal))?.0;
    let old = ui.frame([128.0, 80.0])?;
    renderer.submit(&old, [128, 80], 1.0)?;
    let image = ImageResource::rgba8([8, 4], vec![255; 128])?;
    let result = ui.edit(
        |tx| {
            tx.set(color, 0x4488ffff)?;
            Ok(())
        },
        |(), edit| {
            let host = edit.append(owner, root, absolute(60.0, 8.0, 48.0, 48.0), 0x334455ff)?;
            edit.reparent(portal, host)?;
            edit.remove(old_host)?;
            let node = edit.append(scopes[0], root, absolute(8.0, 56.0, 24.0, 16.0), 0)?;
            edit.bind(derived, Property::color(node))?;
            let img = edit.append(
                owner,
                root,
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(90.0),
                        top: length(60.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    ..Default::default()
                },
                0,
            )?;
            edit.properties().image(img, Some(image.clone()));
            edit.properties().opacity(host, 0.75);
            Ok((host, node, img))
        },
    )?;
    assert_eq!(result.result.commit.revision, old.revision + 1);
    let frame = ui.frame([128.0, 80.0])?;
    let portal_item = frame.items.iter().find(|i| i.node == portal).unwrap();
    assert!(Arc::ptr_eq(
        &portal_item.commands,
        &old.items
            .iter()
            .find(|i| i.node == portal)
            .unwrap()
            .commands
    ));
    assert_eq!(portal_item.fill_color, 0x4488ffff);
    let img = frame
        .items
        .iter()
        .find(|i| i.node == result.result.value.2)
        .unwrap();
    assert_eq!((img.bounds.width, img.bounds.height), (8.0, 4.0));
    renderer.submit(&frame, [128, 80], 1.0)?;
    compare(
        renderer,
        &frame,
        [128, 80],
        1.0,
        "edit-source-construct-move-image",
        output,
        results,
    )?;
    // Compare the complete final scene with separately constructed output too.
    let mut fresh = UiTree::new(absolute(0.0, 0.0, 128.0, 80.0), 0x112233ff)?;
    let host = fresh.append(fresh.root(), absolute(60.0, 8.0, 48.0, 48.0), 0x334455ff)?;
    fresh.append(host, absolute(4.0, 4.0, 24.0, 24.0), 0x4488ffff)?;
    fresh.append(fresh.root(), absolute(8.0, 56.0, 24.0, 16.0), 0x4488ffff)?;
    let img = fresh.append(
        fresh.root(),
        Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(90.0),
                top: length(60.0),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            ..Default::default()
        },
        0,
    )?;
    let mut tx = Transaction::default();
    tx.opacity(host, 0.75);
    tx.image(img, Some(image));
    fresh.commit(tx, caps)?;
    let fresh = fresh.frame([128.0, 80.0])?;
    let actual = renderer.image()?;
    let reference = renderer.reference_image(&fresh, [128, 80], 1.0)?;
    assert!(
        actual.as_raw() == reference.as_raw(),
        "draft matches fresh scene"
    );
    let stats = ui.stats();
    assert!(
        ui.edit(
            |tx| {
                tx.set(color, 7)?;
                Ok(())
            },
            |(), edit| {
                let n = edit.append(owner, root, Style::default(), 1)?;
                edit.bind(derived, Property::color(n))?;
                edit.properties().translation(n, [f32::NAN, 0.0]);
                edit.remove(result.result.value.0)?;
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(ui.stats(), stats);
    assert_eq!(*ui.read(color)?, 0x4488ffff);
    let unchanged = ui.frame([128.0, 80.0])?;
    assert_eq!(unchanged.revision, frame.revision);
    assert!(renderer.submit(&unchanged, [128, 80], 1.0)?.is_none());
    compare(
        renderer,
        &unchanged,
        [128, 80],
        1.0,
        "edit-rejected-no-submit",
        output,
        results,
    )?;
    let alternate = ui.state(scopes[0], 2, || 0xff88ccffu32)?;
    ui.edit(
        |tx| {
            tx.set(color, 0x88cc44ff)?;
            tx.set(alternate, 0xdd66aaff)?;
            Ok(())
        },
        |(), edit| {
            edit.unbind(portal_binding)?;
            edit.bind(alternate, Property::color(portal))?;
            Ok(())
        },
    )?;
    let rebound = ui.frame([128.0, 80.0])?;
    assert_eq!(rebound.revision, frame.revision + 1);
    assert_eq!(
        rebound
            .items
            .iter()
            .find(|i| i.node == portal)
            .unwrap()
            .fill_color,
        0xdd66aaff
    );
    assert_eq!(
        rebound
            .items
            .iter()
            .find(|i| i.node == result.result.value.1)
            .unwrap()
            .fill_color,
        0x88cc44ff
    );
    renderer.submit(&rebound, [128, 80], 1.0)?;
    compare(
        renderer,
        &rebound,
        [128, 80],
        1.0,
        "edit-rebind-final-writer",
        output,
        results,
    )?;
    ui.edit(
        |tx| {
            tx.set(color, 0x22dd88ff)?;
            Ok(())
        },
        |(), edit| {
            edit.remove(portal)?;
            edit.remove(result.result.value.0)?;
            edit.remove(result.result.value.1)?;
            edit.remove(result.result.value.2)?;
            Ok(())
        },
    )?;
    let removed = ui.frame([128.0, 80.0])?;
    assert_eq!(removed.revision, rebound.revision + 1);
    assert_eq!(ui.reactive().binding_count(), 0);
    renderer.submit(&removed, [128, 80], 1.0)?;
    assert!(old.items.iter().any(|i| i.node == portal));
    assert!(matches!(
        renderer.submit(&old, [128, 80], 1.0),
        Err(gpui_box::retained::Error::StaleFrame)
    ));
    compare(
        renderer,
        &removed,
        [128, 80],
        1.0,
        "edit-retirement-old-snapshot",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1, 32, 256] {
        let mut times = [vec![], vec![]];
        let mut totals = [vec![], vec![]];
        let mut rounds = vec![];
        for round in 0..3 {
            for step in 0..2 {
                let mode = (round + step) % 2;
                let mut tree = UiTree::new(absolute(0.0, 0.0, 256.0, 128.0), 0)?;
                for n in 0..1024 {
                    tree.append(
                        tree.root(),
                        absolute((n % 128) as f32, (n / 128) as f32, 1.0, 1.0),
                        0x112233ff,
                    )?;
                }
                let root = tree.root();
                let mut old = vec![];
                let mut samples = vec![];
                for iteration in 0..70 {
                    let before = tree.frame([256.0, 128.0])?;
                    let color = if iteration % 2 == 0 {
                        0x4488ffff
                    } else {
                        0x22dd88ff
                    };
                    let start = Instant::now();
                    let mut next = vec![];
                    if mode == 0 {
                        for n in 0..count {
                            next.push(tree.append(
                                root,
                                absolute((n % 64) as f32, (n / 64 + 20) as f32, 1.0, 1.0),
                                0,
                            )?);
                        }
                        let mut tx = Transaction::default();
                        for node in &next {
                            tx.color(*node, color);
                        }
                        tree.commit(tx, BackendCapabilities::default())?;
                        for node in &old {
                            tree.remove(*node)?;
                        }
                    } else {
                        next = tree
                            .edit(BackendCapabilities::default(), |edit| {
                                let mut next = vec![];
                                for n in 0..count {
                                    let node = edit.append(
                                        root,
                                        absolute((n % 64) as f32, (n / 64 + 20) as f32, 1.0, 1.0),
                                        0,
                                    )?;
                                    edit.properties().color(node, color);
                                    next.push(node);
                                }
                                for node in &old {
                                    edit.remove(*node)?;
                                }
                                Ok(next)
                            })?
                            .value;
                    }
                    let commit_us = start.elapsed().as_secs_f64() * 1e6;
                    let frame = tree.frame([256.0, 128.0])?;
                    let total_us = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(frame.items.len(), 1025 + count);
                    assert_eq!(
                        frame.revision - before.revision,
                        if mode == 0 {
                            (count + old.len() + 1) as u64
                        } else {
                            1
                        }
                    );
                    for (n, node) in next.iter().enumerate() {
                        let item = frame.items.iter().find(|i| i.node == *node).unwrap();
                        assert_eq!(
                            (item.bounds.x, item.bounds.y, item.fill_color),
                            ((n % 64) as f32, (n / 64 + 20) as f32, color)
                        );
                    }
                    if iteration >= 10 {
                        times[mode].push(commit_us);
                        totals[mode].push(total_us);
                        samples.push(serde_json::json!([commit_us, total_us]));
                    }
                    old = next;
                }
                rounds.push(serde_json::json!({"round":round,"mode":mode,"samples_commit_total_us":samples}));
            }
        }
        let summarize = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            serde_json::json!({"median_us":v[v.len()/2],"p95_us":v[v.len()*95/100]})
        };
        cases.push(serde_json::json!({"stable_nodes":1024,"replaced_nodes":count,
            "sequential_commit":summarize(&mut times[0]),"draft_commit":summarize(&mut times[1]),
            "sequential_with_frame":summarize(&mut totals[0]),"draft_with_frame":summarize(&mut totals[1]),"rounds":rounds}));
    }
    Ok(
        serde_json::json!({"scope":"CPU only; 3 rotating rounds/mode, 10 warmups + 60 measured replacement events. Sequential append + one property batch + individual direct removes versus one validated sparse draft; same final geometry/material and one frame per event. Commit includes transaction construction; total adds layout/extraction; no GPU/host latency. Sources/bindings are tested separately, not timed here.","cases":cases}),
    )
}
