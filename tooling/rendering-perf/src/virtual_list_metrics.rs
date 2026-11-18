//! Narrow CPU A/B: the previous flat prefix snapshot versus the current public
//! immutable metric API. No layout, renderer, presentation or FPS claim.
use gpui_box::retained::virtual_list::{VirtualListMetrics, VirtualListWindow};
use serde_json::json;
use std::{collections::BTreeMap, hint::black_box, time::Instant};

#[derive(Clone)]
struct Flat(Vec<f64>);
impl Flat {
    fn new(count: usize) -> Self {
        let mut p = Vec::with_capacity(count + 1);
        p.push(0.0);
        for i in 0..count {
            p.push(p.last().unwrap() + f64::from(height(i)));
        }
        Self(p)
    }
    fn height(&self, i: usize) -> f32 {
        (self.0[i + 1] - self.0[i]) as f32
    }
    fn window(&self, offset: f32) -> VirtualListWindow {
        let total = *self.0.last().unwrap();
        let maximum = (total - 600.0).max(0.0) as f32;
        let offset = offset.min(maximum);
        let start = (f64::from(offset) - 100.0).max(0.0);
        let end = (f64::from(offset) + 700.0).min(total);
        let range = self.0.partition_point(|v| *v <= start).saturating_sub(1)
            ..self.0.partition_point(|v| *v < end).min(self.0.len() - 1);
        VirtualListWindow {
            leading: self.0[range.start] as f32,
            trailing: (total - self.0[range.end]) as f32,
            range,
            offset,
            maximum,
            total: total as f32,
        }
    }
    fn updated(&self, updates: &[(usize, f32)]) -> Self {
        let mut changes = BTreeMap::new();
        for &(index, height) in updates {
            assert!(index + 1 < self.0.len() && height.is_finite() && height > 0.0);
            changes.insert(index, height);
        }
        changes.retain(|i, h| self.height(*i) != *h);
        if changes.is_empty() {
            return self.clone();
        }
        let mut changes = changes.into_iter().peekable();
        let mut out = Vec::with_capacity(self.0.len());
        out.push(0.0);
        for i in 0..self.0.len() - 1 {
            let height = if changes.peek().is_some_and(|(index, _)| *index == i) {
                changes.next().unwrap().1
            } else {
                self.height(i)
            };
            out.push(out.last().unwrap() + f64::from(height));
        }
        Self(out)
    }
}
fn height(i: usize) -> f32 {
    20.0 + (i % 7) as f32 * 0.25
}
fn ns(samples: &mut [f64]) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    json!({"p50_ns":samples[samples.len()/2],"p95_ns":samples[(samples.len()*95/100).min(samples.len()-1)]})
}
fn main() {
    let mut results = vec![];
    for count in [1_000, 100_000, 1_000_000] {
        for changed in [1, 8, 128] {
            let mut flat = Flat::new(count);
            let mut indexed = VirtualListMetrics::variable((0..count).map(height)).unwrap();
            let updates = [0.0, 1.0].map(|extra| {
                (0..changed)
                    .map(|i| {
                        let index = (count / 2 + i * 13) % count;
                        (index, 40.0 + (i % 5) as f32 + extra)
                    })
                    .collect::<Vec<_>>()
            });
            let rounds = if count == 1_000 { 100 } else { 12 };
            let mut old_samples = vec![];
            let mut new_samples = vec![];
            // Alternate sample order to reduce systematic cache/clock drift.
            for sample in 0..32 {
                let mut old = || {
                    let start = Instant::now();
                    for turn in 0..rounds {
                        flat = black_box(
                            flat.updated(black_box(&updates[(sample * rounds + turn) % 2])),
                        );
                    }
                    start.elapsed().as_nanos() as f64 / rounds as f64
                };
                let mut new = || {
                    let start = Instant::now();
                    for turn in 0..rounds {
                        indexed = black_box(
                            indexed
                                .with_heights(black_box(
                                    updates[(sample * rounds + turn) % 2].iter().copied(),
                                ))
                                .unwrap(),
                        );
                    }
                    start.elapsed().as_nanos() as f64 / rounds as f64
                };
                let (a, b) = if sample % 2 == 0 {
                    (old(), new())
                } else {
                    let b = new();
                    (old(), b)
                };
                if sample > 1 {
                    old_samples.push(a);
                    new_samples.push(b);
                }
            }
            assert_eq!(indexed.total(), *flat.0.last().unwrap() as f32);
            for i in 0..count {
                assert_eq!(indexed.height(i).unwrap(), flat.height(i));
            }
            let old = ns(&mut old_samples);
            let new = ns(&mut new_samples);
            let mut query_old = vec![];
            let mut query_new = vec![];
            let offsets = (0..1024)
                .map(|i| ((i * 7919) % count) as f32 * 20.0)
                .collect::<Vec<_>>();
            for &offset in &offsets {
                assert_eq!(
                    indexed.window(offset, 600.0, 100.0, count + 1).unwrap(),
                    flat.window(offset)
                );
            }
            for sample in 0..32 {
                let old = || {
                    let start = Instant::now();
                    for &o in &offsets {
                        black_box(flat.window(black_box(o)));
                    }
                    start.elapsed().as_nanos() as f64 / offsets.len() as f64
                };
                let new = || {
                    let start = Instant::now();
                    for &o in &offsets {
                        black_box(
                            indexed
                                .window(black_box(o), 600.0, 100.0, count + 1)
                                .unwrap(),
                        );
                    }
                    start.elapsed().as_nanos() as f64 / offsets.len() as f64
                };
                let (a, b) = if sample % 2 == 0 {
                    (old(), new())
                } else {
                    let b = new();
                    (old(), b)
                };
                if sample > 1 {
                    query_old.push(a);
                    query_new.push(b);
                }
            }
            results.push(json!({"rows":count,"changed":changed,"samples":old_samples.len(),"rounds_per_sample":rounds,"flat_prefix":old,"persistent_index":new,"p50_ratio":old["p50_ns"].as_f64().unwrap()/new["p50_ns"].as_f64().unwrap(),"window_flat":ns(&mut query_old),"window_persistent":ns(&mut query_new)}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&json!({"scope":"CPU metric snapshot height updates only; release; excludes dataset keys, layout, rendering, presentation","results":results})).unwrap());
}
