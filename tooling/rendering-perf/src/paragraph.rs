//! CPU-only owned wrapping versus cached and cold-window GPUI layout.
use gpui::{TextRun, WindowTextSystem, font, px};
use gpui_box::retained::paragraph::ParagraphSnapshot;
use std::{borrow::Cow, error::Error, fs, path::PathBuf, sync::Arc, time::Instant};
fn main() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/paragraph-runtime-perf".into()),
    );
    fs::create_dir_all(&output)?;
    let platform = Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        "Noto Sans CJK SC",
    ));
    let system = Arc::new(gpui::TextSystem::new(platform));
    system.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../../../examples/hello/assets/NotoSansSC-Regular.otf"
    ))])?;
    let window = WindowTextSystem::new(system.clone());
    let mut cases = vec![];
    for repeats in [10, 100] {
        let text = "office affine a\u{301} 中文混排 one two three. ".repeat(repeats);
        let snapshot =
            ParagraphSnapshot::shape(&window, &text, font("Noto Sans CJK SC"), 16.0, 32.0)?;
        let oracle =
            ParagraphSnapshot::shape(&window, &text, font("Noto Sans CJK SC"), 16.0, 32.0)?;
        let runs = [TextRun {
            len: text.len(),
            font: font("Noto Sans CJK SC"),
            ..Default::default()
        }];
        let mut samples = [vec![], vec![], vec![], vec![]];
        for round in 0..3 {
            for mode in [round % 4, (round + 1) % 4, (round + 2) % 4, (round + 3) % 4] {
                for i in 0..36 {
                    let width = if mode == 1 {
                        240.0 + (i + round * 36) as f32
                    } else if i % 2 == 0 {
                        240.0
                    } else {
                        480.0
                    };
                    let expected = oracle.layout(Some(width))?.size;
                    let start = Instant::now();
                    let actual = match mode {
                        0 | 1 => snapshot.layout(Some(width))?.size,
                        2 => {
                            let layout = window.shape_text(
                                text.clone().into(),
                                px(16.0),
                                &runs,
                                Some(px(width)),
                                None,
                            )?;
                            let size = layout[0].size(px(32.0));
                            [f32::from(size.width), f32::from(size.height)]
                        }
                        _ => {
                            let cold = WindowTextSystem::new(system.clone());
                            let layout = cold.shape_text(
                                text.clone().into(),
                                px(16.0),
                                &runs,
                                Some(px(width)),
                                None,
                            )?;
                            let size = layout[0].size(px(32.0));
                            [f32::from(size.width), f32::from(size.height)]
                        }
                    };
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(
                        actual, expected,
                        "real shaping/wrapping geometry must agree"
                    );
                    std::hint::black_box(actual);
                    if i >= 6 {
                        samples[mode].push(elapsed);
                    }
                }
            }
        }
        for (mode, values) in samples.iter_mut().enumerate() {
            values.sort_by(f64::total_cmp);
            let name = [
                "owned snapshot four-width cache hit",
                "owned snapshot cache miss at distinct widths",
                "warm GPUI window layout cache",
                "fresh GPUI window cache; shared font system",
            ][mode];
            cases.push(serde_json::json!({"utf8_bytes":text.len(),"mode":name,"samples":values.len(),"median_us":values[values.len()/2],"p95_us":values[values.len()*95/100],"sorted_samples_us":values,"geometry_equal":true}));
        }
    }
    let result = serde_json::json!({"scope":"CPU paragraph wrap/measure only; same real-font content; cache-hit/warm/fresh modes alternate 240/480 logical-pixel widths; owned miss mode uses distinct 240..347 widths; 3 rotated rounds each mode, 6 warmups and 30 measured samples per round; owned snapshot shaped before timing; independent snapshot supplies expected geometry outside timing; warm and fresh GPUI paths include text clone and decorated output; no GPU/raster/paint or full UI extraction","cases":cases});
    fs::write(
        output.join("paragraph-benchmark.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{}",serde_json::to_string_pretty(&result["cases"].as_array().unwrap().iter().map(|value|serde_json::json!({"utf8_bytes":value["utf8_bytes"],"mode":value["mode"],"median_us":value["median_us"],"p95_us":value["p95_us"]})).collect::<Vec<_>>())?);
    Ok(())
}
