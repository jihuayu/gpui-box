//! Deterministic fixed-height list windowing.
//!
//! The view owns scroll state. This module only converts a scroll offset into a
//! bounded visible slice and the exact spacer heights needed to preserve the
//! full scroll extent, so filtering can shrink the list without retaining an
//! out-of-range offset.

use gpui::{prelude::*, *};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListSpec {
    pub item_count: usize,
    pub row_height_px: f32,
    pub viewport_height_px: f32,
    pub scroll_offset_px: f32,
    pub overscan: usize,
}

impl Default for VirtualListSpec {
    fn default() -> Self {
        Self {
            item_count: 0,
            row_height_px: 0.0,
            viewport_height_px: 0.0,
            scroll_offset_px: 0.0,
            overscan: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualWindow {
    pub scroll_offset_px: f32,
    pub total_height_px: f32,
    pub visible_range: Range<usize>,
    pub top_spacer_px: f32,
    pub bottom_spacer_px: f32,
}

impl VirtualListSpec {
    /// Compute the bounded visible slice. Invalid row/viewport/offset values are
    /// normalized without producing NaN, infinity or a panic.
    pub fn window(&self) -> VirtualWindow {
        let row_height = finite_non_negative(self.row_height_px);
        let viewport_height = finite_non_negative(self.viewport_height_px);
        let total_height = self.item_count as f32 * row_height;
        let max_scroll = (total_height - viewport_height).max(0.0);
        let scroll_offset = if self.scroll_offset_px.is_finite() {
            self.scroll_offset_px.clamp(0.0, max_scroll)
        } else {
            0.0
        };

        if self.item_count == 0 || row_height == 0.0 {
            return VirtualWindow {
                scroll_offset_px: 0.0,
                total_height_px: 0.0,
                visible_range: 0..0,
                top_spacer_px: 0.0,
                bottom_spacer_px: 0.0,
            };
        }

        let overscan = self.overscan.min(self.item_count);
        let first = (scroll_offset / row_height).floor() as usize;
        let first = first.min(self.item_count);
        let start = first.saturating_sub(overscan);
        let end = if viewport_height == 0.0 {
            start
        } else {
            let last = ((scroll_offset + viewport_height) / row_height).ceil() as usize;
            last.saturating_add(overscan).min(self.item_count)
        };
        let end = end.max(start);
        let top_spacer = start as f32 * row_height;
        let bottom_spacer =
            (total_height - top_spacer - (end - start) as f32 * row_height).max(0.0);
        VirtualWindow {
            scroll_offset_px: scroll_offset,
            total_height_px: total_height,
            visible_range: start..end,
            top_spacer_px: top_spacer,
            bottom_spacer_px: bottom_spacer,
        }
    }

    pub fn scroll_to_index(&mut self, index: usize) {
        if self.item_count == 0 || finite_non_negative(self.row_height_px) == 0.0 {
            self.scroll_offset_px = 0.0;
            return;
        }
        let row_height = finite_non_negative(self.row_height_px);
        let total_height = self.item_count as f32 * row_height;
        let viewport_height = finite_non_negative(self.viewport_height_px);
        let max_scroll = (total_height - viewport_height).max(0.0);
        let index = index.min(self.item_count - 1);
        self.scroll_offset_px = (index as f32 * row_height).clamp(0.0, max_scroll);
    }

    /// Replace the item count and immediately clamp the retained scroll offset.
    /// This is the filtering/shrinking path: no caller can keep an offset beyond
    /// the new available extent.
    pub fn set_item_count(&mut self, item_count: usize) {
        self.item_count = item_count;
        let row_height = finite_non_negative(self.row_height_px);
        let viewport_height = finite_non_negative(self.viewport_height_px);
        let total_height = item_count as f32 * row_height;
        let max_scroll = (total_height - viewport_height).max(0.0);
        self.scroll_offset_px = if self.scroll_offset_px.is_finite() {
            self.scroll_offset_px.clamp(0.0, max_scroll)
        } else {
            0.0
        };
    }
}

fn finite_non_negative(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// Render a Role::List containing only the visible window slice plus spacers
/// that preserve the full scroll extent. `render_item` receives the global item
/// index, never a window-relative index.
pub fn virtual_list(
    id: impl Into<ElementId>,
    window: VirtualWindow,
    total_count: usize,
    render_item: impl Fn(usize) -> AnyElement,
) -> Stateful<Div> {
    let start = window.visible_range.start.min(total_count);
    let end = window.visible_range.end.min(total_count).max(start);
    let mut children = Vec::with_capacity(end.saturating_sub(start) + 2);
    if window.top_spacer_px > 0.0 {
        children.push(div().h(px(window.top_spacer_px)).into_any_element());
    }
    children.extend((start..end).map(&render_item));
    if window.bottom_spacer_px > 0.0 {
        children.push(div().h(px(window.bottom_spacer_px)).into_any_element());
    }
    div()
        .id(id)
        .role(accesskit::Role::List)
        .flex()
        .flex_col()
        .w_full()
        .children(children)
}
