use super::*;
use gpui_box::retained::{logical::LogicalUi, reactive::Property};
use std::{cell::Cell, rc::Rc, time::Instant};
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
        absolute(0.0, 0.0, 128.0, 80.0),
        0x112233ff,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let root = ui.root_scope();
    let render = ui.root_render();
    let input = ui.state(root, 0, || 0x4488ffffu32)?;
    let before = ui.frame([128.0, 80.0])?;
    renderer.submit(&before, [128, 80], 1.0)?;
    let runs = Rc::new(Cell::new(0));
    let cleanups = Rc::new(Cell::new(0));
    let log = runs.clone();
    let cleanup = cleanups.clone();
    let result = ui.edit(
        |tx| {
            tx.set(input, 0x88cc44ff)?;
            Ok(())
        },
        |(), draft| {
            let scopes = draft.reconcile_keys(root, &[1, 2])?;
            let state = draft.state(scopes[0], 0, || 0u32)?;
            let material = draft.computed(
                scopes[0],
                1,
                &[state.dependency(), input.dependency()],
                move |i| Ok(*i.read(state)? + *i.read(input)?),
            )?;
            draft.effect(scopes[0], 0, &[material.dependency()], move |_| {
                log.set(log.get() + 1);
                let cleanup = cleanup.clone();
                Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
            })?;
            let a = draft.append(scopes[0], render, absolute(8.0, 8.0, 48.0, 48.0), 0)?;
            let b = draft.append(
                scopes[1],
                render,
                absolute(24.0, 24.0, 48.0, 48.0),
                0xdd44aa80,
            )?;
            draft.bind(material, Property::color(a))?;
            Ok((scopes, state, material, a, b))
        },
    )?;
    let (scopes, state, material, a, b) = result.result.value;
    let built = ui.frame([128.0, 80.0])?;
    assert_eq!(built.revision, before.revision + 1);
    assert_eq!(runs.get(), 1);
    renderer.submit(&built, [128, 80], 1.0)?;
    compare(
        renderer,
        &built,
        [128, 80],
        1.0,
        "lifecycle-keyed-build",
        output,
        results,
    )?;
    let stats = ui.stats();
    assert!(
        ui.edit(
            |tx| {
                tx.set(input, 0xff8844ff)?;
                Ok(())
            },
            |(), draft| {
                let order = draft.reconcile_keys(root, &[2, 3])?;
                let new = draft.state(order[1], 0, || 0x66ccffffu32)?;
                let node = draft.append(order[1], render, absolute(60.0, 8.0, 32.0, 32.0), 0)?;
                draft.bind(new, Property::color(node))?;
                draft.properties().opacity(node, f32::NAN);
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(ui.stats(), stats);
    assert_eq!(cleanups.get(), 0);
    assert_eq!(*ui.read(input)?, 0x88cc44ff);
    let same = ui.frame([128.0, 80.0])?;
    assert_eq!(same.revision, built.revision);
    assert!(renderer.submit(&same, [128, 80], 1.0)?.is_none());
    compare(
        renderer,
        &same,
        [128, 80],
        1.0,
        "lifecycle-failure-preserves-effects",
        output,
        results,
    )?;
    ui.edit(
        |tx| {
            tx.set(input, 0x22cc88ff)?;
            Ok(())
        },
        |(), draft| {
            let order = draft.reconcile_keys(root, &[2, 3])?;
            let new = draft.state(order[1], 0, || 0x66ccffffu32)?;
            let node = draft.append(order[1], render, absolute(60.0, 8.0, 32.0, 32.0), 0)?;
            draft.bind(new, Property::color(node))?;
            Ok(())
        },
    )?;
    let replaced = ui.frame([128.0, 80.0])?;
    assert_eq!(replaced.revision, built.revision + 1);
    assert_eq!((runs.get(), cleanups.get()), (1, 1));
    assert!(ui.read(state).is_err() && ui.read(material).is_err());
    assert!(ui.renders(scopes[0]).is_err());
    assert!(!replaced.items.iter().any(|i| i.node == a));
    assert!(replaced.items.iter().any(|i| i.node == b));
    renderer.submit(&replaced, [128, 80], 1.0)?;
    compare(
        renderer,
        &replaced,
        [128, 80],
        1.0,
        "lifecycle-replace-retire-once",
        output,
        results,
    )?;
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reconcile_keys(root, &[])?;
            Ok(())
        },
    )?;
    let empty = ui.frame([128.0, 80.0])?;
    assert_eq!(ui.reactive().source_count(), 1);
    assert_eq!(ui.reactive().binding_count(), 0);
    assert_eq!(empty.items.len(), 1);
    renderer.submit(&empty, [128, 80], 1.0)?;
    compare(
        renderer,
        &empty,
        [128, 80],
        1.0,
        "lifecycle-final-retirement",
        output,
        results,
    )?;
    assert!(built.items.iter().any(|i| i.node == a));
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1, 16, 128] {
        let mut commit_samples = [vec![], vec![]];
        let mut total_samples = [vec![], vec![]];
        let mut rounds = vec![];
        for round in 0..3 {
            for step in 0..2 {
                let mode = (round + step) % 2;
                let mut ui = LogicalUi::new(
                    absolute(0.0, 0.0, 256.0, 128.0),
                    0,
                    BackendCapabilities {
                        solid_color_patch: true,
                    },
                )?;
                let root = ui.root_scope();
                let render = ui.root_render();
                for n in 0..512 {
                    ui.append_render(
                        root,
                        render,
                        absolute((n % 128) as f32, (n / 128) as f32, 1.0, 1.0),
                        0x112233ff,
                    )?;
                }
                let input = ui.state(root, 0, || 0u32)?;
                let runs = Rc::new(Cell::new(0));
                let cleanups = Rc::new(Cell::new(0));
                let mut samples = vec![];
                for iteration in 0..50 {
                    let before = ui.frame([256.0, 128.0])?;
                    let keys: Vec<_> = (0..count)
                        .map(|n| ((iteration % 2) * count + n + 1) as u64)
                        .collect();
                    let color = if iteration % 2 == 0 {
                        0x4488ffff
                    } else {
                        0x22cc88ff
                    };
                    let start = Instant::now();
                    let nodes = if mode == 0 {
                        let scopes = ui.reconcile_keys(root, &keys)?.order;
                        let mut nodes = vec![];
                        for (n, scope) in scopes.iter().enumerate() {
                            let source = ui.state(*scope, 0, || color)?;
                            let derived = ui.computed(
                                *scope,
                                1,
                                &[input.dependency(), source.dependency()],
                                move |i| Ok(*i.read(input)? + *i.read(source)?),
                            )?;
                            let log = runs.clone();
                            let cleanup = cleanups.clone();
                            ui.effect(*scope, 0, &[derived.dependency()], move |_| {
                                log.set(log.get() + 1);
                                let cleanup = cleanup.clone();
                                Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
                            })?;
                            let node = ui.append_render(
                                *scope,
                                render,
                                absolute((n % 64) as f32, (n / 64 + 20) as f32, 1.0, 1.0),
                                0,
                            )?;
                            ui.bind(derived, Property::color(node))?;
                            nodes.push(node);
                        }
                        nodes
                    } else {
                        ui.edit(
                            |_| Ok(()),
                            |(), draft| {
                                let scopes = draft.reconcile_keys(root, &keys)?;
                                let mut nodes = vec![];
                                for (n, scope) in scopes.iter().enumerate() {
                                    let source = draft.state(*scope, 0, || color)?;
                                    let derived = draft.computed(
                                        *scope,
                                        1,
                                        &[input.dependency(), source.dependency()],
                                        move |i| Ok(*i.read(input)? + *i.read(source)?),
                                    )?;
                                    let log = runs.clone();
                                    let cleanup = cleanups.clone();
                                    draft.effect(
                                        *scope,
                                        0,
                                        &[derived.dependency()],
                                        move |_| {
                                            log.set(log.get() + 1);
                                            let cleanup = cleanup.clone();
                                            Some(Box::new(move || cleanup.set(cleanup.get() + 1)))
                                        },
                                    )?;
                                    let node = draft.append(
                                        *scope,
                                        render,
                                        absolute((n % 64) as f32, (n / 64 + 20) as f32, 1.0, 1.0),
                                        0,
                                    )?;
                                    draft.bind(derived, Property::color(node))?;
                                    nodes.push(node);
                                }
                                Ok(nodes)
                            },
                        )?
                        .result
                        .value
                    };
                    let commit_us = start.elapsed().as_secs_f64() * 1e6;
                    let frame = ui.frame([256.0, 128.0])?;
                    let total_us = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(
                        frame.revision - before.revision,
                        if mode == 0 {
                            (count * 2 + usize::from(iteration > 0)) as u64
                        } else {
                            1
                        }
                    );
                    assert_eq!(frame.items.len(), 513 + count);
                    assert_eq!(ui.reactive().source_count(), 1 + 2 * count);
                    assert_eq!(ui.reactive().effect_count(), count);
                    assert_eq!(ui.reactive().binding_count(), count);
                    assert_eq!(runs.get(), (iteration + 1) * count);
                    assert_eq!(cleanups.get(), iteration * count);
                    for (n, node) in nodes.iter().enumerate() {
                        let item = frame.items.iter().find(|i| i.node == *node).unwrap();
                        assert_eq!(
                            (item.bounds.x, item.bounds.y, item.fill_color),
                            ((n % 64) as f32, (n / 64 + 20) as f32, color)
                        );
                    }
                    if iteration >= 10 {
                        commit_samples[mode].push(commit_us);
                        total_samples[mode].push(total_us);
                        samples.push(serde_json::json!([commit_us, total_us]));
                    }
                }
                rounds.push(serde_json::json!({"round":round,"mode":mode,"samples_commit_total_us":samples}));
            }
        }
        let summarize = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            serde_json::json!({"median_us":v[v.len()/2],"p95_us":v[v.len()*95/100]})
        };
        cases.push(serde_json::json!({"stable_nodes":512,"replaced_scopes":count,"sequential_commit":summarize(&mut commit_samples[0]),"draft_commit":summarize(&mut commit_samples[1]),"sequential_with_frame":summarize(&mut total_samples[0]),"draft_with_frame":summarize(&mut total_samples[1]),"rounds":rounds}));
    }
    Ok(
        serde_json::json!({"scope":"CPU lifecycle replacement; 3 rotating rounds per mode, 10 warmups + 40 measured events. Each replaced keyed scope owns writable state, a computed source using root state, effect+cleanup and a bound output. Sequential existing keyed retirement then individual initialization versus unified lifecycle draft. Exactly one frame/event; final geometry, colors, live counts and callback counts checked outside timing. Commit includes construction/cleanup, total adds frame layout/extraction. No GPU/host latency or energy claim.","cases":cases}),
    )
}
