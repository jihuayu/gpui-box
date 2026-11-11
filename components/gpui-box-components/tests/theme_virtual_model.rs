use gpui::{div, prelude::*};
use gpui_box_components::{
    Navigation, ScreenModel, Theme, VirtualListSpec, VirtualWindow, virtual_list,
};
use std::cell::Cell;
use std::rc::Rc;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn conserved(spec: &VirtualListSpec, window: &VirtualWindow) -> bool {
    let rows = if window.total_height_px == 0.0 {
        0.0
    } else {
        window.visible_range.len() as f32 * spec.row_height_px
    };
    close(
        window.top_spacer_px + rows + window.bottom_spacer_px,
        window.total_height_px,
    )
}

#[test]
fn theme_high_contrast_motion_and_builder_tokens() {
    assert!(
        format!("{:?}", Theme::DARK).contains("Theme"),
        "Theme derives Debug"
    );
    for (theme, high_contrast, reduced_motion) in [
        (Theme::DARK, false, false),
        (Theme::LIGHT, false, false),
        (Theme::HIGH_CONTRAST_DARK, true, false),
        (Theme::HIGH_CONTRAST_LIGHT, true, false),
    ] {
        assert_eq!(theme.high_contrast, high_contrast);
        assert_eq!(theme.reduced_motion, reduced_motion);
    }
    assert_eq!(Theme::DARK.transition_ms(200), 200);
    assert_eq!(Theme::DARK.with_reduced_motion(true).transition_ms(200), 0);
    assert_eq!(
        Theme::DARK.with_text_scale(1.75).text_scale,
        1.75,
        "text scale builder preserves other tokens"
    );
    let contrasted = Theme::LIGHT.with_high_contrast(true);
    assert!(contrasted.high_contrast);
    assert_eq!(contrasted.background, Theme::LIGHT.background);
    assert_eq!(contrasted.surface, Theme::LIGHT.surface);
    assert_ne!(contrasted.text, Theme::LIGHT.text);
    assert_ne!(contrasted.border, Theme::LIGHT.border);
    assert!(!contrasted.with_high_contrast(false).high_contrast);
    let dark = Theme::DARK.with_high_contrast(true);
    assert_eq!(dark.text, 0xffffff);
    assert_eq!(dark.border, 0xf2f6fc);
    assert_eq!(
        Theme::DARK
            .with_high_contrast(true)
            .with_high_contrast(false),
        Theme::DARK
    );
    assert_eq!(
        Theme::LIGHT
            .with_high_contrast(true)
            .with_high_contrast(false),
        Theme::LIGHT
    );
}

#[test]
fn virtual_window_handles_empty_invalid_and_overscrolled_lists() {
    let empty = VirtualListSpec {
        item_count: 0,
        row_height_px: 40.0,
        viewport_height_px: 400.0,
        scroll_offset_px: 100.0,
        overscan: 3,
    }
    .window();
    assert_eq!(empty.total_height_px, 0.0);
    assert_eq!(empty.visible_range, 0..0);
    assert_eq!(empty.scroll_offset_px, 0.0);
    assert_eq!(empty.top_spacer_px, 0.0);
    assert_eq!(empty.bottom_spacer_px, 0.0);

    for (row_height, viewport, offset) in [
        (0.0, 400.0, 10.0),
        (-10.0, 400.0, 10.0),
        (f32::NAN, 400.0, 10.0),
        (40.0, 0.0, 10.0),
        (40.0, -1.0, 10.0),
        (40.0, 400.0, f32::INFINITY),
        (40.0, 400.0, f32::NAN),
    ] {
        let spec = VirtualListSpec {
            item_count: 100,
            row_height_px: row_height,
            viewport_height_px: viewport,
            scroll_offset_px: offset,
            overscan: 4,
        };
        let window = spec.window();
        assert!(window.total_height_px.is_finite());
        assert!(window.scroll_offset_px.is_finite());
        assert!(conserved(&spec, &window));
    }
}

#[test]
fn virtual_window_clamps_overscan_and_conserves_spacers_for_ten_thousand_items() {
    let mut spec = VirtualListSpec {
        item_count: 10_000,
        row_height_px: 40.0,
        viewport_height_px: 400.0,
        scroll_offset_px: 0.0,
        overscan: 2,
    };
    let top = spec.window();
    assert_eq!(top.total_height_px, 400_000.0);
    assert_eq!(top.visible_range, 0..12);
    assert_eq!(top.top_spacer_px, 0.0);
    assert!(conserved(&spec, &top));

    spec.scroll_offset_px = f32::MAX;
    let bottom = spec.window();
    assert_eq!(bottom.scroll_offset_px, 399_600.0);
    assert_eq!(bottom.visible_range.start, 9_988);
    assert_eq!(bottom.visible_range.end, 10_000);
    assert!(conserved(&spec, &bottom));

    spec.scroll_offset_px = 0.0;
    spec.overscan = usize::MAX;
    let fully_overscanned = spec.window();
    assert_eq!(fully_overscanned.visible_range, 0..10_000);
    assert!(conserved(&spec, &fully_overscanned));

    spec.overscan = 2;
    spec.scroll_offset_px = 399_000.0;
    spec.viewport_height_px = 500_000.0;
    let viewport_larger_than_content = spec.window();
    assert_eq!(viewport_larger_than_content.scroll_offset_px, 0.0);
    assert_eq!(viewport_larger_than_content.visible_range, 0..10_000);
    assert!(conserved(&spec, &viewport_larger_than_content));
}

#[test]
fn scroll_to_index_and_filter_shrink_retain_a_clamped_offset() {
    let mut spec = VirtualListSpec {
        item_count: 100,
        row_height_px: 20.0,
        viewport_height_px: 200.0,
        scroll_offset_px: 0.0,
        overscan: 1,
    };
    spec.scroll_to_index(99);
    assert_eq!(spec.scroll_offset_px, 1_800.0);
    assert!(spec.window().visible_range.contains(&99));

    spec.set_item_count(3);
    assert_eq!(spec.scroll_offset_px, 0.0);
    let shrunk = spec.window();
    assert_eq!(shrunk.total_height_px, 60.0);
    assert_eq!(shrunk.visible_range, 0..3);
    assert!(conserved(&spec, &shrunk));

    spec.set_item_count(0);
    assert_eq!(spec.scroll_offset_px, 0.0);
    assert_eq!(spec.window().visible_range, 0..0);

    spec.item_count = 10;
    spec.scroll_offset_px = f32::NAN;
    assert_eq!(spec.window().scroll_offset_px, 0.0);
}

#[test]
fn virtual_list_renders_only_the_visible_slice_for_ten_thousand_items() {
    let spec = VirtualListSpec {
        item_count: 10_000,
        row_height_px: 24.0,
        viewport_height_px: 240.0,
        scroll_offset_px: 24_000.0,
        overscan: 2,
    };
    let window = spec.window();
    let rendered = Rc::new(Cell::new(0usize));
    let counter = rendered.clone();
    let _element = virtual_list("large-list", window.clone(), 10_000, move |index| {
        counter.set(counter.get() + 1);
        div().child(index.to_string()).into_any_element()
    });
    assert_eq!(rendered.get(), window.visible_range.len());
    assert!(rendered.get() <= 14);
    assert!(conserved(&spec, &window));
}

#[derive(Debug, PartialEq, Eq)]
struct Counter(u32);

#[test]
fn screen_model_registers_overrides_and_reports_missing_services() {
    let mut model = ScreenModel::new("root");
    assert_eq!(model.navigation().current(), &"root");
    assert!(model.service::<Counter>().is_err());
    model.insert_service(Counter(1)).expect("first insertion");
    assert_eq!(model.service::<Counter>().expect("registered").0, 1);
    assert!(model.insert_service(Counter(2)).is_err());
    model.replace_service(Counter(3));
    assert_eq!(model.service::<Counter>().expect("replaced").0, 3);

    let built = ScreenModel::new("built")
        .with_service(Counter(4))
        .expect("builder registration");
    assert_eq!(built.service::<Counter>().expect("built service").0, 4);
    assert_eq!(built.services().service::<Counter>().expect("access").0, 4);
    let mut built = built;
    assert!(built.services_mut().service::<Counter>().is_ok());
}

#[test]
fn navigation_exposes_root_routes_and_resets_to_root() {
    let mut nav = Navigation::new("catalog");
    assert_eq!(nav.root(), &"catalog");
    assert!(nav.routes().is_empty());
    nav.push("settings");
    nav.push("details");
    assert_eq!(nav.routes(), &["settings", "details"]);
    assert_eq!(nav.current(), &"details");
    nav.reset_to_root();
    assert!(nav.routes().is_empty());
    assert_eq!(nav.current(), &"catalog");
    assert_eq!(nav.depth(), 0);
}
