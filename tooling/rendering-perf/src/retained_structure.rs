use super::*;
use gpui_box::retained::logical::{LogicalStructureTransaction, LogicalUi};
use gpui_box::retained::reactive::Property;
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
    let mut ui = LogicalUi::new(
        absolute(0.0, 0.0, 160.0, 96.0),
        0x112233ff,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let root = ui.root_scope();
    let scopes = ui.reconcile_keys(root, &[1, 2, 3])?.order;
    let host_a = ui.append_render(
        scopes[2],
        ui.root_render(),
        absolute(4.0, 4.0, 64.0, 64.0),
        0x223344ff,
    )?;
    let host_b = ui.append_render(
        root,
        ui.root_render(),
        absolute(84.0, 8.0, 64.0, 64.0),
        0x334455ff,
    )?;
    let a = ui.append_render(
        scopes[0],
        host_a,
        absolute(4.0, 4.0, 40.0, 40.0),
        0xff440080,
    )?;
    let b = ui.append_render(
        scopes[1],
        host_a,
        absolute(4.0, 4.0, 40.0, 40.0),
        0x0044ff80,
    )?;
    let c = ui.append_render(
        scopes[0],
        host_b,
        absolute(4.0, 4.0, 40.0, 40.0),
        0x44ff0080,
    )?;
    let color = ui.state(scopes[0], 0, || 0xff440080u32)?;
    ui.bind(color, Property::color(a))?;
    let translation = ui.state(root, 0, || [0.0, 0.0])?;
    ui.bind(translation, Property::translation(host_b))?;
    ui.sync_render_order()?;
    let old = ui.frame([160.0, 96.0])?;
    renderer.submit(&old, [160, 96], 1.0)?;
    let mut tx = LogicalStructureTransaction::default();
    tx.reorder(root, vec![scopes[1], scopes[0], scopes[2]]);
    tx.reparent(a, host_b);
    tx.reparent(c, host_a);
    let commit = ui.commit_structure(tx)?;
    assert_eq!(commit.revision, old.revision + 1);
    let frame = ui.frame([160.0, 96.0])?;
    assert_eq!(frame.recorded_chunks, 0);
    for item in frame.items.iter() {
        let previous = old.items.iter().find(|i| i.node == item.node).unwrap();
        assert!(Arc::ptr_eq(&item.commands, &previous.commands));
    }
    assert_eq!(frame.hit_test(92.0, 16.0), Some(a));
    renderer.submit(&frame, [160, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [160, 96],
        1.0,
        "structure-atomic-portal",
        output,
        results,
    )?;
    // Invalid cycle includes a valid logical permutation: neither may publish.
    let mut invalid = LogicalStructureTransaction::default();
    invalid.reorder(root, scopes.clone());
    invalid.reparent(host_b, a);
    assert!(ui.commit_structure(invalid).is_err());
    let unchanged = ui.frame([160.0, 96.0])?;
    assert_eq!(unchanged.revision, frame.revision);
    assert!(renderer.submit(&unchanged, [160, 96], 1.0)?.is_none());
    compare(
        renderer,
        &unchanged,
        [160, 96],
        1.0,
        "structure-invalid-atomic",
        output,
        results,
    )?;
    // Move the second surviving portal out, then remove the old host's scope.
    ui.reparent_render(b, host_b)?;
    ui.reparent_render(c, host_b)?;
    ui.reconcile_keys(root, &[2, 1])?;
    ui.transaction(|tx| {
        tx.set(color, 0x88ccffff)?;
        tx.set(translation, [-3.0, 4.0])?;
        Ok(())
    })?;
    let frame = ui.frame([160.0, 96.0])?;
    assert!(!frame.items.iter().any(|i| i.node == host_a));
    assert!(
        frame
            .items
            .iter()
            .any(|i| i.node == a && i.fill_color == 0x88ccffff)
    );
    renderer.submit(&frame, [160, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [160, 96],
        1.0,
        "structure-retire-former-host",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for (hosts, per_host) in [(2, 8), (32, 32)] {
        let mut tree = UiTree::new(Style::default(), 0)?;
        let mut groups = vec![];
        for _ in 0..hosts {
            let host = tree.append(tree.root(), Style::default(), 0)?;
            let mut children = vec![];
            for _ in 0..per_host {
                children.push(tree.append(host, Style::default(), 0x4488ffff)?);
            }
            groups.push((host, children));
        }
        let mut modes = [vec![], vec![], vec![]];
        let mut rounds = vec![];
        for round in 0..3 {
            for step in 0..3 {
                let mode = (round + step) % 3;
                let mut samples = vec![];
                for iteration in 0..110 {
                    let before = tree.frame([256.0, 128.0])?;
                    let next: Vec<_> = groups
                        .iter()
                        .map(|(host, children)| {
                            (
                                *host,
                                if iteration % 2 == 0 {
                                    children.iter().rev().copied().collect::<Vec<_>>()
                                } else {
                                    children.clone()
                                },
                            )
                        })
                        .collect();
                    let start = Instant::now();
                    if mode == 0 {
                        for (host, children) in &next {
                            let mut tx = Transaction::default();
                            tx.reorder(*host, children.clone());
                            tree.commit(tx, BackendCapabilities::default())?;
                        }
                    } else if mode == 1 {
                        let mut tx = Transaction::default();
                        for (host, children) in &next {
                            tx.reorder(*host, children.clone());
                        }
                        tree.commit(tx, BackendCapabilities::default())?;
                    } else {
                        let mut tx = StructureTransaction::default();
                        for (host, children) in &next {
                            tx.children(*host, children.clone());
                        }
                        tree.commit_structure(tx)?;
                    }
                    let us = start.elapsed().as_secs_f64() * 1e6;
                    let frame = tree.frame([256.0, 128.0])?;
                    assert_eq!(
                        frame.revision - before.revision,
                        if mode == 0 { hosts } else { 1 }
                    );
                    assert_eq!(frame.recorded_chunks, 0);
                    let expected: Vec<_> = std::iter::once(tree.root())
                        .chain(next.iter().flat_map(|(host, children)| {
                            std::iter::once(*host).chain(children.iter().copied())
                        }))
                        .collect();
                    assert_eq!(
                        frame.items.iter().map(|i| i.node).collect::<Vec<_>>(),
                        expected
                    );
                    if iteration >= 10 {
                        samples.push(us);
                    }
                }
                modes[mode].extend_from_slice(&samples);
                rounds.push(serde_json::json!({"round":round,"mode":mode,"samples_us":samples}));
            }
        }
        let summarize = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            serde_json::json!({"median_us":v[v.len()/2],"p95_us":v[v.len()*95/100],"samples_us":v})
        };
        cases.push(serde_json::json!({"hosts":hosts,"children_per_host":per_host,"sequential_reorder":summarize(&mut modes[0]),"property_transaction":summarize(&mut modes[1]),"atomic_final_topology":summarize(&mut modes[2]),"rounds":rounds,"same_order_and_chunk_reuse":true}));
    }
    Ok(
        serde_json::json!({"scope":"3 rotating rounds, 10 warmups+100 measured samples/mode/round; commit includes transaction construction/list clones, excludes layout/extraction/GPU. Existing sequential per-host reorder, single property transaction and full-tree-validating atomic final topology. Same final node order; revisions hosts vs one vs one.","cases":cases}),
    )
}
