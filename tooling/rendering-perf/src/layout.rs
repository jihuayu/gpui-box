use crate::{Recorder, Work};
use std::{error::Error, hint::black_box};
use taffy::prelude::*;

struct Fixture {
    tree: TaffyTree<f32>,
    root: NodeId,
    leaves: Vec<NodeId>,
    wide: bool,
    viewport: f32,
}

impl Fixture {
    fn new(n: usize, rounding: bool) -> Self {
        let mut result = Self::uncomputed(n, rounding);
        result.layout();
        result
    }

    fn uncomputed(n: usize, rounding: bool) -> Self {
        let mut tree = TaffyTree::new();
        if !rounding {
            tree.disable_rounding();
        }
        let mut result = Self {
            tree,
            root: NodeId::from(0_usize),
            leaves: Vec::with_capacity(n),
            wide: false,
            viewport: 1_000.0,
        };
        result.build(n);
        result
    }

    fn build(&mut self, n: usize) {
        self.leaves.clear();
        for _ in 0..n {
            self.leaves.push(
                self.tree
                    .new_leaf_with_context(
                        Style {
                            size: Size {
                                width: Dimension::auto(),
                                height: Dimension::length(18.0),
                            },
                            min_size: Size {
                                width: LengthPercentageAuto::length(0.0),
                                height: LengthPercentageAuto::length(0.0),
                            },
                            flex_shrink: 0.0,
                            ..Style::default()
                        },
                        24.0,
                    )
                    .unwrap(),
            );
        }
        self.root = self
            .tree
            .new_with_children(
                Style {
                    size: Size {
                        width: Dimension::percent(1.0),
                        height: Dimension::auto(),
                    },
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::START,
                    ..Style::default()
                },
                &self.leaves,
            )
            .unwrap();
    }

    fn layout(&mut self) -> Work {
        let mut work = Work::default();
        self.tree
            .compute_layout_with_measure(
                self.root,
                Size {
                    width: AvailableSpace::Definite(self.viewport),
                    height: AvailableSpace::MaxContent,
                },
                |input, _id, context, style| {
                    work.leaf_layouts += 1;
                    taffy::compute_leaf_layout(
                        input,
                        style,
                        |_, _| 0.0,
                        |known, _available| {
                            work.measures += 1;
                            Size {
                                width: known.width.unwrap_or(context.copied().unwrap_or(0.0)),
                                height: known.height.unwrap_or(18.0),
                            }
                        },
                    )
                },
            )
            .unwrap();
        black_box(self.tree.layout(self.root).unwrap());
        work
    }

    fn change_leaf(&mut self) -> Work {
        self.wide = !self.wide;
        self.tree
            .set_node_context(
                self.leaves[self.leaves.len() / 2],
                Some(if self.wide { 48.0 } else { 24.0 }),
            )
            .unwrap();
        self.layout()
    }
}

pub(super) fn run(recorder: &mut Recorder, n: usize) -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::new(n, false);
    recorder.run("layout_clear_build_compute", n, || {
        fixture.tree.clear();
        fixture.build(n);
        fixture.layout()
    })?;
    recorder.run("layout_cached_unrounded", n, || fixture.layout())?;
    recorder.run("layout_equal_style_unguarded", n, || {
        let leaf = fixture.leaves[n / 2];
        let style = fixture.tree.style(leaf).unwrap().clone();
        fixture.tree.set_style(leaf, style).unwrap();
        fixture.layout()
    })?;
    recorder.run("layout_equal_style_guarded", n, || {
        let leaf = fixture.leaves[n / 2];
        let old = black_box(fixture.tree.style(leaf).unwrap());
        let new = black_box(old.clone());
        if *old != new {
            fixture.tree.set_style(leaf, new).unwrap();
            fixture.layout()
        } else {
            Work::default()
        }
    })?;
    recorder.run("layout_one_intrinsic_change", n, || fixture.change_leaf())?;
    recorder.run("layout_viewport_resize", n, || {
        fixture.viewport = if fixture.viewport == 1_000.0 {
            984.0
        } else {
            1_000.0
        };
        fixture.layout()
    })?;
    let mut rounded = Fixture::new(n, true);
    recorder.run("layout_cached_rounded_control", n, || rounded.layout())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_layout_matches_fresh_tree_after_measure_and_viewport_changes() {
        let mut retained = Fixture::new(300, false);
        for width in [1_000.0, 984.0] {
            retained.viewport = width;
            retained.change_leaf();
            let mut reference = Fixture::uncomputed(300, false);
            reference.viewport = width;
            reference
                .tree
                .set_node_context(
                    reference.leaves[150],
                    Some(if retained.wide { 48.0 } else { 24.0 }),
                )
                .unwrap();
            reference.layout();
            for (left, right) in retained.leaves.iter().zip(&reference.leaves) {
                assert_eq!(
                    retained.tree.layout(*left).unwrap(),
                    reference.tree.layout(*right).unwrap()
                );
            }
            assert_eq!(
                retained.tree.layout(retained.root).unwrap(),
                reference.tree.layout(reference.root).unwrap()
            );
        }
    }

    #[test]
    fn cached_compute_skips_measure_but_equal_set_style_invalidates() {
        let mut fixture = Fixture::new(300, false);
        let cached = fixture.layout();
        assert_eq!((cached.leaf_layouts, cached.measures), (0, 0));
        let leaf = fixture.leaves[150];
        fixture
            .tree
            .set_style(leaf, fixture.tree.style(leaf).unwrap().clone())
            .unwrap();
        assert!(fixture.tree.dirty(fixture.root).unwrap());
        let unguarded = fixture.layout();
        assert!(unguarded.leaf_layouts > 0);
        assert!(unguarded.measures > 0);
    }
}
