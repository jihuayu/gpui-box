use crate::{Recorder, Work};
use gpui::{Bounds, ContentMask, Quad, ScaledPixels, Scene, hsla, point, size};
use std::{error::Error, hint::black_box, mem::size_of};

fn quad(index: usize) -> Quad {
    let bounds = Bounds {
        origin: point(
            ScaledPixels((index % 300) as f32 * 4.0),
            ScaledPixels((index / 300) as f32 * 4.0),
        ),
        size: size(ScaledPixels(3.0), ScaledPixels(3.0)),
    };
    Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: hsla(0.5, 0.6, 0.5, 1.0).into(),
        ..Quad::default()
    }
}

fn populate(scene: &mut Scene, n: usize) {
    for index in 0..n {
        scene.insert_primitive(quad(index));
    }
    scene.finish();
}

fn grouped_bounds(n: usize) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(
            ScaledPixels(1_200.0),
            ScaledPixels(n.div_ceil(300) as f32 * 4.0),
        ),
    }
}

pub(super) fn run(recorder: &mut Recorder, n: usize) -> Result<(), Box<dyn Error>> {
    let mut previous = Scene::default();
    populate(&mut previous, n);
    let mut next = Scene::default();
    let work = Work {
        primitives: n as u64,
        modeled_instance_bytes: (n * size_of::<Quad>()) as u64,
        ..Work::default()
    };
    recorder.run("scene_fresh_record_finish", n, || {
        next.clear();
        populate(&mut next, n);
        black_box(&next);
        work
    })?;
    recorder.run("scene_cached_replay_finish", n, || {
        next.clear();
        next.replay(0..previous.len(), &previous);
        next.finish();
        black_box(&next);
        work
    })?;
    let mut alternate = false;
    recorder.run("scene_one_color_cached_rest", n, || {
        alternate = !alternate;
        let changed_index = n / 2;
        let mut changed = quad(changed_index);
        changed.background = hsla(if alternate { 0.1 } else { 0.5 }, 0.6, 0.5, 1.0).into();
        next.clear();
        next.replay(0..changed_index, &previous);
        next.insert_primitive(changed);
        next.replay(changed_index + 1..previous.len(), &previous);
        next.finish();
        black_box(&next);
        work
    })?;
    recorder.run("scene_finish_already_sorted", n, || {
        next.finish();
        black_box(&next);
        Work::default()
    })?;
    recorder.run("scene_batch_iteration", n, || {
        black_box(previous.batches().count());
        Work::default()
    })?;
    // GPUI's paint_layer is an existing ordering optimization, not a raster cache.
    // These separated quads can legally share a paint layer. Replay still walks N.
    let mut grouped = Scene::default();
    grouped.push_layer(grouped_bounds(n));
    for index in 0..n {
        grouped.insert_primitive(quad(index));
    }
    grouped.pop_layer();
    grouped.finish();
    recorder.run("scene_grouped_cached_replay", n, || {
        next.clear();
        next.replay(0..grouped.len(), &grouped);
        next.finish();
        black_box(&next);
        work
    })?;
    recorder.run("scene_grouped_one_color", n, || {
        alternate = !alternate;
        let changed_index = n / 2;
        let mut changed = quad(changed_index);
        changed.background = hsla(if alternate { 0.1 } else { 0.5 }, 0.6, 0.5, 1.0).into();
        next.clear();
        next.push_layer(grouped_bounds(n));
        // Paint operations: StartLayer, N primitives, EndLayer.
        next.replay(1..1 + changed_index, &grouped);
        next.insert_primitive(changed);
        next.replay(changed_index + 2..n + 1, &grouped);
        next.pop_layer();
        next.finish();
        black_box(&next);
        work
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_gpui_replay_retains_geometry_color_and_order() {
        for n in [1, 300, 3_000] {
            let mut previous = Scene::default();
            populate(&mut previous, n);
            let mut replayed = Scene::default();
            replayed.replay(0..previous.len(), &previous);
            replayed.finish();
            assert_eq!(replayed.len(), previous.len());
            assert_eq!(replayed.quads.len(), n);
            for (left, right) in replayed.quads.iter().zip(&previous.quads) {
                assert_eq!(left.bounds, right.bounds);
                assert_eq!(left.background, right.background);
                assert_eq!(left.order, right.order);
                assert_eq!(left.content_mask, right.content_mask);
            }
            assert_eq!(replayed.batches().count(), previous.batches().count());
        }
    }

    #[test]
    fn grouped_replay_preserves_visual_inputs_and_uses_one_batch() {
        let n = 300;
        let mut ungrouped = Scene::default();
        populate(&mut ungrouped, n);
        let mut grouped = Scene::default();
        grouped.push_layer(grouped_bounds(n));
        for index in 0..n {
            grouped.insert_primitive(quad(index));
        }
        grouped.pop_layer();
        grouped.finish();
        let mut replayed = Scene::default();
        replayed.replay(0..grouped.len(), &grouped);
        replayed.finish();
        assert_eq!(replayed.quads.len(), n);
        assert_eq!(replayed.batches().count(), 1);
        for (left, right) in replayed.quads.iter().zip(&ungrouped.quads) {
            assert_eq!(left.bounds, right.bounds);
            assert_eq!(left.background, right.background);
            assert_eq!(left.content_mask, right.content_mask);
        }
    }
}
