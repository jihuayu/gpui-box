//! Actual GPU pixels after selective layout-derived geometry publication.
use super::*;

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (mut tree, branches) = retained_layout::fixture(8)?;
    let original = tree.frame([800.0; 2])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&original, [800; 2], 1.0)?;
    let (parent, leaves) = &branches[2];
    let fixed = |w, h| Style {
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    for (round, name) in [
        "one-size",
        "dependent-siblings",
        "wrap-and-scroll",
        "scroll-clamp",
        "inherited-move",
        "mixed-image",
        "image-intrinsic",
    ]
    .into_iter()
    .enumerate()
    {
        let mut tx = Transaction::default();
        match round {
            0 => tx.style(leaves[99], fixed(2.0, 12.0)),
            1 => tx.style(leaves[0], fixed(3.0, 10.0)),
            2 => {
                tx.style(
                    *parent,
                    Style {
                        flex_wrap: taffy::FlexWrap::Wrap,
                        ..fixed(30.0, 10.0)
                    },
                );
                tx.clip(
                    *parent,
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 30.0,
                        height: 10.0,
                    }),
                );
                tx.scroll(*parent, [1000.0, 1000.0]);
                tx.opacity(*parent, 0.6);
            }
            3 => tx.style(leaves[99], fixed(2.0, 20.0)),
            4 => {
                tx.style(
                    *parent,
                    Style {
                        flex_wrap: taffy::FlexWrap::Wrap,
                        ..fixed(40.0, 20.0)
                    },
                );
                tx.translation(*parent, [1.25, 0.5]);
                tx.color(branches[6].1[2], 0xff4488ff);
            }
            5 => {
                tx.style(
                    leaves[98],
                    Style {
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                );
                tx.image(
                    leaves[98],
                    Some(ImageResource::rgba8(
                        [2, 2],
                        [255u8, 64, 128, 255].repeat(4),
                    )?),
                );
            }
            _ => tx.image(
                leaves[98],
                Some(ImageResource::rgba8(
                    [4, 3],
                    [64u8, 255, 128, 255].repeat(12),
                )?),
            ),
        }
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let frame = tree.frame([800.0; 2])?;
        assert_eq!(frame.layout_runs, 1);
        assert!(!frame.build_stats.full_rebuild);
        assert!(frame.build_stats.extracted_nodes < frame.items.len());
        renderer.submit(&frame, [800; 2], 1.0)?;
        compare(
            renderer,
            &frame,
            [800; 2],
            1.0,
            &format!("layout-output-{name}"),
            output,
            results,
        )?;
        let result = results.last_mut().unwrap();
        result["extracted_nodes"] = frame.build_stats.extracted_nodes.into();
        result["layout_writes"] = frame.layout_stats.layout_writes.into();
        result["net_changed_nodes"] = frame.layout_stats.net_changed_nodes.into();
    }
    assert_eq!(
        original
            .items
            .iter()
            .find(|i| i.node == leaves[99])
            .unwrap()
            .bounds
            .width,
        0.5
    );
    let mut tx = Transaction::default();
    tx.style(
        *parent,
        Style {
            display: taffy::Display::None,
            ..fixed(40.0, 20.0)
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let hidden = tree.frame([800.0; 2])?;
    assert!(hidden.build_stats.full_rebuild);
    renderer.submit(&hidden, [800; 2], 1.0)?;
    compare(
        renderer,
        &hidden,
        [800; 2],
        1.0,
        "layout-output-hidden-fallback",
        output,
        results,
    )?;
    let (system, mut raster) = retained_text::context()?;
    let resource = retained_text::text(
        &system,
        &mut raster,
        "Layout 中文 wraps across several lines",
        1.0,
    )?;
    let mut text_tree = UiTree::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            ..fixed(320.0, 240.0)
        },
        0x111111ff,
    )?;
    let text_node = text_tree.append(
        text_tree.root(),
        Style {
            size: taffy::Size {
                width: length(220.0),
                height: taffy::Dimension::auto(),
            },
            flex_shrink: 0.0,
            ..Default::default()
        },
        0,
    )?;
    let sibling = text_tree.append(text_tree.root(), fixed(40.0, 12.0), 0xff4488ff)?;
    let mut tx = Transaction::default();
    tx.text(
        text_node,
        Some(text::TextPaint {
            resource,
            color: 0xffdd88ff,
        }),
    );
    text_tree.commit(tx, BackendCapabilities::default())?;
    let before_text = text_tree.frame([320.0, 240.0])?;
    renderer.submit(&before_text, [320, 240], 1.0)?;
    let mut tx = Transaction::default();
    tx.style(
        text_node,
        Style {
            size: taffy::Size {
                width: length(90.0),
                height: taffy::Dimension::auto(),
            },
            flex_shrink: 0.0,
            ..Default::default()
        },
    );
    text_tree.commit(tx, BackendCapabilities::default())?;
    let wrapped = text_tree.frame([320.0, 240.0])?;
    assert!(!wrapped.build_stats.full_rebuild);
    assert_eq!(wrapped.build_stats.extracted_nodes, 2);
    let item = |frame: &FramePacket, id| {
        frame
            .items
            .iter()
            .find(|item| item.node == id)
            .unwrap()
            .bounds
    };
    assert!(item(&wrapped, sibling).y > item(&before_text, sibling).y);
    assert!(!std::sync::Arc::ptr_eq(
        before_text.items[1].glyphs.as_ref().unwrap(),
        wrapped.items[1].glyphs.as_ref().unwrap()
    ));
    renderer.submit(&wrapped, [320, 240], 1.0)?;
    compare(
        renderer,
        &wrapped,
        [320, 240],
        1.0,
        "layout-output-text-wrap",
        output,
        results,
    )?;
    Ok(())
}
