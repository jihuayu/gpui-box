use super::*;
use crate::{Dimension, RequestedAxis, Style};

#[test]
fn intrinsic_limit_demand_is_distinct_from_outer_axis_and_legacy_wrapper_agrees() {
    for axis in [
        RequestedAxis::Horizontal,
        RequestedAxis::Vertical,
        RequestedAxis::Both,
    ] {
        let style: Style = Style {
            min_size: Size {
                width: Dimension::min_content(),
                height: Dimension::max_content(),
            },
            ..Style::DEFAULT
        };
        let input = LayoutInput {
            run_mode: RunMode::PerformLayout,
            axis,
            available_space: Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
            ..LayoutInput::HIDDEN
        };
        let mut demands = std::vec::Vec::new();
        let complete = |space: Size<AvailableSpace>| Size {
            width: if space.width == AvailableSpace::MinContent {
                10.0
            } else {
                80.0
            },
            height: 20.0,
        };
        let partial = compute_leaf_layout_with_measure_axis(
            input,
            &style,
            |_, _| 0.0,
            |_, available, axis| {
                demands.push(axis);
                let result = complete(available);
                match axis {
                    RequestedAxis::Horizontal => Size {
                        width: result.width,
                        height: 0.0,
                    },
                    RequestedAxis::Vertical => Size {
                        width: 0.0,
                        height: result.height,
                    },
                    RequestedAxis::Both => result,
                }
            },
        );
        let legacy = compute_leaf_layout(
            input,
            &style,
            |_, _| 0.0,
            |_, available| complete(available),
        );
        assert_eq!(
            demands,
            [
                RequestedAxis::Horizontal,
                RequestedAxis::Vertical,
                RequestedAxis::Both
            ]
        );
        assert_eq!(partial, legacy);
    }
}

#[test]
fn scalar_leaf_query_reports_axis_but_complete_known_sizes_still_short_circuit() {
    let mut calls = 0;
    let style: Style = Style::DEFAULT;
    let input = LayoutInput {
        known_dimensions: Size {
            width: Some(80.0),
            height: Some(40.0),
        },
        run_mode: RunMode::ComputeSize,
        axis: RequestedAxis::Horizontal,
        ..LayoutInput::HIDDEN
    };
    let output = compute_leaf_layout_with_measure_axis(
        input,
        &style,
        |_, _| 0.0,
        |_, _, _| {
            calls += 1;
            Size::ZERO
        },
    );
    assert_eq!(calls, 0);
    assert_eq!(
        output.size,
        Size {
            width: 80.0,
            height: 40.0
        }
    );
    let mut demands = std::vec::Vec::new();
    compute_leaf_layout_with_measure_axis(
        LayoutInput {
            known_dimensions: Size::NONE,
            ..input
        },
        &style,
        |_, _| 0.0,
        |_, _, axis| {
            demands.push(axis);
            Size {
                width: 80.0,
                height: 0.0,
            }
        },
    );
    assert_eq!(demands, [RequestedAxis::Horizontal]);
}
