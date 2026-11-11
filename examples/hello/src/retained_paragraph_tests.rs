//! Real font shaping → owned wrapping → persistent Taffy correctness.
use gpui::{TextRun, WindowTextSystem, font, px};
use gpui_box::retained::{paragraph::ParagraphSnapshot, *};
use std::{borrow::Cow, sync::Arc};

fn text_system() -> WindowTextSystem {
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let system = Arc::new(gpui::TextSystem::new(platform));
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    WindowTextSystem::new(system)
}
fn shape(system: &WindowTextSystem, text: &str) -> ParagraphSnapshot {
    ParagraphSnapshot::shape(system, text, font("Noto Sans CJK SC"), 16.0, 32.0).unwrap()
}

#[test]
fn owned_paragraph_wrap_matches_real_gpui_layout_for_unicode_and_cluster_boundaries() {
    let system = text_system();
    let text = "office affine a\u{301} 中文混排 one two three\n\nlast line\n";
    let snapshot = shape(&system, text);
    assert_eq!(snapshot.hard_line_count(), 4);
    assert!(snapshot.min_content_width() > 0.0);
    assert!(snapshot.max_content_width() > snapshot.min_content_width());
    let cached = snapshot.layout(Some(80.0)).unwrap();
    assert!(Arc::ptr_eq(&cached, &snapshot.layout(Some(80.0)).unwrap()));
    let weak = Arc::downgrade(&cached);
    drop(cached);
    for width in [81.0, 82.0, 83.0, 84.0] {
        snapshot.layout(Some(width)).unwrap();
    }
    assert!(
        weak.upgrade().is_none(),
        "four-width memoization must evict old results"
    );
    let mut narrowest_height = 0.0;
    for width in [0.0, 1.0, 31.0, 80.0, 200.0, 1000.0] {
        let owned = snapshot.layout(Some(width)).unwrap();
        let mut expected_height = 0.0;
        for (original, actual) in text.split('\n').zip(&owned.lines) {
            let expected = system
                .shape_text(
                    original.to_owned().into(),
                    px(16.0),
                    &[TextRun {
                        len: original.len(),
                        font: font("Noto Sans CJK SC"),
                        ..Default::default()
                    }],
                    Some(px(width)),
                    None,
                )
                .unwrap();
            assert_eq!(actual.wrap_boundaries(), expected[0].wrap_boundaries());
            assert_eq!(actual.runs().len(), expected[0].runs().len());
            expected_height += f32::from(expected[0].size(px(32.0)).height);
            for (a, b) in actual.runs().iter().zip(expected[0].runs()) {
                assert_eq!(a.font_id, b.font_id);
                assert_eq!(a.glyphs.len(), b.glyphs.len());
                for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
                    assert_eq!((a.id, a.position, a.index), (b.id, b.position, b.index));
                }
            }
        }
        assert_eq!(owned.size[1], expected_height);
        if width == 0.0 {
            narrowest_height = owned.size[1];
        }
        assert!(owned.size[1] <= narrowest_height);
    }
    let owned = snapshot.clone();
    let before = owned.layout(Some(80.0)).unwrap().size;
    drop(system);
    let after = std::thread::spawn(move || owned.layout(Some(80.0)).unwrap().size)
        .join()
        .unwrap();
    assert_eq!(
        before, after,
        "pure wrapping survives context disposal and thread transfer"
    );
}

#[test]
fn paragraph_constraints_reflow_siblings_preserve_chunks_and_equal_input_is_idle() {
    let system = text_system();
    let paragraph = shape(
        &system,
        "one two three four five six seven eight nine ten 中文测试",
    );
    let make = |width: f32| Style {
        size: taffy::Size {
            width: length(width),
            height: taffy::Dimension::auto(),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            align_items: taffy::AlignItems::START,
            size: taffy::Size {
                width: length(400.0),
                height: length(400.0),
            },
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let text = tree.append(tree.root(), make(200.0), 0).unwrap();
    let sibling = tree
        .append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(10.0),
                    height: length(10.0),
                },
                ..Default::default()
            },
            0xff0000ff,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([400.0; 2]).unwrap();
    let bounds = |frame: &FramePacket, node| {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    };
    assert_eq!(
        bounds(&before, text).height,
        paragraph.layout(Some(200.0)).unwrap().size[1]
    );
    assert_eq!(bounds(&before, sibling).y, bounds(&before, text).height);
    let mut tx = Transaction::default();
    tx.style(text, make(80.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let narrow = tree.frame([400.0; 2]).unwrap();
    assert_eq!(
        bounds(&narrow, text).height,
        paragraph.layout(Some(80.0)).unwrap().size[1]
    );
    assert!(bounds(&narrow, sibling).y > bounds(&before, sibling).y);
    assert_eq!(narrow.recorded_chunks, 0);
    assert!(Arc::ptr_eq(
        &before.items[1].commands,
        &narrow.items[1].commands
    ));
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    assert!(
        !tree
            .commit(tx, BackendCapabilities::default())
            .unwrap()
            .request_frame
    );
    assert_eq!(
        tree.frame([400.0; 2]).unwrap().layout_stats,
        LayoutStats::default()
    );
    let mut style = make(80.0);
    style.padding = taffy::Rect {
        left: length(4.0),
        right: length(4.0),
        top: length(3.0),
        bottom: length(3.0),
    };
    let mut tx = Transaction::default();
    tx.style(text, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let padded = tree.frame([400.0; 2]).unwrap();
    assert_eq!(
        bounds(&padded, text).height,
        paragraph.layout(Some(72.0)).unwrap().size[1] + 6.0
    );
}

#[test]
fn font_epoch_replacement_and_invalid_paragraph_inputs_are_explicit() {
    let system = text_system();
    let old = shape(&system, "retained font epoch");
    let old_size = old.layout(Some(80.0)).unwrap().size;
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let next = shape(&system, "retained font epoch");
    assert!(next.font_epoch() > old.font_epoch());
    assert_ne!(old, next);
    assert_eq!(old.layout(Some(80.0)).unwrap().size, old_size);
    assert!(old.layout(Some(f32::NAN)).is_err());
    assert!(
        ParagraphSnapshot::shape(&system, "x", font("Noto Sans CJK SC"), f32::NAN, 32.0).is_err()
    );
    assert!(ParagraphSnapshot::shape(&system, "x", font("Noto Sans CJK SC"), 16.0, 1.0).is_err());
    assert_eq!(
        ParagraphSnapshot::shape(&system, "tab\tstop", font("Noto Sans CJK SC"), 16.0, 32.0)
            .unwrap_err(),
        gpui_box::retained::Error::UnsupportedText
    );
    assert_eq!(shape(&system, "").layout(None).unwrap().size, [0.0, 32.0]);
}

#[test]
fn paragraph_min_max_content_baseline_and_new_text_have_independent_dependencies() {
    let system = text_system();
    let paragraph = shape(&system, "one two 中文 paragraph");
    let mut tree = UiTree::new(
        Style {
            align_items: taffy::AlignItems::BASELINE,
            size: taffy::Size {
                width: length(800.0),
                height: length(300.0),
            },
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let text = tree
        .append(
            tree.root(),
            Style {
                flex_shrink: 0.0,
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let marker = tree.append(tree.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    tx.measure(
        marker,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 4.0,
            height: 40.0,
            baseline: Some(35.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let max = tree.frame([800.0, 300.0]).unwrap();
    let bounds = |frame: &FramePacket, node| {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    };
    assert!((bounds(&max, text).width - paragraph.max_content_width()).abs() < 0.01);
    assert!(
        (bounds(&max, text).y + paragraph.first_baseline() - bounds(&max, marker).y - 35.0).abs()
            < 0.01
    );
    let longer = shape(&system, "one two 中文 paragraph\nsecond hard line");
    let mut tx = Transaction::default();
    tx.measure(text, Some(MeasureSnapshot::Paragraph(longer.clone())));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.frame([800.0, 300.0]).unwrap();
    assert_eq!(next.layout_runs, 1);
    assert_eq!(next.recorded_chunks, 0);
    assert_eq!(bounds(&next, text).height, 64.0);
    assert_eq!(
        bounds(&max, text).height,
        32.0,
        "old frame retains old text metrics"
    );
    let mut small = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(1.0),
                height: length(300.0),
            },
            align_items: taffy::AlignItems::START,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    let child = small.append(small.root(), Style::default(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(child, Some(MeasureSnapshot::Paragraph(paragraph.clone())));
    small.commit(tx, BackendCapabilities::default()).unwrap();
    let min = small.frame([1.0, 300.0]).unwrap();
    assert!(
        (bounds(&min, child).width - paragraph.min_content_width()).abs() < 0.01,
        "automatic minimum: {:?}, min {}",
        bounds(&min, child),
        paragraph.min_content_width()
    );
}
