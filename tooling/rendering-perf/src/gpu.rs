//! Actual GPUI WGPU full-upload / persistent single-quad comparison.
use gpui::{Bounds, ContentMask, DevicePixels, Quad, ScaledPixels, Scene, hsla, point, size};
use gpui_wgpu::{Metrics, QuadPatchPrototype, UploadMode};
use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
};

fn fixture(n: usize) -> Scene {
    let mut scene = Scene::default();
    for index in 0..n {
        let bounds = Bounds {
            origin: point(
                ScaledPixels((index % 300) as f32 * 4.0),
                ScaledPixels((index / 300) as f32 * 4.0),
            ),
            size: size(ScaledPixels(3.0), ScaledPixels(3.0)),
        };
        scene.insert_primitive(Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: hsla(0.5, 0.6, 0.5, 1.0).into(),
            ..Default::default()
        });
    }
    scene.finish();
    scene
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut output = PathBuf::from("target/quad-patch-gpu");
    let mut samples = 201_usize;
    let mut rounds = 3_usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("missing option value")?;
        match arg.as_str() {
            "--output" => output = value.into(),
            "--samples" => samples = value.parse()?,
            "--rounds" => rounds = value.parse()?,
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if !(10..=10_000).contains(&samples) || !(1..=10).contains(&rounds) {
        return Err("invalid sample/round count".into());
    }
    fs::create_dir_all(&output)?;
    let mut csv = BufWriter::new(File::create(output.join("samples.csv"))?);
    writeln!(
        csv,
        "nodes,round,sample,path,instance_bytes,instance_writes,upload_ns,submit_ns,cpu_ns,completion_ns,gpu_draw_ns"
    )?;
    let mut validation = Vec::new();
    for n in [300, 3_000, 30_000] {
        let target = size(DevicePixels(1_200), DevicePixels(400));
        let mut full = QuadPatchPrototype::new(fixture(n), target)?;
        let mut retained_full = QuadPatchPrototype::new(fixture(n), target)?;
        let mut patch = QuadPatchPrototype::new(fixture(n), target)?;
        let info = patch.adapter_info();
        println!(
            "GPU {} {:?} {:?}, n={n}",
            info.name, info.backend, info.device_type
        );
        fs::write(
            output.join("adapter.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "name":info.name, "backend":format!("{:?}",info.backend), "device_type":format!("{:?}",info.device_type),
                "driver":info.driver, "driver_info":info.driver_info, "vendor":info.vendor, "device":info.device,
                "target":[1200,400], "samples":samples, "rounds":rounds,
            "method":"per-update CPU timing; one outstanding submission, completion wait; render-pass GPU timestamps exclude upload and readback; three paths in rotating order",
            }))?,
        )?;

        let stale = full.handle(0)?;
        assert!(
            patch
                .set_color(stale, hsla(0.1, 1.0, 0.5, 1.0), UploadMode::Patch)
                .is_err()
        );
        assert!(patch.handle(n).is_err());
        let mut before = patch.image()?;
        for (step, slot) in [0, n / 2, n - 1, n / 3, n / 7, n / 2]
            .into_iter()
            .enumerate()
        {
            let hue = [0.1, 0.2, 0.3, 0.4, 0.7, 0.6][step];
            let metrics = patch.set_color(
                patch.handle(slot)?,
                hsla(hue, 1.0, 0.5, 1.0),
                UploadMode::Patch,
            )?;
            assert_eq!(metrics.instance_bytes, 160);
            let actual = patch.image()?;
            let reference = patch.reference_image()?;
            assert_eq!(
                actual.as_raw(),
                reference.as_raw(),
                "pixel mismatch n={n} slot={slot}"
            );
            let changed_pixels = actual
                .pixels()
                .zip(before.pixels())
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                changed_pixels, 9,
                "expected exactly one 3x3 rectangle to change"
            );
            actual.save(output.join(format!("verified-{n}-{step}.png")))?;
            validation.push(serde_json::json!({"nodes":n,"slot":slot,"pixel_equal":true,"changed_pixels":changed_pixels}));
            before = actual;
        }
        let handle = patch.handle(n / 2)?;
        assert!(
            patch
                .set_color(handle, hsla(f32::NAN, 1.0, 0.5, 1.0), UploadMode::Patch)
                .is_err()
        );
        assert!(
            patch
                .set_color(handle, hsla(0.1, 1.0, 0.5, 0.5), UploadMode::Patch)
                .is_err()
        );
        let equal = patch.set_color(handle, hsla(0.6, 1.0, 0.5, 1.0), UploadMode::Patch)?;
        assert_eq!(equal.instance_bytes, 0);
        patch = QuadPatchPrototype::new(fixture(n), target)?;
        assert!(
            patch
                .set_color(handle, hsla(0.1, 1.0, 0.5, 1.0), UploadMode::Patch)
                .is_err()
        );
        let handle = patch.handle(n / 2)?;
        for i in 0..40 {
            let color = hsla(if i % 2 == 0 { 0.1 } else { 0.5 }, 0.6, 0.5, 1.0);
            full.set_color(full.handle(n / 2)?, color, UploadMode::Original)?;
            patch.set_color(handle, color, UploadMode::Patch)?;
            retained_full.set_color(
                retained_full.handle(n / 2)?,
                color,
                UploadMode::RetainedFull,
            )?;
        }
        for round in 0..rounds {
            let mut full_samples = Vec::new();
            let mut patch_samples = Vec::new();
            let mut retained_samples = Vec::new();
            for sample in 0..samples {
                let color = hsla(
                    if (round * samples + sample).is_multiple_of(2) {
                        0.1
                    } else {
                        0.5
                    },
                    0.6,
                    0.5,
                    1.0,
                );
                let mut measured = [Metrics::default(); 3];
                // Rotate all three paths through first/middle/last positions.
                for offset in 0..3 {
                    let position = (sample + round + offset) % 3;
                    measured[position] = match position {
                        0 => full.set_color(full.handle(n / 2)?, color, UploadMode::Original)?,
                        1 => retained_full.set_color(
                            retained_full.handle(n / 2)?,
                            color,
                            UploadMode::RetainedFull,
                        )?,
                        _ => patch.set_color(handle, color, UploadMode::Patch)?,
                    };
                }
                let [a, c, b] = measured;
                for (path, m) in [("full", a), ("retained_full", c), ("patch", b)] {
                    let expected = if path == "patch" { 160 } else { n as u64 * 160 };
                    assert_eq!(m.instance_bytes, expected, "unexpected upload/no-op");
                    writeln!(
                        csv,
                        "{n},{round},{sample},{path},{},{},{},{},{},{},{}",
                        m.instance_bytes,
                        m.instance_writes,
                        m.upload_ns,
                        m.submit_ns,
                        m.cpu_ns,
                        m.completion_ns,
                        m.gpu_draw_ns.map(|v| v.to_string()).unwrap_or_default()
                    )?;
                }
                full_samples.push(a);
                patch_samples.push(b);
                retained_samples.push(c);
            }
            for (path, values) in [
                ("full", full_samples),
                ("retained_full", retained_samples),
                ("patch", patch_samples),
            ] {
                println!(
                    "n={n} round={round} {path}: CPU p50 {:.1}us completion p50 {:.1}us GPU draw p50 {:.1}us",
                    median(&values, |m| m.cpu_ns as f64) / 1000.0,
                    median(&values, |m| m.completion_ns as f64) / 1000.0,
                    median(&values, |m| m.gpu_draw_ns.unwrap_or(f64::NAN)) / 1000.0
                );
            }
            csv.flush()?;
        }
        assert_eq!(
            full.image()?.as_raw(),
            patch.image()?.as_raw(),
            "final path pixels differ"
        );
        assert_eq!(
            full.image()?.as_raw(),
            retained_full.image()?.as_raw(),
            "retained full pixels differ"
        );
    }
    fs::write(
        output.join("validation.json"),
        serde_json::to_vec_pretty(&validation)?,
    )?;
    Ok(())
}

fn median(values: &[Metrics], field: impl Fn(&Metrics) -> f64) -> f64 {
    let mut values: Vec<_> = values.iter().map(field).collect();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
