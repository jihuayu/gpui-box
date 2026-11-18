//! Owner preparation and ordinary Text/Flex A/B; no raster/GPU/presentation time.
use gpui_box::retained::{logical::*, text::*, text_style::*, *};
use std::{
    borrow::Cow, error::Error, fs, hint::black_box, path::PathBuf, rc::Rc, sync::Arc, time::Instant,
};

fn preparer(cached: bool) -> Rc<GpuiTextPreparer> {
    let system = Arc::new(gpui::TextSystem::new(Arc::new(
        gpui_wgpu::CosmicTextSystem::new_without_system_fonts("Noto Sans CJK SC"),
    )));
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../../../examples/hello/assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let prep = Rc::new(GpuiTextPreparer::new(Rc::new(gpui::WindowTextSystem::new(
        system,
    ))));
    if !cached {
        prep.set_cache_limits(TextCacheLimits {
            max_entries: 0,
            max_payload_bytes: 0,
        });
    }
    prep
}
fn typography(size: f32) -> TextTypography {
    TextTypography {
        font: FontDescriptor {
            family: "Noto Sans CJK SC".into(),
            ..Default::default()
        },
        font_size: size,
        line_height: size * 2.0,
    }
}
fn quantile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).ceil() as usize]
}
struct Preparation {
    prep: Rc<GpuiTextPreparer>,
    texts: Vec<String>,
    current: Vec<TextResource>,
}
impl Preparation {
    fn new(cached: bool, count: usize, distinct: usize) -> Self {
        let texts = (0..count)
            .map(|i| format!("Label {} one two three 中文 ordinary text", i % distinct))
            .collect();
        Self {
            prep: preparer(cached),
            texts,
            current: vec![],
        }
    }
    fn step(&mut self, phase: usize) {
        let style = typography(if phase.is_multiple_of(2) { 16.0 } else { 20.0 });
        let next = self
            .texts
            .iter()
            .map(|text| self.prep.prepare(text, &style, 1.25, 0).unwrap())
            .collect();
        self.current = next;
        black_box(&self.current);
    }
}
struct UiCase {
    ui: LogicalUi,
    prep: Rc<GpuiTextPreparer>,
    p: TextProps,
    count: usize,
}
impl UiCase {
    fn new(cached: bool, count: usize) -> Self {
        let prep = preparer(cached);
        let mut p = TextProps::new(
            "one two three 中文 ordinary label",
            TextPreparation::new(prep.clone()),
        );
        p.scale = 1.25;
        p.style.font_family = TextValue::Value("Noto Sans CJK SC".into());
        p.layout = LayoutStyle::column()
            .size(LayoutDimension::Px(120.0), LayoutDimension::Auto)
            .flex_shrink(0.0);
        let mut ui =
            LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default())
                .unwrap();
        let root = ui.root_scope();
        let host = ui.root_render();
        ui.edit_local_frame(
            LayoutConstraints::new(
                LayoutAvailableSpace::Unbounded,
                LayoutAvailableSpace::Unbounded,
            ),
            [800.0, 600.0],
            |_| Ok(()),
            |(), e| e.components::<Text>(root, host, (0..count as u64).map(|key| (key, p.clone()))),
        )
        .unwrap();
        Self { ui, prep, p, count }
    }
    fn step(&mut self, phase: usize) {
        self.p.style.font_size =
            TextValue::Value(if phase.is_multiple_of(2) { 16.0 } else { 20.0 });
        let root = self.ui.root_scope();
        let host = self.ui.root_render();
        self.ui
            .edit(
                |_| Ok(()),
                |(), e| {
                    e.components::<Text>(
                        root,
                        host,
                        (0..self.count as u64).map(|key| (key, self.p.clone())),
                    )
                },
            )
            .unwrap();
        black_box(self.ui.published_local_frame().unwrap());
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let output: PathBuf = args
        .next()
        .map_or_else(|| "target/text-preparation-perf".into(), Into::into);
    let samples: usize = args.next().map_or(Ok(32), |s| s.parse())?;
    if !(8..=1000).contains(&samples) {
        return Err("samples must be 8..1000".into());
    }
    fs::create_dir_all(&output)?;
    let mut raw = String::from("case,count,distinct,sample,uncached_us,cached_us\n");
    let mut summary = vec![];
    for (count, distinct) in [(32, 1), (300, 12), (300, 300)] {
        let mut old = Preparation::new(false, count, distinct);
        let mut new = Preparation::new(true, count, distinct);
        let mut a = vec![];
        let mut b = vec![];
        for sample in 0..samples + 2 {
            let measure = |case: &mut Preparation| {
                let t = Instant::now();
                case.step(sample);
                t.elapsed().as_secs_f64() * 1e6
            };
            let (x, y) = if sample.is_multiple_of(2) {
                (measure(&mut old), measure(&mut new))
            } else {
                let y = measure(&mut new);
                (measure(&mut old), y)
            };
            assert_eq!(old.current.len(), new.current.len());
            for (left, right) in old.current.iter().zip(&new.current) {
                let l = left.paragraph().layout(Some(120.0))?;
                let r = right.paragraph().layout(Some(120.0))?;
                assert_eq!(
                    (l.size, l.first_baseline, l.last_baseline),
                    (r.size, r.first_baseline, r.last_baseline)
                );
            }
            if sample >= 2 {
                a.push(x);
                b.push(y);
                raw.push_str(&format!(
                    "preparation,{count},{distinct},{sample},{x:.3},{y:.3}\n"
                ));
            }
        }
        summary.push(serde_json::json!({"case":"preparation","count":count,"distinct":distinct,"uncached_p50_us":quantile(&a,0.5),"uncached_p95_us":quantile(&a,0.95),"cached_p50_us":quantile(&b,0.5),"cached_p95_us":quantile(&b,0.95),"prepared_uncached":old.prep.prepared_resources(),"prepared_cached":new.prep.prepared_resources(),"cached_entries":new.prep.cache_stats().entries,"accounted_payload":new.prep.cache_stats().payload_bytes}));
    }
    for count in [8, 64] {
        let mut old = UiCase::new(false, count);
        let mut new = UiCase::new(true, count);
        let mut a = vec![];
        let mut b = vec![];
        for sample in 0..samples + 2 {
            let measure = |case: &mut UiCase| {
                let t = Instant::now();
                case.step(sample);
                t.elapsed().as_secs_f64() * 1e6
            };
            let (x, y) = if sample.is_multiple_of(2) {
                (measure(&mut old), measure(&mut new))
            } else {
                let y = measure(&mut new);
                (measure(&mut old), y)
            };
            let left = old.ui.published_local_frame().unwrap().resolve();
            let right = new.ui.published_local_frame().unwrap().resolve();
            assert_eq!(left.items.len(), right.items.len());
            for (l, r) in left.items.iter().zip(right.items.iter()) {
                assert_eq!((l.bounds, l.content_bounds), (r.bounds, r.content_bounds));
            }
            if sample >= 2 {
                a.push(x);
                b.push(y);
                raw.push_str(&format!("ui_flex,{count},1,{sample},{x:.3},{y:.3}\n"));
            }
        }
        summary.push(serde_json::json!({"case":"ui_flex","count":count,"distinct":1,"uncached_p50_us":quantile(&a,0.5),"uncached_p95_us":quantile(&a,0.95),"cached_p50_us":quantile(&b,0.5),"cached_p95_us":quantile(&b,0.95),"prepared_uncached":old.prep.prepared_resources(),"prepared_cached":new.prep.prepared_resources()}));
    }
    fs::write(output.join("raw.csv"), raw)?;
    fs::write(
        output.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
