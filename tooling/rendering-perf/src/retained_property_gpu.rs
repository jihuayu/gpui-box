//! Hardware checks for persistent property snapshots.
use super::*;

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let fixed = || Style {
        size: taffy::Size {
            width: length(8.0),
            height: length(8.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(96.0),
                height: length(64.0),
            },
            flex_wrap: taffy::FlexWrap::Wrap,
            ..Default::default()
        },
        0x112233ff,
    )?;
    let mut nodes = vec![];
    let mut tx = Transaction::default();
    for _ in 0..96 {
        let node = tree.append(tree.root(), fixed(), 0x4488ff80)?;
        tx.translation(node, [0.25, 0.125]);
        tx.opacity(node, 0.8);
        tx.clip(
            node,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 7.0,
                height: 7.0,
            }),
        );
        tx.scroll(node, [1.0, 0.0]);
        nodes.push(node);
    }
    tree.commit(tx, BackendCapabilities::default())?;
    let original = tree.frame([96.0, 64.0])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&original, [96, 64], 1.0)?;
    let leaf = nodes[50];
    for (case, round) in ["translation", "opacity", "reset-defaults"]
        .into_iter()
        .zip(0..)
    {
        let mut tx = Transaction::default();
        match round {
            0 => tx.translation(leaf, [1.25, 0.5]),
            1 => tx.opacity(leaf, 0.4),
            _ => {
                tx.translation(leaf, [0.0; 2]);
                tx.opacity(leaf, 1.0);
                tx.clip(leaf, None);
                tx.scroll(leaf, [0.0; 2]);
            }
        }
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([96.0, 64.0])?;
        assert_eq!(frame.build_stats.extracted_nodes, 1);
        assert_eq!(original.properties.transforms[&leaf].value, [0.25, 0.125]);
        if round == 0 {
            assert!(
                frame
                    .properties
                    .effects
                    .shares_root(&original.properties.effects)
            );
        }
        renderer.submit(&frame, [96, 64], 1.0)?;
        compare(
            renderer,
            &frame,
            [96, 64],
            1.0,
            &format!("property-map-{case}"),
            output,
            results,
        )?;
    }
    tree.remove(leaf)?;
    let replacement = tree.append(tree.root(), fixed(), 0xff8844ff)?;
    assert_ne!(replacement, leaf);
    let mut tx = Transaction::default();
    tx.opacity(replacement, 0.6);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([96.0, 64.0])?;
    assert!(!frame.properties.effects.contains_key(&leaf));
    assert_eq!(original.properties.effects[&leaf].value, 0.8);
    renderer.submit(&frame, [96, 64], 1.0)?;
    compare(
        renderer,
        &frame,
        [96, 64],
        1.0,
        "property-map-generation-reuse",
        output,
        results,
    )?;
    // The same old snapshot must remain consumable by an independent renderer.
    let mut old_consumer = WgpuFrameRenderer::new(2)?;
    old_consumer.submit(&original, [96, 64], 1.0)?;
    compare(
        &mut old_consumer,
        &original,
        [96, 64],
        1.0,
        "property-map-old-snapshot",
        output,
        results,
    )?;
    Ok(())
}
